//! The pencil stroke under a word and the rings (IMPLEMENTATION.md §2.1;
//! `PencilStroke` and `PencilRing` in Theme.swift).

use crate::{Path, Pt, Rect, Seg, pt};

/// The stroke's width: 2.1 pt, not scaled with the stroke.
pub const PENCIL_WIDTH: f64 = 2.1;
/// The box the stroke is stretched to is this tall, under the baseline.
pub const PENCIL_HEIGHT: f64 = 6.0;
/// The highlighter's width (Light table): an 11 pt band, less its round ends.
pub const HIGHLIGHTER_WIDTH: f64 = 8.5;
/// A ring's line.
pub const RING_LINE_WIDTH: f64 = 2.0;
/// How far a ring sits outside the picture it rings (width, height).
pub const RING_OUTSET: (f64, f64) = (7.0, 6.0);

/// The one hand-drawn stroke: the SVG path
/// `M1 3.6 C 22 1.2, 47 5.2, 70 2.6 S 93 2.2, 99 3.4` in a 100 × 6 box,
/// stretched to `rect`. Stroke it `PENCIL_WIDTH` wide with round caps. Plain
/// draws a straight line through the middle instead.
pub fn pencil(rect: Rect, plain: bool) -> Path {
    if plain {
        return Path(vec![Seg::Move(pt(rect.x + 1.0, rect.mid_y())), Seg::Line(pt(rect.max_x() - 1.0, rect.mid_y()))]);
    }
    let (sx, sy) = (rect.w / 100.0, rect.h / 6.0);
    let p = |x: f64, y: f64| pt(rect.x + x * sx, rect.y + y * sy);
    Path(vec![
        Seg::Move(p(1.0, 3.6)),
        Seg::Cubic(p(22.0, 1.2), p(47.0, 5.2), p(70.0, 2.6)),
        // S: the first control point mirrors (47, 5.2) about (70, 2.6).
        Seg::Cubic(p(93.0, 0.0), p(93.0, 2.2), p(99.0, 3.4)),
    ])
}

/// A ring's corner radii, as CSS `border-radius` gives them: horizontal and
/// vertical per corner, clockwise from the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub horizontal: [f64; 4],
    pub vertical: [f64; 4],
    /// Fractions of the width and height (CSS %) rather than points.
    pub relative: bool,
}

impl Radii {
    /// Round a picked frame, tile or thumbnail: `30 40 28 44 / 34 26 40 28`.
    pub const BOX: Radii = Radii { horizontal: [30.0, 40.0, 28.0, 44.0], vertical: [34.0, 26.0, 40.0, 28.0], relative: false };
    /// Round a number: `47% 53% 50% 50% / 56% 44% 58% 42%`.
    pub const NUMBER: Radii = Radii { horizontal: [0.47, 0.53, 0.50, 0.50], vertical: [0.56, 0.44, 0.58, 0.42], relative: true };
}

/// The rotation a ring is drawn at, in degrees (−0.8 round thumbnails, −1
/// round tiles); none in Plain.
pub fn ring_tilt(degrees: f64, plain: bool) -> f64 {
    if plain { 0.0 } else { degrees }
}

/// A loose ellipse drawn in pencil: a rounded rectangle whose corners each
/// have their own elliptical radii, scaled down together when they don't
/// fit, as CSS does. Draw it `RING_LINE_WIDTH` wide, turned by `ring_tilt`
/// round its centre. Plain: a rounded rectangle, or an ellipse round a number.
pub fn ring(rect: Rect, radii: Radii, plain: bool) -> Path {
    if plain {
        let r = if radii.relative { rect.w.min(rect.h) / 2.0 } else { 6.0f64.min(rect.w / 2.0).min(rect.h / 2.0) };
        return corners(rect, [r; 4], [r; 4]);
    }
    let (w, h) = (rect.w, rect.h);
    let (mut rx, mut ry) = (radii.horizontal, radii.vertical);
    if radii.relative {
        rx = rx.map(|v| v * w);
        ry = ry.map(|v| v * h);
    }
    // CSS: scale every radius by the smallest side / (sum of its radii).
    let f = 1f64.min(w / (rx[0] + rx[1])).min(w / (rx[3] + rx[2])).min(h / (ry[0] + ry[3])).min(h / (ry[1] + ry[2]));
    corners(rect, rx.map(|v| v * f), ry.map(|v| v * f))
}

