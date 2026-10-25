//! Independent Python 28bf883 output-compressor references, including saturated
//! and above-white inputs. The input hull is checked in input_gamut unit tests.

use serde::Deserialize;
use spektrafilm_core::{
    gamut_compression::OutputGamutCompress,
    params::{OutputGamutCompressParams, RuntimeParams},
    pipeline::Pipeline,
    profile,
};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f32};
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    output: OutputCase,
    pipeline_cases: Vec<PipelineCase>,
}

#[derive(Deserialize)]
struct OutputCase {
    spec: OutputGamutCompressParams,
    rgb: Vec<[f64; 3]>,
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
    expected_rgb: Vec<f64>,
}

#[test]
fn experimental_bw_gamut_pipeline_matches_python_with_and_without_luts() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/gamut_python_28bf883.json")).unwrap();
    assert_eq!(fixture.pipeline_cases.len(), 6);
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    for case in fixture.pipeline_cases {
        for lut in [false, true] {
            let mut params = case.params.clone();
            params.settings.use_enlarger_lut = lut;
            params.settings.use_scanner_lut = lut;
            let film = profile::load_profile_by_name(&data_dir, &case.film).unwrap();
            let paper = if case.scan_film {
                &case.film
            } else {
                &case.paper
            };
            let print = profile::load_profile_by_name(&data_dir, paper).unwrap();
            let pipeline = Pipeline::new_with_spectral(film, print, params, &data_dir).unwrap();
            let input =
                ImageBuf::from_data(4, 4, case.input.iter().copied().map(from_f32).collect());
            let output = pipeline.process(input, &CpuBackend);
            assert_eq!(output.data.len(), case.expected_rgb.len());
            #[cfg(feature = "precision-f64")]
            let tolerance = 1e-6;
            #[cfg(not(feature = "precision-f64"))]
            let tolerance = 3e-3;
            for (i, (&got, &expected)) in output.data.iter().zip(&case.expected_rgb).enumerate() {
                assert!(
                    got.is_finite() && (got as f64 - expected).abs() < tolerance,
                    "{} -> {paper}, {}, LUT={lut}, component {i}: {got} vs {expected}",
                    case.film,
                    case.name
                );
            }
        }
    }
}

#[test]
fn experimental_output_gamut_matches_python() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/gamut_python_28bf883.json")).unwrap();
    let case = fixture.output;
    let compressor = OutputGamutCompress::build(&case.spec, "sRGB");
    assert_eq!(case.rgb.len(), case.expected.len());
    for (rgb, expected) in case.rgb.into_iter().zip(case.expected) {
        let actual = compressor.compress(rgb);
        for c in 0..3 {
            assert!(
                (actual[c] - expected[c]).abs() < 1e-6,
                "input {rgb:?}, channel {c}: {} vs {}",
                actual[c],
                expected[c]
            );
        }
    }
}

#[derive(Deserialize)]
struct DefaultsFixture {
    input: PythonInputSpec,
    input_center: String,
    output: PythonOutputSpec,
    params_io_input: PythonInputSpec,
    params_io_output: PythonOutputSpec,
}

#[derive(Deserialize, PartialEq, Debug)]
struct PythonInputSpec {
    active: bool,
    algorithm: String,
    boundary: String,
    hull_detail: f64,
    knee: [f64; 3],
}

#[derive(Deserialize, PartialEq, Debug)]
struct PythonOutputSpec {
    algorithm: String,
    knee: [f64; 3],
    lightness_compression: Option<[f64; 3]>,
}

/// The gamut defaults, including those of a params file that leaves the
/// gamut out, are experimental 28bf883's (scripts/parity/freeze_gamut_defaults.py).
#[test]
fn gamut_defaults_match_python() {
    let fixture: DefaultsFixture =
        serde_json::from_str(include_str!("fixtures/gamut_defaults_python_28bf883.json")).unwrap();
    assert_eq!(fixture.input, fixture.params_io_input);
    assert_eq!(fixture.output, fixture.params_io_output);
    let parsed: RuntimeParams = serde_json::from_str("{}").unwrap();
    for params in [RuntimeParams::default(), parsed] {
        let input = &params.io.input_gamut_compress;
        assert!(fixture.input.active);
        let knee = |k: [f32; 3]| k.map(f64::from);
        // The f32 knees round-trip the Python literals.
        let python_knee = |k: [f64; 3]| k.map(|v| v as f32).map(f64::from);
        assert_eq!(
            (input.algorithm.as_str(), input.boundary.as_str(), input.hull_detail, input.center.as_str()),
            (fixture.input.algorithm.as_str(), fixture.input.boundary.as_str(), fixture.input.hull_detail,
             fixture.input_center.as_str())
        );
        assert_eq!(knee(input.knee), python_knee(fixture.input.knee));
        let output = &params.io.output_gamut_compress;
        assert_eq!(output.algorithm, fixture.output.algorithm);
        assert_eq!(knee(output.knee), python_knee(fixture.output.knee));
        assert_eq!(output.lightness_compression.map(knee), fixture.output.lightness_compression.map(python_knee));
    }
}
