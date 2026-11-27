//! Single-line editorial darkroom hairline slider (`InkSlider`, `L18`) driven
//! by `SliderValue`.
//!
//! Layout (`29 px` row, `22 px` gutter):
//! - Left: UI `12 pt` label (`144 px`, `label` → `paper` when modified/hovered;
//!   double-click resets).
//! - Middle: flex hairline track (`1 px` `track`, `1 × 9 px` `dim` neutral tick,
//!   `2 px` `accent` fill ONLY when modified, `9 px` circle knob: `panel` fill +
//!   `label`/`paper` `1.5 px` stroke when unmodified, solid `accent` when
//!   modified, solid `paper` when dragging, `1.2×` scale on hover/drag).
//! - Right: `50 px` Newsreader `14.5 pt` value (`400 dim`, `paper` on hover,
//!   `700 paper` + `PencilStroke`/Light Table highlighter when modified; click
//!   switches to a borderless ruled serif editor) aligned flush with `☒` and
//!   choices.
//! - Gutter: `↺` tucked neatly into the right gutter (`right: 3..21 px`) when
//!   modified and not typing.

use gpui::{
    Bounds, Context, Corners, Edges, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, SharedString, Window, canvas, div, fill, point, prelude::*, px, quad,
    size,
};
use negs_app::inspector_view::{RowAction, RowId, RowView, SliderValue};
use negs_app::theme::DISABLED_OPACITY;

use crate::marks_gpui::{paint_highlighter, paint_pencil};
use crate::theme_gpui::{GpuiTheme, scale};
use crate::widgets::controls::{GUTTER, ROW_HEIGHT};
use crate::widgets::text_input::{InlineTextState, TextInputStyle, render_text_input};

/// Label column width — wide enough for `"Negative exposure (EV)"` without clipping (`L18`).
pub const SLIDER_LABEL_WIDTH: f32 = 144.0;
/// Right-aligned serif value width (`L18`).
pub const SLIDER_VALUE_WIDTH: f32 = 50.0;
/// Horizontal inset inside the track canvas so the `9 px` / `10.8 px` knob never clips.
const TRACK_INSET: f32 = 5.0;

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
    pub on_hover: Box<dyn Fn(&mut V, RowId, bool, &mut Context<V>)>,
}

