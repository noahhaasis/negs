//! The tone graph's window (`ToneGraphView` in `ToneGraph.swift`). The
//! inspector owns its row, the label and the RGB/R/G/B buttons; this draws
//! the graph and turns the pointer into edits: the middle handle sideways
//! for Print exposure, the side handles and the curve up or down for the
//! gammas, each drag one undo step, double-click to reset. GDI draws only
//! aliased lines, so it is drawn into a pixel buffer like the histogram.

use std::cell::RefCell;

use serde_json::Value;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::{post, with_app};
use crate::json;
use crate::model::Model;
use crate::tone_graph::{self, Controls, Curve, Drag, FilmHistogram, Geometry, Handle};

const ACTION: &str = "Change Tone Curve";

#[derive(Default)]
struct View {
    curve: Option<Curve>,
    /// The curve is for the current settings; otherwise dimmed until the next.
    current: bool,
    controls: Controls,
    /// Print exposure, gamma, fast and slow as set, for the label.
    values: [f64; 4],
    histogram: Option<FilmHistogram>,
    channel: Option<usize>,
    enabled: bool,
    dpi: u32,
    decimal: char,
    label: HWND,
    hovered: Option<Handle>,
    /// Pressed on a handle or the curve, before it moved.
    pressed: Option<(Handle, (f64, f64))>,
    drag: Option<Drag>,
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
}

impl View {
    fn px(&self, dip: f64) -> f64 {
        dip * self.dpi.max(96) as f64 / 96.0
    }

    fn geometry(&self, hwnd: HWND) -> Geometry {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
        }
        Geometry { width: r.right.max(1) as f64, height: r.bottom.max(1) as f64, inset: self.px(8.0) }
    }

    fn display(&self) -> Option<tone_graph::Display> {
        self.curve.as_ref().map(|curve| curve.predicted(self.controls))
    }

    fn hit(&self, hwnd: HWND, point: (f64, f64)) -> Option<Handle> {
        let display = self.display()?;
        self.geometry(hwnd).hit(&display, self.channel, point, self.px(10.0), self.px(12.0))
    }

    fn active(&self) -> Option<Handle> {
        self.drag.as_ref().map(|d| d.handle).or(self.hovered)
    }

    fn label(&self) -> String {
        let Some(active) = self.active() else {
            return if self.curve.is_none() { "Tone graph" } else { "Drag the curve or a handle" }.into();
        };
        let value = match active.path() {
            tone_graph::PRINT_EXPOSURE => self.values[0],
            tone_graph::GAMMA => self.values[1],
            tone_graph::FAST => self.values[2],
            _ => self.values[3],
        };
        let mut value = format!("{value:.2}");
        if self.decimal != '.' {
            value = value.replace('.', &self.decimal.to_string());
        }
        let hint = if self.drag.is_none() { format!(" · {}", active.hint()) } else { String::new() };
        format!("{} {value}{hint}", active.title())
    }
}

fn set_label(view: &View) {
    if view.label.is_invalid() {
        return;
    }
    let text = crate::platform::wide(view.label());
    unsafe {
        let mut current = [0u16; 128];
        let n = GetWindowTextW(view.label, &mut current) as usize;
        if current[..n] != text[..text.len() - 1] {
            let _ = SetWindowTextW(view.label, crate::platform::pcwstr(&text));
        }
    }
}

pub fn register() {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(graph_proc),
        hInstance: super::instance(),
        lpszClassName: w!("NegsToneGraph"),
        hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&class);
    }
}

pub fn create(parent: HWND, id: u16, label: HWND) -> HWND {
    let graph = super::create(w!("NegsToneGraph"), "Tone graph", WS_CHILD, WINDOW_EX_STYLE(0), parent, id, HFONT::default());
    super::accessible_name(graph, "Tone graph: the print's brightness for each film exposure");
    super::dialogs::tooltip(graph, "Drag the middle handle sideways for print exposure, the side handles or the curve up or down for contrast. Double-click a handle to reset it.");
    VIEW.with(|v| v.borrow_mut().label = label);
    graph
}

/// The channel buttons' choices, as Settings keeps them.
pub const CHANNELS: [&str; 4] = ["rgb", "r", "g", "b"];

pub fn channel(key: &str) -> Option<usize> {
    CHANNELS.iter().position(|c| *c == key).and_then(|i| i.checked_sub(1))
}

