//! windows/src/negative.rs's pure rules on the shared Odin core
//! (core/src/rules/negative). Your negative (ticket 33, `Negative.swift`):
//! a scan of a real colour negative, read back to the film's dyes with
//! upstream's convert-film route, then printed.
//!
//! What runs where:
//! - In Odin: the words and the rules that take plain values (scan stops,
//!   the found paths, the picture's crop, the flat field's row, the shown
//!   edge, the base's sentence, the picks' prompts, a base number's
//!   tolerance, the route and the readable stocks).
//! - In Rust: the constants, as `const`s (a test holds them to the core's),
//!   reading the helper's JSON answers into plain values, and the JSON
//!   object walk of `same_base`. What changes an edit state (`joining`,
//!   `leaving`, `keeping_negative`, `key`, and `is_negative`/`readable` on
//!   the state's types) stays in windows/src/negative.rs: Phase 4's.

use negs_core_sys::picture as sys;
use negs_core_sys::take_buf;
use serde_json::Value;

use crate::crop::{CropSettings, Orientation, static_str};

/// Upstream's `workflow.route` strings.
pub const ROUTE: &str = "input > convert-film > print > scan";
pub const SCENE_ROUTE: &str = "input > film > print > scan";
pub const ROUTE_PATH: &str = "workflow.route";
/// Read as this unless the photograph's film is a colour negative already.
pub const DEFAULT_FILM: &str = "kodak_portra_400";
/// The tray's "Your negative" tile; never a stock the helper knows.
pub const STOCK_ID: &str = "negs.your-negative";
pub const NAME: &str = "Your negative";
pub const TILE_TEXT: &str = "Read the photograph as a scan of a colour negative";

