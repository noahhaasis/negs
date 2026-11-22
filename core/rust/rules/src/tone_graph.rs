//! The tone graph for the print curves (ticket 20, prototype), a port of
//! `ToneGraph.swift` (windows/src/tone_graph.rs's API): the paper's real
//! response to a ramp of greys, with the photograph's film exposure behind
//! it; dragging it sets Print exposure and the print curves' gammas. The
//! curve and graph maths run in core/src/rules/tone; reading the helper's
//! JSON and the edit's params is state plumbing and stays here (Phase 4
//! moves `json` and `state` to the core). The window that draws it is
//! windows/src/ui/tone_graph.rs.

use serde_json::Value;

use negs_core_sys::media as sys;

pub const PRINT_EXPOSURE: &str = "enlarger.print_exposure";
pub const ACTIVE: &str = "print_render.density_curves_morph.active";
pub const GAMMA: &str = "print_render.density_curves_morph.gamma_factor";
pub const FAST: &str = "print_render.density_curves_morph.gamma_factor_fast";
pub const SLOW: &str = "print_render.density_curves_morph.gamma_factor_slow";
/// The sliders' ranges and step, which the drags keep to.
pub const EXPOSURE_RANGE: (f64, f64) = (0.1, 5.0);
pub const GAMMA_RANGE: (f64, f64) = (0.5, 2.0);
/// The film exposures the graph shows, in stops over mid-grey.
pub const SHOWN: (f64, f64) = (-6.0, 6.0);
/// Greys per curve the helper computes.
pub const POINTS: usize = 256;

/// windows/src/json.rs's helpers this module uses, as they are there.
mod json {
    use serde_json::{Map, Value};

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
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same(v, w)),
            _ => a == b,
        }
    }
}

/// The controls the graph moves, as they act: the gammas are 1 while the
/// print curves are off.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    pub print_exposure: f64,
    pub gamma: f64,
    pub fast: f64,
    pub slow: f64,
}

impl Default for Controls {
    fn default() -> Self {
        Self { print_exposure: 1.0, gamma: 1.0, fast: 1.0, slow: 1.0 }
    }
}

impl Controls {
    pub fn read<'a>(value: impl Fn(&str) -> &'a Value) -> Self {
        let positive = |path: &str| Some(json::number(value(path))).filter(|v| *v > 0.0).unwrap_or(1.0);
        let mut controls = Self { print_exposure: positive(PRINT_EXPOSURE), ..Self::default() };
        if json::boolean(value(ACTIVE)) {
            (controls.gamma, controls.fast, controls.slow) = (positive(GAMMA), positive(FAST), positive(SLOW));
        }
        controls
    }

    fn raw(self) -> sys::negs_tone_controls_t {
        sys::negs_tone_controls_t { print_exposure: self.print_exposure, gamma: self.gamma, fast: self.fast, slow: self.slow }
    }
}

/// What a curve is asked for: the film, paper and params as rendered.
pub fn request(film: &str, paper: &str, params: &Value) -> Value {
    serde_json::json!({"film": film, "paper": paper, "params": params})
}

/// A request without the controls the graph predicts, so a curve for the
/// same settings otherwise can stand in while the next is computed.
fn settings(request: &Value) -> Value {
    let mut settings = request.clone();
    for path in [PRINT_EXPOSURE, ACTIVE, GAMMA, FAST, SLOW] {
        json::set(&mut settings, &format!("params.{path}"), Value::Null);
    }
    settings
}

/// The helper's `tone_curve`: per grey, the film's exposure in stops, the
/// paper's log10 exposure and the scan, per channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// What it was computed for (`request`).
    pub request: Value,
    pub controls: Controls,
    /// Mean film exposure per grey, in stops (the x axis).
    pub stops: Vec<f64>,
    /// Per channel, per grey; None when scanning the film directly.
    pub paper: Option<[Vec<f64>; 3]>,
    pub rgb: [Vec<f64>; 3],
}

fn triples(value: &Value) -> Option<Vec<[f64; 3]>> {
    value
        .as_array()?
        .iter()
        .map(|t| match t.as_array()?.as_slice() {
            [r, g, b] => Some([r.as_f64()?, g.as_f64()?, b.as_f64()?]),
            _ => None,
        })
        .collect()
}

fn channels(triples: &[[f64; 3]]) -> [Vec<f64>; 3] {
    [0, 1, 2].map(|c| triples.iter().map(|t| t[c]).collect())
}

