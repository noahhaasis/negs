//! Bottom status bar: film & paper names, dimensions/facts, zoom %, active
//! monitor ICC profile name, render status/activity, and dismissible error banner.

use gpui::{Context, IntoElement, Styled, div, prelude::*, px};
use negs_library::Flag;

use crate::NegsApp;

pub const STATUS_HEIGHT: f32 = 28.0;

pub fn render_status_bar(app: &NegsApp, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let theme = &app.theme;
    let has_photo = app.model.source.is_some();

    let stock_text = if has_photo {
        let film = app.model.film_name();
        let paper = app.model.paper_name();
        if negs_app::json::boolean(app.model.value("io.scan_film")) || paper.is_empty() {
            film
        } else {
            format!("{film} on {paper}")
        }
    } else {
        "Ready".to_string()
    };

    let activity = if app.model.rendering || app.model.shows_activity {
        Some("Developing…")
    } else if app.model.saving {
        Some("Saving…")
    } else if app.model.exporting {
        Some("Exporting…")
    } else {
        None
    };

    // Comparing: "Comparing 3 and 7 · differ in shutter and aperture".
    let comparing = app.model.comparison().cloned();
    let compare_text = comparing.as_ref().map(|c| {
        let differences = crate::compare_view::differences(app);
        let summary = app.model.compare_summary();
        let mut text = if differences.is_empty() { summary } else { format!("{summary} · differ in {differences}") };
        if !c.against_original() && app.model.shown_items().len() > 2 {
            let place = if app.compare.column { "bottom" } else { "right" };
            text.push_str(&format!("    ← → steps the {place} one"));
        }
        text
    });
    let status_text = if let Some(text) = compare_text.clone() {
        text
    } else if let Some(session) = &app.model.crop_session {
        let size = match app.model.crop_draft_size() {
            Some(Ok(size)) => (size.photo.0 as f64, size.photo.1 as f64),
            _ => (session.shown().width, session.shown().height),
        };
        format!("Cropping · {} · {} × {} px", session.aspect.title(), size.0.round() as i64, size.1.round() as i64)
    } else if !app.model.status.is_empty() {
        app.model.status.clone()
    } else if let Some(size) = has_photo
        .then(|| app.model.photo_size())
        .filter(|s| s.width > 0.0 && s.height > 0.0)
    {
        format!("{} × {}", size.width.round() as u64, size.height.round() as u64)
    } else {
        String::new()
    };

    let (_, rating, flag) = app.current_marks();
    let marks_summary = {
        let mut parts = Vec::new();
        if let Some(f) = flag {
            parts.push(match f {
                Flag::Pick => "Picked".to_string(),
                Flag::Reject => "Rejected".to_string(),
            });
        }
        if rating > 0 {
            parts.push("★".repeat(rating.min(5) as usize));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    };

    let zoom_text = if !has_photo {
        "—".to_string()
    } else if app.canvas.fit {
        format!("Fit · {:.0}%", (app.canvas.zoom * 100.0).round())
    } else {
        format!("{:.0}%", (app.canvas.zoom * 100.0).round())
    };

    let icc_label = app.canvas.display_profile.label.clone();
    let wide_tag = app
        .model
        .image
        .as_ref()
        .is_some_and(|p| p.wide)
        .then_some("Rec. 2020");

    let error_banner = app.model.error.clone();
    let comparing_none = comparing.is_none();
    let tools = comparing.map(|c| compare_tools(app, c.layout == negs_app::compare::Layout::Slider, cx).into_any_element());

    div()
        .flex()
        .flex_col()
        .w_full()
        .when_some(error_banner, |el, message| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .px_3()
                    .py(px(6.0))
                    .bg(theme.panel)
                    .border_t_1()
                    .border_color(theme.warn)
                    .child(
                        div()
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
                            .text_size(px(11.0))
                            .text_color(theme.label)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.model.error = None;
                                cx.notify();
                            }))
                            .child("Dismiss"),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .h(px(STATUS_HEIGHT))
                .px_3()
                .bg(theme.panel)
                .border_t_1()
                .border_color(theme.line)
                .font_family(theme.ui_family.clone())
                .text_size(px(11.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .min_w_0()
                        .when(comparing_none, |el| el.child(div().text_color(theme.label).child(stock_text)))
                        .when(!status_text.is_empty() && !comparing_none, |el| el.child(div().text_color(theme.label).truncate().child(status_text.clone())))
                        .when(!status_text.is_empty() && comparing_none, |el| {
                            el.child(div().text_color(theme.dim).child("·"))
                                .child(div().text_color(theme.dim).truncate().child(status_text))
                        })
                        .when_some(activity, |el, act| {
                            el.child(div().text_color(theme.accent).child(act))
                        }),
                )
                .when_some(tools, |el, tools| el.child(tools))
                .when(comparing_none, |el| el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when_some(marks_summary, |el, marks| {
                            el.child(div().text_color(theme.accent).child(marks))
                                .child(div().text_color(theme.line).child("|"))
                        })
                        .when_some(wide_tag, |el, wide| {
                            el.child(div().text_color(theme.dim).child(wide))
                        })
                        .child(div().text_color(theme.dim).child(format!("ICC: {icc_label}")))
                        .child(div().text_color(theme.line).child("|"))
                        .child(div().text_color(theme.label).child(zoom_text)),
                )),
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
        div().id(id).cursor_pointer().text_color(if on { paper } else { dim }).hover(move |s| s.text_color(paper)).child(text)
    };
    let cap = |key: &'static str| div().px(px(4.0)).h(px(16.0)).flex().items_center().rounded(px(3.0)).border_1().border_color(track).text_size(px(10.0)).text_color(dim).child(key);
    let rule = || div().w(px(1.0)).h(px(10.0)).bg(track);
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(word("cmp-side", "Side by side", !slider).on_click(cx.listener(|this, _, _w, cx| {
            this.model.set_compare_layout(Layout::SideBySide);
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(word("cmp-slider", "Slider", slider).on_click(cx.listener(|this, _, _w, cx| {
            this.model.set_compare_layout(Layout::Slider);
            this.after_model_update(cx);
            cx.notify();
        })))
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
        .child(word("cmp-actual", "100%", !fitting).on_click(cx.listener(|this, _, _w, cx| {
            this.model.zoom(false);
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(rule())
        .child(word("cmp-done", "Done", false).on_click(cx.listener(|this, _, _w, cx| {
            crate::compare_view::leave(this);
            this.after_model_update(cx);
            cx.notify();
        })))
        .child(cap("Esc"))
}
