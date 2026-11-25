//! Shared memory-bounded tiled full-resolution export for native (Android,
//! low-VRAM desktop) and browser (WASM) clients.
//!
//! Key parity guarantees against whole-frame export:
//! - **Exact auto-exposure metering**: streams `stages::filming::metering_preview_streaming`
//!   over the compact source (bit-identical to whole-frame `metering_preview`).
//! - **Exact global grain USM renormalization**: when multilayer grain's
//!   multiplicative unsharp mask is active and the grid has >1 tile, Pass 1
//!   accumulates exact 64-bit fixed-point `(Σ_before, Σ_after)` sums over each
//!   tile's non-overlapping interior rectangle and feeds the global sums into
//!   Pass 2's `grain_finalize`, matching whole-frame bit for bit.
//! - **Zero fake edge padding**: every tile has the exact same `(padded_w, padded_h)`
//!   so GPU buffer pools allocate ONE buffer set and reuse it across all tiles,
//!   while right/bottom tiles shift `(ox, oy)` inward to `full - padded` so
//!   every tile lies 100% inside `[0..full_w, 0..full_h]` and touches the true
//!   image boundaries for FIR reflect and IIR backward init.
//! - **Exact 8-bit & 16-bit quantization**: GPU `display_rgba8.wgsl` and CPU
//!   quantization both use IEEE round-half-to-even (`round_ties_even`),
//!   matching `spektrafilm_io::save::quantize_u8` / `quantize_u16`.
//! - **Seamless CPU fallback**: tiled CPU renders index grain & glare RNG by
//!   global coordinates `(ox + lx, oy + ly)` and honor global grain USM sums.
//! - **Full finishing & metadata**: supports axis-aligned and straightened
//!   crops (evaluated in `f32`/`f64` per tile without a full-frame `f32`
//!   buffer), flip, quarter-turn rotation, borders + Instagram extension, fit
//!   resizing, output color-space conversion, 8/16-bit JPEG/PNG/TIFF, EXIF,
//!   and ICC profiles.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use image::ImageDecoder;
use rayon::prelude::*;
use spektrafilm_gpu::{ChainOutput, ChainResult, ComputeBackend, cpu_backend::CpuBackend};
use spektrafilm_io::{
    Formats, InputColour, InputInterpretation, LensCorrection, RawWhiteBalance,
    border::{
        BorderOptions, BorderStyle, add_border, add_border_rgb8_with, extend_for_instagram,
        extend_for_instagram_rgb8,
    },
    bounded::CompactRawSensor,
    colour::FileColour,
    crop::Crop,
    encoding::Encoding,
    metadata::ImageMetadata,
    resize::{self, Fit},
    save::{SaveOptions, save_image_with_options, save_rgb8_with_options, save_rgb16_with_options},
};
use spektrafilm_math::{
    colorspace::RgbColourspace,
    image::ImageBuf,
    precision::{from_f32, from_f64, to_f32, to_f64},
};
use web_time::Instant;

use crate::{
    image_size,
    params::RuntimeParams,
    pipeline::{Pipeline, clip_to_display},
    post::{PostParams, PostProcess},
    prep::TiledPrep,
    stages,
};

/// Minimum sensor pixel count to keep RAWs in compact `u16` CFA / LinearRaw
/// storage instead of developing into a full-frame `f32` `ImageBuf`.
pub const COMPACT_RAW_MIN_PIXELS: usize = 8_000_000;

/// Default target padded tile pixel count for GPU tiled rendering (~2.4 MP →
/// ~29 MiB/buffer, ~115 MiB 4-buffer GPU working set, reused across every tile).
pub const DEFAULT_GPU_TILE_PIXELS: u64 = 2_400_000;

/// Conservative tile pixel count for low-RAM (<5 GiB) GPUs / WebGPU workers.
pub const COMPACT_GPU_TILE_PIXELS: u64 = 1_800_000;

/// Default target padded tile pixel count on the CPU fallback.
pub const DEFAULT_CPU_TILE_PIXELS: u64 = 1_800_000;

/// Upper bound on per-tile overlap halo so extreme blur radii cannot collapse
/// the tile interior below a productive size.
pub const DEFAULT_MAX_TILE_HALO: u32 = 384;

/// Compact source abstraction for tiled full-resolution export.
pub enum ExportSource {
    CompactRaw(CompactRawSensor),
    RasterRgb8 {
        img: image::RgbImage,
        info: FileColour,
        png: bool,
        interpretation: InputInterpretation,
    },
    Full(ImageBuf),
}

impl ExportSource {
    /// Wrap an already-decoded `ImageBuf` (useful for tests and small inputs).
    pub fn from_image(image: ImageBuf) -> Self {
        Self::Full(image)
    }

    /// Open a native file path compactly (no extra full-file byte buffer copy,
    /// LibRaw fallback supported). Returns `(source, input_colour, lens_supported)`.
    pub fn open_path(
        path: &Path,
        wb: RawWhiteBalance,
        lens: LensCorrection,
        interpretation: &InputInterpretation,
        compact_min_pixels: usize,
    ) -> Result<(Self, InputColour, bool)> {
        if spektrafilm_io::is_raw(path) {
            wb.validate()?;
            if let Some(sensor) = CompactRawSensor::try_open_path(path, wb, lens, compact_min_pixels)? {
                let colour = InputColour::raw(sensor.decoder_name());
                let lens_supported = sensor.lens_correction_supported();
                return Ok((Self::CompactRaw(sensor), colour, lens_supported));
            }
            let loaded = spektrafilm_io::load_image_colour(path, Formats::Any, wb, lens, interpretation)?;
            return Ok((Self::Full(loaded.image), loaded.colour, loaded.lens_correction_supported));
        }

        // Raster file: keep 8-bit sRGB/ICC images as compact 3 B/px RGB8.
        if let Ok(compact) = decode_raster_path_compact(path, interpretation) {
            return Ok((compact.0, compact.1, false));
        }
        let loaded = spektrafilm_io::load_image_colour(path, Formats::Any, wb, lens, interpretation)?;
        Ok((Self::Full(loaded.image), loaded.colour, false))
    }

    /// Open an in-memory file byte buffer compactly (browser/WASM).
    pub fn open_bytes(
        bytes: &[u8],
        wb: RawWhiteBalance,
        xtrans_bilinear_above: usize,
        compact_min_pixels: usize,
    ) -> Result<(Self, InputColour)> {
        if !spektrafilm_io::raw::is_dng(bytes) {
            if let Ok(result) = decode_raster_bytes_compact(bytes, &InputInterpretation::default()) {
                return Ok(result);
            }
        }
        wb.validate()?;
        if let Some(sensor) = CompactRawSensor::try_open_bytes(bytes, wb, compact_min_pixels)? {
            let colour = InputColour::raw(sensor.decoder_name());
            return Ok((Self::CompactRaw(sensor), colour));
        }
        let xtrans = |sensor_pixels| {
            if sensor_pixels > xtrans_bilinear_above {
                spektrafilm_io::XTransDemosaic::Bilinear
            } else {
                spektrafilm_io::XTransDemosaic::Markesteijn
            }
        };
        let (image, exif) = spektrafilm_io::load_raw_bytes_unoriented_with(bytes, wb, xtrans)?;
        Ok((Self::Full(image.oriented(exif)), InputColour::raw("rawler")))
    }

    pub fn full_size(&self) -> (u32, u32) {
        match self {
            Self::CompactRaw(s) => s.full_size(),
            Self::RasterRgb8 { img, .. } => img.dimensions(),
            Self::Full(buf) => (buf.width, buf.height),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::CompactRaw(s) => s.kind(),
            Self::RasterRgb8 { .. } => "raster-rgb8",
            Self::Full(_) => "full-f32",
        }
    }