fn series(s: &[f64]) -> sys::negs_tone_series_t {
    sys::negs_tone_series_t::of(s)
}

fn three(s: &[Vec<f64>; 3]) -> [sys::negs_tone_series_t; 3] {
    [series(&s[0]), series(&s[1]), series(&s[2])]
}

/// The series as the core borrows them for one call.
fn raw_curve(stops: &[f64], rgb: &[Vec<f64>; 3], paper: Option<&[Vec<f64>; 3]>) -> sys::negs_tone_curve_t {
    sys::negs_tone_curve_t {
        stops: series(stops),
        rgb: three(rgb),
        paper: paper.map_or([sys::negs_tone_series_t::EMPTY; 3], three),
        has_paper: paper.is_some() as u8,
        reserved: [0; 7],
    }
}

/// Panics where the Rust original indexes past a channel's end, before the
/// core reads it: a channel past 2, or (all channels) one shorter than the
/// first.
fn check(series: &[Vec<f64>; 3], channel: Option<usize>) {
    match channel {
        Some(c) => assert!(c < 3, "index out of bounds: the len is 3 but the index is {c}"),
        None => {
            let n = series[0].len();
            assert!(series[1].len() >= n && series[2].len() >= n, "index out of bounds: a channel is shorter than the first");
        }
    }
}

fn channel_of(channel: Option<usize>) -> i32 {
    channel.map_or(-1, |c| c as i32)
}

/// One channel's series, or the luma or mean of all three.
fn combined(series: &[Vec<f64>; 3], channel: Option<usize>, luma: bool) -> Vec<f64> {
    check(series, channel);
    match channel {
        Some(c) => series[c].clone(),
        None => {
            let mut out = vec![0.0; series[0].len()];
            unsafe { sys::negs_tone_combine(&three(series), luma as u8, out.as_mut_ptr()) };
            out
        }
    }
}

fn brightness(rgb: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    combined(rgb, channel, true)
}

fn mean_or(series: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    combined(series, channel, false)
}

impl Curve {
    pub fn parse(result: &Value, request: Value, controls: Controls) -> Option<Self> {
        let film = triples(&result["film"])?;
        let rgb = triples(&result["rgb"])?;
        if film.len() < 2 || film.len() != rgb.len() {
            return None;
        }
        let paper = triples(&result["paper"]).filter(|p| p.len() == film.len());
        Some(Self {
            request,
            controls,
            stops: film.iter().map(|f| (f[0] + f[1] + f[2]) / 3.0).collect(),
            paper: paper.as_deref().map(channels),
            rgb: channels(&rgb),
        })
    }

    /// Computed for the same settings as `request`, apart from the controls
    /// the prediction moves.
    pub fn matches(&self, request: &Value) -> bool {
        json::same(&settings(&self.request), &settings(request))
    }

    pub fn luma(r: f64, g: f64, b: f64) -> f64 {
        unsafe { sys::negs_tone_luma(r, g, b) }
    }

    /// The curve's brightness per grey for `channel` (None: neutral, the luma).
    pub fn brightness(&self, channel: Option<usize>) -> Vec<f64> {
        brightness(&self.rgb, channel)
    }

    /// Paper exposure per grey for `channel` (None: the layers' mean).
    pub fn paper_exposure(&self, channel: Option<usize>) -> Option<Vec<f64>> {
        self.paper.as_ref().map(|paper| mean_or(paper, channel))
    }

    fn raw(&self) -> sys::negs_tone_curve_t {
        raw_curve(&self.stops, &self.rgb, self.paper.as_ref())
    }

    /// The curve `next` would give, predicted from this one: print exposure
    /// shifts the paper exposure by log10(new/old); the gammas scale it about
    /// 0 (slow above, fast below), and the brightness is this curve's at the
    /// scaled exposure. Exact for print exposure and overall gamma, close for
    /// fast and slow; the helper's next curve corrects it.
    pub fn predicted(&self, next: Controls) -> Display {
        let Some(paper) = self.paper.as_ref().filter(|_| next != self.controls) else {
            return Display { stops: self.stops.clone(), rgb: self.rgb.clone(), paper: self.paper.clone() };
        };
        let n = self.stops.len();
        assert!(
            (0..3).all(|c| paper[c].len() >= n && self.rgb[c].len() >= n),
            "index out of bounds: a channel is shorter than the stops"
        );
        let (mut rgb, mut out) = (self.rgb.clone(), paper.clone());
        let rgb_out = [rgb[0].as_mut_ptr(), rgb[1].as_mut_ptr(), rgb[2].as_mut_ptr()];
        let paper_out = [out[0].as_mut_ptr(), out[1].as_mut_ptr(), out[2].as_mut_ptr()];
        unsafe { sys::negs_tone_predict(&self.raw(), self.controls.raw(), next.raw(), &rgb_out, &paper_out) };
        Display { stops: self.stops.clone(), rgb, paper: Some(out) }
    }
}

