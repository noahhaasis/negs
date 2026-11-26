//! Memory-bounded tiled full-resolution export for browsers (especially mobile
//! Safari, where WebContent is killed around 1.5–2.3 GiB and WASM memory never
//! shrinks).
//!
//! Delegates tile grid planning, compact Bayer/X-Trans/LinearRaw decoding,
//! streaming AE metering, two-pass global grain USM mass normalization, edge-
//! aligned zero-padding tiles, and EXIF+ICC JPEG encoding to the shared
//! [`spektrafilm_core::tiled_export`] module.

use std::{cell::RefCell, rc::Rc};

use serde::Deserialize;
use spektrafilm_core::{
    data_fs,
    params::RuntimeParams,
    pipeline::{Pipeline, clip_to_display},
    post::PostProcess,
    prep::TiledPrep,
    tiled_export::{
        COMPACT_GPU_TILE_PIXELS, COMPACT_RAW_MIN_PIXELS, DEFAULT_CPU_TILE_PIXELS,
        DEFAULT_MAX_TILE_HALO, ExportSource, TileGrid, apply_crop_flip_rotate_rgb8,
        copy_rgba_interior_to_rgb8, copy_scan_interior_to_rgb8, needs_grain_usm_pass,
        usm_totals_to_f32,
    },
};
use spektrafilm_gpu::{
    ChainOutput, ChainResult, ComputeBackend,
    cpu_backend::CpuBackend,
    wgpu_backend::WgpuBackend,
};
use spektrafilm_io::{
    InputColour,
    border::{BorderOptions, BorderStyle},
    crop::Crop,
    encoding::Encoding,
    metadata::{ImageMetadata, read_metadata_bytes},
    save::{SaveOptions, encode_jpeg_rgb8},
};
use wasm_bindgen::prelude::*;

use crate::{
    REDUCED_XTRANS_BILINEAR_ABOVE, calibrate, js_error, load_data_image, memory_bytes, missing,
    parse_params, parse_white_balance,
};

/// Diffusion FFT grid budget inside the export worker (matches mobile editing).
const EXPORT_DIFFUSION_GRID_BUDGET: u64 = 160 * 1024 * 1024;

#[derive(Deserialize)]
struct BorderSpec {
    style: String,
    options: BorderOptions,
    #[serde(default)]
    instagram: bool,
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

struct ExporterState {
    backend: Rc<dyn ComputeBackend>,
    gpu: Option<Rc<WgpuBackend>>,
    film: String,
    paper: String,
    params: Option<RuntimeParams>,
    pipeline: Option<Pipeline>,
    source: Option<ExportSource>,
    metadata: Option<ImageMetadata>,
    input_colour: Option<InputColour>,
    autoexposure_ev: f64,
    tiled_prep: Option<TiledPrep>,
    grid: Option<TileGrid>,
    two_pass_usm: bool,
    usm_totals: [u64; 6],
    grain_usm_sums: Option<[f32; 6]>,
    next_step: u32,
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
                metadata: None,
                input_colour: None,
                autoexposure_ev: 0.0,
                tiled_prep: None,
                grid: None,
                two_pass_usm: false,
                usm_totals: [0; 6],
                grain_usm_sums: None,
                next_step: 0,
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

    /// Decode `bytes` compactly, extract EXIF metadata, drop `bytes`
    /// immediately, compute exact streaming auto-exposure / Prep guided base,
    /// and plan the edge-aligned fixed-size tile grid.
    pub fn start(
        &self,
        bytes: Vec<u8>,
        white_balance: Option<String>,
        autoexposure_ev: Option<f64>,
    ) -> Result<ExportPlan, JsError> {
        let wb = parse_white_balance(white_balance.as_deref())?;
        let metadata = read_metadata_bytes(&bytes);
        let (source, colour) = ExportSource::open_bytes(
            &bytes,
            wb,
            REDUCED_XTRANS_BILINEAR_ABOVE,
            COMPACT_RAW_MIN_PIXELS,
        )
        .map_err(|raw| {
            let msg = format!("{raw:#}");
            match msg.split_once("DecoderFailed(\"") {
                Some((_, reason)) => JsError::new(reason.split('"').next().unwrap_or(&msg)),
                None => JsError::new(&msg),
            }
        })?;
        drop(bytes);
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

        let target_pixels = if state.backend.is_gpu() {
            COMPACT_GPU_TILE_PIXELS
        } else {
            DEFAULT_CPU_TILE_PIXELS
        };
        let grid = TileGrid::plan(full_w, full_h, &pipeline, target_pixels, DEFAULT_MAX_TILE_HALO);
        let two_pass_usm = needs_grain_usm_pass(&pipeline, &grid);
        let total_steps = if two_pass_usm { grid.total() * 2 } else { grid.total() };
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
        state.metadata = metadata;
        state.input_colour = Some(colour);
        state.autoexposure_ev = ae_ev;
        state.tiled_prep = tiled_prep;
        state.grid = Some(grid);
        state.two_pass_usm = two_pass_usm;
        state.usm_totals = [0; 6];
        state.grain_usm_sums = None;
        state.next_step = 0;

        Ok(ExportPlan {
            width: full_w,
            height: full_h,
            total_tiles: total_steps,
            tile_width,
            tile_height,
            halo,
            source_kind,
            memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
        })
    }

