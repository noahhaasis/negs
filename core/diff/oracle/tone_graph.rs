//! The tone graph for the print curves (ticket 20, prototype), a port of
//! `ToneGraph.swift`: the paper's real response to a ramp of greys, with the
//! photograph's film exposure behind it; dragging it sets Print exposure and
//! the print curves' gammas. Adapted from SpektraMac's Print tab "Contrast"
//! graph (GPL-3.0). The window that draws it is `ui::tone_graph`.

use serde_json::Value;

use crate::json;
use crate::state;

pub const PRINT_EXPOSURE: &str = "enlarger.print_exposure";
pub const ACTIVE: &str = "print_render.density_curves_morph.active";
pub const GAMMA: &str = "print_render.density_curves_morph.gamma_factor";
pub const FAST: &str = "print_render.density_curves_morph.gamma_factor_fast";
pub const SLOW: &str = "print_render.density_curves_morph.gamma_factor_slow";
/// The sliders' ranges and step, which the drags keep to.
pub const EXPOSURE_RANGE: (f64, f64) = (0.1, 5.0);
pub const GAMMA_RANGE: (f64, f64) = (0.5, 2.0);
const STEP: f64 = 0.01;
/// The film exposures the graph shows, in stops over mid-grey.
pub const SHOWN: (f64, f64) = (-6.0, 6.0);
/// Greys per curve the helper computes.
pub const POINTS: usize = 256;

/// The controls the graph moves, as they act: the gammas are 1 while the
/// print curves are off.
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
    value.as_array()?.iter().map(|t| match t.as_array()?.as_slice() {
        [r, g, b] => Some([r.as_f64()?, g.as_f64()?, b.as_f64()?]),
        _ => None,
    }).collect()
}

fn channels(triples: &[[f64; 3]]) -> [Vec<f64>; 3] {
    [0, 1, 2].map(|c| triples.iter().map(|t| t[c]).collect())
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
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// The curve's brightness per grey for `channel` (None: neutral, the luma).
    pub fn brightness(&self, channel: Option<usize>) -> Vec<f64> {
        brightness(&self.rgb, channel)
    }

    /// Paper exposure per grey for `channel` (None: the layers' mean).
    pub fn paper_exposure(&self, channel: Option<usize>) -> Option<Vec<f64>> {
        self.paper.as_ref().map(|paper| mean_or(paper, channel))
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
        let shift = (next.print_exposure / self.controls.print_exposure).log10();
        let gamma = next.gamma / self.controls.gamma;
        let (slow, fast) = (next.slow / self.controls.slow, next.fast / self.controls.fast);
        let (mut rgb, mut out) = (self.rgb.clone(), paper.clone());
        for c in 0..3 {
            for i in 0..self.stops.len() {
                let x = paper[c][i] + shift;
                let k = gamma * if x >= 0.0 { slow } else { fast };
                out[c][i] = x;
                rgb[c][i] = interpolate(&self.rgb[c], index_of_decreasing(&paper[c], k * x));
            }
        }
        Display { stops: self.stops.clone(), rgb, paper: Some(out) }
    }
}

fn brightness(rgb: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    match channel {
        Some(c) => rgb[c].clone(),
        None => (0..rgb[0].len()).map(|i| Curve::luma(rgb[0][i], rgb[1][i], rgb[2][i])).collect(),
    }
}

fn mean_or(series: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    match channel {
        Some(c) => series[c].clone(),
        None => (0..series[0].len()).map(|i| (series[0][i] + series[1][i] + series[2][i]) / 3.0).collect(),
    }
}

/// The fractional index where a decreasing series reaches `value`, clamped to the ends.
pub fn index_of_decreasing(series: &[f64], value: f64) -> f64 {
    let (Some(&first), Some(&last)) = (series.first(), series.last()) else { return 0.0 };
    if value >= first {
        return 0.0;
    }
    if value <= last {
        return (series.len() - 1) as f64;
    }
    let (mut lo, mut hi) = (0, series.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if series[mid] > value { lo = mid } else { hi = mid }
    }
    let span = series[lo] - series[hi];
    lo as f64 + if span > 0.0 { (series[lo] - value) / span } else { 0.0 }
}

pub fn index_of_increasing(series: &[f64], value: f64) -> f64 {
    let negated: Vec<f64> = series.iter().map(|v| -v).collect();
    index_of_decreasing(&negated, -value)
}

