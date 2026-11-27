//! Interactive Print Curves tone graph (`ToneGraphView`, `L22`) driven by
//! `negs_app::tone_graph`, `model.tone_curve`, and `model.tone_controls()`.
//!
//! Matches SwiftUI's redesigned `ToneGraph.swift`:
//! - Right-aligned inline `RGB · R · G · B` words (`11 pt` UI, active semibold
//!   `paper` + `4 px` `PencilStroke` / Light Table highlighter).
//! - Borderless `170 px` well in `theme.canvas` (`r = 3`), `line`/`track` grid,
//!   and dashed `track` diagonal.
//! - Circular `InkSlider`-style knobs (`panel` / `accent` / `paper`, `r = 4.5`
//!   or `5.4` when active).
//! - Italic Newsreader `shadows · midtones · highlights` axis labels and italic
//!   status hint underneath.

use gpui::{
    Bounds, Context, Corners, Edges, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, Point, SharedString, Window, canvas, div, fill, point,
    prelude::*, px, quad, size,
};
use negs_app::inspector_view::schema::{TONE_CHANNEL_BUTTONS, tone_channel};
use negs_app::inspector_view::{RowAction, RowId, RowView};
use negs_app::json;
use negs_app::model::Model;
use negs_app::tone_graph::{self, Drag, Geometry, Handle};
use serde_json::Value;

use crate::marks_gpui::{paint_highlighter, paint_pencil};
use crate::theme_gpui::{GpuiTheme, hex, scale};
use crate::widgets::controls::GUTTER;

const UNDO_ACTION: &str = "Change Tone Curve";
const GRAPH_HEIGHT: f32 = 170.0;
const GRAPH_INSET: f64 = 8.0;

/// Interactive state for the Print Curves tone graph.
#[derive(Default)]
pub struct ToneGraphState {
    pub bounds: Bounds<Pixels>,
    pub hovered: Option<Handle>,
    pub pressed: Option<(Handle, (f64, f64))>,
    pub drag: Option<Drag>,
}

impl ToneGraphState {
    pub fn is_dragging(&self) -> bool {
        self.pressed.is_some() || self.drag.is_some()
    }

    pub fn active(&self) -> Option<Handle> {
        self.drag.as_ref().map(|d| d.handle).or(self.hovered)
    }

    pub fn geometry(&self) -> Geometry {
        Geometry {
            width: f32::from(self.bounds.size.width).max(1.0) as f64,
            height: f32::from(self.bounds.size.height).max(1.0) as f64,
            inset: GRAPH_INSET,
        }
    }

    fn local_point(&self, pos: Point<Pixels>) -> (f64, f64) {
        (
            (f32::from(pos.x) - f32::from(self.bounds.origin.x)) as f64,
            (f32::from(pos.y) - f32::from(self.bounds.origin.y)) as f64,
        )
    }

    pub fn label(&self, model: &Model, decimal: char) -> String {
        let Some(active) = self.active() else {
            return if model.tone_curve.is_none() {
                "Tone graph".to_string()
            } else {
                "Drag the curve or a handle".to_string()
            };
        };
        let value = json::number(model.value(active.path()));
        let mut formatted = format!("{value:.2}");
        if decimal != '.' {
            formatted = formatted.replace('.', &decimal.to_string());
        }
        let hint = if self.drag.is_none() {
            format!(" · {}", active.hint())
        } else {
            String::new()
        };
        format!("{} {formatted}{hint}", active.title())
    }

    pub fn handle_mouse_down(
        &mut self,
        model: &mut Model,
        pos: Point<Pixels>,
        click_count: usize,
        enabled: bool,
    ) -> bool {
        if !enabled {
            return false;
        }
        let Some(curve) = model.tone_curve.as_ref() else {
            return false;
        };
        let display = curve.predicted(model.tone_controls());
        let channel = tone_channel(&model.prefs.tone_graph_channel);
        let at = self.local_point(pos);
        let hit = self.geometry().hit(&display, channel, at, 10.0, 12.0);

        if click_count >= 2 {
            if let Some(handle) = hit {
                model.reset_value(handle.path());
                self.pressed = None;
                self.drag = None;
                return true;
            }
            return false;
        }

        self.pressed = hit.map(|h| (h, at));
        hit.is_some()
    }

