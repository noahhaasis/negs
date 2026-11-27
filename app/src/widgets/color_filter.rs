//! Lab › Camera › Color Filter tactile glass swatch discs (`ColorFilterPicker`, `L23`).
//!
//! Renders "None" (with a `-45°` `1.2 px` slash) plus every `ColorFilter` from
//! `model.color_filters` as `26 px` glass discs with a tilted `34 × 32`
//! `PencilRing` (`-8°`) on the chosen disc, followed by Newsreader name & glass
//! code, italic stop-loss hint, and color-film warning.

use gpui::{
    Bounds, Context, IntoElement, PathBuilder, Pixels, SharedString, Window, canvas, div, point,
    prelude::*, px, size,
};
use negs_app::inspector_view::RowView;
use negs_app::state::ColorFilter;
use negs_app::theme::DISABLED_OPACITY;

use crate::marks_gpui::paint_ring;
use crate::theme_gpui::{GpuiTheme, hex, hex_alpha, scale};
use crate::widgets::controls::GUTTER;

const DISC_SIZE: f32 = 26.0;

/// Renders the `Kind::ColorFilter` glass swatch discs and hint/warning lines (`L23`).
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

    let plain = theme.tokens.plain;
    let accent = theme.accent;
    let dim_color = theme.dim;
    let label_hover = theme.label;
    let (title_font, title_size) = theme.serif_spec(15.0, 600.0, false);
    let (glass_font, glass_size) = theme.serif_spec(scale::HINT, 400.0, true);
    let (hint_font, hint_size) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .id(SharedString::from(format!("color-filter-{}", row.id)))
        .w_full()
        .flex()
        .flex_col()
        .when(!enabled, |el| el.opacity(DISABLED_OPACITY as f32))
        // Row of glass swatch discs
        .child(
            div()
                .w_full()
                .px(px(GUTTER - 4.0))
                .pt(px(8.0))
                .flex()
                .items_start()
                .justify_between()
                .gap(px(2.0))
                .children(items.into_iter().enumerate().map(|(i, (key, name, colour, selected))| {
                    let cb = on_choose.clone();
                    let (lbl_font, lbl_size) =
                        theme.serif_spec(11.5, if selected { 600.0 } else { 400.0 }, false);

                    let disc = if let Some(rgb_hex) = colour {
                        div()
                            .relative()
                            .w(px(DISC_SIZE))
                            .h(px(DISC_SIZE))
                            .rounded_full()
                            .bg(hex(rgb_hex))
                            .border(px(1.5))
                            .border_color(hex_alpha(0x000000, 0.35))
                            // Layered glass specular highlight
                            .child(
                                div()
                                    .absolute()
                                    .top(px(3.0))
                                    .left(px(4.0))
                                    .w(px(10.0))
                                    .h(px(8.0))
                                    .rounded_full()
                                    .bg(hex_alpha(0xffffff, 0.24)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(4.0))
                                    .left(px(5.0))
                                    .w(px(6.0))
                                    .h(px(4.5))
                                    .rounded_full()
                                    .bg(hex_alpha(0xffffff, 0.36)),
                            )
                    } else {
                        // "None" disc with -45° 1.2 px diagonal slash
                        div()
                            .relative()
                            .w(px(DISC_SIZE))
                            .h(px(DISC_SIZE))
                            .rounded_full()
                            .bg(theme.blank)
                            .border(px(1.5))
                            .border_color(hex_alpha(0x000000, 0.35))
                            .child(
                                canvas(
                                    |_b, _w, _cx| {},
                                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                                        let cx_p = f32::from(bounds.origin.x) + f32::from(bounds.size.width) * 0.5;
                                        let cy_p = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;
                                        let d = 6.0f32;
                                        let mut builder = PathBuilder::stroke(px(1.2));
                                        builder.move_to(point(px(cx_p - d), px(cy_p + d)));
                                        builder.line_to(point(px(cx_p + d), px(cy_p - d)));
                                        if let Ok(path) = builder.build() {
                                            window.paint_path(path, dim_color);
                                        }
                                    },
                                )
                                .size_full(),
                            )
                    };

                    div()
                        .id(SharedString::from(format!("cf-disc-{i}")))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(7.0))
                        .min_w(px(34.0))
                        .when(enabled, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(label_hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cb(this, key.clone(), window, cx);
                                }))
                        })
                        .child(
                            div()
                                .relative()
                                .w(px(34.0))
                                .h(px(32.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(disc)
                                .when(selected, |el| {
                                    el.child(
                                        canvas(
                                            |_b, _w, _cx| {},
                                            move |bounds: Bounds<Pixels>, (), window, _cx| {
                                                let ring_rect = Bounds {
                                                    origin: bounds.origin,
                                                    size: size(px(34.0), px(32.0)),
                                                };
                                                paint_ring(
                                                    window,
                                                    ring_rect,
                                                    negs_marks::Radii::NUMBER,
                                                    -8.0,
                                                    plain,
                                                    accent,
                                                );
                                            },
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .font(lbl_font)
                                .text_size(px(lbl_size))
                                .text_color(if selected { theme.paper } else { theme.dim })
                                .whitespace_nowrap()
                                .child(name),
                        )
                })),
        )
        // Chosen filter name + glass code
        .child(
            div()
                .w_full()
                .px(px(GUTTER))
                .pt(px(10.0))
                .pb(px(3.0))
                .flex()
                .items_baseline()
                .gap(px(7.0))
                .child(
                    div()
                        .font(title_font)
                        .text_size(px(title_size))
                        .text_color(theme.paper)
                        .child(chosen_title.to_string()),
                )
                .when_some(chosen_glass, |el, glass| {
                    el.child(
                        div()
                            .font(glass_font)
                            .text_size(px(glass_size))
                            .text_color(theme.dim)
                            .child(glass),
                    )
                }),
        )
        // Stops / filter factor hint
        .when(!row.hint.is_empty(), |el| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(GUTTER))
                    .pb(px(7.0))
                    .font(hint_font.clone())
                    .text_size(px(hint_size))
                    .line_height(px(16.5))
                    .text_color(theme.dim)
                    .child(row.hint.clone()),
            )
        })
        // Color-film warning
        .when(!row.warning.is_empty(), |el| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(GUTTER))
                    .pb(px(7.0))
                    .font(hint_font)
                    .text_size(px(hint_size))
                    .line_height(px(16.5))
                    .text_color(theme.warn)
                    .child(row.warning.clone()),
            )
        })
}
