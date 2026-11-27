//! Editorial inspector controls (`L19`, `L20`, `L23`, `L24`):
//! SectionHeader, GroupHeader, TickBox, Toggle, Choice, MoreRow, NoteRow,
//! ActionButtonsRow, BalancePrint, FlatField, BorderStyles visual swatches,
//! BorderColor circle well, LongestSide ruled serif input, Photograph readout,
//! plus shared WordButton / OutlinedButton / EditDot primitives.

use gpui::{
    AnyElement, Bounds, Context, Corners, Edges, FontWeight, Hsla, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, PathBuilder, Pixels, SharedString, TextRun, Window, canvas, div,
    fill, point, prelude::*, px, quad, size,
};
use negs_app::border::BorderColor;
use negs_app::inspector_view::RowView;
use negs_app::theme::DISABLED_OPACITY;

use crate::marks_gpui::{paint_dot, paint_highlighter, paint_pencil, paint_ring};
use crate::theme_gpui::{GpuiTheme, hex, scale};
use crate::widgets::text_input::{InlineTextState, TextInputStyle, render_text_input};

/// Shared horizontal gutter for all inspector rows (`Theme.Size.gutter`, `L13`).
pub const GUTTER: f32 = 22.0;
/// Standard single-line control row height (`Theme.Size.row`, `L18`, `L20`).
pub const ROW_HEIGHT: f32 = 29.0;
/// Foldout module header height (`Theme.Size.moduleRow`, `L19`).
pub const MODULE_HEIGHT: f32 = 37.0;
/// Section header band height (`Theme.Size.sectionHead`, `L19`).
pub const SECTION_HEAD_HEIGHT: f32 = 34.0;

/// Editorial darkroom border color presets offered in the inline color picker popover.
pub const BORDER_COLOR_PRESETS: [(&str, BorderColor); 4] = [
    ("White", BorderColor::WHITE),
    ("Black", BorderColor::BLACK),
    ("Warm", BorderColor([0xf5, 0xef, 0xe6])),
    ("Cream", BorderColor([0xed, 0xe4, 0xd3])),
];

/// Converts a `BorderColor` into a GPUI `Hsla`.
pub fn border_color_hsla(c: BorderColor) -> Hsla {
    let packed = ((c.0[0] as u32) << 16) | ((c.0[1] as u32) << 8) | (c.0[2] as u32);
    hex(packed)
}

/// 5 px grease-pencil edit dot (`EditDot`).
pub fn render_edit_dot(theme: &GpuiTheme) -> impl IntoElement {
    div()
        .w(px(5.0))
        .h(px(5.0))
        .flex_shrink_0()
        .rounded_full()
        .bg(theme.accent)
}

/// Editorial link word (`WordButton`, `L14`, `L17`, `L24`): italic Newsreader
/// with a `1 px` `track` underline that darkens to `dim` and brightens text to
/// `paper` on hover.
#[allow(clippy::too_many_arguments)]
pub fn render_word_button<V: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    size_pt: f32,
    enabled: bool,
    tooltip: Option<SharedString>,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> AnyElement {
    let (font, sz) = theme.serif_spec(size_pt, 400.0, true);
    let hover_paper = theme.paper;
    let hover_dim = theme.dim;

    div()
        .id(id.into())
        .when_some(tooltip, |d, tip| d.tooltip(crate::tooltip::tip(tip, theme)))
        .pb(px(1.0))
        .border_b_1()
        .border_color(theme.track)
        .font(font)
        .text_size(px(sz))
        .line_height(px(sz + 3.0))
        .text_color(theme.label)
        .whitespace_nowrap()
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.text_color(hover_paper).border_color(hover_dim))
                .active(|s| s.opacity(0.62))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_click(this, window, cx);
                }))
        })
        .child(label.into())
        .into_any_element()
}

