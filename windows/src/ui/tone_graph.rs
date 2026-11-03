//! The tone graph's window (`ToneGraphView` in `ToneGraph.swift`). The
//! inspector owns its row, the label and the RGB/R/G/B buttons; this draws
//! the graph and turns the pointer into edits: the middle handle sideways
//! for Print exposure, the side handles and the curve up or down for the
//! gammas, each drag one undo step, double-click to reset. GDI draws only
//! aliased lines, so it is drawn with the shared anti-aliased `Canvas`.

use std::cell::RefCell;

use serde_json::Value;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::draw::{Canvas, RectF};
use super::theme::{self, Color};
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
        // It paints all of itself, from the palette.
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

/// The graph's colours. In light and dark it is a dark well like the
/// histogram's (and the Mac's graph); in high contrast, the system's
/// colours only, with the faint layers left out.
struct Ink {
    well: Color,
    line: Color,
    /// The grid's opacity, and the dashed zero line's.
    grid: (f32, f32),
    channels: [Color; 3],
    handle: Color,
    outline: Color,
    middle: Color,
    /// The layers that aren't chosen are drawn faintly behind.
    faint_layers: bool,
}

impl Ink {
    fn of(theme: theme::Theme) -> Self {
        let p = theme.palette;
        if theme.high_contrast {
            return Self {
                well: p.window,
                line: p.text,
                grid: (0.25, 0.5),
                channels: [p.text; 3],
                handle: p.window,
                outline: p.text,
                middle: p.accent,
                faint_layers: false,
            };
        }
        Self {
            well: theme::rgb(0x1a, 0x1a, 0x1a),
            line: theme::rgb(0xff, 0xff, 0xff),
            grid: (0.07, 0.18),
            channels: [theme::rgb(0xff, 0x59, 0x59), theme::rgb(0x59, 0xe6, 0x59), theme::rgb(0x66, 0x8c, 0xff)],
            handle: theme::rgb(0xff, 0xff, 0xff),
            outline: theme::rgb(0, 0, 0),
            middle: theme::rgb(0xff, 0xcc, 0x00),
            faint_layers: true,
        }
    }
}

/// Signed distance from a rounded box centred at `c` with half-size `half`.
fn rounded_box(p: (f32, f32), c: (f32, f32), half: (f32, f32), radius: f32) -> f32 {
    let q = ((p.0 - c.0).abs() - half.0 + radius, (p.1 - c.1).abs() - half.1 + radius);
    q.0.max(0.0).hypot(q.1.max(0.0)) + q.0.max(q.1).min(0.0) - radius
}

fn paint(hwnd: HWND, dc: HDC, view: &View) {
    let geometry = view.geometry(hwnd);
    let theme = theme::current();
    let ink = Ink::of(theme);
    let px = |dip: f64| view.px(dip) as f32;
    let at = |x: f64, y: f64| (x as f32, y as f32);
    let (gw, gh) = (geometry.width as f32, geometry.height as f32);
    let mut c = Canvas::new(dc, RECT { left: 0, top: 0, right: gw as i32, bottom: gh as i32 });
    c.clear(theme.palette.panel);
    let well = RectF::new(0.0, 0.0, gw, gh);
    c.round_rect(well, px(6.0), ink.well);
    // Grid: every 2 stops, the middle one dashed; brightness quarters.
    let line = px(1.0).round().max(1.0);
    let crisp = |v: f64| v.floor() as f32 + 0.5;
    for stop in [-4.0, -2.0, 0.0, 2.0, 4.0] {
        let x = crisp(geometry.x(stop));
        if stop == 0.0 {
            let dash = px(3.0);
            let mut y = 0.0;
            while y < gh {
                c.stroke(&[(x, y), (x, (y + dash).min(gh))], line, ink.line, ink.grid.1);
                y += 2.0 * dash;
            }
        } else {
            c.stroke(&[(x, 0.0), (x, gh)], line, ink.line, ink.grid.0);
        }
    }
    for level in [0.25, 0.5, 0.75] {
        let y = crisp(geometry.y(level));
        c.stroke(&[(0.0, y), (gw, y)], line, ink.line, ink.grid.0);
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
            let gh = geometry.height;
            let mut area = vec![at(geometry.x(low), gh)];
            area.extend(bins.iter().enumerate().map(|(i, count)| at(geometry.x(low + (i as f64 + 0.5) * width), gh - (count / peak).min(1.0) * gh * 0.55)));
            area.push(at(geometry.x(high), gh));
            c.fill_under(&area, gh as f32, channel.map_or(ink.line, |i| ink.channels[i]), 0.16 * strength);
        }
    }
    if let Some(display) = view.display() {
        let path = |series: &[f64]| -> Vec<(f32, f32)> { display.stops.iter().zip(series).map(|(t, b)| at(geometry.x(*t), geometry.y(*b))).collect() };
        let alpha = strength;
        for i in 0..3 {
            let strong = channel == Some(i);
            if strong || ink.faint_layers {
                c.stroke(&path(&display.rgb[i]), px(if strong { 2.0 } else { 1.0 }), ink.channels[i], if strong { alpha } else { 0.35 * alpha });
            }
        }
        if channel.is_none() {
            c.stroke(&path(&display.brightness(None)), px(2.0), ink.line, alpha);
        }
        if let Some(Handle::Curve(t)) = view.active() {
            let tone = view.drag.as_ref().map_or(t, |d| d.tone);
            let (x, y) = at(geometry.x(t), geometry.y(display.value(&display.brightness(channel), tone)));
            let r = px(3.0);
            c.shape((x - r, y - r, x + r, y + r), ink.line, 0.6 * alpha, |px, py| (px - x).hypot(py - y) - r);
        }
        for (handle, (x, y)) in geometry.handle_points(&display, channel) {
            let (x, y) = at(x, y);
            let r = px(if handle.same(view.active()) { 6.0 } else { 4.5 });
            let bounds = (x - r - 1.0, y - r - 1.0, x + r + 1.0, y + r + 1.0);
            let corner = px(1.5);
            let distance = |px: f32, py: f32| {
                if handle == Handle::Middle { rounded_box((px, py), (x, y), (r, r), corner) } else { (px - x).hypot(py - y) - r }
            };
            c.shape(bounds, if handle == Handle::Middle { ink.middle } else { ink.handle }, alpha, distance);
            c.shape(bounds, ink.outline, 0.8 * alpha, |px, py| distance(px, py).abs() - line / 2.0);
        }
    }
    if theme.high_contrast {
        c.round_frame(well, px(6.0), line, ink.line);
    }
    c.present(dc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_buttons_name_a_layer_or_the_neutral_curve() {
        assert_eq!(CHANNELS.map(channel), [None, Some(0), Some(1), Some(2)]);
        assert_eq!(channel("unknown"), None);
    }

    #[test]
    fn high_contrast_draws_in_the_system_colours_only() {
        let palette = theme::LIGHT;
        let ink = Ink::of(theme::Theme { palette, dark: false, high_contrast: true });
        let system = [palette.window, palette.text, palette.accent];
        let used = [ink.well, ink.line, ink.handle, ink.outline, ink.middle].into_iter().chain(ink.channels);
        assert!(used.into_iter().all(|c| system.contains(&c)));
        assert!(!ink.faint_layers);
    }
}
