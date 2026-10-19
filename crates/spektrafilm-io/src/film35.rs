//! The 35mm border: a strip of 135 film, drawn at whatever resolution it is
//! shown at, so its perforations and edge print stay sharp on a 60 MP
//! export. What a drawing can't invent (dust, specks and the film base's
//! uneven density) comes from a real scan, `data/borders/35mm/material.png`
//! (see `examples/film35_material.rs`); the grain is synthesised at the
//! output's resolution.
//!
//! The geometry is ISO 1007 135 film, in millimetres. `u` runs along the
//! strip and `v` across it from the top edge. The macOS preview shows PNGs
//! rendered by this code (`examples/film35_borders.rs`).

use std::path::Path;
use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result, ensure};
use image::{GrayAlphaImage, RgbaImage};
use rayon::prelude::*;

use crate::border::FilmFormat;

/// Width of 135 film.
pub const FILM_WIDTH: f32 = 35.0;
/// Distance between perforations along the film.
pub const PERF_PITCH: f32 = 4.75;
/// Length of film the scanned material covers: one full frame.
pub const MATERIAL_LENGTH: f32 = 8.0 * PERF_PITCH;
/// Resolution of `data/borders/35mm/material.png`.
pub const MATERIAL_PX_PER_MM: f32 = 125.0;

/// Kodak Standard perforations (still and print films): size along and
/// across the film, and corner radius.
const KS: (f32, f32, f32) = (1.981, 2.794, 0.51);
/// Bell & Howell perforations (cine camera negatives): straight leading
/// and trailing edges, ends curved with this radius.
const BH: (f32, f32, f32) = (1.854, 2.794, 2.083);
/// Film edge to the outside of the perforations.
const PERF_EDGE: f32 = 2.01;
const GATE_HEIGHT: f32 = 24.0;
const GATE_RADIUS: f32 = 0.35;
/// The gate's shadow is a little out of focus on the film.
const GATE_SOFTNESS: f32 = 0.03;

const TEXT_SIZE: f32 = 1.25;
const SMALL_TEXT: f32 = 0.95;
const TOP_BASELINE: f32 = 1.48;
const BOTTOM_BASELINE: f32 = FILM_WIDTH - 0.55;
const FRAME_NUMBER: u32 = 12;

/// Edge print exposure: its blur and the halation around it, in mm, and
/// the halation's strength.
const INK_BLUR: f32 = 0.012;
const INK_GLOW: f32 = 0.07;
const INK_GLOW_STRENGTH: f32 = 0.12;
/// Backlight bleeding out of the perforations: reach in mm and strength.
const BLOOM: f32 = 0.09;
const BLOOM_STRENGTH: f32 = 0.06;
/// Grain: clump size in mm and its strength (log exposure).
const GRAIN: f32 = 0.006;
const GRAIN_STRENGTH: f32 = 0.22;
/// How strongly the base's density variation shows in the rebate.
const VARIATION_GAIN: f32 = 15.0;
const DUST_OPACITY: f32 = 0.9;

/// How a film's rebate looks once scanned and inverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Colour negative: black rebate, amber edge print, white dust.
    Negative,
    /// Reversal (slide) film: black rebate, yellow edge print; dust is
    /// dark, so it disappears into the rebate.
    Slide,
    /// Black-and-white negative: grey-black rebate, grey edge print.
    BlackAndWhite,
}

impl Kind {
    /// Rebate, edge print and dust colours, sRGB.
    fn colors(self) -> ([u8; 3], [u8; 3], Option<[u8; 3]>) {
        match self {
            Self::Negative => ([15, 13, 12], [242, 164, 60], Some([214, 208, 196])),
            Self::Slide => ([10, 9, 9], [233, 210, 130], None),
            Self::BlackAndWhite => ([24, 24, 23], [216, 216, 212], Some([222, 222, 218])),
        }
    }
}

/// What a stock prints along its edges.
#[derive(Clone, Copy, Debug)]
pub enum EdgePrint {
    /// Still film: frame numbers, these words along the top edge and a DX
    /// barcode along the bottom.
    Still(&'static [&'static str]),
    /// Cine camera negative: Bell & Howell perforations and, along the
    /// bottom edge, the KeyKode number and barcode (when the film code is
    /// known) and the manufacturing line, which starts with the product
    /// code.
    Cine {
        product: &'static str,
        keykode: Option<&'static str>,
    },
    /// Print film: the product code and maker along the top edge.
    Print(&'static str),
}

