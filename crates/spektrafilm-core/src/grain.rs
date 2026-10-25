//! Grain inputs for a film: the per-stock presets (upstream
//! `_apply_grain_preset`) and the density curves the grain reads.
//!
//! The model itself is `spektrafilm_model::grain`; the CPU (`stages::filming::
//! develop`) and the GPU (`Pipeline::gpu_film_chain`) both resolve their
//! parameters here, so they always agree.

use std::sync::OnceLock;

use spektrafilm_model::grain::{GrainCurves, GrainSettings};

use crate::params::{GrainParams, RuntimeParams};
use crate::profile::Profile;

/// Upstream `data/presets/grain.toml`, compiled in so every pipeline
/// constructor sees it.
const PRESETS_TOML: &str = include_str!("../../../data/presets/grain.toml");

/// Upstream `GrainParams` schema defaults, used for stocks without a preset.
const SCHEMA_RMS_GRANULARITY: [f64; 3] = [6.0, 8.0, 10.0];
const SCHEMA_DENSITY_MIN: [f64; 3] = [0.03, 0.03, 0.03];
const SCHEMA_UNIFORMITY: [f64; 3] = [0.97, 0.97, 0.97];
const SCHEMA_PARTICLE_SCALE_SUBLAYERS: [f64; 3] = [1.0, 0.5, 0.25];

/// The four per-stock grain fields; `None` where a table doesn't set one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrainPreset {
    pub rms_granularity: Option<[f64; 3]>,
    pub density_min: Option<[f64; 3]>,
    pub uniformity: Option<[f64; 3]>,
    pub particle_scale_sublayers: Option<[f64; 3]>,
}

impl GrainPreset {
    fn from_table(table: &toml::Table) -> Self {
        // Upstream pads short arrays with None ("a single-value B&W
        // rms_granularity = [14.0] becomes (14.0, None, None)") and B&W
        // reads only channel 0. Repeating the last value fills the lanes.
        let field = |key: &str| -> Option<[f64; 3]> {
            let values: Vec<f64> = table
                .get(key)?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .collect();
            let last = *values.last()?;
            Some(std::array::from_fn(|i| values.get(i).copied().unwrap_or(last)))
        };
        Self {
            rms_granularity: field("rms_granularity"),
            density_min: field("density_min"),
            uniformity: field("uniformity"),
            particle_scale_sublayers: field("particle_scale_sublayers"),
        }
    }

    /// `self` where it sets a field, `other` elsewhere.
    fn or(&self, other: &Self) -> Self {
        Self {
            rms_granularity: self.rms_granularity.or(other.rms_granularity),
            density_min: self.density_min.or(other.density_min),
            uniformity: self.uniformity.or(other.uniformity),
            particle_scale_sublayers: self.particle_scale_sublayers.or(other.particle_scale_sublayers),
        }
    }
}

struct Presets {
    stocks: std::collections::HashMap<String, GrainPreset>,
    /// `defaults.<color|bw>.<negative|positive>`.
    defaults: std::collections::HashMap<(String, String), GrainPreset>,
}

fn presets() -> &'static Presets {
    static PRESETS: OnceLock<Presets> = OnceLock::new();
    PRESETS.get_or_init(|| {
        let root: toml::Table = PRESETS_TOML.parse().expect("data/presets/grain.toml parses");
        let mut stocks = std::collections::HashMap::new();
        let mut defaults = std::collections::HashMap::new();
        for (name, value) in &root {
            let Some(table) = value.as_table() else { continue };
            if name == "defaults" {
                for (channel_model, polarities) in table {
                    for (polarity, leaf) in polarities.as_table().into_iter().flatten() {
                        if let Some(leaf) = leaf.as_table() {
                            defaults.insert(
                                (channel_model.clone(), polarity.clone()),
                                GrainPreset::from_table(leaf),
                            );
                        }
                    }
                }
            } else {
                stocks.insert(name.clone(), GrainPreset::from_table(table));
            }
        }
        Presets { stocks, defaults }
    })
}

/// The film's grain preset as upstream `_apply_grain_preset` applies it: the
/// class defaults (`color|bw` × `negative|positive`) overridden by the stock's
/// own table. A stock without a table gets nothing (the schema defaults stand),
/// exactly as upstream returns early.
pub fn stock_preset(film: &Profile) -> GrainPreset {
    let presets = presets();
    let Some(stock) = film.info.stock.as_deref().and_then(|s| presets.stocks.get(s)) else {
        return GrainPreset::default();
    };
    let channel_model = if film.is_bw() { "bw" } else { "color" };
    let polarity = if film.is_positive() { "positive" } else { "negative" };
    let defaults = presets
        .defaults
        .get(&(channel_model.to_string(), polarity.to_string()))
        .cloned()
        .unwrap_or_default();
    stock.or(&defaults)
}

/// The fully resolved grain parameters for a film: values set in `params`,
/// else the stock preset, else upstream's schema defaults. B&W stocks are one
/// emulsion upstream (`match_channels` keeps channel 0); the three lanes
/// repeat it.
pub fn grain_settings(film: &Profile, params: &GrainParams) -> GrainSettings {
    let preset = stock_preset(film);
    let bw = |v: [f64; 3]| if film.is_bw() { [v[0]; 3] } else { v };
    GrainSettings {
        rms_granularity: bw(params
            .rms_granularity
            .or(preset.rms_granularity)
            .unwrap_or(SCHEMA_RMS_GRANULARITY)),
        density_min: bw(params.density_min.or(preset.density_min).unwrap_or(SCHEMA_DENSITY_MIN)),
        uniformity: bw(params.uniformity.or(preset.uniformity).unwrap_or(SCHEMA_UNIFORMITY)),
        particle_scale_sublayers: params
            .particle_scale_sublayers
            .or(preset.particle_scale_sublayers)
            .unwrap_or(SCHEMA_PARTICLE_SCALE_SUBLAYERS),
        blur: params.blur,
        mult_usm_sigma: params.mult_usm_sigma,
        mult_usm_amount: params.mult_usm_amount,
        blur_dye_clouds_um: params.blur_dye_clouds_um,
        micro_structure: params.micro_structure,
    }
}