/// Renders a single-line editorial darkroom hairline slider for a `RowView` (`L18`).
#[allow(clippy::too_many_arguments)]
pub fn render_ink_slider<V: 'static>(
    row: &RowView,
    slider: SliderValue,
    dragging: bool,
    hovered: bool,
    editing_state: Option<&InlineTextState>,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<V>,
    cb: SliderCallbacks<V>,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let changed = row.changed;
    let hot = enabled && (hovered || dragging);
    let reset_help: Option<SharedString> = row.help.clone().map(SharedString::from);
    let label_text: SharedString = row.title.clone().into();
    let value_text = row.value.clone();

    let value_frac = value_to_fraction(slider.value, slider.low, slider.high);
    let default_frac = value_to_fraction(slider.default, slider.low, slider.high);

    let track_color = theme.track;
    let tick_color = theme.dim;
    let fill_color = theme.accent;
    let knob_fill = if dragging {
        theme.paper
    } else if changed {
        theme.accent
    } else {
        theme.panel
    };
    let knob_stroke = if dragging {
        theme.paper
    } else if changed {
        theme.accent
    } else if hot {
        theme.paper
    } else {
        theme.label
    };

    let entity = cx.entity().clone();
    let on_bounds = cb.on_bounds;
    let on_action = std::rc::Rc::new(cb.on_action);
    let on_action_label = on_action.clone();
    let on_action_reset = on_action.clone();
    let on_track_down = cb.on_track_down;
    let on_begin_edit = cb.on_begin_edit;
    let on_edit_key = cb.on_edit_key;
    let on_edit_focus = cb.on_edit_focus;
    let on_hover = cb.on_hover;

    let track_canvas = canvas(
        move |bounds: Bounds<Pixels>, _window, cx| {
            let inset_bounds = Bounds {
                origin: point(bounds.origin.x + px(TRACK_INSET), bounds.origin.y),
                size: size(
                    (bounds.size.width - px(TRACK_INSET * 2.0)).max(px(1.0)),
                    bounds.size.height,
                ),
            };
            entity.update(cx, |this, _cx| {
                on_bounds(this, row_id, inset_bounds);
            });
            bounds
        },
        move |bounds: Bounds<Pixels>, _prepaint: Bounds<Pixels>, window, _cx| {
            let x0 = f32::from(bounds.origin.x) + TRACK_INSET;
            let w = (f32::from(bounds.size.width) - TRACK_INSET * 2.0).max(1.0);
            let cy = (f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5).floor();

            let def_x = x0 + default_frac * w;
            let val_x = x0 + value_frac * w;
            let r = if hot { 5.4f32 } else { 4.5f32 };

            // 1 px hairline track (split around knob so disabled-row translucency never bleeds through)
            let left_w = (val_x - r - x0).max(0.0);
            if left_w > 0.5 {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(x0), px(cy)),
                        size: size(px(left_w), px(1.0)),
                    },
                    track_color,
                ));
            }
            let right_x = val_x + r;
            let right_w = (x0 + w - right_x).max(0.0);
            if right_w > 0.5 {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(right_x), px(cy)),
                        size: size(px(right_w), px(1.0)),
                    },
                    track_color,
                ));
            }

            // 1 × 9 px default-value tick mark (hidden when directly behind the knob)
            if (val_x - def_x).abs() > r - 0.5 {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(def_x.round() - 0.5), px(cy - 4.0)),
                        size: size(px(1.0), px(9.0)),
                    },
                    tick_color,
                ));
            }

            // 2 px accent fill from default to knob ONLY when changed
            if changed {
                let fill_left = def_x.min(val_x);
                let fill_w = (val_x - def_x).abs();
                if fill_w > 0.5 {
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(px(fill_left), px(cy - 0.5)),
                            size: size(px(fill_w), px(2.0)),
                        },
                        fill_color,
                    ));
                }
            }

            // 9 px circle knob (1.2× on hover/drag)
            let d = r * 2.0;
            let knob_rect = Bounds {
                origin: point(px(val_x - r), px(cy + 0.5 - r)),
                size: size(px(d), px(d)),
            };
            window.paint_quad(quad(
                knob_rect,
                Corners::all(px(r)),
                knob_fill,
                Edges::all(px(1.5)),
                knob_stroke,
                gpui::BorderStyle::Solid,
            ));
        },
    )
    .w_full()
    .h(px(ROW_HEIGHT));

    let label_color = if changed || hot { theme.paper } else { theme.label };
    let val_color = if changed || hot { theme.paper } else { theme.dim };
    let (val_font, val_size) = theme.serif_spec(scale::VALUE, if changed { 700.0 } else { 400.0 }, false);
    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let accent = theme.accent;
    let highlighter_ink = theme.highlighter_ink;
    let dim_hover = theme.dim;
    let paper_hover = theme.paper;

    let value_cell = if let Some(input_state) = editing_state {
        render_text_input(
            SharedString::from(format!("slider-edit-{row_id}")),
            input_state,
            TextInputStyle {
                width: SLIDER_VALUE_WIDTH,
                height: 22.0,
                font_size: scale::VALUE,
                placeholder: SharedString::default(),
                align_right: true,
                error: false,
                disabled: !enabled,
                ruled: true,
                serif: true,
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
            .w(px(SLIDER_VALUE_WIDTH))
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .child(
                div()
                    .id(SharedString::from(format!("slider-val-{row_id}")))
                    .relative()
                    .pb(px(2.0))
                    .when(enabled, |el| {
                        el.cursor_text()
                            .when(!changed, |e| e.hover(move |s| s.border_b_1().border_color(dim_hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _event, window, cx| {
                                    cx.stop_propagation();
                                    on_begin_edit(this, row_id, value_for_click.clone(), window, cx);
                                }),
                            )
                    })
                    .when(changed, |el| {
                        el.child(
                            canvas(
                                |_b, _w, _cx| {},
                                move |bounds: Bounds<Pixels>, (), window, _cx| {
                                    let mark_rect = Bounds {
                                        origin: point(bounds.origin.x, bounds.origin.y + bounds.size.height - px(4.0)),
                                        size: size(bounds.size.width, px(5.0)),
                                    };
                                    if highlighter {
                                        paint_highlighter(window, mark_rect, plain, 5.0, highlighter_ink);
                                    } else {
                                        paint_pencil(window, mark_rect, plain, negs_marks::PENCIL_WIDTH as f32, accent);
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
                            .font(val_font)
                            .text_size(px(val_size))
                            .line_height(px(18.0))
                            .text_color(val_color)
                            .whitespace_nowrap()
                            .child(value_text),
                    ),
            )
            .into_any_element()
    };

    div()
        .id(SharedString::from(format!("slider-row-{row_id}")))
        .relative()
        .flex()
        .items_center()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .on_hover(cx.listener(move |this, &inside, _window, cx| {
            on_hover(this, row_id, inside, cx);
        }))
        // Label
        .child(
            div()
                .id(SharedString::from(format!("slider-label-{row_id}")))
                .w(px(SLIDER_LABEL_WIDTH))
                .flex_shrink_0()
                .pr(px(4.0))
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
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
        // Hairline track
        .child(
            div()
                .id(SharedString::from(format!("slider-track-{row_id}")))
                .flex_1()
                .min_w(px(48.0))
                .h(px(ROW_HEIGHT))
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
        // Right-aligned serif value (flush with right gutter)
        .child(value_cell)
        // Reset glyph tucked into the 22 px right gutter when modified & not typing
        .when(changed && enabled && editing_state.is_none(), |el| {
            el.child(
                div()
                    .id(SharedString::from(format!("slider-reset-{row_id}")))
                    .when_some(reset_help.clone(), |d, help| {
                        d.tooltip(crate::tooltip::tip(help, theme))
                    })
                    .absolute()
                    .right(px(3.0))
                    .top(px(5.0))
                    .w(px(18.0))
                    .h(px(18.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font(theme.ui_font(FontWeight::NORMAL))
                    .text_size(px(12.0))
                    .text_color(theme.dim)
                    .cursor_pointer()
                    .hover(move |s| s.text_color(paper_hover))
                    .active(|s| s.opacity(0.62))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
        })
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