    pub fn handle_mouse_move(
        &mut self,
        model: &mut Model,
        pos: Point<Pixels>,
        enabled: bool,
    ) -> bool {
        let at = self.local_point(pos);
        let channel = tone_channel(&model.prefs.tone_graph_channel);
        let geometry = self.geometry();

        if let Some(drag) = &self.drag {
            if let Some((path, value)) =
                drag.value(model.tone_controls(), channel, &geometry, at)
            {
                if (json::number(model.value(path)) - value).abs() > 1e-9 {
                    model.set(path, json::num(value));
                }
            }
            return true;
        }

        if let Some((handle, from)) = self.pressed {
            if (at.0 - from.0).hypot(at.1 - from.1) < 1.0 {
                return false;
            }
            let Some(curve) = model.tone_curve.clone() else {
                self.pressed = None;
                return false;
            };
            let display = curve.predicted(model.tone_controls());
            self.pressed = None;
            self.hovered = None;
            self.drag = Drag::start(handle, &curve, &display, channel);
            if let Some(drag) = &self.drag {
                model.begin_interaction();
                if handle != Handle::Middle && !json::boolean(model.value(tone_graph::ACTIVE)) {
                    model.set(tone_graph::ACTIVE, Value::Bool(true));
                }
                if let Some((path, value)) =
                    drag.value(model.tone_controls(), channel, &geometry, at)
                {
                    if (json::number(model.value(path)) - value).abs() > 1e-9 {
                        model.set(path, json::num(value));
                    }
                }
                return true;
            }
            return false;
        }

        if !enabled {
            return self.hovered.take().is_some();
        }

        let hovered = model.tone_curve.as_ref().and_then(|curve| {
            let display = curve.predicted(model.tone_controls());
            geometry.hit(&display, channel, at, 10.0, 12.0)
        });
        if hovered != self.hovered {
            self.hovered = hovered;
            true
        } else {
            false
        }
    }

    pub fn handle_mouse_up(&mut self, model: &mut Model) -> bool {
        self.pressed = None;
        if self.drag.take().is_some() {
            model.end_interaction(UNDO_ACTION);
            return true;
        }
        false
    }
}

fn paint_curve_line(
    window: &mut Window,
    stops: &[f64],
    series: &[f64],
    origin: Point<Pixels>,
    geometry: &Geometry,
    width: f32,
    color: Hsla,
) {
    if stops.len() < 2 || stops.len() != series.len() {
        return;
    }
    let mut stroke = PathBuilder::stroke(px(width));
    for (i, (&t, &b)) in stops.iter().zip(series.iter()).enumerate() {
        let pt = point(
            origin.x + px(geometry.x(t) as f32),
            origin.y + px(geometry.y(b) as f32),
        );
        if i == 0 {
            stroke.move_to(pt);
        } else {
            stroke.line_to(pt);
        }
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, color);
    }
}

fn paint_dashed_segment(
    window: &mut Window,
    p0: (f32, f32),
    p1: (f32, f32),
    dash: f32,
    gap: f32,
    color: Hsla,
) {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let dist = dx.hypot(dy);
    if dist < 1.0 {
        return;
    }
    let ux = dx / dist;
    let uy = dy / dist;
    let step = dash + gap;
    let mut t = 0.0f32;
    while t < dist {
        let end = (t + dash).min(dist);
        let mut builder = PathBuilder::stroke(px(1.0));
        builder.move_to(point(px(p0.0 + ux * t), px(p0.1 + uy * t)));
        builder.line_to(point(px(p0.0 + ux * end), px(p0.1 + uy * end)));
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
        t += step;
    }
}

