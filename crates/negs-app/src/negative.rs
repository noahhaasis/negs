//! Your negative (ticket 33, `Negative.swift`): a scan of a real colour
//! negative, read back to the film's dyes with upstream's convert-film
//! route, then printed. The words and rules the label, the tray and Prep
//! share; no Win32 here, so `windows/library-map` tests them on any OS.

use serde_json::Value;

use crate::crop::{CropAspect, CropSettings, Orientation, Point, Rect};
use crate::json;
use crate::state::{EditState, Roll, Stock};

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

// MARK: Darkroom roll (§9.2)

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

/// What a camera recorded of a scan's light: shutter (seconds), aperture
/// and ISO.
pub type ScanLight = (Option<f64>, Option<f64>, Option<f64>);

/// How much more light the camera gave `frame`'s scan than `reference`'s,
/// in stops, from what both recorded (a lens without contacts records no
/// aperture); None when they share none (a scanner's TIFF: the same light
/// each time).
pub fn scan_stops(frame: ScanLight, reference: ScanLight) -> Option<f64> {
    let mut stops: Option<f64> = None;
    let mut add = |value: Option<f64>, base: Option<f64>, term: fn(f64, f64) -> f64| {
        if let (Some(value), Some(base)) = (value, base) {
            *stops.get_or_insert(0.0) += term(value, base);
        }
    };
    add(frame.0, reference.0, |v, b| (v / b).log2());
    add(frame.1, reference.1, |v, b| 2.0 * (b / v).log2());
    add(frame.2, reference.2, |v, b| (v / b).log2());
    stops
}

/// The base tuning's exposure for a frame without clear film of its own:
/// the roll's, less the extra light its scan got.
pub fn exposure_ev(roll: f64, scan_stops: Option<f64>) -> f64 {
    roll - scan_stops.unwrap_or(0.0)
}

/// Use for whole roll's word for the photographs it left alone.
pub fn skipped(count: usize) -> String {
    if count == 1 { "1 photograph skipped: it doesn't look like a negative.".into() } else { format!("{count} photographs skipped: they don't look like negatives.") }
}

pub fn is_negative(state: &EditState) -> bool {
    json::string(state.value(ROUTE_PATH)) == ROUTE
}

/// A film a negative can be read as: a colour negative.
pub fn readable(stock: &Stock) -> bool {
    !stock.paper && !stock.positive && !stock.bw
}

/// `path` is, holds or lies in one of `FOUND_PATHS`.
pub fn is_found(path: &str) -> bool {
    FOUND_PATHS.iter().any(|f| *f == path || path.starts_with(&format!("{f}.")) || f.starts_with(&format!("{path}.")))
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

/// The crop to the picture the rebate finder found (`negative_base`'s
/// `picture`, in the decoded photograph's frame like the crop). None for none.
pub fn crop(picture: &Value) -> Option<CropSettings> {
    if !picture.is_object() {
        return None;
    }
    let n = |k: &str| picture[k].as_f64().unwrap_or(f64::NAN);
    let (x0, y0, x1, y1) = (n("x0"), n("y0"), n("x1"), n("y1"));
    let ok = [x0, y0, x1, y1].iter().all(|v| v.is_finite()) && 0.0 <= x0 && x0 < x1 && x1 <= 1.0 && 0.0 <= y0 && y0 < y1 && y1 <= 1.0;
    if !ok {
        return None;
    }
    let angle = picture["angle_deg"].as_f64().unwrap_or(0.0).clamp(-crate::crop::MAX_ANGLE, crate::crop::MAX_ANGLE);
    Some(CropSettings { rect: Rect::new(x0, y0, x1 - x0, y1 - y0), aspect: CropAspect::Freeform, angle, ..Default::default() })
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
            flat.used = Some(json::boolean(&answer["used"]));
            flat.reason = Some(json::string(&answer["reason"]).to_string()).filter(|r| !r.is_empty());
        }
        flat
    }

    pub fn is_on(&self) -> bool {
        !self.path.is_empty()
    }

    /// The row's value: the light frame's name, or Off.
    pub fn value(&self) -> String {
        if !self.is_on() {
            return "Off".into();
        }
        let name = self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path);
        name.to_string()
    }

    /// Under the row: why this frame doesn't use it.
    pub fn hint(&self) -> Option<String> {
        (self.is_on() && self.used == Some(false)).then(|| self.reason.clone().unwrap_or_else(|| "The light frame isn't used on this frame.".into()))
    }
}

/// Prep's word when the scan doesn't look like a colour negative.
pub fn not_negative(raw: bool) -> &'static str {
    if raw { "This doesn't look like a colour negative. If it is, set White balance to Daylight." } else { "This doesn't look like a colour negative." }
}

/// The photograph's edge ("left", "top"…) as `orientation` shows it.
pub fn shown_edge(orientation: Orientation, edge: &str) -> String {
    let point = match edge {
        "left" => Point { x: 0.0, y: 0.5 },
        "right" => Point { x: 1.0, y: 0.5 },
        "top" => Point { x: 0.5, y: 0.0 },
        "bottom" => Point { x: 0.5, y: 1.0 },
        _ => return edge.to_string(),
    };
    let p = orientation.shown(point);
    (if p.x < 0.25 { "left" } else if p.x > 0.75 { "right" } else if p.y < 0.25 { "top" } else { "bottom" }).to_string()
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
        let edge = json::string(&result["edge"]);
        Self { found: json::boolean(&result["found"]), picked: json::boolean(&result["picked"]), edge: (!edge.is_empty()).then(|| edge.to_string()), by_hand: false, from_roll: false }
    }

    /// The base is a guess from the picture: no clear film was found.
    pub fn guessed(&self) -> bool {
        !self.found && !self.picked && !self.by_hand && !self.from_roll
    }

    pub fn sentence(&self, orientation: Orientation) -> String {
        if self.from_roll {
            return "Film base from the roll: this frame shows no clear film of its own.".into();
        }
        if self.by_hand {
            return "Film base as picked or set in Lab.".into();
        }
        if self.picked {
            return "Film base read where you clicked.".into();
        }
        if self.found {
            return match &self.edge {
                Some(edge) => format!("Film base found along the {} edge.", shown_edge(orientation, edge)),
                None => "Film base found all round the picture.".into(),
            };
        }
        "No clear film found: the base is a guess from the thinnest part of the picture.".into()
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
        match self {
            Pick::Base => "Click the clear film between frames or along the edge. Esc cancels.",
            Pick::Neutral => "Click something that should be grey. Esc cancels.",
        }
    }
}

/// `a` and `b` are the same film base, allowing for a number's last bit.
pub fn same_base(a: &Value, b: &Value) -> bool {
    match (a.as_object(), b.as_object()) {
        (Some(x), Some(y)) => x.keys().chain(y.keys()).all(|k| {
            let (p, q) = (x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null));
            match (p.as_f64(), q.as_f64()) {
                (Some(p), Some(q)) => (p - q).abs() <= 1e-9 * q.abs().max(1.0),
                _ => p == q,
            }
        }),
        _ => a == b,
    }
}
