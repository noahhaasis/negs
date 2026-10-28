//! Saved values beat stock presets. Upstream's GUI applies the DIR coupler
//! and halation presets when the stock is chosen, and a saved GUI state then
//! replaces those groups (`spektrafilm_gui/params_mapper.py` at 28bf883), so
//! a value set in params must survive `apply_film_specific_params`, while an
//! unset one takes the stock's preset.

use spektrafilm_core::{params::RuntimeParams, pipeline, profile};
use std::path::{Path, PathBuf};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn film(stock: &str) -> profile::Profile {
    let film = profile::load_profile_by_name(&data_dir(), stock).unwrap();
    profile::resolve_for_render(film, None)
}

fn digest(stock: &str, params: RuntimeParams) -> RuntimeParams {
    let mut params = params;
    pipeline::apply_film_specific_params(&film(stock), &mut params);
    params
}

#[test]
fn unset_values_take_the_stock_preset() {
    // Portra 400: couplers.toml's stock table, and "still" / "strong".
    let params = digest("kodak_portra_400", RuntimeParams::default());
    let dir = &params.film_render.dir_couplers;
    assert_eq!(dir.gamma_samelayer_rgb, Some([0.5159, 0.5934, 0.2829]));
    assert_eq!(dir.gamma_interlayer_r_to_gb, Some([0.4032, 0.2488]));
    let h = &params.film_render.halation;
    assert_eq!(h.halation_strength, Some([0.015, 0.005, 0.0]));
    assert_eq!(h.halation_first_sigma_um, Some([65.0; 3]));
}

#[test]
fn set_values_survive_the_stock_preset() {
    let mut params = RuntimeParams::default();
    let dir = &mut params.film_render.dir_couplers;
    dir.gamma_samelayer_rgb = Some([0.52, 0.51, 0.26]);
    dir.gamma_interlayer_r_to_gb = Some([0.16, 0.2]);
    params.film_render.halation.halation_strength = Some([0.095, 0.06, 0.01]);
    let params = digest("kodak_portra_400", params);

    let dir = &params.film_render.dir_couplers;
    assert_eq!(dir.gamma_samelayer_rgb, Some([0.52, 0.51, 0.26]));
    assert_eq!(dir.gamma_interlayer_r_to_gb, Some([0.16, 0.2]));
    // Fields left unset still take the preset, one by one.
    assert_eq!(dir.gamma_interlayer_g_to_rb, Some([0.2227, 0.434]));
    let h = &params.film_render.halation;
    assert_eq!(h.halation_strength, Some([0.095, 0.06, 0.01]));
    assert_eq!(h.halation_first_sigma_um, Some([65.0; 3]));
}

#[test]
fn black_and_white_collapses_set_values_to_channel_zero() {
    let mut params = RuntimeParams::default();
    params.film_render.dir_couplers.gamma_samelayer_rgb = Some([0.3, 0.1, 0.2]);
    params.film_render.dir_couplers.gamma_interlayer_r_to_gb = Some([0.4, 0.5]);
    params.film_render.halation.halation_strength = Some([0.2, 0.1, 0.0]);
    let params = digest("kodak_trix", params);

    let dir = &params.film_render.dir_couplers;
    assert_eq!(dir.gamma_samelayer_rgb, Some([0.3; 3]));
    assert_eq!(dir.gamma_interlayer_r_to_gb, Some([0.0; 2]));
    assert_eq!(params.film_render.halation.halation_strength, Some([0.2; 3]));
}

#[test]
fn unset_values_serialize_as_null() {
    let json = serde_json::to_value(RuntimeParams::default()).unwrap();
    for path in [
        "/film_render/dir_couplers/gamma_samelayer_rgb",
        "/film_render/dir_couplers/gamma_interlayer_r_to_gb",
        "/film_render/dir_couplers/gamma_interlayer_g_to_rb",
        "/film_render/dir_couplers/gamma_interlayer_b_to_rg",
        "/film_render/halation/halation_strength",
        "/film_render/halation/halation_first_sigma_um",
    ] {
        assert_eq!(json.pointer(path), Some(&serde_json::Value::Null), "{path}");
    }
    // And a params file that sets them reads them back as set.
    let set: RuntimeParams = serde_json::from_value(serde_json::json!({
        "film_render": {"halation": {"halation_strength": [0.1, 0.2, 0.3]}}
    }))
    .unwrap();
    assert_eq!(set.film_render.halation.halation_strength, Some([0.1, 0.2, 0.3]));
}

/// The apps keep a saved `null` at these paths when restoring a session, so
/// each must read `null` back, and the stock-preset fields must be listed.
#[test]
fn nullable_paths_accept_null() {
    let paths = RuntimeParams::nullable_paths();
    for required in [
        "film_render.dir_couplers.gamma_samelayer_rgb",
        "film_render.halation.halation_strength",
        "film_render.grain.rms_granularity",
        "film_render.development_time",
        "io.photo_long_side_px",
        "film_render.dir_couplers.langmuir_donor_k_rgb[*]",
        "io.output_gamut_compress.lightness_compression",
    ] {
        assert!(paths.iter().any(|p| p == required), "{required} missing from {paths:?}");
    }
    for path in &paths {
        let mut json = serde_json::to_value(RuntimeParams::default()).unwrap();
        let (field, all) = match path.strip_suffix("[*]") {
            Some(field) => (field, true),
            None => (path.as_str(), false),
        };
        let pointer = format!("/{}", field.replace('.', "/"));
        let slot = json.pointer_mut(&pointer).unwrap_or_else(|| panic!("{path} not in the schema"));
        if all {
            for element in slot.as_array_mut().unwrap() {
                *element = serde_json::Value::Null;
            }
        } else {
            *slot = serde_json::Value::Null;
        }
        serde_json::from_value::<RuntimeParams>(json).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}
