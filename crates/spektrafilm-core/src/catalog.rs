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
        profiles.push(json!({
            "id": stock, "name": p.info.name.as_ref().unwrap_or(&stock),
            "paper": p.info.support == "paper" || p.info.stage == "printing",
            "positive": p.is_positive(), "bw": p.is_bw(),
            "target_print": p.info.target_print, "development_times": p.data.development_time,
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
    Ok(json!({"profiles": profiles, "defaults": RuntimeParams::default(), "nullable_paths": RuntimeParams::nullable_paths()}))
}
