//! Canvas-column status line (`StatusLine.swift`, `L26`, `L27`):
//! `28 px` tall in `bg`, `18 px` horizontal padding, no top border, hidden
//! when no photograph is open.
//! - Left: active state (Compare, Export/Batch/Lightroom, Cropping, render
//!   failure) or `dims · developed in X.XX s` — no film/paper prefix and never
//!   `theme.accent` text color.
//! - Right: interactive `Original ⌄` (Hold / Split / Side-by-side + RAW/JPEG
//!   upward menu, `PencilStroke` / highlighter while active) | `1 × 10` `track`
//!   rule | `Fit` | `100%` (or `CompareTools` while comparing).

use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, MouseButton, PathBuilder, Pixels,
    SharedString, Styled, canvas, div, point, prelude::*, px, size,
};
use negs_app::compare::{OriginalMode, OriginalReference};

use crate::NegsApp;
use crate::marks_gpui::{paint_highlighter, paint_pencil};
use crate::theme_gpui::scale;

pub const STATUS_HEIGHT: f32 = 28.0;

/// Renders the bottom status line inside the canvas column (`L26`, `L27`).
/// Returns `None` when no photograph is open and there is no error banner.
pub fn render_status_bar(app: &NegsApp, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    let has_photo = app.model.source.is_some();
    let error_banner = app.model.error.clone();
    if !has_photo && error_banner.is_none() {
        return None;
    }

    let theme = &app.theme;
    let ui_font = theme.ui_font(FontWeight::NORMAL);
    let ui_semi = theme.ui_font(FontWeight::SEMIBOLD);

    let comparing = app.model.comparison().cloned();
    let export_words = crate::batch_view::words(&app.model);
    let exporting_or_batch = app.model.exporting
        || app.model.batch.as_ref().is_some_and(|b| b.running);

    let left_el: AnyElement = if let Some(words) = export_words {
        crate::batch_view::render_words(app, words, cx)
    } else if let Some(ref c) = comparing {
        let differences = crate::compare_view::differences(app);
        let summary = app.model.compare_summary();
        let text = if differences.is_empty() {
            summary
        } else {
            format!("{summary} · differ in {differences}")
        };
        let hint = (!c.against_original() && app.model.shown_items().len() > 2).then(|| {
            let place = if c.layout == negs_app::compare::Layout::Slider {
                "right"
            } else if app.compare.column {
                "bottom"
            } else {
                "right"
            };
            format!("← → steps the {place} one")
        });
        let (hint_font, hint_size) = theme.serif_spec(scale::HINT, 400.0, true);
        div()
            .flex()
            .items_center()
            .gap(px(14.0))
            .min_w_0()
            .child(div().text_color(theme.label).truncate().child(text))
            .when_some(hint, |el, h| {
                el.child(
                    div()
                        .font(hint_font)
                        .text_size(px(hint_size))
                        .text_color(theme.dim)
                        .whitespace_nowrap()
                        .child(h),
                )
            })
            .into_any_element()
    } else if let Some(session) = &app.model.crop_session {
        let sz = match app.model.crop_draft_size() {
            Some(Ok(size)) => (size.photo.0 as f64, size.photo.1 as f64),
            _ => (session.shown().width, session.shown().height),
        };
        let text = format!(
            "Cropping · {} · {} × {} px",
            session.aspect.title(),
            sz.0.round() as i64,
            sz.1.round() as i64
        );
        div()
            .text_color(theme.dim)
            .truncate()
            .child(text)
            .into_any_element()
    } else if app.model.render_failed {
        let name = app
            .model
            .source
            .as_deref()
            .map(negs_app::model::file_name)
            .unwrap_or_else(|| "the photograph".to_string());
        div()
            .flex()
            .items_center()
            .gap(px(5.0))
            .min_w_0()
            .child(
                div()
                    .font(ui_semi)
                    .text_color(theme.paper)
                    .child("Couldn\u{2019}t develop"),
            )
            .child(div().text_color(theme.label).truncate().child(name))
            .into_any_element()
    } else if !app.model.status.is_empty() {
        div()
            .text_color(theme.dim)
            .truncate()
            .child(app.model.status.clone())
            .into_any_element()
    } else if let Some(sz) = has_photo
        .then(|| app.model.photo_size())
        .filter(|s| s.width > 0.0 && s.height > 0.0)
    {
        div()
            .text_color(theme.dim)
            .truncate()
            .child(format!(
                "{} × {}",
                sz.width.round() as u64,
                sz.height.round() as u64
            ))
            .into_any_element()
    } else {
        div().into_any_element()
    };

    let right_el: Option<AnyElement> = if exporting_or_batch {
        None
    } else if let Some(ref c) = comparing {
        Some(
            compare_tools(app, c.layout == negs_app::compare::Layout::Slider, cx)
                .into_any_element(),
        )
    } else if has_photo {
        Some(status_tools(app, cx).into_any_element())
    } else {
        None
    };

    Some(
        div()
            .flex()
            .flex_col()
            .w_full()
            .flex_shrink_0()
            .when_some(error_banner, |el, message| {
                let hover_paper = theme.paper;
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .px(px(18.0))
                        .py(px(6.0))
                        .bg(theme.panel)
                        .border_t_1()
                        .border_color(theme.warn)
                        .child(
                            div()
                                .font(ui_font.clone())
                                .text_size(px(12.0))
                                .text_color(theme.warn)
                                .child(message),
                        )
                        .child(
                            div()
                                .id("dismiss-error")
                                .px_2()
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .cursor_pointer()
                                .font(ui_font.clone())
                                .text_size(px(11.0))
                                .text_color(theme.label)
                                .hover(move |s| s.text_color(hover_paper))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.model.error = None;
                                    cx.notify();
                                }))
                                .child("Dismiss"),
                        ),
                )
            })
            .when(has_photo, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .w_full()
                        .h(px(STATUS_HEIGHT))
                        .px(px(18.0))
                        .bg(theme.canvas)
                        .font(ui_font)
                        .text_size(px(scale::STATUS))
                        .text_color(theme.dim)
                        .child(div().flex_1().min_w_0().child(left_el))
                        .when_some(right_el, |row, tools| row.child(tools)),
                )
            })
            .into_any_element(),
    )
}

