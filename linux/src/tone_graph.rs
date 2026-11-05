//! The tone graph for the print curves (ticket 20, prototype), a port of
//! `ToneGraph.swift`: the paper's real response to a ramp of greys, with the
//! photograph's film exposure behind it; dragging it sets Print exposure and
//! the print curves' gammas. Adapted from SpektraMac's Print tab "Contrast"
//! graph (GPL-3.0). The GTK graph and scheduler are defined below.

use serde_json::Value;

mod json {
    pub use crate::parameters::{get, set};
    pub fn number(value: &serde_json::Value) -> f64 {
        value.as_f64().unwrap_or(0.0)
    }
    pub fn boolean(value: &serde_json::Value) -> bool {
        value.as_bool().unwrap_or(false)
    }
    pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        a == b
    }
}
fn snap(value: f64, low: f64, high: f64, step: f64) -> f64 {
    ((value.clamp(low, high) / step).round() * step).clamp(low, high)
}

use crate::Editor;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const ACTION: &str = "Change Tone Curve";

/// Authoritative helper data and optional GTK widgets. Nothing here owns a
/// photograph buffer, and hidden graphs never submit helper work.
#[derive(Default)]
pub struct Panel {
    curve: RefCell<Option<Curve>>,
    histogram: RefCell<Option<FilmHistogram>>,
    area: RefCell<Option<gtk::DrawingArea>>,
    label: RefCell<Option<gtk::Label>>,
    last_request: RefCell<Value>,
    in_flight: Cell<bool>,
    epoch: Cell<u64>,
    drag: RefCell<Option<(Drag, (f64, f64))>>,
    hovered: Cell<Option<Handle>>,
    keyboard: Cell<Option<Handle>>,
}

fn selected(ui: &Editor) -> Option<usize> {
    match ui
        .preferences
        .borrow()
        .tone_graph_channel
        .to_ascii_lowercase()
        .as_str()
    {
        "r" => Some(0),
        "g" => Some(1),
        "b" => Some(2),
        _ => None,
    }
}

fn params(ui: &Editor) -> Value {
    crate::parameters::runtime(&ui.history.borrow().state, &ui.runtime.borrow().defaults)
}

fn controls(ui: &Editor) -> Controls {
    let params = params(ui);
    Controls::read(|path| json::get(&params, path))
}

fn current_request(ui: &Editor) -> Value {
    let state = ui.history.borrow().state.clone();
    request(
        &crate::catalog::selected(&state, false).id,
        &crate::catalog::selected(&state, true).id,
        &crate::parameters::runtime(&state, &ui.runtime.borrow().defaults),
    )
}

fn enabled(ui: &Editor) -> bool {
    !ui.canvas.cropping()
        && !crate::inspector::flag(&ui.history.borrow().state, "Scan film directly", false)
}

fn geometry(area: &gtk::DrawingArea) -> Geometry {
    Geometry {
        width: area.width().max(1) as f64,
        height: area.height().max(1) as f64,
        inset: 8.,
    }
}

fn redraw(ui: &Editor) {
    if let Some(area) = ui.tone_graph.area.borrow().as_ref() {
        area.queue_draw();
    }
    if let Some(label) = ui.tone_graph.label.borrow().as_ref() {
        let active = ui
            .tone_graph
            .drag
            .borrow()
            .as_ref()
            .map(|d| d.0.handle)
            .or(ui.tone_graph.hovered.get())
            .or(ui.tone_graph.keyboard.get());
        let text = if let Some(handle) = active {
            format!(
                "{} {:.2} · {}",
                handle.title(),
                json::get(&params(ui), handle.path()).as_f64().unwrap_or(1.),
                handle.hint()
            )
        } else {
            "Drag the curve or a handle".into()
        };
        if label.text().as_str() != text {
            label.set_text(&text);
        }
    }
}

pub fn clear(ui: &Editor) {
    ui.tone_graph
        .epoch
        .set(ui.tone_graph.epoch.get().wrapping_add(1));
    ui.tone_graph.curve.take();
    ui.tone_graph.histogram.take();
    ui.tone_graph.last_request.replace(Value::Null);
    let had_drag = ui.tone_graph.drag.take().is_some();
    if had_drag {
        ui.runtime.borrow_mut().interacting = false;
        ui.history.borrow_mut().commit();
    }
    ui.tone_graph.hovered.set(None);
    redraw(ui);
}

/// Called only for a render whose source/revision survived the main scheduler.
pub fn rendered(ui: &Rc<Editor>, reply: &Value) {
    ui.tone_graph
        .histogram
        .replace(FilmHistogram::parse(&reply["film_histogram"]));
    redraw(ui);
    pump(ui);
}

