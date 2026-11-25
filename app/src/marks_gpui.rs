//! Vector rendering of `negs-marks` hand-drawn grease-pencil marks (stars,
//! pick box, reject cross) using GPUI `PathBuilder` + `window.paint_path`.

use gpui::{
    AnyElement, Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, div, point, prelude::*,
    px,
};
use negs_library::Flag;
use negs_marks::{STAR_GAP, STAR_SIZE};

use crate::theme_gpui::GpuiTheme;

/// Paints a filled `negs_marks` polygon outline translated by `origin` and scaled by `scale`.
fn paint_outline(
    window: &mut Window,
    outline: &[negs_marks::Pt],
    origin: Point<Pixels>,
    scale: f32,
    color: Hsla,
) {
    if outline.len() < 3 {
        return;
    }
    let mut builder = PathBuilder::fill();
    for (i, pt) in outline.iter().enumerate() {
        let p = point(
            origin.x + px(pt.x as f32 * scale),
            origin.y + px(pt.y as f32 * scale),
        );
        if i == 0 {
            builder.move_to(p);
        } else {
            builder.line_to(p);
        }
    }
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Paints a row of 1–5 hand-drawn rating stars seeded by `name`.
pub fn paint_stars(
    window: &mut Window,
    origin: Point<Pixels>,
    name: &str,
    rating: u8,
    scale: f32,
    color: Hsla,
) {
    let count = rating.min(5) as usize;
    let step = (STAR_SIZE + STAR_GAP) as f32 * scale;
    let half = (STAR_SIZE * 0.5) as f32 * scale;
    for i in 0..count {
        let center = point(origin.x + px(i as f32 * step + half), origin.y + px(half));
        for stroke in negs_marks::star_strokes(name, i) {
            paint_outline(window, &stroke, center, scale, color);
        }
    }
}

/// Paints a hand-drawn grease-pencil pick box around a `w × h` icon area.
pub fn paint_pick_box(
    window: &mut Window,
    origin: Point<Pixels>,
    w: f32,
    h: f32,
    name: &str,
    color: Hsla,
) {
    let pick = negs_marks::pick_box_strip(w as f64, h as f64, name);
    let (sin, cos) = pick.tilt.to_radians().sin_cos();
    let (cx, cy) = (w as f64 * 0.5, h as f64 * 0.5);

    for stroke in &pick.strokes {
        if stroke.len() < 3 {
            continue;
        }
        let mut builder = PathBuilder::fill();
        for (i, pt) in stroke.iter().enumerate() {
            let dx = pt.x - cx;
            let dy = pt.y - cy;
            let rx = (dx * cos - dy * sin + cx) as f32;
            let ry = (dx * sin + dy * cos + cy) as f32;
            let p = point(origin.x + px(rx), origin.y + px(ry));
            if i == 0 {
                builder.move_to(p);
            } else {
                builder.line_to(p);
            }
        }
        builder.close();
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

/// Paints a hand-drawn reject cross inside a `w × h` icon area.
pub fn paint_reject_cross(
    window: &mut Window,
    origin: Point<Pixels>,
    w: f32,
    h: f32,
    name: &str,
    color: Hsla,
) {
    for stroke in negs_marks::cross_strokes(w as f64, h as f64, name) {
        paint_outline(
            window,
            &stroke.outline,
            origin,
            1.0,
            color.opacity(stroke.opacity as f32),
        );
    }
}

/// Builds the floating vector-marks badge rendered in the top-right corner of the canvas
/// whenever the open photograph has a star rating or pick/reject flag.
pub fn marks_overlay_badge(
    photo_name: String,
    rating: u8,
    flag: Option<Flag>,
    theme: &GpuiTheme,
) -> Option<AnyElement> {
    if rating == 0 && flag.is_none() {
        return None;
    }

    let star_scale = 1.15f32;
    let stars_w = if rating > 0 {
        let n = rating.min(5) as f32;
        (n * STAR_SIZE as f32 + (n - 1.0) * STAR_GAP as f32) * star_scale
    } else {
        0.0
    };
    let flag_box = 22.0f32;
    let gap = if rating > 0 && flag.is_some() { 10.0 } else { 0.0 };
    let inner_w = stars_w + gap + if flag.is_some() { flag_box } else { 0.0 };
    let inner_h = flag_box.max(STAR_SIZE as f32 * star_scale);

    let accent = theme.accent;
    let paper_on_photo = theme.on_photo;
    let warn = theme.warn;
    let badge_bg = theme.badge_bg;
    let line = theme.line;

    let label = match flag {
        Some(Flag::Pick) => Some("Pick"),
        Some(Flag::Reject) => Some("Reject"),
        None => None,
    };

    let mark_canvas = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let mut cursor_x = bounds.origin.x;
            if let Some(flag) = flag {
                let flag_y = bounds.origin.y + (bounds.size.height - px(flag_box)) * 0.5;
                match flag {
                    Flag::Pick => {
                        // Draw a subtle miniature frame and wrap it in `negs_marks::pick_box_strip`.
                        paint_pick_box(
                            window,
                            point(cursor_x + px(3.0), flag_y + px(4.0)),
                            flag_box - 6.0,
                            flag_box - 8.0,
                            &photo_name,
                            accent,
                        );
                    }
                    Flag::Reject => {
                        paint_reject_cross(
                            window,
                            point(cursor_x - px(4.0), flag_y - px(4.0)),
                            flag_box + 8.0,
                            flag_box + 8.0,
                            &photo_name,
                            warn,
                        );
                    }
                }
                cursor_x += px(flag_box + gap);
            }
            if rating > 0 {
                let star_h = STAR_SIZE as f32 * star_scale;
                let star_y = bounds.origin.y + (bounds.size.height - px(star_h)) * 0.5;
                paint_stars(
                    window,
                    point(cursor_x, star_y),
                    &photo_name,
                    rating,
                    star_scale,
                    accent,
                );
            }
        },
    )
    .w(px(inner_w.ceil()))
    .h(px(inner_h.ceil()));

    Some(
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(badge_bg)
            .border_1()
            .border_color(line.opacity(0.6))
            .child(mark_canvas)
            .when_some(label, |el, text| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(paper_on_photo.opacity(0.85))
                        .child(text),
                )
            })
            .into_any_element(),
    )
}
