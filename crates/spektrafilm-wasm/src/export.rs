//! Memory-bounded tiled full-resolution export for browsers (especially mobile
//! Safari, where WebContent is killed around 1.5–2.3 GiB and WASM memory never
//! shrinks).
//!
//! Designed to run inside a dedicated short-lived Web Worker (`web/exporter.js`):
//! - Large Bayer RAWs (>20 MP, e.g. 60 MP Leica Q3) keep only the compact u16
//!   CFA mosaic (`BayerSensor`, 115 MiB instead of 690 MiB f32 RGB) and
//!   demosaic each tile on demand.
//! - 8-bit JPEGs/PNGs/TIFFs keep only RGB8 (3 B/px) and convert each tile on
//!   demand with the embedded ICC profile.
//! - Every tile is padded to the exact same `(padded_w, padded_h)` (~2.8 MP),
//!   so WebGPU allocates ONE buffer set (~135 MiB) and reuses it across all
//!   tiles with zero additional GPU allocations.
//! - Whole-frame Prep uses `TiledPrep` (guided base on `(w/4)×(h/4)` = 15 MiB
//!   at 60 MP) for bit-identical, seamless tone/detail adjustments.
//! - Output accumulates directly into RGB8 (3 B/px), drops the source and GPU
//!   buffers before crop/border, and encodes JPEG directly in Rust so the page
//!   never allocates a full-size `OffscreenCanvas` or `ImageData`.

use std::{cell::RefCell, rc::Rc};

use image::ImageDecoder;
use serde::Deserialize;
use spektrafilm_core::{
    data_fs,
    params::RuntimeParams,
    pipeline::Pipeline,
    prep::TiledPrep,
    preview,
    stages,
};
use spektrafilm_gpu::{
    ChainOutput, ChainResult, ComputeBackend,
    cpu_backend::CpuBackend,
    wgpu_backend::WgpuBackend,
};
use spektrafilm_io::{
    InputColour, RawWhiteBalance,
    border::{BorderOptions, BorderStyle},
    bounded::BayerSensor,
    colour::FileColour,
    encoding::Encoding,
};
use spektrafilm_math::image::ImageBuf;
use wasm_bindgen::prelude::*;

use crate::{
    REDUCED_XTRANS_BILINEAR_ABOVE, calibrate, js_error, load_data_image, memory_bytes, missing,
    parse_params, parse_white_balance,
};

/// Bayer and X-Trans RAWs at or above this sensor pixel count use compact CFA
/// storage (`BayerSensor`, 2 B/px) and demosaic each tile on demand instead of
/// materializing a full-frame f32 RGB `ImageBuf` (and Markesteijn scratch).
const COMPACT_BAYER_MIN_PIXELS: usize = 12_000_000;

/// Target padded tile pixel count for GPU tiled rendering (~1.8 MP → ~90 MiB
/// GPU working set, reused across every tile).
const GPU_TILE_PIXELS: u64 = 1_800_000;

/// Slightly smaller tiles on the CPU fallback so CPU FFT / scratch stays small.
const CPU_TILE_PIXELS: u64 = 1_800_000;

/// Cap the per-tile overlap halo so extreme multi-bounce or diffusion radii do
/// not shrink the tile interior below a productive size.
const MAX_TILE_HALO: u32 = 256;

/// Diffusion FFT grid budget inside the export worker (matches mobile editing).
const EXPORT_DIFFUSION_GRID_BUDGET: u64 = 160 * 1024 * 1024;

enum ExportSource {
    Bayer(BayerSensor),
    RasterRgb8 {
        img: image::RgbImage,
        info: FileColour,
        png: bool,
    },
    Full(ImageBuf),
}

impl ExportSource {
    fn full_size(&self) -> (u32, u32) {
        match self {
            Self::Bayer(s) => s.full_size(),
            Self::RasterRgb8 { img, .. } => img.dimensions(),
            Self::Full(buf) => (buf.width, buf.height),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Bayer(s) if s.is_xtrans() => "xtrans-sensor",
            Self::Bayer(_) => "bayer-sensor",
            Self::RasterRgb8 { .. } => "raster-rgb8",
            Self::Full(_) => "full-f32",
        }
    }