/// Capsule outlined button (`OutlinedButtonStyle`, `L24`): `23 px` tall, full
/// pill, `1 px` `track` border (`dim` on hover), `paper @ 0.04/0.09` hover/press
/// fill, Newsreader `13.5 pt` `500` in `paper`.
#[allow(clippy::too_many_arguments)]
pub fn render_outlined_button<V: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    tooltip: Option<SharedString>,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> AnyElement {
    let (font, sz) = theme.serif_spec(scale::OUTLINED_BUTTON, 500.0, false);
    let hover_bg = theme.paper.opacity(0.04);
    let active_bg = theme.paper.opacity(0.09);
    let hover_border = theme.dim;

    div()
        .id(id.into())
        .when_some(tooltip, |d, tip| d.tooltip(crate::tooltip::tip(tip, theme)))
        .h(px(23.0))
        .px(px(12.0))
        .pt(px(1.5))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme.track)
        .font(font)
        .text_size(px(sz))
        .text_color(theme.paper)
        .whitespace_nowrap()
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.bg(hover_bg).border_color(hover_border))
                .active(move |s| s.bg(active_bg).border_color(hover_border))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_click(this, window, cx);
                }))
        })
        .child(label.into())
        .into_any_element()
}

/// `13 × 13` square `TickBox` (`r = 2`, `1.2 px` `track`/`dim`/`label` border,
/// diagonal `×` in `1.4 px` `paper`, `paper @ 0.12` press fill, `L19`, `L20`).
pub fn render_tick_box<V: 'static>(
    id: SharedString,
    checked: bool,
    enabled: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let border_col = if checked { theme.dim } else { theme.track };
    let hover_border = theme.label;
    let press_bg = theme.paper.opacity(0.12);
    let cross_col = theme.paper;
    let next = !checked;

    div()
        .id(id)
        .w(px(13.0))
        .h(px(13.0))
        .flex_shrink_0()
        .rounded(px(2.0))
        .border(px(1.2))
        .border_color(border_col)
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.border_color(hover_border))
                .active(move |s| s.bg(press_bg).border_color(hover_border))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        cx.stop_propagation();
                        on_toggle(this, next, window, cx);
                    }),
                )
        })
        .when(checked, |el| {
            el.child(
                canvas(
                    |_b, _w, _cx| {},
                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                        let x0 = f32::from(bounds.origin.x);
                        let y0 = f32::from(bounds.origin.y);
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);
                        let pad = 1.4f32;
                        let p1 = point(px(x0 + pad), px(y0 + pad));
                        let p2 = point(px(x0 + w - pad), px(y0 + h - pad));
                        let p3 = point(px(x0 + w - pad), px(y0 + pad));
                        let p4 = point(px(x0 + pad), px(y0 + h - pad));
                        let mut b = PathBuilder::stroke(px(1.4));
                        b.move_to(p1);
                        b.line_to(p2);
                        b.move_to(p3);
                        b.line_to(p4);
                        if let Ok(path) = b.build() {
                            window.paint_path(path, cross_col);
                        }
                        for p in [p1, p2, p3, p4] {
                            paint_dot(window, p, 0.7, cross_col);
                        }
                    },
                )
                .size_full(),
            )
        })
}

/// Paints tracked uppercase serif text (e.g. `CAMERA`, `TONE`, `IMAGE SIZE`)
/// with exact `tracking` px between characters and optional `PencilStroke` strike (`L19`).
fn tracked_section_title(
    title: String,
    skipped: bool,
    theme: &GpuiTheme,
    window: &Window,
) -> AnyElement {
    let upper = title.to_uppercase();
    let (font, sz) = theme.serif_spec(scale::SECTION_HEAD, 700.0, false);
    let tracking = sz * scale::SECTION_TRACKING_EM;
    let color = if skipped { theme.dim } else { theme.label };
    let plain = theme.tokens.plain;
    let strike_color = theme.accent;

    let chars: Vec<String> = upper.chars().map(|c| c.to_string()).collect();
    let mut widths: Vec<f32> = Vec::with_capacity(chars.len());
    let mut total_w = 0.0f32;
    for (i, ch) in chars.iter().enumerate() {
        let run = TextRun {
            len: ch.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(SharedString::from(ch.clone()), px(sz), &[run], None);
        let cw = f32::from(shaped.width);
        widths.push(cw);
        total_w += cw;
        if i + 1 < chars.len() {
            total_w += tracking;
        }
    }
    let height = (sz + 5.0).ceil();

    canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, cx| {
            let mut x = f32::from(bounds.origin.x);
            let y = bounds.origin.y + px(1.0);
            for (i, ch) in chars.iter().enumerate() {
                let run = TextRun {
                    len: ch.len(),
                    font: font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(ch.clone()),
                    px(sz),
                    &[run],
                    None,
                );
                let _ = shaped.paint(point(px(x), y), px(height), window, cx);
                x += widths[i] + tracking;
            }
            if skipped {
                let strike_rect = Bounds {
                    origin: point(bounds.origin.x, bounds.origin.y + bounds.size.height * 0.5 - px(2.0)),
                    size: size(bounds.size.width, px(5.0)),
                };
                paint_pencil(window, strike_rect, plain, 1.8, strike_color);
            }
        },
    )
    .w(px(total_w.ceil().max(1.0)))
    .h(px(height))
    .into_any_element()
}

