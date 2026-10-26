//! Every RGB colour-space conversion against Python 28bf883 and colour-science
//! 0.4.7: the generated colourspace table, the CATs, the illuminant whites,
//! the input projection, the scan's XYZ → RGB, the output gamut compressor,
//! the output encoding, and whole pipelines across input and output spaces.
//! Regenerate the fixture with `scripts/parity/freeze_colorspaces_reference.py`.

use serde::Deserialize;
use spektrafilm_core::{
    gamut_compression::OutputGamutCompress,
    params::{OutputGamutCompressParams, RuntimeParams},
    pipeline::Pipeline,
    profile,
    stages::scanning::encode_output,
};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{
    colorspace::{self, CAT02_INVERSE, CAT16_INVERSE, COLOURSPACES, RgbColourspace},
    image::ImageBuf,
    precision::{from_f32, from_f64},
    spectral,
};
use std::collections::HashMap;
use std::path::Path;

type M3 = [[f64; 3]; 3];

#[derive(Deserialize)]
struct Fixture {
    colourspaces: HashMap<String, Colourspace>,
    cctf_x: Vec<f64>,
    cat_inverse: HashMap<String, M3>,
    illuminant_whites: HashMap<String, [f64; 3]>,
    tc_b_rgb: Vec<[f64; 3]>,
    tc_b: Vec<TcB>,
    scan_xyz: Vec<[f64; 3]>,
    scan_rgb: HashMap<String, Vec<[f64; 3]>>,
    gamut_rgb: Vec<[f64; 3]>,
    gamut: Vec<GamutCase>,
    encode_rgb: Vec<f64>,
    encoded_output: HashMap<String, Vec<Option<f64>>>,
    pipeline_cases: Vec<PipelineCase>,
}

#[derive(Deserialize)]
struct Colourspace {
    whitepoint_xyz: [f64; 3],
    rgb_to_xyz: M3,
    xyz_to_rgb: M3,
    rgb_to_rgb_self: M3,
    cctf_encoded: Vec<Option<f64>>,
    cctf_decoded: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct TcB {
    color_space: String,
    decode: bool,
    tc: Vec<[f64; 2]>,
    b: Vec<f64>,
}

#[derive(Deserialize)]
struct GamutCase {
    algorithm: String,
    color_space: String,
    knee: [f32; 3],
    lightness_compression: [f32; 3],
    expected: Vec<[f64; 3]>,
}

#[derive(Deserialize)]
struct PipelineCase {
    name: String,
    film: String,
    paper: String,
    scan_film: bool,
    params: RuntimeParams,
    input: Vec<f32>,
    expected_rgb: Vec<Option<f64>>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/colorspaces_python_28bf883.json")).unwrap()
}

fn close(got: f64, want: f64, tol: f64) -> bool {
    (got - want).abs() <= tol * want.abs().max(1.0)
}

/// Each value equals colour's or is NaN where colour's is.
fn check_curve(name: &str, what: &str, xs: &[f64], want: &[Option<f64>], f: impl Fn(f64) -> f64) {
    for (&x, &want) in xs.iter().zip(want) {
        let got = f(x);
        match want {
            Some(want) => assert!(close(got, want, 1e-14), "{name} {what}({x}): {got} vs {want}"),
            None => assert!(got.is_nan(), "{name} {what}({x}): {got}, colour gives NaN"),
        }
    }
}

/// The generated table holds colour's own values, to the last bit or two.
#[test]
fn colourspace_table_is_colours() {
    let fixture = fixture();
    assert_eq!(fixture.colourspaces.len(), COLOURSPACES.len());
    for cs in &COLOURSPACES {
        let want = &fixture.colourspaces[cs.name];
        for (g, w) in cs.whitepoint_xyz.iter().zip(want.whitepoint_xyz) {
            assert!(close(*g, w, 1e-15), "{}: {:?} vs {:?}", cs.name, cs.whitepoint_xyz, want.whitepoint_xyz);
        }
        // colour derives some matrices at import (DCI-P3, Display P3) and the
        // self-conversion is a matrix product; numpy's vectorised arithmetic
        // may round the last bit differently from run to run.
        for (got, want) in [
            (&cs.rgb_to_xyz, &want.rgb_to_xyz),
            (&cs.xyz_to_rgb, &want.xyz_to_rgb),
            (&cs.rgb_to_rgb_self, &want.rgb_to_rgb_self),
        ] {
            for (g, w) in got.iter().flatten().zip(want.iter().flatten()) {
                assert!(close(*g, *w, 1e-15), "{}: {got:?} vs {want:?}", cs.name);
            }
        }
        check_curve(cs.name, "encode", &fixture.cctf_x, &want.cctf_encoded, |x| cs.cctf.encode(x));
        check_curve(cs.name, "decode", &fixture.cctf_x, &want.cctf_decoded, |x| cs.cctf.decode(x));
    }
    assert_eq!(CAT02_INVERSE, fixture.cat_inverse["CAT02"]);
    assert_eq!(CAT16_INVERSE, fixture.cat_inverse["CAT16"]);
}

/// The whites the input CAT adapts to: `_illuminant_to_xy` then colour's
/// `xy_to_XYZ`.
#[test]
fn illuminant_whites_are_colours() {
    let fixture = fixture();
    for (name, table) in [
        ("D50", &spectral::ILLUMINANT_D50[..]),
        ("D55", &spectral::ILLUMINANT_D55[..]),
        ("D65", &spectral::ILLUMINANT_D65[..]),
        ("T", &spectral::ILLUMINANT_T[..]),
        ("K75P", &spectral::ILLUMINANT_K75P[..]),
    ] {
        // numpy's pairwise spectral sums can differ in the last bit between runs.
        let got = spectral::illuminant_xyz_f64(table);
        for (g, w) in got.iter().zip(fixture.illuminant_whites[name]) {
            assert!(close(*g, w, 1e-15), "{name}: {got:?} vs {w}");
        }
    }
}

/// `_rgb_to_tc_b` (CAT16 to D55) for every input space, linear and decoded.
#[test]
fn input_projection_matches_python() {
    let fixture = fixture();
    for case in &fixture.tc_b {
        let cs = RgbColourspace::find(&case.color_space).unwrap();
        for (k, rgb) in fixture.tc_b_rgb.iter().enumerate() {
            let rgb = rgb.map(|v| from_f64(if case.decode { cs.cctf.decode(v) } else { v }));
            let (tc, b) = spectral::rgb_to_tc_b(rgb, cs.name, &spectral::ILLUMINANT_D55, true);
            #[cfg(feature = "precision-f64")]
            let tol = 1e-12;
            #[cfg(not(feature = "precision-f64"))]
            let tol = if case.decode { 1e-6 } else { 1e-12 };
            let want = (case.tc[k], case.b[k]);
            assert!(
                close(tc.0, want.0[0], tol) && close(tc.1, want.0[1], tol) && close(b, want.1, tol),
                "{} decode={} rgb {k}: {tc:?} {b} vs {want:?}",
                case.color_space,
                case.decode
            );
        }
    }
}

/// The scan's `XYZ_to_RGB(illuminant=K75P)`: CAT02 to the output white,
/// then the output matrix.
#[test]
fn scan_xyz_to_rgb_matches_python() {
    let fixture = fixture();
    let white = spectral::illuminant_xyz_f64(&spectral::ILLUMINANT_K75P);
    for (name, expected) in &fixture.scan_rgb {
        let cs = RgbColourspace::output(name);
        assert_eq!(cs.name, name);
        let adapt = colorspace::chromatic_adaptation_matrix_f64(white, cs.whitepoint_xyz);
        for (xyz, want) in fixture.scan_xyz.iter().zip(expected) {
            let mv = |m: &M3, v: [f64; 3]| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]);
            let got = mv(&cs.xyz_to_rgb, mv(&adapt, *xyz));
            for c in 0..3 {
                assert!(close(got[c], want[c], 1e-12), "{name} {xyz:?}: {got:?} vs {want:?}");
            }
        }
    }
}

