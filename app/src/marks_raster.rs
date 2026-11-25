//! The strip's and grid's hand-drawn marks as cached sprites.
//!
//! GPUI 0.2.2 tessellates every `paint_path` with lyon on the CPU, each
//! frame: about 7 µs a polygon, so a sheet of rated, picked, selected
//! thumbnails scrolling at 60 fps would spend 15–20 ms a frame on stars
//! alone. Instead each mark group is rasterised once (nonzero winding, four
//! sub-scanlines per pixel with exact horizontal coverage, the halo a real
//! blur as Direct2D's) into a straight-alpha BGRA8 `RenderImage` at the
//! window's scale, and painted as one sprite per group per frame:
//!
//! - **stars**: a photograph's row, independent of the frame's size (so a
//!   Size slider drag reuses them);
//! - **cross**, **pick box**, **selection box**: at the frame's size.
//!
//! A frame's new sprites are made within a small budget (`BUDGET`); past it
//! the caller paints the marks as vector paths this frame. Sprites live in
//! a bounded `Lru`; one let go is released from the atlas through the same
//! queue as thumbnails (`thumbs::release`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Bounds, Hsla, Pixels, Point, RenderImage, Rgba, Window, point, px, size};
use negs_app::sheet_rules::Lru;
use negs_marks::Pt;

use crate::color_display::bgra_to_render_image;
use crate::marks_gpui::BoxInk;

/// How long a frame may spend making new sprites.
const BUDGET: Duration = Duration::from_millis(3);
/// Sub-scanlines per pixel row.
const SUB: usize = 4;
/// How far a halo reaches past its mark, in DIPs.
const HALO_REACH: f32 = 4.0;

/// A mark group's sprite: its image and where it sits relative to the
/// anchor it was made for (DIPs).
pub struct Sprite {
    image: Option<Arc<RenderImage>>,
    pub offset: (f32, f32),
    pub size: (f32, f32),
}

impl Drop for Sprite {
    fn drop(&mut self) {
        if let Some(image) = self.image.take() {
            crate::thumbs::release(image);
        }
    }
}