/// Right-hand photo tools (`StatusTools`, `L27`):
/// `Original ⌄` | `1 × 10` `track` rule | `Fit` | `100%`.
fn status_tools(app: &NegsApp, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let theme = &app.theme;
    let cropping = app.model.cropping();
    let failed = app.model.render_failed;
    let can_original = app.model.can_show_original() && !failed;
    let mode = app.model.original_mode();
    let original_on = app.canvas.before_after || app.canvas.split_mode;
    let original_word = app.model.original_word();
    let original_phrase = if app.model.shows_camera_jpeg() {
        "camera JPEG"
    } else {
        "original"
    };
    let original_tip = mode.help(original_phrase);

    let fit_active = app.canvas.fit;
    let paper = theme.paper;
    let label = theme.label;
    let dim = theme.dim;
    let track = theme.track;
    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let accent = theme.accent;
    let highlighter_ink = theme.highlighter_ink;

    let caret_canvas = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let cx_p = f32::from(bounds.origin.x) + f32::from(bounds.size.width) * 0.5;
            let cy_p = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;
            let mut b = PathBuilder::stroke(px(1.15));
            b.move_to(point(px(cx_p - 3.0), px(cy_p - 1.5)));
            b.line_to(point(px(cx_p), px(cy_p + 1.5)));
            b.line_to(point(px(cx_p + 3.0), px(cy_p - 1.5)));
            if let Ok(path) = b.build() {
                window.paint_path(path, dim);
            }
        },
    )
    .w(px(10.0))
    .h(px(12.0));

    div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .flex_shrink_0()
        .when(failed, |el| el.opacity(0.38))
        // Original ⌄
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(3.0))
                .child(
                    div()
                        .id("status-original")
                        .tooltip(crate::tooltip::tip(original_tip, theme))
                        .relative()
                        .pb(px(1.0))
                        .when(!can_original, |el| el.opacity(0.38))
                        .when(can_original, |el| {
                            el.cursor_pointer()
                                .on_hover(cx.listener(|this, &inside, _window, _cx| {
                                    if inside {
                                        this.model.prepare_original();
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _event, _window, cx| {
                                        cx.stop_propagation();
                                        this.original_menu_open = false;
                                        if mode == OriginalMode::Hold {
                                            this.set_hold_original(true, cx);
                                        } else {
                                            this.model.toggle_original_compare();
                                            this.after_model_update(cx);
                                            cx.notify();
                                        }
                                    }),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(move |this, _event, _window, cx| {
                                        if mode == OriginalMode::Hold {
                                            this.set_hold_original(false, cx);
                                        }
                                    }),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(move |this, _event, _window, cx| {
                                        if mode == OriginalMode::Hold {
                                            this.set_hold_original(false, cx);
                                        }
                                    }),
                                )
                        })
                        .when(original_on, |el| {
                            el.child(
                                canvas(
                                    |_b, _w, _cx| {},
                                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                                        let r = Bounds {
                                            origin: point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height - px(2.0),
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
                                .text_color(if original_on { paper } else { dim })
                                .when(can_original && !original_on, |d| {
                                    d.hover(move |s| s.text_color(label))
                                })
                                .child(original_word),
                        ),
                )
                .child(
                    div()
                        .id("status-original-menu-btn")
                        .tooltip(crate::tooltip::tip("Choose what Original does", theme))
                        .w(px(12.0))
                        .h(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!failed, |el| {
                            el.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    this.original_menu_open = !this.original_menu_open;
                                    cx.notify();
                                }),
                            )
                        })
                        .child(caret_canvas),
                ),
        )
        // 1 × 10 vertical track rule
        .child(div().w(px(1.0)).h(px(10.0)).bg(track))
        // Fit  100%
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.0))
                .when(cropping && !failed, |el| el.opacity(0.38))
                .child(
                    div()
                        .id("status-zoom-fit")
                        .tooltip(crate::tooltip::tip(
                            if cfg!(target_os = "macos") {
                                "Fit photograph (⌘0)"
                            } else {
                                "Fit photograph (Ctrl+0)"
                            },
                            theme,
                        ))
                        .text_color(if fit_active { paper } else { dim })
                        .when(!cropping && !failed, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(if fit_active { paper } else { label }))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.set_zoom_fit(true, cx);
                                }))
                        })
                        .child("Fit"),
                )
                .child(
                    div()
                        .id("status-zoom-100")
                        .tooltip(crate::tooltip::tip(
                            if cfg!(target_os = "macos") {
                                "Actual size (⌘1)"
                            } else {
                                "Actual size (Ctrl+1)"
                            },
                            theme,
                        ))
                        .text_color(if !fit_active { paper } else { dim })
                        .when(!cropping && !failed, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(if !fit_active { paper } else { label }))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.set_zoom_fit(false, cx);
                                }))
                        })
                        .child("100%"),
                ),
        )
}