/// Renders a section header (`Kind::Section`, `L19`): full-width bottom rule
/// closing the prior section (when `!is_first`), then `34 px` tracked
/// uppercase Newsreader `11 pt` `700` header (`+ "— skipped"` and `5 px` edit
/// dot). Untitled sections emit an `8 px` top gap after closing the prior rule.
pub fn render_section_header(
    row: &RowView,
    is_first: bool,
    theme: &GpuiTheme,
    window: &Window,
) -> AnyElement {
    let prior_rule = (!is_first).then(|| {
        div()
            .w_full()
            .pt(px(9.0))
            .child(div().w_full().h(px(1.0)).bg(theme.line))
    });

    if row.title.is_empty() {
        return div()
            .w_full()
            .flex()
            .flex_col()
            .when_some(prior_rule, |el, r| el.child(r))
            .child(div().h(px(8.0)))
            .into_any_element();
    }

    let (skip_font, skip_size) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .w_full()
        .flex()
        .flex_col()
        .when_some(prior_rule, |el, r| el.child(r))
        .child(
            div()
                .w_full()
                .h(px(SECTION_HEAD_HEIGHT))
                .pt(px(8.0))
                .px(px(GUTTER))
                .flex()
                .items_center()
                .child(tracked_section_title(
                    row.title.to_string(),
                    row.skipped,
                    theme,
                    window,
                ))
                .when(row.skipped, |el| {
                    el.child(
                        div()
                            .pl(px(10.0))
                            .font(skip_font)
                            .text_size(px(skip_size))
                            .text_color(theme.dim)
                            .child("— skipped"),
                    )
                })
                .when(row.changed, |el| {
                    el.child(div().pl(px(8.0)).pt(px(1.0)).child(render_edit_dot(theme)))
                }),
        )
        .into_any_element()
}

