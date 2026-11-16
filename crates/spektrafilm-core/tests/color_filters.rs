//! Camera colour filters against frozen upstream experimental 28bf883
//! transmittances (scripts/parity/freeze_color_filters.py), and their effect
//! on the film exposure.

use serde::Deserialize;
use spektrafilm_core::{color_filters, params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Fixture {
    wavelengths: Vec<f64>,
    filters: Vec<Filter>,
}

#[derive(Deserialize)]
struct Filter {
    key: String,
    label: String,
    transmittance: Vec<f64>,
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn transmittances_match_python() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/color_filters_python_28bf883.json")).unwrap();
    assert_eq!(fixture.wavelengths.len(), 81);
    assert_eq!((fixture.wavelengths[0], fixture.wavelengths[80]), (380.0, 780.0));
    let keys: Vec<&str> = color_filters::COLOR_FILTER_LIBRARY.iter().map(|spec| spec.key).collect();
    let frozen: Vec<&str> = fixture.filters.iter().map(|filter| filter.key.as_str()).collect();
    assert_eq!(keys, frozen);
    for (filter, spec) in fixture.filters.iter().zip(color_filters::COLOR_FILTER_LIBRARY) {
        assert_eq!(filter.label, spec.label);
        let actual = color_filters::transmittance(&data_dir(), &filter.key).unwrap().unwrap();
        assert_eq!(actual.len(), 81);
        for (i, (a, e)) in actual.iter().zip(&filter.transmittance).enumerate() {
            assert!((a - e).abs() < 1e-12, "{} at {} nm: {a} vs {e}", filter.key, fixture.wavelengths[i]);
        }
    }
    assert_eq!(color_filters::transmittance(&data_dir(), "none").unwrap(), None);
    assert!(color_filters::transmittance(&data_dir(), "hoya_typo").is_err());
}

fn film_density(film: &str, color_filter: &str) -> Vec<f64> {
    let data = data_dir();
    let mut params = RuntimeParams::default();
    params.camera.auto_exposure = false;
    params.camera.color_filter = color_filter.into();
    params.io.scan_film = true;
    params.film_render.grain.active = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    let film_profile = profile::load_profile_by_name(&data, film).unwrap();
    let pipeline = Pipeline::new_with_spectral(film_profile.clone(), film_profile, params, &data).unwrap();
    let image = ImageBuf::from_data(1, 1, vec![from_f64(0.184); 3]);
    pipeline.process(image, &CpuBackend).data.iter().map(|&v| v as f64).collect()
}

/// A filter changes the render through the film's sensitivity, "none" is
/// the identity, and an unknown key fails the pipeline build.
#[test]
fn filters_reach_the_film_exposure() {
    let none = film_density("kodak_portra_400", "none");
    let r1 = film_density("kodak_portra_400", "hoya_r1");
    assert_eq!(none, film_density("kodak_portra_400", "none"));
    assert!(r1.iter().all(|v| v.is_finite()));
    assert!(none.iter().zip(&r1).any(|(a, b)| (a - b).abs() > 1e-3), "{none:?} vs {r1:?}");
    let mut params = RuntimeParams::default();
    params.camera.color_filter = "hoya_typo".into();
    let data = data_dir();
    let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
    assert!(Pipeline::new_with_spectral(film.clone(), film, params, &data).is_err());
}

/// Tri-X 400's sensitivity is traced from Kodak's datasheet (F-4017), which
/// also lists daylight filter factors: No. 8 yellow ×2 (1 stop) and No. 25
/// red ×8 (3 stops). Y2 and R1 cut where those do (about 499 and 604 nm
/// against 494 and ~600), so the film and the glass should land near them.
#[test]
fn trix_filter_factors_match_kodaks() {
    let data = data_dir();
    let film = profile::load_profile_by_name(&data, "kodak_trix400_d76_8min").unwrap();
    let stops = |key| color_filters::factor_stops(&film, &color_filters::transmittance(&data, key).unwrap().unwrap()).unwrap();
    let (y2, r1) = (stops("hoya_y2"), stops("hoya_r1"));
    assert!((y2 - 1.0).abs() < 0.3, "Y2 on Tri-X: {y2} stops, Kodak's No. 8: 1");
    assert!((r1 - 3.0).abs() < 0.5, "R1 on Tri-X: {r1} stops, Kodak's No. 25: 3");
}

/// The catalog lists the filters in the apps' order with a swatch colour
/// of the right hue, and filter factors for B&W camera films only.
#[test]
fn the_catalog_carries_filters_and_bw_film_factors() {
    let data = data_dir();
    let stocks = ["ilford_hp5_plus", "kodak_portra_400", "ilford_mgrc_deluxe_grade2"].map(String::from);
    let catalog = spektrafilm_core::catalog::catalog(&data, stocks).unwrap();
    let filters = catalog["color_filters"].as_array().unwrap();
    let names: Vec<&str> = filters.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Green", "Yellow-green", "Yellow", "Orange", "Red"]);
    let rgb = |key: &str| {
        let hex = filters.iter().find(|f| f["key"] == key).unwrap()["colour"].as_str().unwrap();
        (1..7).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect::<Vec<_>>()
    };
    let (red, yellow, green) = (rgb("hoya_r1"), rgb("hoya_y2"), rgb("hoya_x1"));
    assert!(red[0] > 200 && red[1] < 120 && red[2] < 120, "red {red:?}");
    assert!(yellow[0] > 200 && yellow[1] > 180 && yellow[2] < 100, "yellow {yellow:?}");
    assert!(green[1] > green[0] && green[1] > green[2], "green {green:?}");
    let profile = |id: &str| catalog["profiles"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    let hp5 = profile("ilford_hp5_plus")["color_filter_stops"].clone();
    assert_eq!(hp5.as_object().unwrap().len(), 5);
    assert!(hp5["hoya_r1"].as_f64().unwrap() > hp5["hoya_y2"].as_f64().unwrap());
    assert!(profile("kodak_portra_400")["color_filter_stops"].as_object().unwrap().is_empty());
    assert!(profile("ilford_mgrc_deluxe_grade2")["color_filter_stops"].as_object().unwrap().is_empty());
}
