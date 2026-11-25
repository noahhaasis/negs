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

    let status_text = if !app.model.status.is_empty() {
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
                        .child(div().text_color(theme.label).child(stock_text))
                        .when(!status_text.is_empty(), |el| {
                            el.child(div().text_color(theme.dim).child("·"))
                                .child(div().text_color(theme.dim).truncate().child(status_text))
                        })
                        .when_some(activity, |el, act| {
                            el.child(div().text_color(theme.accent).child(act))
                        }),
                )
                .child(
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
                ),
        )
}