    /// Execute the next tile step (Pass 1 global grain USM sum when active,
    /// then Pass 2 full tile render into `output_rgb`).
    #[wasm_bindgen(js_name = renderNextTile)]
    pub fn render_next_tile(&self) -> js_sys::Promise {
        let state = self.state.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            render_next_tile(&state).await.map(JsValue::from).map_err(JsValue::from)
        })
    }

    /// Free source & GPU buffers, apply crop/rotation/border on RGB8, and
    /// encode the final JPEG with EXIF + ICC profile directly in Rust.
    #[wasm_bindgen(js_name = finishJpeg)]
    pub fn finish_jpeg(
        &self,
        crop_json: Option<String>,
        rotation: i32,
        border_json: Option<String>,
        quality: u8,
    ) -> Result<ExportedJpeg, JsError> {
        let (rgb, full_w, full_h, film, metadata, output_color_space, encoded) = {
            let mut state = self.state.borrow_mut();
            state.source = None;
            state.tiled_prep = None;
            state.pipeline = None;
            if let Some(gpu) = state.gpu.take() {
                gpu.destroy_device();
            }
            state.backend = Rc::new(CpuBackend);
            let grid = state.grid.take().ok_or_else(|| JsError::new("no export in progress"))?;
            let rgb = std::mem::take(&mut state.output_rgb);
            let metadata = state.metadata.take();
            let (cs, enc) = state
                .params
                .as_ref()
                .map(|p| (p.io.output_color_space.clone(), p.io.output_cctf_encoding))
                .unwrap_or_else(|| ("sRGB".to_owned(), true));
            (rgb, grid.full_w, grid.full_h, state.film.clone(), metadata, cs, enc)
        };

        let crop: Option<Crop> = match crop_json.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(s) => Some(serde_json::from_str(s).map_err(|e| JsError::new(&e.to_string()))?),
        };
        let (mut rgb, mut w, mut h) =
            apply_crop_flip_rotate_rgb8(rgb, full_w, full_h, crop, false, rotation);

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

        let save_options = SaveOptions {
            jpeg_quality: quality.clamp(1, 100),
            color_space: output_color_space,
            linear: !encoded,
            copy_metadata: metadata.is_some(),
            ..SaveOptions::default()
        };
        let jpeg = encode_jpeg_rgb8(&rgb, w, h, metadata.as_ref(), &save_options)
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

