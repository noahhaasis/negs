//! Compare whole-frame vs tiled export natively (byte-identity & peak RSS).
use std::{path::PathBuf, time::Instant};

use spektrafilm_core::{
    params::RuntimeParams,
    pipeline::Pipeline,
    prep::TiledPrep,
    preview,
    profile,
    stages,
};
use spektrafilm_gpu::{
    ChainOutput, ChainResult, ComputeBackend,
    cpu_backend::CpuBackend,
};
use spektrafilm_io::{
    RawWhiteBalance,
    bounded::BayerSensor,
    encoding::Encoding,
    raw::{LensCorrection, RawDecodeOptions, RawDecoderPreference, decode_raw},
};
use spektrafilm_math::image::ImageBuf;

const GPU_TILE_PIXELS: u64 = 1_800_000;
const MAX_TILE_HALO: u32 = 256;

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

fn build_params(args: &[String]) -> RuntimeParams {
    let mut params = RuntimeParams::default();
    params.io.apply_input_colour(&spektrafilm_io::InputColour::raw("rawler"));
    params.settings.serial_grain = false;
    if args.iter().any(|a| a == "--prep") {
        params.prep.highlights = -45.0;
        params.prep.shadows = 35.0;
        params.prep.midtone_detail = 35.0;
        params.prep.exposure_ev = -0.3;
    }
    if args.iter().any(|a| a == "--diffusion") {
        params.camera.diffusion_filter.active = true;
    }
    if args.iter().any(|a| a == "--no-grain") {
        params.film_render.grain.active = false;
    }
    if args.iter().any(|a| a == "--no-grain-usm") {
        params.film_render.grain.mult_usm_amount = 0.0;
    }
    if args.iter().any(|a| a == "--no-halation") {
        params.film_render.halation.active = false;
    }
    if args.iter().any(|a| a == "--no-dir") {
        params.film_render.dir_couplers.active = false;
    }
    if args.iter().any(|a| a == "--no-glare") {
        params.film_render.glare.active = false;
        params.print_render.glare.active = false;
    }
    if args.iter().any(|a| a == "--no-ae") {
        params.camera.auto_exposure = false;
    }
    params
}