/// Renders a foldout module header (`Kind::Group`, `L19`):
/// optional full-width `1 px` `line` rule above, `37 px` height, `22 px`
/// gutter, crisp vector disclosure chevron, Newsreader `16 pt` (`600 paper` on
/// / `400 dim` off or skipped), `PencilStroke` strike when skipped, right
/// `5 px` yellow dot + Newsreader `14 pt` summary + `13 × 13` `TickBox`.
#[allow(clippy::too_many_arguments)]
pub fn render_group_header<V: 'static>(
    row: &RowView,
    has_switch: bool,
    ruled: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle_open: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    on_toggle_switch: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let skipped = row.skipped;
    let off = row.off;
    let open = row.open && !skipped;
    let muted = off || skipped;

    let (title_font, title_size) =
        theme.serif_spec(scale::MODULE_TITLE, if muted { 400.0 } else { 600.0 }, false);
    let title_color = if muted || !enabled {
        theme.dim
    } else {
        theme.paper
    };
    let (sum_font, sum_size) = theme.serif_spec(scale::WORD_BUTTON, 400.0, off);
    let summary_text = if off {
        "Off".to_string()
    } else {
        row.summary.clone()
    };
    let show_summary = !summary_text.is_empty() && (!row.open || skipped);

    let chevron_col = theme.dim;
    let plain = theme.tokens.plain;
    let accent = theme.accent;

    let chevron_canvas = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let cx_p = f32::from(bounds.origin.x) + 4.5;
            let cy_p = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;
            let mut b = PathBuilder::stroke(px(1.3));
            if open {
                // Downward chevron `⌄`
                b.move_to(point(px(cx_p - 3.5), px(cy_p - 1.5)));
                b.line_to(point(px(cx_p), px(cy_p + 2.5)));
                b.line_to(point(px(cx_p + 3.5), px(cy_p - 1.5)));
            } else {
                // Rightward chevron `›`
                b.move_to(point(px(cx_p - 1.5), px(cy_p - 3.5)));
                b.line_to(point(px(cx_p + 2.5), px(cy_p)));
                b.line_to(point(px(cx_p - 1.5), px(cy_p + 3.5)));
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, chevron_col);
            }
        },
    )
    .w(px(19.0))
    .h(px(MODULE_HEIGHT))
    .flex_shrink_0();

    div()
        .w_full()
        .flex()
        .flex_col()
        .when(ruled, |el| el.child(div().w_full().h(px(1.0)).bg(theme.line)))
        .child(
            div()
                .id(SharedString::from(format!("group-hdr-{row_id}")))
                .flex()
                .items_center()
                .w_full()
                .h(px(MODULE_HEIGHT))
                .px(px(GUTTER))
                .when(enabled && !skipped, |el| {
                    el.cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
                        on_toggle_open(this, window, cx);
                    }))
                })
                .child(chevron_canvas)
                // Module title + optional skipped pencil strike
                .child(
                    div()
                        .relative()
                        .min_w_0()
                        .child(
                            div()
                                .font(title_font)
                                .text_size(px(title_size))
                                .text_color(title_color)
                                .truncate()
                                .child(SharedString::from(row.title.clone())),
                        )
                        .when(skipped, |el| {
                            el.child(
                                canvas(
                                    |_b, _w, _cx| {},
                                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                                        let r = Bounds {
                                            origin: point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height * 0.5 - px(2.0),
                                            ),
                                            size: size(bounds.size.width, px(5.0)),
                                        };
                                        paint_pencil(window, r, plain, 1.8, accent);
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            )
                        }),
                )
                .child(div().flex_1().min_w(px(8.0)))
                .when(row.changed, |el| {
                    el.child(div().pr(px(8.0)).child(render_edit_dot(theme)))
                })
                .when(show_summary, |el| {
                    el.child(
                        div()
                            .pr(px(if has_switch { 11.0 } else { 0.0 }))
                            .font(sum_font)
                            .text_size(px(sum_size))
                            .text_color(if row.changed { theme.paper } else { theme.dim })
                            .whitespace_nowrap()
                            .child(summary_text),
                    )
                })
                .when(has_switch, |el| {
                    el.child(
                        div()
                            .when(skipped, |d| d.opacity(0.4))
                            .child(render_tick_box(
                                SharedString::from(format!("group-sw-{row_id}")),
                                row.checked,
                                row.switch_enabled,
                                theme,
                                cx,
                                on_toggle_switch,
                            )),
                    )
                }),
        )
}

/// Renders a toggle row (`Kind::Toggle`, `Kind::MatchBrightness`, `Kind::BorderToggle`, `L20`):
/// `29 px` tall, `22 px` gutter, UI `12 pt` label (`paper` when changed, else `label`),
/// right-aligned `13 × 13` `TickBox`. Clicking anywhere on the row toggles it.
pub fn render_toggle_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_check: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + Clone + 'static,
    on_reset: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let next = !row.checked;
    let label_color = if row.changed { theme.paper } else { theme.label };
    let hover_paper = theme.paper;
    let on_check_row = on_check.clone();

    div()
        .id(SharedString::from(format!("toggle-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .gap(px(8.0))
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.text_color(hover_paper))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if event.click_count >= 2 {
                            on_reset(this, window, cx);
                        } else {
                            on_check_row(this, next, window, cx);
                        }
                    }),
                )
        })
        .child(
            div()
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
                .text_color(label_color)
                .truncate()
                .min_w_0()
                .child(SharedString::from(row.title.clone())),
        )
        .child(render_tick_box(
            SharedString::from(format!("toggle-sw-{row_id}")),
            row.checked,
            enabled,
            theme,
            cx,
            on_check,
        ))
}