async fn render_next_tile(state: &RefCell<ExporterState>) -> Result<TileProgress, JsError> {
    let (
        tile_buf,
        pipeline,
        backend,
        encoded,
        is_pass1,
        tile,
        total_steps,
        grid_tiles,
    ) = {
        let state = state.borrow();
        let grid = state.grid.as_ref().ok_or_else(|| JsError::new("no export in progress"))?;
        let grid_tiles = grid.total();
        let total_steps = if state.two_pass_usm { grid_tiles * 2 } else { grid_tiles };
        if state.next_step >= total_steps {
            return Ok(TileProgress {
                done: total_steps,
                total: total_steps,
                memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
            });
        }
        let is_pass1 = state.two_pass_usm && state.next_step < grid_tiles;
        let tile_idx = if is_pass1 { state.next_step } else { state.next_step - if state.two_pass_usm { grid_tiles } else { 0 } };
        let tile = grid.tile(tile_idx);

        let source = state.source.as_ref().ok_or_else(|| JsError::new("missing export source"))?;
        let mut tile_buf = source
            .extract_rect(tile.ox, tile.oy, tile.padded_w, tile.padded_h)
            .map_err(js_error)?;
        let mut pipeline = state
            .pipeline
            .as_ref()
            .ok_or_else(|| JsError::new("missing export pipeline"))?
            .clone()
            .with_autoexposure_ev(state.autoexposure_ev)
            .with_tile(grid.full_w, grid.full_h, tile.ox, tile.oy);
        if !is_pass1 {
            if let Some(sums) = state.grain_usm_sums {
                pipeline = pipeline.with_grain_usm_sums(sums);
            }
        }
        if let Some(prep) = &state.tiled_prep {
            prep.apply_tile(&mut tile_buf, tile.ox, tile.oy).map_err(|e| JsError::new(&e))?;
            pipeline = pipeline.with_prep_applied();
        }
        let encoded = pipeline.params.io.output_cctf_encoding;
        (
            tile_buf,
            pipeline,
            state.backend.clone(),
            encoded,
            is_pass1,
            tile,
            total_steps,
            grid_tiles,
        )
    };

    if is_pass1 {
        let (mapped_tx, mapped_rx) = futures_channel::oneshot::channel();
        let on_mapped = Box::new(move || {
            let _ = mapped_tx.send(());
        });
        let output = ChainOutput::GrainUsmSums {
            crop_x: tile.src_dx,
            crop_y: tile.src_dy,
            crop_w: tile.copy_w,
            crop_h: tile.copy_h,
        };
        let submitted = if backend.is_gpu() {
            pipeline.submit_resident(&tile_buf, None, backend.as_ref(), output, on_mapped)
        } else {
            None
        };
        let sums = match submitted {
            Some(pending) => {
                mapped_rx.await.map_err(|_| JsError::new("GPU readback was cancelled"))?;
                match pipeline.finish_submitted(pending, backend.as_ref()) {
                    ChainResult::GrainUsmSums(s) => s,
                    _ => return Err(JsError::new("Unexpected GPU result in grain USM pass")),
                }
            }
            None => pipeline.grain_usm_sums_cpu(tile_buf, (tile.src_dx, tile.src_dy, tile.copy_w, tile.copy_h)),
        };
        let done = {
            let mut state = state.borrow_mut();
            for c in 0..6 {
                state.usm_totals[c] = state.usm_totals[c].wrapping_add(sums[c]);
            }
            state.next_step += 1;
            if state.next_step == grid_tiles {
                state.grain_usm_sums = Some(usm_totals_to_f32(state.usm_totals));
            }
            state.next_step
        };
        return Ok(TileProgress {
            done,
            total: total_steps,
            memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
        });
    }

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
            let (scan, _) = pipeline
                .try_process_with_backend(tile_buf, &CpuBackend)
                .map_err(|e| JsError::new(&e.to_string()))?;
            ChainResult::Scan(scan)
        }
    };

    let done = {
        let mut state = state.borrow_mut();
        let full_w = state.grid.as_ref().unwrap().full_w as usize;
        match result {
            ChainResult::Rgba8 { width, rgba, .. } => {
                copy_rgba_interior_to_rgb8(&rgba, width as usize, &tile, full_w, &mut state.output_rgb);
            }
            ChainResult::Scan(mut scan) => {
                clip_to_display(&mut scan, encoded);
                if !pipeline.params.post.is_identity() {
                    let encoding = Encoding::new(&pipeline.params.io.output_color_space, encoded);
                    PostProcess::new(&pipeline.params.post, encoding.encoded, encoding.space.name)
                        .apply(&mut scan);
                }
                copy_scan_interior_to_rgb8(&scan, &tile, full_w, &mut state.output_rgb);
            }
            ChainResult::Presented { .. } | ChainResult::GrainUsmSums(_) => {
                return Err(JsError::new("Unexpected GPU export tile result"));
            }
        }
        state.next_step += 1;
        state.next_step
    };

    Ok(TileProgress {
        done,
        total: total_steps,
        memory_mib: (memory_bytes() / (1024.0 * 1024.0)).round() as u32,
    })
}
