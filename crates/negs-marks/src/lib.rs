//! The marks drawn in pencil, as geometry: the stroke under a word, the
//! rings, the hand-drawn stars, the reject's cross and the pick's box. Each
//! is a port of the macOS app's Swift (`Theme.swift` `PencilStroke` and
//! `PencilRing`, `HandStar.swift`, `RejectCross.swift`, `PickBox.swift`),
//! with the same hash and random numbers in the same order, so a photograph
//! gets the same star, cross and box on both. Plain Rust: the Windows app
//! turns these into Direct2D geometry (`ui/d2d.rs`).

mod cross;
mod pencil;
mod pick;
mod star;

pub use cross::{CrossKind, CrossStroke, cross_kind, cross_strokes};
pub use pencil::{PENCIL_HEIGHT, PENCIL_WIDTH, HIGHLIGHTER_WIDTH, RING_LINE_WIDTH, RING_OUTSET, Radii, pencil, ring, ring_tilt};
pub use pick::{BoxKind, PickBox, REACH, box_kind, pick_box, pick_box_grid, pick_box_selection, pick_box_strip};
pub use star::{STAR_GAP, STAR_SIZE, star_path, star_seed, star_strokes};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

pub const fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }
}

/// One step of a path, as Core Graphics and Direct2D both take them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Pt),
    Line(Pt),
    /// Two control points, then the end.
    Cubic(Pt, Pt, Pt),
    Close,
}

/// An outline to fill or a line to stroke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path(pub Vec<Seg>);

impl Path {
    /// A closed polygon through `points`, as Swift's `addLines` + `closeSubpath`.
    pub fn polygon(points: &[Pt]) -> Self {
        let mut segs = Vec::with_capacity(points.len() + 1);
        for (i, &p) in points.iter().enumerate() {
            segs.push(if i == 0 { Seg::Move(p) } else { Seg::Line(p) });
        }
        if !points.is_empty() {
            segs.push(Seg::Close);
        }
        Path(segs)
    }

    /// Whether every figure in it is closed.
    pub fn closed(&self) -> bool {
        matches!(self.0.last(), Some(Seg::Close))
    }

    /// The smallest box round the path itself (Bézier extremes, not control
    /// points), as `boundingBoxOfPath`. `None` when empty.
    pub fn bounds(&self) -> Option<Rect> {
        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut add = |p: Pt| {
            b = Some(b.map_or((p.x, p.y, p.x, p.y), |(x0, y0, x1, y1)| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y))));
        };
        let mut at = pt(0.0, 0.0);
        for seg in &self.0 {
            match *seg {
                Seg::Move(p) | Seg::Line(p) => {
                    add(p);
                    at = p;
                }
                Seg::Cubic(c1, c2, p) => {
                    add(p);
                    for t in cubic_extremes(at, c1, c2, p) {
                        add(cubic_at(at, c1, c2, p, t));
                    }
                    at = p;
                }
                Seg::Close => {}
            }
        }
        b.map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

fn cubic_at(a: Pt, b: Pt, c: Pt, d: Pt, t: f64) -> Pt {
    let u = 1.0 - t;
    let f = |a: f64, b: f64, c: f64, d: f64| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
    pt(f(a.x, b.x, c.x, d.x), f(a.y, b.y, c.y, d.y))
}

/// The parameters in (0, 1) where a cubic turns in x or y.
fn cubic_extremes(a: Pt, b: Pt, c: Pt, d: Pt) -> impl Iterator<Item = f64> {
    let roots = |a: f64, b: f64, c: f64, d: f64| {
        // The derivative's coefficients, over 3.
        let (qa, qb, qc) = (-a + 3.0 * b - 3.0 * c + d, 2.0 * (a - 2.0 * b + c), b - a);
        let mut out = [f64::NAN; 2];
        if qa.abs() < 1e-12 {
            if qb.abs() > 1e-12 {
                out[0] = -qc / qb;
            }
        } else {
            let disc = qb * qb - 4.0 * qa * qc;
            if disc >= 0.0 {
                let s = disc.sqrt();
                out = [(-qb + s) / (2.0 * qa), (-qb - s) / (2.0 * qa)];
            }
        }
        out
    };
    let [x0, x1] = roots(a.x, b.x, c.x, d.x);
    let [y0, y1] = roots(a.y, b.y, c.y, d.y);
    [x0, x1, y0, y1].into_iter().filter(|t| *t > 0.0 && *t < 1.0)
}

