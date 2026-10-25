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
