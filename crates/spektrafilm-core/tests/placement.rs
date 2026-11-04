//! `camera.placement_ev` moves the scene along the tone curve: unlike the
//! camera's exposure compensation, the print exposure doesn't cancel it.
use spektrafilm_core::{lut_export::per_pixel_params, params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{ComputeBackend, cpu_backend::CpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::{from_f64, to_f64}};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn pipeline(params: RuntimeParams) -> Pipeline {
    let dir = data();
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
        params,
        &dir,
    )
    .unwrap()
}

/// Neutral patches one stop apart in linear sRGB; index `GREY` is 18%.
const STOPS: [i32; 7] = [-3, -2, -1, 0, 1, 2, 3];
const GREY: usize = 3;

fn chart() -> ImageBuf {
    let mut image = ImageBuf::new(STOPS.len() as u32, 1);
    for (x, stop) in STOPS.iter().enumerate() {
        let v = from_f64(0.18 * 2f64.powi(*stop));
        image.set(x as u32, 0, [v, v, v]);
    }
    image
}

fn lstar(rgb: [f64; 3]) -> f64 {
    let decode = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let y = 0.2126 * decode(rgb[0]) + 0.7152 * decode(rgb[1]) + 0.0722 * decode(rgb[2]);
    if y > 216.0 / 24389.0 { 116.0 * y.cbrt() - 16.0 } else { y * 24389.0 / 27.0 }
}

fn develop(placement: f32, backend: &dyn ComputeBackend) -> Vec<f64> {
    let mut params = per_pixel_params(RuntimeParams::default());
    assert!(params.enlarger.print_exposure_compensation);
    params.camera.placement_ev = placement;
    let output = pipeline(params).process_with_backend(chart(), backend).0;
    (0..STOPS.len() as u32)
        .map(|x| lstar(output.get(x, 0).map(to_f64)))
        .collect()
}

#[test]
fn placement_moves_midgrey_with_print_compensation_on() {
    let base = develop(0.0, &CpuBackend);
    let moved = develop(1.0, &CpuBackend);
    // One stop up the curve: about 68, where exposure compensation stays near 49.
    for i in GREY - 1..=GREY + 1 {
        assert!((moved[i] - base[i + 1]).abs() < 1.0, "patch {i}: {moved:?} vs {base:?}");
    }
    assert!(moved[GREY] > base[GREY] + 10.0, "{moved:?} vs {base:?}");
}

#[test]
fn exposure_compensation_is_still_cancelled_by_the_print() {
    let base = develop(0.0, &CpuBackend);
    let mut params = per_pixel_params(RuntimeParams::default());
    params.camera.exposure_compensation_ev = 1.0;
    let output = pipeline(params).process(chart(), &CpuBackend);
    let compensated = lstar(output.get(GREY as u32, 0).map(to_f64));
    assert!((compensated - base[GREY]).abs() < 2.0, "{compensated} vs {}", base[GREY]);
}

#[test]
fn placement_is_the_same_on_cpu_and_gpu_paths() {
    let backend = spektrafilm_gpu::select_backend();
    if !backend.name().starts_with("wgpu") {
        return; // no adapter
    }
    let (cpu, gpu) = (develop(1.0, &CpuBackend), develop(1.0, backend.as_ref()));
    for (c, g) in cpu.iter().zip(&gpu) {
        assert!((c - g).abs() < 1.0, "{cpu:?} vs {gpu:?}");
    }
}

#[test]
fn zero_placement_is_byte_identical() {
    let omitted: RuntimeParams = serde_json::from_str(r#"{"camera": {"auto_exposure": false}}"#).unwrap();
    assert_eq!(omitted.camera.placement_ev, 0.0);
    let explicit: RuntimeParams =
        serde_json::from_str(r#"{"camera": {"auto_exposure": false, "placement_ev": 0.0}}"#).unwrap();
    let render = |params: RuntimeParams| pipeline(per_pixel_params(params)).process(chart(), &CpuBackend);
    assert_eq!(render(omitted).data, render(explicit).data);
}
