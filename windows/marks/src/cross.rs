//! A reject's cross (round 14, `r14-reject.html`; RejectCross.swift): one of
//! two hand-drawn crosses, chosen from the file name, with its own bow,
//! wobble, weight and retrace.
//!
//! - **Loose:** drawn fast. Each stroke starts short of one corner, runs past
//!   the other and ends in a flick; the second is a little lighter.
//! - **Gone over twice:** a thinner line traced again just beside itself.

use crate::{Profile, Pt, Random, hash, lerp, pt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossKind {
    Loose,
    Twice,
}

/// One stroke: a filled outline, drawn in `paper` at `opacity`.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossStroke {
    pub outline: Vec<Pt>,
    pub opacity: f64,
}

/// One way of drawing the cross. Lengths are in points.
#[derive(Clone, Copy)]
struct Style {
    inset: f64,
    jitter: f64,
    /// How far short of its first corner a stroke starts (+), as a range.
    start: Option<(f64, f64)>,
    /// How far short of its last corner a stroke ends: − runs past it.
    end: Option<(f64, f64)>,
    /// The bow at the middle, as a share of the stroke's length.
    bow: f64,
    /// The chance a stroke bows the other way.
    bow_flip: f64,
    wobble: f64,
    passes: usize,
    retrace: f64,
    pass_opacity: f64,
    profile: Profile,
    scale: f64,
    opacity: f64,
    /// The second stroke's width scale and opacity, if they differ.
    second: Option<(f64, f64)>,
}

const LOOSE: Style = Style {
    inset: 12.0,
    jitter: 3.0,
    start: Some((10.0, 24.0)),
    end: Some((-14.0, -5.0)),
    bow: 0.0,
    bow_flip: 0.5,
    wobble: 0.0,
    passes: 1,
    retrace: 0.0,
    pass_opacity: 1.0,
    profile: Profile(&[(0.0, 2.2), (0.1, 3.0), (0.7, 2.4), (1.0, 0.2)]),
    scale: 1.0,
    opacity: 0.9,
    second: None,
};

const TWICE: Style = Style {
    inset: 8.0,
    jitter: 2.0,
    start: None,
    end: None,
    bow: 0.02,
    bow_flip: 0.3,
    wobble: 0.45,
    passes: 2,
    retrace: 0.0,
    pass_opacity: 1.0,
    profile: Profile(&[(0.0, 0.7), (0.12, 1.7), (0.8, 1.5), (1.0, 0.4)]),
    scale: 1.0,
    opacity: 0.95,
    second: None,
};

/// The style a name draws, with its own spread (the page's `mixed`).
fn style(random: &mut Random) -> (CrossKind, Style) {
    if random.next() < 0.5 {
        let mut style = LOOSE;
        style.scale = lerp(0.85, 1.15, random.next());
        style.bow = lerp(0.025, 0.05, random.next());
        style.wobble = lerp(0.5, 1.0, random.next());
        style.second = Some((lerp(0.8, 1.0, random.next()), 0.8));
        return (CrossKind::Loose, style);
    }
    let mut style = TWICE;
    style.scale = lerp(0.85, 1.2, random.next());
    style.retrace = lerp(1.2, 2.3, random.next());
    style.pass_opacity = lerp(0.55, 0.8, random.next());
    (CrossKind::Twice, style)
}

pub fn cross_kind(name: &str) -> CrossKind {
    style(&mut Random::new(hash(name))).0
}