/// Renders a borderless inline choice row (`Kind::Choice`, `FilmFormat`,
/// `PreviewSpace`, `SizeMode`, `FilterLink`, `DevTime`, `L20`):
/// `29 px` tall, `22 px` gutter, UI `12 pt` label on left, borderless
/// Newsreader `14.5 pt` (`700` + `PencilStroke`/highlighter when changed, else
/// `400`) + `6 px` gap + crisp vector `⌄` caret on the right.
pub fn render_choice_row<V: 'static>(
    row: &RowView,
    options: &[String],
    _menu_open: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle_menu: impl Fn(&mut V, f32, &mut Window, &mut Context<V>) + 'static,
    on_reset: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let changed = row.changed;
    let current = row
        .selected
        .and_then(|i| options.get(i))
        .cloned()
        .unwrap_or_else(|| "—".to_string());
    let label_color = if changed { theme.paper } else { theme.label };
    let (val_font, val_size) =
        theme.serif_spec(scale::VALUE, if changed { 700.0 } else { 400.0 }, false);
    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let accent = theme.accent;
    let highlighter_ink = theme.highlighter_ink;
    let caret_col = theme.dim;

    let caret_canvas = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let cx_p = f32::from(bounds.origin.x) + f32::from(bounds.size.width) * 0.5;
            let cy_p = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5 - 0.5;
            let mut b = PathBuilder::stroke(px(1.2));
            b.move_to(point(px(cx_p - 3.5), px(cy_p - 1.5)));
            b.line_to(point(px(cx_p), px(cy_p + 2.0)));
            b.line_to(point(px(cx_p + 3.5), px(cy_p - 1.5)));
            if let Ok(path) = b.build() {
                window.paint_path(path, caret_col);
            }
        },
    )
    .w(px(9.0))
    .h(px(12.0))
    .flex_shrink_0();

    div()
        .id(SharedString::from(format!("choice-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .gap(px(8.0))
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .when(enabled, |el| {
            el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        on_reset(this, window, cx);
                    } else {
                        on_toggle_menu(this, f32::from(event.position.y), window, cx);
                    }
                }),
            )
        })
        .child(
            div()
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
                .text_color(label_color)
                .truncate()
                .min_w_0()
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .id(SharedString::from(format!("choice-btn-{row_id}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .flex_shrink_0()
                .child(
                    div()
                        .relative()
                        .pb(px(2.0))
                        .when(changed, |el| {
                            el.child(
                                canvas(
                                    |_b, _w, _cx| {},
                                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                                        let r = Bounds {
                                            origin: point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height - px(4.0),
                                            ),
                                            size: size(bounds.size.width, px(5.0)),
                                        };
                                        if highlighter {
                                            paint_highlighter(window, r, plain, 5.0, highlighter_ink);
                                        } else {
                                            paint_pencil(
                                                window,
                                                r,
                                                plain,
                                                negs_marks::PENCIL_WIDTH as f32,
                                                accent,
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
                                .font(val_font)
                                .text_size(px(val_size))
                                .text_color(theme.paper)
                                .whitespace_nowrap()
                                .child(current),
                        ),
                )
                .child(caret_canvas),
        )
}

/// Renders an editorial italic disclosure line (`Kind::More`, `L20`):
/// Newsreader `13 pt` italic `dim`, hover `label`, `px(22)`, `pt(2)`, `pb(8)`.
pub fn render_more_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let hover_label = theme.label;
    let (font, sz) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .id(SharedString::from(format!("more-row-{row_id}")))
        .w_full()
        .px(px(GUTTER))
        .pt(px(4.0))
        .pb(px(8.0))
        .font(font)
        .text_size(px(sz))
        .text_color(theme.dim)
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.text_color(hover_label))
                .active(|s| s.opacity(0.62))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_press(this, window, cx);
                }))
        })
        .child(SharedString::from(row.title.clone()))
}

/// Renders an italic helper hint or warning (`Kind::Note`, `Kind::Footer`, `L20`):
/// Newsreader `13 pt` italic (`warn` or `dim`), `px(22)`, `pb(7)`.
pub fn render_note_row(row: &RowView, theme: &GpuiTheme) -> impl IntoElement {
    let color = if row.warn { theme.warn } else { theme.dim };
    let (font, sz) = theme.serif_spec(scale::HINT, 400.0, true);
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px(px(GUTTER))
        .pb(px(7.0))
        .font(font)
        .text_size(px(sz))
        .line_height(px(16.5))
        .text_color(color)
        .child(row.hint.clone())
}

