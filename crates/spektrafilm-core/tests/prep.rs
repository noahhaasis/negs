//! Prep's scene-space contract and its placement on every rendering path.
use spektrafilm_core::{
    params::RuntimeParams,
    pipeline::Pipeline,
    prep::{self, PrepParams},
    profile,
};
use spektrafilm_gpu::{
    ChainOutput, ChainResult, cpu_backend::CpuBackend, wgpu_backend::WgpuBackend,
};
use spektrafilm_math::{
    colorspace::{RgbColourspace, SRGB, icc_rgb_to_rgb, mat3_vec_f64},
    image::ImageBuf,
    precision::{from_f32, from_f64, to_f64},
};
use std::path::Path;

fn image() -> ImageBuf {
    ImageBuf::from_data(
        48,
        32,
        (0..48 * 32)
            .flat_map(|i| {
                let x = (i % 48) as f64 / 47.;
                let y = (i / 48) as f64 / 31.;
                [0.04 + x * 1.1, 0.03 + x * y, 0.06 + y * 0.6]
            })
            .map(from_f64)
            .collect(),
    )
}
fn controls() -> PrepParams {
    PrepParams {
        exposure_ev: 0.65,
        highlights: -130.,
        shadows: 32.,
        contrast: 18.,
        lift: -3.,
        gain: 6.,
        color_boost: 15.,
        saturation: -12.,
        midtone_detail: 27.,
    }
}
fn compare(a: &ImageBuf, b: &ImageBuf, tolerance: f64) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let max = a
        .data
        .iter()
        .zip(&b.data)
        .map(|(a, b)| (to_f64(*a) - to_f64(*b)).abs())
        .fold(0f64, f64::max);
    assert!(max <= tolerance, "maximum error {max} exceeds {tolerance}");
    assert!(a.data.iter().all(|v| v.is_finite()));
}
#[test]
fn old_settings_and_neutral_prep_are_bit_exact() {
    let old: RuntimeParams = serde_json::from_str("{}").unwrap();
    assert!(old.prep.is_identity());
    let partial: PrepParams = serde_json::from_str(r#"{"shadows":20}"#).unwrap();
    assert_eq!(partial.shadows, 20.);
    assert_eq!(partial.gain, 0.);
    assert_eq!(partial.exposure_ev, 0.);
    let mut frame = ImageBuf::from_data(
        2,
        1,
        [-0., -0.25, 100., 0.18, 0., 9.].map(from_f64).to_vec(),
    );
    let bits = frame.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    let ptr = frame.data.as_ptr();
    prep::apply_linear(&mut frame, &old.prep, "ProPhoto RGB").unwrap();
    assert_eq!(ptr, frame.data.as_ptr());
    assert_eq!(
        bits,
        frame.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}
#[test]
fn controls_reject_invalid_settings_and_keep_signed_hdr() {
    for p in [
        PrepParams {
            exposure_ev: 3.01,
            ..Default::default()
        },
        PrepParams {
            exposure_ev: f32::INFINITY,
            ..Default::default()
        },
        PrepParams {
            highlights: -180.1,
            ..Default::default()
        },
        PrepParams {
            gain: 101.,
            ..Default::default()
        },
        PrepParams {
            shadows: f32::NAN,
            ..Default::default()
        },
    ] {
        assert!(p.validate().is_err());
    }
    let mut frame = ImageBuf::from_data(
        4,
        1,
        [-0.2, -0.2, -0.2, 0., 0., 0., 8., 8., 8., -0.02, 0.18, 4.]
            .map(from_f64)
            .to_vec(),
    );
    prep::apply_linear(
        &mut frame,
        &PrepParams {
            highlights: -180.,
            ..Default::default()
        },
        "sRGB",
    )
    .unwrap();
    assert!(frame.data.iter().all(|v| v.is_finite()));
    assert!(frame.data[0] < from_f64(0.));
    assert!(frame.data[6] > from_f64(1.));
}
#[test]
fn placement_only_is_a_signed_hdr_gain_without_a_colour_roundtrip() {
    let source = ImageBuf::from_data(
        2,
        1,
        [-0.0, -0.25, 100.0, 0.18, 0.0, 9.0].map(from_f64).to_vec(),
    );
    for name in ["sRGB", "ProPhoto RGB", "DaVinci Wide Gamut"] {
        let mut frame = source.clone();
        let pointer = frame.data.as_ptr();
        prep::apply_linear(
            &mut frame,
            &PrepParams {
                exposure_ev: 1.0,
                ..Default::default()
            },
            name,
        )
        .unwrap();
        assert_eq!(frame.data.as_ptr(), pointer);
        for (actual, original) in frame.data.iter().zip(&source.data) {
            assert_eq!(actual.to_bits(), (*original * from_f64(2.)).to_bits());
        }
    }
}
#[test]
fn placement_precedes_recovered_controls() {
    let source = image();
    let p = controls();
    let mut actual = source.clone();
    prep::apply_linear(&mut actual, &p, "ProPhoto RGB").unwrap();
    let mut expected = source;
    let gain = from_f64((p.exposure_ev as f64).exp2());
    for v in &mut expected.data {
        *v *= gain;
    }
    prep::apply_linear(
        &mut expected,
        &PrepParams {
            exposure_ev: 0.,
            ..p
        },
        "ProPhoto RGB",
    )
    .unwrap();
    compare(&actual, &expected, 0.);
}
#[test]
fn preparation_agrees_across_d50_d65_and_wide_gamut_inputs() {
    let source = image();
    let mut reference = source.clone();
    prep::apply_linear(&mut reference, &controls(), "sRGB").unwrap();
    for name in [
        "ProPhoto RGB",
        "Display P3",
        "ACES2065-1",
        "DaVinci Wide Gamut",
    ] {
        let space = RgbColourspace::find(name).unwrap();
        let into = icc_rgb_to_rgb(&SRGB, space);
        let back = icc_rgb_to_rgb(space, &SRGB);
        let mut converted = source.clone();
        for p in converted.pixels_mut() {
            p.copy_from_slice(
                &mat3_vec_f64(&into, [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]).map(from_f64),
            );
        }
        prep::apply_linear(&mut converted, &controls(), name).unwrap();
        for p in converted.pixels_mut() {
            p.copy_from_slice(
                &mat3_vec_f64(&back, [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]).map(from_f64),
            );
        }
        compare(&converted, &reference, 5e-6);
    }
}
fn pipeline(params: RuntimeParams) -> Pipeline {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
        params,
        &dir,
    )
    .unwrap()
}
fn params() -> RuntimeParams {
    let mut p = RuntimeParams::default();
    p.io.input_color_space = "ProPhoto RGB".into();
    p.io.input_cctf_decoding = true;
    p.camera.exposure_compensation_ev = 0.75;
    p.camera.auto_exposure = true;
    p.enlarger.normalize_print_exposure = false;
    p.enlarger.print_exposure_compensation = false;
    p.film_render.grain.active = false;
    p.film_render.glare.active = false;
    p.print_render.glare.active = false;
    p.prep = controls();
    p
}
// Independently prepare a source to test placement: decode once, apply exposure
// before Prep, then feed a neutral pipeline. Fails for omitted/double Prep,
// wrong transfer placement, exposure applied twice, or different resident input.
fn prepared(source: &ImageBuf, p: &RuntimeParams) -> (ImageBuf, RuntimeParams) {
    let mut frame = source.clone();
    let transfer = RgbColourspace::input(&p.io.input_color_space).cctf;
    for v in &mut frame.data {
        if p.io.input_cctf_decoding {
            *v = from_f64(transfer.decode(to_f64(*v)));
        }
        *v *= from_f64(0.4f64.exp2());
        *v *= from_f32((p.camera.exposure_compensation_ev as f64).exp2() as f32);
    }
    prep::apply_linear(&mut frame, &p.prep, &p.io.input_color_space).unwrap();
    let mut neutral = p.clone();
    neutral.io.input_cctf_decoding = false;
    neutral.camera.auto_exposure = false;
    neutral.camera.exposure_compensation_ev = 0.;
    neutral.prep = Default::default();
    (frame, neutral)
}
#[test]
fn cpu_film_sees_decoded_exposed_prepared_scene_exactly_once() {
    let source = image();
    let p = params();
    let (ready, neutral) = prepared(&source, &p);
    let actual = pipeline(p)
        .with_autoexposure_ev(0.4)
        .process(source, &CpuBackend);
    let expected = pipeline(neutral).process(ready, &CpuBackend);
    compare(&actual, &expected, 1e-6);
}
#[test]
fn resident_and_submitted_gpu_apply_prep_and_invalidate_source_uploads() {
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping resident Prep integration");
        return;
    };
    let source = image();
    for encoded in [true, false] {
        for variation in 0..4 {
            let mut p = params();
            p.io.input_cctf_decoding = encoded;
            if variation == 0 || variation == 3 {
                p.prep = Default::default();
            }
            if variation == 2 {
                p.prep.shadows = -45.;
            }
            let (ready, neutral) = prepared(&source, &p);
            let pipe = pipeline(p).with_autoexposure_ev(0.4);
            let expected = pipeline(neutral)
                .process_resident_borrowed(&ready, &gpu)
                .expect("GPU resident reference");
            let actual = pipe
                .process_resident_borrowed(&source, &gpu)
                .expect("GPU resident Prep");
            compare(&actual, &expected, 2e-5);
            let pending = pipe
                .submit_resident(&source, Some(123), &gpu, ChainOutput::Scan, Box::new(|| {}))
                .expect("submitted Prep");
            let ChainResult::Scan(actual) = pipe.finish_submitted(pending, &gpu) else {
                panic!("scan result")
            };
            compare(&actual, &expected, 2e-5);
        }
    }
}
#[test]
fn a_prepared_frame_is_reused_only_while_what_prep_reads_is_unchanged() {
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping prepared-frame reuse");
        return;
    };
    let source = image();
    let bits = |image: &ImageBuf| image.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    let render = |p: &RuntimeParams, frame: Option<&prep::PreparedFrame>| {
        let mut pipe = pipeline(p.clone()).with_autoexposure_ev(0.4);
        if let Some(frame) = frame {
            pipe = pipe.with_prepared_frame(frame.clone());
        }
        bits(&pipe.process_resident_borrowed(&source, &gpu).expect("GPU resident Prep"))
    };
    let frame = prep::PreparedFrame::default();
    let mut edits: Vec<Box<dyn Fn(&mut RuntimeParams)>> = vec![
        Box::new(|_| {}),
        Box::new(|_| {}), // reused
        Box::new(|p| p.enlarger.print_exposure = 1.3), // reused: after Prep
        Box::new(|p| p.camera.exposure_compensation_ev = 0.2),
        Box::new(|p| p.prep.exposure_ev = 0.3),
        Box::new(|p| p.prep.shadows = -45.),
        Box::new(|p| p.io.input_cctf_decoding = false),
        Box::new(|p| p.prep = Default::default()), // neutral: the slot is emptied
    ];
    edits.push(Box::new(|_| {}));
    for edit in &edits {
        let mut p = params();
        edit(&mut p);
        assert_eq!(render(&p, Some(&frame)), render(&p, None));
    }
}
