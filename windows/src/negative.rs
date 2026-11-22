//! Your negative (ticket 33, `Negative.swift`): a scan of a real colour
//! negative, read back to the film's dyes with upstream's convert-film
//! route, then printed. The words and rules the label, the tray and Prep
//! share; no Win32 here, so `windows/library-map` tests them on any OS.
//!
//! The pure rules are the shared core's (core/src/rules/negative, through
//! negs_core_rules), the constants Rust `const`s the core holds the same
//! (core/diff/tests/negative.rs). What changes an edit state stays here.

use serde_json::Value;

pub use negs_core_rules::negative::*;

use crate::json;
use crate::state::{EditState, Roll, Stock};

/// `target` joining `roll`, printed like `reference` (`NegativeScan.joining`):
/// the read-as film, paper, backlight, how the scan is read and base
/// tuning are the reference frame's, and the filtration the roll's unless
/// `target` is a frame of this roll that set its own. Its print exposure,
/// crop and own base are measured for it afterwards.
pub fn joining(roll: &Roll, reference: &EditState, target: &EditState) -> EditState {
    let mut next = target.clone();
    next.film = reference.film.clone();
    next.paper = reference.paper.clone();
    for path in ["workflow", "film_render.base", "film_render.convert", "film_render.development_time", "print_render.development_time"].into_iter().chain(SCAN_READING) {
        json::set(&mut next.params, path, reference.value(path).clone());
    }
    let own = target.roll.as_ref().is_some_and(|r| r.id == roll.id && !r.filtered(target));
    if !own {
        json::set(&mut next.params, "enlarger.m_filter_shift", Value::from(roll.magenta));
        json::set(&mut next.params, "enlarger.y_filter_shift", Value::from(roll.yellow));
    }
    json::set(&mut next.params, "io.scan_film", Value::Bool(false));
    next.brightness_match = None;
    next.roll = Some(roll.clone());
    next
}

pub fn is_negative(state: &EditState) -> bool {
    is_route(json::string(state.value(ROUTE_PATH)))
}

/// A film a negative can be read as: a colour negative.
pub fn readable(stock: &Stock) -> bool {
    readable_kind(stock.paper, stock.positive, stock.bw)
}

/// `state` printed as a scene again: what reading a negative set, back to
/// `defaults` (the catalog's). The crop to the picture stays: it's geometry.
pub fn leaving(state: &EditState, defaults: &Value) -> EditState {
    let mut next = state.clone();
    for path in OWN_PATHS {
        let value = json::get(defaults, path);
        let value = if value.is_null() && path == ROUTE_PATH { Value::String(SCENE_ROUTE.into()) } else { value.clone() };
        if !value.is_null() {
            json::set(&mut next.params, path, value);
        }
    }
    next.roll = None;
    next
}

/// `applied` (a preset over `current`) with `current`'s negative kept.
pub fn keeping_negative(applied: &EditState, current: &EditState) -> EditState {
    if !is_negative(current) {
        return applied.clone();
    }
    let mut next = applied.clone();
    next.film = current.film.clone();
    for path in PRESET_KEEPS {
        json::set(&mut next.params, path, current.value(path).clone());
    }
    json::set(&mut next.params, "io.scan_film", Value::Bool(false));
    next.roll = current.roll.clone();
    next
}

/// What a base was found for: changing any of it finds it again. The paper
/// isn't: a new paper only prints it as Auto again.
pub fn key(state: &EditState) -> Option<String> {
    if !is_negative(state) {
        return None;
    }
    Some(
        [
            state.film.clone(),
            json::string(state.value(SCAN_ILLUMINANT)).to_string(),
            state.value("io.raw_white_balance").to_string(),
            state.value("io.input_interpretation").to_string(),
            state.value("io.lens_correction").to_string(),
            json::string(state.value("film_render.convert.flat_field")).to_string(),
        ]
        .join("|"),
    )
}