/// One optional curve request at a time. Main rendering has priority; changed
/// requests are coalesced by inspecting current settings again on completion.
pub fn pump(ui: &Rc<Editor>) {
    redraw(ui);
    let shown = ui
        .tone_graph
        .area
        .borrow()
        .as_ref()
        .is_some_and(|a| a.is_mapped());
    if !shown || !enabled(ui) || ui.tone_graph.in_flight.get() || crate::brightness::pending(ui) {
        return;
    }
    let runtime = ui.runtime.borrow();
    if !runtime.ready
        || !runtime.supports_tone_curve
        || runtime.closing
        || runtime.rendering
        || runtime.interacting
        || runtime.exporting.is_some()
        || runtime.block_helper_render
        || runtime.balancing_print
        || (runtime.source.is_some() && runtime.current_revision != Some(runtime.revision))
    {
        return;
    }
    let Some(client) = runtime.client.clone() else {
        return;
    };
    let generation = runtime.source_generation;
    drop(runtime);
    let request = current_request(ui);
    if *ui.tone_graph.last_request.borrow() == request {
        return;
    }
    let values = controls(ui);
    let mut command = request.clone();
    command["command"] = serde_json::json!("tone_curve");
    command["count"] = serde_json::json!(POINTS);
    ui.tone_graph.last_request.replace(request.clone());
    ui.tone_graph.in_flight.set(true);
    let epoch = ui.tone_graph.epoch.get();
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let result =
            gio::spawn_blocking(move || client.request(command, Duration::from_secs(60))).await;
        ui.tone_graph.in_flight.set(false);
        if ui.runtime.borrow().closing {
            return;
        }
        if epoch == ui.tone_graph.epoch.get() && generation == ui.runtime.borrow().source_generation
        {
            if let Ok(Ok((_, reply))) = result {
                if let Some(curve) = Curve::parse(&reply, request, values) {
                    ui.tone_graph.curve.replace(Some(curve));
                }
            }
        }
        redraw(&ui);
        pump(&ui);
    });
}

fn key_for(path: &str) -> &'static str {
    match path {
        PRINT_EXPOSURE => "Enlarger.Print exposure",
        GAMMA => "Print Curves.Gamma",
        FAST => "Print Curves.Fast gamma",
        _ => "Print Curves.Slow gamma",
    }
}

fn set_value(ui: &Editor, path: &str, value: f64, complete: bool) {
    let mut history = ui.history.borrow_mut();
    history.begin(ACTION);
    history.state.values.insert(key_for(path).into(), value);
    if path != PRINT_EXPOSURE {
        history
            .state
            .choices
            .insert("Print Curves enabled".into(), "true".into());
    }
    if complete {
        history.commit();
    }
    drop(history);
    ui.sync_state();
    ui.observe_edit();
    redraw(ui);
}

fn hit(ui: &Editor, area: &gtk::DrawingArea, point: (f64, f64)) -> Option<Handle> {
    if !enabled(ui) {
        return None;
    }
    let curve = ui.tone_graph.curve.borrow();
    let display = curve.as_ref()?.predicted(controls(ui));
    geometry(area).hit(&display, selected(ui), point, 10., 12.)
}

fn finish_drag(ui: &Rc<Editor>) {
    if ui.tone_graph.drag.borrow().is_none() {
        return;
    }
    // GTK can cancel or unmap a gesture while sync_history is visiting rows
    // under its read borrow. Finish once that update has released the state.
    if ui.history.try_borrow_mut().is_err() {
        let ui = ui.clone();
        glib::idle_add_local_once(move || finish_drag(&ui));
        return;
    }
    let had_drag = ui.tone_graph.drag.borrow_mut().take().is_some();
    if had_drag {
        ui.runtime.borrow_mut().interacting = false;
        ui.history.borrow_mut().commit();
        ui.sync_state();
        ui.observe_edit();
        pump(ui);
    }
}

