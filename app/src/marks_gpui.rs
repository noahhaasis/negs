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

// MARK: Strip and grid marks (Phase 4)

/// Which of `negs_marks`' boxes: a pick in the strip (2 pt out, tight), a
/// pick in the grid (3 pt out), or the grid's selection (8 pt out, 4 pt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxInk {
    Strip,
    Grid,
    Selection,
}

fn polygon(points: &[negs_marks::Pt], at: impl Fn(negs_marks::Pt) -> Point<Pixels>, builder: &mut PathBuilder) {
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            builder.move_to(at(*p));
        } else {
            builder.line_to(at(*p));
        }
    }
    builder.close();
}

/// A `negs_marks::Path` from `origin`: filled, or stroked `stroke` wide.
pub fn paint_marks_path(window: &mut Window, path: &negs_marks::Path, origin: Point<Pixels>, stroke: Option<f32>, color: Hsla) {
    let at = |p: negs_marks::Pt| point(origin.x + px(p.x as f32), origin.y + px(p.y as f32));
    let mut builder = match stroke {
        Some(w) => PathBuilder::stroke(px(w)),
        None => PathBuilder::fill(),
    };
    for seg in &path.0 {
        match *seg {
            negs_marks::Seg::Move(p) => builder.move_to(at(p)),
            negs_marks::Seg::Line(p) => builder.line_to(at(p)),
            negs_marks::Seg::Cubic(a, b, p) => builder.cubic_bezier_to(at(p), at(a), at(b)),
            negs_marks::Seg::Close => builder.close(),
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// A filled dot of radius `r` at `centre`.
pub fn paint_dot(window: &mut Window, centre: Point<Pixels>, r: f32, color: Hsla) {
    let bounds = Bounds { origin: point(centre.x - px(r), centre.y - px(r)), size: gpui::size(px(2.0 * r), px(2.0 * r)) };
    window.paint_quad(gpui::fill(bounds, color).corner_radii(px(r)));
}

/// The pencil stroke stretched along `rect` (`negs_marks::pencil`), `width`
/// wide with round ends; straight in Plain.
pub fn paint_pencil(window: &mut Window, rect: Bounds<Pixels>, plain: bool, width: f32, color: Hsla) {
    let r = negs_marks::Rect::new(0.0, 0.0, f32::from(rect.size.width) as f64, f32::from(rect.size.height) as f64);
    let path = negs_marks::pencil(r, plain);
    paint_marks_path(window, &path, rect.origin, Some(width), color);
    // Round caps: lyon's cap style isn't reachable through GPUI's builder.
    let ends = [path.0.first(), path.0.last()];
    for seg in ends.into_iter().flatten() {
        let p = match *seg {
            negs_marks::Seg::Move(p) | negs_marks::Seg::Line(p) | negs_marks::Seg::Cubic(_, _, p) => p,
            negs_marks::Seg::Close => continue,
        };
        paint_dot(window, point(rect.origin.x + px(p.x as f32), rect.origin.y + px(p.y as f32)), width / 2.0, color);
    }
}

/// The mark's dark halo where there's no blur (`halo_on` on Windows): two
/// soft outlines under it.
fn halo_strokes(window: &mut Window, outline: &[negs_marks::Pt], at: &impl Fn(negs_marks::Pt) -> Point<Pixels>) {
    for (width, alpha) in [(4.0, 0.22), (1.5, 0.6)] {
        let mut builder = PathBuilder::stroke(px(width));
        polygon(outline, at, &mut builder);
        if let Ok(path) = builder.build() {
            window.paint_path(path, gpui::black().opacity(alpha));
        }
    }
}

/// The `index`th hand-drawn star of `name`'s row centred on `centre`, with
/// its halo over a picture.
pub fn paint_star(window: &mut Window, name: &str, index: usize, centre: Point<Pixels>, color: Hsla, halo: bool) {
    let at = |p: negs_marks::Pt| point(centre.x + px(p.x as f32), centre.y + px(p.y as f32));
    let strokes = negs_marks::star_strokes(name, index);
    if halo {
        for stroke in &strokes {
            halo_strokes(window, stroke, &at);
        }
    }
    for stroke in &strokes {
        paint_outline(window, stroke, centre, 1.0, color);
    }
}

/// A reject's cross over the picture at `frame`, with its halo.
pub fn paint_cross(window: &mut Window, frame: Bounds<Pixels>, name: &str, color: Hsla, halo: bool) {
    let (w, h) = (f32::from(frame.size.width) as f64, f32::from(frame.size.height) as f64);
    let strokes = negs_marks::cross_strokes(w, h, name);
    let origin = frame.origin;
    let at = |p: negs_marks::Pt| point(origin.x + px(p.x as f32), origin.y + px(p.y as f32));
    if halo {
        for s in &strokes {
            halo_strokes(window, &s.outline, &at);
        }
    }
    for s in &strokes {
        paint_outline(window, &s.outline, origin, 1.0, color.opacity(color.a * s.opacity as f32));
    }
}

/// A pick's (or the selection's) box round the picture at `frame`.
pub fn paint_box(window: &mut Window, kind: BoxInk, frame: Bounds<Pixels>, name: &str, color: Hsla) {
    let (w, h) = (f32::from(frame.size.width) as f64, f32::from(frame.size.height) as f64);
    let pick = match kind {
        BoxInk::Strip => negs_marks::pick_box_strip(w, h, name),
        BoxInk::Grid => negs_marks::pick_box_grid(w, h, name),
        BoxInk::Selection => negs_marks::pick_box_selection(w, h, name),
    };
    let (sin, cos) = pick.tilt.to_radians().sin_cos();
    let (cx, cy) = (w * 0.5, h * 0.5);
    let origin = frame.origin;
    let at = |p: negs_marks::Pt| {
        let (dx, dy) = (p.x - cx, p.y - cy);
        point(origin.x + px((dx * cos - dy * sin + cx) as f32), origin.y + px((dx * sin + dy * cos + cy) as f32))
    };
    for stroke in &pick.strokes {
        if stroke.len() < 3 {
            continue;
        }
        let mut builder = PathBuilder::fill();
        polygon(stroke, at, &mut builder);
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}


/// A pencil ring round `rect` (`negs_marks::ring`), turned `tilt` degrees
/// about its centre (upright in Plain), `RING_LINE_WIDTH` wide.
pub fn paint_ring(window: &mut Window, rect: Bounds<Pixels>, radii: negs_marks::Radii, tilt: f64, plain: bool, color: Hsla) {
    let (w, h) = (f32::from(rect.size.width) as f64, f32::from(rect.size.height) as f64);
    let path = negs_marks::ring(negs_marks::Rect::new(0.0, 0.0, w, h), radii, plain);
    let (s, c) = negs_marks::ring_tilt(tilt, plain).to_radians().sin_cos();
    let turn = |p: negs_marks::Pt| {
        let (dx, dy) = (p.x - w / 2.0, p.y - h / 2.0);
        negs_marks::Pt { x: w / 2.0 + dx * c - dy * s, y: h / 2.0 + dx * s + dy * c }
    };
    let turned = negs_marks::Path(
        path.0
            .into_iter()
            .map(|seg| match seg {
                negs_marks::Seg::Move(p) => negs_marks::Seg::Move(turn(p)),
                negs_marks::Seg::Line(p) => negs_marks::Seg::Line(turn(p)),
                negs_marks::Seg::Cubic(a, b, p) => negs_marks::Seg::Cubic(turn(a), turn(b), turn(p)),
                negs_marks::Seg::Close => negs_marks::Seg::Close,
            })
            .collect(),
    );
    paint_marks_path(window, &turned, rect.origin, Some(negs_marks::RING_LINE_WIDTH as f32), color);
}