/// Each film profile's edge print and look. Sources for the edge prints
/// are in `docs/35mm-scan-sources.md`. Films not listed (Tri-X 7266 is
/// only made in 16 mm and Super 8) get the generic strip.
pub const STOCKS: &[(&str, Kind, EdgePrint)] = &[
    ("fujifilm_c200", Kind::Negative, EdgePrint::Still(&["FUJI", "200", "CA24"])),
    ("fujifilm_pro_400h", Kind::Negative, EdgePrint::Still(&["PRO400H", "FUJI"])),
    ("fujifilm_provia_100f", Kind::Slide, EdgePrint::Still(&["FUJI RDPIII"])),
    ("fujifilm_velvia_100", Kind::Slide, EdgePrint::Still(&["FUJI RVP100"])),
    ("fujifilm_xtra_400", Kind::Negative, EdgePrint::Still(&["FUJI", "S-400", "CH23"])),
    ("kodak_2302", Kind::BlackAndWhite, EdgePrint::Print("2302 KODAK")),
    ("kodak_2383", Kind::Negative, EdgePrint::Print("2383 KODAK")),
    ("kodak_2393", Kind::Negative, EdgePrint::Print("2393 KODAK")),
    (
        "kodak_doublex",
        Kind::BlackAndWhite,
        EdgePrint::Cine { product: "5222", keykode: Some("KE") },
    ),
    ("kodak_ektachrome_100", Kind::Slide, EdgePrint::Still(&["KODAK E100"])),
    ("kodak_ektar_100", Kind::Negative, EdgePrint::Still(&["KODAK EKTAR 100"])),
    ("kodak_gold_200", Kind::Negative, EdgePrint::Still(&["KODAK GB 200"])),
    ("kodak_kodachrome_64", Kind::Slide, EdgePrint::Still(&["KODAK KR"])),
    ("kodak_portra_160", Kind::Negative, EdgePrint::Still(&["KODAK PORTRA 160"])),
    ("kodak_portra_400", Kind::Negative, EdgePrint::Still(&["KODAK PORTRA 400"])),
    ("kodak_portra_800", Kind::Negative, EdgePrint::Still(&["KODAK PORTRA 800"])),
    ("kodak_portra_800_push1", Kind::Negative, EdgePrint::Still(&["KODAK PORTRA 800"])),
    ("kodak_portra_800_push2", Kind::Negative, EdgePrint::Still(&["KODAK PORTRA 800"])),
    ("kodak_trix400_d76_8min", Kind::BlackAndWhite, EdgePrint::Still(&["KODAK 400TX"])),
    ("kodak_ultramax_400", Kind::Negative, EdgePrint::Still(&["KODAK GC 400-9"])),
    (
        "kodak_verita_200d",
        Kind::Negative,
        EdgePrint::Cine { product: "5206", keykode: None },
    ),
    (
        "kodak_vision3_200t",
        Kind::Negative,
        EdgePrint::Cine { product: "5213", keykode: Some("EO") },
    ),
    (
        "kodak_vision3_250d",
        Kind::Negative,
        EdgePrint::Cine { product: "5207", keykode: Some("EN") },
    ),
    (
        "kodak_vision3_500t",
        Kind::Negative,
        EdgePrint::Cine { product: "5219", keykode: Some("EJ") },
    ),
    (
        "kodak_vision3_50d",
        Kind::Negative,
        EdgePrint::Cine { product: "5203", keykode: Some("ER") },
    ),
];

/// The strip for a generic film: frame numbers and DX code, no name.
const GENERIC: (Kind, EdgePrint) = (Kind::Negative, EdgePrint::Still(&[]));

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| {
        FontRef::try_from_slice(include_bytes!("../fonts/BarlowCondensed-SemiBold.ttf"))
            .expect("the bundled font parses")
    })
}

fn srgb_to_linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn linear(c: [u8; 3]) -> [f32; 3] {
    c.map(srgb_to_linear)
}

