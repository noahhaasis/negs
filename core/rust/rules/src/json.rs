//! windows/src/json.rs on the shared core (core/src/value): the engine's
//! parameters as serde_json Values. The logic runs in Odin; values cross as
//! serde_json's own text, which reads back to the same Value.
//!
//! Kept in Rust because they return borrows into the caller's Value:
//! `get` and `string`.

use negs_core_sys::state as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde_json::Value;

pub(crate) fn text(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("a serde_json Value always writes")
}

/// The Value of JSON text the core wrote.
pub(crate) fn read(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("the core writes JSON serde_json reads")
}

/// A buf the core filled (`ok` its return), as a Value.
pub(crate) fn take(ok: u8, buf: negs_buf_t) -> Value {
    let bytes = take_buf(buf);
    assert!(ok == 1, "the core refused serde_json's own text");
    read(&bytes)
}

pub(crate) fn empty() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

pub(crate) fn strs(list: &[String]) -> Vec<sys::negs_str_t> {
    list.iter().map(|s| sys::negs_str_t { data: s.as_ptr(), len: s.len() }).collect()
}

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
    let (tree, item) = (text(value), text(&new));
    let mut out = empty();
    let ok = unsafe { sys::negs_json_set(tree.as_ptr(), tree.len(), path.as_ptr(), path.len(), item.as_ptr(), item.len(), &mut out) };
    *value = take(ok, out);
}

pub fn number(value: &Value) -> f64 {
    let t = text(value);
    unsafe { sys::negs_json_number(t.as_ptr(), t.len()) }
}

pub fn boolean(value: &Value) -> bool {
    let t = text(value);
    unsafe { sys::negs_json_boolean(t.as_ptr(), t.len()) == 1 }
}

pub fn string(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// A number as the macOS app's JSONEncoder writes it: whole numbers without
/// a fraction, so integer parameters accept them.
pub fn num(v: f64) -> Value {
    let mut out = [0u8; 64];
    let n = unsafe { sys::negs_json_num(v, out.as_mut_ptr(), out.len()) };
    read(&out[..n])
}

/// Equality with numbers compared as doubles, like the macOS app's
/// `JSONValue`: `1` and `1.0` are the same value.
pub fn same(a: &Value, b: &Value) -> bool {
    let (x, y) = (text(a), text(b));
    unsafe { sys::negs_json_same(x.as_ptr(), x.len(), y.as_ptr(), y.len()) == 1 }
}

/// Saved values merged into the current defaults: new engine fields keep
/// their defaults, known fields are restored, removed fields are dropped.
/// A saved null stays where `nullable` (the catalog's `nullable_paths`, `[*]`
/// for every element) says it is a value, such as an infinite Langmuir knee.
pub fn restoring(defaults: &Value, saved: &Value, nullable: &[String]) -> Value {
    let (d, s, n) = (text(defaults), text(saved), strs(nullable));
    let mut out = empty();
    let ok = unsafe { sys::negs_json_restoring(d.as_ptr(), d.len(), s.as_ptr(), s.len(), n.as_ptr(), n.len(), &mut out) };
    take(ok, out)
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
        assert_eq!(restoring(&defaults, &saved, &[]), json!({"a": 5, "b": {"c": false, "d": [1, 2]}, "e": 3, "f": "x"}));
    }

    #[test]
    fn restoring_keeps_saved_nulls_where_they_are_values() {
        let defaults = json!({"dir": {"langmuir": [1, 1, 1], "gamma": null}, "gamut": {"lightness": [0.95, 1, 1.6], "knee": 1}});
        let saved = json!({"dir": {"langmuir": [null, 0.5, null], "gamma": [0.52, 0.51, 0.26]}, "gamut": {"lightness": null, "knee": null}});
        let nullable = ["dir.langmuir[*]", "dir.gamma", "gamut.lightness"].map(String::from);
        assert_eq!(
            restoring(&defaults, &saved, &nullable),
            json!({"dir": {"langmuir": [null, 0.5, null], "gamma": [0.52, 0.51, 0.26]}, "gamut": {"lightness": null, "knee": 1}})
        );
        assert_eq!(restoring(&defaults, &saved, &[])["dir"]["langmuir"], json!([1, 0.5, 1]));
    }

    #[test]
    fn scalars_follow_serde() {
        assert_eq!((number(&json!(2.5)), number(&json!("x")), number(&json!(-3))), (2.5, 0.0, -3.0));
        assert!(boolean(&json!(true)) && !boolean(&json!(1)));
        assert_eq!(string(&json!("film")), "film");
        assert_eq!(num(f64::NAN), Value::Null);
        assert_eq!(num(-0.0).to_string(), "0");
        assert_eq!(num(1e16).to_string(), "1e+16");
    }
}