pub fn build(ui: &Rc<Editor>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let channels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    channels.add_css_class("linked");
    let mut first: Option<gtk::ToggleButton> = None;
    for name in ["RGB", "R", "G", "B"] {
        let button = gtk::ToggleButton::with_label(name);
        button.set_hexpand(true);
        button.set_group(first.as_ref());
        if first.is_none() {
            first = Some(button.clone());
        }
        let chosen = match name {
            "R" => Some(0),
            "G" => Some(1),
            "B" => Some(2),
            _ => None,
        };
        button.set_active(selected(ui) == chosen);
        button.set_tooltip_text(Some(match name {
            "R" => "Show the red layer’s print response.",
            "G" => "Show the green layer’s print response.",
            "B" => "Show the blue layer’s print response.",
            _ => "Show the neutral print response.",
        }));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Tone graph {name}"
        ))]);
        button.connect_toggled(glib::clone!(
            #[weak]
            ui,
            move |button| if button.is_active() {
                ui.preferences.borrow_mut().tone_graph_channel = name.into();
                redraw(&ui);
            }
        ));
        channels.append(&button);
    }
    panel.append(&channels);
    let area = gtk::DrawingArea::new();
    area.set_content_height(170);
    area.set_hexpand(true);
    area.set_focusable(true);
    area.set_tooltip_text(Some(crate::help::TONE_GRAPH));
    area.update_property(&[gtk::accessible::Property::Label("Print tone graph"),
        gtk::accessible::Property::Description("Print brightness against film exposure. Keys 1, 2, 3 and 4 select exposure, shadows, highlights and contrast; arrow keys adjust; Home resets.")]);
    ui.tone_graph.area.replace(Some(area.clone()));
    area.set_draw_func(glib::clone!(
        #[weak]
        ui,
        move |_, cr, w, h| paint(&ui, cr, w, h)
    ));
    area.connect_map(glib::clone!(
        #[weak]
        ui,
        move |_| pump(&ui)
    ));
    area.connect_unmap(glib::clone!(
        #[weak]
        ui,
        move |_| finish_drag(&ui)
    ));
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin(glib::clone!(
        #[weak]
        ui,
        #[weak]
        area,
        move |gesture, x, y| {
            let Some(handle) = hit(&ui, &area, (x, y)) else {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            };
            let curve = ui.tone_graph.curve.borrow().clone();
            let Some(curve) = curve else { return };
            let display = curve.predicted(controls(&ui));
            if let Some(drag) = Drag::start(handle, &curve, &display, selected(&ui)) {
                area.grab_focus();
                ui.history.borrow_mut().begin(ACTION);
                ui.runtime.borrow_mut().interacting = true;
                ui.tone_graph.drag.replace(Some((drag, (x, y))));
                redraw(&ui);
            }
        }
    ));
    drag.connect_drag_update(glib::clone!(
        #[weak]
        ui,
        #[weak]
        area,
        move |_, dx, dy| {
            let next = ui
                .tone_graph
                .drag
                .borrow()
                .as_ref()
                .and_then(|(drag, (x, y))| {
                    drag.value(
                        controls(&ui),
                        selected(&ui),
                        &geometry(&area),
                        (x + dx, y + dy),
                    )
                });
            if let Some((path, value)) = next {
                set_value(&ui, path, value, false);
            }
        }
    ));
    drag.connect_drag_end(glib::clone!(
        #[weak]
        ui,
        move |_, _, _| finish_drag(&ui)
    ));
    drag.connect_cancel(glib::clone!(
        #[weak]
        ui,
        move |_, _| finish_drag(&ui)
    ));
    area.add_controller(drag);
    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_pressed(glib::clone!(
        #[weak]
        ui,
        #[weak]
        area,
        move |_, count, x, y| if count == 2 {
            if let Some(handle) = hit(&ui, &area, (x, y)) {
                set_value(&ui, handle.path(), 1., true);
            }
        }
    ));
    area.add_controller(click);
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[weak]
        ui,
        #[weak]
        area,
        move |_, x, y| {
            let handle = hit(&ui, &area, (x, y));
            ui.tone_graph.hovered.set(handle);
            area.set_cursor_from_name(Some(if handle == Some(Handle::Middle) {
                "ew-resize"
            } else if handle.is_some() {
                "ns-resize"
            } else {
                "default"
            }));
            redraw(&ui);
        }
    ));
    motion.connect_leave(glib::clone!(
        #[weak]
        ui,
        move |_| {
            ui.tone_graph.hovered.set(None);
            redraw(&ui);
        }
    ));
    area.add_controller(motion);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if !enabled(&ui) {
                return glib::Propagation::Proceed;
            }
            let select = match key {
                gdk::Key::_1 => Some(Handle::Middle),
                gdk::Key::_2 => Some(Handle::Shadows),
                gdk::Key::_3 => Some(Handle::Highlights),
                gdk::Key::_4 => Some(Handle::Curve(3.)),
                _ => None,
            };
            if let Some(select) = select {
                ui.tone_graph.keyboard.set(Some(select));
                redraw(&ui);
                return glib::Propagation::Stop;
            }
            let handle = ui.tone_graph.keyboard.get().unwrap_or(Handle::Middle);
            let value = json::get(&params(&ui), handle.path())
                .as_f64()
                .unwrap_or(1.);
            let range = if handle == Handle::Middle {
                EXPOSURE_RANGE
            } else {
                GAMMA_RANGE
            };
            let value = match key {
                gdk::Key::Left | gdk::Key::Down => snap(value - STEP, range.0, range.1, STEP),
                gdk::Key::Right | gdk::Key::Up => snap(value + STEP, range.0, range.1, STEP),
                gdk::Key::Home => 1.,
                _ => return glib::Propagation::Proceed,
            };
            set_value(&ui, handle.path(), value, true);
            glib::Propagation::Stop
        }
    ));
    area.add_controller(keys);
    panel.append(&area);
    let label = gtk::Label::new(Some("Drag the curve or a handle"));
    label.add_css_class("caption");
    label.set_xalign(0.);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    ui.tone_graph.label.replace(Some(label.clone()));
    panel.append(&label);
    panel
}

