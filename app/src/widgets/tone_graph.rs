//! Interactive Print Curves tone graph (`ToneGraphCanvas`) driven by
//! `negs_app::tone_graph`, `model.tone_curve`, and `model.tone_controls()`.
//!
//! Matches `windows/src/ui/tone_graph.rs`:
//! - Middle handle drags horizontally to adjust Print exposure.
//! - Side handles (Shadows / Highlights) and the curve itself drag vertically
//!   to adjust `gamma_factor_slow`, `gamma_factor_fast`, and `gamma_factor`
//!   (auto-enabling Print Curves in the same undo step).
//! - Double-clicking a handle resets its parameter.
//! - Live drag renders `Curve::predicted(...)` at 60 fps while the engine computes.

use gpui::{
    Bounds, Context, Corners, Edges, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, Point, SharedString, Window, canvas, div, fill, point,
    prelude::*, px, quad, size,
};
use negs_app::inspector_view::schema::{TONE_CHANNEL_BUTTONS, tone_channel};
use negs_app::inspector_view::{RowAction, RowId, RowView};
use negs_app::json;
use negs_app::model::Model;
use negs_app::tone_graph::{self, Drag, Geometry, Handle};
use serde_json::Value;

use crate::theme_gpui::{GpuiTheme, hex, hex_alpha};

const UNDO_ACTION: &str = "Change Tone Curve";
const GRAPH_HEIGHT: f32 = 156.0;
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
            let changed = self.hovered.take().is_some();
            return changed;
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