/// A rounded rectangle with elliptical corners, each quarter a cubic with
/// the usual 0.5523 control length, from the top edge clockwise.
fn corners(rect: Rect, rx: [f64; 4], ry: [f64; 4]) -> Path {
    let k = 0.5522847;
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.max_x(), rect.max_y());
    let c = |a: Pt, b: Pt, e: Pt| Seg::Cubic(a, b, e);
    Path(vec![
        Seg::Move(pt(x0 + rx[0], y0)),
        Seg::Line(pt(x1 - rx[1], y0)),
        c(pt(x1 - rx[1] * (1.0 - k), y0), pt(x1, y0 + ry[1] * (1.0 - k)), pt(x1, y0 + ry[1])),
        Seg::Line(pt(x1, y1 - ry[2])),
        c(pt(x1, y1 - ry[2] * (1.0 - k)), pt(x1 - rx[2] * (1.0 - k), y1), pt(x1 - rx[2], y1)),
        Seg::Line(pt(x0 + rx[3], y1)),
        c(pt(x0 + rx[3] * (1.0 - k), y1), pt(x0, y1 - ry[3] * (1.0 - k)), pt(x0, y1 - ry[3])),
        Seg::Line(pt(x0, y0 + ry[0])),
        c(pt(x0, y0 + ry[0] * (1.0 - k)), pt(x0 + rx[0] * (1.0 - k), y0), pt(x0 + rx[0], y0)),
        Seg::Close,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Theme tests on macOS: `testPencilStrokeSpansItsWord`.
    #[test]
    fn pencil_spans_its_word() {
        let rect = Rect::new(10.0, 20.0, 80.0, 6.0);
        let path = pencil(rect, false);
        let b = path.bounds().unwrap();
        assert!((b.x - (10.0 + 0.8)).abs() < 0.01, "{b:?}");
        assert!((b.max_x() - (10.0 + 79.2)).abs() < 0.01, "{b:?}");
        assert!(b.y >= rect.y && b.max_y() <= rect.max_y(), "{b:?}");
        // The SVG's own numbers, stretched.
        assert_eq!(path.0[0], Seg::Move(pt(10.8, 20.0 + 3.6)));
        assert_eq!(path.0[2], Seg::Cubic(pt(10.0 + 93.0 * 0.8, 20.0), pt(10.0 + 93.0 * 0.8, 20.0 + 2.2), pt(10.0 + 99.0 * 0.8, 20.0 + 3.4)));
        assert!(!path.closed());
    }

    #[test]
    fn plain_pencil_is_straight() {
        let path = pencil(Rect::new(0.0, 0.0, 50.0, 6.0), true);
        assert_eq!(path.0, vec![Seg::Move(pt(1.0, 3.0)), Seg::Line(pt(49.0, 3.0))]);
    }

    /// `testRingScalesItsCornersLikeCSS`.
    #[test]
    fn ring_scales_its_corners_like_css() {
        let large = ring(Rect::new(0.0, 0.0, 200.0, 120.0), Radii::BOX, false).bounds().unwrap();
        for (a, b) in [(large.x, 0.0), (large.y, 0.0), (large.w, 200.0), (large.h, 120.0)] {
            assert!((a - b).abs() < 1e-9, "{large:?}");
        }
        let small = ring(Rect::new(0.0, 0.0, 60.0, 40.0), Radii::BOX, false);
        let b = small.bounds().unwrap();
        assert!((b.w - 60.0).abs() < 0.01 && (b.h - 40.0).abs() < 0.01, "{b:?}");
        assert!(small.closed());
        // 40 can't fit 26 + 40 down the right side, the tightest: every
        // radius scales by 40/66.
        let f = 40.0 / 66.0;
        assert_eq!(small.0[0], Seg::Move(pt(30.0 * f, 0.0)));
        // The corner is cut away: the top-left quarter starts below (0.5, 0.5).
        let Seg::Line(left) = small.0[7] else { panic!() };
        assert!((left.y - 34.0 * f).abs() < 1e-9);
    }

    #[test]
    fn ring_round_a_number_uses_shares() {
        let path = ring(Rect::new(0.0, 0.0, 20.0, 20.0), Radii::NUMBER, false);
        // 44% + 58% down the right side don't fit: every radius scales by 1/1.02.
        let Seg::Move(start) = path.0[0] else { panic!() };
        assert!((start.x - 0.47 * 20.0 / 1.02).abs() < 1e-9 && start.y == 0.0, "{start:?}");
        assert_eq!(ring_tilt(-0.8, false), -0.8);
        assert_eq!(ring_tilt(-0.8, true), 0.0);
    }

    #[test]
    fn plain_ring_is_a_rounded_rectangle() {
        let path = ring(Rect::new(0.0, 0.0, 100.0, 60.0), Radii::BOX, true);
        assert_eq!(path.0[0], Seg::Move(pt(6.0, 0.0)));
        let number = ring(Rect::new(0.0, 0.0, 20.0, 16.0), Radii::NUMBER, true);
        assert_eq!(number.0[0], Seg::Move(pt(8.0, 0.0)));
    }
}