fn paint(ui: &Editor, cr: &gtk::cairo::Context, width: i32, height: i32) {
    let geometry = Geometry {
        width: width as f64,
        height: height as f64,
        inset: 8.,
    };
    cr.set_source_rgb(0.095, 0.098, 0.106);
    let _ = cr.paint();
    cr.set_source_rgba(1., 1., 1., 0.10);
    cr.set_line_width(0.5);
    for stop in [-4., -2., 0., 2., 4.] {
        let x = geometry.x(stop);
        cr.move_to(x, 0.);
        cr.line_to(x, height as f64);
    }
    for f in [0.25, 0.5, 0.75] {
        let y = geometry.y(f);
        cr.move_to(0., y);
        cr.line_to(width as f64, y);
    }
    let _ = cr.stroke();
    let curve = ui.tone_graph.curve.borrow();
    let Some(curve) = curve.as_ref() else { return };
    let strength = if !enabled(ui) {
        0.35
    } else if curve.matches(&current_request(ui)) {
        1.
    } else {
        0.55
    };
    let channel = selected(ui);
    let colors = [(1., 0.35, 0.35), (0.35, 0.9, 0.35), (0.4, 0.55, 1.)];
    if let Some(histogram) = ui.tone_graph.histogram.borrow().as_ref() {
        let bins = histogram.bins(channel);
        let peak = bins.iter().copied().fold(0., f64::max);
        if peak > 0. {
            let (r, g, b) = channel.map_or((1., 1., 1.), |i| colors[i]);
            cr.set_source_rgba(r, g, b, 0.16 * strength);
            cr.move_to(geometry.x(histogram.range.0), height as f64);
            for (i, count) in bins.iter().enumerate() {
                let stop = histogram.range.0
                    + (i as f64 + 0.5) / bins.len() as f64
                        * (histogram.range.1 - histogram.range.0);
                cr.line_to(geometry.x(stop), height as f64 * (1. - count / peak * 0.55));
            }
            cr.line_to(geometry.x(histogram.range.1), height as f64);
            cr.close_path();
            let _ = cr.fill();
        }
    }
    let display = curve.predicted(controls(ui));
    let stroke = |series: &[f64], color: (f64, f64, f64), alpha: f64, line: f64| {
        cr.set_source_rgba(color.0, color.1, color.2, alpha * strength);
        cr.set_line_width(line);
        for (i, (t, b)) in display.stops.iter().zip(series).enumerate() {
            if i == 0 {
                cr.move_to(geometry.x(*t), geometry.y(*b));
            } else {
                cr.line_to(geometry.x(*t), geometry.y(*b));
            }
        }
        let _ = cr.stroke();
    };
    for i in 0..3 {
        stroke(
            &display.rgb[i],
            colors[i],
            if channel == Some(i) { 1. } else { 0.35 },
            if channel == Some(i) { 2. } else { 1. },
        );
    }
    if channel.is_none() {
        stroke(&display.brightness(None), (1., 1., 1.), 1., 2.);
    }
    for (handle, (x, y)) in geometry.handle_points(&display, channel) {
        if handle == Handle::Middle {
            cr.set_source_rgba(1., 0.8, 0., strength);
            cr.rectangle(x - 4.5, y - 4.5, 9., 9.);
        } else {
            cr.set_source_rgba(1., 1., 1., strength);
            cr.arc(x, y, 4.5, 0., 2. * std::f64::consts::PI);
        }
        let _ = cr.fill_preserve();
        cr.set_source_rgba(0., 0., 0., 0.8 * strength);
        cr.set_line_width(1.);
        let _ = cr.stroke();
    }
}

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
        Self {
            print_exposure: 1.0,
            gamma: 1.0,
            fast: 1.0,
            slow: 1.0,
        }
    }
}