/// What reading a negative sets, put back to the defaults when the
/// photograph is printed as a scene again.
pub const OWN_PATHS: [&str; 6] = [ROUTE_PATH, "film_render.base", "film_render.convert", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"];

/// What finding the base sets: this frame's measurements.
pub const FOUND_PATHS: [&str; 5] = ["film_render.base", "film_render.convert.exposure_compensation_ev", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"];

/// What a preset never changes on a negative (decision 6): how it's read
/// (the film, base, backlight, route and how the file is read: a RAW's
/// white balance and lens correction, a TIFF's interpretation) and its
/// print balance. The preset brings the look and the paper.
pub const PRESET_KEEPS: [&str; 9] = [
    "workflow", "film_render.base", "film_render.convert", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure",
    "io.raw_white_balance", "io.lens_correction", "io.input_interpretation",
];

pub const SCAN_ILLUMINANT: &str = "film_render.convert.scan_illuminant";

/// How the scan is read from the file: a RAW's white balance and lens
/// correction, a TIFF's interpretation. A roll's frames share it.
pub const SCAN_READING: [&str; 3] = ["io.raw_white_balance", "io.lens_correction", "io.input_interpretation"];

/// What a camera recorded of a scan's light: shutter (seconds), aperture
/// and ISO.
pub type ScanLight = (Option<f64>, Option<f64>, Option<f64>);

fn light(l: ScanLight) -> sys::negs_scan_light_t {
    let parts = [l.0, l.1, l.2];
    sys::negs_scan_light_t { value: parts.map(|v| v.unwrap_or(0.0)), has: parts.map(|v| v.is_some() as u8), reserved: [0; 5] }
}

/// How much more light the camera gave `frame`'s scan than `reference`'s,
/// in stops, from what both recorded (a lens without contacts records no
/// aperture); None when they share none (a scanner's TIFF: the same light
/// each time).
pub fn scan_stops(frame: ScanLight, reference: ScanLight) -> Option<f64> {
    let mut out = 0.0;
    (unsafe { sys::negs_negative_scan_stops(light(frame), light(reference), &mut out) } != 0).then_some(out)
}

/// The base tuning's exposure for a frame without clear film of its own:
/// the roll's, less the extra light its scan got.
pub fn exposure_ev(roll: f64, scan_stops: Option<f64>) -> f64 {
    unsafe { sys::negs_negative_exposure_ev(roll, scan_stops.is_some() as u8, scan_stops.unwrap_or(0.0)) }
}

fn text(buf: negs_core_sys::negs_buf_t) -> String {
    String::from_utf8_lossy(&take_buf(buf)).into_owned()
}

/// Use for whole roll's word for the photographs it left alone.
pub fn skipped(count: usize) -> String {
    text(unsafe { sys::negs_negative_skipped(count as u64) })
}

/// A `workflow.route` that reads the photograph as a negative.
pub fn is_route(route: &str) -> bool {
    unsafe { sys::negs_negative_is_route(route.as_ptr(), route.len()) != 0 }
}

/// A film a negative can be read as: a colour negative, from the stock's
/// kind.
pub fn readable_kind(paper: bool, positive: bool, bw: bool) -> bool {
    unsafe { sys::negs_negative_readable(paper as u8, positive as u8, bw as u8) != 0 }
}

/// `path` is, holds or lies in one of `FOUND_PATHS`.
pub fn is_found(path: &str) -> bool {
    unsafe { sys::negs_negative_is_found(path.as_ptr(), path.len()) != 0 }
}

/// The crop to the picture the rebate finder found (`negative_base`'s
/// `picture`, in the decoded photograph's frame like the crop). None for none.
pub fn crop(picture: &Value) -> Option<CropSettings> {
    if !picture.is_object() {
        return None;
    }
    let n = |k: &str| picture[k].as_f64().unwrap_or(f64::NAN);
    let angle = picture["angle_deg"].as_f64().unwrap_or(0.0);
    let mut out = sys::negs_crop_settings_t::default();
    (unsafe { sys::negs_negative_picture_crop(n("x0"), n("y0"), n("x1"), n("y1"), angle, &mut out) } != 0).then(|| CropSettings::of(&out))
}

/// The picture as `negative_auto` and `negative_meter` bound it.
pub fn bounds(picture: &Value) -> Option<Value> {
    crop(picture)?;
    Some(Value::Array(["x0", "y0", "x1", "y1"].iter().map(|k| picture[*k].clone()).collect()))
}

/// `film_render.convert.flat_field`: the light frame each scan is divided
/// by (§9.4), a path or empty for off. It sits with the backlight in the
/// negative's convert settings, so it goes where they go (the roll).
pub const FLAT_FIELD: &str = "film_render.convert.flat_field";

/// Prep's Flat field row: the roll's light frame, and whether the helper
/// used it on this frame (`NegativeScan.FlatField`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatField {
    /// The light frame's path; empty is off.
    pub path: String,
    /// The helper's answer for this frame and this light frame: None until it has said.
    pub used: Option<bool>,
    pub reason: Option<String>,
}

impl FlatField {
    pub const HELP: &'static str = "A frame of the bare light, shot with the roll's camera, lens and aperture. Each scan is divided by it before the base is read, so the light's falloff doesn't print.";

    /// `answer` is a render's or a base detection's `flat_field` (`{path,
    /// used, reason}`); one for another light frame is ignored.
    pub fn of(path: &str, answer: &Value) -> Self {
        let mut flat = Self { path: path.to_string(), ..Self::default() };
        if !path.is_empty() && answer["path"].as_str() == Some(path) {
            flat.used = Some(answer["used"].as_bool().unwrap_or(false));
            flat.reason = Some(answer["reason"].as_str().unwrap_or("").to_string()).filter(|r| !r.is_empty());
        }
        flat
    }

    pub fn is_on(&self) -> bool {
        !self.path.is_empty()
    }

    /// The row's value: the light frame's name, or Off.
    pub fn value(&self) -> String {
        text(unsafe { sys::negs_flat_field_value(self.path.as_ptr(), self.path.len()) })
    }

    /// Under the row: why this frame doesn't use it.
    pub fn hint(&self) -> Option<String> {
        let used = match self.used {
            None => -1,
            Some(false) => 0,
            Some(true) => 1,
        };
        let reason = self.reason.as_deref().unwrap_or("");
        let mut out = negs_core_sys::negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        let some = unsafe {
            sys::negs_flat_field_hint(self.path.as_ptr(), self.path.len(), used, self.reason.is_some() as u8, reason.as_ptr(), reason.len(), &mut out)
        };
        (some != 0).then(|| text(out))
    }
}

/// Prep's word when the scan doesn't look like a colour negative.
pub fn not_negative(raw: bool) -> &'static str {
    static_str(unsafe { sys::negs_negative_not_negative(raw as u8) })
}

/// The photograph's edge ("left", "top"…) as `orientation` shows it.
pub fn shown_edge(orientation: Orientation, edge: &str) -> String {
    let p = unsafe { sys::negs_negative_shown_edge(orientation.c(), edge.as_ptr(), edge.len()) };
    if p.is_null() { edge.to_string() } else { static_str(p).to_string() }
}