/// The fractional index where a decreasing series reaches `value`, clamped to the ends.
pub fn index_of_decreasing(series: &[f64], value: f64) -> f64 {
    unsafe { sys::negs_tone_index_of_decreasing(series.as_ptr(), series.len(), value) }
}

pub fn index_of_increasing(series: &[f64], value: f64) -> f64 {
    unsafe { sys::negs_tone_index_of_increasing(series.as_ptr(), series.len(), value) }
}

pub fn interpolate(series: &[f64], index: f64) -> f64 {
    unsafe { sys::negs_tone_interpolate(series.as_ptr(), series.len(), index) }
}

/// A curve as drawn: per channel brightness and paper exposure per grey.
#[derive(Clone, Debug)]
pub struct Display {
    pub stops: Vec<f64>,
    pub rgb: [Vec<f64>; 3],
    pub paper: Option<[Vec<f64>; 3]>,
}

impl Display {
    pub fn brightness(&self, channel: Option<usize>) -> Vec<f64> {
        brightness(&self.rgb, channel)
    }

    fn raw(&self) -> sys::negs_tone_curve_t {
        raw_curve(&self.stops, &self.rgb, self.paper.as_ref())
    }

    /// The handles' tones: where the paper's exposure is 0, and 2 stops
    /// either side. None if the paper exposure never crosses 0.
    pub fn handles(&self, channel: Option<usize>) -> Option<(f64, f64, f64)> {
        check(self.paper.as_ref()?, channel);
        let mut out = [0.0; 3];
        (unsafe { sys::negs_tone_handles(&self.raw(), channel_of(channel), &mut out) } != 0).then_some((out[0], out[1], out[2]))
    }

    pub fn value(&self, series: &[f64], stops: f64) -> f64 {
        unsafe { sys::negs_tone_value(&self.raw(), series.as_ptr(), series.len(), stops) }
    }
}

/// The photograph's film exposure: counts per layer over `range` stops.
#[derive(Clone, Debug, PartialEq)]
pub struct FilmHistogram {
    pub range: (f64, f64),
    pub counts: [Vec<f64>; 3],
}

impl FilmHistogram {
    pub fn parse(value: &Value) -> Option<Self> {
        let stops: Vec<f64> = value["stops"].as_array()?.iter().map(|v| v.as_f64()).collect::<Option<_>>()?;
        let counts: Vec<Vec<f64>> = value["counts"]
            .as_array()?
            .iter()
            .map(|layer| layer.as_array()?.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
            .collect::<Option<_>>()?;
        let [low, high] = stops[..] else { return None };
        let bins = counts.first()?.len();
        if counts.len() != 3 || high <= low || bins < 2 || counts.iter().any(|c| c.len() != bins) {
            return None;
        }
        let mut counts = counts.into_iter();
        Some(Self { range: (low, high), counts: [0; 3].map(|_| counts.next().unwrap()) })
    }

    pub fn bins(&self, channel: Option<usize>) -> Vec<f64> {
        mean_or(&self.counts, channel)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    Middle,
    Shadows,
    Highlights,
    /// Anywhere else on the curve, at this tone in stops.
    Curve(f64),
}

fn static_text(p: *const std::os::raw::c_char) -> &'static str {
    // The core's static, NUL-terminated ASCII.
    unsafe { std::ffi::CStr::from_ptr(p) }.to_str().expect("UTF-8")
}

impl Handle {
    fn raw(self) -> (u32, f64) {
        match self {
            Handle::Middle => (sys::NEGS_TONE_MIDDLE, 0.0),
            Handle::Shadows => (sys::NEGS_TONE_SHADOWS, 0.0),
            Handle::Highlights => (sys::NEGS_TONE_HIGHLIGHTS, 0.0),
            Handle::Curve(t) => (sys::NEGS_TONE_CURVE, t),
        }
    }