/// The strokes for a picture of this size: top left to bottom right, then
/// top right to bottom left, as a hand draws an X.
pub fn cross_strokes(w: f64, h: f64, name: &str) -> Vec<CrossStroke> {
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let mut random = Random::new(hash(name));
    let style = style(&mut random).1;
    let m = style.inset;
    let corners = [(pt(m, m), pt(w - m, h - m)), (pt(w - m, m), pt(m, h - m))];
    let mut strokes = Vec::new();
    for (k, (a, b)) in corners.into_iter().enumerate() {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length = dx.hypot(dy);
        if length <= 0.0 {
            continue;
        }
        let (ux, uy) = (dx / length, dy / length);
        let s = style.start.map_or(0.0, |r| lerp(r.0, r.1, random.next()));
        let e = style.end.map_or(0.0, |r| lerp(r.0, r.1, random.next()));
        let ax = a.x + ux * s + random.jitter(style.jitter);
        let ay = a.y + uy * s + random.jitter(style.jitter);
        let bx = b.x - ux * e + random.jitter(style.jitter);
        let by = b.y - uy * e + random.jitter(style.jitter);
        let (mut scale, mut opacity) = (style.scale, style.opacity);
        if k == 1 {
            if let Some(second) = style.second {
                (scale, opacity) = second;
            }
        }
        for pass in 0..style.passes {
            let off = if pass > 0 { style.retrace * if random.next() < 0.5 { -1.0 } else { 1.0 } } else { 0.0 };
            let (nx, ny) = (-uy * off, ux * off);
            let from = pt(ax + nx + random.jitter(style.jitter) * 0.4, ay + ny);
            let to = pt(bx + nx, by + ny + random.jitter(style.jitter) * 0.4);
            let outline = outline(from, to, &style, scale, &mut random);
            strokes.push(CrossStroke { outline, opacity: opacity * if pass > 0 { style.pass_opacity } else { 1.0 } });
        }
    }
    strokes
}

