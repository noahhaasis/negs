//! The neutral ramp measures the film-and-print curve the zone readout
//! inverts: output L\* against scene EV, with the finishing edits included.
use spektrafilm_core::{
    params::RuntimeParams,
    pipeline::Pipeline,
    post::PostParams,
    profile,
    tone_curve::{self, ToneRamp, EV_MIN, EV_STEP, SAMPLES},
};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_io::encoding::Encoding;
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// The app's pipeline for `params`: a decoded file is linear sRGB.
fn pipeline(params: RuntimeParams) -> Pipeline {
    let dir = data();
    let print = if params.io.scan_film {
        "kodak_portra_400"
    } else {
        "kodak_portra_endura"
    };
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&dir, print).unwrap(),
        params,
        &dir,
    )
    .unwrap()
}

fn app_params() -> RuntimeParams {
    let mut params = RuntimeParams::default();
    params.camera.auto_exposure = false;
    params.io.input_color_space = "sRGB".into();
    params.io.input_cctf_decoding = false;
    params
}

fn measured(params: RuntimeParams, post: &PostParams, auto_ev: f64) -> ToneRamp {
    tone_curve::measure(&pipeline(params), post, auto_ev)
}

fn ev_of(index: usize) -> f64 {
    EV_MIN + index as f64 * EV_STEP
}

/// The curve's L\* at `ev`, interpolated as Swift inverts it.
fn lstar_at(ramp: &ToneRamp, ev: f64) -> f64 {
    let x = ((ev - EV_MIN) / EV_STEP).clamp(0.0, (SAMPLES - 1) as f64);
    let i = x.floor() as usize;
    let j = (i + 1).min(SAMPLES - 1);
    let t = x - i as f64;
    f64::from(ramp.lstar[i]) * (1.0 - t) + f64::from(ramp.lstar[j]) * t
}

fn lstar(rgb: [f64; 3]) -> f64 {
    let decode = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = 0.2126 * decode(rgb[0]) + 0.7152 * decode(rgb[1]) + 0.0722 * decode(rgb[2]);
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        y * 24389.0 / 27.0
    }
}

/// A uniform patch, wide enough for the spatial stages to work on.
fn uniform(value: f64, width: u32, height: u32) -> ImageBuf {
    let mut image = ImageBuf::new(width, height);
    let v = from_f64(value);
    for y in 0..height {
        for x in 0..width {
            image.set(x, y, [v, v, v]);
        }
    }
    image
}

#[test]
fn ramp_matches_measured_print() {
    let ramp = measured(app_params(), &PostParams::default(), 0.0);
    let mid = lstar_at(&ramp, 0.18f64.log2());
    assert!((mid - 49.2).abs() < 1.0, "18% grey: {mid}");
    // The print's white is about L* 92, not 100.
    assert!(lstar_at(&ramp, 6.0) > mid + 30.0);
}

/// The ramp develops a flat field: the whole pipeline on a uniform frame
/// must agree with the curve, which leaves out only the spatial effects.
#[test]
fn ramp_predicts_a_flat_field_through_the_whole_pipeline() {
    let params = app_params();
    let ramp = measured(params.clone(), &PostParams::default(), 0.0);
    let stops: [i32; 8] = [-3, -2, -1, 0, 1, 2, 3, 4];
    // Each patch is wide enough that the spatial stages are local to it.
    let (patch, height) = (16u32, 4u32);
    let mut image = ImageBuf::new(stops.len() as u32 * patch, height);
    for (index, stop) in stops.iter().enumerate() {
        let v = from_f64(0.18 * 2f64.powi(*stop));
        for y in 0..height {
            for x in 0..patch {
                image.set(index as u32 * patch + x, y, [v, v, v]);
            }
        }
    }
    let mut output = pipeline(params).process(image, &CpuBackend);
    spektrafilm_core::pipeline::clip_to_display(&mut output, true);
    for (index, stop) in stops.iter().enumerate() {
        let x = index as u32 * patch + patch / 2;
        let actual = lstar(output.get(x, height / 2).map(f64::from));
        let curve = lstar_at(&ramp, 0.18f64.log2() + *stop as f64);
        if actual >= 7.0 {
            assert!(
                (actual - curve).abs() < 1.0,
                "{stop:+} EV: pipeline {actual:.2}, ramp {curve:.2}"
            );
        }
    }
}

