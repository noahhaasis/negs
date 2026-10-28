//! Halation presets against frozen upstream experimental 28bf883 references
//! (scripts/parity/freeze_halation_presets.py): the `(use, antihalation)`
//! table, and the effective halation params after upstream's `digest_params`
//! for every profile this repository ships.

use serde::Deserialize;
use spektrafilm_core::{params::RuntimeParams, pipeline, profile};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Fixture {
    table: Vec<TableRow>,
    profiles: Vec<ProfileCase>,
}

#[derive(Deserialize)]
struct TableRow {
    #[serde(rename = "use")]
    usage: String,
    antihalation: String,
    sigma_um: [f64; 3],
    strength: [f64; 3],
}

#[derive(Deserialize)]
struct ProfileCase {
    stock: String,
    #[serde(rename = "use")]
    usage: String,
    antihalation: String,
    is_bw: bool,
    halation_first_sigma_um: [f64; 3],
    halation_strength: [f64; 3],
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/halation_presets_python_28bf883.json")).unwrap()
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn preset_table_matches_python() {
    let fixture = fixture();
    assert_eq!(fixture.table.len(), 6);
    for row in &fixture.table {
        assert_eq!(
            pipeline::halation_preset(&row.usage, &row.antihalation),
            Some((row.sigma_um, row.strength)),
            "{} / {}",
            row.usage,
            row.antihalation
        );
    }
    assert_eq!(pipeline::halation_preset("still", "unknown"), None);
    assert_eq!(pipeline::halation_preset("unknown", "strong"), None);
}

/// Upstream keeps a B&W stock's channel 0 (`match_channels`); the
/// three-lane engine broadcasts it.
fn lanes(values: [f64; 3], bw: bool) -> [f64; 3] {
    if bw { [values[0]; 3] } else { values }
}

#[test]
fn every_shipped_profile_digests_like_python() {
    let fixture = fixture();
    let data = data_dir();
    let mut shipped: Vec<String> = std::fs::read_dir(data.join("profiles"))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension()? == "json").then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    shipped.sort();
    let frozen: Vec<&str> = fixture.profiles.iter().map(|case| case.stock.as_str()).collect();
    assert_eq!(frozen, shipped, "refreeze the fixture after adding or removing profiles");
    for case in &fixture.profiles {
        let film = profile::load_profile_by_name(&data, &case.stock).unwrap();
        assert_eq!((film.info.usage.as_str(), film.info.antihalation.as_str()),
                   (case.usage.as_str(), case.antihalation.as_str()), "{}", case.stock);
        let film = profile::resolve_for_render(film, None);
        let mut params = RuntimeParams::default();
        pipeline::apply_film_specific_params(&film, &mut params);
        let h = &params.film_render.halation;
        assert_eq!(h.first_sigma_um(), lanes(case.halation_first_sigma_um, case.is_bw), "{}", case.stock);
        assert_eq!(h.strength(), lanes(case.halation_strength, case.is_bw), "{}", case.stock);
    }
}