    /// Extract an upright `(w × h)` f32 `ImageBuf` starting at `(ox, oy)`,
    /// clamping coordinates past the right/bottom edge to the edge pixel.
    fn extract_rect(&self, ox: u32, oy: u32, w: u32, h: u32) -> anyhow::Result<ImageBuf> {
        match self {
            Self::Bayer(sensor) => sensor.demosaic_upright_rect(ox, oy, w, h),
            Self::RasterRgb8 { img, info, png } => {
                let (fw, fh) = img.dimensions();
                let (fw, fh) = (fw as usize, fh as usize);
                let (tw, th) = (w as usize, h as usize);
                let (ox, oy) = (ox as usize, oy as usize);
                let src = img.as_raw();
                let mut raw = vec![0u8; tw * th * 3];
                raw.chunks_exact_mut(tw * 3).enumerate().for_each(|(ly, row)| {
                    let gy = (oy + ly).min(fh - 1);
                    let src_row = gy * fw * 3;
                    for lx in 0..tw {
                        let gx = (ox + lx).min(fw - 1);
                        let si = src_row + gx * 3;
                        row[lx * 3..lx * 3 + 3].copy_from_slice(&src[si..si + 3]);
                    }
                });
                let tile = image::RgbImage::from_raw(w, h, raw)
                    .ok_or_else(|| anyhow::anyhow!("tile RGB8 buffer mismatch"))?;
                let interpretation = spektrafilm_io::InputInterpretation::default();
                let (buf, _) = spektrafilm_io::colour::convert(
                    image::DynamicImage::ImageRgb8(tile),
                    info,
                    &interpretation,
                    *png,
                )?;
                Ok(buf)
            }
            Self::Full(buf) => {
                let (fw, fh) = (buf.width as usize, buf.height as usize);
                let (tw, th) = (w as usize, h as usize);
                let (ox, oy) = (ox as usize, oy as usize);
                if ox == 0 && oy == 0 && tw == fw && th == fh {
                    return Ok(buf.clone());
                }
                let mut data = vec![spektrafilm_math::precision::ZERO; tw * th * 3];
                data.chunks_exact_mut(tw * 3).enumerate().for_each(|(ly, row)| {
                    let gy = (oy + ly).min(fh - 1);
                    let src_row = gy * fw * 3;
                    if ox + tw <= fw {
                        row.copy_from_slice(&buf.data[src_row + ox * 3..src_row + (ox + tw) * 3]);
                    } else {
                        for lx in 0..tw {
                            let gx = (ox + lx).min(fw - 1);
                            let si = src_row + gx * 3;
                            row[lx * 3..lx * 3 + 3].copy_from_slice(&buf.data[si..si + 3]);
                        }
                    }
                });
                Ok(ImageBuf::from_data(w, h, data))
            }
        }
    }

    /// Small upright preview (≤ 256 px on the long side) for auto-exposure
    /// metering when no pre-computed EV was passed.
    fn metering_preview(&self) -> anyhow::Result<ImageBuf> {
        match self {
            Self::Bayer(sensor) => sensor.small_preview(256),
            Self::RasterRgb8 { img, info, png } => {
                let (fw, fh) = img.dimensions();
                let scale = (256.0 / fw.max(fh).max(1) as f64).min(1.0);
                let tw = ((fw as f64 * scale).round() as u32).max(1);
                let th = ((fh as f64 * scale).round() as u32).max(1);
                let small = if (tw, th) == (fw, fh) {
                    img.clone()
                } else {
                    image::imageops::resize(img, tw, th, image::imageops::FilterType::Triangle)
                };
                let interpretation = spektrafilm_io::InputInterpretation::default();
                let (buf, _) = spektrafilm_io::colour::convert(
                    image::DynamicImage::ImageRgb8(small),
                    info,
                    &interpretation,
                    *png,
                )?;
                Ok(buf)
            }
            Self::Full(buf) => Ok(stages::filming::metering_preview(buf)),
        }
    }
}

fn decode_export_source(bytes: Vec<u8>, wb: RawWhiteBalance) -> Result<(ExportSource, InputColour), JsError> {
    if !spektrafilm_io::raw::is_dng(&bytes) {
        if let Ok(result) = decode_raster_compact(&bytes) {
            drop(bytes);
            return Ok(result);
        }
    }
    // RAW path (DNG or non-raster file).
    wb.validate().map_err(js_error)?;
    if let Ok(Some(sensor)) = BayerSensor::try_open(&bytes, wb, COMPACT_BAYER_MIN_PIXELS) {
        drop(bytes);
        return Ok((ExportSource::Bayer(sensor), InputColour::raw("rawler")));
    }
    let xtrans = |sensor_pixels| {
        if sensor_pixels > REDUCED_XTRANS_BILINEAR_ABOVE {
            spektrafilm_io::XTransDemosaic::Bilinear
        } else {
            spektrafilm_io::XTransDemosaic::Markesteijn
        }
    };
    let (image, exif) = spektrafilm_io::load_raw_bytes_unoriented_with(&bytes, wb, xtrans).map_err(|raw| {
        let raw = format!("{raw:#}");
        match raw.split_once("DecoderFailed(\"") {
            Some((_, reason)) => JsError::new(reason.split('"').next().unwrap_or(&raw)),
            None => JsError::new(&raw),
        }
    })?;
    drop(bytes);
    Ok((ExportSource::Full(image.oriented(exif)), InputColour::raw("rawler")))
}