/// `compress_rgb` against each output cube.
#[test]
fn output_gamut_matches_python_for_every_output() {
    let fixture = fixture();
    for case in &fixture.gamut {
        let params = OutputGamutCompressParams {
            algorithm: case.algorithm.clone(),
            knee: case.knee,
            lightness_compression: Some(case.lightness_compression),
        };
        let compressor = OutputGamutCompress::build(&params, &case.color_space);
        assert!(compressor.is_active());
        for (rgb, want) in fixture.gamut_rgb.iter().zip(&case.expected) {
            let got = compressor.compress(*rgb);
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-6,
                    "{} {}: {rgb:?} -> {got:?} vs {want:?}",
                    case.algorithm,
                    case.color_space
                );
            }
        }
    }
}

/// `RGB_to_RGB(cs, cs, apply_cctf_encoding=True)` after the scan, unclipped:
/// values above 1 and below 0 too, NaN where colour's pure gammas give it.
#[test]
fn output_encoding_matches_python() {
    let fixture = fixture();
    for (name, want) in &fixture.encoded_output {
        let n = fixture.encode_rgb.len() as u32 / 3;
        let mut rgb = ImageBuf::from_data(n, 1, fixture.encode_rgb.iter().map(|&v| from_f64(v)).collect());
        encode_output(&mut rgb, name);
        #[cfg(feature = "precision-f64")]
        let tol = 1e-14;
        #[cfg(not(feature = "precision-f64"))]
        let tol = 1e-6;
        for (i, (&got, want)) in rgb.data.iter().zip(want).enumerate() {
            match want {
                Some(want) => assert!(close(got as f64, *want, tol), "{name} [{i}]: {got} vs {want}"),
                None => assert!(got.is_nan(), "{name} [{i}]: {got}, colour gives NaN"),
            }
        }
    }
}

/// Whole pipelines across input/output spaces, with CCTF decoding and
/// encoding and each perceptual compressor. The scan is unclipped.
#[test]
fn pipelines_across_colour_spaces_match_python() {
    let fixture = fixture();
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    for case in fixture.pipeline_cases {
        let film = profile::load_profile_by_name(&data_dir, &case.film).unwrap();
        let paper = if case.scan_film { &case.film } else { &case.paper };
        let print = profile::load_profile_by_name(&data_dir, paper).unwrap();
        let pipeline = Pipeline::new_with_spectral(film, print, case.params.clone(), &data_dir).unwrap();
        let input = ImageBuf::from_data(4, 4, case.input.iter().copied().map(from_f32).collect());
        let output = pipeline.process(input, &CpuBackend);
        assert_eq!(output.data.len(), case.expected_rgb.len());
        #[cfg(feature = "precision-f64")]
        let tolerance = 1e-6;
        #[cfg(not(feature = "precision-f64"))]
        let tolerance = 3e-3;
        for (i, (&got, expected)) in output.data.iter().zip(&case.expected_rgb).enumerate() {
            let ok = match expected {
                Some(expected) => (got as f64 - expected).abs() < tolerance,
                None => got.is_nan(),
            };
            assert!(ok, "{}, component {i}: {got} vs {expected:?}", case.name);
        }
    }
}