/// One frame of 35mm film for a film profile and format.
pub struct Strip {
    pub format: FilmFormat,
    pub kind: Kind,
    pub print: EdgePrint,
    material: Option<GrayAlphaImage>,
}

impl Strip {
    /// The strip for `film`, or the generic one, without scanned material.
    pub fn new(film: &str, format: FilmFormat) -> Self {
        let (kind, print) = STOCKS
            .iter()
            .find(|(id, ..)| *id == film)
            .map(|&(_, kind, print)| (kind, print))
            .unwrap_or(GENERIC);
        Self { format, kind, print, material: None }
    }

    /// The strip for `film` with the scanned material from `data_dir`.
    pub fn load(data_dir: &Path, film: &str, format: FilmFormat) -> Result<Self> {
        let path = data_dir.join("borders/35mm/material.png");
        let material = image::open(&path)
            .with_context(|| format!("loading border material {}", path.display()))?
            .to_luma_alpha8();
        ensure!(
            material.width() == (MATERIAL_LENGTH * MATERIAL_PX_PER_MM).round() as u32
                && material.height() == (FILM_WIDTH * MATERIAL_PX_PER_MM).round() as u32,
            "{} has the wrong size",
            path.display()
        );
        Ok(Self { material: Some(material), ..Self::new(film, format) })
    }

    /// Strip length (along the film) and gate width, in mm.
    pub fn size(format: FilmFormat) -> (f32, f32) {
        match format {
            FilmFormat::Full => (8.0 * PERF_PITCH, 36.0),
            FilmFormat::Half => (4.0 * PERF_PITCH, 18.0),
        }
    }

    /// The gate, which the photograph shows through, as fractions of the
    /// strip laid out horizontally: [x, y, width, height].
    pub fn window(format: FilmFormat) -> [f64; 4] {
        let (length, gate) = Self::size(format);
        let (length, gate, film) = (length as f64, gate as f64, FILM_WIDTH as f64);
        let height = GATE_HEIGHT as f64;
        [(length - gate) / 2.0 / length, (film - height) / 2.0 / film, gate / length, height / film]
    }

    /// Whether the strip has to turn to hold a `width` × `height` photograph.
    pub fn turned(&self, width: u32, height: u32) -> bool {
        let (_, gate) = Self::size(self.format);
        (gate > GATE_HEIGHT) != (width > height)
    }

    /// The canvas for a `width` × `height` photograph: the strip scaled so
    /// its gate covers the photograph.
    pub fn canvas(&self, width: u32, height: u32) -> (u32, u32) {
        let (length, _) = Self::size(self.format);
        let (mut tw, mut th) = (length as f64, FILM_WIDTH as f64);
        let [_, _, mut ww, mut wh] = Self::window(self.format);
        if self.turned(width, height) {
            (tw, th, ww, wh) = (th, tw, wh, ww);
        }
        let k = (width as f64 / (ww * tw)).max(height as f64 / (wh * th));
        ((tw * k).round() as u32, (th * k).round() as u32)
    }

    /// The strip laid out horizontally at `px_per_mm`, sRGB with alpha,
    /// for the preview. It has no grain: at preview sizes the grain is
    /// below one sRGB level, and it would make the PNG ten times larger.
    pub fn preview(&self, px_per_mm: f32) -> RgbaImage {
        let (length, _) = Self::size(self.format);
        let (w, h) = (
            (length * px_per_mm).round() as u32,
            (FILM_WIDTH * px_per_mm).round() as u32,
        );
        let mut renderer = self.renderer(w, h, false);
        renderer.grain = false;
        let mut image = RgbaImage::new(w, h);
        image
            .par_chunks_exact_mut(w as usize * 4)
            .enumerate()
            .for_each(|(y, row)| {
                let mut pixels = vec![[0.0; 4]; w as usize];
                renderer.row(y as u32, &mut pixels);
                for (out, p) in row.chunks_exact_mut(4).zip(&pixels) {
                    for c in 0..3 {
                        out[c] = (linear_to_srgb(p[c]) * 255.0).round() as u8;
                    }
                    out[3] = (p[3] * 255.0).round() as u8;
                }
            });
        image
    }