fn decode_raster_compact(bytes: &[u8]) -> anyhow::Result<(ExportSource, InputColour)> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.no_limits();
    let format = reader.format().ok_or_else(|| anyhow::anyhow!("unknown image format"))?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut info = FileColour {
        icc: decoder.icc_profile().ok().flatten(),
        cmyk: matches!(
            decoder.original_color_type(),
            image::ExtendedColorType::Cmyk8 | image::ExtendedColorType::Cmyk16
        ),
        ..Default::default()
    };
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    info.float = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    info.gray = !img.color().has_color();
    let png = format == image::ImageFormat::Png;
    let interpretation = spektrafilm_io::InputInterpretation::default();
    if !info.float && !info.cmyk && matches!(
        img,
        image::DynamicImage::ImageRgb8(_)
            | image::DynamicImage::ImageRgba8(_)
            | image::DynamicImage::ImageLuma8(_)
            | image::DynamicImage::ImageLumaA8(_)
    ) {
        // Probe colour metadata on a 1×1 pixel without converting the whole frame.
        let probe = image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        let (_, colour) = spektrafilm_io::colour::convert(probe, &info, &interpretation, png)?;
        let rgb8 = img.into_rgb8();
        Ok((ExportSource::RasterRgb8 { img: rgb8, info, png }, colour))
    } else {
        let (buf, colour) = spektrafilm_io::colour::convert(img, &info, &interpretation, png)?;
        Ok((ExportSource::Full(buf), colour))
    }
}

/// Compute the pixel halo needed around each tile for all active spatial
/// kernels at full resolution `(full_w, full_h)`.
fn required_halo(pipeline: &Pipeline, full_w: u32, full_h: u32) -> u32 {
    let params = &pipeline.params;
    let converting = params.workflow.converts_film();
    let film_effects = !converting;
    let scan_film = params.io.scan_film && !converting;
    let pix_um = stages::filming::pixel_size_um(params.camera.film_format_mm, full_w, full_h) as f64;
    let mut halo = 0.0_f64;

    if film_effects && params.camera.lens_blur_um > 0.0 {
        halo = halo.max(3.0 * (params.camera.lens_blur_um as f64 / pix_um));
    }
    if film_effects && params.film_render.halation.active {
        let h = &params.film_render.halation;
        if h.scatter_amount > 0.0 {
            for c in 0..3 {
                halo = halo.max(3.0 * h.scatter_core_um[c] * h.scatter_spatial_scale / pix_um);
                halo = halo.max(4.0 * h.scatter_tail_um[c] * h.scatter_spatial_scale / pix_um);
            }
        }
        if h.halation_amount > 0.0 && h.halation_n_bounces > 0 {
            let max_first = h.first_sigma_um().into_iter().fold(0.0_f64, f64::max);
            let max_sigma = max_first * h.halation_spatial_scale / pix_um * (h.halation_n_bounces as f64).sqrt();
            halo = halo.max(3.0 * max_sigma);
        }
    }
    if film_effects && params.film_render.dir_couplers.active {
        let dir = &params.film_render.dir_couplers;
        halo = halo.max(3.0 * (dir.diffusion_size_um / pix_um));
        halo = halo.max(3.0 * (dir.diffusion_tail_um / pix_um));
    }
    if film_effects && spektrafilm_core::grain::grain_active(params) {
        let gp = spektrafilm_model::grain::gpu_params(
            &spektrafilm_core::grain::grain_curves(&pipeline.film),
            &spektrafilm_core::grain::grain_settings(&pipeline.film, &params.film_render.grain),
            stages::filming::grain_pixel_size_um(params, full_w, full_h),
            pipeline.film.is_positive(),
            params.film_render.grain.monochrome,
        );
        halo = halo.max(3.0 * gp.blur_px as f64);
        for sigmas in &gp.dye_cloud_sigma_px {
            for &s in sigmas {
                halo = halo.max(3.0 * s as f64);
            }
        }
        if let Some([_, blur]) = gp.micro_structure {
            halo = halo.max(3.0 * blur as f64);
        }
        if let Some([sigma, _]) = gp.mult_usm {
            halo = halo.max(3.0 * sigma as f64);
        }
    }
    let glare = if scan_film { &params.film_render.glare } else { &params.print_render.glare };
    if glare.active && glare.percent > 0.0 && glare.blur > 0.0 {
        halo = halo.max(3.0 * glare.blur as f64);
    }
    if let Some(cd) = stages::scanning::chroma_denoise_params(params, full_w, full_h) {
        halo = halo.max((cd.factor * (cd.radius + 1)) as f64);
    }
    if params.scanner.lens_blur > 0.0 {
        halo = halo.max(3.0 * params.scanner.lens_blur as f64);
    }
    let [usm_sigma, usm_amount] = params.scanner.unsharp_mask;
    if usm_sigma > 0.0 && usm_amount > 0.0 {
        halo = halo.max(3.0 * usm_sigma as f64);
    }
    if film_effects && params.camera.diffusion_filter.active {
        if let Some(plan) = spektrafilm_model::diffusion::diffusion_gpu_plan(
            &params.camera.diffusion_filter.to_model(),
            pix_um,
            full_w,
            full_h,
        ) {
            halo = halo.max(plan.psf.radius as f64);
        }
    }
    if !scan_film && params.enlarger.diffusion_filter.active {
        if let Some(plan) = spektrafilm_model::diffusion::diffusion_gpu_plan(
            &params.enlarger.diffusion_filter.to_model(),
            pix_um,
            full_w,
            full_h,
        ) {
            halo = halo.max(plan.psf.radius as f64);
        }
    }
    (halo.ceil() as u32).min(MAX_TILE_HALO)
}