// MARK: The page's numbers (RejectCross.swift)

/// FNV-1a over the name's UTF-16 units, as the mockup pages hash it.
pub fn hash(name: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for unit in name.encode_utf16() {
        h = (h ^ unit as u32).wrapping_mul(16_777_619);
    }
    h
}

/// mulberry32, as the pages' `rng`.
#[derive(Clone, Copy, Debug)]
pub struct Random {
    state: u32,
}

impl Random {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }
    pub fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let s = self.state;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
    /// ±`m`, evenly.
    fn jitter(&mut self, m: f64) -> f64 {
        (self.next() - 0.5) * 2.0 * m
    }
}

/// Width along a stroke, from keyframes `(t, width)` eased between.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Profile(pub &'static [(f64, f64)]);

impl Profile {
    pub fn width(&self, t: f64) -> f64 {
        let keys = self.0;
        for i in 1..keys.len() {
            if t <= keys[i].0 {
                let ((t0, w0), (t1, w1)) = (keys[i - 1], keys[i]);
                return lerp(w0, w1, smooth((t - t0) / (t1 - t0)));
            }
        }
        keys[keys.len() - 1].1
    }
}

pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// The page's `x || 1`.
pub(crate) fn non_zero(x: f64) -> f64 {
    if x == 0.0 { 1.0 } else { x }
}

/// A stroke of varying width along any centreline, as a filled outline with
/// round ends: `width(t)` at the share `t` of the way along (PickBox.ribbon).
pub(crate) fn ribbon(p: &[Pt], width: impl Fn(f64) -> f64) -> Vec<Pt> {
    let n = p.len();
    if n < 2 {
        return Vec::new();
    }
    let mut acc = vec![0.0; n];
    for i in 1..n {
        acc[i] = acc[i - 1] + (p[i].x - p[i - 1].x).hypot(p[i].y - p[i - 1].y);
    }
    let total = non_zero(acc[n - 1]);
    let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let (mut tangent, mut half) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let (q, s) = (p[i.saturating_sub(1)], p[(i + 1).min(n - 1)]);
        let (tx, ty) = (s.x - q.x, s.y - q.y);
        let tl = non_zero(tx.hypot(ty));
        let h = width(acc[i] / total) / 2.0;
        tangent.push(pt(tx / tl, ty / tl));
        half.push(h);
        left.push(pt(p[i].x - ty / tl * h, p[i].y + tx / tl * h));
        right.push(pt(p[i].x + ty / tl * h, p[i].y - tx / tl * h));
    }
    // Half a circle round `p`, from its left side to its right, bulging along `t` × `d`.
    let cap = |p: Pt, t: Pt, h: f64, d: f64| {
        (1..6).map(move |k| {
            let a = k as f64 / 6.0 * std::f64::consts::PI;
            let (c, s) = (a.cos(), a.sin());
            pt(p.x + (-t.y * c * d) * h + t.x * s * h * d, p.y + (t.x * c * d) * h + t.y * s * h * d)
        })
    };
    let mut out = Vec::with_capacity(2 * n + 10);
    out.extend_from_slice(&left);
    out.extend(cap(p[n - 1], tangent[n - 1], half[n - 1], 1.0));
    out.extend(right.iter().rev());
    out.extend(cap(p[0], tangent[0], half[0], -1.0));
    out
}

#[cfg(test)]
pub(crate) mod testing {
    use super::Pt;

    /// The first, a third of the way round and the last, against the
    /// page's numbers (written to two places).
    pub fn check_outline(label: &str, points: &[Pt], count: usize, want: [(f64, f64); 3]) {
        assert_eq!(points.len(), count, "{label}: point count");
        let n = points.len();
        for (p, w) in [points[0], points[n / 3], points[n - 1]].iter().zip(want) {
            assert!((p.x - w.0).abs() < 0.006 && (p.y - w.1).abs() < 0.006, "{label}: {p:?} against {w:?}");
        }
    }
}
