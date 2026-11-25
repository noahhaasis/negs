//! Custom darkroom titlebar in Newsreader + UI typography with file actions,
//! Before/After & Split controls, zoom indicator, Inspector toggle, and 6-theme switcher.

use gpui::{Context, InteractiveElement, IntoElement, SharedString, Styled, Window, div, prelude::*, px};
use negs_app::theme::ThemeName;

use crate::NegsApp;
use crate::theme_gpui::{GpuiTheme, hex};

pub const TITLEBAR_HEIGHT: f32 = 42.0;

fn pill_button(
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
    disabled: bool,
    theme: &GpuiTheme,
) -> gpui::Stateful<gpui::Div> {
    let bg = if active {
        theme.accent.opacity(0.18)
    } else {
        theme.panel
    };
    let border = if active { theme.accent } else { theme.track.opacity(0.65) };
    let text = if disabled {
        theme.dim.opacity(0.45)
    } else if active {
        theme.paper
    } else {
        theme.label
    };
    let hover = theme.subtle_hover;

    div()
        .id(id)
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(12.0))
        .text_color(text)
        .when(!disabled, |el| el.cursor_pointer().hover(move |s| s.bg(hover)))
        .child(label.into())
}

pub fn render_titlebar(
    app: &NegsApp,
    _window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let theme = &app.theme;
    let has_photo = app.model.source.is_some();
    let can_save = app.model.can_save();
    let can_original = app.model.can_show_original();

    let photo_name = app
        .model
        .source
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());

    let stock_badge = if has_photo {
        let film = app.model.film_name();
        let paper = app.model.paper_name();
        let scan_only = negs_app::json::boolean(app.model.value("io.scan_film"));
        if scan_only || paper.is_empty() {
            Some(film)
        } else {
            Some(format!("{film} · {paper}"))
        }
    } else {
        None
    };

    let zoom_label = if !has_photo {
        "Fit".to_string()
    } else if app.canvas.fit {
        format!("Fit ({:.0}%)", (app.canvas.zoom * 100.0).round())
    } else {
        format!("{:.0}%", (app.canvas.zoom * 100.0).round())
    };

    let left_pad = if cfg!(target_os = "macos") { 78.0 } else { 14.0 };
    let in_grid = app.model.shelf.in_grid;
    let can_grid = app.model.shelf.library.is_some() && has_photo && !app.model.cropping();
    let comparing = app.model.comparison().is_some();
    let can_compare = comparing || app.model.compare_word().0;
    let in_grid = in_grid && !comparing;

    div()
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(TITLEBAR_HEIGHT))
        .pl(px(left_pad))
        .pr(px(14.0))
        .bg(theme.panel)
        .border_b_1()
        .border_color(theme.line)
        .font_family(theme.ui_family.clone())
        // Left: Brand + Filename + Active film/paper stock
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(17.0))
                        .text_color(theme.paper)
                        .child("Negs"),
                )
                .when_some(photo_name, |el, name| {
                    el.child(div().text_size(px(12.0)).text_color(theme.dim).child("/"))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(theme.paper)
                                .truncate()
                                .child(name),
                        )
                })
                .when_some(stock_badge, |el, badge| {
                    el.child(
                        div()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.canvas)
                            .border_1()
                            .border_color(theme.line)
                            .font_family(theme.serif_family.clone())
                            .italic()
                            .text_size(px(12.0))
                            .text_color(theme.label)
                            .child(badge),
                    )
                }),
        )
        // Right: Open, Save, Original, Split, Zoom, Theme switcher, Inspector toggle
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                // Photo · Grid: the two views of the folder (G).
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            pill_button("tb-photo", "Photo", !in_grid && has_photo && !comparing, !has_photo, theme).on_click(cx.listener(
                                |this, _, window, cx| {
                                    if this.model.comparison().is_some() {
                                        this.model.leave_compare();
                                        this.after_model_update(cx);
                                        cx.notify();
                                    } else if this.grid.shown && this.model.shelf.in_grid {
                                        this.toggle_grid(window, cx);
                                    }
                                },
                            )),
                        )
                        .child(
                            pill_button("tb-grid", "Grid", in_grid, !can_grid, theme).on_click(cx.listener(
                                |this, _, window, cx| {
                                    if this.model.comparison().is_some() {
                                        this.model.leave_compare();
                                        crate::grid::open(this);
                                        this.after_model_update(cx);
                                        cx.notify();
                                    } else if !this.model.shelf.in_grid {
                                        this.toggle_grid(window, cx);
                                    }
                                },
                            )),
                        )
                        // Compare: the two selected, or back out (C).
                        .child(
                            pill_button("tb-compare", "Compare", comparing, !can_compare, theme).on_click(cx.listener(
                                |this, _, _window, cx| {
                                    if this.model.comparison().is_none() {
                                        crate::compare_view::toggle(this);
                                        this.after_model_update(cx);
                                        cx.notify();
                                    }
                                },
                            )),
                        ),
                )
                .child(div().w(px(1.0)).h(px(18.0)).bg(theme.line))
                .child(
                    pill_button("tb-open", "Open", false, false, theme)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.prompt_open_photo(window, cx);
                        })),
                )
                .child(
                    pill_button("tb-save", "Save", false, !can_save, theme).on_click(cx.listener(
                        |this, _, window, cx| {
                            if this.model.can_save() {
                                this.prompt_save_preview(window, cx);
                            }
                        },
                    )),
                )
                .child(div().w(px(1.0)).h(px(18.0)).bg(theme.line))
                .child(
                    pill_button(
                        "tb-original",
                        "Original",
                        app.canvas.before_after,
                        !can_original,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.toggle_before_after(cx);
                    })),
                )
                .child(
                    pill_button(
                        "tb-split",
                        "Split",
                        app.canvas.split_mode,
                        !can_original,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.toggle_split_mode(cx);
                    })),
                )
                .child(
                    pill_button("tb-zoom", zoom_label, !app.canvas.fit && has_photo, !has_photo, theme)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.toggle_zoom_fit(cx);
                        })),
                )
                .child(div().w(px(1.0)).h(px(18.0)).bg(theme.line))
                .child(
                    pill_button(
                        "tb-theme",
                        format!("{} ▾", theme.name.title()),
                        app.theme_menu_open,
                        false,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.theme_menu_open = !this.theme_menu_open;
                        cx.notify();
                    })),
                )
                .child(
                    pill_button(
                        "tb-inspector",
                        "Inspector",
                        app.model.inspector_visible,
                        false,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.toggle_inspector(cx);
                    })),
                ),
        )
}

