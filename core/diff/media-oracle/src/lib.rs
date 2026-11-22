//! Phase 3c's oracles (core/diff/oracle), frozen at 00b0d0d, with the
//! windows/src items they reach through `crate::` copied as they were there.
//! The embedded previews' oracle is negs-core-diff's own
//! (`negs_core_diff::embedded_preview`), as the bench uses it.

#[allow(dead_code)]
#[path = "../../oracle/photo_facts.rs"]
pub mod photo_facts;
#[allow(dead_code)]
#[path = "../../oracle/tone_graph.rs"]
pub mod tone_graph;
#[allow(dead_code)]
#[path = "../../oracle/help.rs"]
pub mod help;

/// windows/src/json.rs (00b0d0d): the helpers tone_graph uses.
#[allow(dead_code)]
pub mod json {
    use serde_json::{Map, Value};

    pub fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
        static NULL: Value = Value::Null;
        let mut current = value;
        for key in path.split('.') {
            current = match current {
                Value::Object(map) => map.get(key).unwrap_or(&NULL),
                Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)).unwrap_or(&NULL),
                _ => &NULL,
            };
        }
        current
    }

    pub fn set(value: &mut Value, path: &str, new: Value) {
        let keys: Vec<&str> = path.split('.').collect();
        set_at(value, &keys, new);
    }

    fn set_at(value: &mut Value, keys: &[&str], new: Value) {
        let Some((key, rest)) = keys.split_first() else {
            *value = new;
            return;
        };
        match value {
            Value::Object(map) => {
                let child = map.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
                set_at(child, rest, new);
            }
            Value::Array(items) => {
                if let Some(child) = key.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                    set_at(child, rest, new);
                }
            }
            _ => {}
        }
    }

    pub fn number(value: &Value) -> f64 {
        value.as_f64().unwrap_or(0.0)
    }

    pub fn boolean(value: &Value) -> bool {
        value.as_bool().unwrap_or(false)
    }

    pub fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
            }
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same(v, w)),
            _ => a == b,
        }
    }
}

/// windows/src/state.rs (00b0d0d): snap, and the grain lanes help reads.
pub mod state {
    /// Grain arrays that stay null until edited, so the film's preset from the
    /// catalog applies (`Stock.grainFields` in `Parameters.swift`).
    pub const GRAIN_STOCK_FIELDS: [&str; 4] = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];

    /// A slider on one lane of a grain preset array:
    /// `film_render.grain.<field>.<index>`.
    pub fn grain_lane(path: &str) -> Option<(&str, usize)> {
        let (field, index) = path.strip_prefix("film_render.grain.")?.split_once('.')?;
        GRAIN_STOCK_FIELDS.contains(&field).then_some(())?;
        Some((field, index.parse().ok()?))
    }

    /// Sliders move in steps; the engine expects the stepped value (integer
    /// fields reject fractions). Like `EditorModel.snap`.
    pub fn snap(value: f64, low: f64, high: f64, step: f64) -> f64 {
        let clamped = value.clamp(low, high);
        let steps = ((clamped - low) / step).round();
        let decimals = ((-step.log10()).floor() as i32 + 1).max(0);
        let scale = 10f64.powi(decimals);
        (((low + steps * step) * scale).round() / scale).min(high)
    }
}

/// windows/src/negative.rs (00b0d0d): the backlight's path.
pub mod negative {
    pub const SCAN_ILLUMINANT: &str = "film_render.convert.scan_illuminant";
}
