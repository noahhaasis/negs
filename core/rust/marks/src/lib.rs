//! The marks drawn in pencil, from the shared Odin core (core/src/marks):
//! the stroke under a word, the rings, the hand-drawn stars, the reject's
//! cross and the pick's box. The API is windows/marks's (`negs-marks`), so a
//! front end moves onto the core with a Cargo rename; the geometry itself is
//! computed in the core, bit for bit as the Rust it replaces
//! (core/diff's marks differential test).

use negs_core_sys::marks as ffi;
use std::os::raw::c_char;

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
        let segs: Vec<ffi::negs_seg_t> = self.0.iter().map(to_seg).collect();
        let mut out = ffi::negs_rect_t::default();
        let ok = unsafe { ffi::negs_marks_path_bounds(segs.as_ptr(), segs.len(), &mut out) };
        (ok != 0).then(|| Rect::new(out.x, out.y, out.w, out.h))
    }
}

fn c_pt(p: Pt) -> ffi::negs_pt_t {
    ffi::negs_pt_t { x: p.x, y: p.y }
}

fn from_c(p: ffi::negs_pt_t) -> Pt {
    pt(p.x, p.y)
}

fn c_rect(r: Rect) -> ffi::negs_rect_t {
    ffi::negs_rect_t { x: r.x, y: r.y, w: r.w, h: r.h }
}

fn to_seg(seg: &Seg) -> ffi::negs_seg_t {
    let zero = ffi::negs_pt_t::default();
    match *seg {
        Seg::Move(p) => ffi::negs_seg_t { kind: ffi::NEGS_SEG_MOVE, reserved: 0, p: [c_pt(p), zero, zero] },
        Seg::Line(p) => ffi::negs_seg_t { kind: ffi::NEGS_SEG_LINE, reserved: 0, p: [c_pt(p), zero, zero] },
        Seg::Cubic(a, b, c) => ffi::negs_seg_t { kind: ffi::NEGS_SEG_CUBIC, reserved: 0, p: [c_pt(a), c_pt(b), c_pt(c)] },
        Seg::Close => ffi::negs_seg_t { kind: ffi::NEGS_SEG_CLOSE, reserved: 0, p: [zero; 3] },
    }
}

fn from_segs(segs: &[ffi::negs_seg_t]) -> Path {
    Path(
        segs.iter()
            .map(|s| match s.kind {
                ffi::NEGS_SEG_MOVE => Seg::Move(from_c(s.p[0])),
                ffi::NEGS_SEG_LINE => Seg::Line(from_c(s.p[0])),
                ffi::NEGS_SEG_CUBIC => Seg::Cubic(from_c(s.p[0]), from_c(s.p[1]), from_c(s.p[2])),
                _ => Seg::Close,
            })
            .collect(),
    )
}

fn name_ptr(name: &str) -> (*const c_char, usize) {
    (name.as_ptr().cast(), name.len())
}

// MARK: The page's numbers

/// FNV-1a over the name's UTF-16 units, as the mockup pages hash it.
pub fn hash(name: &str) -> u32 {
    let (p, n) = name_ptr(name);
    unsafe { ffi::negs_marks_hash(p, n) }
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
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        unsafe { ffi::negs_marks_random_next(&mut self.state) }
    }
}

// MARK: Pencil and rings

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
/// The box each star is drawn in (round (0, 0)), and the gap between two.
pub const STAR_SIZE: f64 = 12.0;
pub const STAR_GAP: f64 = 2.5;
/// How far outside the picture a box may draw.
pub const REACH: f64 = 32.0;

