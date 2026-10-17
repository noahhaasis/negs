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
