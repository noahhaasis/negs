//! Editorial darkroom hairline slider (`InkSlider`) driven by `SliderValue`.
//!
//! Features:
//! - Title row with modified accent dot, hover/visible `↺` reset button, and
//!   clickable numeric value readout that switches to an inline `TextInput`.
//! - Hairline track with default-value tick mark, filled ink/accent bar, and
//!   crisp thumb indicator.
//! - Smooth 60+ fps pointer drag (`RowAction::Track(_, Track::Drag)` -> `Track::End`)
//!   with single-undo-step coalescing via `Model::begin_interaction` / `end_interaction`.
//! - Double-click on label or track resets (`RowAction::Reset`).

use gpui::{
    Bounds, Context, Corners, Edges, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, SharedString, Window, canvas, div, fill, point, prelude::*, px, quad, size,
};
use negs_app::inspector_view::{RowAction, RowId, RowView, SliderValue};

use crate::theme_gpui::GpuiTheme;
use crate::widgets::text_input::{InlineTextState, TextInputStyle, render_text_input};

/// Active slider pointer drag tracked at window level so fast drags never stall
/// when the pointer moves outside the hairline track.
#[derive(Clone, Debug)]
pub struct SliderDrag {
    pub row_id: RowId,
    pub low: f64,
    pub high: f64,
    pub step: f64,
    pub track_left: f32,
    pub track_width: f32,
    pub last_position: Option<i32>,
}

impl SliderDrag {
    /// Computes the snapped trackbar integer step position for a window X coordinate.
    pub fn position_for_x(&self, x: f32) -> i32 {
        x_to_position(x, self.track_left, self.track_width, self.low, self.high, self.step)
    }
}

/// Maps `value` inside `[low, high]` to a clamped `[0.0, 1.0]` fraction.
pub fn value_to_fraction(value: f64, low: f64, high: f64) -> f32 {
    let span = high - low;
    if !span.is_finite() || span.abs() < 1e-12 {
        return 0.0;
    }
    ((value - low) / span).clamp(0.0, 1.0) as f32
}

/// Maps a `[0.0, 1.0]` fraction to the integer step index from `low` expected by
/// `RowAction::Track(position, _)`.
pub fn fraction_to_position(fraction: f32, low: f64, high: f64, step: f64) -> i32 {
    if !step.is_finite() || step <= 0.0 || high <= low {
        return 0;
    }
    let f = (fraction as f64).clamp(0.0, 1.0);
    let max_pos = ((high - low) / step).round().max(0.0);
    (f * max_pos).round().clamp(0.0, max_pos) as i32
}

/// Converts a step `position` back to a snapped value within `[low, high]`.
#[cfg(test)]
pub fn position_to_value(position: i32, low: f64, high: f64, step: f64) -> f64 {
    negs_app::state::snap(low + position as f64 * step, low, high, step)
}

/// Converts a pointer X coordinate over `[track_left, track_left + track_width]`
/// to the integer step position.
pub fn x_to_position(x: f32, track_left: f32, track_width: f32, low: f64, high: f64, step: f64) -> i32 {
    let w = track_width.max(1.0);
    let frac = ((x - track_left) / w).clamp(0.0, 1.0);
    fraction_to_position(frac, low, high, step)
}

/// Callbacks connecting an `InkSlider` row to its owning GPUI view.
pub struct SliderCallbacks<V: 'static> {
    pub on_bounds: Box<dyn Fn(&mut V, RowId, Bounds<Pixels>)>,
    pub on_track_down: Box<dyn Fn(&mut V, RowId, SliderValue, &MouseDownEvent, &mut Window, &mut Context<V>)>,
    pub on_action: Box<dyn Fn(&mut V, RowId, RowAction, &mut Window, &mut Context<V>)>,
    pub on_begin_edit: Box<dyn Fn(&mut V, RowId, String, &mut Window, &mut Context<V>)>,
    pub on_edit_key: Box<dyn Fn(&mut V, RowId, &KeyDownEvent, &mut Window, &mut Context<V>)>,
    pub on_edit_focus: Box<dyn Fn(&mut V, &mut Window, &mut Context<V>)>,
}

