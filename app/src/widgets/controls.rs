//! Inspector darkroom controls: SectionHeader, GroupHeader, Toggle, Choice,
//! MoreRow, NoteRow/FooterRow, ActionButtonsRow, FlatField, BorderStyles,
//! BorderColor well, LongestSide, and Photograph readout.

use gpui::{
    Context, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, px,
};
use negs_app::border::BorderColor;
use negs_app::inspector_view::RowView;

use crate::theme_gpui::{GpuiTheme, hex};
use crate::widgets::text_input::{InlineTextState, TextInputStyle, render_text_input};

/// Editorial darkroom border color presets offered in the inline color picker popover.
pub const BORDER_COLOR_PRESETS: [(&str, BorderColor); 4] = [
    ("White", BorderColor::WHITE),
    ("Black", BorderColor::BLACK),
    ("Warm", BorderColor([0xf5, 0xef, 0xe6])),
    ("Cream", BorderColor([0xed, 0xe4, 0xd3])),
];

/// Converts a `BorderColor` into a GPUI `Hsla`.
pub fn border_color_hsla(c: BorderColor) -> gpui::Hsla {
    let packed = ((c.0[0] as u32) << 16) | ((c.0[1] as u32) << 8) | (c.0[2] as u32);
    hex(packed)
}

/// Renders a crisp pill toggle switch.
pub fn pill_switch<V: 'static>(
    id: SharedString,
    checked: bool,
    enabled: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let track_bg = if !enabled {
        theme.track.opacity(0.45)
    } else if checked {
        theme.accent
    } else {
        theme.track
    };
    let knob_color = if checked && enabled {
        theme.on_fill
    } else {
        theme.paper.opacity(if enabled { 0.92 } else { 0.5 })
    };
    let next = !checked;

    div()
        .id(id)
        .flex()
        .items_center()
        .w(px(28.0))
        .h(px(16.0))
        .p(px(2.0))
        .rounded_full()
        .bg(track_bg)
        .when(checked, |el| el.justify_end())
        .when(enabled, |el| {
            el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    on_toggle(this, next, window, cx);
                }),
            )
        })
        .child(div().w(px(12.0)).h(px(12.0)).rounded_full().bg(knob_color))
}

/// Renders a section header (`Kind::Section`). Untitled sections become a subtle divider gap.
pub fn render_section_header(row: &RowView, is_first: bool, theme: &GpuiTheme) -> impl IntoElement {
    if row.title.is_empty() {
        return div()
            .w_full()
            .px(px(16.0))
            .pt(px(if is_first { 4.0 } else { 10.0 }))
            .pb(px(4.0))
            .child(div().w_full().h(px(1.0)).bg(theme.line))
            .into_any_element();
    }

    let title = row.title.to_uppercase();
    div()
        .w_full()
        .px(px(16.0))
        .pt(px(if is_first { 10.0 } else { 14.0 }))
        .pb(px(6.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .when(!is_first, |el| el.child(div().w_full().h(px(1.0)).bg(theme.line)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(if row.skipped {
                            theme.dim.opacity(0.6)
                        } else {
                            theme.dim
                        })
                        .when(row.skipped, |el| el.line_through())
                        .child(title),
                )
                .when(row.changed, |el| {
                    el.child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                }),
        )
        .into_any_element()
}

/// Renders a foldout module header (`Kind::Group`).
pub fn render_group_header<V: 'static>(
    row: &RowView,
    has_switch: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle_open: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    on_toggle_switch: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let hover_bg = theme.subtle_hover;
    let chevron = if row.open { "▾" } else { "▸" };
    let title_color = if row.skipped || row.off || !enabled {
        theme.dim
    } else {
        theme.paper
    };

    div()
        .id(SharedString::from(format!("group-hdr-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(7.0))
        .gap_2()
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_toggle_open(this, window, cx);
                }))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_w_0()
                .child(
                    div()
                        .w(px(10.0))
                        .text_size(px(11.0))
                        .text_color(theme.dim)
                        .child(chevron),
                )
                .child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(14.0))
                        .text_color(title_color)
                        .when(row.skipped, |el| el.line_through())
                        .truncate()
                        .child(SharedString::from(row.title.clone())),
                )
                .when(row.changed, |el| {
                    el.child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .when(!row.open && !row.summary.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.dim)
                            .child(row.summary.clone()),
                    )
                })
                .when(has_switch, |el| {
                    el.child(pill_switch(
                        SharedString::from(format!("group-sw-{row_id}")),
                        row.checked,
                        row.switch_enabled,
                        theme,
                        cx,
                        on_toggle_switch,
                    ))
                }),
        )
}