/// Where the film base shown came from (`NegativeScan.BaseStatus`).
#[derive(Clone, Debug, PartialEq)]
pub struct BaseStatus {
    pub found: bool,
    pub picked: bool,
    pub edge: Option<String>,
    /// A base no detection made while the photograph is open: picked
    /// before, or set in Lab.
    pub by_hand: bool,
    /// The roll's base, on a frame without clear film of its own.
    pub from_roll: bool,
}

impl BaseStatus {
    pub fn of(result: &Value) -> Self {
        let edge = result["edge"].as_str().unwrap_or("");
        Self {
            found: result["found"].as_bool().unwrap_or(false),
            picked: result["picked"].as_bool().unwrap_or(false),
            edge: (!edge.is_empty()).then(|| edge.to_string()),
            by_hand: false,
            from_roll: false,
        }
    }

    /// The base is a guess from the picture: no clear film was found.
    pub fn guessed(&self) -> bool {
        unsafe { sys::negs_base_status_guessed(self.found as u8, self.picked as u8, self.by_hand as u8, self.from_roll as u8) != 0 }
    }

    pub fn sentence(&self, orientation: Orientation) -> String {
        let edge = self.edge.as_deref().unwrap_or("");
        text(unsafe {
            sys::negs_base_status_sentence(
                self.found as u8,
                self.picked as u8,
                self.by_hand as u8,
                self.from_roll as u8,
                self.edge.is_some() as u8,
                edge.as_ptr(),
                edge.len(),
                orientation.c(),
            )
        })
    }
}

/// What a click on the photograph reads while picking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A click on clear film (the rebate) reads the base there.
    Base,
    /// A click on something grey balances the print on it.
    Neutral,
}

impl Pick {
    pub fn prompt(self) -> &'static str {
        let id = match self {
            Pick::Base => sys::NEGS_PICK_BASE,
            Pick::Neutral => sys::NEGS_PICK_NEUTRAL,
        };
        static_str(unsafe { sys::negs_pick_prompt(id) })
    }
}

/// `a` and `b` are the same film base, allowing for a number's last bit.
pub fn same_base(a: &Value, b: &Value) -> bool {
    match (a.as_object(), b.as_object()) {
        (Some(x), Some(y)) => x.keys().chain(y.keys()).all(|k| {
            let (p, q) = (x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null));
            match (p.as_f64(), q.as_f64()) {
                (Some(p), Some(q)) => unsafe { sys::negs_negative_same_number(p, q) != 0 },
                _ => p == q,
            }
        }),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_text(which: u32) -> &'static str {
        static_str(unsafe { sys::negs_negative_text(which) })
    }

    fn core_paths(list: u32) -> Vec<&'static str> {
        (0..).map_while(|i| { let p = unsafe { sys::negs_negative_path(list, i) }; (!p.is_null()).then(|| static_str(p)) }).collect()
    }

    /// The constants stay Rust `const`s (help.rs reads SCAN_ILLUMINANT); the
    /// core holds the same.
    #[test]
    fn the_constants_are_the_cores() {
        assert_eq!(core_text(sys::NEGS_NEGATIVE_ROUTE), ROUTE);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_SCENE_ROUTE), SCENE_ROUTE);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_ROUTE_PATH), ROUTE_PATH);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_DEFAULT_FILM), DEFAULT_FILM);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_STOCK_ID), STOCK_ID);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_NAME), NAME);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_TILE_TEXT), TILE_TEXT);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_SCAN_ILLUMINANT), SCAN_ILLUMINANT);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_FLAT_FIELD), FLAT_FIELD);
        assert_eq!(core_text(sys::NEGS_NEGATIVE_FLAT_FIELD_HELP), FlatField::HELP);
        assert!(unsafe { sys::negs_negative_text(10) }.is_null());
        assert_eq!(core_paths(sys::NEGS_NEGATIVE_OWN_PATHS), OWN_PATHS);
        assert_eq!(core_paths(sys::NEGS_NEGATIVE_FOUND_PATHS), FOUND_PATHS);
        assert_eq!(core_paths(sys::NEGS_NEGATIVE_PRESET_KEEPS), PRESET_KEEPS);
        assert_eq!(core_paths(sys::NEGS_NEGATIVE_SCAN_READING), SCAN_READING);
        assert_eq!(SCAN_ILLUMINANT, "film_render.convert.scan_illuminant");
    }
}