pub fn interpolate(series: &[f64], index: f64) -> f64 {
    if series.is_empty() {
        return 0.0;
    }
    let i = index.clamp(0.0, (series.len() - 1) as f64);
    let lo = i.floor() as usize;
    let hi = (lo + 1).min(series.len() - 1);
    series[lo] + (series[hi] - series[lo]) * (i - lo as f64)
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

    /// The handles' tones: where the paper's exposure is 0, and 2 stops
    /// either side. None if the paper exposure never crosses 0.
    pub fn handles(&self, channel: Option<usize>) -> Option<(f64, f64, f64)> {
        let series = mean_or(self.paper.as_ref()?, channel);
        let (&first, &last) = (series.first()?, series.last()?);
        if !(first > 0.0 && last < 0.0) {
            return None;
        }
        let middle = interpolate(&self.stops, index_of_decreasing(&series, 0.0));
        Some((middle - 2.0, middle, middle + 2.0))
    }

    pub fn value(&self, series: &[f64], stops: f64) -> f64 {
        interpolate(series, index_of_increasing(&self.stops, stops))
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

impl Handle {
    pub fn title(self) -> &'static str {
        match self {
            Handle::Middle => "Print exposure",
            Handle::Shadows => "Shadow contrast",
            Handle::Highlights => "Highlight contrast",
            Handle::Curve(_) => "Contrast",
        }
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
        match self {
            Handle::Middle => "drag sideways",
            _ => "drag up or down",
        }
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
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub inset: f64,
}

impl Geometry {
    pub fn x(&self, stops: f64) -> f64 {
        (stops - SHOWN.0) / (SHOWN.1 - SHOWN.0) * self.width
    }
    pub fn stops(&self, x: f64) -> f64 {
        SHOWN.0 + x / self.width.max(1.0) * (SHOWN.1 - SHOWN.0)
    }
    pub fn y(&self, brightness: f64) -> f64 {
        self.inset + (1.0 - brightness.clamp(0.0, 1.0)) * (self.height - 2.0 * self.inset)
    }
    pub fn brightness(&self, y: f64) -> f64 {
        (1.0 - (y - self.inset) / (self.height - 2.0 * self.inset).max(1.0)).clamp(0.001, 0.999)
    }

    pub fn handle_points(&self, display: &Display, channel: Option<usize>) -> Vec<(Handle, (f64, f64))> {
        let Some((shadows, middle, highlights)) = display.handles(channel) else { return vec![] };
        let curve = display.brightness(channel);
        [(Handle::Shadows, shadows), (Handle::Middle, middle), (Handle::Highlights, highlights)]
            .into_iter()
            .map(|(handle, t)| (handle, (self.x(t), self.y(display.value(&curve, t)))))
            .collect()
    }

    /// The handle within `radius` of `point`, or the curve within `tolerance` of it.
    pub fn hit(&self, display: &Display, channel: Option<usize>, point: (f64, f64), radius: f64, tolerance: f64) -> Option<Handle> {
        let nearest = self
            .handle_points(display, channel)
            .into_iter()
            .map(|(handle, (x, y))| (handle, (x - point.0).hypot(y - point.1)))
            .filter(|(_, d)| *d < radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((handle, _)) = nearest {
            return Some(handle);
        }
        display.paper.as_ref()?;
        let t = self.stops(point.0);
        let y = self.y(display.value(&display.brightness(channel), t));
        ((y - point.1).abs() < tolerance).then_some(Handle::Curve(t))
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
        let tone = match handle {
            Handle::Curve(t) => t,
            _ => {
                let (shadows, middle, highlights) = display.handles(channel)?;
                match handle {
                    Handle::Shadows => shadows,
                    Handle::Highlights => highlights,
                    _ => middle,
                }
            }
        };
        Some(Self { handle, base: base.clone(), tone })
    }

    /// The control the pointer at `point` asks for, snapped to its slider's
    /// range and step; None where it can't be solved.
    pub fn value(&self, current: Controls, channel: Option<usize>, geometry: &Geometry, point: (f64, f64)) -> Option<(&'static str, f64)> {
        let base = &self.base;
        let start = base.controls;
        let paper = base.paper_exposure(channel)?;
        let (path, value, range) = match self.handle {
            Handle::Middle => {
                // Paper exposure 0 under the pointer: new = base · 10^(−paper(t)).
                let t = geometry.stops(point.0);
                let p = interpolate(&paper, index_of_increasing(&base.stops, t));
                (PRINT_EXPOSURE, start.print_exposure * 10f64.powf(-p), EXPOSURE_RANGE)
            }
            handle => {
                // The tone on the base curve that already has the pointer's
                // brightness; the scale is its paper exposure over the handle's.
                let target = geometry.brightness(point.1);
                let reached = interpolate(&paper, index_of_increasing(&base.brightness(channel), target));
                let shift = (current.print_exposure / start.print_exposure).log10();
                let at = interpolate(&paper, index_of_increasing(&base.stops, self.tone)) + shift;
                if at.abs() <= 0.03 {
                    return None;
                }
                let k = (reached / at).max(0.01);
                let side = if at >= 0.0 { current.slow / start.slow } else { current.fast / start.fast };
                let gamma = current.gamma / start.gamma;
                match handle {
                    Handle::Shadows => (SLOW, start.slow * k / gamma, GAMMA_RANGE),
                    Handle::Highlights => (FAST, start.fast * k / gamma, GAMMA_RANGE),
                    _ => (GAMMA, start.gamma * k / side, GAMMA_RANGE),
                }
            }
        };
        value.is_finite().then(|| (path, state::snap(value, range.0, range.1, STEP)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let off = Controls::read(|p| json::get(&params, p));
        assert_eq!(off, Controls { print_exposure: 1.5, ..Controls::default() });
        let mut on = params.clone();
        json::set(&mut on, ACTIVE, json!(true));
        let read = Controls::read(|p| json::get(&on, p));
        // A gamma of 0 isn't one the engine uses: 1.
        assert_eq!(read, Controls { print_exposure: 1.5, gamma: 1.3, fast: 0.8, slow: 1.0 });
    }

    #[test]
    fn malformed_replies_are_refused() {
        let controls = Controls::default();
        let r = json!({});
        assert!(Curve::parse(&json!({"film": [[0, 0, 0]], "rgb": [[0, 0, 0]]}), r.clone(), controls).is_none());
        assert!(Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0]]}), r.clone(), controls).is_none());
        assert!(Curve::parse(&json!({"film": [[0, 0], [1, 1]], "rgb": [[0, 0], [1, 1]]}), r.clone(), controls).is_none());
        // Scanning the film directly: no paper, the curve alone.
        let scan = Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "paper": null, "rgb": [[0, 0, 0], [1, 1, 1]]}), r, controls).unwrap();
        assert!(scan.paper.is_none());
        assert!(scan.predicted(Controls { print_exposure: 2.0, ..controls }).handles(None).is_none());
        assert!(FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4]]})).is_none());
        assert!(FilmHistogram::parse(&json!({"stops": [8, -8], "counts": [[1, 2], [3, 4], [5, 6]]})).is_none());
        let histogram = FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4], [5, 6]]})).unwrap();
        assert_eq!(histogram.bins(None), vec![3.0, 4.0]);
        assert_eq!(histogram.bins(Some(2)), vec![5.0, 6.0]);
    }

    #[test]
    fn a_curve_stands_in_only_for_the_same_settings() {
        let params = json!({"enlarger": {"print_exposure": 1, "y_filter_shift": 0}});
        let base = Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0], [1, 1, 1]]}), request("f", "p", &params), Controls::default()).unwrap();
        let mut moved = params.clone();
        json::set(&mut moved, PRINT_EXPOSURE, json!(2.5));
        json::set(&mut moved, GAMMA, json!(1.2));
        assert!(base.matches(&request("f", "p", &moved)));
        json::set(&mut moved, "enlarger.y_filter_shift", json!(3));
        assert!(!base.matches(&request("f", "p", &moved)));
        assert!(!base.matches(&request("f", "other", &params)));
    }

    #[test]
    fn prediction_is_exact_for_print_exposure_and_gamma() {
        let base = curve(Controls::default());
        let same = base.predicted(Controls::default());
        assert_eq!(same.rgb, base.rgb);
        // Twice the light on the paper: each grey prints like one log10(2)
        // higher on the paper exposure axis, i.e. log10(2)/0.3 stops darker.
        let brighter = base.predicted(Controls { print_exposure: 2.0, ..Controls::default() });
        let shift = 2f64.log10() / 0.3;
        for i in [40, 80, 120] {
            let expected = 0.5 + 0.05 * (base.stops[i] - shift);
            assert!((brighter.rgb[1][i] - expected).abs() < 1e-9, "{i}: {} vs {expected}", brighter.rgb[1][i]);
        }
        let handles = brighter.handles(None).unwrap();
        assert!((handles.1 - shift).abs() < 1e-9);
        // Gamma 1.25 about mid-grey: the paper exposure scales by it.
        let steeper = base.predicted(Controls { gamma: 1.25, ..Controls::default() });
        for i in [50, 110] {
            assert!((steeper.rgb[0][i] - (0.5 + 0.05 * 1.25 * base.stops[i])).abs() < 1e-9);
        }
        // Slow gamma changes only the shadows' side, above paper exposure 0.
        let slow = base.predicted(Controls { slow: 1.5, ..Controls::default() });
        assert!((slow.rgb[0][120] - base.rgb[0][120]).abs() < 1e-12);
        assert!(slow.rgb[0][40] < base.rgb[0][40]);
    }

    #[test]
    fn handles_sit_at_paper_exposure_zero_and_two_stops_either_side() {
        let display = curve(Controls::default()).predicted(Controls::default());
        let (shadows, middle, highlights) = display.handles(None).unwrap();
        assert!(close(shadows, -2.0) && close(middle, 0.0) && close(highlights, 2.0));
        let points = GEOMETRY.handle_points(&display, None);
        assert_eq!(points.len(), 3);
        assert!(close(points[1].1.0, 150.0));
        assert!(close(points[1].1.1, GEOMETRY.y(0.5)));
        let at = points[1].1;
        assert_eq!(GEOMETRY.hit(&display, None, (at.0 + 3.0, at.1), 10.0, 12.0), Some(Handle::Middle));
        let t = 4.0;
        let on_curve = (GEOMETRY.x(t), GEOMETRY.y(0.5 + 0.05 * t) + 5.0);
        assert!(matches!(GEOMETRY.hit(&display, None, on_curve, 10.0, 12.0), Some(Handle::Curve(s)) if (s - t).abs() < 1e-9));
        assert_eq!(GEOMETRY.hit(&display, None, (on_curve.0, on_curve.1 + 30.0), 10.0, 12.0), None);
    }

    #[test]
    fn the_middle_handle_puts_mid_grey_under_the_pointer() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        let drag = Drag::start(Handle::Middle, &base, &display, None).unwrap();
        // One stop right: paper −0.3 there, so 10^0.3 = 1.995 → 2.00.
        let (path, value) = drag.value(base.controls, None, &GEOMETRY, (GEOMETRY.x(1.0), 0.0)).unwrap();
        assert_eq!((path, value), (PRINT_EXPOSURE, 2.0));
        // Far left: clamped to the slider's minimum.
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (0.0, 0.0)).unwrap().1, EXPOSURE_RANGE.0);
    }

    #[test]
    fn contrast_drags_scale_the_paper_exposure_at_their_tone() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        // Dragging the curve at +2 stops (paper −0.6) to the brightness the
        // base curve has at +2.4 stops (paper −0.72): gamma 1.2.
        let drag = Drag::start(Handle::Curve(2.0), &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, point), Some((GAMMA, 1.2)));
        // The shadows' handle at −2 stops, moved to the base's −2.5: slow 1.25.
        let drag = Drag::start(Handle::Shadows, &base, &display, None).unwrap();
        assert!(close(drag.tone, -2.0));
        let point = (GEOMETRY.x(-2.0), GEOMETRY.y(0.5 - 0.05 * 2.5));
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, point), Some((SLOW, 1.25)));
        // A drag at mid-grey can't scale anything.
        let drag = Drag::start(Handle::Curve(0.0), &base, &display, None).unwrap();
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (150.0, 20.0)), None);
        // Solved on the drag's own curve: a gamma already raised mid-drag
        // is divided out of the next slow value.
        let drag = Drag::start(Handle::Highlights, &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        let raised = Controls { gamma: 1.2, ..base.controls };
        assert_eq!(drag.value(raised, None, &GEOMETRY, point), Some((FAST, 1.0)));
    }
}