/// Upward-opening `Original ⌄` menu anchored over the bottom-right of the
/// canvas column directly above the status bar (`StatusLine.swift:320-362`, `L27`).
pub fn render_original_menu(app: &NegsApp, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if !app.original_menu_open || app.model.source.is_none() || app.model.comparison().is_some() {
        return None;
    }
    let theme = &app.theme;
    let mode = app.model.original_mode();
    let reference = app.model.original_reference();
    let has_jpeg = app.model.camera_jpeg().is_some();
    let (item_font, item_size) = theme.serif_spec(14.5, 400.0, true);
    let hover_bg = theme.subtle_hover;

    Some(
        div()
            .id("original-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.original_menu_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("original-menu-popover")
                    .absolute()
                    .bottom(px(STATUS_HEIGHT + 4.0))
                    .right(px(90.0))
                    .min_w(px(190.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(theme.panel)
                    .border_1()
                    .border_color(theme.track)
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(OriginalMode::ALL.into_iter().enumerate().map(|(i, m)| {
                        let checked = m == mode;
                        div()
                            .id(SharedString::from(format!("orig-mode-{i}")))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(12.0))
                            .py(px(5.0))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.original_menu_open = false;
                                this.model.set_original_mode(m);
                                this.after_model_update(cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(12.0))
                                    .font(theme.ui_font(FontWeight::MEDIUM))
                                    .text_size(px(11.0))
                                    .text_color(theme.paper)
                                    .child(if checked { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .font(item_font.clone())
                                    .text_size(px(item_size))
                                    .text_color(if checked { theme.paper } else { theme.label })
                                    .child(m.title()),
                            )
                    }))
                    .when(has_jpeg, |el| {
                        el.child(div().my(px(4.0)).w_full().h(px(1.0)).bg(theme.line))
                            .children(OriginalReference::ALL.into_iter().enumerate().map(
                                |(i, r)| {
                                    let checked = r == reference;
                                    div()
                                        .id(SharedString::from(format!("orig-ref-{i}")))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .px(px(12.0))
                                        .py(px(5.0))
                                        .cursor_pointer()
                                        .hover(move |s| s.bg(hover_bg))
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            this.original_menu_open = false;
                                            this.model.set_original_reference(r);
                                            this.after_model_update(cx);
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .w(px(12.0))
                                                .font(theme.ui_font(FontWeight::MEDIUM))
                                                .text_size(px(11.0))
                                                .text_color(theme.paper)
                                                .child(if checked { "✓" } else { "" }),
                                        )
                                        .child(
                                            div()
                                                .font(item_font.clone())
                                                .text_size(px(item_size))
                                                .text_color(if checked {
                                                    theme.paper
                                                } else {
                                                    theme.label
                                                })
                                                .child(r.title()),
                                        )
                                },
                            ))
                    }),
            )
            .into_any_element(),
    )
}

/// Compare's tools (`CompareTools`): Side by side  Slider [S] | Swap |
/// Fit  100% | Done [Esc].
fn compare_tools(app: &NegsApp, slider: bool, cx: &mut Context<NegsApp>) -> impl IntoElement {
    use negs_app::compare::Layout;
    let theme = &app.theme;
    let fitting = app.model.fit_image;
    let (paper, dim, track) = (theme.paper, theme.dim, theme.track);
    let word = |id: &'static str, text: &'static str, on: bool| {
        div()
            .id(id)
            .cursor_pointer()
            .text_color(if on { paper } else { dim })
            .hover(move |s| s.text_color(paper))
            .child(text)
    };
    let cap = |key: &'static str| {
        div()
            .px(px(4.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .rounded(px(3.0))
            .border_1()
            .border_color(track)
            .text_size(px(10.0))
            .text_color(dim)
            .child(key)
    };
    let rule = || div().w(px(1.0)).h(px(10.0)).bg(track);
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .flex_shrink_0()
        .child(word("cmp-side", "Side by side", !slider).on_click(cx.listener(
            |this, _, _w, cx| {
                this.model.set_compare_layout(Layout::SideBySide);
                this.after_model_update(cx);
                cx.notify();
            },
        )))
        .child(word("cmp-slider", "Slider", slider).on_click(cx.listener(
            |this, _, _w, cx| {
                this.model.set_compare_layout(Layout::Slider);
                this.after_model_update(cx);
                cx.notify();
            },
        )))
        .child(cap("S"))
        .child(rule())
        .child(word("cmp-swap", "Swap", false).on_click(cx.listener(|this, _, _w, cx| {
            this.model.swap_compared();
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(rule())
        .child(word("cmp-fit", "Fit", fitting).on_click(cx.listener(|this, _, _w, cx| {
            this.model.zoom(true);
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(word("cmp-actual", "100%", !fitting).on_click(cx.listener(
            |this, _, _w, cx| {
                this.model.zoom(false);
                this.after_model_update(cx);
                cx.notify();
            },
        )))
        .child(rule())
        .child(word("cmp-done", "Done", false).on_click(cx.listener(|this, _, _w, cx| {
            crate::compare_view::leave(this);
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(cap("Esc"))
}