/// Renders an editorial darkroom hairline slider for a `RowView`.
#[allow(clippy::too_many_arguments)]
pub fn render_ink_slider<V: 'static>(
    row: &RowView,
    slider: SliderValue,
    dragging: bool,
    editing_state: Option<&InlineTextState>,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<V>,
    cb: SliderCallbacks<V>,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let changed = row.changed;
    let label_text: SharedString = row.title.clone().into();
    let value_text = row.value.clone();

    let value_frac = value_to_fraction(slider.value, slider.low, slider.high);
    let default_frac = value_to_fraction(slider.default, slider.low, slider.high);

    let track_color = theme.track;
    let fill_color = if !enabled {
        theme.dim.opacity(0.4)
    } else if dragging || changed {
        theme.accent
    } else {
        theme.label.opacity(0.75)
    };
    let thumb_fill = if enabled { theme.paper } else { theme.dim };
    let thumb_border = if dragging || changed {
        theme.accent
    } else {
        theme.panel
    };
    let tick_color = theme.dim.opacity(0.65);

    let entity = cx.entity().clone();
    let on_bounds = cb.on_bounds;
    let on_action = std::rc::Rc::new(cb.on_action);
    let on_action_label = on_action.clone();
    let on_action_reset = on_action.clone();
    let on_track_down = cb.on_track_down;
    let on_begin_edit = cb.on_begin_edit;
    let on_edit_key = cb.on_edit_key;
    let on_edit_focus = cb.on_edit_focus;

    let track_canvas = canvas(
        move |bounds: Bounds<Pixels>, _window, cx| {
            entity.update(cx, |this, _cx| {
                on_bounds(this, row_id, bounds);
            });
            bounds
        },
        move |bounds: Bounds<Pixels>, _prepaint: Bounds<Pixels>, window, _cx| {
            let x0 = f32::from(bounds.origin.x);
            let w = f32::from(bounds.size.width).max(1.0);
            let cy = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;

            // Hairline track
            let track_h = 2.0f32;
            let track_rect = Bounds {
                origin: point(px(x0), px(cy - track_h * 0.5)),
                size: size(px(w), px(track_h)),
            };
            window.paint_quad(quad(
                track_rect,
                Corners::all(px(1.0)),
                track_color,
                Edges::default(),
                track_color,
                gpui::BorderStyle::Solid,
            ));

            // Default-value tick mark
            let def_x = x0 + default_frac * w;
            let tick_rect = Bounds {
                origin: point(px(def_x - 0.75), px(cy - 4.0)),
                size: size(px(1.5), px(8.0)),
            };
            window.paint_quad(fill(tick_rect, tick_color));

            // Filled ink/accent bar from default (if interior) or low to current thumb
            let val_x = x0 + value_frac * w;
            let anchor_x = if default_frac > 0.01 && default_frac < 0.99 {
                def_x
            } else {
                x0
            };
            let fill_left = anchor_x.min(val_x);
            let fill_w = (val_x - anchor_x).abs();
            if fill_w > 0.5 {
                let fill_rect = Bounds {
                    origin: point(px(fill_left), px(cy - track_h * 0.5)),
                    size: size(px(fill_w), px(track_h)),
                };
                window.paint_quad(fill(fill_rect, fill_color));
            }

            // Crisp thumb indicator
            let thumb_r = if dragging { 5.5f32 } else { 4.5f32 };
            let thumb_d = thumb_r * 2.0;
            let thumb_rect = Bounds {
                origin: point(px(val_x - thumb_r), px(cy - thumb_r)),
                size: size(px(thumb_d), px(thumb_d)),
            };
            window.paint_quad(quad(
                thumb_rect,
                Corners::all(px(thumb_r)),
                thumb_fill,
                Edges::all(px(1.5)),
                thumb_border,
                gpui::BorderStyle::Solid,
            ));
        },
    )
    .w_full()
    .h(px(18.0));

    let label_color = if !enabled {
        theme.dim.opacity(0.55)
    } else if changed {
        theme.paper
    } else {
        theme.label
    };
    let val_color = if !enabled {
        theme.dim.opacity(0.55)
    } else if changed {
        theme.paper
    } else {
        theme.dim
    };
    let hover_bg = theme.subtle_hover;

    div()
        .id(SharedString::from(format!("slider-row-{row_id}")))
        .flex()
        .flex_col()
        .w_full()
        .px(px(16.0))
        .py(px(4.0))
        .gap(px(2.0))
        .when(!enabled, |el| el.opacity(0.55))
        // Title & Value row
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .min_w_0()
                        .child(
                            div()
                                .id(SharedString::from(format!("slider-label-{row_id}")))
                                .text_size(px(12.0))
                                .text_color(label_color)
                                .truncate()
                                .when(enabled, |el| el.cursor_pointer())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        if enabled && event.click_count >= 2 {
                                            on_action_label(
                                                this,
                                                row_id,
                                                RowAction::Reset { double_click: false },
                                                window,
                                                cx,
                                            );
                                        }
                                    }),
                                )
                                .child(label_text),
                        )
                        .when(changed, |el| {
                            el.child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(theme.accent),
                            )
                        })
                        .when(changed && enabled, |el| {
                            el.child(
                                div()
                                    .id(SharedString::from(format!("slider-reset-{row_id}")))
                                    .px(px(3.0))
                                    .rounded(px(3.0))
                                    .text_size(px(11.0))
                                    .text_color(theme.dim)
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(hover_bg))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        on_action_reset(
                                            this,
                                            row_id,
                                            RowAction::Reset { double_click: false },
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child("↺"),
                            )
                        }),
                )
                .child(if let Some(input_state) = editing_state {
                    render_text_input(
                        SharedString::from(format!("slider-edit-{row_id}")),
                        input_state,
                        TextInputStyle {
                            width: 64.0,
                            height: 20.0,
                            font_size: 11.5,
                            placeholder: SharedString::default(),
                            align_right: true,
                            error: false,
                            disabled: !enabled,
                        },
                        theme,
                        window,
                        cx,
                        move |this, event, window, cx| on_edit_key(this, row_id, event, window, cx),
                        move |this, window, cx| on_edit_focus(this, window, cx),
                    )
                    .into_any_element()
                } else {
                    let value_for_click = value_text.clone();
                    div()
                        .id(SharedString::from(format!("slider-val-{row_id}")))
                        .px(px(5.0))
                        .py(px(1.0))
                        .min_w(px(44.0))
                        .flex()
                        .justify_end()
                        .rounded(px(3.0))
                        .text_size(px(11.5))
                        .text_color(val_color)
                        .when(enabled, |el| {
                            el.cursor_text()
                                .hover(move |s| s.bg(hover_bg))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _event, window, cx| {
                                        cx.stop_propagation();
                                        on_begin_edit(this, row_id, value_for_click.clone(), window, cx);
                                    }),
                                )
                        })
                        .child(value_text)
                        .into_any_element()
                }),
        )
        // Track row
        .child(
            div()
                .id(SharedString::from(format!("slider-track-{row_id}")))
                .w_full()
                .h(px(18.0))
                .when(enabled, |el| el.cursor_pointer())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if !enabled {
                            return;
                        }
                        cx.stop_propagation();
                        on_track_down(this, row_id, slider, event, window, cx);
                    }),
                )
                .child(track_canvas),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_fraction_and_position_mapping_across_ranges() {
        // Symmetric EV range [-3.0, 3.0], step 0.05
        assert!((value_to_fraction(-3.0, -3.0, 3.0) - 0.0).abs() < 1e-6);
        assert!((value_to_fraction(0.0, -3.0, 3.0) - 0.5).abs() < 1e-6);
        assert!((value_to_fraction(3.0, -3.0, 3.0) - 1.0).abs() < 1e-6);
        assert_eq!(value_to_fraction(-10.0, -3.0, 3.0), 0.0);
        assert_eq!(value_to_fraction(10.0, -3.0, 3.0), 1.0);

        assert_eq!(fraction_to_position(0.0, -3.0, 3.0, 0.05), 0);
        assert_eq!(fraction_to_position(0.5, -3.0, 3.0, 0.05), 60);
        assert_eq!(fraction_to_position(1.0, -3.0, 3.0, 0.05), 120);
        assert!((position_to_value(60, -3.0, 3.0, 0.05) - 0.0).abs() < 1e-9);
        assert!((position_to_value(75, -3.0, 3.0, 0.05) - 0.75).abs() < 1e-9);

        // Positive range [0.1, 5.0], step 0.01 (Print exposure)
        let pos = x_to_position(150.0, 100.0, 200.0, 0.1, 5.0, 0.01);
        let val = position_to_value(pos, 0.1, 5.0, 0.01);
        assert!((val - 1.33).abs() <= 0.01);

        // Border line width [0.05, 2.0], step 0.05
        assert_eq!(fraction_to_position(0.0, 0.05, 2.0, 0.05), 0);
        let max_pos = fraction_to_position(1.0, 0.05, 2.0, 0.05);
        assert!((position_to_value(max_pos, 0.05, 2.0, 0.05) - 2.0).abs() < 1e-9);

        // Degenerate range stays safe
        assert_eq!(value_to_fraction(1.0, 1.0, 1.0), 0.0);
        assert_eq!(fraction_to_position(0.5, 1.0, 1.0, 0.1), 0);
    }
}