#[derive(Deserialize)]
struct CropSpec {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(default)]
    angle: f64,
}

#[derive(Deserialize)]
struct BorderSpec {
    style: String,
    options: BorderOptions,
    #[serde(default)]
    instagram: bool,
}

/// Crop, straighten, and quarter-turn rotate an RGB8 image, matching
/// `web/crop.js::applyRgba` and `spektrafilm-io::crop.rs` pixel for pixel.
fn apply_crop_rotate_rgb8(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    crop: Option<&CropSpec>,
    rotation: i32,
) -> (Vec<u8>, u32, u32) {
    let turns = rotation.rem_euclid(4) as u32;
    let c = match crop {
        Some(c) => c,
        None => &CropSpec { x: 0.0, y: 0.0, width: 1.0, height: 1.0, angle: 0.0 },
    };
    let is_full = c.angle == 0.0 && c.x == 0.0 && c.y == 0.0 && c.width == 1.0 && c.height == 1.0;
    if turns == 0 && is_full {
        return (rgb, width, height);
    }
    let (w, h) = (width as usize, height as usize);
    if c.angle == 0.0 {
        let edge = |f: f64, n: u32| (f * n as f64).round().clamp(0.0, n as f64) as usize;
        let x0 = edge(c.x, width);
        let y0 = edge(c.y, height);
        let x1 = edge(c.x + c.width, width).max(x0 + 1).min(w);
        let y1 = edge(c.y + c.height, height).max(y0 + 1).min(h);
        let x0 = x0.min(x1 - 1);
        let y0 = y0.min(y1 - 1);
        let (rw, rh) = (x1 - x0, y1 - y0);
        if turns == 0 {
            let mut out = vec![0u8; rw * rh * 3];
            out.chunks_exact_mut(rw * 3).enumerate().for_each(|(y, row)| {
                let si = ((y0 + y) * w + x0) * 3;
                row.copy_from_slice(&rgb[si..si + rw * 3]);
            });
            return (out, rw as u32, rh as u32);
        }
        let (dw, dh) = if turns % 2 == 1 { (rh, rw) } else { (rw, rh) };
        let mut out = vec![0u8; dw * dh * 3];
        out.chunks_exact_mut(dw * 3).enumerate().for_each(|(y, row)| {
            for x in 0..dw {
                let (sx, sy) = match turns {
                    1 => (y, rh - 1 - x),
                    2 => (rw - 1 - x, rh - 1 - y),
                    _ => (rw - 1 - y, x),
                };
                let si = ((y0 + sy) * w + (x0 + sx)) * 3;
                row[x * 3..x * 3 + 3].copy_from_slice(&rgb[si..si + 3]);
            }
        });
        return (out, dw as u32, dh as u32);
    }

    // Straightened crop + rotation (matches web/crop.js::presentation + applyRgba).
    let rx = c.x * width as f64;
    let ry = c.y * height as f64;
    let rw = (c.width * width as f64).round().max(1.0);
    let rh = (c.height * height as f64).round().max(1.0);
    let (dw, dh) = if turns % 2 == 1 { (rh as usize, rw as usize) } else { (rw as usize, rh as usize) };
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let rad = (-c.angle).to_radians();
    let (sin_a, cos_a) = (rad.sin(), rad.cos());
    let turn_pt = |px: f64, py: f64| -> (f64, f64) {
        let dx = px - cx;
        let dy = py - cy;
        (cx + dx * cos_a - dy * sin_a, cy + dx * sin_a + dy * cos_a)
    };
    let at = |u: f64, v: f64| -> (f64, f64) {
        let (mut px, mut py) = (u, v);
        for _ in 0..turns {
            (px, py) = (py, 1.0 - px);
        }
        turn_pt(rx + px * rw, ry + py * rh)
    };
    let (ox, oy) = at(0.0, 0.0);
    let (ax, ay) = at(1.0, 0.0);
    let (bx, by) = at(0.0, 1.0);
    let m0 = (ax - ox) / width as f64;
    let m1 = (bx - ox) / width as f64;
    let m2 = ox / width as f64;
    let m4 = (ay - oy) / height as f64;
    let m5 = (by - oy) / height as f64;
    let m6 = oy / height as f64;
    let (max_x, max_y) = (w - 1, h - 1);
    let mut out = vec![0u8; dw * dh * 3];
    out.chunks_exact_mut(dw * 3).enumerate().for_each(|(y, row)| {
        let v = (y as f64 + 0.5) / dh as f64;
        for x in 0..dw {
            let u = (x as f64 + 0.5) / dw as f64;
            let sx = ((m0 * u + m1 * v + m2) * width as f64 - 0.5).clamp(0.0, max_x as f64);
            let sy = ((m4 * u + m5 * v + m6) * height as f64 - 0.5).clamp(0.0, max_y as f64);
            let x0 = sx.floor() as usize;
            let y0 = sy.floor() as usize;
            let x1 = (x0 + 1).min(max_x);
            let y1 = (y0 + 1).min(max_y);
            let fx = sx - x0 as f64;
            let fy = sy - y0 as f64;
            let i00 = (y0 * w + x0) * 3;
            let i10 = (y0 * w + x1) * 3;
            let i01 = (y1 * w + x0) * 3;
            let i11 = (y1 * w + x1) * 3;
            for ch in 0..3 {
                let top = rgb[i00 + ch] as f64 * (1.0 - fx) + rgb[i10 + ch] as f64 * fx;
                let low = rgb[i01 + ch] as f64 * (1.0 - fx) + rgb[i11 + ch] as f64 * fx;
                row[x * 3 + ch] = (top * (1.0 - fy) + low * fy).round() as u8;
            }
        }
    });
    (out, dw as u32, dh as u32)
}