/// Brings the graph up to the model; redraws only if what it shows changed.
pub fn update(graph: HWND, model: &Model, enabled: bool, dpi: u32, decimal: char) {
    let value = |path: &str| json::number(model.value(path));
    let current = model.tone_curve_current();
    VIEW.with(|v| {
        let mut view = v.borrow_mut();
        let values = [tone_graph::PRINT_EXPOSURE, tone_graph::GAMMA, tone_graph::FAST, tone_graph::SLOW].map(value);
        let channel = channel(&model.prefs.tone_graph_channel);
        let changed = view.curve != model.tone_curve
            || view.current != current
            || view.controls != model.tone_controls()
            || view.values != values
            || view.histogram != model.film_histogram
            || view.channel != channel
            || view.enabled != enabled
            || view.dpi != dpi;
        if !changed {
            return;
        }
        view.curve = model.tone_curve.clone();
        view.current = current;
        view.controls = model.tone_controls();
        view.values = values;
        view.histogram = model.film_histogram.clone();
        view.channel = channel;
        view.enabled = enabled;
        view.dpi = dpi;
        view.decimal = decimal;
        if !enabled {
            view.hovered = None;
        }
        set_label(&view);
        unsafe {
            let _ = InvalidateRect(graph, None, false);
        }
    });
}

fn point(lparam: LPARAM) -> (f64, f64) {
    ((lparam.0 & 0xffff) as i16 as f64, ((lparam.0 >> 16) & 0xffff) as i16 as f64)
}

