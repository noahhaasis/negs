//! The GPU display pass applies the Post adjustments as the CPU does: its
//! display bytes (`ChainOutput::Rgba8` with `Pipeline::display_post`) match
//! `PostProcess` on the same GPU scan, then `preview::display_rgba`, within
//! one 8-bit step.

use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, post::PostParams, preview, profile};
use spektrafilm_gpu::{ChainOutput, ChainResult, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::Path;
use std::sync::Arc;

fn pipeline(params: RuntimeParams) -> Pipeline {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&data, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
        params,
        &data,
    )
    .unwrap()
}

/// Colours from deep shadow to past white, so clipping and the gamut stops
/// are exercised.
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

fn post(brightness: f32, contrast: f32, whites: f32, blacks: f32, saturation: f32) -> PostParams {
    PostParams { brightness, contrast, whites, blacks, saturation }
}

#[test]
fn display_post_matches_the_cpu() {
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping");
        return;
    };
    let image = image();
    let cases = [
        post(0.0, 0.0, 0.0, 0.0, 0.0),
        post(40.0, 0.0, 0.0, 0.0, 0.0),
        post(-60.0, 35.0, 0.0, 0.0, 0.0),
        post(0.0, -100.0, 20.0, -30.0, 0.0),
        post(0.0, 0.0, 100.0, 100.0, 0.0),
        post(0.0, 0.0, -100.0, -100.0, 0.0),
        post(0.0, 0.0, 0.0, 0.0, 100.0),
        post(0.0, 0.0, 0.0, 0.0, -100.0),
        post(100.0, 100.0, 100.0, 0.0, 60.0),
    ];
    for encoded in [true, false] {
        for case in cases {
            let mut params = RuntimeParams::default();
            params.camera.auto_exposure = false;
            params.film_render.grain.active = false;
            params.io.output_cctf_encoding = encoded;
            params.post = case;
            let pipe = pipeline(params);
            assert!(pipe.display_on_gpu(&gpu), "{case:?}");
            let run = |output: ChainOutput| {
                let pending = pipe.submit_resident(&image, None, &gpu, output, Box::new(|| {})).expect("resident chain");
                pipe.finish_submitted(pending, &gpu)
            };
            let ChainResult::Scan(scan) = run(ChainOutput::Scan) else { panic!("scan") };
            let (_, _, want) = preview::display_rgba(&scan, usize::MAX, encoded);
            let post = pipe.display_post().map(Arc::new);
            assert_eq!(post.is_none(), case == PostParams::default());
            let ChainResult::Rgba8 { rgba, .. } = run(ChainOutput::Rgba8 { encoded, post }) else { panic!("rgba8") };
            assert_eq!(rgba.len(), want.len());
            let off: Vec<_> = rgba.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).collect();
            let max = off.iter().copied().max().unwrap();
            let count = off.iter().filter(|d| **d > 0).count();
            eprintln!("encoded {encoded} {case:?}: {count} of {} bytes off by {max}", off.len());
            assert!(max <= 1, "encoded {encoded} {case:?}: off by up to {max}");
            // f32 rounding on either side lands a few values on the other
            // side of a half step.
            assert!(count * 200 <= off.len(), "encoded {encoded} {case:?}: {count} of {} bytes differ", off.len());
        }
    }
}