#[wasm_bindgen(getter_with_clone)]
pub struct ExportPrepareResult {
    pub missing: Vec<String>,
}

#[wasm_bindgen(getter_with_clone)]
pub struct ExportPlan {
    pub width: u32,
    pub height: u32,
    #[wasm_bindgen(js_name = totalTiles)]
    pub total_tiles: u32,
    #[wasm_bindgen(js_name = tileWidth)]
    pub tile_width: u32,
    #[wasm_bindgen(js_name = tileHeight)]
    pub tile_height: u32,
    pub halo: u32,
    #[wasm_bindgen(js_name = sourceKind)]
    pub source_kind: String,
    #[wasm_bindgen(js_name = memoryMiB)]
    pub memory_mib: u32,
}

#[wasm_bindgen(getter_with_clone)]
pub struct TileProgress {
    pub done: u32,
    pub total: u32,
    #[wasm_bindgen(js_name = memoryMiB)]
    pub memory_mib: u32,
}

#[wasm_bindgen]
pub struct ExportedJpeg {
    pub width: u32,
    pub height: u32,
    #[wasm_bindgen(js_name = memoryMiB)]
    pub memory_mib: u32,
    jpeg: Vec<u8>,
}

#[wasm_bindgen]
impl ExportedJpeg {
    #[wasm_bindgen(js_name = takeJpeg)]
    pub fn take_jpeg(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.jpeg)
    }
}

struct TileGrid {
    full_w: u32,
    full_h: u32,
    step_w: u32,
    step_h: u32,
    padded_w: u32,
    padded_h: u32,
    cols: u32,
    rows: u32,
    halo: u32,
}

impl TileGrid {
    fn total(&self) -> u32 {
        self.cols * self.rows
    }
}

struct ExporterState {
    backend: Rc<dyn ComputeBackend>,
    gpu: Option<Rc<WgpuBackend>>,
    film: String,
    paper: String,
    params: Option<RuntimeParams>,
    pipeline: Option<Pipeline>,
    source: Option<ExportSource>,
    input_colour: Option<InputColour>,
    autoexposure_ev: f64,
    tiled_prep: Option<TiledPrep>,
    grid: Option<TileGrid>,
    next_tile: u32,
    output_rgb: Vec<u8>,
}

impl Drop for ExporterState {
    fn drop(&mut self) {
        if let Some(gpu) = self.gpu.take() {
            gpu.destroy_device();
        }
    }
}

#[wasm_bindgen]
pub struct TiledExporter {
    state: Rc<RefCell<ExporterState>>,
}

#[wasm_bindgen]
impl TiledExporter {
    /// Start an export engine (WebGPU if `use_gpu`, else CPU).
    pub async fn create(use_gpu: bool) -> Result<TiledExporter, JsError> {
        let (backend, gpu): (Rc<dyn ComputeBackend>, Option<Rc<WgpuBackend>>) = if use_gpu {
            match WgpuBackend::new_async().await {
                Some(g) => {
                    g.set_diffusion_grid_budget(EXPORT_DIFFUSION_GRID_BUDGET);
                    let g = Rc::new(g);
                    (g.clone(), Some(g))
                }
                None => (Rc::new(CpuBackend), None),
            }
        } else {
            (Rc::new(CpuBackend), None)
        };
        Ok(TiledExporter {
            state: Rc::new(RefCell::new(ExporterState {
                backend,
                gpu,
                film: String::new(),
                paper: String::new(),
                params: None,
                pipeline: None,
                source: None,
                input_colour: None,
                autoexposure_ev: 0.0,
                tiled_prep: None,
                grid: None,
                next_tile: 0,
                output_rgb: Vec::new(),
            })),
        })
    }

