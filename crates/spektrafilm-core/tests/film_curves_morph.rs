//! Film-curve morph against frozen upstream experimental 28bf883 references
//! (scripts/parity/freeze_film_morph_reference.py): model-derived film and
//! print curves, sub-layer curves, and the pipeline's re-baking of the morph.

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
    stock: String,
    role: String,
    bw: bool,
    development_time: Option<f64>,
    morph: Option<CurvesMorphParams>,
    curves: Vec<Vec<f64>>,
    /// `[n_le][n_layers][n_ch]`, stored for some cases only.
    layers: Option<Vec<Vec<Vec<f64>>>>,
    has_layers: bool,
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn pipeline(film: &str, morph: CurvesMorphParams, development_time: Option<f64>) -> Pipeline {
    let data = data_dir();
    let mut params = RuntimeParams::default();
    params.film_render.density_curves_morph = morph;
    params.film_render.development_time = development_time;
    Pipeline::new(
        profile::load_profile_by_name(&data, film).unwrap(),
        profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
        params,
    )
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-12 + 1e-12 * want.abs(), "{what}: {got} vs {want}");
}

/// Upstream experimental derives every profile's curves from its fitted
/// model, collapses B&W families and bakes the film chemistry in, keeping
/// the sub-layer curves for grain. B&W stocks are one channel upstream and
/// three equal lanes here.
#[test]
fn model_curves_and_film_morph_match_python_experimental() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/film_morph_python_28bf883.json")).unwrap();
    assert_eq!(fixture.cases.len(), 10);
    for case in fixture.cases {
        let label = format!("{} {} {:?}", case.stock, case.role, case.development_time);
        let resolved = if case.role == "film" {
            pipeline(&case.stock, case.morph.clone().unwrap(), case.development_time).film
        } else {
            let p = profile::load_profile_by_name(&data_dir(), &case.stock).unwrap();
            profile::resolve_for_render(p, case.development_time)
        };
        let lane = |row: &[f64], c: usize| if case.bw { row[0] } else { row[c] };
        let curves = &resolved.data.density_curves;
        assert_eq!(curves.len(), case.curves.len(), "{label}");
        for (i, (got, want)) in curves.iter().zip(&case.curves).enumerate() {
            for c in 0..3 {
                assert_close(got[c], lane(want, c), &format!("{label} row {i} ch {c}"));
            }
        }
        let layers = resolved.density_curves_layers_f64();
        assert_eq!(layers.is_some(), case.has_layers, "{label} layers");
        if let (Some(got), Some(want)) = (layers, case.layers.as_ref()) {
            assert_eq!(got.len(), want.len(), "{label}");
            for (i, (g, w)) in got.iter().zip(want).enumerate() {
                assert_eq!(g.len(), w.len(), "{label} row {i} layer count");
                for (l, (gl, wl)) in g.iter().zip(w).enumerate() {
                    for c in 0..3 {
                        assert_close(gl[c], lane(wl, c), &format!("{label} row {i} layer {l} ch {c}"));
                    }
                }
            }
        }
    }
}

/// Inactive and identity settings both reproduce the model-derived curves
/// the profile was loaded with, and `with_params` re-bakes the morph from
/// the model in both directions.
#[test]
fn film_morph_rebakes_from_the_model_and_with_params_switches() {
    let loaded = profile::load_profile_by_name(&data_dir(), "kodak_portra_400").unwrap();
    let morph = CurvesMorphParams { active: true, gamma_factor: 1.3, ..Default::default() };

    let inactive = pipeline("kodak_portra_400", CurvesMorphParams::default(), None);
    assert_eq!(inactive.film.data.density_curves, loaded.data.density_curves);
    assert_eq!(inactive.film.data.density_curves_layers, loaded.data.density_curves_layers);
    let identity =
        pipeline("kodak_portra_400", CurvesMorphParams { active: true, ..Default::default() }, None);
    assert_eq!(identity.film.data.density_curves, loaded.data.density_curves);

    let active = pipeline("kodak_portra_400", morph.clone(), None);
    let morphed = active.film.data.density_curves.clone();
    let morphed_layers = active.film.data.density_curves_layers.clone();
    assert_ne!(morphed, loaded.data.density_curves);
    // The total is the in-order sum of the sub-layers.
    for (row, layers) in morphed.iter().zip(&morphed_layers) {
        for c in 0..3 {
            let sum = layers.iter().fold(0.0, |acc, l| acc + l[c]);
            assert_eq!(row[c].to_bits(), sum.to_bits());
        }
    }

    let mut params = active.params.clone();
    params.film_render.density_curves_morph.active = false;
    let switched_off = active.with_params(params.clone());
    assert_eq!(switched_off.film.data.density_curves, loaded.data.density_curves);
    params.film_render.density_curves_morph = morph;
    let switched_on = switched_off.with_params(params);
    assert_eq!(switched_on.film.data.density_curves, morphed);
    assert_eq!(switched_on.film.data.density_curves_layers, morphed_layers);
}