/// Floating dropdown listing all 6 built-in themes with color swatches.
pub fn render_theme_menu(
    app: &NegsApp,
    cx: &mut Context<NegsApp>,
) -> Option<gpui::AnyElement> {
    if !app.theme_menu_open {
        return None;
    }
    let theme = &app.theme;
    let current = theme.name;
    let hover = theme.subtle_hover;

    Some(
        div()
            .id("theme-dropdown")
            .absolute()
            .top(px(TITLEBAR_HEIGHT + 4.0))
            .right(px(96.0))
            .w(px(210.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(theme.panel)
            .border_1()
            .border_color(theme.track)
            .shadow_md()
            .flex()
            .flex_col()
            .children(ThemeName::ALL.into_iter().map(|candidate| {
                let tokens = candidate.tokens();
                let selected = candidate == current;
                div()
                    .id(SharedString::from(format!("theme-opt-{}", candidate.key())))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.set_theme(candidate, cx);
                        this.theme_menu_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .rounded(px(3.0))
                                    .bg(hex(tokens.hex.bg))
                                    .border_1()
                                    .border_color(hex(tokens.hex.accent)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(if selected { theme.paper } else { theme.label })
                                    .child(candidate.title()),
                            ),
                    )
                    .when(selected, |el| {
                        el.child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded(px(3.0))
                                .bg(theme.accent),
                        )
                    })
            }))
            .into_any_element(),
    )
}