    fn from_raw(kind: u32, tone: f64) -> Self {
        match kind {
            sys::NEGS_TONE_MIDDLE => Handle::Middle,
            sys::NEGS_TONE_SHADOWS => Handle::Shadows,
            sys::NEGS_TONE_HIGHLIGHTS => Handle::Highlights,
            _ => Handle::Curve(tone),
        }
    }

    pub fn title(self) -> &'static str {
        static_text(unsafe { sys::negs_tone_handle_title(self.raw().0) })
    }
    pub fn path(self) -> &'static str {
        match self {
            Handle::Middle => PRINT_EXPOSURE,
            Handle::Shadows => SLOW,
            Handle::Highlights => FAST,
            Handle::Curve(_) => GAMMA,
        }
    }
    pub fn hint(self) -> &'static str {
        static_text(unsafe { sys::negs_tone_handle_hint(self.raw().0) })
    }
    /// The same handle, wherever on the curve.
    pub fn same(self, other: Option<Handle>) -> bool {
        match (self, other) {
            (Handle::Curve(_), Some(Handle::Curve(_))) => true,
            (a, Some(b)) => a == b,
            _ => false,
        }
    }
}

/// The graph's frame in pixels: film exposure across, brightness up, with
/// `inset` above and below.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub inset: f64,
}

impl Geometry {
    fn raw(&self) -> sys::negs_tone_geometry_t {
        sys::negs_tone_geometry_t { width: self.width, height: self.height, inset: self.inset }
    }
    pub fn x(&self, stops: f64) -> f64 {
        unsafe { sys::negs_tone_x(self.raw(), stops) }
    }
    pub fn stops(&self, x: f64) -> f64 {
        unsafe { sys::negs_tone_stops(self.raw(), x) }
    }
    pub fn y(&self, brightness: f64) -> f64 {
        unsafe { sys::negs_tone_y(self.raw(), brightness) }
    }
    pub fn brightness(&self, y: f64) -> f64 {
        unsafe { sys::negs_tone_brightness(self.raw(), y) }
    }

    pub fn handle_points(&self, display: &Display, channel: Option<usize>) -> Vec<(Handle, (f64, f64))> {
        if display.handles(channel).is_none() {
            return vec![];
        }
        check(&display.rgb, channel);
        let mut out = [sys::negs_tone_point_t::default(); 3];
        let n = unsafe { sys::negs_tone_handle_points(self.raw(), &display.raw(), channel_of(channel), &mut out) };
        out[..n].iter().map(|p| (Handle::from_raw(p.kind, 0.0), (p.x, p.y))).collect()
    }

    /// The handle within `radius` of `point`, or the curve within `tolerance` of it.
    pub fn hit(&self, display: &Display, channel: Option<usize>, point: (f64, f64), radius: f64, tolerance: f64) -> Option<Handle> {
        if let Some(paper) = display.paper.as_ref() {
            check(paper, channel);
            check(&display.rgb, channel);
        }
        let (mut kind, mut tone) = (0, 0.0);
        let hit = unsafe { sys::negs_tone_hit(self.raw(), &display.raw(), channel_of(channel), point.0, point.1, radius, tolerance, &mut kind, &mut tone) };
        (hit != 0).then(|| Handle::from_raw(kind, tone))
    }
}

/// A drag in progress. Its values are solved on the curve it began with, so
/// a newer curve arriving mid-drag doesn't make it jitter.
#[derive(Clone, Debug)]
pub struct Drag {
    pub handle: Handle,
    pub base: Curve,
    /// The tone the drag acts at, in stops.
    pub tone: f64,
}

impl Drag {
    pub fn start(handle: Handle, base: &Curve, display: &Display, channel: Option<usize>) -> Option<Self> {
        if !matches!(handle, Handle::Curve(_)) {
            check(display.paper.as_ref()?, channel);
        }
        let (kind, at) = handle.raw();
        let mut tone = 0.0;
        (unsafe { sys::negs_tone_drag_tone(kind, at, &display.raw(), channel_of(channel), &mut tone) } != 0).then(|| Self { handle, base: base.clone(), tone })
    }

