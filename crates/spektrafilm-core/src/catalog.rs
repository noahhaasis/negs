//! The stock catalog the apps build their pickers and parameter defaults
//! from: each profile's kind and pairing, the per-stock preset values an edit
//! shows while it leaves them unset (null), the engine's defaults and the
//! paths where null is a value. The native helper answers `catalog` with it,
//! and the browser build generates `web/catalog.json` from it.
use crate::{params::RuntimeParams, profile};
use serde_json::{Value, json};
use std::path::Path;

/// The catalog of `stocks` (profile names under `data/profiles`), sorted by
/// display name.
pub fn catalog(data: &Path, stocks: impl IntoIterator<Item = String>) -> Result<Value, profile::ProfileError> {
    // The camera colour filters in the apps' order, with their curves for
    // each B&W film's filter factors below.
    let invalid = profile::ProfileError::Validation;
    let mut filters = Vec::new();
    for key in crate::color_filters::APP_ORDER {
        let spec = crate::color_filters::COLOR_FILTER_LIBRARY.iter().find(|spec| spec.key == key).expect("APP_ORDER names library keys");
        let curve = crate::color_filters::transmittance(data, key).map_err(invalid)?.expect("a library key has a curve");
        filters.push((spec, curve));
    }
    let mut profiles = Vec::new();
    for stock in stocks {
        let p = profile::load_profile_by_name(data, &stock)?;
        // The stock's grain preset, resolved: the apps show it while the
        // edit leaves a value unset (null).
        let grain = crate::grain::grain_settings(&p, &Default::default());
        // Likewise the DIR gammas and halation baseline the stock's presets
        // give an edit that leaves them unset.
        let mut resolved = RuntimeParams::default();
        crate::pipeline::apply_film_specific_params(&p, &mut resolved);
        let dir = &resolved.film_render.dir_couplers;
        let halation = &resolved.film_render.halation;
        // How many stops each filter takes on a B&W camera film, for the
        // Color Filter hint. Colour films get none: the filters are for B&W.
        // A film the glass leaves blind (a blue-sensitive stock behind red)
        // has no factor for it rather than no catalog.
        let is_camera_film = !(p.info.support == "paper" || p.info.stage == "printing");
        let mut stops = serde_json::Map::new();
        if p.is_bw() && is_camera_film {
            for (spec, curve) in &filters {
                if let Ok(value) = crate::color_filters::factor_stops(&p, curve) {
                    stops.insert(spec.key.into(), json!((value * 10.0).round() / 10.0));
                }
            }
        }
        profiles.push(json!({
            "color_filter_stops": stops,
            "id": stock, "name": p.info.name.as_ref().unwrap_or(&stock),
            "paper": p.info.support == "paper" || p.info.stage == "printing",
            "positive": p.is_positive(), "bw": p.is_bw(),
            "target_print": p.info.target_print, "development_times": p.data.development_time,
            // Where the profile comes from, for Settings' Fotufilm films choice.
            "fotufilm": p.metadata.version.starts_with("fotufilm-port"),
            "manufacturer_data": p.metadata.manufacturer_data,
            // Prepared by Negs itself from datasheets (tools/film_profiles), for
            // Settings' Show Negs-built films, off by default.
            "negs_built": p.metadata.version.starts_with("negs-"),
            "grain": {
                "rms_granularity": grain.rms_granularity, "density_min": grain.density_min,
                "uniformity": grain.uniformity, "particle_scale_sublayers": grain.particle_scale_sublayers,
            },
            "dir_couplers": {
                "gamma_samelayer_rgb": dir.gamma_samelayer(), "gamma_interlayer_r_to_gb": dir.gamma_r_to_gb(),
                "gamma_interlayer_g_to_rb": dir.gamma_g_to_rb(), "gamma_interlayer_b_to_rg": dir.gamma_b_to_rg(),
            },
            "halation": {
                "halation_strength": halation.strength(), "halation_first_sigma_um": halation.first_sigma_um(),
            },
        }));
    }
    profiles.sort_by_key(|p| p["name"].as_str().unwrap_or_default().to_lowercase());
    let color_filters: Vec<Value> = filters
        .iter()
        .map(|(spec, curve)| {
            let [r, g, b] = crate::color_filters::glass_srgb(curve);
            json!({"key": spec.key, "name": spec.colour, "glass": spec.glass, "colour": format!("#{r:02x}{g:02x}{b:02x}")})
        })
        .collect();
    Ok(json!({"profiles": profiles, "color_filters": color_filters, "defaults": RuntimeParams::default(),
              "nullable_paths": RuntimeParams::nullable_paths()}))
}