fn run_whole(data: &PathBuf, photo: &PathBuf, params: &RuntimeParams, backend: &dyn ComputeBackend) -> (u32, u32, Vec<u8>) {
    let t0 = Instant::now();
    let decoded = decode_raw(
        photo,
        RawDecodeOptions {
            white_balance: RawWhiteBalance::default(),
            apply_orientation: true,
            lens_correction: LensCorrection::default(),
            max_long_side: 0,
        },
        RawDecoderPreference::Rawler,
    )
    .unwrap();
    let t_dec = t0.elapsed();
    let metering = stages::filming::metering_preview(&decoded.image);
    let film = profile::load_profile_by_name(data, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(data, "kodak_portra_endura").unwrap();
    let pipeline = Pipeline::new_with_spectral(film, paper, params.clone(), data)
        .unwrap()
        .with_metering_preview(metering.clone());
    let ae = if params.camera.auto_exposure { pipeline.autoexposure_ev(&metering) } else { 0.0 };
    eprintln!("[whole] ae_ev={ae:.9}");
    let t_pipe = Instant::now();
    let output = pipeline
        .process_resident_borrowed(&decoded.image, backend)
        .unwrap_or_else(|| pipeline.process_with_backend(decoded.image.clone(), backend).0);
    let t_ren = t_pipe.elapsed();
    let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
    let (w, h, rgba) = preview::display_rgba(&output, usize::MAX, encoding);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    eprintln!(
        "[whole] {}x{} decode={:.0}ms render={:.0}ms total={:.0}ms backend={}",
        w, h, t_dec.as_secs_f64() * 1000.0, t_ren.as_secs_f64() * 1000.0, t0.elapsed().as_secs_f64() * 1000.0, backend.name()
    );
    (w as u32, h as u32, rgb)
}

fn run_tiled(data: &PathBuf, photo: &PathBuf, params: &RuntimeParams, backend: &dyn ComputeBackend, use_gpu_rgba8: bool) -> (u32, u32, Vec<u8>) {
    let t0 = Instant::now();
    let bytes = std::fs::read(photo).unwrap();
    let sensor = BayerSensor::try_open(&bytes, RawWhiteBalance::default(), 12_000_000)
        .unwrap()
        .expect("expected BayerSensor support");
    drop(bytes);
    let (full_w, full_h) = sensor.full_size();
    let t_dec = t0.elapsed();

    let film = profile::load_profile_by_name(data, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(data, "kodak_portra_endura").unwrap();
    let pipeline = Pipeline::new_with_spectral(film, paper, params.clone(), data).unwrap();

    let ae_ev = if params.camera.auto_exposure {
        let m = sensor.small_preview(256).unwrap();
        let ev = pipeline.autoexposure_ev(&m);
        eprintln!("[tiled] ae_ev={ev:.9}");
        ev
    } else {
        0.0
    };
    let need_tiled_prep = !params.prep.is_identity() && !params.prep.is_exposure_only();
    let tiled_prep = if need_tiled_prep {
        Some(
            TiledPrep::build(full_w, full_h, params, ae_ev, |y0, h| {
                sensor.demosaic_upright_rect(0, y0, full_w, h)
            })
            .unwrap(),
        )
    } else {
        None
    };

    let halo = required_halo(&pipeline, full_w, full_h);
    let side = (GPU_TILE_PIXELS as f64).sqrt().floor() as u32;
    let raw_pw = side.min(full_w).max(1);
    let raw_ph = ((GPU_TILE_PIXELS / raw_pw as u64) as u32).min(full_h).max(1);
    let step_w = raw_pw.saturating_sub(2 * halo).max(256).min(full_w);
    let step_h = raw_ph.saturating_sub(2 * halo).max(256).min(full_h);
    let padded_w = (step_w + 2 * halo).min(full_w);
    let padded_h = (step_h + 2 * halo).min(full_h);
    let cols = full_w.div_ceil(step_w);
    let rows = full_h.div_ceil(step_h);
    let total_tiles = cols * rows;

    let mut out_rgb = vec![0u8; (full_w as usize) * (full_h as usize) * 3];
    let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
    let t_ren_start = Instant::now();

    for idx in 0..total_tiles {
        let tx = idx % cols;
        let ty = idx / cols;
        let ix0 = tx * step_w;
        let iy0 = ty * step_h;
        let ix1 = (ix0 + step_w).min(full_w);
        let iy1 = (iy0 + step_h).min(full_h);
        let ox = ix0.saturating_sub(halo);
        let oy = iy0.saturating_sub(halo);
        let src_dx = (ix0 - ox) as usize;
        let src_dy = (iy0 - oy) as usize;
        let copy_w = (ix1 - ix0) as usize;
        let copy_h = (iy1 - iy0) as usize;

        let mut tile_buf: ImageBuf = sensor.demosaic_upright_rect(ox, oy, padded_w, padded_h).unwrap();
        let mut tpipe = pipeline
            .clone()
            .with_autoexposure_ev(ae_ev)
            .with_tile(full_w, full_h, ox, oy);
        if let Some(prep) = &tiled_prep {
            prep.apply_tile(&mut tile_buf, ox, oy).unwrap();
            tpipe = tpipe.with_prep_applied();
        }

        let (tw, rgba) = if use_gpu_rgba8 && backend.is_gpu() && tpipe.display_on_gpu(backend) {
            let output = ChainOutput::Rgba8 { encoded: params.io.output_cctf_encoding, post: tpipe.display_post().map(std::sync::Arc::new) };
            let pending = tpipe.submit_resident(&tile_buf, None, backend, output, Box::new(|| {})).expect("submit_resident");
            match tpipe.finish_submitted(pending, backend) {
                ChainResult::Rgba8 { width, rgba, .. } => (width as usize, rgba),
                _ => unreachable!(),
            }
        } else {
            let scan = tpipe
                .process_resident_borrowed(&tile_buf, backend)
                .unwrap_or_else(|| tpipe.process_with_backend(tile_buf, &CpuBackend).0);
            let (width, _, rgba) = preview::display_rgba(&scan, usize::MAX, encoding);
            (width, rgba)
        };

        for row in 0..copy_h {
            let sy = src_dy + row;
            let dy = iy0 as usize + row;
            let src_start = (sy * tw + src_dx) * 4;
            let dst_start = (dy * full_w as usize + ix0 as usize) * 3;
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
    eprintln!(
        "[tiled] {}x{} tiles={}x{}={} padded={}x{} halo={} decode={:.0}ms render={:.0}ms total={:.0}ms gpu_rgba8={}",
        full_w, full_h, cols, rows, total_tiles, padded_w, padded_h, halo,
        t_dec.as_secs_f64() * 1000.0, t_ren_start.elapsed().as_secs_f64() * 1000.0,
        t0.elapsed().as_secs_f64() * 1000.0, use_gpu_rgba8
    );
    (full_w, full_h, out_rgb)
}

fn compare(a: &[u8], b: &[u8], w: u32, h: u32, label: &str) {
    assert_eq!(a.len(), b.len());
    let total_px = (w as usize) * (h as usize);
    let mut diff_px = 0usize;
    let mut diff_gt1_px = 0usize;
    let mut diff_gt2_px = 0usize;
    let mut max_diff = 0u8;
    let mut sum_diff = 0u64;
    let mut worst_pos = (0usize, 0usize);
    for idx in 0..total_px {
        let i = idx * 3;
        let dr = a[i].abs_diff(b[i]);
        let dg = a[i + 1].abs_diff(b[i + 1]);
        let db = a[i + 2].abs_diff(b[i + 2]);
        let m = dr.max(dg).max(db);
        if m > 0 {
            diff_px += 1;
            if m > 1 { diff_gt1_px += 1; }
            if m > 2 { diff_gt2_px += 1; }
            if m > max_diff {
                max_diff = m;
                worst_pos = (idx % w as usize, idx / w as usize);
            }
            sum_diff += dr as u64 + dg as u64 + db as u64;
        }
    }
    let pct = 100.0 * diff_px as f64 / total_px as f64;
    let pct_gt1 = 100.0 * diff_gt1_px as f64 / total_px as f64;
    let mean = sum_diff as f64 / (total_px as f64 * 3.0);
    println!(
        "[{label}] total_px={total_px} diff_px={diff_px} ({pct:.3}%) >1={diff_gt1_px} ({pct_gt1:.4}%) >2={diff_gt2_px} max_u8_diff={max_diff} at {worst_pos:?} mean_abs_diff={mean:.6}"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("compare");
    let data = PathBuf::from(&args[2]);
    let photo = PathBuf::from(&args[3]);
    let params = build_params(&args);
    let backend = spektrafilm_gpu::select_backend();

    match mode {
        "whole" => {
            let (w, h, rgb) = run_whole(&data, &photo, &params, backend.as_ref());
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
                .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
                .unwrap();
            println!("whole jpeg_bytes={}", jpeg.len());
        }
        "tiled" => {
            let (w, h, rgb) = run_tiled(&data, &photo, &params, backend.as_ref(), true);
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
                .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
                .unwrap();
            println!("tiled jpeg_bytes={}", jpeg.len());
        }
        "compare" => {
            let (w, h, whole_rgb) = run_whole(&data, &photo, &params, backend.as_ref());
            let (_, _, tiled_gpu8) = run_tiled(&data, &photo, &params, backend.as_ref(), true);
            compare(&whole_rgb, &tiled_gpu8, w, h, "whole vs tiled(GPU rgba8)");
        }
        "ablate" => {
            for (name, extra) in [
                ("default", vec![]),
                ("no-grain", vec!["--no-grain"]),
                ("no-grain-usm", vec!["--no-grain-usm"]),
                ("no-halation", vec!["--no-halation"]),
                ("no-dir", vec!["--no-dir"]),
                ("no-glare", vec!["--no-glare"]),
                ("no-ae", vec!["--no-ae"]),
                ("no-grain+no-halation+no-dir+no-glare", vec!["--no-grain", "--no-halation", "--no-dir", "--no-glare"]),
                ("no-grain+no-halation+no-dir+no-glare+no-ae", vec!["--no-grain", "--no-halation", "--no-dir", "--no-glare", "--no-ae"]),
            ] {
                let extra_s: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
                let p = build_params(&extra_s);
                let (w, h, whole_rgb) = run_whole(&data, &photo, &p, backend.as_ref());
                let (_, _, tiled_rgb) = run_tiled(&data, &photo, &p, backend.as_ref(), true);
                compare(&whole_rgb, &tiled_rgb, w, h, name);
            }
        }
        _ => panic!("unknown mode {mode}"),
    }
}