    /// Draws the strip onto a `width` × `height` canvas, turned a quarter
    /// turn (like `imageops::rotate270`) when `turned`.
    pub fn renderer(&self, width: u32, height: u32, turned: bool) -> Renderer<'_> {
        Renderer::new(self, width, height, turned)
    }
}

/// Edge print rasterised for one edge of the strip.
struct Band {
    /// Top of the band, in mm across the film.
    top: f32,
    width: usize,
    height: usize,
    /// Pixels per mm.
    scale: f32,
    ink: Vec<f32>,
    glow: Vec<f32>,
}

impl Band {
    fn new(top: f32, bottom: f32, length: f32, scale: f32) -> Self {
        let (width, height) = (
            (length * scale).ceil() as usize + 1,
            ((bottom - top) * scale).ceil() as usize + 1,
        );
        Self {
            top,
            width,
            height,
            scale,
            ink: vec![0.0; width * height],
            glow: Vec::new(),
        }
    }

    fn add(&mut self, x: i64, y: i64, k: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let i = y as usize * self.width + x as usize;
            self.ink[i] = (self.ink[i] + k).min(1.0);
        }
    }

    /// An antialiased rectangle in mm, its coverage multiplied by `dots`.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, dots: impl Fn(f32, f32) -> f32) {
        let s = self.scale;
        let (x0, x1) = (x * s, (x + w) * s);
        let (y0, y1) = ((y - self.top) * s, (y + h - self.top) * s);
        for py in y0.floor() as i64..y1.ceil() as i64 {
            let ky = ((py + 1) as f32).min(y1) - (py as f32).max(y0);
            for px in x0.floor() as i64..x1.ceil() as i64 {
                let kx = ((px + 1) as f32).min(x1) - (px as f32).max(x0);
                let (u, v) = ((px as f32 + 0.5) / s, (py as f32 + 0.5) / s + self.top);
                self.add(px, py, kx.max(0.0) * ky.max(0.0) * dots(u, v));
            }
        }
    }

    /// Advance of `text` at `size` mm, in mm.
    fn measure(text: &str, size: f32) -> f32 {
        let scaled = font().as_scaled(PxScale::from(size * 100.0));
        let tracking = size * 0.06;
        text.chars()
            .map(|c| scaled.h_advance(font().glyph_id(c)) / 100.0 + tracking)
            .sum()
    }

    fn text(&mut self, text: &str, x: f32, baseline: f32, size: f32) -> f32 {
        let s = self.scale;
        let scale = PxScale::from(size * s);
        let scaled = font().as_scaled(scale);
        let tracking = size * 0.06 * s;
        let mut pen = x * s;
        for c in text.chars() {
            let id = font().glyph_id(c);
            let glyph = id.with_scale_and_position(scale, point(pen, (baseline - self.top) * s));
            if let Some(outline) = font().outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, k| {
                    self.add(bounds.min.x as i64 + gx as i64, bounds.min.y as i64 + gy as i64, k)
                });
            }
            pen += scaled.h_advance(id) + tracking;
        }
        pen / s
    }

    /// A small triangle pointing along the strip, 4×4 supersampled.
    fn arrow(&mut self, x: f32, baseline: f32, left: bool) {
        let s = self.scale;
        let (w, h) = (0.62 * s, 0.78 * s);
        let (x0, y0) = (x * s, (baseline - self.top) * s - 0.08 * s - h);
        for py in y0.floor() as i64..=(y0 + h).ceil() as i64 {
            for px in x0.floor() as i64..=(x0 + w).ceil() as i64 {
                let mut inside = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let u = (px as f32 + (sx as f32 + 0.5) / 4.0 - x0) / w;
                        let v = (py as f32 + (sy as f32 + 0.5) / 4.0 - y0) / h;
                        let u = if left { 1.0 - u } else { u };
                        if (0.0..=1.0).contains(&u) && (v - 0.5).abs() <= 0.5 * (1.0 - u) {
                            inside += 1;
                        }
                    }
                }
                self.add(px, py, inside as f32 / 16.0);
            }
        }
    }

    /// A two-track DX code, exposed by a row of LEDs: each bar is a grid
    /// of soft dots.
    fn dx(&mut self, x: f32, width: f32, seed: u32) {
        let n = (width / 0.44).round() as usize;
        let pitch = width / n as f32;
        let y = FILM_WIDTH - 1.62;
        let dots = |u: f32, v: f32| {
            let (a, b) = ((u / 0.065).fract() - 0.5, (v / 0.065).fract() - 0.5);
            0.78 + 0.22 * (1.0 - 4.0 * (a * a + b * b)).max(0.0)
        };
        for i in 0..n {
            let bx = x + i as f32 * pitch;
            self.rect(bx, y, pitch * 0.55, 0.42, dots);
            if (hash(seed, i as u32, 7) & 1 == 1) || i % 6 == 0 {
                self.rect(bx, y + 0.52, pitch * 0.55, 0.42, dots);
            }
        }
    }

    /// A one-track barcode of bars and spaces 1–3 modules wide.
    fn barcode(&mut self, x: f32, width: f32, seed: u32) {
        let module = 0.075;
        let y = FILM_WIDTH - 1.45;
        let mut pen = x;
        let mut i = 0;
        while pen < x + width {
            let bar = (1 + hash(seed, i, 1) % 3) as f32 * module;
            let space = (1 + hash(seed, i, 2) % 3) as f32 * module;
            self.rect(pen, y, bar.min(x + width - pen), 0.8, |_, _| 1.0);
            pen += bar + space;
            i += 1;
        }
    }

    /// Exposure blur and halation.
    fn develop(&mut self) {
        let (w, h) = (self.width, self.height);
        blur(&mut self.ink, w, h, INK_BLUR * self.scale);
        self.glow = self.ink.clone();
        blur(&mut self.glow, w, h, INK_GLOW * self.scale);
    }

    /// Ink and glow at strip coordinates in mm.
    fn sample(&self, u: f32, v: f32) -> (f32, f32) {
        let (x, y) = (u * self.scale - 0.5, (v - self.top) * self.scale - 0.5);
        if x < -1.0 || y < -1.0 || x > self.width as f32 || y > self.height as f32 {
            return (0.0, 0.0);
        }
        (
            bilinear(&self.ink, self.width, self.height, x, y),
            bilinear(&self.glow, self.width, self.height, x, y),
        )
    }
}

