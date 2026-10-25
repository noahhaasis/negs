//! Python 28bf883 references for the blurs at the image borders: the FIR
//! (σ < 3) reflects with the edge sample repeated, the IIR (σ ≥ 3) starts from
//! the edge samples, and the diffusion filter pads with numpy `reflect`. The
//! fields carry bright pixels on two edges and in a corner, and some kernels
//! are wider than the field. Frozen by scripts/parity/freeze_blur_edges_reference.py.

use serde::Deserialize;
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::fft_conv::convolve2d_reflect;
use spektrafilm_math::gaussian::{exponential_filter_channel, gaussian_blur_channel};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f64};
use spektrafilm_model::diffusion::{apply_halation_um, apply_unsharp_mask};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Field {
    height: u32,
    width: u32,
    data: Vec<f64>,
}

#[derive(Deserialize)]
struct BlurCase {
    field: String,
    sigma: f32,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct ExponentialCase {
    field: String,
    decay: f32,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct UnsharpCase {
    sigma: f32,
    amount: f32,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct HalationParams {
    scatter_amount: f64,
    scatter_spatial_scale: f64,
    scatter_core_um: [f64; 3],
    scatter_tail_um: [f64; 3],
    scatter_tail_weight: [f64; 3],
    halation_amount: f64,
    halation_spatial_scale: f64,
    halation_strength: [f64; 3],
    halation_first_sigma_um: [f64; 3],
    halation_n_bounces: u32,
    halation_bounce_decay: f64,
    halation_renormalize: bool,
}

#[derive(Deserialize)]
struct HalationCase {
    pixel_size_um: f32,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct DirCase {
    size_pixel: f32,
    tail_pixel: f32,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct DiffusionConv {
    radius: usize,
    psf: Vec<Vec<f64>>,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct Fixture {
    fields: HashMap<String, Field>,
    rgb_field: Field,
    blur: Vec<BlurCase>,
    exponential: Vec<ExponentialCase>,
    unsharp: Vec<UnsharpCase>,
    halation_params: HalationParams,
    halation: Vec<HalationCase>,
    dir_tail_weight: f64,
    dir_diffusion: Vec<DirCase>,
    diffusion_conv: DiffusionConv,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/blur_edges_python_28bf883.json")).unwrap()
}

#[cfg(feature = "precision-f64")]
const TOLERANCE: f64 = 1e-6;
// The fields reach 8, and the IIR recursion accumulates f32 rounding.
#[cfg(not(feature = "precision-f64"))]
const TOLERANCE: f64 = 1e-4;

fn scalars(v: &[f64]) -> Vec<Scalar> {
    v.iter().map(|&x| from_f64(x)).collect()
}

fn assert_close(got: &[Scalar], expected: &[f64], what: &str) {
    assert_eq!(got.len(), expected.len(), "{what}: length");
    let (i, err) = got
        .iter()
        .zip(expected)
        .map(|(&g, &e)| (g as f64 - e).abs())
        .enumerate()
        .fold(
            (0, 0.0f64),
            |m, (i, e)| if e > m.1 || e.is_nan() { (i, e) } else { m },
        );
    eprintln!("{what}: max |Rust − Python| = {err:.1e}");
    assert!(
        err < TOLERANCE,
        "{what}: max error {err:.3e} at {i} ({} vs {})",
        got[i],
        expected[i]
    );
}

/// The interleaved RGB field as an `ImageBuf`.
fn rgb_image(field: &Field) -> ImageBuf {
    ImageBuf::from_data(field.width, field.height, scalars(&field.data))
}

#[test]
fn gaussian_blur_borders_match_python() {
    let fixture = fixture();
    assert_eq!(fixture.blur.len(), 44);
    for case in &fixture.blur {
        let field = &fixture.fields[&case.field];
        let got =
            gaussian_blur_channel(&scalars(&field.data), field.width, field.height, case.sigma);
        assert_close(
            &got,
            &case.expected,
            &format!("{} σ {}", case.field, case.sigma),
        );
    }
}

#[test]
fn exponential_filter_borders_match_python() {
    let fixture = fixture();
    assert_eq!(fixture.exponential.len(), 10);
    for case in &fixture.exponential {
        let field = &fixture.fields[&case.field];
        let got = exponential_filter_channel(
            &scalars(&field.data),
            field.width,
            field.height,
            case.decay,
        );
        assert_close(
            &got,
            &case.expected,
            &format!("{} λ {}", case.field, case.decay),
        );
    }
}

#[test]
fn unsharp_mask_borders_match_python() {
    let fixture = fixture();
    for case in &fixture.unsharp {
        let got = apply_unsharp_mask(
            rgb_image(&fixture.rgb_field),
            case.sigma,
            case.amount,
            &CpuBackend,
        );
        assert_close(
            &got.data,
            &case.expected,
            &format!("unsharp σ {}", case.sigma),
        );
    }
}

#[test]
fn halation_borders_match_python() {
    let fixture = fixture();
    let p = &fixture.halation_params;
    for case in &fixture.halation {
        let got = apply_halation_um(
            &rgb_image(&fixture.rgb_field),
            case.pixel_size_um,
            p.scatter_amount,
            p.scatter_spatial_scale,
            p.scatter_core_um,
            p.scatter_tail_um,
            p.scatter_tail_weight,
            p.halation_amount,
            p.halation_spatial_scale,
            p.halation_strength,
            p.halation_first_sigma_um,
            p.halation_n_bounces,
            p.halation_bounce_decay,
            p.halation_renormalize,
            &CpuBackend,
        );
        assert_close(
            &got.data,
            &case.expected,
            &format!("halation at {} µm", case.pixel_size_um),
        );
    }
}

/// The DIR inhibitor diffusion, `(1 − w)·G(σ) + w·Exp(λ)` per channel, as
/// `spektrafilm_model::couplers` composes it.
#[test]
fn dir_diffusion_borders_match_python() {
    let fixture = fixture();
    let image = rgb_image(&fixture.rgb_field);
    let (w, h) = (image.width, image.height);
    let weight = from_f64(fixture.dir_tail_weight);
    for case in &fixture.dir_diffusion {
        let mut out = ImageBuf::new(w, h);
        for c in 0..3 {
            let ch = image.extract_channel(c);
            let g = gaussian_blur_channel(&ch, w, h, case.size_pixel);
            let t = exponential_filter_channel(&ch, w, h, case.tail_pixel);
            let mixed: Vec<Scalar> = g
                .iter()
                .zip(&t)
                .map(|(&g, &t)| (from_f64(1.0) - weight) * g + weight * t)
                .collect();
            out.write_channel(c, &mixed);
        }
        assert_close(
            &out.data,
            &case.expected,
            &format!("DIR diffusion σ {}", case.size_pixel),
        );
    }
}

/// The diffusion filter's reflect-padded convolution, in f64 on both sides
/// (upstream runs it in f32; Negs keeps f64).
#[test]
fn diffusion_filter_borders_match_python() {
    let fixture = fixture();
    let conv = &fixture.diffusion_conv;
    let field = &fixture.rgb_field;
    let (h, w) = (field.height as usize, field.width as usize);
    let k = 2 * conv.radius + 1;
    for c in 0..3 {
        let chan: Vec<f64> = (0..h * w).map(|i| field.data[i * 3 + c]).collect();
        let got = convolve2d_reflect(&chan, h, w, &conv.psf[c], k);
        for (i, &got) in got.iter().enumerate() {
            let expected = conv.expected[i * 3 + c];
            assert!(
                (got - expected).abs() < 1e-9,
                "channel {c}, pixel {i}: {got} vs {expected}"
            );
        }
    }
}