#[test]
fn ramp_includes_post_tone() {
    let post = PostParams {
        brightness: 50.0,
        ..Default::default()
    };
    let base = measured(app_params(), &PostParams::default(), 0.0);
    let lifted = measured(app_params(), &post, 0.0);
    let grey = 0.18f64.log2();
    assert!(lstar_at(&lifted, grey) > lstar_at(&base, grey) + 5.0);
    // The finishing edits are applied after the scan, as the preview applies
    // them: a pipeline that carries them agrees with `measure`.
    let mut params = app_params();
    params.post = post;
    let output = pipeline(params).process(uniform(0.18, 64, 4), &CpuBackend);
    let actual = lstar(output.get(32, 2).map(f64::from));
    assert!((actual - lstar_at(&lifted, grey)).abs() < 1.0, "{actual}");
}

#[test]
fn ramp_shifts_rigidly_with_placement() {
    let mut moved_params = app_params();
    moved_params.camera.placement_ev = 1.0;
    let base = measured(app_params(), &PostParams::default(), 0.0);
    let moved = measured(moved_params, &PostParams::default(), 0.0);
    let offset = (1.0 / EV_STEP).round() as usize;
    for i in 0..SAMPLES - offset {
        let (a, b) = (f64::from(moved.lstar[i]), f64::from(base.lstar[i + offset]));
        assert!((a - b).abs() < 0.5, "sample {i}: {a} vs {b}");
    }
}

#[test]
fn ramp_keeps_print_filter_shifts_and_preflash() {
    let base = measured(app_params(), &PostParams::default(), 0.0);
    let grey = 0.18f64.log2();
    let mut filtered_params = app_params();
    filtered_params.enlarger.m_filter_shift = 10.0;
    let filtered = measured(filtered_params, &PostParams::default(), 0.0);
    assert!((lstar_at(&filtered, grey) - lstar_at(&base, grey)).abs() > 1.0);
    let mut preflashed_params = app_params();
    preflashed_params.enlarger.preflash_exposure = 0.5;
    let preflashed = measured(preflashed_params, &PostParams::default(), 0.0);
    assert!((lstar_at(&preflashed, grey) - lstar_at(&base, grey)).abs() > 0.5);
}

#[test]
fn ramp_uses_the_frames_auto_exposure() {
    let mut params = app_params();
    params.camera.auto_exposure = true;
    let base = measured(params.clone(), &PostParams::default(), 0.0);
    let lifted = measured(params, &PostParams::default(), 1.0);
    let offset = (1.0 / EV_STEP).round() as usize;
    for i in 0..SAMPLES - offset {
        let (a, b) = (f64::from(lifted.lstar[i]), f64::from(base.lstar[i + offset]));
        assert!((a - b).abs() < 0.5, "sample {i}: {a} vs {b}");
    }
}

#[test]
fn negative_scan_ramp_falls() {
    let mut params = app_params();
    params.io.scan_film = true;
    let ramp = measured(params, &PostParams::default(), 0.0);
    assert!(
        ramp.lstar[SAMPLES - 1] < ramp.lstar[0],
        "a scan of the negative runs backwards"
    );
    // And its green channel falls too, which the EV scale relies on.
    assert!(ramp.green[SAMPLES - 1] < ramp.green[0]);
}

#[test]
fn the_ramp_is_encoded_like_the_output() {
    let mut params = app_params();
    params.io.output_color_space = "Display P3".into();
    let ramp = measured(params, &PostParams::default(), 0.0);
    // Encoded green stays in range, and is the encoding of the output's
    // luminance at that sample.
    assert!(ramp.green.iter().all(|v| (0.0..=1.0).contains(v)));
    let encoding = Encoding::new("Display P3", true);
    let top = f64::from(ramp.lstar[SAMPLES - 1]);
    let y = if top > 8.0 {
        ((top + 16.0) / 116.0).powi(3)
    } else {
        top / 903.296_296
    };
    let expect = encoding.space.cctf.encode(y);
    assert!(
        (f64::from(ramp.green[SAMPLES - 1]) - expect).abs() < 0.01,
        "{} vs {expect}",
        ramp.green[SAMPLES - 1]
    );
    assert_eq!(ev_of(SAMPLES - 1), 8.0);
}
