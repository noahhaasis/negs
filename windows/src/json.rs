//! The engine's parameters as JSON, keeping the full Rust schema, including
//! fields the inspector doesn't expose. Defaults come from the engine, so
//! adding a Rust parameter cannot reset it here. Mirrors `JSONValue` in the
//! macOS app (`Parameters.swift`).

use serde_json::{Map, Value};

/// The value at a dotted path such as `film_render.grain.active`; array
/// elements by index (`scanner.unsharp_mask.0`). Null when absent.
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

/// Sets the value at a dotted path, creating objects on the way. An array
/// index out of range, or a path through a scalar, changes nothing.
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

pub fn string(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// A number as the macOS app's JSONEncoder writes it: whole numbers without
/// a fraction, so integer parameters accept them.
pub fn num(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

/// Equality with numbers compared as doubles, like the macOS app's
/// `JSONValue`: `1` and `1.0` are the same value.
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

/// Saved values merged into the current defaults: new engine fields keep
/// their defaults, known fields are restored, removed fields are dropped.
pub fn restoring(defaults: &Value, saved: &Value) -> Value {
    match (defaults, saved) {
        (Value::Object(d), Value::Object(s)) => Value::Object(
            d.iter()
                .map(|(k, v)| (k.clone(), s.get(k).map_or_else(|| v.clone(), |saved| restoring(v, saved))))
                .collect(),
        ),
        (Value::Array(d), Value::Array(s)) if d.len() == s.len() => {
            Value::Array(d.iter().zip(s).map(|(v, w)| restoring(v, w)).collect())
        }
        (Value::Number(_), Value::Number(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::String(_), Value::String(_))
        | (Value::Null, _) => saved.clone(),
        _ => defaults.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_reach_objects_and_arrays() {
        let mut v = json!({"scanner": {"unsharp_mask": [0.5, 1.0]}, "io": {"rotate_cw": 0}});
        assert_eq!(get(&v, "scanner.unsharp_mask.1"), &json!(1.0));
        assert_eq!(get(&v, "missing.path"), &Value::Null);
        set(&mut v, "scanner.unsharp_mask.0", json!(2));
        set(&mut v, "io.new.deep", json!(true));
        set(&mut v, "scanner.unsharp_mask.7", json!(3));
        assert_eq!(v, json!({"scanner": {"unsharp_mask": [2, 1.0]}, "io": {"rotate_cw": 0, "new": {"deep": true}}}));
    }

    #[test]
    fn whole_numbers_are_integers_and_compare_as_doubles() {
        assert_eq!(num(2.0).to_string(), "2");
        assert_eq!(num(0.25).to_string(), "0.25");
        assert!(same(&json!({"a": [1, 2.5]}), &json!({"a": [1.0, 2.5]})));
        assert!(!same(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    }

    #[test]
    fn restoring_keeps_new_defaults_and_drops_removed_fields() {
        let defaults = json!({"a": 1, "b": {"c": true, "d": [1, 2]}, "e": null, "f": "x"});
        let saved = json!({"a": 5, "b": {"c": false, "d": [1, 2, 3], "gone": 1}, "e": 3, "f": 7, "removed": 1});
        assert_eq!(restoring(&defaults, &saved), json!({"a": 5, "b": {"c": false, "d": [1, 2]}, "e": 3, "f": "x"}));
    }
}