/// The hand-drawn stroke under a word, stretched to `rect`; Plain: a
/// straight line through the middle.
pub fn pencil(rect: Rect, plain: bool) -> Path {
    let mut segs = [ffi::negs_seg_t::default(); 3];
    let n = unsafe { ffi::negs_marks_pencil(c_rect(rect), plain as u8, segs.as_mut_ptr(), segs.len()) };
    from_segs(&segs[..n.min(segs.len())])
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

/// The rotation a ring is drawn at, in degrees; none in Plain.
pub fn ring_tilt(degrees: f64, plain: bool) -> f64 {
    unsafe { ffi::negs_marks_ring_tilt(degrees, plain as u8) }
}

/// A loose ellipse drawn in pencil round `rect`; Plain: a rounded
/// rectangle, or an ellipse round a number.
pub fn ring(rect: Rect, radii: Radii, plain: bool) -> Path {
    let r = ffi::negs_radii_t { horizontal: radii.horizontal, vertical: radii.vertical, relative: radii.relative as u8 };
    let mut segs = [ffi::negs_seg_t::default(); 10];
    let n = unsafe { ffi::negs_marks_ring(c_rect(rect), &r, plain as u8, segs.as_mut_ptr(), segs.len()) };
    from_segs(&segs[..n.min(segs.len())])
}

// MARK: Strokes

/// A strokes block from the core, freed when dropped.
struct Strokes(ffi::negs_strokes_t);

impl Strokes {
    fn outlines(&self) -> Vec<(Vec<Pt>, f64)> {
        let s = &self.0;
        if s.stroke_count == 0 || s.points.is_null() {
            return Vec::new();
        }
        let points = unsafe { std::slice::from_raw_parts(s.points, s.point_count) };
        let counts = unsafe { std::slice::from_raw_parts(s.counts, s.stroke_count) };
        let opacities = unsafe { std::slice::from_raw_parts(s.opacities, s.stroke_count) };
        let mut at = 0;
        counts
            .iter()
            .zip(opacities)
            .map(|(&count, &opacity)| {
                let outline = points[at..at + count as usize].iter().map(|&p| from_c(p)).collect();
                at += count as usize;
                (outline, opacity)
            })
            .collect()
    }
}

impl Drop for Strokes {
    fn drop(&mut self) {
        unsafe { ffi::negs_strokes_free(&mut self.0) };
    }
}

/// The page's seed: the name, "·star" and the star's place, from 0.
pub fn star_seed(name: &str, index: usize) -> u32 {
    let (p, n) = name_ptr(name);
    unsafe { ffi::negs_marks_star_seed(p, n, index as u32) }
}

/// The three strokes of the `index`th star in `name`'s row, round (0, 0),
/// each a filled outline.
pub fn star_strokes(name: &str, index: usize) -> Vec<Vec<Pt>> {
    let (p, n) = name_ptr(name);
    let strokes = Strokes(unsafe { ffi::negs_marks_star(p, n, index as u32) });
    strokes.outlines().into_iter().map(|(outline, _)| outline).collect()
}

/// The star's strokes as one path round (0, 0).
pub fn star_path(name: &str, index: usize) -> Path {
    let mut path = Path::default();
    for stroke in star_strokes(name, index) {
        path.0.extend(Path::polygon(&stroke).0);
    }
    path
}

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

pub fn cross_kind(name: &str) -> CrossKind {
    let (p, n) = name_ptr(name);
    if unsafe { ffi::negs_marks_cross_kind(p, n) } == ffi::NEGS_CROSS_TWICE { CrossKind::Twice } else { CrossKind::Loose }
}

/// The strokes for a picture of this size: top left to bottom right, then
/// top right to bottom left, as a hand draws an X.
pub fn cross_strokes(w: f64, h: f64, name: &str) -> Vec<CrossStroke> {
    let (p, n) = name_ptr(name);
    let strokes = Strokes(unsafe { ffi::negs_marks_cross(w, h, p, n) });
    strokes.outlines().into_iter().map(|(outline, opacity)| CrossStroke { outline, opacity }).collect()
}

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

fn box_kind_of(kind: u32) -> BoxKind {
    match kind {
        ffi::NEGS_BOX_FOUR => BoxKind::Four,
        ffi::NEGS_BOX_LOOP => BoxKind::Loop,
        _ => BoxKind::Two,
    }
}

fn pick_box_of(raw: ffi::negs_strokes_t) -> PickBox {
    let strokes = Strokes(raw);
    PickBox { kind: box_kind_of(strokes.0.kind), tilt: strokes.0.tilt, strokes: strokes.outlines().into_iter().map(|(o, _)| o).collect() }
}

pub fn box_kind(name: &str) -> BoxKind {
    let (p, n) = name_ptr(name);
    box_kind_of(unsafe { ffi::negs_marks_box_kind(p, n) })
}

/// The box round a `w` × `h` picture, `o` outside it. `tight` keeps the
/// corners and overshoots small (the strip); `weight` scales the stroke.
pub fn pick_box(w: f64, h: f64, o: f64, name: &str, tight: bool, weight: f64) -> PickBox {
    let (p, n) = name_ptr(name);
    pick_box_of(unsafe { ffi::negs_marks_pick_box(w, h, o, p, n, tight as u8, weight) })
}

/// A pick in the grid: 3 pt out.
pub fn pick_box_grid(w: f64, h: f64, name: &str) -> PickBox {
    let (p, n) = name_ptr(name);
    pick_box_of(unsafe { ffi::negs_marks_pick_box_grid(w, h, p, n) })
}

/// A pick in the strip: 2 pt out, the overshoots kept short.
pub fn pick_box_strip(w: f64, h: f64, name: &str) -> PickBox {
    let (p, n) = name_ptr(name);
    pick_box_of(unsafe { ffi::negs_marks_pick_box_strip(w, h, p, n) })
}

/// Selection in the grid: a second box, 8 pt out, in a 4 pt line.
pub fn pick_box_selection(w: f64, h: f64, name: &str) -> PickBox {
    let (p, n) = name_ptr(name);
    pick_box_of(unsafe { ffi::negs_marks_pick_box_selection(w, h, p, n) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Rust constants are the core's.
    #[test]
    fn constants_are_the_cores() {
        let mut c = ffi::negs_marks_constants_t::default();
        unsafe { ffi::negs_marks_constants(&mut c) };
        assert_eq!(
            (c.pencil_width, c.pencil_height, c.highlighter_width, c.ring_line_width),
            (PENCIL_WIDTH, PENCIL_HEIGHT, HIGHLIGHTER_WIDTH, RING_LINE_WIDTH)
        );
        assert_eq!((c.ring_outset_w, c.ring_outset_h), RING_OUTSET);
        assert_eq!((c.star_size, c.star_gap, c.reach), (STAR_SIZE, STAR_GAP, REACH));
        for (which, radii) in [(ffi::NEGS_RADII_BOX, Radii::BOX), (ffi::NEGS_RADII_NUMBER, Radii::NUMBER)] {
            let mut r = ffi::negs_radii_t::default();
            unsafe { ffi::negs_marks_radii(which, &mut r) };
            assert_eq!(Radii { horizontal: r.horizontal, vertical: r.vertical, relative: r.relative != 0 }, radii);
        }
    }

    #[test]
    fn layouts_match_the_cores() {
        use negs_core_sys::layout;
        use std::mem::{offset_of, size_of};
        assert_eq!(layout("negs_seg_t"), size_of::<ffi::negs_seg_t>() as i64);
        assert_eq!(layout("negs_seg_t.p"), offset_of!(ffi::negs_seg_t, p) as i64);
        assert_eq!(layout("negs_radii_t"), size_of::<ffi::negs_radii_t>() as i64);
        assert_eq!(layout("negs_radii_t.relative"), offset_of!(ffi::negs_radii_t, relative) as i64);
        assert_eq!(layout("negs_strokes_t"), size_of::<ffi::negs_strokes_t>() as i64);
        assert_eq!(layout("negs_strokes_t.tilt"), offset_of!(ffi::negs_strokes_t, tilt) as i64);
        assert_eq!(layout("negs_strokes_t.point_count"), offset_of!(ffi::negs_strokes_t, point_count) as i64);
        assert_eq!(layout("negs_marks_constants_t"), size_of::<ffi::negs_marks_constants_t>() as i64);
    }

    #[test]
    fn pencil_and_plain_pencil() {
        let path = pencil(Rect::new(10.0, 20.0, 80.0, 6.0), false);
        assert_eq!(path.0[0], Seg::Move(pt(10.8, 20.0 + 3.6)));
        assert!(!path.closed());
        assert_eq!(pencil(Rect::new(0.0, 0.0, 50.0, 6.0), true).0, vec![Seg::Move(pt(1.0, 3.0)), Seg::Line(pt(49.0, 3.0))]);
        let b = ring(Rect::new(0.0, 0.0, 200.0, 120.0), Radii::BOX, false).bounds().unwrap();
        assert!((b.w - 200.0).abs() < 1e-9 && (b.h - 120.0).abs() < 1e-9, "{b:?}");
        assert!(Path::default().bounds().is_none());
    }

    #[test]
    fn strokes_come_back_and_nothing_leaks() {
        let before = negs_core_sys::alloc_stats();
        for i in 0..50 {
            let name = format!("DSC_{}.NEF", 7000 + i);
            assert_eq!(star_strokes(&name, i % 5).len(), 3);
            let cross = cross_strokes(200.0, 140.0, &name);
            assert_eq!(cross.len(), if cross_kind(&name) == CrossKind::Loose { 2 } else { 4 });
            let b = pick_box_grid(264.0, 176.0, &name);
            assert_eq!(b.kind, box_kind(&name));
        }
        assert!(cross_strokes(0.0, 0.0, "x").is_empty());
        let after = negs_core_sys::alloc_stats();
        if after.tracked == 1 {
            assert_eq!(after.live_count, before.live_count, "{after:?}");
            assert_eq!(after.bad_frees, 0);
        }
    }
}