    pub fn backend(&self) -> String {
        self.state.borrow().backend.name().to_owned()
    }

    /// Calibrate the pipeline and probe any border textures so the caller can
    /// fetch missing data files BEFORE sending the photograph's bytes.
    pub fn prepare(
        &self,
        film: String,
        paper: String,
        params_json: String,
        border_json: Option<String>,
    ) -> Result<ExportPrepareResult, JsError> {
        let params = parse_params(&params_json).map_err(js_error)?;
        data_fs::take_missing();
        let built = calibrate(&film, &paper, &params);
        let mut need = missing();
        if let Some(ref bj) = border_json {
            if let Ok(spec) = serde_json::from_str::<BorderSpec>(bj) {
                if let Ok(style) = spec.style.parse::<BorderStyle>() {
                    if matches!(style, BorderStyle::Film | BorderStyle::Film35) {
                        // Probe missing border textures on a tiny 3×2 dummy image.
                        let _ = spektrafilm_io::border::add_border_rgb8_with(
                            &[0u8; 18],
                            3,
                            2,
                            style,
                            &spec.options,
                            &film,
                            &load_data_image,
                        );
                        need.extend(missing());
                    }
                }
            }
        }
        need.sort();
        need.dedup();
        if !need.is_empty() {
            return Ok(ExportPrepareResult { missing: need });
        }
        let pipeline = built.map_err(|e| JsError::new(&e))?;
        let mut state = self.state.borrow_mut();
        state.film = film;
        state.paper = paper;
        state.params = Some(params);
        state.pipeline = Some(pipeline);
        Ok(ExportPrepareResult { missing: Vec::new() })
    }

    /// Decode `bytes` compactly, drop `bytes` immediately, compute global
    /// auto-exposure / Prep guided base, and plan the fixed-size tile grid.
    pub fn start(
        &self,
        bytes: Vec<u8>,
        white_balance: Option<String>,
        autoexposure_ev: Option<f64>,
    ) -> Result<ExportPlan, JsError> {
        let wb = parse_white_balance(white_balance.as_deref())?;
        let (source, colour) = decode_export_source(bytes, wb)?;
        let (full_w, full_h) = source.full_size();
        let source_kind = source.kind().to_owned();

        let mut state = self.state.borrow_mut();
        let mut params = state
            .params
            .clone()
            .ok_or_else(|| JsError::new("prepare() must succeed before start()"))?;
        params.io.apply_input_colour(&colour);
        spektrafilm_core::image_size::clear(&mut params);
        let pipeline = state
            .pipeline
            .as_ref()
            .ok_or_else(|| JsError::new("prepare() must succeed before start()"))?
            .clone()
            .with_params(params.clone());

        let ae_ev = if !params.workflow.converts_film() && params.camera.auto_exposure {
            match autoexposure_ev.filter(|v| v.is_finite()) {
                Some(ev) => ev,
                None => {
                    let preview = source.metering_preview().map_err(js_error)?;
                    pipeline.autoexposure_ev(&preview)
                }
            }
        } else {
            0.0
        };

        let converting = params.workflow.converts_film();
        let need_tiled_prep = !converting && !params.prep.is_identity() && !params.prep.is_exposure_only();
        let tiled_prep = if need_tiled_prep {
            Some(
                TiledPrep::build(full_w, full_h, &params, ae_ev, |y0, h| {
                    source.extract_rect(0, y0, full_w, h)
                })
                .map_err(|e| JsError::new(&e))?,
            )
        } else {
            None
        };

        let target_pixels = if state.backend.is_gpu() { GPU_TILE_PIXELS } else { CPU_TILE_PIXELS };
        let grid = plan_grid(full_w, full_h, &pipeline, target_pixels);
        let total_tiles = grid.total();
        let tile_width = grid.padded_w;
        let tile_height = grid.padded_h;
        let halo = grid.halo;

        let out_len = (full_w as usize)
            .checked_mul(full_h as usize)
            .and_then(|n| n.checked_mul(3))
            .ok_or_else(|| JsError::new("export image size overflow"))?;
        state.output_rgb = vec![0u8; out_len];
        state.params = Some(params);
        state.pipeline = Some(pipeline);
        state.source = Some(source);
        state.input_colour = Some(colour);
        state.autoexposure_ev = ae_ev;
        state.tiled_prep = tiled_prep;
        state.grid = Some(grid);
        state.next_tile = 0;

        Ok(ExportPlan {
            width: full_w,
            height: full_h,
            total_tiles,
            tile_width,
            tile_height,
            halo,
            source_kind,
            memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
        })
    }

