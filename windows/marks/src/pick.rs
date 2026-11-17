//! A box drawn in grease pencil round a picture (rounds 20 and 21,
//! `r20-box.html`, B; PickBox.swift): a pick's box, and in the grid
//! selection's second box over it. Each name draws one of three:
//!
//! - **Four:** four separate sides, crossing at the corners.
//! - **Loop:** one stroke round, its end running past its start.
//! - **Two:** down and across, then across and up, meeting at the corners.

use crate::{Profile, Pt, Random, hash, lerp, non_zero, pt, ribbon};

/// How far outside the picture a box may draw: the yellow box's 8 pt, a
/// corner's jitter, an overshoot of up to 12 pt, the stroke's width and
/// wobble, and the tilt on a wide picture.
pub const REACH: f64 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Four,
    Loop,
    Two,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickBox {
    pub kind: BoxKind,
    /// Each stroke as a filled outline, before the tilt, in the picture's
    /// coordinates.
    pub strokes: Vec<Vec<Pt>>,
    /// The tilt round the picture's centre, in degrees, clockwise.
    pub tilt: f64,
}

const PROFILE: Profile = Profile(&[(0.0, 1.4), (0.05, 2.9), (0.85, 2.6), (1.0, 0.9)]);

/// A pick in the grid: 3 pt out.
pub fn pick_box_grid(w: f64, h: f64, name: &str) -> PickBox {
    pick_box(w, h, 3.0, name, false, 1.0)
}

/// A pick in the strip: 2 pt out, the overshoots kept short.
pub fn pick_box_strip(w: f64, h: f64, name: &str) -> PickBox {
    pick_box(w, h, 2.0, name, true, 1.0)
}

/// Selection in the grid: a second box, 8 pt out, in a 4 pt line (the
/// page's "Yellow line" over its 2.6 pt stroke).
pub fn pick_box_selection(w: f64, h: f64, name: &str) -> PickBox {
    pick_box(w, h, 8.0, &format!("{name}\u{00B7}second"), false, 4.0 / 2.6)
}

fn seed(name: &str) -> u32 {
    hash(&format!("{name}\u{00B7}box"))
}

fn kind_of(k: f64) -> BoxKind {
    if k < 0.36 {
        BoxKind::Four
    } else if k < 0.7 {
        BoxKind::Loop
    } else {
        BoxKind::Two
    }
}

pub fn box_kind(name: &str) -> BoxKind {
    kind_of(Random::new(seed(name)).next())
}

