//! Film and print base tuning and the print-balance solver against frozen
//! Python experimental 28bf883 values (ticket 16, item 7): the tuned base
//! spectra (`model/develop._tuned_base_density`) and the M/Y filter shifts
//! from `runtime/print_balance.solve_neutral_filter_shifts`, for several
//! film/paper pairs with and without a tuned base.
//!
//! Regenerate the fixture with
//! `scripts/parity/freeze_print_balance_reference.py` (Python only, never
//! from Rust output).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use spektrafilm_core::bases::{tuned_film_base, tuned_print_base};
use spektrafilm_core::params::{FilmBaseParams, PrintBaseParams, RuntimeParams};
use spektrafilm_core::print_balance::solve_neutral_filter_shifts;
use spektrafilm_core::profile;

#[derive(Deserialize)]
struct Fixture {
    midgray: f64,
    bases: Vec<Base>,
    solves: Vec<Solve>,
}

#[derive(Deserialize)]
struct Base {
    stock: String,
    is_film: bool,
    params: serde_json::Value,
    tuned: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct Solve {
    film: String,
    paper: String,
    params: RuntimeParams,
    m_filter_shift: f64,
    y_filter_shift: f64,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/print_balance_python_28bf883.json")).unwrap()
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn tuned_bases_match_python() {
    let fixture = fixture();
    assert_eq!(fixture.bases.len(), 8);
    for case in &fixture.bases {
        let p = profile::load_profile_by_name(&data_dir(), &case.stock).unwrap();
        let p = profile::resolve_for_render(p, None);
        let got = if case.is_film {
            let params: FilmBaseParams = serde_json::from_value(case.params.clone()).unwrap();
            tuned_film_base(&p.data.base_density, &params)
        } else {
            let params: PrintBaseParams = serde_json::from_value(case.params.clone()).unwrap();
            tuned_print_base(&p.data.base_density, &params)
        };
        assert_eq!(got.len(), case.tuned.len());
        for (i, (g, w)) in got.iter().zip(&case.tuned).enumerate() {
            match w {
                None => assert!(
                    g.is_nan(),
                    "{} {:?}[{i}]: {g} vs NaN",
                    case.stock,
                    case.params
                ),
                Some(w) => assert!(
                    (g - w).abs() <= 1e-13 * w.abs().max(1.0),
                    "{} {:?}[{i}]: {g} vs {w}",
                    case.stock,
                    case.params
                ),
            }
        }
    }
}

#[test]
fn print_balance_matches_python() {
    let fixture = fixture();
    assert_eq!(fixture.solves.len(), 6);
    // The f32 build's residual is rounded to f32, which moves the root by
    // about 1e-5 filter units.
    let tolerance = if cfg!(feature = "precision-f64") {
        1e-6
    } else {
        1e-4
    };
    for case in &fixture.solves {
        let film = profile::load_profile_by_name(&data_dir(), &case.film).unwrap();
        let print = profile::load_profile_by_name(&data_dir(), &case.paper).unwrap();
        let (m, y) =
            solve_neutral_filter_shifts(film, print, &case.params, &data_dir(), fixture.midgray)
                .unwrap();
        let err = (m - case.m_filter_shift)
            .abs()
            .max((y - case.y_filter_shift).abs());
        eprintln!(
            "{} -> {}: M {m:+.6} Y {y:+.6} (Python {:+.6} {:+.6}), max diff {err:.2e}",
            case.film, case.paper, case.m_filter_shift, case.y_filter_shift
        );
        assert!(
            err <= tolerance,
            "{} -> {}: diff {err}",
            case.film,
            case.paper
        );
    }
}
