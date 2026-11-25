//! Lab › Camera › Color Filter tactile glass swatch discs (`ColorFilterDiscs`).
//!
//! Renders "None" plus every `ColorFilter` from `model.color_filters` as colored
//! glass discs ringed with the theme accent when active, followed by the chosen
//! filter's name & glass code, stop-loss hint, and color-film warning.

use gpui::{Context, IntoElement, SharedString, Window, div, prelude::*, px};
use negs_app::inspector_view::RowView;
use negs_app::state::ColorFilter;

use crate::theme_gpui::{GpuiTheme, hex, hex_alpha};

/// Renders the `Kind::ColorFilter` glass swatch discs and hint/warning lines.
pub fn render_color_filter<V: 'static>(
    row: &RowView,
    filters: &[ColorFilter],
    chosen_key: &str,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_choose: impl Fn(&mut V, String, &mut Window, &mut Context<V>) + Clone + 'static,
) -> impl IntoElement {
    let enabled = row.enabled;
    let chosen_filter = filters.iter().find(|f| f.key == chosen_key);
    let chosen_title = chosen_filter.map_or("None", |f| f.name.as_str());
    let chosen_glass = chosen_filter.map(|f| f.glass.clone());

    let none_selected = chosen_key == ColorFilter::NONE || chosen_filter.is_none();

    let mut items: Vec<(String, String, Option<u32>, bool)> = Vec::with_capacity(filters.len() + 1);
    items.push((
        ColorFilter::NONE.to_string(),
        "None".to_string(),
        None,
        none_selected,
    ));
    for f in filters {
        items.push((
            f.key.clone(),
            f.name.clone(),
            Some(f.colour),
            f.key == chosen_key,
        ));
    }

    div()
        .id(SharedString::from(format!("color-filter-{}", row.id)))
        .w_full()
        .px(px(16.0))
        .py(px(6.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .when(!enabled, |el| el.opacity(0.5))
        // Row of glass swatch discs
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .justify_between()
                .gap(px(6.0))
                .children(items.into_iter().enumerate().map(|(i, (key, name, colour, selected))| {
                    let cb = on_choose.clone();
                    let ring_color = if selected {
                        theme.accent
                    } else {
                        theme.panel
                    };
                    let disc = if let Some(rgb_hex) = colour {
                        div()
                            .relative()
                            .w(px(26.0))
                            .h(px(26.0))
                            .rounded_full()
                            .bg(hex(rgb_hex))
                            .border_1()
                            .border_color(hex_alpha(0x000000, 0.35))
                            // Subtle glass specular highlight
                            .child(
                                div()
                                    .absolute()
                                    .top(px(4.0))
                                    .left(px(5.0))
                                    .w(px(7.0))
                                    .h(px(5.0))
                                    .rounded_full()
                                    .bg(hex_alpha(0xffffff, 0.38)),
                            )
                    } else {
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(26.0))
                            .h(px(26.0))
                            .rounded_full()
                            .bg(theme.blank)
                            .border_1()
                            .border_color(theme.track)
                            .text_size(px(11.0))
                            .text_color(theme.dim)
                            .child("⊘")
                    };

                    div()
                        .id(SharedString::from(format!("cf-disc-{i}")))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .min_w(px(34.0))
                        .when(enabled, |el| {
                            el.cursor_pointer().on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cb(this, key.clone(), window, cx);
                                },
                            ))
                        })
                        .child(
                            div()
                                .p(px(2.0))
                                .rounded_full()
                                .border_2()
                                .border_color(ring_color)
                                .child(disc),
                        )
                        .child(
                            div()
                                .font_family(theme.serif_family.clone())
                                .text_size(px(11.0))
                                .text_color(if selected { theme.paper } else { theme.dim })
                                .child(name),
                        )
                })),
        )
        // Chosen filter name + glass code
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(7.0))
                .child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(14.5))
                        .text_color(theme.paper)
                        .child(chosen_title.to_string()),
                )
                .when_some(chosen_glass, |el, glass| {
                    el.child(
                        div()
                            .font_family(theme.serif_family.clone())
                            .italic()
                            .text_size(px(12.5))
                            .text_color(theme.dim)
                            .child(glass),
                    )
                }),
        )
        // Stops / filter factor hint
        .when(!row.hint.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .line_height(px(15.0))
                    .text_color(theme.dim)
                    .child(row.hint.clone()),
            )
        })
        // Color-film warning
        .when(!row.warning.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .line_height(px(15.0))
                    .text_color(theme.warn)
                    .child(row.warning.clone()),
            )
        })
}