impl Controls {
    pub fn read<'a>(value: impl Fn(&str) -> &'a Value) -> Self {
        let positive = |path: &str| {
            Some(json::number(value(path)))
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0)
        };
        let mut controls = Self {
            print_exposure: positive(PRINT_EXPOSURE),
            ..Self::default()
        };
        if json::boolean(value(ACTIVE)) {
            (controls.gamma, controls.fast, controls.slow) =
                (positive(GAMMA), positive(FAST), positive(SLOW));
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
            return Display {
                stops: self.stops.clone(),
                rgb: self.rgb.clone(),
                paper: self.paper.clone(),
            };
        };
        let shift = (next.print_exposure / self.controls.print_exposure).log10();
        let gamma = next.gamma / self.controls.gamma;
        let (slow, fast) = (
            next.slow / self.controls.slow,
            next.fast / self.controls.fast,
        );
        let (mut rgb, mut out) = (self.rgb.clone(), paper.clone());
        for c in 0..3 {
            for i in 0..self.stops.len() {
                let x = paper[c][i] + shift;
                let k = gamma * if x >= 0.0 { slow } else { fast };
                out[c][i] = x;
                rgb[c][i] = interpolate(&self.rgb[c], index_of_decreasing(&paper[c], k * x));
            }
        }
        Display {
            stops: self.stops.clone(),
            rgb,
            paper: Some(out),
        }
    }
}

fn brightness(rgb: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    match channel {
        Some(c) => rgb[c].clone(),
        None => (0..rgb[0].len())
            .map(|i| Curve::luma(rgb[0][i], rgb[1][i], rgb[2][i]))
            .collect(),
    }
}

fn mean_or(series: &[Vec<f64>; 3], channel: Option<usize>) -> Vec<f64> {
    match channel {
        Some(c) => series[c].clone(),
        None => (0..series[0].len())
            .map(|i| (series[0][i] + series[1][i] + series[2][i]) / 3.0)
            .collect(),
    }
}