/// Renders the Print Curves tone graph row (status label, RGB/R/G/B channel
/// pills, and interactive GPUI canvas).
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
    let drag_tone = state.drag.as_ref().map(|d| d.tone);

    let display = model
        .tone_curve
        .as_ref()
        .map(|c| c.predicted(model.tone_controls()));
    let current = model.tone_curve_current();
    let film_histogram = model.film_histogram.clone();

    let entity = cx.entity().clone();
    let well_bg = hex(0x1a1a1a);
    let line_white = hex(0xffffff);
    let channel_colors = [hex(0xff5959), hex(0x59e659), hex(0x668cff)];
    let middle_color = hex(0xffcc00);
    let outline_black = hex_alpha(0x000000, 0.8);

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

            // Grid: vertical every 2 stops (center 0 dashed), horizontal at quarters.
            for stop in [-4.0, -2.0, 0.0, 2.0, 4.0] {
                let x = bounds.origin.x + px((geometry.x(stop) as f32).floor());
                if stop == 0.0 {
                    let dash = 3.0f32;
                    let mut y = 0.0f32;
                    while y < gh {
                        let seg_h = dash.min(gh - y);
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(x, bounds.origin.y + px(y)),
                                size: size(px(1.0), px(seg_h)),
                            },
                            line_white.opacity(0.18),
                        ));
                        y += 2.0 * dash;
                    }
                } else {
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(x, bounds.origin.y),
                            size: size(px(1.0), bounds.size.height),
                        },
                        line_white.opacity(0.07),
                    ));
                }
            }
            for level in [0.25, 0.5, 0.75] {
                let y = bounds.origin.y + px((geometry.y(level) as f32).floor());
                window.paint_quad(fill(
                    Bounds {
                        origin: point(bounds.origin.x, y),
                        size: size(bounds.size.width, px(1.0)),
                    },
                    line_white.opacity(0.07),
                ));
            }

            let strength: f32 = if !enabled {
                0.35
            } else if current {
                1.0
            } else {
                0.55
            };

            // Film exposure histogram backdrop.
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
                    let hist_col = channel.map_or(line_white, |i| channel_colors[i]);
                    if let Ok(path) = area.build() {
                        window.paint_path(path, hist_col.opacity(0.16 * strength));
                    }
                }
            }

            // Paper response curves + handles.
            if let Some(display) = &display {
                for i in 0..3 {
                    let strong = channel == Some(i);
                    paint_curve_line(
                        window,
                        &display.stops,
                        &display.rgb[i],
                        bounds.origin,
                        &geometry,
                        if strong { 2.0 } else { 1.0 },
                        channel_colors[i].opacity(if strong { strength } else { 0.35 * strength }),
                    );
                }
                if channel.is_none() {
                    paint_curve_line(
                        window,
                        &display.stops,
                        &display.brightness(None),
                        bounds.origin,
                        &geometry,
                        2.0,
                        line_white.opacity(strength),
                    );
                }

                // Active curve point indicator when dragging/hovering the curve body.
                if let Some(Handle::Curve(t)) = active_handle {
                    let tone = drag_tone.unwrap_or(t);
                    let cx_p = bounds.origin.x + px(geometry.x(t) as f32);
                    let cy_p = bounds.origin.y
                        + px(geometry.y(display.value(&display.brightness(channel), tone)) as f32);
                    let r = 3.5f32;
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(cx_p - px(r), cy_p - px(r)),
                            size: size(px(r * 2.0), px(r * 2.0)),
                        },
                        Corners::all(px(r)),
                        line_white.opacity(0.75 * strength),
                        Edges::default(),
                        line_white,
                        gpui::BorderStyle::Solid,
                    ));
                }

                // Shadows, Middle (gold square), and Highlights handles.
                for (handle, (hx, hy)) in geometry.handle_points(display, channel) {
                    let r = if handle.same(active_handle) {
                        5.5f32
                    } else {
                        4.25f32
                    };
                    let cx_p = bounds.origin.x + px(hx as f32);
                    let cy_p = bounds.origin.y + px(hy as f32);
                    let corner = if handle == Handle::Middle {
                        px(2.0)
                    } else {
                        px(r)
                    };
                    let fill_col = if handle == Handle::Middle {
                        middle_color.opacity(strength)
                    } else {
                        line_white.opacity(strength)
                    };
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(cx_p - px(r), cy_p - px(r)),
                            size: size(px(r * 2.0), px(r * 2.0)),
                        },
                        Corners::all(corner),
                        fill_col,
                        Edges::all(px(1.25)),
                        outline_black.opacity(strength),
                        gpui::BorderStyle::Solid,
                    ));
                }
            }
        },
    )
    .size_full();

    let hover_bg = theme.subtle_hover;

    div()
        .id(SharedString::from(format!("tone-graph-row-{row_id}")))
        .w_full()
        .px(px(16.0))
        .py(px(6.0))
        .flex()
        .flex_col()
        .gap(px(5.0))
        // Header: dynamic status label + RGB/R/G/B pills
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.dim)
                        .truncate()
                        .child(status_label),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .children(TONE_CHANNEL_BUTTONS.iter().enumerate().map(|(i, (label, _))| {
                            let active = i.checked_sub(1) == channel;
                            let cb = on_channel.clone();
                            div()
                                .id(SharedString::from(format!("tg-ch-{row_id}-{i}")))
                                .px(px(5.0))
                                .py(px(1.5))
                                .rounded(px(3.0))
                                .bg(if active {
                                    theme.accent.opacity(0.22)
                                } else {
                                    theme.canvas
                                })
                                .border_1()
                                .border_color(if active {
                                    theme.accent
                                } else {
                                    theme.line
                                })
                                .text_size(px(10.0))
                                .text_color(if active { theme.paper } else { theme.dim })
                                .when(enabled, |el| {
                                    el.cursor_pointer()
                                        .hover(move |s| s.bg(hover_bg))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cb(this, row_id, RowAction::Press(i), window, cx);
                                        }))
                                })
                                .child(*label)
                        })),
                ),
        )
        // Interactive graph well
        .child(
            div()
                .id(SharedString::from(format!("tone-graph-well-{row_id}")))
                .w_full()
                .h(px(GRAPH_HEIGHT))
                .rounded(px(6.0))
                .bg(well_bg)
                .border_1()
                .border_color(theme.line)
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
                size: size(px(300.0), px(156.0)),
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