/// Renders a toggle row (`Kind::Toggle`, `Kind::MatchBrightness`, `Kind::BorderToggle`).
pub fn render_toggle_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_check: impl Fn(&mut V, bool, &mut Window, &mut Context<V>) + 'static,
    on_reset: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let label_color = if !enabled {
        theme.dim.opacity(0.55)
    } else if row.changed {
        theme.paper
    } else {
        theme.label
    };

    div()
        .id(SharedString::from(format!("toggle-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .gap_2()
        .when(!enabled, |el| el.opacity(0.55))
        .child(
            div()
                .id(SharedString::from(format!("toggle-lbl-{row_id}")))
                .flex()
                .items_center()
                .gap(px(5.0))
                .min_w_0()
                .when(enabled, |el| el.cursor_pointer())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if enabled && event.click_count >= 2 {
                            on_reset(this, window, cx);
                        }
                    }),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(label_color)
                        .truncate()
                        .child(SharedString::from(row.title.clone())),
                )
                .when(row.changed, |el| {
                    el.child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                }),
        )
        .child(pill_switch(
            SharedString::from(format!("toggle-sw-{row_id}")),
            row.checked,
            enabled,
            theme,
            cx,
            on_check,
        ))
}

/// Renders a dropdown choice row (`Kind::Choice`, `Kind::FilmFormat`, `Kind::PreviewSpace`,
/// `Kind::SizeMode`, `Kind::FilterLink`, `Kind::DevTime`).
pub fn render_choice_row<V: 'static>(
    row: &RowView,
    options: &[String],
    menu_open: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_toggle_menu: impl Fn(&mut V, f32, &mut Window, &mut Context<V>) + 'static,
    on_reset: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let current = row
        .selected
        .and_then(|i| options.get(i))
        .cloned()
        .unwrap_or_else(|| "—".to_string());
    let hover_bg = theme.subtle_hover;
    let border = if menu_open {
        theme.accent
    } else {
        theme.track.opacity(0.75)
    };
    let label_color = if !enabled {
        theme.dim.opacity(0.55)
    } else if row.changed {
        theme.paper
    } else {
        theme.label
    };

    div()
        .id(SharedString::from(format!("choice-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .gap_2()
        .when(!enabled, |el| el.opacity(0.55))
        .child(
            div()
                .id(SharedString::from(format!("choice-lbl-{row_id}")))
                .flex()
                .items_center()
                .gap(px(5.0))
                .min_w_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if enabled && event.click_count >= 2 {
                            on_reset(this, window, cx);
                        }
                    }),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(label_color)
                        .truncate()
                        .child(SharedString::from(row.title.clone())),
                )
                .when(row.changed, |el| {
                    el.child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("choice-btn-{row_id}")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .max_w(px(168.0))
                .px(px(8.0))
                .py(px(3.5))
                .rounded(px(4.0))
                .bg(theme.canvas)
                .border_1()
                .border_color(border)
                .text_size(px(11.5))
                .text_color(if enabled { theme.paper } else { theme.dim })
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                on_toggle_menu(this, f32::from(event.position.y), window, cx);
                            }),
                        )
                })
                .child(div().truncate().child(current))
                .child(div().text_size(px(10.0)).text_color(theme.dim).child("▾")),
        )
}

/// Renders an editorial italic disclosure line (`Kind::More`).
pub fn render_more_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let hover_text = theme.paper;

    div()
        .id(SharedString::from(format!("more-row-{row_id}")))
        .w_full()
        .px(px(16.0))
        .py(px(4.0))
        .font_family(theme.serif_family.clone())
        .italic()
        .text_size(px(12.5))
        .text_color(theme.dim)
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.text_color(hover_text))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_press(this, window, cx);
                }))
        })
        .child(SharedString::from(row.title.clone()))
}

/// Renders a dim helper note or warning text (`Kind::Note`, `Kind::Footer`).
pub fn render_note_row(row: &RowView, theme: &GpuiTheme) -> impl IntoElement {
    let color = if row.warn { theme.warn } else { theme.dim };
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px(px(16.0))
        .py(px(3.0))
        .text_size(px(11.0))
        .line_height(px(15.0))
        .text_color(color)
        .child(row.hint.clone())
}

/// Renders a compact darkroom action button.
fn action_button<V: 'static>(
    id: SharedString,
    label: impl Into<SharedString>,
    enabled: bool,
    active: bool,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    let bg = if active {
        theme.accent.opacity(0.2)
    } else {
        theme.canvas
    };
    let border = if active {
        theme.accent
    } else {
        theme.track.opacity(0.75)
    };
    let text = if !enabled {
        theme.dim.opacity(0.45)
    } else if active {
        theme.paper
    } else {
        theme.label
    };
    let hover = theme.subtle_hover;

    div()
        .id(id)
        .px(px(9.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(11.5))
        .text_color(text)
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_click(this, window, cx);
                }))
        })
        .child(label.into())
        .into_any_element()
}

/// Renders a row of negative/prep action buttons (`Kind::Actions`).
pub fn render_actions_row<V: 'static>(
    row: &RowView,
    buttons: &'static [(&'static str, &'static str)],
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + Clone + 'static,
) -> gpui::AnyElement {
    let row_id = row.id;
    let children: Vec<gpui::AnyElement> = buttons
        .iter()
        .enumerate()
        .map(|(i, (label, _tip))| {
            let enabled = row.buttons.get(i).copied().unwrap_or(row.enabled);
            let cb = on_press.clone();
            action_button(
                SharedString::from(format!("act-{row_id}-{i}")),
                *label,
                enabled,
                false,
                theme,
                cx,
                move |this, window, cx| cb(this, i, window, cx),
            )
        })
        .collect();
    div()
        .w_full()
        .px(px(16.0))
        .py(px(4.0))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .children(children)
        .into_any_element()
}

