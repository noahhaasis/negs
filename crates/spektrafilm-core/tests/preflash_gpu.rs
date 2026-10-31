//! The GPU-resident chain applies the enlarger preflash as the CPU does:
//! `(raw × factor + preflash) × print_exposure × bw`. Without enlarger
//! diffusion the GPU folds `print_exposure × bw` into the print_spectral
//! normalization, so the preflash has to be scaled with it — a print
//! exposure ≠ 1 or scanner white correction otherwise shifts the whole
//! print (IMG_0316 + COOKED rendered ~6 ΔE00 brighter on the GPU).

use spektrafilm_core::{color_reference::ColorReference, params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{cpu_backend::CpuBackend, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::Path;

fn pipeline(params: RuntimeParams) -> Pipeline {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&data, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&data, "kodak_ultra_endura").unwrap(),
        params,
        &data,
    )
    .unwrap()
}

/// Colours from deep shadow to past white, so the preflash lifts both the
/// print's highlights and shadows.
fn image() -> ImageBuf {
    let (w, h) = (96u32, 64u32);
    let data = (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f64 / (w - 1) as f64, (i / w) as f64 / (h - 1) as f64);
            let level = 0.002 + 1.4 * x * x;
            [level * (0.3 + 0.7 * y), level * (1.0 - 0.6 * y), level * (0.2 + 0.8 * (1.0 - x) * y)]
        })
        .map(from_f64)
        .collect();
    ImageBuf::from_data(w, h, data)
}

#[test]
fn preflash_with_print_exposure_scale_matches_the_cpu() {
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping");
        return;
    };
    // (print_exposure, scanner white correction, enlarger diffusion)
    let cases = [(1.0, true, false), (0.8, false, false), (0.8, true, true)];
    for (print_exposure, white_correction, diffusion) in cases {
        let mut params = RuntimeParams::default();
        params.camera.auto_exposure = false;
        params.film_render.grain.active = false;
        params.film_render.glare.active = false;
        params.print_render.glare.active = false;
        params.enlarger.preflash_exposure = 0.03;
        params.enlarger.preflash_y_filter_shift = -6.0;
        params.enlarger.preflash_m_filter_shift = -12.0;
        params.enlarger.print_exposure = print_exposure;
        params.scanner.white_correction = white_correction;
        params.enlarger.diffusion_filter.active = diffusion;
        let case = format!("print_exposure {print_exposure} white_correction {white_correction} diffusion {diffusion}");
        let pipe = pipeline(params);
        assert!(pipe.preflash_raw().iter().all(|&v| v > 0.0), "{case}: preflash is off");
        let correction = ColorReference::compute(
            &pipe.film,
            &pipe.print,
            &pipe.params,
            pipe.print_illuminant_slice(),
            pipe.print_exposure_factor(),
            pipe.preflash_raw(),
        )
        .printing_exposure_correction;
        let scale = print_exposure as f64 * correction;
        assert!((scale - 1.0).abs() > 0.05, "{case}: exposure scale {scale} doesn't exercise the fold");

        let cpu = pipe.process(image(), &CpuBackend);
        let gpu_out = pipe.process(image(), &gpu);
        let max = cpu.data.iter().zip(&gpu_out.data).map(|(a, b)| (*a as f64 - *b as f64).abs()).fold(0.0, f64::max);
        eprintln!("{case}: exposure scale {scale:.4}, max |gpu - cpu| {max:.3e}");
        assert!(max < 1e-4, "{case}: GPU scan differs from CPU by {max}");
    }
}