    /// Render the next tile and write its valid interior into `output_rgb`.
    #[wasm_bindgen(js_name = renderNextTile)]
    pub fn render_next_tile(&self) -> js_sys::Promise {
        let state = self.state.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            render_next_tile(&state).await.map(JsValue::from).map_err(JsValue::from)
        })
    }

    /// Free source & GPU buffers, apply crop/rotation/border on RGB8, and
    /// encode the final JPEG directly in Rust.
    #[wasm_bindgen(js_name = finishJpeg)]
    pub fn finish_jpeg(
        &self,
        crop_json: Option<String>,
        rotation: i32,
        border_json: Option<String>,
        quality: u8,
    ) -> Result<ExportedJpeg, JsError> {
        let (rgb, full_w, full_h, film) = {
            let mut state = self.state.borrow_mut();
            // Release decoded source, Prep base, pipeline, and tear down the
            // export WebGPU device immediately before crop/border/JPEG encode
            // so Safari reclaims Metal memory and never starves the editor's GPU.
            state.source = None;
            state.tiled_prep = None;
            state.pipeline = None;
            if let Some(gpu) = state.gpu.take() {
                gpu.destroy_device();
            }
            state.backend = Rc::new(CpuBackend);
            let grid = state.grid.take().ok_or_else(|| JsError::new("no export in progress"))?;
            let rgb = std::mem::take(&mut state.output_rgb);
            (rgb, grid.full_w, grid.full_h, state.film.clone())
        };

        let crop: Option<CropSpec> = match crop_json.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(s) => Some(serde_json::from_str(s).map_err(|e| JsError::new(&e.to_string()))?),
        };
        let (mut rgb, mut w, mut h) = apply_crop_rotate_rgb8(rgb, full_w, full_h, crop.as_ref(), rotation);

        if let Some(bj) = border_json.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let spec: BorderSpec = serde_json::from_str(bj).map_err(|e| JsError::new(&e.to_string()))?;
            let style: BorderStyle = spec.style.parse().map_err(js_error)?;
            let (bordered, bw, bh) = spektrafilm_io::border::add_border_rgb8_with(
                &rgb,
                w,
                h,
                style,
                &spec.options,
                &film,
                &load_data_image,
            )
            .map_err(js_error)?;
            drop(rgb);
            if spec.instagram {
                let (ig, iw, ih) = spektrafilm_io::border::extend_for_instagram_rgb8(
                    &bordered,
                    bw,
                    bh,
                    style,
                    &spec.options,
                );
                drop(bordered);
                rgb = ig;
                w = iw;
                h = ih;
            } else {
                rgb = bordered;
                w = bw;
                h = bh;
            }
        }

        let q = quality.clamp(1, 100);
        let mut jpeg = Vec::with_capacity(((w as usize * h as usize) / 6).max(64 * 1024));
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, q);
        encoder
            .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
            .map_err(|e| JsError::new(&format!("JPEG encode failed: {e}")))?;
        drop(rgb);

        Ok(ExportedJpeg {
            width: w,
            height: h,
            memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
            jpeg,
        })
    }
}

fn plan_grid(full_w: u32, full_h: u32, pipeline: &Pipeline, target_pixels: u64) -> TileGrid {
    if (full_w as u64) * (full_h as u64) <= target_pixels {
        return TileGrid {
            full_w,
            full_h,
            step_w: full_w,
            step_h: full_h,
            padded_w: full_w,
            padded_h: full_h,
            cols: 1,
            rows: 1,
            halo: 0,
        };
    }
    let halo = required_halo(pipeline, full_w, full_h);
    let side = (target_pixels as f64).sqrt().floor() as u32;
    let raw_pw = side.min(full_w).max(1);
    let raw_ph = ((target_pixels / raw_pw as u64) as u32).min(full_h).max(1);
    let step_w = if raw_pw >= full_w {
        full_w
    } else {
        raw_pw.saturating_sub(2 * halo).max(256).min(full_w)
    };
    let step_h = if raw_ph >= full_h {
        full_h
    } else {
        raw_ph.saturating_sub(2 * halo).max(256).min(full_h)
    };
    let padded_w = if step_w >= full_w { full_w } else { (step_w + 2 * halo).min(full_w) };
    let padded_h = if step_h >= full_h { full_h } else { (step_h + 2 * halo).min(full_h) };
    let cols = full_w.div_ceil(step_w);
    let rows = full_h.div_ceil(step_h);
    TileGrid { full_w, full_h, step_w, step_h, padded_w, padded_h, cols, rows, halo }
}