/// One stroke as a filled outline: a bowed, slightly wobbling line from `a`
/// to `b`, its width following the profile, with round ends.
fn outline(a: Pt, b: Pt, style: &Style, scale: f64, random: &mut Random) -> Vec<Pt> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    let (nx, ny) = (-dy / length, dx / length);
    let bow_size = style.bow * length * (0.6 + 0.8 * random.next());
    let bow = bow_size * if random.next() < style.bow_flip { -1.0 } else { 1.0 };
    let (cx, cy) = ((a.x + b.x) / 2.0 + nx * bow * 2.0, (a.y + b.y) / 2.0 + ny * bow * 2.0);
    let amp = style.wobble * (0.5 + random.next());
    let frequency = 1.2 + 1.6 * random.next();
    let phase = random.next() * 6.28;
    const N: usize = 56;
    let mut centre = Vec::with_capacity(N + 1);
    for i in 0..=N {
        let t = i as f64 / N as f64;
        let u = 1.0 - t;
        let wave = (std::f64::consts::PI * t).sin() * amp * (6.283 * frequency * t + phase).sin();
        let x = u * u * a.x + 2.0 * u * t * cx + t * t * b.x + nx * wave;
        let y = u * u * a.y + 2.0 * u * t * cy + t * t * b.y + ny * wave;
        centre.push((pt(x, y), style.profile.width(t) * scale));
    }
    let (mut left, mut right) = (Vec::with_capacity(N + 1), Vec::with_capacity(N + 1));
    for i in 0..=N {
        let (p, width) = centre[i];
        let (q, s) = (centre[i.saturating_sub(1)].0, centre[(i + 1).min(N)].0);
        let (tx, ty) = (s.x - q.x, s.y - q.y);
        let tl = if tx.hypot(ty) == 0.0 { 1.0 } else { tx.hypot(ty) };
        let half = width / 2.0;
        left.push(pt(p.x - ty / tl * half, p.y + tx / tl * half));
        right.push(pt(p.x + ty / tl * half, p.y - tx / tl * half));
    }
    // Half a circle round `p`, from one side of the stroke to the other.
    let cap = |p: (Pt, f64), from: Pt, to: Pt| {
        let a0 = (from.y - p.0.y).atan2(from.x - p.0.x);
        let half = p.1 / 2.0;
        let mut a1 = (to.y - p.0.y).atan2(to.x - p.0.x);
        while a1 < a0 {
            a1 += 6.283;
        }
        if a1 - a0 > 3.3 {
            a1 -= 6.283;
        }
        (1..6).map(move |k| {
            let angle = lerp(a0, a1, k as f64 / 6.0);
            pt(p.0.x + angle.cos() * half, p.0.y + angle.sin() * half)
        })
    };
    let mut points = Vec::with_capacity(2 * N + 12);
    points.extend_from_slice(&left);
    points.extend(cap(centre[N], left[N], right[N]));
    points.extend(right.iter().rev());
    points.extend(cap(centre[0], right[0], left[0]));
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Path;

    type Photo = (&'static str, u32, [f64; 2], &'static [(f64, f64)], &'static [f64]);

    /// From RejectCrossTests.swift: the page's script on `mixed(200, 140,
    /// name)`: the hash, the first two random numbers, where each stroke
    /// starts and each stroke's opacity.
    const FROM_THE_PAGE: &[Photo] = &[
        ("DSC_6435.NEF", 2_972_515_683, [0.212829355, 0.331041164], &[(24.84, 19.71), (173.45, 18.11)], &[0.9, 0.8]),
        ("DSC_5784.NEF", 1_338_732_189, [0.090616154, 0.541751701], &[(22.24, 24.10), (172.57, 17.38)], &[0.9, 0.8]),
        ("IMG_0001.JPG", 2_515_027_847, [0.705942326, 0.111862622], &[(5.90, 10.17), (5.79, 11.72), (191.77, 5.74), (191.37, 4.19)], &[0.95, 0.69, 0.95, 0.69]),
        ("DSC_8200.NEF", 2_362_156_491, [0.952154420, 0.436344665], &[(6.83, 7.86), (5.85, 9.28), (192.66, 8.28), (192.06, 6.88)], &[0.95, 0.66, 0.95, 0.66]),
    ];

    #[test]
    fn matches_the_drawing() {
        for (name, seed, first, starts, opacities) in FROM_THE_PAGE {
            assert_eq!(hash(name), *seed, "{name}");
            let mut random = Random::new(*seed);
            for value in first {
                assert!((random.next() - value).abs() < 1e-9, "{name}");
            }
            let strokes = cross_strokes(200.0, 140.0, name);
            assert_eq!(strokes.len(), starts.len(), "{name}");
            for (stroke, (x, y)) in strokes.iter().zip(starts.iter()) {
                let p = stroke.outline[0];
                assert!((p.x - x).abs() < 0.006 && (p.y - y).abs() < 0.006, "{name}: {p:?} against ({x}, {y})");
            }
            for (stroke, opacity) in strokes.iter().zip(opacities.iter()) {
                assert!((stroke.opacity - opacity).abs() < 0.006, "{name}");
            }
        }
    }

    #[test]
    fn the_same_photograph_always_gets_the_same_cross() {
        let a = cross_strokes(214.0, 140.0, "DSC_6435.NEF");
        assert_eq!(a, cross_strokes(214.0, 140.0, "DSC_6435.NEF"));
        assert_ne!(a, cross_strokes(214.0, 140.0, "DSC_6436.NEF"));
    }

    #[test]
    fn both_crosses_turn_up() {
        let loose = (0..40).filter(|n| cross_kind(&format!("DSC_{}.NEF", 7000 + n)) == CrossKind::Loose).count();
        assert!((8..=32).contains(&loose), "{loose} of 40 loose");
        assert_eq!(cross_strokes(200.0, 140.0, "DSC_6435.NEF").len(), 2);
        assert_eq!(cross_strokes(200.0, 140.0, "IMG_0001.JPG").len(), 4);
        assert_eq!(cross_kind("DSC_6435.NEF"), CrossKind::Loose);
        assert_eq!(cross_kind("IMG_0001.JPG"), CrossKind::Twice);
    }

    #[test]
    fn draws_inside_the_smallest_frames() {
        for n in 0..40 {
            let name = format!("DSC_{}.NEF", 7000 + n);
            for (w, h) in [(86.0, 140.0), (264.0, 176.0), (0.0, 0.0)] {
                for stroke in cross_strokes(w, h, &name) {
                    let b = Path::polygon(&stroke.outline).bounds().unwrap();
                    assert!(b.x.is_finite() && b.y.is_finite(), "{name}");
                    // A loose stroke may run a few points past its corner.
                    assert!(b.x + 6.0 >= 0.0 && b.y + 6.0 >= 0.0 && b.max_x() - 6.0 <= w && b.max_y() - 6.0 <= h, "{name} {w}×{h} {b:?}");
                }
            }
        }
    }
}