/// Renders a right-aligned row of negative/prep capsule action buttons (`Kind::Actions`, `L24`).
pub fn render_actions_row<V: 'static>(
    row: &RowView,
    buttons: &'static [(&'static str, &'static str)],
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + Clone + 'static,
) -> AnyElement {
    let row_id = row.id;
    let children: Vec<AnyElement> = buttons
        .iter()
        .enumerate()
        .map(|(i, (label, tip))| {
            let enabled = row.buttons.get(i).copied().unwrap_or(row.enabled);
            let cb = on_press.clone();
            let tooltip = (!tip.is_empty()).then(|| SharedString::from(*tip));
            render_outlined_button(
                SharedString::from(format!("act-{row_id}-{i}")),
                *label,
                enabled,
                tooltip,
                theme,
                cx,
                move |this, window, cx| cb(this, i, window, cx),
            )
        })
        .collect();
    div()
        .w_full()
        .px(px(GUTTER))
        .py(px(4.0))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap(px(8.0))
        .children(children)
        .into_any_element()
}

/// Renders the right-aligned `Kind::BalancePrint` capsule button row (`L24`).
pub fn render_balance_print_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    div()
        .w_full()
        .px(px(GUTTER))
        .py(px(4.0))
        .flex()
        .items_center()
        .justify_end()
        .child(render_outlined_button(
            SharedString::from(format!("balance-print-{row_id}")),
            SharedString::from(row.title.clone()),
            row.enabled,
            None,
            theme,
            cx,
            on_press,
        ))
}

