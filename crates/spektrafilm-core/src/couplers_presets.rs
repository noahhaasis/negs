//! DIR-coupler presets from `data/presets/couplers.toml` (upstream
//! experimental's per-stock gammas, CC BY-SA 4.0), embedded at build time.
//!
//! Port of `_apply_couplers_preset` in `runtime/params_builder.py` at
//! 28bf883: a `defaults.<color|bw>.<positive|negative>` table seeds the
//! film's class, colour negative cine stocks then take the
//! `cine.<tungsten|daylight>` branch (tungsten when the reference illuminant
//! starts with "T"), and a per-stock table overrides on top. As upstream,
//! only keys that name a `DirCouplersParams` field apply.
//!
//! The preset fills only fields left unset (`None`). Upstream applies it when
//! the stock is chosen and a saved GUI state then overrides it
//! (`params_mapper.py` replaces the whole couplers group after
//! `init_params`), so a value set in params wins here too.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::params::DirCouplersParams;
use crate::profile::Profile;

const COUPLERS_TOML: &str = include_str!("../../../data/presets/couplers.toml");

fn presets() -> &'static Map<String, Value> {
    static PRESETS: OnceLock<Map<String, Value>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        let table: toml::Table = toml::from_str(COUPLERS_TOML).expect("couplers.toml parses");
        match serde_json::to_value(table).expect("couplers.toml converts to JSON") {
            Value::Object(map) => map,
            _ => unreachable!("a TOML document is a table"),
        }
    })
}

fn table<'a>(map: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Map<String, Value>> {
    path.iter()
        .try_fold(map, |map, key| map.get(*key)?.as_object())
}

/// The fields a preset may set: `DirCouplersParams`' optional gammas.
const PRESET_FIELDS: [&str; 4] = [
    "gamma_samelayer_rgb",
    "gamma_interlayer_r_to_gb",
    "gamma_interlayer_g_to_rb",
    "gamma_interlayer_b_to_rg",
];

/// Fill `dir`'s unset gammas with the preset values for `film`.
pub fn apply_couplers_preset(film: &Profile, dir: &mut DirCouplersParams) {
    let presets = presets();
    let channel_model = if film.is_bw() { "bw" } else { "color" };
    let polarity = if film.is_positive() {
        "positive"
    } else {
        "negative"
    };
    let mut fields = match serde_json::to_value(&*dir).expect("DirCouplersParams serializes") {
        Value::Object(map) => map,
        _ => unreachable!("DirCouplersParams is a struct"),
    };
    let mut merged = Map::new();
    let mut merge = |source: Option<&Map<String, Value>>| {
        for (key, value) in source.into_iter().flatten() {
            if PRESET_FIELDS.contains(&key.as_str()) && fields.get(key) == Some(&Value::Null) {
                merged.insert(key.clone(), value.clone());
            }
        }
    };
    let defaults = table(presets, &["defaults", channel_model, polarity]);
    merge(defaults);
    if channel_model == "color" && polarity == "negative" && film.info.usage == "cine" {
        let tungsten = film
            .info
            .reference_illuminant
            .to_uppercase()
            .starts_with('T');
        let branch = if tungsten { "tungsten" } else { "daylight" };
        merge(defaults.and_then(|d| table(d, &["cine", branch])));
    }
    if let Some(stock) = film.info.stock.as_deref() {
        merge(table(presets, &[stock]));
    }
    if merged.is_empty() {
        return;
    }
    fields.extend(merged);
    *dir = serde_json::from_value(Value::Object(fields))
        .expect("couplers.toml values fit DirCouplersParams");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse_with_every_class() {
        let presets = presets();
        for class in [
            ["color", "positive"],
            ["color", "negative"],
            ["bw", "positive"],
            ["bw", "negative"],
        ] {
            assert!(table(presets, &["defaults", class[0], class[1]]).is_some());
        }
        for branch in ["daylight", "tungsten"] {
            assert!(table(presets, &["defaults", "color", "negative", "cine", branch]).is_some());
        }
    }

    /// Every value the TOML sets must be one `apply_couplers_preset` fills;
    /// a new key would otherwise be dropped silently.
    #[test]
    fn presets_set_only_optional_gammas() {
        fn walk(map: &Map<String, Value>, path: &str) {
            for (key, value) in map {
                match value {
                    Value::Object(child) => walk(child, &format!("{path}.{key}")),
                    _ => assert!(PRESET_FIELDS.contains(&key.as_str()), "{path}.{key}"),
                }
            }
        }
        walk(presets(), "couplers.toml");
        let unset = serde_json::to_value(DirCouplersParams::default()).unwrap();
        for key in PRESET_FIELDS {
            assert_eq!(unset[key], Value::Null, "{key} is unset by default");
        }
    }
}
