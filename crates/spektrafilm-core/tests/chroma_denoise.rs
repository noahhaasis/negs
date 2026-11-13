//! The scanner's colour noise reduction: the GPU-resident chain matches the
//! CPU reference, it takes the colour out of noise without touching each
//! pixel's luminance, and 0 leaves the render as it was.

use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{cpu_backend::CpuBackend, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::{from_f64, to_f64, Scalar}};
use std::path::Path;

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

/// Mid tones with per-channel noise, as colour grain gives, and a darker
/// blue block whose edge the filter must keep.
fn image() -> ImageBuf {
    let (w, h) = (160u32, 96u32);
    let mut seed = 11u64;
    let mut noise = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 40) as f64 / (1u64 << 24) as f64) - 0.5
    };
    let data = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let base = if x > 100 && y > 30 { [0.05, 0.08, 0.3] } else { [0.25, 0.18, 0.12] };
            base.map(|v| v * (0.6 * noise()).exp2())
        })
        .map(from_f64)
        .collect();
    ImageBuf::from_data(w, h, data)
}

fn params(amount: f32, scan_film: bool) -> RuntimeParams {
    let mut params = RuntimeParams::default();
    params.camera.auto_exposure = false;
    params.film_render.grain.active = false;
    params.film_render.glare.active = false;
    params.print_render.glare.active = false;
    params.io.scan_film = scan_film;
    params.io.output_cctf_encoding = false;
    // 12.5 µm pixels: the 75 µm radius is 6 px, a 2-px grid of radius 3.
    params.camera.film_format_mm = 2.0;
    params.scanner.chroma_noise_radius_um = 75.0;
    params.scanner.chroma_noise_reduction = amount;
    params
}

const SRGB_Y: [f64; 3] = [0.2126390059, 0.7151686788, 0.0721923153];

fn luminance(px: &[Scalar]) -> f64 {
    px.iter().zip(SRGB_Y).map(|(v, w)| to_f64(*v) * w).sum()
}