fn redraw(hwnd: HWND, view: &View) {
    set_label(view);
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// The pointer moved: hover, start a drag once a press moves, or drag.
fn moved(hwnd: HWND, at: (f64, f64)) {
    enum Next {
        Nothing,
        Start(Handle),
        Set(&'static str, f64),
    }
    let next = VIEW.with(|v| {
        let mut view = v.borrow_mut();
        if let Some(drag) = &view.drag {
            let geometry = view.geometry(hwnd);
            return match drag.value(view.controls, view.channel, &geometry, at) {
                Some((path, value)) => Next::Set(path, value),
                None => Next::Nothing,
            };
        }
        if let Some((handle, from)) = view.pressed {
            if (at.0 - from.0).hypot(at.1 - from.1) < 1.0 {
                return Next::Nothing;
            }
            let (Some(curve), Some(display)) = (view.curve.clone(), view.display()) else { return Next::Nothing };
            view.pressed = None;
            view.drag = Drag::start(handle, &curve, &display, view.channel);
            view.hovered = None;
            redraw(hwnd, &view);
            return if view.drag.is_some() { Next::Start(handle) } else { Next::Nothing };
        }
        let hovered = if view.enabled { view.hit(hwnd, at) } else { None };
        if hovered != view.hovered {
            view.hovered = hovered;
            redraw(hwnd, &view);
        }
        Next::Nothing
    });
    match next {
        Next::Nothing => {}
        Next::Start(handle) => {
            let started = with_app(|app| {
                app.model.begin_interaction();
                // Contrast drags turn the print curves on, in the same undo step.
                if handle != Handle::Middle && !json::boolean(app.model.value(tone_graph::ACTIVE)) {
                    app.model.set(tone_graph::ACTIVE, Value::Bool(true));
                }
            });
            if started.is_none() {
                VIEW.with(|v| v.borrow_mut().drag = None);
            } else {
                moved(hwnd, at);
            }
        }
        Next::Set(path, value) => {
            with_app(|app| {
                if (json::number(app.model.value(path)) - value).abs() > 1e-9 {
                    app.model.set(path, json::num(value));
                }
            });
        }
    }
}

/// The press ended or the capture was lost: a drag becomes one undo step.
fn released(hwnd: HWND) {
    let dragged = VIEW.with(|v| {
        let mut view = v.borrow_mut();
        view.pressed = None;
        let dragged = view.drag.take().is_some();
        if dragged {
            redraw(hwnd, &view);
        }
        dragged
    });
    if dragged && with_app(|app| app.model.end_interaction(ACTION)).is_none() {
        // A modal loop took the capture: end it once the app is free.
        post(|app| app.model.end_interaction(ACTION));
    }
}

extern "system" fn graph_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                VIEW.with(|v| paint(hwnd, dc, &v.borrow()));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE | WM_ENABLE => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                moved(hwnd, point(lparam));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                VIEW.with(|v| {
                    let mut view = v.borrow_mut();
                    if view.hovered.take().is_some() {
                        redraw(hwnd, &view);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let at = point(lparam);
                let hit = VIEW.with(|v| {
                    let mut view = v.borrow_mut();
                    let hit = view.hit(hwnd, at);
                    view.pressed = hit.map(|h| (h, at));
                    hit
                });
                if hit.is_some() {
                    SetCapture(hwnd);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if GetCapture() == hwnd {
                    let _ = ReleaseCapture();
                } else {
                    released(hwnd);
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                released(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                if let Some(handle) = VIEW.with(|v| v.borrow().hit(hwnd, point(lparam))) {
                    with_app(|app| app.model.reset_value(handle.path()));
                }
                LRESULT(0)
            }
            WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
                let cursor = match VIEW.with(|v| v.borrow().active()) {
                    Some(Handle::Middle) => IDC_SIZEWE,
                    Some(_) => IDC_SIZENS,
                    None => IDC_ARROW,
                };
                let _ = SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
                LRESULT(1)
            }
            // Scrolls the column, as over the sliders.
            WM_MOUSEWHEEL => SendMessageW(GetParent(hwnd).unwrap_or_default(), msg, wparam, lparam),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// MARK: Drawing

type Rgb = [f32; 3];

const WHITE: Rgb = [1.0, 1.0, 1.0];
const BLACK: Rgb = [0.0, 0.0, 0.0];
const YELLOW: Rgb = [1.0, 0.8, 0.0];
const CHANNEL_COLORS: [Rgb; 3] = [[1.0, 0.35, 0.35], [0.35, 0.9, 0.35], [0.4, 0.55, 1.0]];

/// An antialiased drawing: each shape's coverage is worked out in `mask`
/// and composited once, so a polyline's joints don't darken.
struct Raster {
    width: usize,
    height: usize,
    pixels: Vec<Rgb>,
    mask: Vec<f32>,
}

impl Raster {
    fn new(width: usize, height: usize, background: Rgb) -> Self {
        Self { width, height, pixels: vec![background; width * height], mask: vec![0.0; width * height] }
    }

    /// The pixels a box covers, clipped.
    fn span(&self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> (usize, usize, usize, usize) {
        let clip = |v: f64, n: usize| (v.max(0.0) as usize).min(n);
        (clip(x0.floor(), self.width), clip(y0.floor(), self.height), clip(x1.ceil() + 1.0, self.width), clip(y1.ceil() + 1.0, self.height))
    }

    /// Composites `color` over the mask's coverage within `bounds`, and clears it.
    fn composite(&mut self, bounds: (usize, usize, usize, usize), color: Rgb, alpha: f32) {
        let (x0, y0, x1, y1) = bounds;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * self.width + x;
                let a = std::mem::take(&mut self.mask[i]) * alpha;
                if a > 0.0 {
                    let p = &mut self.pixels[i];
                    for c in 0..3 {
                        p[c] += (color[c] - p[c]) * a;
                    }
                }
            }
        }
    }

    /// A shape given by its coverage at each pixel centre, within `bounds`.
    fn shape(&mut self, bounds: (f64, f64, f64, f64), color: Rgb, alpha: f32, coverage: impl Fn(f64, f64) -> f64) {
        let span = self.span(bounds);
        for y in span.1..span.3 {
            for x in span.0..span.2 {
                self.mask[y * self.width + x] = coverage(x as f64 + 0.5, y as f64 + 0.5).clamp(0.0, 1.0) as f32;
            }
        }
        self.composite(span, color, alpha);
    }

    fn stroke(&mut self, points: &[(f64, f64)], width: f64, color: Rgb, alpha: f32) {
        if points.len() < 2 {
            return;
        }
        let pad = width / 2.0 + 1.0;
        let mut all = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for pair in points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let bounds = (ax.min(bx) - pad, ay.min(by) - pad, ax.max(bx) + pad, ay.max(by) + pad);
            all = (all.0.min(bounds.0), all.1.min(bounds.1), all.2.max(bounds.2), all.3.max(bounds.3));
            let span = self.span(bounds);
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx * dx + dy * dy;
            for y in span.1..span.3 {
                for x in span.0..span.2 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let t = if length > 0.0 { (((px - ax) * dx + (py - ay) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
                    let d = (px - ax - t * dx).hypot(py - ay - t * dy);
                    let m = &mut self.mask[y * self.width + x];
                    *m = m.max((width / 2.0 + 0.5 - d).clamp(0.0, 1.0) as f32);
                }
            }
        }
        let span = self.span(all);
        self.composite(span, color, alpha);
    }

    /// The area under a polyline whose x increases, down to `bottom`.
    fn fill_under(&mut self, points: &[(f64, f64)], bottom: f64, color: Rgb, alpha: f32) {
        let (Some(first), Some(last)) = (points.first(), points.last()) else { return };
        let span = self.span((first.0, 0.0, last.0, bottom));
        let mut segment = 0;
        for x in span.0..span.2 {
            let px = x as f64 + 0.5;
            if px < first.0 || px > last.0 {
                continue;
            }
            while segment + 2 < points.len() && points[segment + 1].0 < px {
                segment += 1;
            }
            let ((ax, ay), (bx, by)) = (points[segment], points[segment + 1]);
            let top = if bx > ax { ay + (by - ay) * (px - ax) / (bx - ax) } else { ay };
            for y in span.1..span.3.min(bottom.ceil() as usize) {
                self.mask[y * self.width + x] = (y as f64 + 1.0 - top).clamp(0.0, 1.0) as f32;
            }
        }
        self.composite(span, color, alpha);
    }

    fn colorrefs(&self) -> Vec<u32> {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        self.pixels.iter().map(|p| byte(p[0]) << 16 | byte(p[1]) << 8 | byte(p[2])).collect()
    }
}

/// Signed distance from a rounded box centred at `c` with half-size `half`.
fn rounded_box(p: (f64, f64), c: (f64, f64), half: (f64, f64), radius: f64) -> f64 {
    let q = ((p.0 - c.0).abs() - half.0 + radius, (p.1 - c.1).abs() - half.1 + radius);
    q.0.max(0.0).hypot(q.1.max(0.0)) + q.0.max(q.1).min(0.0) - radius
}

fn paint(hwnd: HWND, dc: HDC, view: &View) {
    let geometry = view.geometry(hwnd);
    let (w, h) = (geometry.width as usize, geometry.height as usize);
    let face = unsafe { GetSysColor(COLOR_BTNFACE) };
    let face = [face & 0xff, face >> 8 & 0xff, face >> 16 & 0xff].map(|c| c as f32 / 255.0);
    let mut raster = Raster::new(w, h, face);
    let (gw, gh) = (geometry.width, geometry.height);
    raster.shape((0.0, 0.0, gw, gh), [0.1; 3], 1.0, |x, y| 0.5 - rounded_box((x, y), (gw / 2.0, gh / 2.0), (gw / 2.0, gh / 2.0), view.px(6.0)));
    // Grid: every 2 stops, mid-grey dashed; brightness quarters.
    let line = view.px(1.0).round().max(1.0);
    let crisp = |v: f64| v.floor() + 0.5;
    for stop in [-4.0, -2.0, 0.0, 2.0, 4.0] {
        let x = crisp(geometry.x(stop));
        if stop == 0.0 {
            let dash = view.px(3.0);
            let mut y = 0.0;
            while y < gh {
                raster.stroke(&[(x, y), (x, (y + dash).min(gh))], line, WHITE, 0.18);
                y += 2.0 * dash;
            }
        } else {
            raster.stroke(&[(x, 0.0), (x, gh)], line, WHITE, 0.07);
        }
    }
    for level in [0.25, 0.5, 0.75] {
        let y = crisp(geometry.y(level));
        raster.stroke(&[(0.0, y), (gw, y)], line, WHITE, 0.07);
    }
    // Disabled while scanning film directly; dimmed while a curve for other
    // settings stands in for the next.
    let strength = if !view.enabled { 0.35 } else if view.current { 1.0 } else { 0.55 };
    let channel = view.channel;
    // The photograph's film exposure, behind the curve.
    if let Some(histogram) = &view.histogram {
        let bins = histogram.bins(channel);
        let peak = bins.iter().copied().fold(0.0, f64::max);
        if peak > 0.0 {
            let (low, high) = histogram.range;
            let width = (high - low) / bins.len() as f64;
            let mut area = vec![(geometry.x(low), gh)];
            area.extend(bins.iter().enumerate().map(|(i, count)| (geometry.x(low + (i as f64 + 0.5) * width), gh - (count / peak).min(1.0) * gh * 0.55)));
            area.push((geometry.x(high), gh));
            raster.fill_under(&area, gh, channel.map_or(WHITE, |c| CHANNEL_COLORS[c]), 0.16 * strength as f32);
        }
    }
    if let Some(display) = view.display() {
        let path = |series: &[f64]| -> Vec<(f64, f64)> { display.stops.iter().zip(series).map(|(t, b)| (geometry.x(*t), geometry.y(*b))).collect() };
        let alpha = strength as f32;
        for c in 0..3 {
            let strong = channel == Some(c);
            raster.stroke(&path(&display.rgb[c]), view.px(if strong { 2.0 } else { 1.0 }), CHANNEL_COLORS[c], if strong { alpha } else { 0.35 * alpha });
        }
        if channel.is_none() {
            raster.stroke(&path(&display.brightness(None)), view.px(2.0), WHITE, alpha);
        }
        if let Some(Handle::Curve(t)) = view.active() {
            let tone = view.drag.as_ref().map_or(t, |d| d.tone);
            let (x, y) = (geometry.x(t), geometry.y(display.value(&display.brightness(channel), tone)));
            let r = view.px(3.0);
            raster.shape((x - r - 1.0, y - r - 1.0, x + r + 1.0, y + r + 1.0), WHITE, 0.6 * alpha, |px, py| r + 0.5 - (px - x).hypot(py - y));
        }
        for (handle, (x, y)) in geometry.handle_points(&display, channel) {
            let r = view.px(if handle.same(view.active()) { 6.0 } else { 4.5 });
            let bounds = (x - r - 2.0, y - r - 2.0, x + r + 2.0, y + r + 2.0);
            let distance = |px: f64, py: f64| {
                if handle == Handle::Middle { rounded_box((px, py), (x, y), (r, r), view.px(1.5)) } else { (px - x).hypot(py - y) - r }
            };
            raster.shape(bounds, if handle == Handle::Middle { YELLOW } else { WHITE }, alpha, |px, py| 0.5 - distance(px, py));
            raster.shape(bounds, BLACK, 0.8 * alpha, |px, py| line / 2.0 + 0.5 - distance(px, py).abs());
        }
    }
    let pixels = raster.colorrefs();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetDIBitsToDevice(dc, 0, 0, w as u32, h as u32, 0, 0, 0, h as u32, pixels.as_ptr() as *const _, &info, DIB_RGB_COLORS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_are_antialiased_and_joints_dont_darken() {
        let mut raster = Raster::new(20, 10, BLACK);
        // A horizontal line through pixel centres, in two segments.
        raster.stroke(&[(0.0, 4.5), (10.0, 4.5), (20.0, 4.5)], 1.0, WHITE, 1.0);
        assert_eq!(raster.pixels[4 * 20 + 10], WHITE);
        assert_eq!(raster.pixels[3 * 20 + 10], BLACK);
        // Between two rows: half on each.
        let mut raster = Raster::new(20, 10, BLACK);
        raster.stroke(&[(0.0, 5.0), (20.0, 5.0)], 1.0, WHITE, 1.0);
        assert!((raster.pixels[4 * 20 + 7][0] - 0.5).abs() < 1e-6 && (raster.pixels[5 * 20 + 7][0] - 0.5).abs() < 1e-6);
        assert!(raster.mask.iter().all(|m| *m == 0.0));
    }

    #[test]
    fn the_area_fills_below_its_edge() {
        let mut raster = Raster::new(10, 10, BLACK);
        raster.fill_under(&[(0.0, 10.0), (0.0, 4.5), (10.0, 4.5), (10.0, 10.0)], 10.0, WHITE, 1.0);
        assert_eq!(raster.pixels[3 * 10 + 5], BLACK);
        assert!((raster.pixels[4 * 10 + 5][0] - 0.5).abs() < 1e-6);
        assert_eq!(raster.pixels[9 * 10 + 5], WHITE);
    }

    #[test]
    fn channel_buttons_name_a_layer_or_the_neutral_curve() {
        assert_eq!(CHANNELS.map(channel), [None, Some(0), Some(1), Some(2)]);
        assert_eq!(channel("unknown"), None);
    }
}