/// The box round a `w` × `h` picture, `o` outside it. `tight` keeps the
/// corners and overshoots small (the strip); `weight` scales the stroke.
pub fn pick_box(w: f64, h: f64, o: f64, name: &str, tight: bool, weight: f64) -> PickBox {
    let mut random = Random::new(seed(name));
    let kind = kind_of(random.next());
    let (over, cj) = if tight { (4.0, 1.5) } else { (12.0, 2.6) };
    let mut c = [Pt::default(); 4];
    for (i, (x, y)) in [(-o, -o), (w + o, -o), (w + o, h + o), (-o, h + o)].into_iter().enumerate() {
        let jx = random.jitter(cj);
        let jy = random.jitter(cj);
        c[i] = pt(x + jx, y + jy);
    }
    // `p` moved `d` further from `q`.
    let ext = |p: Pt, q: Pt, d: f64| {
        let (dx, dy) = (p.x - q.x, p.y - q.y);
        let l = non_zero(dx.hypot(dy));
        pt(p.x + dx / l * d, p.y + dy / l * d)
    };
    let at = |a: Pt, b: Pt, t: f64| pt(lerp(a.x, b.x, t), lerp(a.y, b.y, t));

    let ways: Vec<Vec<Pt>> = match kind {
        BoxKind::Four => (0..4)
            .map(|i| {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                let from = ext(a, b, lerp(-2.0, over * 0.6, random.next()));
                let to = ext(b, a, lerp(-1.0, over, random.next()));
                vec![from, to]
            })
            .collect(),
        BoxKind::Loop => {
            let t0 = lerp(0.1, 0.4, random.next());
            let mut end = at(c[0], c[1], 0.95f64.min(t0 + lerp(0.1, 0.3, random.next())));
            end.y += random.jitter(2.5);
            vec![vec![at(c[0], c[1], t0), c[1], c[2], c[3], c[0], end]]
        }
        BoxKind::Two => {
            let a0 = ext(c[3], c[0], lerp(-2.0, over * 0.5, random.next()));
            let a2 = ext(c[1], c[0], lerp(0.0, over, random.next()));
            let b0 = ext(c[1], c[2], lerp(-4.0, 2.0, random.next()));
            let b2 = ext(c[3], c[2], lerp(0.0, over, random.next()));
            vec![vec![a0, c[0], a2], vec![b0, c[2], b2]]
        }
    };
    let scale = lerp(0.85, 1.2, random.next()) * weight;
    let tilt = random.jitter(0.6);
    let wobble = lerp(0.3, 0.9, random.next());
    let period = lerp(70.0, 150.0, random.next());
    let phase = random.next() * 6.283;

    let mut strokes = Vec::with_capacity(ways.len());
    for way in ways {
        // Each leg in steps of about 3 pt, bowed a little.
        let mut points = Vec::new();
        for i in 0..way.len() - 1 {
            let (a, b) = (way[i], way[i + 1]);
            let length = non_zero((b.x - a.x).hypot(b.y - a.y));
            let n = 2usize.max((length / 3.0).ceil() as usize);
            let (nx, ny) = (-(b.y - a.y) / length, (b.x - a.x) / length);
            let bow = random.jitter(2.2f64.min(length * 0.012));
            for s in 0..n {
                let t = s as f64 / n as f64;
                let q = (std::f64::consts::PI * t).sin() * bow;
                points.push(pt(lerp(a.x, b.x, t) + nx * q, lerp(a.y, b.y, t) + ny * q));
            }
        }
        points.push(way[way.len() - 1]);
        let points = smooth_points(&points, 2);
        // A slow wobble across the line, by the distance along it.
        let mut along = 0.0;
        let mut wobbled = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            let p = points[i];
            if i > 0 {
                along += (p.x - points[i - 1].x).hypot(p.y - points[i - 1].y);
            }
            let (q, u) = (points[i.saturating_sub(1)], points[(i + 1).min(points.len() - 1)]);
            let (tx, ty) = (u.x - q.x, u.y - q.y);
            let tl = non_zero(tx.hypot(ty));
            let d = wobble * (6.283 * along / period + phase).sin();
            wobbled.push(pt(p.x - ty / tl * d, p.y + tx / tl * d));
        }
        strokes.push(ribbon(&wobbled, |t| PROFILE.width(t) * scale));
    }
    PickBox { kind, strokes, tilt }
}

