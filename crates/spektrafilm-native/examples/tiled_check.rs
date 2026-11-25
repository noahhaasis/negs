//! Compare whole-frame vs shared tiled export natively (byte-identity & RSS).
use std::{path::PathBuf, time::Instant};

use spektrafilm_core::{
    params::RuntimeParams,
    pipeline::Pipeline,
    post::PostParams,
    profile,
    stages,
    tiled_export::{
        COMPACT_RAW_MIN_PIXELS, DEFAULT_GPU_TILE_PIXELS, DEFAULT_MAX_TILE_HALO, ExportSource,
        export_tiled_rgb8,
    },
};
use spektrafilm_gpu::{ComputeBackend, cpu_backend::CpuBackend};
use spektrafilm_io::{
    InputInterpretation, RawWhiteBalance,
    raw::{LensCorrection, RawDecodeOptions, RawDecoderPreference, decode_raw},
};

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

fn run_whole(
    data: &PathBuf,
    photo: &PathBuf,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
) -> (u32, u32, Vec<u8>) {
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
    let ae = if params.camera.auto_exposure {
        pipeline.autoexposure_ev(&metering)
    } else {
        0.0
    };
    eprintln!("[whole] ae_ev={ae:.9}");
    let t_pipe = Instant::now();
    let output = pipeline
        .process_resident_borrowed(&decoded.image, backend)
        .unwrap_or_else(|| pipeline.process_with_backend(decoded.image.clone(), backend).0);
    let t_ren = t_pipe.elapsed();
    let rgb: Vec<u8> = output
        .data
        .iter()
        .map(|&v| {
            let x = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            (x * 255.0).round_ties_even() as u8
        })
        .collect();
    let (w, h) = (output.width, output.height);
    eprintln!(
        "[whole] {}x{} decode={:.0}ms render={:.0}ms total={:.0}ms backend={}",
        w,
        h,
        t_dec.as_secs_f64() * 1000.0,
        t_ren.as_secs_f64() * 1000.0,
        t0.elapsed().as_secs_f64() * 1000.0,
        backend.name()
    );
    (w, h, rgb)
}

fn run_tiled(
    data: &PathBuf,
    photo: &PathBuf,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
) -> (u32, u32, Vec<u8>) {
    let t0 = Instant::now();
    let (source, colour, _) = ExportSource::open_path(
        photo,
        RawWhiteBalance::default(),
        LensCorrection::default(),
        &InputInterpretation::default(),
        COMPACT_RAW_MIN_PIXELS,
    )
    .unwrap();
    let t_dec = t0.elapsed();
    let mut params = params.clone();
    params.io.apply_input_colour(&colour);
    let film = profile::load_profile_by_name(data, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(data, "kodak_portra_endura").unwrap();
    let pipeline = Pipeline::new_with_spectral(film, paper, params, data).unwrap();
    let (w, h, rgb, report) = export_tiled_rgb8(
        &source,
        &pipeline,
        &PostParams::default(),
        backend,
        DEFAULT_GPU_TILE_PIXELS,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    eprintln!(
        "[tiled] {}x{} kind={} grid={} tiles ({}x{}, halo={}, two_pass={}) decode={:.0}ms pass1={:.0}ms render={:.0}ms total={:.0}ms backend={}",
        w,
        h,
        report.source_kind,
        report.total_tiles,
        report.tile_width,
        report.tile_height,
        report.halo,
        report.two_pass_usm,
        t_dec.as_secs_f64() * 1000.0,
        report.pass1_ms,
        report.render_ms,
        t0.elapsed().as_secs_f64() * 1000.0,
        report.backend,
    );
    (w, h, rgb)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: tiled_check <photo.dng> [--whole-only|--tiled-only] [--cpu] [--prep] [--diffusion] [--no-grain] [--no-grain-usm] [--no-halation] [--no-dir] [--no-glare] [--no-ae] [--assert-exact]"
        );
        std::process::exit(2);
    }
    let photo = PathBuf::from(&args[1]);
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let params = build_params(&args);
    let cpu_only = args.iter().any(|a| a == "--cpu");
    let whole_only = args.iter().any(|a| a == "--whole-only");
    let tiled_only = args.iter().any(|a| a == "--tiled-only");
    let assert_exact = args.iter().any(|a| a == "--assert-exact");

    let backend: Box<dyn ComputeBackend> = if cpu_only {
        Box::new(CpuBackend)
    } else {
        spektrafilm_gpu::select_backend()
    };

    if whole_only {
        let _ = run_whole(&data, &photo, &params, backend.as_ref());
        return;
    }
    if tiled_only {
        let _ = run_tiled(&data, &photo, &params, backend.as_ref());
        return;
    }

    let (w1, h1, a) = run_whole(&data, &photo, &params, backend.as_ref());
    let (w2, h2, b) = run_tiled(&data, &photo, &params, backend.as_ref());
    assert_eq!((w1, h1), (w2, h2));

    let n_px = (w1 as usize) * (h1 as usize);
    let mut diff_px = 0usize;
    let mut diff_gt1 = 0usize;
    let mut max_diff = 0u8;
    let mut sum_abs = 0u64;
    let mut worst_xy = (0usize, 0usize);
    let mut hist = [0usize; 6];
    for p in 0..n_px {
        let d0 = a[p * 3].abs_diff(b[p * 3]);
        let d1 = a[p * 3 + 1].abs_diff(b[p * 3 + 1]);
        let d2 = a[p * 3 + 2].abs_diff(b[p * 3 + 2]);
        let d = d0.max(d1).max(d2);
        sum_abs += d0 as u64 + d1 as u64 + d2 as u64;
        if d > 0 {
            diff_px += 1;
            if d > 1 {
                diff_gt1 += 1;
            }
            if d > max_diff {
                max_diff = d;
                worst_xy = (p % w1 as usize, p / w1 as usize);
            }
            hist[(d as usize).min(5)] += 1;
        }
    }
    eprintln!(
        "[diff] px={} diff_px={} ({:.3}%) >1={} max_u8_diff={} at ({},{}) mean_abs={:.6} hist[1..=5+]={:?}",
        n_px,
        diff_px,
        100.0 * diff_px as f64 / n_px as f64,
        diff_gt1,
        max_diff,
        worst_xy.0,
        worst_xy.1,
        sum_abs as f64 / (n_px * 3) as f64,
        &hist[1..],
    );
    if assert_exact {
        assert_eq!(max_diff, 0, "Expected byte-identical tiled export (max_u8_diff == 0)");
    }
}