/// Renders the redesigned Print Curves tone graph (`L22`).
#[allow(clippy::too_many_arguments)]
pub fn render_tone_graph<V: 'static>(
    row: &RowView,
    model: &Model,
    state: &ToneGraphState,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_bounds: impl Fn(&mut V, Bounds<Pixels>) + 'static,
    on_channel: impl Fn(&mut V, RowId, RowAction, &mut Window, &mut Context<V>) + Clone + 'static,
    on_down: impl Fn(&mut V, &MouseDownEvent, bool, &mut Window, &mut Context<V>) + 'static,
    on_hover: impl Fn(&mut V, &MouseMoveEvent, bool, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let channel = row.selected;
    let status_label = state.label(model, '.');
    let active_handle = state.active();
    let dragging_handle = state.drag.as_ref().map(|d| d.handle);
    let drag_tone = state.drag.as_ref().map(|d| d.tone);

    let is_moved = |path: &str| {
        (json::number(model.value(path)) - json::number(model.default_value(path))).abs() > 1e-9
    };
    let modified_shadows = is_moved(Handle::Shadows.path());
    let modified_middle = is_moved(Handle::Middle.path());
    let modified_highlights = is_moved(Handle::Highlights.path());

    let display = model
        .tone_curve
        .as_ref()
        .map(|c| c.predicted(model.tone_controls()));
    let current = model.tone_curve_current();
    let film_histogram = model.film_histogram.clone();

    let entity = cx.entity().clone();
    let grid_line = theme.line;
    let track_line = theme.track;
    let paper_col = theme.paper;
    let panel_col = theme.panel;
    let accent_col = theme.accent;
    let label_col = theme.label;
    let channel_colors = [hex(0xff5959), hex(0x59e659), hex(0x668cff)];

    let graph_canvas = canvas(
        move |bounds: Bounds<Pixels>, _window, cx| {
            entity.update(cx, |this, _cx| {
                on_bounds(this, bounds);
            });
            bounds
        },
        move |bounds: Bounds<Pixels>, _prepaint: Bounds<Pixels>, window, _cx| {
            let gw = f32::from(bounds.size.width).max(1.0);
            let gh = f32::from(bounds.size.height).max(1.0);
            let geometry = Geometry {
                width: gw as f64,
                height: gh as f64,
                inset: GRAPH_INSET,
            };

            // Grid in `line` (stop 0 in `track`)
            for stop in [-4.0, -2.0, 0.0, 2.0, 4.0] {
                let x = bounds.origin.x + px((geometry.x(stop) as f32).floor());
                window.paint_quad(fill(
                    Bounds {
                        origin: point(x, bounds.origin.y),
                        size: size(px(1.0), bounds.size.height),
                    },
                    if stop == 0.0 { track_line } else { grid_line },
                ));
            }
            for level in [0.25, 0.5, 0.75] {
                let y = bounds.origin.y + px((geometry.y(level) as f32).floor());
                window.paint_quad(fill(
                    Bounds {
                        origin: point(bounds.origin.x, y),
                        size: size(bounds.size.width, px(1.0)),
                    },
                    grid_line,
                ));
            }
            // Dashed `track` diagonal for reference
            let ox = f32::from(bounds.origin.x);
            let oy = f32::from(bounds.origin.y);
            paint_dashed_segment(
                window,
                (ox, oy + geometry.y(0.0) as f32),
                (ox + gw, oy + geometry.y(1.0) as f32),
                3.0,
                3.0,
                track_line,
            );

            let strength: f32 = if !enabled {
                0.35
            } else if current {
                1.0
            } else {
                0.55
            };

            // Film exposure histogram backdrop
            if let Some(hist) = &film_histogram {
                let bins = hist.bins(channel);
                let peak = bins.iter().copied().fold(0.0f64, f64::max);
                if peak > 0.0 && bins.len() >= 2 {
                    let (low, high) = hist.range;
                    let bin_w = (high - low) / bins.len() as f64;
                    let bottom = bounds.origin.y + px(gh);
                    let mut area = PathBuilder::fill();
                    area.move_to(point(bounds.origin.x + px(geometry.x(low) as f32), bottom));
                    for (i, count) in bins.iter().enumerate() {
                        let x = geometry.x(low + (i as f64 + 0.5) * bin_w) as f32;
                        let y = gh - ((*count / peak).min(1.0) as f32) * gh * 0.55;
                        area.line_to(point(bounds.origin.x + px(x), bounds.origin.y + px(y)));
                    }
                    area.line_to(point(bounds.origin.x + px(geometry.x(high) as f32), bottom));
                    area.close();
                    let hist_col = channel.map_or(paper_col, |i| channel_colors[i]);
                    if let Ok(path) = area.build() {
                        window.paint_path(path, hist_col.opacity(0.16 * strength));
                    }
                }
            }

            // Paper response curves + circular slider-style knobs
            if let Some(display) = &display {
                for i in 0..3 {
                    let strong = channel == Some(i);
                    paint_curve_line(
                        window,
                        &display.stops,
                        &display.rgb[i],
                        bounds.origin,
                        &geometry,
                        if strong { 1.6 } else { 1.0 },
                        channel_colors[i].opacity(if strong { strength } else { 0.30 * strength }),
                    );
                }
                if channel.is_none() {
                    paint_curve_line(
                        window,
                        &display.stops,
                        &display.brightness(None),
                        bounds.origin,
                        &geometry,
                        1.6,
                        paper_col.opacity(strength),
                    );
                }

                if let Some(Handle::Curve(t)) = active_handle {
                    let tone = drag_tone.unwrap_or(t);
                    let cx_p = bounds.origin.x + px(geometry.x(t) as f32);
                    let cy_p = bounds.origin.y
                        + px(geometry.y(display.value(&display.brightness(channel), tone)) as f32);
                    let r = 3.0f32;
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(cx_p - px(r), cy_p - px(r)),
                            size: size(px(r * 2.0), px(r * 2.0)),
                        },
                        Corners::all(px(r)),
                        paper_col.opacity(0.6 * strength),
                        Edges::default(),
                        paper_col,
                        gpui::BorderStyle::Solid,
                    ));
                }

                for (handle, (hx, hy)) in geometry.handle_points(display, channel) {
                    let is_active = handle.same(active_handle);
                    let is_drag = handle.same(dragging_handle);
                    let moved = match handle {
                        Handle::Shadows => modified_shadows,
                        Handle::Middle => modified_middle,
                        Handle::Highlights => modified_highlights,
                        Handle::Curve(_) => false,
                    };
                    let r = if is_active { 5.4f32 } else { 4.5f32 };
                    let cx_p = bounds.origin.x + px(hx as f32);
                    let cy_p = bounds.origin.y + px(hy as f32);
                    let k_fill = if is_drag {
                        paper_col
                    } else if moved {
                        accent_col
                    } else {
                        panel_col
                    };
                    let k_stroke = if is_active {
                        paper_col
                    } else if moved {
                        accent_col
                    } else {
                        label_col
                    };
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(cx_p - px(r), cy_p - px(r)),
                            size: size(px(r * 2.0), px(r * 2.0)),
                        },
                        Corners::all(px(r)),
                        k_fill.opacity(strength),
                        Edges::all(px(1.5)),
                        k_stroke.opacity(strength),
                        gpui::BorderStyle::Solid,
                    ));
                }
            }
        },
    )
    .size_full();

    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let highlighter_ink = theme.highlighter_ink;
    let (axis_font, axis_size) = theme.serif_spec(12.0, 400.0, true);
    let (hint_font, hint_size) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .id(SharedString::from(format!("tone-graph-row-{row_id}")))
        .w_full()
        .px(px(GUTTER))
        .py(px(6.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        // Right-aligned inline RGB · R · G · B words
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .children(TONE_CHANNEL_BUTTONS.iter().enumerate().map(|(i, (label, _))| {
                    let chosen = i.checked_sub(1) == channel;
                    let cb = on_channel.clone();
                    let font = theme.ui_font(if chosen {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    });
                    div()
                        .flex()
                        .items_center()
                        .when(i > 0, |el| {
                            el.child(
                                div()
                                    .font(theme.ui_font(FontWeight::NORMAL))
                                    .text_size(px(11.0))
                                    .text_color(theme.track)
                                    .child(" · "),
                            )
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!("tg-ch-{row_id}-{i}")))
                                .relative()
                                .pb(px(3.0))
                                .when(enabled, |el| {
                                    el.cursor_pointer()
                                        .active(|s| s.opacity(0.62))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cb(this, row_id, RowAction::Press(i), window, cx);
                                        }))
                                })
                                .when(chosen, |el| {
                                    el.child(
                                        canvas(
                                            |_b, _w, _cx| {},
                                            move |bounds: Bounds<Pixels>, (), window, _cx| {
                                                let mark_rect = Bounds {
                                                    origin: point(
                                                        bounds.origin.x,
                                                        bounds.origin.y + bounds.size.height - px(4.0),
                                                    ),
                                                    size: size(bounds.size.width, px(4.0)),
                                                };
                                                if highlighter {
                                                    paint_highlighter(
                                                        window,
                                                        mark_rect,
                                                        plain,
                                                        4.0,
                                                        highlighter_ink,
                                                    );
                                                } else {
                                                    paint_pencil(
                                                        window,
                                                        mark_rect,
                                                        plain,
                                                        negs_marks::PENCIL_WIDTH as f32,
                                                        accent_col,
                                                    );
                                                }
                                            },
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                })
                                .child(
                                    div()
                                        .relative()
                                        .font(font)
                                        .text_size(px(11.0))
                                        .text_color(if chosen { theme.paper } else { theme.dim })
                                        .child(*label),
                                ),
                        )
                })),
        )
        // Borderless 170 px well in `theme.canvas`
        .child(
            div()
                .id(SharedString::from(format!("tone-graph-well-{row_id}")))
                .w_full()
                .h(px(GRAPH_HEIGHT))
                .rounded(px(3.0))
                .bg(theme.canvas)
                .overflow_hidden()
                .when(enabled, |el| match active_handle {
                    Some(Handle::Middle) => el.cursor_ew_resize(),
                    Some(_) => el.cursor_ns_resize(),
                    None => el.cursor_default(),
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        on_down(this, event, enabled, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    on_hover(this, event, enabled, window, cx);
                }))
                .child(graph_canvas),
        )
        // Axis labels: shadows · midtones · highlights
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .font(axis_font)
                .text_size(px(axis_size))
                .text_color(theme.dim)
                .child("shadows")
                .child("midtones")
                .child("highlights"),
        )
        // Italic status / drag readout
        .child(
            div()
                .w_full()
                .font(hint_font)
                .text_size(px(hint_size))
                .text_color(if active_handle.is_some() {
                    theme.paper
                } else {
                    theme.dim
                })
                .truncate()
                .child(status_label),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::session::Preferences;
    use negs_app::tone_graph::{Controls, Curve};
    use serde_json::json;

    fn synthetic_curve(controls: Controls) -> Curve {
        let stops: Vec<f64> = (0..=160).map(|i| -8.0 + i as f64 * 0.1).collect();
        let result = json!({
            "film": stops.iter().map(|s| [s, s, s]).collect::<Vec<_>>(),
            "paper": stops.iter().map(|s| [-0.3 * s, -0.3 * s, -0.3 * s]).collect::<Vec<_>>(),
            "rgb": stops.iter().map(|s| { let v = (0.5 + 0.05 * s).clamp(0.0, 1.0); [v, v, v] }).collect::<Vec<_>>(),
        });
        Curve::parse(&result, tone_graph::request("f", "p", &json!({})), controls).unwrap()
    }

    #[test]
    fn tone_graph_drag_edits_print_exposure_and_gamma_with_reset() {
        let mut model = Model::new(Preferences::default());
        model.defaults = json!({
            "enlarger": {"print_exposure": 1.0},
            "print_render": {
                "density_curves_morph": {
                    "active": false,
                    "gamma_factor": 1.0,
                    "gamma_factor_fast": 1.0,
                    "gamma_factor_slow": 1.0
                }
            }
        });
        model.state.params = model.defaults.clone();
        model.tone_curve = Some(synthetic_curve(Controls::default()));

        let mut state = ToneGraphState {
            bounds: Bounds {
                origin: point(px(10.0), px(20.0)),
                size: size(px(300.0), px(GRAPH_HEIGHT)),
            },
            ..Default::default()
        };
        let geom = state.geometry();

        // Hit middle handle at (stops = 0.0, brightness = 0.5) and drag right (+1 stop)
        let mid_pt = point(px(10.0 + geom.x(0.0) as f32), px(20.0 + geom.y(0.5) as f32));
        assert!(state.handle_mouse_down(&mut model, mid_pt, 1, true));
        let drag_pt = point(px(10.0 + geom.x(1.0) as f32), px(20.0 + geom.y(0.5) as f32));
        assert!(state.handle_mouse_move(&mut model, drag_pt, true));
        assert!(state.handle_mouse_up(&mut model));
        assert!((json::number(model.value(tone_graph::PRINT_EXPOSURE)) - 2.0).abs() < 0.05);

        // Double-click middle handle (now shifted) resets print_exposure back to 1.0
        let display = model
            .tone_curve
            .as_ref()
            .unwrap()
            .predicted(model.tone_controls());
        let (_, mid_pos) = geom
            .handle_points(&display, None)
            .into_iter()
            .find(|(h, _)| *h == Handle::Middle)
            .unwrap();
        let shifted_mid = point(px(10.0 + mid_pos.0 as f32), px(20.0 + mid_pos.1 as f32));
        assert!(state.handle_mouse_down(&mut model, shifted_mid, 2, true));
        assert!((json::number(model.value(tone_graph::PRINT_EXPOSURE)) - 1.0).abs() < 1e-6);

        // Dragging Shadows handle vertically enables Print Curves and modifies gamma_factor_slow
        let shad_pt = point(px(10.0 + geom.x(-2.0) as f32), px(20.0 + geom.y(0.4) as f32));
        assert!(state.handle_mouse_down(&mut model, shad_pt, 1, true));
        let shad_drag = point(px(10.0 + geom.x(-2.0) as f32), px(20.0 + geom.y(0.35) as f32));
        assert!(state.handle_mouse_move(&mut model, shad_drag, true));
        assert!(state.handle_mouse_up(&mut model));
        assert!(json::boolean(model.value(tone_graph::ACTIVE)));
        assert!(json::number(model.value(tone_graph::SLOW)) > 1.05);
    }
}