/// Renders the `Kind::FlatField` row (UI `12 pt` label, serif value, right-aligned
/// capsule `Choose…` and `Off` buttons, `L24`).
pub fn render_flat_field_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_choose: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    on_clear: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let can_choose = row.buttons.first().copied().unwrap_or(row.enabled);
    let can_clear = row.buttons.get(1).copied().unwrap_or(false);
    let (val_font, val_size) = theme.serif_spec(scale::VALUE, 400.0, !row.changed);

    div()
        .w_full()
        .px(px(GUTTER))
        .py(px(4.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .font(theme.ui_font(FontWeight::NORMAL))
                        .text_size(px(scale::ROW_LABEL))
                        .text_color(if row.changed { theme.paper } else { theme.label })
                        .child(SharedString::from(row.title.clone())),
                )
                .child(
                    div()
                        .font(val_font)
                        .text_size(px(val_size))
                        .text_color(if row.changed { theme.paper } else { theme.dim })
                        .truncate()
                        .child(row.value.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(render_outlined_button(
                    SharedString::from(format!("ff-choose-{row_id}")),
                    "Choose…",
                    can_choose,
                    None,
                    theme,
                    cx,
                    on_choose,
                ))
                .child(render_outlined_button(
                    SharedString::from(format!("ff-off-{row_id}")),
                    "Off",
                    can_clear,
                    None,
                    theme,
                    cx,
                    on_clear,
                )),
        )
}

/// Paints a miniature sky-to-sand photograph inside `rect` for a border swatch (`Border.swift:376`).
fn paint_mini_photo(window: &mut Window, rect: Bounds<Pixels>, radius: f32, alpha: f32) {
    let steps = 24usize;
    let y0 = f32::from(rect.origin.y);
    let h = f32::from(rect.size.height);
    let band_h = h / steps as f32;
    let top = (0.55f32, 0.68f32, 0.78f32);
    let bot = (0.86f32, 0.74f32, 0.56f32);
    for s in 0..steps {
        let t = (s as f32 + 0.5) / steps as f32;
        let r = ((top.0 + (bot.0 - top.0) * t) * 255.0).round() as u32;
        let g = ((top.1 + (bot.1 - top.1) * t) * 255.0).round() as u32;
        let b = ((top.2 + (bot.2 - top.2) * t) * 255.0).round() as u32;
        let col = hex((r << 16) | (g << 8) | b).opacity(alpha);
        let seg = Bounds {
            origin: point(rect.origin.x, px(y0 + s as f32 * band_h)),
            size: size(rect.size.width, px(band_h + 0.5)),
        };
        let corners = if radius > 0.0 {
            Corners {
                top_left: px(if s == 0 { radius } else { 0.0 }),
                top_right: px(if s == 0 { radius } else { 0.0 }),
                bottom_right: px(if s + 1 == steps { radius } else { 0.0 }),
                bottom_left: px(if s + 1 == steps { radius } else { 0.0 }),
            }
        } else {
            Corners::default()
        };
        window.paint_quad(quad(
            seg,
            corners,
            col,
            Edges::default(),
            col,
            gpui::BorderStyle::Solid,
        ));
    }
}

/// Renders `Kind::BorderStyles` as 4 visual `66 × 46` swatches (`Solid`, `Film`,
/// `Keyline`, `35mm`) ringed with a tilted `PencilRing` when selected (`L23`).
#[allow(clippy::too_many_arguments)]
pub fn render_border_styles_row<V: 'static>(
    row: &RowView,
    options: &[String],
    mat_color: BorderColor,
    line_color: BorderColor,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + Clone + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let mat_hsla = border_color_hsla(mat_color);
    let line_hsla = border_color_hsla(line_color);
    let track_col = theme.track;
    let accent = theme.accent;
    let plain = theme.tokens.plain;

    div()
        .w_full()
        .px(px(GUTTER))
        .py(px(12.0))
        .flex()
        .items_start()
        .justify_between()
        .gap(px(8.0))
        .when(!row.enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .children(options.iter().enumerate().map(|(i, title)| {
            let btn_enabled = row.buttons.get(i).copied().unwrap_or(row.enabled);
            let chosen = row.selected == Some(i);
            let cb = on_press.clone();
            let (lbl_font, lbl_size) =
                theme.serif_spec(13.0, if chosen { 600.0 } else { 400.0 }, false);
            let swatch_alpha = if btn_enabled { 1.0f32 } else { 0.35f32 };

            let swatch_canvas = canvas(
                |_b, _w, _cx| {},
                move |bounds: Bounds<Pixels>, (), window, _cx| {
                    let x = f32::from(bounds.origin.x);
                    let y = f32::from(bounds.origin.y);
                    let w = f32::from(bounds.size.width);
                    let h = f32::from(bounds.size.height);

                    // Style index order from `border::STYLES`: 0 Solid, 1 Film, 2 Keyline, 3 35mm
                    match i {
                        0 => {
                            // Solid mat
                            window.paint_quad(fill(bounds, mat_hsla.opacity(swatch_alpha)));
                            paint_mini_photo(
                                window,
                                Bounds {
                                    origin: point(px(x + 5.0), px(y + 5.0)),
                                    size: size(px(w - 10.0), px(h - 10.0)),
                                },
                                0.0,
                                swatch_alpha,
                            );
                        }
                        1 => {
                            // Film frame
                            window.paint_quad(fill(bounds, gpui::black().opacity(swatch_alpha)));
                            paint_mini_photo(
                                window,
                                Bounds {
                                    origin: point(px(x + 2.5), px(y + 2.5)),
                                    size: size(px(w - 5.0), px(h - 5.0)),
                                },
                                2.0,
                                swatch_alpha,
                            );
                        }
                        2 => {
                            // Keyline
                            window.paint_quad(fill(bounds, mat_hsla.opacity(swatch_alpha)));
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(px(x + 4.0), px(y + 4.0)),
                                    size: size(px(w - 8.0), px(h - 8.0)),
                                },
                                line_hsla.opacity(swatch_alpha),
                            ));
                            paint_mini_photo(
                                window,
                                Bounds {
                                    origin: point(px(x + 5.0), px(y + 5.0)),
                                    size: size(px(w - 10.0), px(h - 10.0)),
                                },
                                0.0,
                                swatch_alpha,
                            );
                        }
                        _ => {
                            // 35mm strip with 8 top & 8 bottom sprocket holes
                            window.paint_quad(fill(bounds, gpui::black().opacity(swatch_alpha)));
                            let perf_col = hex(0xf2f2f2).opacity(swatch_alpha);
                            let count = 8usize;
                            let gap = 2.5f32;
                            let pad_x = 2.0f32;
                            let hole_w = (w - 2.0 * pad_x - (count - 1) as f32 * gap) / count as f32;
                            for row_y in [y + 2.0, y + h - 5.5] {
                                for k in 0..count {
                                    let hx = x + pad_x + k as f32 * (hole_w + gap);
                                    window.paint_quad(quad(
                                        Bounds {
                                            origin: point(px(hx), px(row_y)),
                                            size: size(px(hole_w), px(3.5)),
                                        },
                                        Corners::all(px(0.8)),
                                        perf_col,
                                        Edges::default(),
                                        perf_col,
                                        gpui::BorderStyle::Solid,
                                    ));
                                }
                            }
                            paint_mini_photo(
                                window,
                                Bounds {
                                    origin: point(px(x + 1.5), px(y + 7.5)),
                                    size: size(px(w - 3.0), px(h - 15.0)),
                                },
                                0.0,
                                swatch_alpha,
                            );
                        }
                    }

                    // Hairline track border
                    window.paint_quad(quad(
                        bounds,
                        Corners::default(),
                        gpui::transparent_black(),
                        Edges::all(px(0.75)),
                        track_col.opacity(swatch_alpha),
                        gpui::BorderStyle::Solid,
                    ));

                    // Tilted PencilRing outside the selected swatch
                    if chosen {
                        let (ox, oy) = (4.0f32, 3.5f32);
                        let ring_rect = Bounds {
                            origin: point(px(x - ox), px(y - oy)),
                            size: size(px(w + ox * 2.0), px(h + oy * 2.0)),
                        };
                        paint_ring(
                            window,
                            ring_rect,
                            negs_marks::Radii::BOX,
                            -0.8,
                            plain,
                            accent,
                        );
                    }
                },
            )
            .w(px(66.0))
            .h(px(46.0));

            div()
                .id(SharedString::from(format!("bstyle-{row_id}-{i}")))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(7.0))
                .when(btn_enabled, |el| {
                    el.cursor_pointer()
                        .active(|s| s.opacity(0.62))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cb(this, i, window, cx);
                        }))
                })
                .child(swatch_canvas)
                .child(
                    div()
                        .font(lbl_font)
                        .text_size(px(lbl_size))
                        .text_color(if chosen { theme.paper } else { theme.dim })
                        .child(title.clone()),
                )
        }))
}