impl Sprite {
    pub fn paint(&self, window: &mut Window, anchor: Point<Pixels>) {
        let Some(image) = &self.image else { return };
        let bounds = Bounds {
            origin: point(anchor.x + px(self.offset.0), anchor.y + px(self.offset.1)),
            size: size(px(self.size.0), px(self.size.1)),
        };
        let _ = window.paint_image(bounds, Default::default(), image.clone(), 0, false);
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Key {
    Stars { name: String, rating: u8, scale: u32, color: u32 },
    Cross { name: String, w: u32, h: u32, scale: u32, color: u32 },
    Box { kind: u8, name: String, w: u32, h: u32, scale: u32, color: u32 },
}

fn q(v: f32) -> u32 {
    (v * 4.0).round().max(0.0) as u32
}

fn color_key(c: Hsla) -> u32 {
    let r: Rgba = c.into();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (b(r.r) << 24) | (b(r.g) << 16) | (b(r.b) << 8) | b(r.a)
}

fn rgba(c: Hsla) -> [f32; 4] {
    let r: Rgba = c.into();
    [r.r, r.g, r.b, r.a]
}

pub struct MarkSprites {
    lru: Lru<Key, Arc<Sprite>>,
    frame_started: Instant,
    spent: Duration,
    pub made: u64,
}

impl Default for MarkSprites {
    fn default() -> Self {
        Self { lru: Lru::new(96 << 20, 1500), frame_started: Instant::now(), spent: Duration::ZERO, made: 0 }
    }
}

impl MarkSprites {
    /// A new frame: a fresh budget.
    pub fn begin_frame(&mut self) {
        self.frame_started = Instant::now();
        self.spent = Duration::ZERO;
    }

    /// Everything goes (the theme or the window's scale changed).
    pub fn clear(&mut self) {
        self.lru.clear();
    }

    fn get_or_make(&mut self, key: Key, make: impl FnOnce() -> Option<Sprite>) -> Option<Arc<Sprite>> {
        if let Some(s) = self.lru.get(&key) {
            return Some(s.clone());
        }
        if self.spent >= BUDGET {
            return None;
        }
        let started = Instant::now();
        let sprite = Arc::new(make()?);
        self.spent += started.elapsed();
        self.made += 1;
        let bytes = (sprite.size.0 * sprite.size.1 * 4.0 * 4.0) as usize;
        self.lru.insert(key, sprite.clone(), bytes.max(64));
        Some(sprite)
    }

    /// The `rating` stars of `name`'s row at a frame's foot: anchored at the
    /// first star's centre.
    pub fn stars(&mut self, name: &str, rating: u8, scale: f32, color: Hsla) -> Option<Arc<Sprite>> {
        let key = Key::Stars { name: name.to_string(), rating, scale: q(scale), color: color_key(color) };
        self.get_or_make(key, || {
            let step = (negs_marks::STAR_SIZE + negs_marks::STAR_GAP) as f32;
            let mut polys = vec![];
            for i in 0..rating.min(5) as usize {
                for stroke in negs_marks::star_strokes(name, i) {
                    polys.push(stroke.iter().map(|p| (p.x as f32 + i as f32 * step, p.y as f32)).collect::<Vec<_>>());
                }
            }
            let half = negs_marks::STAR_SIZE as f32 / 2.0 + 1.5;
            let width = (rating.min(5) as f32 - 1.0).max(0.0) * step + 2.0 * half;
            render(&[Layer { polys, color: rgba(color), halo: true }], (-half, -half), (width, 2.0 * half), scale)
        })
    }

    /// A reject's cross over a `w` × `h` picture, anchored at its top left.
    pub fn cross(&mut self, name: &str, w: f32, h: f32, scale: f32, color: Hsla) -> Option<Arc<Sprite>> {
        let key = Key::Cross { name: name.to_string(), w: q(w), h: q(h), scale: q(scale), color: color_key(color) };
        self.get_or_make(key, || {
            let strokes = negs_marks::cross_strokes(w as f64, h as f64, name);
            // Each stroke at its own strength: one layer each.
            let layers: Vec<Layer> = strokes
                .iter()
                .map(|s| {
                    let mut c = rgba(color);
                    c[3] *= s.opacity as f32;
                    Layer { polys: vec![to_f32(&s.outline)], color: c, halo: true }
                })
                .collect();
            render(&layers, (-HALO_REACH * 2.0, -HALO_REACH * 2.0), (w + HALO_REACH * 4.0, h + HALO_REACH * 4.0), scale)
        })
    }

    /// A pick's or the selection's box round a `w` × `h` picture, anchored
    /// at its top left.
    pub fn pick_box(&mut self, kind: BoxInk, name: &str, w: f32, h: f32, scale: f32, color: Hsla) -> Option<Arc<Sprite>> {
        let key = Key::Box { kind: kind as u8, name: name.to_string(), w: q(w), h: q(h), scale: q(scale), color: color_key(color) };
        self.get_or_make(key, || {
            let (wd, hd) = (w as f64, h as f64);
            let pick = match kind {
                BoxInk::Strip => negs_marks::pick_box_strip(wd, hd, name),
                BoxInk::Grid => negs_marks::pick_box_grid(wd, hd, name),
                BoxInk::Selection => negs_marks::pick_box_selection(wd, hd, name),
            };
            let (sin, cos) = pick.tilt.to_radians().sin_cos();
            let (cx, cy) = (wd * 0.5, hd * 0.5);
            let polys = pick
                .strokes
                .iter()
                .filter(|s| s.len() >= 3)
                .map(|s| {
                    s.iter()
                        .map(|p| {
                            let (dx, dy) = (p.x - cx, p.y - cy);
                            ((dx * cos - dy * sin + cx) as f32, (dx * sin + dy * cos + cy) as f32)
                        })
                        .collect()
                })
                .collect();
            let reach = negs_marks::REACH as f32;
            render(&[Layer { polys, color: rgba(color), halo: false }], (-reach, -reach), (w + 2.0 * reach, h + 2.0 * reach), scale)
        })
    }
}

fn to_f32(points: &[Pt]) -> Vec<(f32, f32)> {
    points.iter().map(|p| (p.x as f32, p.y as f32)).collect()
}

/// One colour's polygons, filled nonzero, optionally over a dark halo.
struct Layer {
    polys: Vec<Vec<(f32, f32)>>,
    color: [f32; 4],
    halo: bool,
}

/// Rasterises `layers` over the DIP rect at `origin` of `extent` at `scale`.
/// The halos of every layer that has one are one blur, under all fills.
fn render(layers: &[Layer], origin: (f32, f32), extent: (f32, f32), scale: f32) -> Option<Sprite> {
    let (w, h) = ((extent.0 * scale).ceil().max(1.0) as usize, (extent.1 * scale).ceil().max(1.0) as usize);
    if w * h > 4096 * 4096 {
        return None;
    }
    let to_px = |p: &Vec<(f32, f32)>| -> Vec<(f32, f32)> { p.iter().map(|&(x, y)| ((x - origin.0) * scale, (y - origin.1) * scale)).collect() };
    let covers: Vec<Vec<f32>> = layers.iter().map(|l| coverage(&l.polys.iter().map(to_px).collect::<Vec<_>>(), w, h)).collect();
    let image = |bgra: Vec<u8>| Sprite { image: Some(bgra_to_render_image(bgra, w as u32, h as u32)), offset: origin, size: (w as f32 / scale, h as f32 / scale) };
    // One colour and no halo (the boxes): the colour at the coverage's strength.
    if layers.len() == 1 && !layers[0].halo {
        let c = layers[0].color;
        let (r, g, b) = ((c[0].clamp(0.0, 1.0) * 255.0).round() as u8, (c[1].clamp(0.0, 1.0) * 255.0).round() as u8, (c[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        let mut bgra = vec![0u8; w * h * 4];
        let mut any = false;
        for (p, &v) in bgra.chunks_exact_mut(4).zip(&covers[0]) {
            if v > 0.0 {
                any = true;
                p.copy_from_slice(&[b, g, r, ((v.min(1.0) * c[3]) * 255.0).round() as u8]);
            }
        }
        return any.then(|| image(bgra));
    }
    // Premultiplied RGBA accumulation.
    let mut acc = vec![[0f32; 4]; w * h];
    let haloed: Vec<usize> = (0..layers.len()).filter(|&i| layers[i].halo).collect();
    if !haloed.is_empty() {
        let mut sum = vec![0f32; w * h];
        for &i in &haloed {
            for (s, c) in sum.iter_mut().zip(&covers[i]) {
                *s = (*s + c).min(1.0);
            }
        }
        let strength = haloed.iter().map(|&i| layers[i].color[3]).fold(0.0f32, f32::max).max(0.4);
        let halo = blur(&sum, w, h, (1.6 * scale).max(1.0));
        composite(&mut acc, &halo, [0.0, 0.0, 0.0, 0.62 * strength]);
    }
    for (layer, cov) in layers.iter().zip(&covers) {
        composite(&mut acc, cov, layer.color);
    }
    let mut bgra = vec![0u8; w * h * 4];
    let mut any = false;
    for (px_, a) in bgra.chunks_exact_mut(4).zip(&acc) {
        let alpha = a[3].clamp(0.0, 1.0);
        if alpha <= 1.0 / 512.0 {
            continue;
        }
        any = true;
        let straight = |v: f32| ((v / alpha).clamp(0.0, 1.0) * 255.0).round() as u8;
        px_[0] = straight(a[2]);
        px_[1] = straight(a[1]);
        px_[2] = straight(a[0]);
        px_[3] = (alpha * 255.0).round() as u8;
    }
    any.then(|| image(bgra))
}

fn composite(acc: &mut [[f32; 4]], cov: &[f32], color: [f32; 4]) {
    for (a, &c) in acc.iter_mut().zip(cov) {
        if c <= 0.0 {
            continue;
        }
        let alpha = c.min(1.0) * color[3];
        let keep = 1.0 - alpha;
        a[0] = color[0] * alpha + a[0] * keep;
        a[1] = color[1] * alpha + a[1] * keep;
        a[2] = color[2] * alpha + a[2] * keep;
        a[3] = alpha + a[3] * keep;
    }
}

/// Coverage of `polys` (pixel coordinates) on a `w` × `h` grid, nonzero.
fn coverage(polys: &[Vec<(f32, f32)>], w: usize, h: usize) -> Vec<f32> {
    let mut cov = vec![0f32; w * h];
    // Edges as (x0, y0, x1, y1, direction), top to bottom.
    let mut edges: Vec<(f32, f32, f32, f32, i32)> = vec![];
    for poly in polys {
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 - b.1).abs() < 1e-6 {
                continue;
            }
            if a.1 < b.1 { edges.push((a.0, a.1, b.0, b.1, 1)) } else { edges.push((b.0, b.1, a.0, a.1, -1)) }
        }
    }
    if edges.is_empty() {
        return cov;
    }
    let top = edges.iter().map(|e| e.1).fold(f32::INFINITY, f32::min).max(0.0);
    let bottom = edges.iter().map(|e| e.3).fold(f32::NEG_INFINITY, f32::max).min(h as f32);
    let weight = 1.0 / SUB as f32;
    let mut crossings: Vec<(f32, i32)> = Vec::with_capacity(16);
    let first = (top * SUB as f32).floor() as usize;
    let last = ((bottom * SUB as f32).ceil() as usize).min(h * SUB);
    for sub in first..last {
        let y = (sub as f32 + 0.5) / SUB as f32;
        crossings.clear();
        for &(x0, y0, x1, y1, dir) in &edges {
            if y >= y0 && y < y1 {
                crossings.push((x0 + (y - y0) * (x1 - x0) / (y1 - y0), dir));
            }
        }
        if crossings.len() < 2 {
            continue;
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let row = &mut cov[(sub / SUB) * w..(sub / SUB + 1) * w];
        let mut winding = 0;
        for pair in crossings.windows(2) {
            winding += pair[0].1;
            if winding != 0 {
                span(row, pair[0].0, pair[1].0, weight);
            }
        }
    }
    cov
}

fn span(row: &mut [f32], a: f32, b: f32, weight: f32) {
    let w = row.len() as f32;
    let (a, b) = (a.clamp(0.0, w), b.clamp(0.0, w));
    if b <= a {
        return;
    }
    let (ia, ib) = (a.floor() as usize, b.floor() as usize);
    if ia == ib {
        let last = row.len() - 1;
        row[ia.min(last)] += (b - a) * weight;
        return;
    }
    row[ia] += (ia as f32 + 1.0 - a) * weight;
    let end = ib.min(row.len());
    for v in &mut row[ia + 1..end] {
        *v += weight;
    }
    if ib < row.len() {
        row[ib] += (b - ib as f32) * weight;
    }
}

/// Three box blurs of `radius` px (close to a Gaussian), worked only over
/// the box round what's covered (a cross's strokes, a row of stars).
fn blur(src: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    let r = radius.round().max(1.0) as usize;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if src[y * w + x] > 0.0 {
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                y0 = y0.min(y);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 {
        return vec![0.0; w * h];
    }
    let pad = 3 * r;
    let (x0, y0, x1, y1) = (x0.saturating_sub(pad), y0.saturating_sub(pad), (x1 + pad).min(w), (y1 + pad).min(h));
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut a: Vec<f32> = (0..bh).flat_map(|y| src[(y0 + y) * w + x0..(y0 + y) * w + x1].iter().copied()).collect();
    let mut b = vec![0f32; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, bw, bh, r, true);
        box_pass(&b, &mut a, bw, bh, r, false);
    }
    let mut out = vec![0f32; w * h];
    for y in 0..bh {
        out[(y0 + y) * w + x0..(y0 + y) * w + x1].copy_from_slice(&a[y * bw..(y + 1) * bw]);
    }
    out
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let at = |o: usize, i: usize| if horizontal { o * w + i } else { i * w + o };
    let norm = 1.0 / (2 * r + 1) as f32;
    for o in 0..outer {
        let mut sum = 0f32;
        for i in 0..=r.min(inner - 1) {
            sum += src[at(o, i)];
        }
        for i in 0..inner {
            dst[at(o, i)] = sum * norm;
            if i + r + 1 < inner {
                sum += src[at(o, i + r + 1)];
            }
            if i >= r {
                sum -= src[at(o, i - r)];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_of_a_square_is_exact_inside_and_fractional_at_its_edges() {
        let square = vec![vec![(1.5, 1.0), (4.5, 1.0), (4.5, 3.0), (1.5, 3.0)]];
        let cov = coverage(&square, 6, 4);
        let at = |x: usize, y: usize| cov[y * 6 + x];
        assert!((at(2, 1) - 1.0).abs() < 1e-5 && (at(3, 2) - 1.0).abs() < 1e-5);
        assert!((at(1, 1) - 0.5).abs() < 1e-5, "half the pixel: {}", at(1, 1));
        assert!((at(4, 2) - 0.5).abs() < 1e-5);
        assert_eq!(at(0, 0), 0.0);
        assert_eq!(at(5, 3), 0.0);
        let total: f32 = cov.iter().sum();
        assert!((total - 6.0).abs() < 1e-4, "area 3 × 2: {total}");
    }

    #[test]
    fn nonzero_winding_fills_a_self_overlapping_star() {
        // A pentagram drawn in one stroke: its middle is covered twice and
        // must still be filled (nonzero), as Core Graphics and Direct2D fill it.
        let (cx, cy, r) = (20.0f32, 20.0f32, 18.0f32);
        let pts: Vec<(f32, f32)> = (0..5)
            .map(|i| {
                let a = (i as f32 * 144.0 - 90.0).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        let cov = coverage(&[pts], 40, 40);
        assert!(cov[20 * 40 + 20] > 0.99, "the pentagon in the middle is filled");
        assert_eq!(cov[2 * 40 + 2], 0.0);
    }

    #[test]
    fn star_sprites_ignore_frame_size_and_are_reused() {
        let mut sprites = MarkSprites::default();
        let paper = gpui::white();
        let a = sprites.stars("DSC_0001.NEF", 3, 2.0, paper).expect("made");
        let b = sprites.stars("DSC_0001.NEF", 3, 2.0, paper).expect("cached");
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(sprites.made, 1);
        // Three stars and their halos: about 3 × 14.5 DIP across.
        assert!(a.size.0 > 35.0 && a.size.0 < 50.0, "{:?}", a.size);
        let box_ = sprites.pick_box(BoxInk::Selection, "DSC_0001.NEF", 240.0, 160.0, 2.0, paper).expect("box");
        assert!(box_.size.0 >= 240.0 + 2.0 * negs_marks::REACH as f32 - 0.5);
        assert_eq!(box_.offset, (-(negs_marks::REACH as f32), -(negs_marks::REACH as f32)));
    }

    #[test]
    fn sprites_are_fast_enough_to_make_on_the_ui_thread() {
        let mut sprites = MarkSprites::default();
        let started = Instant::now();
        for i in 0..20 {
            sprites.begin_frame();
            let name = format!("IMG_{i:04}.CR3");
            let _ = sprites.stars(&name, 5, 2.0, gpui::white());
            let _ = sprites.pick_box(BoxInk::Grid, &name, 264.0, 176.0, 2.0, gpui::white());
            let _ = sprites.cross(&name, 264.0, 176.0, 2.0, gpui::white());
        }
        let per = started.elapsed().as_secs_f64() * 1000.0 / 20.0;
        println!("{per:.2} ms per frame's three sprites");
        assert!(per < 25.0, "{per:.1} ms");
    }
}