fn bilinear(data: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |x: f32, y: f32| {
        if x < 0.0 || y < 0.0 || x as usize >= w || y as usize >= h {
            0.0
        } else {
            data[y as usize * w + x as usize]
        }
    };
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
    let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Approximately Gaussian blur, `sigma` in pixels: three box passes.
fn blur(values: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.4 {
        return;
    }
    // Three boxes of radius r have variance r(r+1).
    let r = ((0.25 + sigma * sigma).sqrt() - 0.5).round().max(1.0) as usize;
    let mut line = Vec::new();
    for _ in 0..3 {
        for row in values.chunks_exact_mut(w) {
            box_blur(row, r, &mut line);
        }
        let mut column = vec![0.0; h];
        for x in 0..w {
            for (y, c) in column.iter_mut().enumerate() {
                *c = values[y * w + x];
            }
            box_blur(&mut column, r, &mut line);
            for (y, c) in column.iter().enumerate() {
                values[y * w + x] = *c;
            }
        }
    }
}

fn box_blur(row: &mut [f32], r: usize, line: &mut Vec<f32>) {
    line.clear();
    line.extend_from_slice(row);
    let n = row.len() as i64;
    let at = |i: i64| if (0..n).contains(&i) { line[i as usize] } else { 0.0 };
    let mut sum: f32 = (-(r as i64)..=r as i64).map(at).sum();
    for (i, out) in row.iter_mut().enumerate() {
        *out = sum / (2 * r + 1) as f32;
        let i = i as i64;
        sum += at(i + r as i64 + 1) - at(i - r as i64);
    }
}

/// A well-mixed 32-bit hash of three integers.
fn hash(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

/// Roughly unit-variance noise from a hash.
fn noise(a: u32, b: u32, c: u32) -> f32 {
    let h = hash(a, b, c);
    let (x, y) = ((h & 0xFFFF) as f32 / 65535.0, (h >> 16) as f32 / 65535.0);
    (x + y - 1.0) * 2.45
}

/// Draws one strip at one size. Rows can be rendered in parallel.
pub struct Renderer<'a> {
    strip: &'a Strip,
    width: u32,
    turned: bool,
    /// Pixels per mm along and across the strip.
    along: f32,
    across: f32,
    length: f32,
    gate: f32,
    perf: (f32, f32, f32),
    bands: [Band; 2],
    base: [f32; 3],
    ink: [f32; 3],
    dust: Option<[f32; 3]>,
    backlight: [f32; 3],
    grain: bool,
}

impl<'a> Renderer<'a> {
    fn new(strip: &'a Strip, width: u32, height: u32, turned: bool) -> Self {
        let (length, gate) = Strip::size(strip.format);
        let (along_px, across_px) = if turned { (height, width) } else { (width, height) };
        let (along, across) = (along_px as f32 / length, across_px as f32 / FILM_WIDTH);
        let scale = (along + across) / 2.0;
        let mut top = Band::new(0.0, PERF_EDGE, length, scale);
        let mut bottom = Band::new(FILM_WIDTH - PERF_EDGE, FILM_WIDTH, length, scale);
        let perf = match strip.print {
            EdgePrint::Cine { .. } => BH,
            _ => KS,
        };
        let seed = match strip.print {
            EdgePrint::Still(words) => words.iter().flat_map(|w| w.bytes()).fold(17, |h, b| hash(h, b as u32, 3)),
            EdgePrint::Cine { product, .. } | EdgePrint::Print(product) => {
                product.bytes().fold(29, |h, b| hash(h, b as u32, 5))
            }
        };
        edge_print(strip, &mut top, &mut bottom, length, seed);
        top.develop();
        bottom.develop();
        let (base, ink, dust) = strip.kind.colors();
        Self {
            strip,
            width,
            turned,
            along,
            across,
            length,
            gate,
            perf,
            bands: [top, bottom],
            base: linear(base),
            ink: linear(ink),
            dust: dust.map(linear),
            backlight: linear([247, 244, 236]),
            grain: true,
        }
    }

    /// Linear RGB and coverage for row `y`; `out` holds one entry per pixel.
    pub fn row(&self, y: u32, out: &mut [[f32; 4]]) {
        for (x, pixel) in out.iter_mut().enumerate().take(self.width as usize) {
            let (u, v) = if self.turned {
                (
                    self.length - (y as f32 + 0.5) / self.along,
                    (x as f32 + 0.5) / self.across,
                )
            } else {
                ((x as f32 + 0.5) / self.along, (y as f32 + 0.5) / self.across)
            };
            *pixel = self.shade(u, v, x as u32, y);
        }
    }

    fn shade(&self, u: f32, v: f32, x: u32, y: u32) -> [f32; 4] {
        let pixel = 1.0 / self.along.min(self.across);
        // The gate: transparent, with a soft edge.
        let gate = rounded_rect(
            u - self.length / 2.0,
            v - FILM_WIDTH / 2.0,
            self.gate / 2.0,
            GATE_HEIGHT / 2.0,
            GATE_RADIUS,
        );
        let softness = GATE_SOFTNESS.max(pixel);
        let alpha = (gate / softness + 0.5).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return [0.0; 4];
        }

        // Grain: clumps of GRAIN mm, averaged over larger pixels, partly
        // shared between the colour layers.
        let strength = if self.grain { GRAIN_STRENGTH * (GRAIN / pixel).min(1.0) } else { 0.0 };
        let sample = |channel: u32| {
            if pixel >= GRAIN {
                noise(x, y, channel)
            } else {
                value_noise(u / GRAIN, v / GRAIN, channel)
            }
        };
        let shared = sample(11);
        let grain: [f32; 3] =
            std::array::from_fn(|c| (strength * (0.7 * shared + 0.5 * sample(c as u32))).exp());

        // The film base, with the scan's uneven density.
        let (speck, variation) = self.material(u, v);
        let lift = (VARIATION_GAIN * variation).exp();
        let mut color: [f32; 3] = std::array::from_fn(|c| self.base[c] * lift * grain[c]);

        // Edge print, exposed into the emulsion.
        let band = &self.bands[usize::from(v > FILM_WIDTH / 2.0)];
        let (ink, glow) = band.sample(u, v);
        for c in 0..3 {
            let exposed = self.ink[c] * grain[c];
            color[c] = color[c] * (1.0 - ink) + exposed * ink + self.ink[c] * glow * INK_GLOW_STRENGTH;
        }

        // Dust on the film.
        if speck > 0.0 {
            let k = speck * DUST_OPACITY;
            for c in 0..3 {
                color[c] = match self.dust {
                    Some(dust) => color[c] * (1.0 - k) + dust[c] * k,
                    None => color[c] * (1.0 - k),
                };
            }
        }

        // Perforations: the scanner's backlight, bleeding a little into
        // the film around them.
        let (pw, ph, radius) = self.perf;
        let du = u - ((u / PERF_PITCH).floor() + 0.5) * PERF_PITCH;
        let centre = if v < FILM_WIDTH / 2.0 {
            PERF_EDGE + ph / 2.0
        } else {
            FILM_WIDTH - PERF_EDGE - ph / 2.0
        };
        let dv = v - centre;
        let distance = match self.strip.print {
            EdgePrint::Cine { .. } => bell_howell(du, dv, pw / 2.0, ph / 2.0, radius),
            _ => rounded_rect(du, dv, pw / 2.0, ph / 2.0, radius),
        };
        let hole = (0.5 - distance / pixel).clamp(0.0, 1.0);
        let bloom = BLOOM_STRENGTH * (-0.5 * (distance.max(0.0) / BLOOM).powi(2)).exp();
        for c in 0..3 {
            let lit = color[c] + self.backlight[c] * bloom;
            color[c] = lit * (1.0 - hole) + self.backlight[c] * hole;
        }
        [color[0], color[1], color[2], alpha]
    }

    /// Dust (0–1) and base density variation at strip coordinates.
    fn material(&self, u: f32, v: f32) -> (f32, f32) {
        let Some(material) = &self.strip.material else {
            return (0.0, 0.0);
        };
        // A half-frame shows the middle four perforations' worth.
        let offset = (MATERIAL_LENGTH - self.length) / 2.0;
        let (x, y) = (
            (u + offset) * MATERIAL_PX_PER_MM - 0.5,
            v * MATERIAL_PX_PER_MM - 0.5,
        );
        let (w, h) = (material.width() as i64, material.height() as i64);
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: i64, y: i64| material.get_pixel(x.clamp(0, w - 1) as u32, y.clamp(0, h - 1) as u32).0;
        let mut out = [0.0; 2];
        for (c, value) in out.iter_mut().enumerate() {
            let top = at(x0, y0)[c] as f32 * (1.0 - fx) + at(x0 + 1, y0)[c] as f32 * fx;
            let bottom = at(x0, y0 + 1)[c] as f32 * (1.0 - fx) + at(x0 + 1, y0 + 1)[c] as f32 * fx;
            *value = top * (1.0 - fy) + bottom * fy;
        }
        (out[0] / 255.0, (out[1] - 128.0) / 500.0)
    }
}