/// Each point but the ends moved to the mean of the `k` either side of it.
fn smooth_points(p: &[Pt], k: usize) -> Vec<Pt> {
    (0..p.len())
        .map(|i| {
            if i == 0 || i == p.len() - 1 {
                return p[i];
            }
            let (mut x, mut y, mut count) = (0.0, 0.0, 0.0);
            for q in &p[i.saturating_sub(k)..=(i + k).min(p.len() - 1)] {
                x += q.x;
                y += q.y;
                count += 1.0;
            }
            pt(x / count, y / count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check_outline;
    use crate::{Path, Rect};

    struct Photo {
        name: &'static str,
        size: (f64, f64),
        outset: f64,
        tight: bool,
        weight: f64,
        hash: u32,
        kind: BoxKind,
        tilt: f64,
        strokes: &'static [(usize, [(f64, f64); 3])],
    }

    /// From PickBoxTests.swift: the page's `boxPaths` on the same names.
    const FROM_THE_PAGE: &[Photo] = &[
        Photo { name: "DSC_6435.NEF", size: (264.0, 176.0), outset: 3.0, tight: false, weight: 1.0, hash: 2_056_138_581, kind: BoxKind::Four, tilt: -0.59, strokes: &[
            (206, [(-8.41, -0.79), (193.73, -2.11), (-8.74, -0.88)]),
            (138, [(268.14, -1.58), (264.9, 136.16), (268.24, -1.9)]),
            (202, [(269.28, 181.45), (69.25, 177.21), (269.6, 181.55)]),
            (136, [(-4.62, 177.63), (-4.02, 43.08), (-4.71, 177.95)]),
        ] },
        Photo { name: "DSC_5784.NEF", size: (266.0, 176.0), outset: 3.0, tight: false, weight: 1.0, hash: 1_189_942_715, kind: BoxKind::Loop, tilt: 0.0, strokes: &[
            (668, [(70.5, -2.14), (-1.59, 174.03), (70.19, -2.23)]),
        ] },
        Photo { name: "DSC_6448.NEF", size: (117.0, 176.0), outset: 3.0, tight: false, weight: 1.0, hash: 1_833_535_613, kind: BoxKind::Loop, tilt: 0.56, strokes: &[
            (452, [(38.51, -3.2), (0.39, 124.35), (38.13, -3.32)]),
        ] },
        Photo { name: "DSC_7006.NEF", size: (264.0, 176.0), outset: 3.0, tight: false, weight: 1.0, hash: 1_275_140_550, kind: BoxKind::Two, tilt: -0.13, strokes: &[
            (318, [(-4.46, 182.24), (130.35, -0.59), (-4.55, 182.61)]),
            (316, [(267.83, 1.74), (127.29, 176.95), (267.92, 1.36)]),
        ] },
        // The strip's: 2 pt out, tight.
        Photo { name: "IMG_0001.JPG", size: (210.0, 140.0), outset: 2.0, tight: true, weight: 1.0, hash: 1_738_235_033, kind: BoxKind::Four, tilt: -0.15, strokes: &[
            (158, [(-1.63, -0.65), (152.78, 1.22), (-2.01, -0.75)]),
            (108, [(213.05, 1.02), (210.9, 108.06), (213.16, 0.64)]),
            (156, [(210.36, 140.7), (56.28, 139.43), (210.75, 140.8)]),
            (108, [(-3.16, 140.76), (-0.51, 34.3), (-3.27, 141.14)]),
        ] },
        Photo { name: "DSC_7007.NEF", size: (100.0, 140.0), outset: 2.0, tight: true, weight: 1.0, hash: 969_449_513, kind: BoxKind::Two, tilt: 0.27, strokes: &[
            (182, [(-0.93, 144.85), (29.92, 0.0), (-1.03, 145.27)]),
            (180, [(100.66, -0.05), (65.64, 141.42), (100.79, -0.46)]),
        ] },
        // Selection's: name + "·second", 8 pt out, 4 / 2.6 the weight.
        Photo { name: "DSC_6435.NEF\u{00B7}second", size: (264.0, 176.0), outset: 8.0, tight: false, weight: 4.0 / 2.6, hash: 4_080_007_402, kind: BoxKind::Loop, tilt: 0.08, strokes: &[
            (688, [(96.6, -7.38), (-4.56, 138.58), (95.97, -7.58)]),
        ] },
        Photo { name: "DSC_5784.NEF\u{00B7}second", size: (266.0, 176.0), outset: 8.0, tight: false, weight: 4.0 / 2.6, hash: 2_258_131_768, kind: BoxKind::Four, tilt: -0.43, strokes: &[
            (206, [(-13.07, -6.6), (189.88, -7.2), (-13.69, -6.75)]),
            (144, [(271.51, -10.8), (271.41, 131.71), (271.65, -11.43)]),
            (208, [(278.52, 183.19), (71.65, 183.21), (279.15, 183.33)]),
            (144, [(-8.15, 182.6), (-7.56, 40.08), (-8.31, 183.22)]),
        ] },
    ];

    #[test]
    fn matches_the_drawing() {
        for photo in FROM_THE_PAGE {
            assert_eq!(seed(photo.name), photo.hash, "{}", photo.name);
            assert_eq!(box_kind(photo.name), photo.kind, "{}", photo.name);
            let b = pick_box(photo.size.0, photo.size.1, photo.outset, photo.name, photo.tight, photo.weight);
            assert_eq!(b.kind, photo.kind, "{}", photo.name);
            assert!((b.tilt - photo.tilt).abs() < 0.006, "{}: tilt {}", photo.name, b.tilt);
            assert_eq!(b.strokes.len(), photo.strokes.len(), "{}", photo.name);
            for (points, (count, want)) in b.strokes.iter().zip(photo.strokes) {
                check_outline(photo.name, points, *count, *want);
            }
        }
    }

    #[test]
    fn grid_strip_and_selection_are_the_pages_settings() {
        let (w, h) = (264.0, 176.0);
        assert_eq!(pick_box_grid(w, h, "DSC_6435.NEF"), pick_box(w, h, 3.0, "DSC_6435.NEF", false, 1.0));
        assert_eq!(pick_box_strip(w, h, "DSC_6435.NEF"), pick_box(w, h, 2.0, "DSC_6435.NEF", true, 1.0));
        assert_eq!(pick_box_selection(w, h, "DSC_6435.NEF"), pick_box(w, h, 8.0, "DSC_6435.NEF\u{00B7}second", false, 4.0 / 2.6));
        assert_ne!(pick_box_grid(w, h, "DSC_6435.NEF"), pick_box_grid(w, h, "DSC_6436.NEF"));
        assert_ne!(pick_box_grid(w, h, "DSC_6435.NEF").strokes, pick_box_selection(w, h, "DSC_6435.NEF").strokes);
    }

    #[test]
    fn all_three_boxes_turn_up() {
        let names: Vec<String> = (0..40).map(|n| format!("DSC_{}.NEF", 7000 + n)).collect();
        let kinds: Vec<BoxKind> = names.iter().map(|n| box_kind(n)).collect();
        // The page draws 16 four, 10 loop and 14 two of these.
        assert_eq!(kinds.iter().filter(|k| **k == BoxKind::Four).count(), 16);
        assert_eq!(kinds.iter().filter(|k| **k == BoxKind::Loop).count(), 10);
        assert_eq!(kinds.iter().filter(|k| **k == BoxKind::Two).count(), 14);
        for (name, kind) in names.iter().zip(kinds) {
            let want = match kind {
                BoxKind::Four => 4,
                BoxKind::Loop => 1,
                BoxKind::Two => 2,
            };
            assert_eq!(pick_box_grid(200.0, 140.0, name).strokes.len(), want, "{name}");
        }
    }

    /// The tilted box's bounds, as `Box.path(in:)` turns it.
    fn tilted_bounds(b: &PickBox, w: f64, h: f64) -> Rect {
        let (s, c) = (b.tilt.to_radians().sin(), b.tilt.to_radians().cos());
        let points: Vec<Pt> = b
            .strokes
            .iter()
            .flatten()
            .map(|p| {
                let (x, y) = (p.x - w / 2.0, p.y - h / 2.0);
                pt(x * c - y * s + w / 2.0, x * s + y * c + h / 2.0)
            })
            .collect();
        Path::polygon(&points).bounds().unwrap()
    }

    #[test]
    fn stays_within_its_reach() {
        for n in 0..40 {
            let name = format!("DSC_{}.NEF", 7000 + n);
            for (w, h) in [(86.0, 140.0), (264.0, 176.0), (540.0, 360.0)] {
                for b in [pick_box_grid(w, h, &name), pick_box_strip(w, h, &name), pick_box_selection(w, h, &name)] {
                    let r = tilted_bounds(&b, w, h);
                    // Round the picture, never into it.
                    assert!(r.x <= 4.0 && r.y <= 4.0 && r.max_x() >= w - 4.0 && r.max_y() >= h - 4.0, "{name} {w}×{h} {r:?}");
                    // Leave room for the halo's blur.
                    let m = -REACH + 4.0;
                    assert!(r.x >= m && r.y >= m && r.max_x() <= w - m && r.max_y() <= h - m, "{name} {w}×{h} {r:?}");
                }
            }
        }
        assert!(pick_box_grid(0.0, 0.0, "DSC_6435.NEF").strokes.iter().flatten().all(|p| p.x.is_finite()));
    }
}