/// Standard deviation of log2(R / G) over the noisy mid-tone region.
fn chroma_spread(image: &ImageBuf) -> f64 {
    let values: Vec<f64> = (10..90)
        .flat_map(|y| (10..90).map(move |x| (x, y)))
        .map(|(x, y)| {
            let i = ((y * image.width + x) * 3) as usize;
            (image.data[i] as f64 / image.data[i + 1] as f64).log2()
        })
        .collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

#[test]
fn removes_the_colour_and_keeps_the_luminance() {
    for scan_film in [false, true] {
        let off = pipeline(params(0.0, scan_film)).process(image(), &CpuBackend);
        let on = pipeline(params(100.0, scan_film)).process(image(), &CpuBackend);
        // The unsharp mask runs after the filter, so luminance is compared
        // with it off.
        let mut p0 = params(0.0, scan_film);
        p0.scanner.unsharp_mask = [0.0, 0.0];
        let mut p1 = params(100.0, scan_film);
        p1.scanner.unsharp_mask = [0.0, 0.0];
        let (a, b) = (pipeline(p0).process(image(), &CpuBackend), pipeline(p1).process(image(), &CpuBackend));
        let worst = a
            .data
            .chunks_exact(3)
            .zip(b.data.chunks_exact(3))
            .map(|(a, b)| (luminance(a) - luminance(b)).abs() / luminance(a).max(1e-3))
            .fold(0.0, f64::max);
        assert!(worst < 1e-4, "scan_film {scan_film}: luminance moved by {worst}");
        let (s0, s1) = (chroma_spread(&off), chroma_spread(&on));
        eprintln!("scan_film {scan_film}: chroma spread {s0:.4} → {s1:.4}");
        assert!(s1 < 0.3 * s0, "scan_film {scan_film}: chroma spread {s0} → {s1}");
    }
}

#[test]
fn the_gpu_chain_matches_the_cpu() {
    let Some(gpu) = WgpuBackend::new() else {
        assert!(std::env::var_os("SPEKTRAFILM_REQUIRE_GPU").is_none(), "no GPU adapter, and SPEKTRAFILM_REQUIRE_GPU is set");
        eprintln!("no GPU adapter available; skipping");
        return;
    };
    for (amount, scan_film) in [(100.0, false), (40.0, true)] {
        let pipe = pipeline(params(amount, scan_film));
        let cpu = pipe.process(image(), &CpuBackend);
        let (gpu_out, used) = pipe.process_with_backend(image(), &gpu);
        assert!(used.starts_with("wgpu"), "rendered on {used}");
        let off = pipeline(params(0.0, scan_film)).process(image(), &CpuBackend);
        let effect = cpu.data.iter().zip(&off.data).map(|(a, b)| (*a as f64 - *b as f64).abs()).fold(0.0, f64::max);
        let max = cpu.data.iter().zip(&gpu_out.data).map(|(a, b)| (*a as f64 - *b as f64).abs()).fold(0.0, f64::max);
        eprintln!("amount {amount} scan_film {scan_film}: effect {effect:.3e}, max |gpu - cpu| {max:.3e}");
        assert!(effect > 1e-2, "the filter did nothing");
        assert!(max < 1e-4, "GPU differs from CPU by {max}");
    }
}

/// Grain statistics (σ of log2 luma per pixel and in 2 and 3 px block means,
/// σ of log2 chroma) and the colour the filter leaves at full strength, on
/// flat patches with real grain at 24 MP pixel sizes for 35 mm and half
/// frame. What `RANGE_SIGMA` and the default radius were chosen from:
/// `cargo test --release -p spektrafilm-core --test chroma_denoise -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure_grain() {
    use spektrafilm_gpu::chroma_denoise::{self, ChromaDenoiseParams};
    let (w, h) = (600u32, 300u32);
    let levels = [0.02, 0.18, 0.7];
    let data: Vec<_> = (0..w * h)
        .flat_map(|i| {
            let band = (i % w) as usize * 3 / w as usize;
            [levels[band] * 1.1, levels[band], levels[band] * 0.9]
        })
        .map(from_f64)
        .collect();
    let image = ImageBuf::from_data(w, h, data);
    let lcc = |px: &[f32]| {
        let f: Vec<f32> = px.iter().map(|v| (v.max(0.0) + chroma_denoise::LOG_OFFSET).log2()).collect();
        let l = 0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2];
        [l, f[0] - l, f[2] - l]
    };
    let stats = |img: &ImageBuf, band: u32, k: u32| -> [f64; 3] {
        let (x0, x1) = (band * w / 3 + 20, (band + 1) * w / 3 - 20);
        let mut vals = vec![];
        for by in (10..h - 10 - k).step_by(k as usize) {
            for bx in (x0..x1 - k).step_by(k as usize) {
                let mut s = [0f64; 3];
                for y in by..by + k {
                    for x in bx..bx + k {
                        let i = ((y * w + x) * 3) as usize;
                        let v = lcc(&img.data[i..i + 3].iter().map(|v| to_f64(*v) as f32).collect::<Vec<_>>());
                        for (c, s) in s.iter_mut().enumerate() { *s += v[c] as f64 / (k * k) as f64; }
                    }
                }
                vals.push(s);
            }
        }
        std::array::from_fn(|c| {
            let m = vals.iter().map(|v| v[c]).sum::<f64>() / vals.len() as f64;
            (vals.iter().map(|v| (v[c] - m).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
        })
    };
    for (film, name) in [("kodak_portra_400", "portra400"), ("kodak_ultramax_400", "ultramax400")] {
        for (label, long_mm) in [("35mm", 36.0), ("half", 24.0)] {
            let pix_um = long_mm * 1000.0 / 6000.0;
            let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
            let mut p = params(0.0, false);
            p.film_render.grain.active = true;
            p.scanner.unsharp_mask = [0.0, 0.0];
            p.camera.film_format_mm = pix_um * w as f32 / 1000.0;
            let pipe = Pipeline::new_with_spectral(
                profile::load_profile_by_name(&data, film).unwrap(),
                profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
                p,
                &data,
            )
            .unwrap();
            let out = pipe.process(image.clone(), &CpuBackend);
            for band in 0..3 {
                let s1 = stats(&out, band, 1);
                let s2 = stats(&out, band, 2);
                let s3 = stats(&out, band, 3);
                let mut line = format!(
                    "{name} {label} band {band}: σL {:.3}/{:.3}/{:.3} σcr {:.3}/{:.3} σcb {:.3}",
                    s1[0], s2[0], s3[0], s1[1], s2[1], s1[2]
                );
                for radius in [20.0f32, 30.0, 40.0, 50.0] {
                    let cp = ChromaDenoiseParams::new(100.0, radius / pix_um, [0.2126, 0.7152, 0.0722]).unwrap();
                    let mut f = out.clone();
                    chroma_denoise::apply(&mut f, &cp);
                    let s = stats(&f, band, 1);
                    line += &format!(" | {radius} µm: {:.0}% left", 100.0 * (s[1] + s[2]) / (s1[1] + s1[2]));
                }
                eprintln!("{line}");
            }
        }
    }
}