/// Renders the `Kind::BalancePrint` button row.
pub fn render_balance_print_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    div()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .flex()
        .items_center()
        .child(action_button(
            SharedString::from(format!("balance-print-{row_id}")),
            SharedString::from(row.title.clone()),
            row.enabled,
            false,
            theme,
            cx,
            on_press,
        ))
}

/// Renders the `Kind::FlatField` row (label, light frame filename, Choose… and Off buttons).
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

    div()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .flex()
        .flex_col()
        .gap(px(5.0))
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
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(if row.changed { theme.paper } else { theme.label })
                                .child(SharedString::from(row.title.clone())),
                        )
                        .when(row.changed, |el| {
                            el.child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(theme.accent),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.dim)
                        .truncate()
                        .child(row.value.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(action_button(
                    SharedString::from(format!("ff-choose-{row_id}")),
                    "Choose…",
                    can_choose,
                    false,
                    theme,
                    cx,
                    on_choose,
                ))
                .child(action_button(
                    SharedString::from(format!("ff-off-{row_id}")),
                    "Off",
                    can_clear,
                    false,
                    theme,
                    cx,
                    on_clear,
                )),
        )
}

/// Renders `Kind::BorderStyles` as segmented darkroom style buttons (`Solid`, `Film`, `Keyline`, `35mm`).
pub fn render_border_styles_row<V: 'static>(
    row: &RowView,
    options: &[String],
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + Clone + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    div()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .when(!row.enabled, |el| el.opacity(0.5))
        .children(options.iter().enumerate().map(|(i, title)| {
            let enabled = row.buttons.get(i).copied().unwrap_or(row.enabled);
            let active = row.selected == Some(i);
            let cb = on_press.clone();
            let bg = if active {
                theme.accent.opacity(0.2)
            } else {
                theme.canvas
            };
            let border = if active {
                theme.accent
            } else {
                theme.track.opacity(0.7)
            };
            let text_col = if !enabled {
                theme.dim.opacity(0.45)
            } else if active {
                theme.paper
            } else {
                theme.label
            };
            let hover = theme.subtle_hover;

            div()
                .id(SharedString::from(format!("bstyle-{row_id}-{i}")))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .py(px(5.0))
                .rounded(px(4.0))
                .bg(bg)
                .border_1()
                .border_color(border)
                .text_size(px(11.5))
                .text_color(text_col)
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cb(this, i, window, cx);
                        }))
                })
                .child(title.clone())
        }))
}

/// Renders `Kind::Color(line)` border color swatch well.
pub fn render_color_row<V: 'static>(
    row: &RowView,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_press: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let row_id = row.id;
    let enabled = row.enabled;
    let color = row.color.unwrap_or(BorderColor::WHITE);
    let hover = theme.subtle_hover;

    div()
        .id(SharedString::from(format!("color-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .when(!enabled, |el| el.opacity(0.55))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(if row.changed { theme.paper } else { theme.label })
                        .child(SharedString::from(row.title.clone())),
                )
                .when(row.changed, |el| {
                    el.child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("color-btn-{row_id}")))
                .flex()
                .items_center()
                .gap(px(7.0))
                .px(px(8.0))
                .py(px(3.5))
                .rounded(px(4.0))
                .bg(theme.canvas)
                .border_1()
                .border_color(theme.track.opacity(0.75))
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                on_press(this, window, cx);
                            }),
                        )
                })
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .rounded(px(3.0))
                        .bg(border_color_hsla(color))
                        .border_1()
                        .border_color(theme.track),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.paper)
                        .child(row.summary.clone()),
                ),
        )
}

/// Renders `Kind::LongestSide` numeric pixel input row.
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
    div()
        .id(SharedString::from(format!("longside-row-{row_id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(5.0))
        .when(!row.enabled, |el| el.opacity(0.55))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme.label)
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(render_text_input(
                    SharedString::from(format!("longside-input-{row_id}")),
                    input_state,
                    TextInputStyle {
                        width: 82.0,
                        height: 24.0,
                        font_size: 12.0,
                        placeholder: SharedString::from("4000"),
                        align_right: true,
                        error: has_error,
                        disabled: !row.enabled,
                    },
                    theme,
                    window,
                    cx,
                    on_key,
                    on_focus,
                ))
                .child(div().text_size(px(11.0)).text_color(theme.dim).child("px")),
        )
}

/// Renders `Kind::Photograph` formatted pixel & megapixel readout row.
pub fn render_photograph_row(row: &RowView, theme: &GpuiTheme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(16.0))
        .py(px(4.0))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme.dim)
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(theme.label)
                .child(row.value.clone()),
        )
}
