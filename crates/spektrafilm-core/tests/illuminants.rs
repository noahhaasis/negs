//! Light sources by name against frozen Python main 3bb2c2d values (ticket
//! 16.2). Before this, Rust knew only D50/D55/D65/TH-KG3 and fell back to D55
//! or D50 for anything else, so the tungsten Vision3 stocks (reference `T`)
//! and the 2383/2393 cine prints (viewing light `K75P`) rendered with the
//! wrong light: up to 0.26 off in output RGB.
//!
//! Regenerate the fixture with `scripts/parity/freeze_illuminants_reference.py`
//! (Python only, never from Rust output).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use spektrafilm_core::illuminants::{standard_illuminant, standard_illuminant_f64};
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f32, spectral};

#[derive(Deserialize)]
struct Fixture {
    spectra: BTreeMap<String, Vec<f64>>,
    reference_white_xyz: BTreeMap<String, [f64; 3]>,
    input: Vec<f32>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    film: String,
    paper: String,
    params: RuntimeParams,
    expected_rgb_lut_off: Vec<f64>,
    expected_rgb_lut_on: Vec<f64>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/illuminants_python_3bb2c2d.json")).unwrap()
}

fn data_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn spectra_match_python() {
    let fixture = fixture();
    assert_eq!(fixture.spectra.len(), 11);
    for (name, want) in &fixture.spectra {
        let got = standard_illuminant_f64(name).unwrap();
        assert_eq!(got.len(), want.len(), "{name}");
        for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
            // The baked tables hold the Python values exactly (D-series and
            // TH-KG3 to their printed digits); blackbodies are recomputed.
            assert!(
                (g - w).abs() <= 1e-12 * w.abs().max(1.0),
                "{name}[{i}]: {g} vs {w}"
            );
        }
    }
}

#[test]
fn reference_whites_match_python() {
    let fixture = fixture();
    for (name, want) in &fixture.reference_white_xyz {
        let table = standard_illuminant(name).unwrap();
        let got = spectral::illuminant_xyz_f64(&table);
        // Baked whites are exact; the BB3200 fallback integrates the f32 table.
        let tolerance = if name.starts_with("BB") { 1e-6 } else { 1e-15 };
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= tolerance,
                "{name}[{c}]: {} vs {}",
                got[c],
                want[c]
            );
        }
    }
}

#[test]
fn cine_pipeline_matches_python_with_and_without_luts() {
    let fixture = fixture();
    #[cfg(feature = "precision-f64")]
    let tolerance = 1e-6;
    #[cfg(not(feature = "precision-f64"))]
    let tolerance = 3e-3;
    let data_dir = data_dir();
    assert_eq!(fixture.cases.len(), 2);
    for case in &fixture.cases {
        for lut in [false, true] {
            let mut params = case.params.clone();
            params.settings.use_enlarger_lut = lut;
            params.settings.use_scanner_lut = lut;
            let film = profile::load_profile_by_name(&data_dir, &case.film).unwrap();
            let print = profile::load_profile_by_name(&data_dir, &case.paper).unwrap();
            let pipeline = Pipeline::new_with_spectral(film, print, params, &data_dir).unwrap();
            let expected = if lut {
                &case.expected_rgb_lut_on
            } else {
                &case.expected_rgb_lut_off
            };
            let input =
                ImageBuf::from_data(4, 4, fixture.input.iter().copied().map(from_f32).collect());
            let output = pipeline.process(input, &CpuBackend);
            assert_eq!(output.data.len(), expected.len());
            for (i, (&got, &want)) in output.data.iter().zip(expected).enumerate() {
                assert!(
                    got.is_finite() && (got as f64 - want).abs() < tolerance,
                    "{} -> {}, LUT={lut}, component {i}: {got} vs {want}",
                    case.film,
                    case.paper
                );
            }
        }
    }
}

#[test]
fn unknown_illuminant_names_are_errors() {
    let data_dir = data_dir();
    let load = |name| profile::load_profile_by_name(&data_dir, name).unwrap();
    let base = RuntimeParams::default();
    let cases: [(
        &str,
        Box<dyn Fn(&mut profile::Profile, &mut profile::Profile, &mut RuntimeParams)>,
    ); 4] = [
        (
            "film reference_illuminant",
            Box::new(|f, _, _| f.info.reference_illuminant = "D56".into()),
        ),
        (
            "film viewing_illuminant",
            Box::new(|f, _, _| f.info.viewing_illuminant = "K75".into()),
        ),
        (
            "print viewing_illuminant",
            Box::new(|_, p, _| p.info.viewing_illuminant = "BBhot".into()),
        ),
        (
            "enlarger.illuminant",
            Box::new(|_, _, r| r.enlarger.illuminant = "TH-KG4".into()),
        ),
    ];
    for (what, break_it) in cases {
        let (mut film, mut print, mut params) = (
            load("kodak_portra_400"),
            load("kodak_portra_endura"),
            base.clone(),
        );
        break_it(&mut film, &mut print, &mut params);
        let err = match Pipeline::new_with_spectral(film, print, params, &data_dir) {
            Ok(_) => panic!("{what}: unknown name accepted"),
            Err(e) => e,
        };
        assert!(
            err.starts_with(what) && err.contains("unknown illuminant"),
            "{what}: {err}"
        );
    }
}