    /// The control the pointer at `point` asks for, snapped to its slider's
    /// range and step; None where it can't be solved.
    pub fn value(&self, current: Controls, channel: Option<usize>, geometry: &Geometry, point: (f64, f64)) -> Option<(&'static str, f64)> {
        let base = &self.base;
        check(base.paper.as_ref()?, channel);
        if self.handle != Handle::Middle {
            check(&base.rgb, channel);
        }
        let mut value = 0.0;
        let set = unsafe {
            sys::negs_tone_drag_value(self.handle.raw().0, self.tone, &base.raw(), base.controls.raw(), current.raw(), channel_of(channel), geometry.raw(), point.0, point.1, &mut value)
        };
        let path = match set {
            sys::NEGS_TONE_SET_PRINT_EXPOSURE => PRINT_EXPOSURE,
            sys::NEGS_TONE_SET_GAMMA => GAMMA,
            sys::NEGS_TONE_SET_FAST => FAST,
            sys::NEGS_TONE_SET_SLOW => SLOW,
            _ => return None,
        };
        Some((path, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
        static NULL: Value = Value::Null;
        path.split('.').fold(value, |v, k| v.get(k).unwrap_or(&NULL))
    }

    /// A synthetic curve, linear in film exposure: the paper gets 0.3 log
    /// less per stop, and the print brightens by 0.05 per stop.
    fn curve(controls: Controls) -> Curve {
        let stops: Vec<f64> = (0..=160).map(|i| -8.0 + i as f64 * 0.1).collect();
        let result = json!({
            "film": stops.iter().map(|s| [s, s, s]).collect::<Vec<_>>(),
            "paper": stops.iter().map(|s| [-0.3 * s, -0.3 * s, -0.3 * s]).collect::<Vec<_>>(),
            "rgb": stops.iter().map(|s| { let v = 0.5 + 0.05 * s; [v, v, v] }).collect::<Vec<_>>(),
        });
        Curve::parse(&result, request("film", "paper", &json!({})), controls).unwrap()
    }

    const GEOMETRY: Geometry = Geometry { width: 300.0, height: 170.0, inset: 8.0 };

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn indexes_interpolate_and_clamp_to_the_ends() {
        let falling = [3.0, 2.0, 1.0, 0.0];
        assert!(close(index_of_decreasing(&falling, 1.5), 1.5));
        assert_eq!(index_of_decreasing(&falling, 9.0), 0.0);
        assert_eq!(index_of_decreasing(&falling, -9.0), 3.0);
        assert!(close(index_of_increasing(&[0.0, 1.0, 2.0], 0.25), 0.25));
        assert!(close(interpolate(&[0.0, 10.0, 20.0], 1.5), 15.0));
        assert_eq!(interpolate(&[0.0, 10.0], 7.0), 10.0);
        assert_eq!(interpolate(&[], 1.0), 0.0);
    }

    #[test]
    fn controls_are_one_while_the_print_curves_are_off() {
        let params = json!({"enlarger": {"print_exposure": 1.5},
            "print_render": {"density_curves_morph": {"active": false, "gamma_factor": 1.3, "gamma_factor_fast": 0.8, "gamma_factor_slow": 0.0}}});
        let off = Controls::read(|p| get(&params, p));
        assert_eq!(off, Controls { print_exposure: 1.5, ..Controls::default() });
        let mut on = params.clone();
        json::set(&mut on, ACTIVE, json!(true));
        assert_eq!(Controls::read(|p| get(&on, p)), Controls { print_exposure: 1.5, gamma: 1.3, fast: 0.8, slow: 1.0 });
    }

    #[test]
    fn handles_hits_and_drags() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        let (shadows, middle, highlights) = display.handles(None).unwrap();
        assert!(close(shadows, -2.0) && close(middle, 0.0) && close(highlights, 2.0));
        let points = GEOMETRY.handle_points(&display, None);
        assert_eq!(points.len(), 3);
        assert_eq!(GEOMETRY.hit(&display, None, (points[1].1.0 + 3.0, points[1].1.1), 10.0, 12.0), Some(Handle::Middle));
        let drag = Drag::start(Handle::Middle, &base, &display, None).unwrap();
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (GEOMETRY.x(1.0), 0.0)), Some((PRINT_EXPOSURE, 2.0)));
        let drag = Drag::start(Handle::Curve(2.0), &base, &display, None).unwrap();
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4))), Some((GAMMA, 1.2)));
        assert_eq!(Handle::Shadows.title(), "Shadow contrast");
        assert_eq!(Handle::Middle.hint(), "drag sideways");
        assert_eq!(Handle::Curve(1.0).hint(), "drag up or down");
    }

    #[test]
    #[should_panic]
    fn a_channel_past_blue_panics_as_before() {
        let base = curve(Controls::default());
        base.brightness(Some(3));
    }
}