async fn render_next_tile(state: &RefCell<ExporterState>) -> Result<TileProgress, JsError> {
    let (tile_buf, pipeline, backend, encoded, prep_was_applied, full_w, full_h, ix0, iy0, copy_w, copy_h, src_dx, src_dy, total) = {
        let state = state.borrow();
        let grid = state.grid.as_ref().ok_or_else(|| JsError::new("no export in progress"))?;
        let total = grid.total();
        if state.next_tile >= total {
            return Ok(TileProgress {
                done: total,
                total,
                memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
            });
        }
        let tx = state.next_tile % grid.cols;
        let ty = state.next_tile / grid.cols;
        let ix0 = tx * grid.step_w;
        let iy0 = ty * grid.step_h;
        let ix1 = (ix0 + grid.step_w).min(grid.full_w);
        let iy1 = (iy0 + grid.step_h).min(grid.full_h);
        let ox = ix0.saturating_sub(grid.halo);
        let oy = iy0.saturating_sub(grid.halo);
        let src_dx = ix0 - ox;
        let src_dy = iy0 - oy;

        let source = state.source.as_ref().ok_or_else(|| JsError::new("missing export source"))?;
        let mut tile_buf = source.extract_rect(ox, oy, grid.padded_w, grid.padded_h).map_err(js_error)?;
        let mut pipeline = state
            .pipeline
            .as_ref()
            .ok_or_else(|| JsError::new("missing export pipeline"))?
            .clone()
            .with_autoexposure_ev(state.autoexposure_ev)
            .with_tile(grid.full_w, grid.full_h, ox, oy);
        let prep_was_applied = if let Some(prep) = &state.tiled_prep {
            prep.apply_tile(&mut tile_buf, ox, oy).map_err(|e| JsError::new(&e))?;
            pipeline = pipeline.with_prep_applied();
            true
        } else {
            false
        };
        let encoded = pipeline.params.io.output_cctf_encoding;
        (
            tile_buf,
            pipeline,
            state.backend.clone(),
            encoded,
            prep_was_applied,
            grid.full_w,
            grid.full_h,
            ix0,
            iy0,
            ix1 - ix0,
            iy1 - iy0,
            src_dx,
            src_dy,
            total,
        )
    };

    let (mapped_tx, mapped_rx) = futures_channel::oneshot::channel();
    let on_mapped = Box::new(move || {
        let _ = mapped_tx.send(());
    });
    let post = || pipeline.display_post().map(std::sync::Arc::new);
    let output = if pipeline.display_on_gpu(backend.as_ref()) {
        ChainOutput::Rgba8 { encoded, post: post() }
    } else {
        ChainOutput::Scan
    };
    let submitted = if backend.is_gpu() {
        pipeline.submit_resident(&tile_buf, None, backend.as_ref(), output, on_mapped)
    } else {
        None
    };
    let result = match submitted {
        Some(pending) => {
            mapped_rx.await.map_err(|_| JsError::new("GPU readback was cancelled"))?;
            pipeline.finish_submitted(pending, backend.as_ref())
        }
        None => {
            // Adjust film_format_mm so CPU stages derive the exact full-frame
            // pixel_size_um, and avoid re-applying Prep/EV if TiledPrep ran.
            let mut cpu_params = pipeline.params.clone();
            let tile_long = tile_buf.width.max(tile_buf.height) as f64;
            let full_long = full_w.max(full_h) as f64;
            cpu_params.camera.film_format_mm =
                (cpu_params.camera.film_format_mm as f64 * (tile_long / full_long)) as f32;
            let mut cpu_pipe = pipeline.clone().with_params(cpu_params);
            if prep_was_applied {
                let mut p = cpu_pipe.params.clone();
                p.prep = Default::default();
                p.io.input_cctf_decoding = false;
                p.camera.auto_exposure = false;
                p.camera.exposure_compensation_ev = 0.0;
                cpu_pipe = cpu_pipe.with_params(p).with_autoexposure_ev(0.0);
            }
            let (scan, _) = cpu_pipe
                .try_process_with_backend(tile_buf, &CpuBackend)
                .map_err(|e| JsError::new(&e.to_string()))?;
            ChainResult::Scan(scan)
        }
    };

    let (tw, rgba) = match result {
        ChainResult::Rgba8 { width, rgba, .. } => (width as usize, rgba),
        ChainResult::Scan(scan) => {
            let encoding = Encoding::new(&pipeline.params.io.output_color_space, encoded);
            let (width, _, rgba) = preview::display_rgba(&scan, usize::MAX, encoding);
            (width, rgba)
        }
        ChainResult::Presented { .. } => return Err(JsError::new("An export tile was presented")),
    };

    let done = {
        let mut state = state.borrow_mut();
        let full_w = state.grid.as_ref().unwrap().full_w as usize;
        let out = &mut state.output_rgb;
        for row in 0..copy_h as usize {
            let sy = src_dy as usize + row;
            let dy = iy0 as usize + row;
            let src_start = (sy * tw + src_dx as usize) * 4;
            let dst_start = (dy * full_w + ix0 as usize) * 3;
            let src_slice = &rgba[src_start..src_start + copy_w as usize * 4];
            let dst_slice = &mut out[dst_start..dst_start + copy_w as usize * 3];
            for (s, d) in src_slice.chunks_exact(4).zip(dst_slice.chunks_exact_mut(3)) {
                d[0] = s[0];
                d[1] = s[1];
                d[2] = s[2];
            }
        }
        state.next_tile += 1;
        state.next_tile
    };

    Ok(TileProgress {
        done,
        total,
        memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
    })
}