/// Smooth noise on a unit lattice, roughly unit variance.
fn value_noise(x: f32, y: f32, channel: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i32 as u32, y0 as i32 as u32);
    let n = |dx: u32, dy: u32| noise(ix.wrapping_add(dx), iy.wrapping_add(dy), channel + 100);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = n(0, 0) * (1.0 - sx) + n(1, 0) * sx;
    let bottom = n(0, 1) * (1.0 - sx) + n(1, 1) * sx;
    // Interpolation averages the lattice values; restore the variance.
    (top * (1.0 - sy) + bottom * sy) * 1.5
}

/// Signed distance from a point to a rounded rectangle centred at the
/// origin with half-extents `hx`, `hy`: negative inside.
fn rounded_rect(x: f32, y: f32, hx: f32, hy: f32, radius: f32) -> f32 {
    let qx = x.abs() - (hx - radius);
    let qy = y.abs() - (hy - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// Signed distance (approximately) to a Bell & Howell perforation: a slab
/// `hx` wide along the film whose ends across it are arcs of `radius`.
fn bell_howell(x: f32, y: f32, hx: f32, hy: f32, radius: f32) -> f32 {
    let arc = (x.hypot(y.abs() - (hy - radius))) - radius;
    (x.abs() - hx).max(arc)
}

/// Draws the edge print for `strip` into its two bands.
fn edge_print(strip: &Strip, top: &mut Band, bottom: &mut Band, length: f32, seed: u32) {
    let half = strip.format == FilmFormat::Half;
    let n = FRAME_NUMBER;
    match strip.print {
        EdgePrint::Still(words) => {
            if half {
                spread(top, words, 2.0, length - 1.0);
                bottom.arrow(2.0, BOTTOM_BASELINE, false);
                bottom.text(&format!("{n}A"), 2.8, BOTTOM_BASELINE, TEXT_SIZE);
                bottom.dx(8.0, 9.5, seed);
            } else {
                top.text(&n.to_string(), 2.4, TOP_BASELINE, TEXT_SIZE);
                spread(top, words, 8.0, length - 2.0);
                bottom.text(&n.to_string(), 2.4, BOTTOM_BASELINE, TEXT_SIZE);
                bottom.dx(6.4, 13.0, seed);
                bottom.arrow(20.4, BOTTOM_BASELINE, false);
                bottom.text(&format!("{n}A"), 21.2, BOTTOM_BASELINE, TEXT_SIZE);
                bottom.dx(25.6, 11.6, seed.rotate_left(7));
            }
        }
        EdgePrint::Cine { product, keykode } => {
            let manufacturing = format!("{product} 104 2309 11 174 2025");
            let mut pen = 1.2;
            if let Some(code) = keykode {
                let number = format!("{code} 41 2875 4391+{:02}", if half { 12 } else { 8 });
                pen = bottom.text(&number, pen, BOTTOM_BASELINE, SMALL_TEXT) + 0.8;
                let bars = if half { length - pen - 1.0 } else { 9.0 };
                bottom.barcode(pen, bars, seed);
                pen += bars + 1.2;
            }
            if !half || keykode.is_none() {
                bottom.text(&manufacturing, pen, BOTTOM_BASELINE, SMALL_TEXT);
            }
        }
        EdgePrint::Print(text) => {
            top.text(text, 2.0, TOP_BASELINE, TEXT_SIZE);
            if !half {
                top.text(text, 2.0 + length / 2.0, TOP_BASELINE, TEXT_SIZE);
            }
        }
    }
}

/// `words` along the top edge between `from` and `to`, evenly spaced.
fn spread(band: &mut Band, words: &[&str], from: f32, to: f32) {
    let widths: Vec<f32> = words.iter().map(|w| Band::measure(w, TEXT_SIZE)).collect();
    let gap = if words.len() > 1 {
        ((to - from - widths.iter().sum::<f32>()) / (words.len() - 1) as f32).clamp(1.5, 6.0)
    } else {
        0.0
    };
    let mut pen = from;
    for (word, width) in words.iter().zip(&widths) {
        band.text(word, pen, TOP_BASELINE, TEXT_SIZE);
        pen += width + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perforations_are_taller_than_wide() {
        // Along the film they are narrower than across it.
        assert!(KS.0 < KS.1 && BH.0 < BH.1);
        // Inside the perforation the distance is negative.
        assert!(rounded_rect(0.0, 0.0, KS.0 / 2.0, KS.1 / 2.0, KS.2) < 0.0);
        assert!(bell_howell(0.0, 0.0, BH.0 / 2.0, BH.1 / 2.0, BH.2) < 0.0);
        assert!(bell_howell(0.0, BH.1 / 2.0 + 0.01, BH.0 / 2.0, BH.1 / 2.0, BH.2) > 0.0);
        // The arcs cut the corners.
        assert!(bell_howell(BH.0 / 2.0 - 0.01, BH.1 / 2.0 - 0.01, BH.0 / 2.0, BH.1 / 2.0, BH.2) > 0.0);
    }

    #[test]
    fn blur_keeps_the_total() {
        let mut v = vec![0.0; 41 * 41];
        v[20 * 41 + 20] = 1.0;
        blur(&mut v, 41, 41, 2.0);
        assert!((v.iter().sum::<f32>() - 1.0).abs() < 1e-3);
        assert!(v[20 * 41 + 20] < 0.1);
    }

    #[test]
    fn grain_noise_is_roughly_unit_variance() {
        let n = 20_000;
        let var = (0..n).map(|i| noise(i, 7, 1).powi(2)).sum::<f32>() / n as f32;
        assert!((0.8..1.2).contains(&var), "{var}");
    }
}