    /// Extract an upright `(w × h)` linear sRGB `ImageBuf` starting at `(ox, oy)`.
    pub fn extract_rect(&self, ox: u32, oy: u32, w: u32, h: u32) -> Result<ImageBuf> {
        match self {
            Self::CompactRaw(sensor) => sensor.extract_upright_rect(ox, oy, w, h),
            Self::RasterRgb8 { img, info, png, interpretation } => {
                let (fw, fh) = img.dimensions();
                let (fw, fh) = (fw as usize, fh as usize);
                let (tw, th) = (w as usize, h as usize);
                let (ox, oy) = (ox as usize, oy as usize);
                let src = img.as_raw();
                let mut raw = vec![0u8; tw * th * 3];
                raw.par_chunks_exact_mut(tw * 3).enumerate().for_each(|(ly, row)| {
                    let gy = (oy + ly).min(fh - 1);
                    let src_row = gy * fw * 3;
                    if ox + tw <= fw {
                        row.copy_from_slice(&src[src_row + ox * 3..src_row + (ox + tw) * 3]);
                    } else {
                        for lx in 0..tw {
                            let gx = (ox + lx).min(fw - 1);
                            let si = src_row + gx * 3;
                            row[lx * 3..lx * 3 + 3].copy_from_slice(&src[si..si + 3]);
                        }
                    }
                });
                let tile = image::RgbImage::from_raw(w, h, raw)
                    .ok_or_else(|| anyhow::anyhow!("tile RGB8 buffer mismatch"))?;
                let (buf, _) = spektrafilm_io::colour::convert(
                    image::DynamicImage::ImageRgb8(tile),
                    info,
                    interpretation,
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
                data.par_chunks_exact_mut(tw * 3).enumerate().for_each(|(ly, row)| {
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

    /// Bit-identical auto-exposure metering frame (≤ 256 px on the long side),
    /// matching whole-frame `stages::filming::metering_preview` without
    /// materializing a full-resolution `f32` frame.
    pub fn metering_preview(&self) -> Result<ImageBuf> {
        match self {
            Self::Full(buf) => Ok(stages::filming::metering_preview(buf)),
            _ => {
                let (w, h) = self.full_size();
                stages::filming::metering_preview_streaming(w, h, |y0, rows| {
                    self.extract_rect(0, y0, w, rows)
                })
            }
        }
    }
}

fn decode_raster_bytes_compact(
    bytes: &[u8],
    interpretation: &InputInterpretation,
) -> Result<(ExportSource, InputColour)> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.no_limits();
    let format = reader.format().ok_or_else(|| anyhow::anyhow!("unknown image format"))?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut info = FileColour {
        icc: decoder.icc_profile().ok().flatten(),
        exif: decoder.exif_metadata().ok().flatten(),
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
    finish_compact_raster(img, info, png, interpretation)
}

fn decode_raster_path_compact(
    path: &Path,
    interpretation: &InputInterpretation,
) -> Result<(ExportSource, InputColour)> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    reader.no_limits();
    let format = reader.format().ok_or_else(|| anyhow::anyhow!("unknown image format"))?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let open = || std::fs::File::open(path).map(std::io::BufReader::new);
    let mut info = FileColour {
        icc: decoder.icc_profile().ok().flatten(),
        exif: decoder.exif_metadata().ok().flatten(),
        cmyk: matches!(
            decoder.original_color_type(),
            image::ExtendedColorType::Cmyk8 | image::ExtendedColorType::Cmyk16
        ),
        ..Default::default()
    };
    match format {
        image::ImageFormat::Png => info.png = open().ok().and_then(spektrafilm_io::colour::tags::png_chunks),
        image::ImageFormat::Jpeg => info.cmyk |= open().ok().and_then(spektrafilm_io::colour::tags::jpeg_components) == Some(4),
        image::ImageFormat::Tiff if info.icc.is_none() => info.icc = open().ok().and_then(spektrafilm_io::colour::tags::tiff_icc),
        _ => {}
    }
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    info.float = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    info.gray = !img.color().has_color();
    let png = format == image::ImageFormat::Png;
    finish_compact_raster(img, info, png, interpretation)
}

fn finish_compact_raster(
    img: image::DynamicImage,
    info: FileColour,
    png: bool,
    interpretation: &InputInterpretation,
) -> Result<(ExportSource, InputColour)> {
    if !info.float
        && !info.cmyk
        && matches!(
            img,
            image::DynamicImage::ImageRgb8(_)
                | image::DynamicImage::ImageRgba8(_)
                | image::DynamicImage::ImageLuma8(_)
                | image::DynamicImage::ImageLumaA8(_)
        )
    {
        let probe = image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        let (_, colour) = spektrafilm_io::colour::convert(probe, &info, interpretation, png)?;
        let rgb8 = img.into_rgb8();
        Ok((
            ExportSource::RasterRgb8 {
                img: rgb8,
                info,
                png,
                interpretation: interpretation.clone(),
            },
            colour,
        ))
    } else {
        let (buf, colour) = spektrafilm_io::colour::convert(img, &info, interpretation, png)?;
        Ok((ExportSource::Full(buf), colour))
    }
}

/// Compute the pixel halo needed around each tile for all active sequential
/// spatial kernels at full resolution `(full_w, full_h)`.
pub fn required_halo(pipeline: &Pipeline, full_w: u32, full_h: u32, max_halo: u32) -> u32 {
    let params = &pipeline.params;
    let converting = params.workflow.converts_film();
    let film_effects = !converting;
    let scan_film = params.io.scan_film && !converting;
    let pix_um = stages::filming::pixel_size_um(params.camera.film_format_mm, full_w, full_h) as f64;
    // FIR Gaussian (σ < 3): exact radius = floor(3σ + 0.5).
    // IIR Gaussian (σ ≥ 3): 5.5σ puts the tail at < 3e-7.
    let gauss_radius = |sigma: f64| -> f64 {
        if sigma <= 0.0 {
            0.0
        } else if sigma < 3.0 {
            (3.0 * sigma + 0.5).floor()
        } else {
            5.5 * sigma
        }
    };
    let mut halo = 0.0_f64;

    if film_effects && params.camera.diffusion_filter.active {
        if let Some(plan) = spektrafilm_model::diffusion::diffusion_gpu_plan(
            &params.camera.diffusion_filter.to_model(),
            pix_um,
            full_w,
            full_h,
        ) {
            halo += plan.psf.radius as f64;
        }
    }
    if film_effects && params.camera.lens_blur_um > 0.0 {
        halo += gauss_radius(params.camera.lens_blur_um as f64 / pix_um);
    }
    if film_effects && params.film_render.halation.active {
        let h = &params.film_render.halation;
        if h.scatter_amount > 0.0 {
            let mut s_rad = 0.0_f64;
            for c in 0..3 {
                let core = h.scatter_core_um[c] * h.scatter_spatial_scale / pix_um;
                let tail_max = 2.7684 * h.scatter_tail_um[c] * h.scatter_spatial_scale / pix_um;
                s_rad = s_rad.max(gauss_radius(core)).max(gauss_radius(tail_max));
            }
            halo += s_rad;
        }
        if h.halation_amount > 0.0 && h.halation_n_bounces > 0 {
            let max_first = h.first_sigma_um().into_iter().fold(0.0_f64, f64::max);
            let max_sigma = max_first * h.halation_spatial_scale / pix_um * (h.halation_n_bounces as f64).sqrt();
            halo += gauss_radius(max_sigma);
        }
    }
    if film_effects && params.film_render.dir_couplers.active {
        let dir = &params.film_render.dir_couplers;
        let core = dir.diffusion_size_um / pix_um;
        let tail_max = 2.7684 * dir.diffusion_tail_um / pix_um;
        halo += gauss_radius(core).max(gauss_radius(tail_max));
    }
    if film_effects && crate::grain::grain_active(params) {
        let gp = spektrafilm_model::grain::gpu_params(
            &crate::grain::grain_curves(&pipeline.film),
            &crate::grain::grain_settings(&pipeline.film, &params.film_render.grain),
            stages::filming::grain_pixel_size_um(params, full_w, full_h),
            pipeline.film.is_positive(),
            params.film_render.grain.monochrome,
        );
        let mut dye_rad = 0.0_f64;
        for sigmas in &gp.dye_cloud_sigma_px {
            for &s in sigmas {
                dye_rad = dye_rad.max(gauss_radius(s as f64));
            }
        }
        halo += dye_rad;
        if let Some([_, blur]) = gp.micro_structure {
            halo += gauss_radius(blur as f64);
        }
        halo += gauss_radius(gp.blur_px as f64);
        if let Some([sigma, _]) = gp.mult_usm {
            halo += gauss_radius(sigma as f64);
        }
    }
    if !scan_film && params.enlarger.diffusion_filter.active {
        if let Some(plan) = spektrafilm_model::diffusion::diffusion_gpu_plan(
            &params.enlarger.diffusion_filter.to_model(),
            pix_um,
            full_w,
            full_h,
        ) {
            halo += plan.psf.radius as f64;
        }
    }
    let glare = if scan_film { &params.film_render.glare } else { &params.print_render.glare };
    if glare.active && glare.percent > 0.0 && glare.blur > 0.0 {
        halo += gauss_radius(glare.blur as f64);
    }
    if params.scanner.lens_blur > 0.0 {
        halo += gauss_radius(params.scanner.lens_blur as f64);
    }
    if let Some(cd) = stages::scanning::chroma_denoise_params(params, full_w, full_h) {
        halo += (cd.factor * (cd.radius + 2)) as f64;
    }
    let [usm_sigma, usm_amount] = params.scanner.unsharp_mask;
    if usm_sigma > 0.0 && usm_amount > 0.0 {
        halo += gauss_radius(usm_sigma as f64);
    }
    (halo.ceil() as u32).min(max_halo)
}

/// Fixed-size tile grid over a `(full_w × full_h)` photograph. Every tile has
/// the exact same `(padded_w, padded_h)` and lies 100% inside `[0..full_w, 0..full_h]`.
#[derive(Clone, Copy, Debug)]
pub struct TileGrid {
    pub full_w: u32,
    pub full_h: u32,
    pub step_w: u32,
    pub step_h: u32,
    pub padded_w: u32,
    pub padded_h: u32,
    pub cols: u32,
    pub rows: u32,
    pub halo: u32,
    pub align: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TileSpec {
    pub index: u32,
    pub tx: u32,
    pub ty: u32,
    /// Valid interior rectangle in full-image coordinates.
    pub ix0: u32,
    pub iy0: u32,
    pub ix1: u32,
    pub iy1: u32,
    /// Padded tile origin in full-image coordinates (`0 <= ox`, `ox + padded_w <= full_w`).
    pub ox: u32,
    pub oy: u32,
    /// Offset of the valid interior inside the padded tile.
    pub src_dx: u32,
    pub src_dy: u32,
    pub copy_w: u32,
    pub copy_h: u32,
    pub padded_w: u32,
    pub padded_h: u32,
}

impl TileGrid {
    pub fn plan(
        full_w: u32,
        full_h: u32,
        pipeline: &Pipeline,
        target_pixels: u64,
        max_halo: u32,
    ) -> Self {
        Self::plan_with_min_halo(full_w, full_h, pipeline, target_pixels, max_halo, 0)
    }

    pub fn plan_with_min_halo(
        full_w: u32,
        full_h: u32,
        pipeline: &Pipeline,
        target_pixels: u64,
        max_halo: u32,
        min_halo: u32,
    ) -> Self {
        if (full_w as u64) * (full_h as u64) <= target_pixels {
            return Self {
                full_w,
                full_h,
                step_w: full_w,
                step_h: full_h,
                padded_w: full_w,
                padded_h: full_h,
                cols: 1,
                rows: 1,
                halo: 0,
                align: 1,
            };
        }
        let align = stages::scanning::chroma_denoise_params(&pipeline.params, full_w, full_h)
            .map_or(1, |cd| cd.factor.max(1));
        let side = (target_pixels as f64).sqrt().floor() as u32;
        let raw_pw = side.min(full_w).max(1);
        let raw_ph = ((target_pixels / raw_pw as u64) as u32).min(full_h).max(1);
        // Ensure the tile interior stays at least ~1/3 of the tile side so
        // huge halos don't blow up tile count into 10× redundant work.
        let effective_max_halo = max_halo.min((raw_pw.min(raw_ph) / 3).max(min_halo));
        let halo = required_halo(pipeline, full_w, full_h, effective_max_halo).max(min_halo);

        let align_pad = |raw: u32, full: u32| -> (u32, u32) {
            if raw >= full {
                return (full, full);
            }
            let step = raw.saturating_sub(2 * halo).max(256).min(full);
            let mut padded = (step + 2 * halo).min(full);
            // Make (full - padded) a multiple of `align` so the rightmost /
            // bottommost edge-aligned tile origin is also aligned.
            if align > 1 && padded < full {
                let rem = (full - padded) % align;
                if rem != 0 {
                    padded = (padded + rem).min(full);
                }
            }
            let step = padded.saturating_sub(2 * halo).max(256).min(full);
            (step, padded)
        };
        let (step_w, padded_w) = align_pad(raw_pw, full_w);
        let (step_h, padded_h) = align_pad(raw_ph, full_h);
        let cols = full_w.div_ceil(step_w);
        let rows = full_h.div_ceil(step_h);
        Self {
            full_w,
            full_h,
            step_w,
            step_h,
            padded_w,
            padded_h,
            cols,
            rows,
            halo,
            align,
        }
    }

    pub fn total(&self) -> u32 {
        self.cols * self.rows
    }

    pub fn tile(&self, index: u32) -> TileSpec {
        let tx = index % self.cols;
        let ty = index / self.cols;
        let ix0 = tx * self.step_w;
        let iy0 = ty * self.step_h;
        let ix1 = (ix0 + self.step_w).min(self.full_w);
        let iy1 = (iy0 + self.step_h).min(self.full_h);
        let max_ox = self.full_w - self.padded_w;
        let max_oy = self.full_h - self.padded_h;
        let mut ox = ix0.saturating_sub(self.halo).min(max_ox);
        let mut oy = iy0.saturating_sub(self.halo).min(max_oy);
        if self.align > 1 {
            ox = (ox / self.align) * self.align;
            oy = (oy / self.align) * self.align;
        }
        TileSpec {
            index,
            tx,
            ty,
            ix0,
            iy0,
            ix1,
            iy1,
            ox,
            oy,
            src_dx: ix0 - ox,
            src_dy: iy0 - oy,
            copy_w: ix1 - ix0,
            copy_h: iy1 - iy0,
            padded_w: self.padded_w,
            padded_h: self.padded_h,
        }
    }
}

/// Whether this pipeline & grid needs Pass 1 to compute the global grain
/// multiplicative unsharp mask mass sums before Pass 2.
pub fn needs_grain_usm_pass(pipeline: &Pipeline, grid: &TileGrid) -> bool {
    if grid.total() <= 1 || pipeline.params.workflow.converts_film() {
        return false;
    }
    if !crate::grain::grain_active(&pipeline.params) {
        return false;
    }
    let gs = crate::grain::grain_settings(&pipeline.film, &pipeline.params.film_render.grain);
    gs.mult_usm_amount > 0.0 && gs.mult_usm_sigma > 0.0
}

/// Convert accumulated 64-bit fixed-point (scale 2^16) USM mass sums from
/// Pass 1 into the exact 6 `f32` values `grain_sum_reduce.wgsl` produces for
/// `grain_finalize.wgsl`.
pub fn usm_totals_to_f32(totals: [u64; 6]) -> [f32; 6] {
    std::array::from_fn(|c| {
        let hi = (totals[c] >> 32) as u32 as f32;
        let lo = (totals[c] as u32) as f32;
        (hi * 4_294_967_296.0 + lo) * (1.0 / 65536.0)
    })
}

/// Finishing options applied after tiled rendering (mirrors `Command::Save`).
#[derive(Clone, Debug)]
pub struct ExportFinish {
    pub post: PostParams,
    pub crop: Option<Crop>,
    pub crop_after_post: bool,
    pub flip: bool,
    pub rotate_cw: i32,
    pub border: Option<BorderStyle>,
    pub border_options: BorderOptions,
    pub instagram: bool,
    pub fit: Option<Fit>,
    pub save_options: SaveOptions,
}

impl Default for ExportFinish {
    fn default() -> Self {
        Self {
            post: PostParams::default(),
            crop: None,
            crop_after_post: true,
            flip: false,
            rotate_cw: 0,
            border: None,
            border_options: BorderOptions::default(),
            instagram: false,
            fit: None,
            save_options: SaveOptions::default(),
        }
    }
}

/// Diagnostics returned by [`export_tiled_to_file`] / [`export_tiled_rgb8`].
#[derive(Clone, Debug)]
pub struct TiledExportReport {
    pub source_width: u32,
    pub source_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub source_kind: &'static str,
    pub backend: String,
    pub total_tiles: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub halo: u32,
    pub two_pass_usm: bool,
    pub decode_ms: f64,
    pub pass1_ms: f64,
    pub render_ms: f64,
    pub finish_ms: f64,
    pub elapsed_ms: f64,
}

/// Quantize a single scan scalar to 8 bits, matching `save.rs::quantize_u8`.
#[inline]
pub fn quantize_scalar_u8(v: spektrafilm_math::precision::Scalar) -> u8 {
    let x = if v.is_nan() { 0.0 } else { v.clamp(from_f64(0.0), from_f64(1.0)) };
    (to_f32(x) * 255.0).round_ties_even() as u8
}

/// Quantize a single scan scalar to 16 bits, matching `save.rs::quantize_u16`.
#[inline]
pub fn quantize_scalar_u16(v: spektrafilm_math::precision::Scalar) -> u16 {
    let x = if v.is_nan() { 0.0 } else { v.clamp(from_f64(0.0), from_f64(1.0)) };
    (to_f32(x) * 65535.0).round_ties_even() as u16
}

/// Exact axis-aligned crop + flip + quarter-turn rotate on packed RGB8 bytes
/// (100% bit-identical to `f32` `Crop::apply` + `flipped_rotated_cw` when
/// `crop.angle == 0.0`).
pub fn crop_flip_rotate_rgb8(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    crop: Option<Crop>,
    flip: bool,
    rotate_cw: i32,
) -> (Vec<u8>, u32, u32) {
    let turns = rotate_cw.rem_euclid(4) as u32;
    let (x0, y0, rw, rh) = match crop {
        Some(c) => {
            debug_assert_eq!(c.angle, 0.0, "straightened crop must run in f32 before RGB8");
            c.pixels(width, height)
        }
        None => (0, 0, width, height),
    };
    if !flip && turns == 0 && (x0, y0, rw, rh) == (0, 0, width, height) {
        return (rgb, width, height);
    }
    let (w, rw_u, rh_u) = (width as usize, rw as usize, rh as usize);
    let (x0_u, y0_u) = (x0 as usize, y0 as usize);
    if !flip && turns == 0 {
        let mut out = vec![0u8; rw_u * rh_u * 3];
        out.par_chunks_exact_mut(rw_u * 3).enumerate().for_each(|(y, row)| {
            let si = ((y0_u + y) * w + x0_u) * 3;
            row.copy_from_slice(&rgb[si..si + rw_u * 3]);
        });
        return (out, rw, rh);
    }
    let (dw, dh) = if turns % 2 == 1 { (rh_u, rw_u) } else { (rw_u, rh_u) };
    let mut out = vec![0u8; dw * dh * 3];
    out.par_chunks_exact_mut(dw * 3).enumerate().for_each(|(dy, row)| {
        for dx in 0..dw {
            // Undo quarter-turn CW, then undo horizontal flip (matches `ImageBuf::flipped_rotated_cw`).
            let (mut sx, sy) = match turns {
                0 => (dx, dy),
                1 => (dy, rh_u - 1 - dx),
                2 => (rw_u - 1 - dx, rh_u - 1 - dy),
                _ => (rw_u - 1 - dy, dx),
            };
            if flip {
                sx = rw_u - 1 - sx;
            }
            let si = ((y0_u + sy) * w + (x0_u + sx)) * 3;
            row[dx * 3..dx * 3 + 3].copy_from_slice(&rgb[si..si + 3]);
        }
    });
    (out, dw as u32, dh as u32)
}

/// Exact axis-aligned crop + flip + quarter-turn rotate on packed RGB16 samples.
pub fn crop_flip_rotate_rgb16(
    rgb: Vec<u16>,
    width: u32,
    height: u32,
    crop: Option<Crop>,
    flip: bool,
    rotate_cw: i32,
) -> (Vec<u16>, u32, u32) {
    let turns = rotate_cw.rem_euclid(4) as u32;
    let (x0, y0, rw, rh) = match crop {
        Some(c) => {
            debug_assert_eq!(c.angle, 0.0, "straightened crop must run in f32 before RGB16");
            c.pixels(width, height)
        }
        None => (0, 0, width, height),
    };
    if !flip && turns == 0 && (x0, y0, rw, rh) == (0, 0, width, height) {
        return (rgb, width, height);
    }
    let (w, rw_u, rh_u) = (width as usize, rw as usize, rh as usize);
    let (x0_u, y0_u) = (x0 as usize, y0 as usize);
    let (dw, dh) = if turns % 2 == 1 { (rh_u, rw_u) } else { (rw_u, rh_u) };
    let mut out = vec![0u16; dw * dh * 3];
    out.par_chunks_exact_mut(dw * 3).enumerate().for_each(|(dy, row)| {
        for dx in 0..dw {
            let (mut sx, sy) = match turns {
                0 => (dx, dy),
                1 => (dy, rh_u - 1 - dx),
                2 => (rw_u - 1 - dx, rh_u - 1 - dy),
                _ => (rw_u - 1 - dy, dx),
            };
            if flip {
                sx = rw_u - 1 - sx;
            }
            let si = ((y0_u + sy) * w + (x0_u + sx)) * 3;
            row[dx * 3..dx * 3 + 3].copy_from_slice(&rgb[si..si + 3]);
        }
    });
    (out, dw as u32, dh as u32)
}

/// Crop + straighten + flip + rotate on RGB8 bytes (for browser callers that
/// hold only RGB8). Matches `spektrafilm_io::crop::Crop::apply`.
pub fn apply_crop_flip_rotate_rgb8(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    crop: Option<Crop>,
    flip: bool,
    rotate_cw: i32,
) -> (Vec<u8>, u32, u32) {
    let Some(c) = crop.filter(|c| c.angle != 0.0) else {
        return crop_flip_rotate_rgb8(rgb, width, height, crop, flip, rotate_cw);
    };
    let (cw, ch) = c.straightened_size(width, height);
    let (iw, ih) = (width as f64, height as f64);
    let (cx, cy) = (iw / 2.0, ih / 2.0);
    let (origin_x, origin_y) = (c.x * iw, c.y * ih);
    let (sin, cos) = c.angle.to_radians().sin_cos();
    let (max_x, max_y) = (width as usize - 1, height as usize - 1);
    let mut straightened = vec![0u8; (cw as usize) * (ch as usize) * 3];
    straightened.par_chunks_exact_mut(cw as usize * 3).enumerate().for_each(|(v, row)| {
        for u in 0..cw as usize {
            let (dx, dy) = (origin_x + u as f64 + 0.5 - cx, origin_y + v as f64 + 0.5 - cy);
            let sx = (cx + dx * cos + dy * sin - 0.5).clamp(0.0, max_x as f64);
            let sy = (cy - dx * sin + dy * cos - 0.5).clamp(0.0, max_y as f64);
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(max_x), (y0 + 1).min(max_y));
            let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
            let at = |x: usize, y: usize, ch_i: usize| rgb[(y * width as usize + x) * 3 + ch_i] as f64;
            for ch_i in 0..3 {
                let top = at(x0, y0, ch_i) * (1.0 - fx) + at(x1, y0, ch_i) * fx;
                let bot = at(x0, y1, ch_i) * (1.0 - fx) + at(x1, y1, ch_i) * fx;
                row[u * 3 + ch_i] = (top * (1.0 - fy) + bot * fy).round_ties_even().clamp(0.0, 255.0) as u8;
            }
        }
    });
    crop_flip_rotate_rgb8(straightened, cw, ch, None, flip, rotate_cw)
}

/// Render a full-resolution photograph in tiles directly into packed RGB8
/// (3 B/px), byte-identical to whole-frame GPU export + `save::quantize_u8`.
/// Used by `tiled_check` and integration parity tests.
pub fn export_tiled_rgb8(
    source: &ExportSource,
    pipeline: &Pipeline,
    post: &PostParams,
    backend: &dyn ComputeBackend,
    target_tile_pixels: u64,
    max_halo: u32,
) -> Result<(u32, u32, Vec<u8>, TiledExportReport)> {
    let started = Instant::now();
    let (full_w, full_h) = source.full_size();
    let mut params = pipeline.params.clone();
    params.post = PostParams::default();
    image_size::clear(&mut params);
    let pipeline = pipeline.clone().with_params(params.clone());

    let ae_ev = if !params.workflow.converts_film() && params.camera.auto_exposure {
        let preview = source.metering_preview()?;
        pipeline.autoexposure_ev(&preview)
    } else {
        0.0
    };
    let converting = params.workflow.converts_film();
    let need_tiled_prep = !converting && !params.prep.is_identity() && !params.prep.is_exposure_only();
    let tiled_prep = if need_tiled_prep {
        Some(
            TiledPrep::build(full_w, full_h, &params, ae_ev, |y0, rows| {
                source.extract_rect(0, y0, full_w, rows)
            })
            .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };

    let grid = TileGrid::plan(full_w, full_h, &pipeline, target_tile_pixels, max_halo);
    let total_tiles = grid.total();
    let two_pass_usm = needs_grain_usm_pass(&pipeline, &grid);

    let mut used_backend = backend.name().to_owned();
    let t_pass1 = Instant::now();
    let grain_usm_sums = if two_pass_usm {
        let sums = run_grain_usm_pass1(source, &pipeline, tiled_prep.as_ref(), ae_ev, &grid, backend)?;
        Some(sums)
    } else {
        None
    };
    let pass1_ms = if two_pass_usm { t_pass1.elapsed().as_secs_f64() * 1000.0 } else { 0.0 };

    let t_ren = Instant::now();
    let out_len = (full_w as usize)
        .checked_mul(full_h as usize)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(|| anyhow::anyhow!("export image size overflow"))?;
    let mut out_rgb = vec![0u8; out_len];
    let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
    let post_cpu = (!post.is_identity()).then(|| PostProcess::new(post, encoding.encoded, encoding.space.name));
    let slide_scan = params.io.scan_film && pipeline.film.is_positive();

    for idx in 0..total_tiles {
        let tile = grid.tile(idx);
        let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
        let mut tpipe = pipeline
            .clone()
            .with_autoexposure_ev(ae_ev)
            .with_tile(full_w, full_h, tile.ox, tile.oy);
        if let Some(sums) = grain_usm_sums {
            tpipe = tpipe.with_grain_usm_sums(sums);
        }
        if let Some(prep) = &tiled_prep {
            prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
            tpipe = tpipe.with_prep_applied();
        }

        let gpu_rgba = if !slide_scan
            && post_cpu.is_none()
            && backend.is_gpu()
            && backend.is_healthy()
            && tpipe.display_on_gpu(backend)
        {
            let output = ChainOutput::Rgba8 {
                encoded: encoding.encoded,
                post: None,
            };
            tpipe
                .submit_resident(&tile_buf, None, backend, output, Box::new(|| {}))
                .and_then(|pending| match tpipe.try_finish_submitted(pending, backend) {
                    Ok(ChainResult::Rgba8 { width, rgba, .. }) => Some((width as usize, rgba)),
                    _ => None,
                })
        } else {
            None
        };

        if let Some((tw, rgba)) = gpu_rgba {
            copy_rgba_interior_to_rgb8(&rgba, tw, &tile, full_w as usize, &mut out_rgb);
        } else {
            let (mut scan, ub) = if !slide_scan && backend.is_gpu() && backend.is_healthy() {
                tpipe
                    .process_resident_borrowed(&tile_buf, backend)
                    .map(|s| (s, backend.name()))
                    .unwrap_or_else(|| tpipe.process_with_backend(tile_buf, &CpuBackend))
            } else {
                tpipe.process_with_backend(tile_buf, &CpuBackend)
            };
            used_backend = ub.to_owned();
            clip_to_display(&mut scan, encoding.encoded);
            if let Some(pp) = &post_cpu {
                pp.apply(&mut scan);
            }
            copy_scan_interior_to_rgb8(&scan, &tile, full_w as usize, &mut out_rgb);
        }
    }
    let render_ms = t_ren.elapsed().as_secs_f64() * 1000.0;
    let report = TiledExportReport {
        source_width: full_w,
        source_height: full_h,
        output_width: full_w,
        output_height: full_h,
        source_kind: source.kind(),
        backend: used_backend,
        total_tiles,
        tile_width: grid.padded_w,
        tile_height: grid.padded_h,
        halo: grid.halo,
        two_pass_usm,
        decode_ms: 0.0,
        pass1_ms,
        render_ms,
        finish_ms: 0.0,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    };
    Ok((full_w, full_h, out_rgb, report))
}

/// Full native tiled export from `input_path` to `output_path` with complete
/// finishing (Post, crop/straighten, flip/rotate, border/Instagram, fit,
/// color space conversion, 8/16-bit JPEG/PNG/TIFF, EXIF, and ICC).
#[allow(clippy::too_many_arguments)]
pub fn export_tiled_to_file(
    input_path: &Path,
    output_path: &Path,
    film_name: &str,
    paper_name: &str,
    mut params: RuntimeParams,
    finish: ExportFinish,
    data_dir: &Path,
    backend: &dyn ComputeBackend,
    target_tile_pixels: u64,
    max_halo: u32,
) -> Result<TiledExportReport> {
    let started = Instant::now();
    ensure!(
        input_path != output_path,
        "Choose a different output path to preserve the source"
    );
    if let Some(crop) = &finish.crop {
        crop.validate()?;
    }
    finish.save_options.check(output_path)?;
    ensure!(
        RgbColourspace::find(&params.io.output_color_space).is_some(),
        "Unknown output colour space: {}",
        params.io.output_color_space
    );
    params.io.raw_white_balance.validate()?;
    params.io.input_interpretation.validate()?;
    params.prep.validate().map_err(anyhow::Error::msg)?;
    params.settings.serial_grain = false;
    params.post = PostParams::default();

    let t_dec = Instant::now();
    let (source, input_colour, _) = ExportSource::open_path(
        input_path,
        params.io.raw_white_balance,
        params.io.lens_correction,
        &params.io.input_interpretation,
        COMPACT_RAW_MIN_PIXELS,
    )?;
    let source_kind = source.kind();
    let (full_w, full_h) = source.full_size();
    let decode_ms = t_dec.elapsed().as_secs_f64() * 1000.0;

    params.io.apply_input_colour(&input_colour);
    let size = image_size::resolve(&params, (full_w, full_h), finish.crop.as_ref())?;
    image_size::clear(&mut params);
    // If the user explicitly asked for a custom Image Size working resolution
    // different from the source, resample once before tiling.
    let source = if size.working != (full_w, full_h) {
        let full = source.extract_rect(0, 0, full_w, full_h)?;
        ExportSource::Full(spektrafilm_io::resize::catmull_rom(&full, size.working))
    } else {
        source
    };
    let (work_w, work_h) = source.full_size();

    let film = crate::profile::load_profile_by_name(data_dir, film_name)?;
    let paper = crate::profile::load_profile_by_name(data_dir, paper_name)?;
    let pipeline = Pipeline::new_with_spectral(film, paper, params.clone(), data_dir)
        .map_err(anyhow::Error::msg)?;

    let ae_ev = if !params.workflow.converts_film() && params.camera.auto_exposure {
        let preview = source.metering_preview()?;
        pipeline.autoexposure_ev(&preview)
    } else {
        0.0
    };
    let converting = params.workflow.converts_film();
    let need_tiled_prep = !converting && !params.prep.is_identity() && !params.prep.is_exposure_only();
    let tiled_prep = if need_tiled_prep {
        Some(
            TiledPrep::build(work_w, work_h, &params, ae_ev, |y0, rows| {
                source.extract_rect(0, y0, work_w, rows)
            })
            .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };

    let depth = finish.save_options.resolved_bit_depth(output_path)?;
    let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
    let target_space = RgbColourspace::find(&finish.save_options.color_space)
        .with_context(|| format!("Unknown colour space: {}", finish.save_options.color_space))?;
    let space_differs = target_space.name != encoding.space.name;
    let straightened = finish.crop.is_some_and(|c| c.angle != 0.0);
    let min_halo = if straightened { 2 } else { 0 };

    let grid = TileGrid::plan_with_min_halo(work_w, work_h, &pipeline, target_tile_pixels, max_halo, min_halo);
    let total_tiles = grid.total();
    let two_pass_usm = needs_grain_usm_pass(&pipeline, &grid);

    let t_pass1 = Instant::now();
    let grain_usm_sums = if two_pass_usm {
        Some(run_grain_usm_pass1(&source, &pipeline, tiled_prep.as_ref(), ae_ev, &grid, backend)?)
    } else {
        None
    };
    let pass1_ms = if two_pass_usm { t_pass1.elapsed().as_secs_f64() * 1000.0 } else { 0.0 };

    let t_ren = Instant::now();
    let slide_scan = params.io.scan_film && pipeline.film.is_positive();
    let post_cpu = (!finish.post.is_identity())
        .then(|| PostProcess::new(&finish.post, encoding.encoded, encoding.space.name));
    let mut used_backend = backend.name().to_owned();

    // Fast compact 8-bit path: JPEG / 8-bit PNG / 8-bit TIFF in sRGB with no
    // straightened crop, no fit resize, and no cross-space conversion.
    let fast_rgb8 = depth == 8
        && !straightened
        && finish.fit.is_none()
        && !space_differs
        && (finish.crop_after_post || finish.post.is_identity());

    let metadata = if finish.save_options.copy_metadata {
        spektrafilm_io::read_metadata(input_path)
    } else {
        None
    };
    let save_options = SaveOptions { linear: !encoding.encoded, ..finish.save_options.clone() };

    let (out_w, out_h, render_ms, finish_ms) = if fast_rgb8 {
        let mut out_rgb = vec![0u8; (work_w as usize) * (work_h as usize) * 3];
        for idx in 0..total_tiles {
            let tile = grid.tile(idx);
            let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
            let mut tpipe = pipeline
                .clone()
                .with_autoexposure_ev(ae_ev)
                .with_tile(work_w, work_h, tile.ox, tile.oy);
            if let Some(sums) = grain_usm_sums {
                tpipe = tpipe.with_grain_usm_sums(sums);
            }
            if let Some(prep) = &tiled_prep {
                prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
                tpipe = tpipe.with_prep_applied();
            }

            let gpu_rgba = if !slide_scan
                && post_cpu.is_none()
                && backend.is_gpu()
                && backend.is_healthy()
                && tpipe.display_on_gpu(backend)
            {
                let output = ChainOutput::Rgba8 {
                    encoded: encoding.encoded,
                    post: None,
                };
                tpipe
                    .submit_resident(&tile_buf, None, backend, output, Box::new(|| {}))
                    .and_then(|pending| match tpipe.try_finish_submitted(pending, backend) {
                        Ok(ChainResult::Rgba8 { width, rgba, .. }) => Some((width as usize, rgba)),
                        _ => None,
                    })
            } else {
                None
            };

            if let Some((tw, rgba)) = gpu_rgba {
                copy_rgba_interior_to_rgb8(&rgba, tw, &tile, work_w as usize, &mut out_rgb);
            } else {
                let (mut scan, ub) = render_tile_scan(&tpipe, tile_buf, slide_scan, backend);
                used_backend = ub;
                clip_to_display(&mut scan, encoding.encoded);
                if let Some(pp) = &post_cpu {
                    pp.apply(&mut scan);
                }
                copy_scan_interior_to_rgb8(&scan, &tile, work_w as usize, &mut out_rgb);
            }
        }
        let render_ms = t_ren.elapsed().as_secs_f64() * 1000.0;
        // Release source & prep before finishing/encoding to keep peak RSS low.
        drop(source);
        drop(tiled_prep);

        let t_fin = Instant::now();
        let (mut rgb, mut w, mut h) = crop_flip_rotate_rgb8(
            out_rgb,
            work_w,
            work_h,
            finish.crop,
            finish.flip,
            finish.rotate_cw,
        );
        if let Some(style) = finish.border {
            let load = |name: &str| {
                let path = data_dir.join(name);
                image::open(&path).with_context(|| format!("loading border texture {}", path.display()))
            };
            let (bordered, bw, bh) = add_border_rgb8_with(
                &rgb,
                w,
                h,
                style,
                &finish.border_options,
                film_name,
                &load,
            )?;
            drop(rgb);
            if finish.instagram {
                let (ig, iw, ih) = extend_for_instagram_rgb8(&bordered, bw, bh, style, &finish.border_options);
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
        save_rgb8_with_options(&rgb, w, h, output_path, metadata.as_ref(), &save_options)?;
        (w, h, render_ms, t_fin.elapsed().as_secs_f64() * 1000.0)
    } else if depth == 16 && !straightened && finish.fit.is_none() && finish.border.is_none() {
        // Compact 16-bit RGB16 accumulator (6 B/px, half of f32) for 16-bit PNG/TIFF.
        let mut out_rgb16 = vec![0u16; (work_w as usize) * (work_h as usize) * 3];
        for idx in 0..total_tiles {
            let tile = grid.tile(idx);
            let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
            let mut tpipe = pipeline
                .clone()
                .with_autoexposure_ev(ae_ev)
                .with_tile(work_w, work_h, tile.ox, tile.oy);
            if let Some(sums) = grain_usm_sums {
                tpipe = tpipe.with_grain_usm_sums(sums);
            }
            if let Some(prep) = &tiled_prep {
                prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
                tpipe = tpipe.with_prep_applied();
            }
            let (mut scan, ub) = render_tile_scan(&tpipe, tile_buf, slide_scan, backend);
            used_backend = ub;
            clip_to_display(&mut scan, encoding.encoded);
            if let Some(pp) = &post_cpu {
                pp.apply(&mut scan);
            }
            if space_differs {
                stages::scanning::convert_output(&mut scan, encoding.space.name, target_space.name, encoding.encoded);
            }
            copy_scan_interior_to_rgb16(&scan, &tile, work_w as usize, &mut out_rgb16);
        }
        let render_ms = t_ren.elapsed().as_secs_f64() * 1000.0;
        drop(source);
        drop(tiled_prep);

        let t_fin = Instant::now();
        let (rgb16, w, h) = crop_flip_rotate_rgb16(
            out_rgb16,
            work_w,
            work_h,
            finish.crop,
            finish.flip,
            finish.rotate_cw,
        );
        save_rgb16_with_options(&rgb16, w, h, output_path, metadata.as_ref(), &save_options)?;
        (w, h, render_ms, t_fin.elapsed().as_secs_f64() * 1000.0)
    } else if straightened {
        // Straightened crop: evaluate bilinear taps directly in f32/f64 per tile
        // into the cropped ImageBuf so we never hold an uncropped full-frame f32
        // AND stay bit-identical to whole-frame `Crop::apply` on `f32`.
        let crop = finish.crop.unwrap();
        let (cw, ch) = crop.straightened_size(work_w, work_h);
        let (iw, ih) = (work_w as f64, work_h as f64);
        let (cx, cy) = (iw / 2.0, ih / 2.0);
        let (origin_x, origin_y) = (crop.x * iw, crop.y * ih);
        let (sin, cos) = crop.angle.to_radians().sin_cos();
        let (max_x, max_y) = (work_w as usize - 1, work_h as usize - 1);
        let mut cropped_data = vec![from_f64(0.0); (cw as usize) * (ch as usize) * 3];

        for idx in 0..total_tiles {
            let tile = grid.tile(idx);
            let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
            let mut tpipe = pipeline
                .clone()
                .with_autoexposure_ev(ae_ev)
                .with_tile(work_w, work_h, tile.ox, tile.oy);
            if let Some(sums) = grain_usm_sums {
                tpipe = tpipe.with_grain_usm_sums(sums);
            }
            if let Some(prep) = &tiled_prep {
                prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
                tpipe = tpipe.with_prep_applied();
            }
            let (mut scan, ub) = render_tile_scan(&tpipe, tile_buf, slide_scan, backend);
            used_backend = ub;
            clip_to_display(&mut scan, encoding.encoded);
            if finish.crop_after_post {
                if let Some(pp) = &post_cpu {
                    pp.apply(&mut scan);
                }
            }
            let (ix0, iy0, ix1, iy1) = (tile.ix0 as usize, tile.iy0 as usize, tile.ix1 as usize, tile.iy1 as usize);
            let (ox, oy, tw) = (tile.ox as usize, tile.oy as usize, scan.width as usize);
            cropped_data
                .par_chunks_exact_mut(cw as usize * 3)
                .enumerate()
                .for_each(|(v, row)| {
                    for u in 0..cw as usize {
                        let (dx, dy) = (origin_x + u as f64 + 0.5 - cx, origin_y + v as f64 + 0.5 - cy);
                        let sx = (cx + dx * cos + dy * sin - 0.5).clamp(0.0, max_x as f64);
                        let sy = (cy - dx * sin + dy * cos - 0.5).clamp(0.0, max_y as f64);
                        let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                        if x0 < ix0 || x0 >= ix1 || y0 < iy0 || y0 >= iy1 {
                            continue;
                        }
                        let (x1, y1) = ((x0 + 1).min(max_x), (y0 + 1).min(max_y));
                        let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
                        let at = |gx: usize, gy: usize, c: usize| {
                            to_f64(scan.data[((gy - oy) * tw + (gx - ox)) * 3 + c])
                        };
                        for c in 0..3 {
                            let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                            let bot = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                            row[u * 3 + c] = from_f64(top * (1.0 - fy) + bot * fy);
                        }
                    }
                });
        }
        let render_ms = t_ren.elapsed().as_secs_f64() * 1000.0;
        drop(source);
        drop(tiled_prep);

        let t_fin = Instant::now();
        let mut image = ImageBuf::from_data(cw, ch, cropped_data);
        if !finish.crop_after_post {
            if let Some(pp) = &post_cpu {
                pp.apply(&mut image);
            }
        }
        let (w, h) = finish_and_save_f32(
            image,
            film_name,
            encoding,
            target_space,
            &finish,
            output_path,
            metadata.as_ref(),
            &save_options,
            data_dir,
        )?;
        (w, h, render_ms, t_fin.elapsed().as_secs_f64() * 1000.0)
    } else {
        // General f32 accumulator (used for `fit`, 16-bit with border, or
        // cross-space conversion with border / fit): render tiles in f32 into
        // the cropped frame directly so an axis-aligned crop keeps memory minimal.
        let (cx0, cy0, cw, ch) = match finish.crop {
            Some(c) => c.pixels(work_w, work_h),
            None => (0, 0, work_w, work_h),
        };
        let mut acc = vec![from_f32(0.0); (cw as usize) * (ch as usize) * 3];
        for idx in 0..total_tiles {
            let tile = grid.tile(idx);
            let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
            let mut tpipe = pipeline
                .clone()
                .with_autoexposure_ev(ae_ev)
                .with_tile(work_w, work_h, tile.ox, tile.oy);
            if let Some(sums) = grain_usm_sums {
                tpipe = tpipe.with_grain_usm_sums(sums);
            }
            if let Some(prep) = &tiled_prep {
                prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
                tpipe = tpipe.with_prep_applied();
            }
            let (mut scan, ub) = render_tile_scan(&tpipe, tile_buf, slide_scan, backend);
            used_backend = ub;
            clip_to_display(&mut scan, encoding.encoded);
            if let Some(pp) = &post_cpu {
                pp.apply(&mut scan);
            }
            // Intersect tile interior [ix0..ix1, iy0..iy1] with crop [cx0..cx0+cw, cy0..cy0+ch].
            let x_start = tile.ix0.max(cx0);
            let x_end = tile.ix1.min(cx0 + cw);
            let y_start = tile.iy0.max(cy0);
            let y_end = tile.iy1.min(cy0 + ch);
            if x_start < x_end && y_start < y_end {
                let copy_w = (x_end - x_start) as usize;
                let tw = scan.width as usize;
                for gy in y_start..y_end {
                    let sy = (gy - tile.oy) as usize;
                    let sx = (x_start - tile.ox) as usize;
                    let dy = (gy - cy0) as usize;
                    let dx = (x_start - cx0) as usize;
                    let src_i = (sy * tw + sx) * 3;
                    let dst_i = (dy * cw as usize + dx) * 3;
                    acc[dst_i..dst_i + copy_w * 3]
                        .copy_from_slice(&scan.data[src_i..src_i + copy_w * 3]);
                }
            }
        }
        let render_ms = t_ren.elapsed().as_secs_f64() * 1000.0;
        drop(source);
        drop(tiled_prep);

        let t_fin = Instant::now();
        let image = ImageBuf::from_data(cw, ch, acc);
        let (w, h) = finish_and_save_f32(
            image,
            film_name,
            encoding,
            target_space,
            &finish,
            output_path,
            metadata.as_ref(),
            &save_options,
            data_dir,
        )?;
        (w, h, render_ms, t_fin.elapsed().as_secs_f64() * 1000.0)
    };

    Ok(TiledExportReport {
        source_width: full_w,
        source_height: full_h,
        output_width: out_w,
        output_height: out_h,
        source_kind,
        backend: used_backend,
        total_tiles,
        tile_width: grid.padded_w,
        tile_height: grid.padded_h,
        halo: grid.halo,
        two_pass_usm,
        decode_ms,
        pass1_ms,
        render_ms,
        finish_ms,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

fn render_tile_scan(
    tpipe: &Pipeline,
    tile_buf: ImageBuf,
    slide_scan: bool,
    backend: &dyn ComputeBackend,
) -> (ImageBuf, String) {
    if !slide_scan && backend.is_gpu() && backend.is_healthy() {
        if let Some(scan) = tpipe.process_resident_borrowed(&tile_buf, backend) {
            return (scan, backend.name().to_owned());
        }
    }
    let (scan, ub) = tpipe.process_with_backend(tile_buf, &CpuBackend);
    (scan, ub.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn finish_and_save_f32(
    mut image: ImageBuf,
    film_name: &str,
    encoding: Encoding,
    target_space: &RgbColourspace,
    finish: &ExportFinish,
    output_path: &Path,
    metadata: Option<&ImageMetadata>,
    save_options: &SaveOptions,
    data_dir: &Path,
) -> Result<(u32, u32)> {
    if finish.flip || finish.rotate_cw.rem_euclid(4) != 0 {
        image = image.flipped_rotated_cw(finish.flip, finish.rotate_cw);
    }
    if let Some(style) = finish.border {
        let bordered = add_border(&image, style, &finish.border_options, film_name, encoding, data_dir)?;
        image = if finish.instagram {
            extend_for_instagram(&bordered, style, &finish.border_options, encoding)
        } else {
            bordered
        };
    }
    if let Some(fit) = finish.fit {
        if let Some(resized) = resize::fit(&image, fit, encoding) {
            image = resized;
        }
    }
    if target_space.name != encoding.space.name {
        stages::scanning::convert_output(&mut image, encoding.space.name, target_space.name, encoding.encoded);
    }
    let dims = (image.width, image.height);
    save_image_with_options(&image, output_path, metadata, save_options)?;
    Ok(dims)
}

fn run_grain_usm_pass1(
    source: &ExportSource,
    pipeline: &Pipeline,
    tiled_prep: Option<&TiledPrep>,
    ae_ev: f64,
    grid: &TileGrid,
    backend: &dyn ComputeBackend,
) -> Result<[f32; 6]> {
    let mut totals = [0u64; 6];
    for idx in 0..grid.total() {
        let tile = grid.tile(idx);
        let mut tile_buf = source.extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)?;
        let mut tpipe = pipeline
            .clone()
            .with_autoexposure_ev(ae_ev)
            .with_tile(grid.full_w, grid.full_h, tile.ox, tile.oy);
        if let Some(prep) = tiled_prep {
            prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(anyhow::Error::msg)?;
            tpipe = tpipe.with_prep_applied();
        }
        let tile_sums = if backend.is_gpu() && backend.is_healthy() {
            let output = ChainOutput::GrainUsmSums {
                crop_x: tile.src_dx,
                crop_y: tile.src_dy,
                crop_w: tile.copy_w,
                crop_h: tile.copy_h,
            };
            tpipe
                .submit_resident(&tile_buf, None, backend, output, Box::new(|| {}))
                .and_then(|pending| match tpipe.try_finish_submitted(pending, backend) {
                    Ok(ChainResult::GrainUsmSums(s)) => Some(s),
                    _ => None,
                })
        } else {
            None
        };
        let s = match tile_sums {
            Some(s) => s,
            None => tpipe.grain_usm_sums_cpu(tile_buf, (tile.src_dx, tile.src_dy, tile.copy_w, tile.copy_h)),
        };
        for c in 0..6 {
            totals[c] = totals[c].wrapping_add(s[c]);
        }
    }
    Ok(usm_totals_to_f32(totals))
}

pub fn copy_rgba_interior_to_rgb8(
    rgba: &[u8],
    tile_w: usize,
    tile: &TileSpec,
    full_w: usize,
    out_rgb: &mut [u8],
) {
    let copy_w = tile.copy_w as usize;
    let src_dx = tile.src_dx as usize;
    let src_dy = tile.src_dy as usize;
    for row in 0..tile.copy_h as usize {
        let sy = src_dy + row;
        let dy = tile.iy0 as usize + row;
        let src_start = (sy * tile_w + src_dx) * 4;
        let dst_start = (dy * full_w + tile.ix0 as usize) * 3;
        for (s, d) in rgba[src_start..src_start + copy_w * 4]
            .chunks_exact(4)
            .zip(out_rgb[dst_start..dst_start + copy_w * 3].chunks_exact_mut(3))
        {
            d[0] = s[0];
            d[1] = s[1];
            d[2] = s[2];
        }
    }
}

pub fn copy_scan_interior_to_rgb8(
    scan: &ImageBuf,
    tile: &TileSpec,
    full_w: usize,
    out_rgb: &mut [u8],
) {
    let tile_w = scan.width as usize;
    let copy_w = tile.copy_w as usize;
    let src_dx = tile.src_dx as usize;
    let src_dy = tile.src_dy as usize;
    for row in 0..tile.copy_h as usize {
        let sy = src_dy + row;
        let dy = tile.iy0 as usize + row;
        let src_start = (sy * tile_w + src_dx) * 3;
        let dst_start = (dy * full_w + tile.ix0 as usize) * 3;
        for (&s, d) in scan.data[src_start..src_start + copy_w * 3]
            .iter()
            .zip(&mut out_rgb[dst_start..dst_start + copy_w * 3])
        {
            *d = quantize_scalar_u8(s);
        }
    }
}

fn copy_scan_interior_to_rgb16(
    scan: &ImageBuf,
    tile: &TileSpec,
    full_w: usize,
    out_rgb16: &mut [u16],
) {
    let tile_w = scan.width as usize;
    let copy_w = tile.copy_w as usize;
    let src_dx = tile.src_dx as usize;
    let src_dy = tile.src_dy as usize;
    for row in 0..tile.copy_h as usize {
        let sy = src_dy + row;
        let dy = tile.iy0 as usize + row;
        let src_start = (sy * tile_w + src_dx) * 3;
        let dst_start = (dy * full_w + tile.ix0 as usize) * 3;
        for (&s, d) in scan.data[src_start..src_start + copy_w * 3]
            .iter()
            .zip(&mut out_rgb16[dst_start..dst_start + copy_w * 3])
        {
            *d = quantize_scalar_u16(s);
        }
    }
}