/// The film's grain density floor, resolved like `grain_settings`. Upstream
/// also reads it, grain on or off, for the printing and scanning references
/// (`stages/printing.py:41`, `stages/scanning.py:71`).
pub fn density_min(film: &Profile, params: &RuntimeParams) -> [f64; 3] {
    grain_settings(film, &params.film_render.grain).density_min
}

/// Whether grain runs. Upstream `digest_params` switches grain off in
/// `preview_mode` (which Negs' apps don't use; their previews develop grain).
pub fn grain_active(params: &RuntimeParams) -> bool {
    params.film_render.grain.active && !params.settings.preview_mode
}

/// The curves the grain reads: the film's normalized total curves (as
/// `develop` uses them) and its per-sub-layer curves.
pub fn grain_curves(film: &Profile) -> GrainCurves {
    let total =
        spektrafilm_model::density_curves::normalize_density_curves_f64(&film.density_curves_f64());
    match film_density_curves_layers(film) {
        Some(layers) => GrainCurves::new(total, &layers),
        None => GrainCurves::single_layer(total),
    }
}

/// INTERIM, until exp-data lands: the per-sub-layer curves
/// (`[sample][sub-layer][channel]`), evaluated here from the profile's fitted
/// Gaussian-CDF model on its log-exposure grid (upstream
/// `evaluate_density_curves_layers`, `model/density_curves.py:117`). Upstream
/// takes them from `apply_print_curves_morph_with_layers`, so with Film Curves
/// on they carry the morph; these don't. Replace this function's body with
/// exp-data's `density_curves_layers` and nothing else changes. `None` for a
/// single-layer or missing model, as upstream.
fn film_density_curves_layers(film: &Profile) -> Option<Vec<Vec<[f64; 3]>>> {
    let model = film.data.density_curves_model.as_ref()?;
    if !matches!(model.model_type.as_str(), "cdfs" | "norm_cdfs") || model.n_channels() != 3 {
        return None;
    }
    let n_layers = model.n_layers();
    if n_layers <= 1
        || [&model.centers, &model.amplitudes, &model.sigmas]
            .iter()
            .any(|rows| rows.iter().any(|r| r.len() != n_layers))
    {
        return None;
    }
    let sign = if film.is_positive() { -1.0 } else { 1.0 };
    Some(
        film.data
            .log_exposure
            .iter()
            .map(|&x| {
                (0..n_layers)
                    .map(|l| {
                        std::array::from_fn(|ch| {
                            let z = sign * (x - model.centers[ch][l]) / model.sigmas[ch][l];
                            model.amplitudes[ch][l] * crate::print_morph::norm_cdf(z)
                        })
                    })
                    .collect()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(name: &str) -> Profile {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let p = crate::profile::load_profile_by_name(&data, name).unwrap();
        crate::profile::resolve_for_render(p, None)
    }

    #[test]
    fn presets_resolve_like_upstream() {
        let g = GrainParams::default();
        // Stock table over the colour-negative defaults.
        let s = grain_settings(&film("kodak_vision3_500t"), &g);
        assert_eq!(s.rms_granularity, [9.0, 13.0, 30.0]);
        assert_eq!(s.uniformity, [0.93, 0.97, 0.99]);
        assert_eq!(s.density_min, [0.12, 0.10, 0.35]);
        assert_eq!(s.particle_scale_sublayers, [1.0, 0.3, 0.25]);
        // Defaults fill what the stock leaves out.
        let s = grain_settings(&film("kodak_portra_400"), &g);
        assert_eq!(s.rms_granularity, [4.5; 3]);
        assert_eq!(s.particle_scale_sublayers, [1.0, 0.4, 0.25]);
        // B&W positive defaults, broadcast from channel 0.
        let s = grain_settings(&film("kodak_trix"), &g);
        assert_eq!((s.rms_granularity, s.density_min, s.uniformity), ([44.0; 3], [0.003; 3], [0.6; 3]));
        // No preset: upstream returns early and the schema defaults stand.
        let s = grain_settings(&film("kodak_verita_200d"), &g);
        assert_eq!(s.rms_granularity, [6.0, 8.0, 10.0]);
        assert_eq!(s.particle_scale_sublayers, [1.0, 0.5, 0.25]);
        // A value in the params overrides the preset.
        let g = GrainParams { rms_granularity: Some([1.0, 2.0, 3.0]), ..GrainParams::default() };
        assert_eq!(grain_settings(&film("kodak_portra_400"), &g).rms_granularity, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn model_layers_sum_to_the_model_total() {
        for name in ["kodak_portra_400", "fujifilm_provia_100f", "kodak_doublex"] {
            let f = film(name);
            let layers = film_density_curves_layers(&f).unwrap();
            let model = f.data.density_curves_model.as_ref().unwrap();
            let total = crate::print_morph::morph_density_curves(
                &f.data.log_exposure,
                model,
                &Default::default(),
                f.is_positive(),
            )
            .unwrap();
            for (row, t) in layers.iter().zip(&total) {
                for ch in 0..3 {
                    let sum: f64 = row.iter().map(|v| v[ch]).sum();
                    assert!((sum - t[ch]).abs() < 1e-12, "{name}");
                }
            }
        }
    }
}