/// The fractional index where a decreasing series reaches `value`, clamped to the ends.
pub fn index_of_decreasing(series: &[f64], value: f64) -> f64 {
    let (Some(&first), Some(&last)) = (series.first(), series.last()) else {
        return 0.0;
    };
    if value >= first {
        return 0.0;
    }
    if value <= last {
        return (series.len() - 1) as f64;
    }
    let (mut lo, mut hi) = (0, series.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if series[mid] > value {
            lo = mid
        } else {
            hi = mid
        }
    }
    let span = series[lo] - series[hi];
    lo as f64
        + if span > 0.0 {
            (series[lo] - value) / span
        } else {
            0.0
        }
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
        let stops: Vec<f64> = value["stops"]
            .as_array()?
            .iter()
            .map(|v| v.as_f64())
            .collect::<Option<_>>()?;
        let counts: Vec<Vec<f64>> = value["counts"]
            .as_array()?
            .iter()
            .map(|layer| {
                layer
                    .as_array()?
                    .iter()
                    .map(|v| v.as_f64())
                    .collect::<Option<Vec<f64>>>()
            })
            .collect::<Option<_>>()?;
        let [low, high] = stops[..] else { return None };
        let bins = counts.first()?.len();
        if counts.len() != 3 || high <= low || bins < 2 || counts.iter().any(|c| c.len() != bins) {
            return None;
        }
        let mut counts = counts.into_iter();
        Some(Self {
            range: (low, high),
            counts: [0; 3].map(|_| counts.next().unwrap()),
        })
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

    pub fn handle_points(
        &self,
        display: &Display,
        channel: Option<usize>,
    ) -> Vec<(Handle, (f64, f64))> {
        let Some((shadows, middle, highlights)) = display.handles(channel) else {
            return vec![];
        };
        let curve = display.brightness(channel);
        [
            (Handle::Shadows, shadows),
            (Handle::Middle, middle),
            (Handle::Highlights, highlights),
        ]
        .into_iter()
        .map(|(handle, t)| (handle, (self.x(t), self.y(display.value(&curve, t)))))
        .collect()
    }

    /// The handle within `radius` of `point`, or the curve within `tolerance` of it.
    pub fn hit(
        &self,
        display: &Display,
        channel: Option<usize>,
        point: (f64, f64),
        radius: f64,
        tolerance: f64,
    ) -> Option<Handle> {
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
    pub fn start(
        handle: Handle,
        base: &Curve,
        display: &Display,
        channel: Option<usize>,
    ) -> Option<Self> {
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
        Some(Self {
            handle,
            base: base.clone(),
            tone,
        })
    }

    /// The control the pointer at `point` asks for, snapped to its slider's
    /// range and step; None where it can't be solved.
    pub fn value(
        &self,
        current: Controls,
        channel: Option<usize>,
        geometry: &Geometry,
        point: (f64, f64),
    ) -> Option<(&'static str, f64)> {
        let base = &self.base;
        let start = base.controls;
        let paper = base.paper_exposure(channel)?;
        let (path, value, range) = match self.handle {
            Handle::Middle => {
                // Paper exposure 0 under the pointer: new = base · 10^(−paper(t)).
                let t = geometry.stops(point.0);
                let p = interpolate(&paper, index_of_increasing(&base.stops, t));
                (
                    PRINT_EXPOSURE,
                    start.print_exposure * 10f64.powf(-p),
                    EXPOSURE_RANGE,
                )
            }
            handle => {
                // The tone on the base curve that already has the pointer's
                // brightness; the scale is its paper exposure over the handle's.
                let target = geometry.brightness(point.1);
                let reached = interpolate(
                    &paper,
                    index_of_increasing(&base.brightness(channel), target),
                );
                let shift = (current.print_exposure / start.print_exposure).log10();
                let at = interpolate(&paper, index_of_increasing(&base.stops, self.tone)) + shift;
                if at.abs() <= 0.03 {
                    return None;
                }
                let k = (reached / at).max(0.01);
                let side = if at >= 0.0 {
                    current.slow / start.slow
                } else {
                    current.fast / start.fast
                };
                let gamma = current.gamma / start.gamma;
                match handle {
                    Handle::Shadows => (SLOW, start.slow * k / gamma, GAMMA_RANGE),
                    Handle::Highlights => (FAST, start.fast * k / gamma, GAMMA_RANGE),
                    _ => (GAMMA, start.gamma * k / side, GAMMA_RANGE),
                }
            }
        };
        value
            .is_finite()
            .then(|| (path, snap(value, range.0, range.1, STEP)))
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

    const GEOMETRY: Geometry = Geometry {
        width: 300.0,
        height: 170.0,
        inset: 8.0,
    };

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
        assert_eq!(
            off,
            Controls {
                print_exposure: 1.5,
                ..Controls::default()
            }
        );
        let mut on = params.clone();
        json::set(&mut on, ACTIVE, json!(true));
        let read = Controls::read(|p| json::get(&on, p));
        // A gamma of 0 isn't one the engine uses: 1.
        assert_eq!(
            read,
            Controls {
                print_exposure: 1.5,
                gamma: 1.3,
                fast: 0.8,
                slow: 1.0
            }
        );
    }

    #[test]
    fn malformed_replies_are_refused() {
        let controls = Controls::default();
        let r = json!({});
        assert!(
            Curve::parse(
                &json!({"film": [[0, 0, 0]], "rgb": [[0, 0, 0]]}),
                r.clone(),
                controls
            )
            .is_none()
        );
        assert!(
            Curve::parse(
                &json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0]]}),
                r.clone(),
                controls
            )
            .is_none()
        );
        assert!(
            Curve::parse(
                &json!({"film": [[0, 0], [1, 1]], "rgb": [[0, 0], [1, 1]]}),
                r.clone(),
                controls
            )
            .is_none()
        );
        // Scanning the film directly: no paper, the curve alone.
        let scan = Curve::parse(
            &json!({"film": [[0, 0, 0], [1, 1, 1]], "paper": null, "rgb": [[0, 0, 0], [1, 1, 1]]}),
            r,
            controls,
        )
        .unwrap();
        assert!(scan.paper.is_none());
        assert!(
            scan.predicted(Controls {
                print_exposure: 2.0,
                ..controls
            })
            .handles(None)
            .is_none()
        );
        assert!(
            FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4]]})).is_none()
        );
        assert!(
            FilmHistogram::parse(&json!({"stops": [8, -8], "counts": [[1, 2], [3, 4], [5, 6]]}))
                .is_none()
        );
        let histogram =
            FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4], [5, 6]]}))
                .unwrap();
        assert_eq!(histogram.bins(None), vec![3.0, 4.0]);
        assert_eq!(histogram.bins(Some(2)), vec![5.0, 6.0]);
    }

    #[test]
    fn a_curve_stands_in_only_for_the_same_settings() {
        let params = json!({"enlarger": {"print_exposure": 1, "y_filter_shift": 0}});
        let base = Curve::parse(
            &json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0], [1, 1, 1]]}),
            request("f", "p", &params),
            Controls::default(),
        )
        .unwrap();
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
        let brighter = base.predicted(Controls {
            print_exposure: 2.0,
            ..Controls::default()
        });
        let shift = 2f64.log10() / 0.3;
        for i in [40, 80, 120] {
            let expected = 0.5 + 0.05 * (base.stops[i] - shift);
            assert!(
                (brighter.rgb[1][i] - expected).abs() < 1e-9,
                "{i}: {} vs {expected}",
                brighter.rgb[1][i]
            );
        }
        let handles = brighter.handles(None).unwrap();
        assert!((handles.1 - shift).abs() < 1e-9);
        // Gamma 1.25 about mid-grey: the paper exposure scales by it.
        let steeper = base.predicted(Controls {
            gamma: 1.25,
            ..Controls::default()
        });
        for i in [50, 110] {
            assert!((steeper.rgb[0][i] - (0.5 + 0.05 * 1.25 * base.stops[i])).abs() < 1e-9);
        }
        // Slow gamma changes only the shadows' side, above paper exposure 0.
        let slow = base.predicted(Controls {
            slow: 1.5,
            ..Controls::default()
        });
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
        assert_eq!(
            GEOMETRY.hit(&display, None, (at.0 + 3.0, at.1), 10.0, 12.0),
            Some(Handle::Middle)
        );
        let t = 4.0;
        let on_curve = (GEOMETRY.x(t), GEOMETRY.y(0.5 + 0.05 * t) + 5.0);
        assert!(
            matches!(GEOMETRY.hit(&display, None, on_curve, 10.0, 12.0), Some(Handle::Curve(s)) if (s - t).abs() < 1e-9)
        );
        assert_eq!(
            GEOMETRY.hit(&display, None, (on_curve.0, on_curve.1 + 30.0), 10.0, 12.0),
            None
        );
    }

    #[test]
    fn the_middle_handle_puts_mid_grey_under_the_pointer() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        let drag = Drag::start(Handle::Middle, &base, &display, None).unwrap();
        // One stop right: paper −0.3 there, so 10^0.3 = 1.995 → 2.00.
        let (path, value) = drag
            .value(base.controls, None, &GEOMETRY, (GEOMETRY.x(1.0), 0.0))
            .unwrap();
        assert_eq!((path, value), (PRINT_EXPOSURE, 2.0));
        // Far left: clamped to the slider's minimum.
        assert_eq!(
            drag.value(base.controls, None, &GEOMETRY, (0.0, 0.0))
                .unwrap()
                .1,
            EXPOSURE_RANGE.0
        );
    }

    #[test]
    fn contrast_drags_scale_the_paper_exposure_at_their_tone() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        // Dragging the curve at +2 stops (paper −0.6) to the brightness the
        // base curve has at +2.4 stops (paper −0.72): gamma 1.2.
        let drag = Drag::start(Handle::Curve(2.0), &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        assert_eq!(
            drag.value(base.controls, None, &GEOMETRY, point),
            Some((GAMMA, 1.2))
        );
        // The shadows' handle at −2 stops, moved to the base's −2.5: slow 1.25.
        let drag = Drag::start(Handle::Shadows, &base, &display, None).unwrap();
        assert!(close(drag.tone, -2.0));
        let point = (GEOMETRY.x(-2.0), GEOMETRY.y(0.5 - 0.05 * 2.5));
        assert_eq!(
            drag.value(base.controls, None, &GEOMETRY, point),
            Some((SLOW, 1.25))
        );
        // A drag at mid-grey can't scale anything.
        let drag = Drag::start(Handle::Curve(0.0), &base, &display, None).unwrap();
        assert_eq!(
            drag.value(base.controls, None, &GEOMETRY, (150.0, 20.0)),
            None
        );
        // Solved on the drag's own curve: a gamma already raised mid-drag
        // is divided out of the next slow value.
        let drag = Drag::start(Handle::Highlights, &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        let raised = Controls {
            gamma: 1.2,
            ..base.controls
        };
        assert_eq!(
            drag.value(raised, None, &GEOMETRY, point),
            Some((FAST, 1.0))
        );
    }

    #[test]
    #[ignore = "Requires a desktop; run explicitly with --ignored --test-threads=1"]
    fn native_scope_and_graph_controls_share_history_and_reject_stale_counts() {
        use crate::{engine::Frame, perf::Perf};
        use std::{sync::Arc, time::Instant};
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.negs.ScopeGraphTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Editor::new(&app, Perf::new());
        let scopes = crate::scopes::build(&ui);
        ui.inspector.append(&scopes);
        let graph = build(&ui);
        ui.inspector.append(&graph);
        let area = ui.tone_graph.area.borrow().as_ref().unwrap().clone();
        ui.window.present();
        let settle = || {
            let deadline = Instant::now() + Duration::from_millis(150);
            while Instant::now() < deadline {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        settle();
        assert!(area.is_mapped() && area.width() > 0);
        let mode = scopes
            .first_child()
            .unwrap()
            .downcast::<gtk::DropDown>()
            .unwrap();
        mode.set_selected(1);
        assert_eq!(ui.preferences.borrow().scope_mode, "Waveform");
        let grip = scopes
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let controllers = grip.observe_controllers();
        let resize = (0..controllers.n_items())
            .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>())
            .unwrap();
        resize.emit_by_name::<()>("drag-begin", &[&0f64, &0f64]);
        resize.emit_by_name::<()>("drag-update", &[&0f64, &500f64]);
        assert_eq!(ui.preferences.borrow().scope_height, 420.);
        assert_eq!(
            ui.histogram.borrow().as_ref().unwrap().content_height(),
            420
        );
        // Make space for the graph again before using its measured geometry.
        resize.emit_by_name::<()>("drag-update", &[&0f64, &0f64]);
        settle();
        let frame = Arc::new(Frame {
            width: 100,
            height: 60,
            pixels: vec![128; 100 * 60 * 4].into(),
            result: json!({}),
        });
        ui.runtime.borrow_mut().set_developed(frame, false);
        crate::scopes::update(&ui);
        crate::scopes::clear(&ui);
        settle();
        assert!(
            ui.hist.borrow().is_none(),
            "An old worker count must not revive a cleared photograph"
        );
        crate::scopes::update(&ui);
        settle();
        assert_eq!(ui.hist.borrow().as_ref().unwrap()[0][128], 6000);
        let channels = graph.first_child().unwrap().downcast::<gtk::Box>().unwrap();
        channels
            .last_child()
            .unwrap()
            .downcast::<gtk::ToggleButton>()
            .unwrap()
            .set_active(true);
        assert_eq!(selected(&ui), Some(2));
        let base = curve(Controls::default());
        ui.tone_graph.curve.replace(Some(base.clone()));
        let controllers = area.observe_controllers();
        let gesture = (0..controllers.n_items())
            .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>())
            .unwrap();
        let geometry = geometry(&area);
        let display = base.predicted(base.controls);
        let (_, point) = geometry.handle_points(&display, selected(&ui))[1];
        let destination = geometry.x(1.);
        gesture.emit_by_name::<()>("drag-begin", &[&point.0, &point.1]);
        assert!(ui.runtime.borrow().interacting);
        gesture.emit_by_name::<()>("drag-update", &[&(destination - point.0), &0f64]);
        let borrowed = ui.history.borrow();
        gesture.emit_by_name::<()>("drag-end", &[&(destination - point.0), &0f64]);
        assert!(
            ui.runtime.borrow().interacting,
            "Finishing waits for the GTK state synchronization borrow"
        );
        drop(borrowed);
        settle();
        assert!(!ui.runtime.borrow().interacting);
        assert_eq!(
            ui.history.borrow().state.values["Enlarger.Print exposure"],
            2.
        );
        assert_eq!(ui.history.borrow_mut().undo().as_deref(), Some(ACTION));
        assert!(
            ui.history.borrow_mut().undo().is_none(),
            "A graph drag is one undo step"
        );
        ui.history.borrow_mut().redo();
        let click = (0..controllers.n_items())
            .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
            .unwrap();
        ui.sync_state();
        let moved = base.predicted(controls(&ui));
        let (_, point) = geometry.handle_points(&moved, selected(&ui))[1];
        click.emit_by_name::<()>("pressed", &[&2i32, &point.0, &point.1]);
        assert_eq!(
            ui.history.borrow().state.values["Enlarger.Print exposure"],
            1.
        );
        assert!(
            area.tooltip_text().is_some()
                && mode.tooltip_text().is_some()
                && grip.tooltip_text().is_some()
        );
        clear(&ui);
        assert!(
            ui.tone_graph.curve.borrow().is_none() && ui.tone_graph.histogram.borrow().is_none()
        );
        ui.runtime.borrow_mut().closing = true;
        ui.window.destroy();
    }
}