/// Renders `Kind::Color(line)` border color row (`ColourWellRow`, `L23`):
/// borderless `14 px` circle swatch LEFT of Newsreader `14.5 pt` color name.
pub fn render_color_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let color = row.color.unwrap_or(BorderColor::WHITE);
    let (val_font, val_size) = theme.serif_spec(scale::VALUE, 400.0, false);

    div()
        .id(SharedString::from(format!("color-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .gap(px(8.0))
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .when(enabled, |el| {
            el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    on_press(this, window, cx);
                }),
            )
        })
        .child(
            div()
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
                .text_color(theme.label)
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .id(SharedString::from(format!("color-btn-{row_id}")))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .rounded_full()
                        .bg(border_color_hsla(color))
                        .border_1()
                        .border_color(theme.track),
                )
                .child(
                    div()
                        .font(val_font)
                        .text_size(px(val_size))
                        .text_color(theme.paper)
                        .child(row.summary.clone()),
                ),
        )
}

/// Renders `Kind::LongestSide` borderless ruled serif pixel input row (`L23`).
pub fn render_longest_side_row<V: 'static>(
    row: &RowView,
    input_state: &InlineTextState,
    has_error: bool,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<V>,
    on_key: impl Fn(&mut V, &KeyDownEvent, &mut Window, &mut Context<V>) + 'static,
    on_focus: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let (unit_font, unit_size) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .id(SharedString::from(format!("longside-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .gap(px(8.0))
        .when(!row.enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        .child(
            div()
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
                .text_color(theme.label)
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(6.0))
                .child(render_text_input(
                    SharedString::from(format!("longside-input-{row_id}")),
                    input_state,
                    TextInputStyle {
                        width: 84.0,
                        height: 22.0,
                        font_size: scale::VALUE,
                        placeholder: SharedString::from("4000"),
                        align_right: true,
                        error: has_error,
                        disabled: !row.enabled,
                        ruled: true,
                        serif: true,
                    },
                    theme,
                    window,
                    cx,
                    on_key,
                    on_focus,
                ))
                .child(
                    div()
                        .font(unit_font)
                        .text_size(px(unit_size))
                        .text_color(theme.dim)
                        .child("px"),
                ),
        )
}

/// Renders `Kind::Photograph` formatted pixel & megapixel readout row (`ReadoutRow`, `L23`).
pub fn render_photograph_row(row: &RowView, theme: &GpuiTheme) -> impl IntoElement {
    let (val_font, val_size) = theme.serif_spec(scale::VALUE, 400.0, false);

    div()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .min_h(px(ROW_HEIGHT))
        .px(px(GUTTER))
        .gap(px(8.0))
        .child(
            div()
                .font(theme.ui_font(FontWeight::NORMAL))
                .text_size(px(scale::ROW_LABEL))
                .text_color(theme.label)
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .font(val_font)
                .text_size(px(val_size))
                .text_color(theme.paper)
                .child(row.value.clone()),
        )
}
