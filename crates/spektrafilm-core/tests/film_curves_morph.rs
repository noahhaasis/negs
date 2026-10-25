//! Film-curve morph against frozen upstream experimental 28bf883 references
//! (scripts/parity/freeze_film_morph_reference.py), and the pipeline's
//! switching between morphed and stored film curves.

use serde::Deserialize;
use spektrafilm_core::{
    params::{CurvesMorphParams, RuntimeParams},
    pipeline::Pipeline,
    profile,
};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    film: String,
    bw: bool,
    morph: CurvesMorphParams,
    curves: Vec<Vec<f64>>,
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn pipeline(film: &str, morph: CurvesMorphParams) -> Pipeline {
    let data = data_dir();
    let mut params = RuntimeParams::default();
    params.film_render.density_curves_morph = morph;
    Pipeline::new(
        profile::load_profile_by_name(&data, film).unwrap(),
        profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
        params,
    )
}

#[test]
fn film_curves_morph_matches_python_experimental() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/film_morph_python_28bf883.json")).unwrap();
    assert_eq!(fixture.cases.len(), 5);
    for case in fixture.cases {
        let curves = pipeline(&case.film, case.morph).film.data.density_curves;
        assert_eq!(curves.len(), case.curves.len(), "{}", case.film);
        for (i, (got, want)) in curves.iter().zip(&case.curves).enumerate() {
            // B&W stocks are one channel upstream, broadcast to three equal lanes here.
            let want: Vec<f64> = if case.bw { vec![want[0]; 3] } else { want.clone() };
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= 1e-12 + 1e-12 * want[c].abs(),
                    "{} row {i} ch {c}: {} vs {}",
                    case.film,
                    got[c],
                    want[c]
                );
            }
        }
    }
}

#[test]
fn inactive_film_morph_keeps_stored_curves_and_with_params_switches() {
    let stored = profile::load_profile_by_name(&data_dir(), "kodak_portra_400")
        .unwrap()
        .data
        .density_curves;
    let morph = CurvesMorphParams { active: true, gamma_factor: 1.3, ..Default::default() };

    let inactive = pipeline("kodak_portra_400", CurvesMorphParams::default());
    assert_eq!(inactive.film.data.density_curves, stored);
    // Active at identity re-evaluates the fitted model, which only
    // approximates the stored curves.
    let identity = pipeline("kodak_portra_400", CurvesMorphParams { active: true, ..Default::default() });
    assert_ne!(identity.film.data.density_curves, stored);

    let active = pipeline("kodak_portra_400", morph.clone());
    let morphed = active.film.data.density_curves.clone();
    assert_ne!(morphed, stored);

    let mut params = active.params.clone();
    params.film_render.density_curves_morph.active = false;
    let switched_off = active.with_params(params.clone());
    assert_eq!(switched_off.film.data.density_curves, stored);
    params.film_render.density_curves_morph = morph;
    let switched_on = switched_off.with_params(params);
    assert_eq!(switched_on.film.data.density_curves, morphed);
}
