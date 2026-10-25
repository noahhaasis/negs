//! Slide (positive) films with DIR couplers on, against frozen Python main
//! 3bb2c2d values (ticket 14). Python sets per-stock DIR gammas for Provia
//! 100F and Velvia 100 on top of the positive-film defaults; missing them
//! moved Provia's developed density by up to 0.12.
//!
//! Regenerate the fixture with `scripts/parity/freeze_slide_dir_reference.py`
//! (Python only, never from Rust output).

use serde::Deserialize;
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f32};
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    input: Vec<f32>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    film: String,
    paper: String,
    scan_film: bool,
    #[serde(rename = "type")]
    film_type: String,
    params: RuntimeParams,
    dir_couplers: DirGammas,
    expected_rgb_lut_off: Vec<f64>,
    expected_rgb_lut_on: Vec<f64>,
}

/// The digested Python `DirCouplersParams` (only the fields Rust derives).
#[derive(Deserialize, Debug, PartialEq)]
struct DirGammas {
    gamma_samelayer_rgb: [f64; 3],
    gamma_interlayer_r_to_gb: [f64; 2],
    gamma_interlayer_g_to_rb: [f64; 2],
    gamma_interlayer_b_to_rg: [f64; 2],
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/slide_dir_python_3bb2c2d.json")).unwrap()
}

fn pipeline(case: &Case, lut: bool) -> Pipeline {
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut params = case.params.clone();
    assert!(params.film_render.dir_couplers.active);
    params.settings.use_enlarger_lut = lut;
    params.settings.use_scanner_lut = lut;
    let film = profile::load_profile_by_name(&data_dir, &case.film).unwrap();
    let paper = if case.scan_film {
        &case.film
    } else {
        &case.paper
    };
    let print = profile::load_profile_by_name(&data_dir, paper).unwrap();
    Pipeline::new_with_spectral(film, print, params, &data_dir).unwrap()
}

#[test]
fn slide_dir_coupler_gammas_match_python() {
    let fixture = fixture();
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|c| c.film_type == "positive")
            .count(),
        5
    );
    for case in &fixture.cases {
        let dir = &pipeline(case, false).params.film_render.dir_couplers;
        let actual = DirGammas {
            gamma_samelayer_rgb: dir.gamma_samelayer_rgb,
            gamma_interlayer_r_to_gb: dir.gamma_interlayer_r_to_gb,
            gamma_interlayer_g_to_rb: dir.gamma_interlayer_g_to_rb,
            gamma_interlayer_b_to_rg: dir.gamma_interlayer_b_to_rg,
        };
        assert_eq!(actual, case.dir_couplers, "{}", case.film);
    }
}

#[test]
fn slide_dir_coupler_pipeline_matches_python_with_and_without_luts() {
    let fixture = fixture();
    #[cfg(feature = "precision-f64")]
    let tolerance = 1e-6;
    #[cfg(not(feature = "precision-f64"))]
    let tolerance = 3e-3;
    for case in &fixture.cases {
        for lut in [false, true] {
            let expected = if lut {
                &case.expected_rgb_lut_on
            } else {
                &case.expected_rgb_lut_off
            };
            let input =
                ImageBuf::from_data(4, 4, fixture.input.iter().copied().map(from_f32).collect());
            let output = pipeline(case, lut).process(input, &CpuBackend);
            assert_eq!(output.data.len(), expected.len());
            for (i, (&got, &want)) in output.data.iter().zip(expected).enumerate() {
                assert!(
                    got.is_finite() && (got as f64 - want).abs() < tolerance,
                    "{} (scan_film={}), LUT={lut}, component {i}: {got} vs {want}",
                    case.film,
                    case.scan_film
                );
            }
        }
    }
}
