//! Lens corrections from the data the camera recorded in the RAW: distortion,
//! lateral chromatic aberration and vignetting, without a lens database.
//!
//! - DNG: `WarpRectilinear` and `FixVignetteRadial` from `OpcodeList3`, per
//!   the DNG 1.6 specification ([`dng`]).
//! - Sony ARW, Fujifilm RAF, OM/Olympus ORF and Panasonic RW2: their
//!   private correction tags, ported from darktable's embedded-metadata lens
//!   correction ([`vendor`]).
//!
//! Corrections act on the developed, linear, unoriented image (before the EXIF
//! orientation), so every model is defined in sensor coordinates. Vignetting is
//! a linear gain without an upper clamp; distortion and chromatic aberration
//! are one inverse-mapped bicubic resample, scaled so no empty corner remains.
//! See docs/lens-corrections-plan.md.

mod dng;
mod vendor;

use std::path::Path;

use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32, to_f32};

/// Where the correction data came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Dng,
    Sony,
    Fujifilm,
    Olympus,
    Panasonic,
}

impl Source {
    /// Short name for the interface ("DNG", "Sony", "Fujifilm").
    pub fn name(self) -> &'static str {
        match self {
            Self::Dng => "DNG",
            Self::Sony => "Sony",
            Self::Fujifilm => "Fujifilm",
            Self::Olympus => "OM/Olympus",
            Self::Panasonic => "Panasonic",
        }
    }
}

/// A file's lens corrections.
#[derive(Clone, Debug)]
pub struct Profile {
    pub source: Source,
    model: Model,
}

#[derive(Clone, Debug)]
enum Model {
    Dng(dng::Opcodes),
    Radial(Radial),
}

/// Radial corrections as darktable keeps them: values at knots of the
/// radius, normalised to the half diagonal of the image, about its centre,
/// interpolated linearly. `cor[plane]` is the source radius divided by the
/// corrected radius; a pixel is divided by `vig`.
#[derive(Clone, Debug, PartialEq)]
struct Radial {
    knots_dist: Vec<f64>,
    cor: [Vec<f64>; 3],
    knots_vig: Vec<f64>,
    vig: Vec<f64>,
}

/// Where the developed image lies in the frame a DNG profile describes (its
/// active area), in developed-image pixels: `[left, top, width, height]`.
/// Vendor profiles describe the developed image itself.
pub type Area = [f64; 4];

impl Profile {
    /// Whether the profile corrects distortion or chromatic aberration (so the
    /// image is resampled) rather than only vignetting.
    pub fn warps(&self) -> bool {
        match &self.model {
            Model::Dng(o) => o.warp.is_some(),
            Model::Radial(r) => r.cor.iter().any(|c| c.iter().any(|&v| v != 1.0)),
        }
    }

    fn vignettes(&self) -> bool {
        match &self.model {
            Model::Dng(o) => o.vignette.is_some(),
            Model::Radial(r) => r.vig.iter().any(|&v| v != 1.0),
        }
    }

    /// A profile that changes nothing is no profile.
    fn effective(self) -> Option<Self> {
        (self.warps() || self.vignettes()).then_some(self)
    }
}

/// The correction data in a RAW file, or `None` when it has none Negs can
/// apply. Reads metadata only. Unknown or malformed data yields `None`, never
/// an error: the photograph opens either way.
pub fn read_profile(path: &Path) -> Option<Profile> {
    let src = rawler::rawsource::RawSource::new(path).ok()?;
    read_profile_from(&src)
}

pub(crate) fn read_profile_from(src: &rawler::rawsource::RawSource) -> Option<Profile> {
    // rawler's parsers guard against malformed files with panics in places.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let buf = src.buf();
        if buf.starts_with(b"FUJIFILM") {
            return vendor::fujifilm(src);
        }
        // Panasonic's TIFF magic is 0x55; Olympus's is "RO" or "RS".
        if buf.starts_with(b"IIU\0") {
            return vendor::panasonic(src);
        }
        if matches!(buf.get(0..4), Some(b"IIRO" | b"IIRS" | b"MMOR")) {
            return vendor::olympus(src);
        }
        dng::read(src).or_else(|| vendor::sony(src))
    }))
    .ok()
    .flatten()
    .and_then(Profile::effective)
}

/// Apply `profile` to a developed, unoriented image in place. `area` places a
/// DNG profile's frame; `None` means the image itself.
pub fn apply(image: &mut ImageBuf, profile: &Profile, area: Option<Area>) {
    let (w, h) = (image.width as usize, image.height as usize);
    if w < 2 || h < 2 {
        return;
    }
    let lens = Prepared::new(profile, w as f64, h as f64, area);
    if profile.vignettes() {
        image.par_rows_mut().enumerate().for_each(|(y, row)| {
            let yc = y as f64 + 0.5;
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let g = lens.gain(x as f64 + 0.5, yc) as f32;
                for v in px {
                    *v = from_f32(to_f32(*v) * g);
                }
            }
        });
    }
    if profile.warps() {
        // DNG warps are applied as written, cropping in only to avoid empty
        // borders; darktable's vendor curves assume a fitted scale.
        let least = if matches!(profile.model, Model::Dng(_)) { 1.0 } else { 0.8 };
        let zoom = fill_zoom(&lens, w as f64, h as f64, least);
        image.data = resample(&image.data, w, h, &lens, zoom);
    }
}

/// A profile placed on an image of known size.
enum Prepared<'a> {
    Dng(dng::Placed<'a>),
    Radial { radial: &'a Radial, cx: f64, cy: f64, norm: f64 },
}

impl<'a> Prepared<'a> {
    fn new(profile: &'a Profile, w: f64, h: f64, area: Option<Area>) -> Self {
        match &profile.model {
            Model::Dng(o) => Self::Dng(o.place(area.unwrap_or([0.0, 0.0, w, h]))),
            Model::Radial(radial) => Self::Radial {
                radial,
                cx: w / 2.0,
                cy: h / 2.0,
                norm: (w / 2.0).hypot(h / 2.0),
            },
        }
    }

    /// Gain to multiply the uncorrected pixel at (x, y) by.
    fn gain(&self, x: f64, y: f64) -> f64 {
        match self {
            Self::Dng(d) => d.gain(x, y),
            Self::Radial { radial, cx, cy, norm } => {
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt() / norm;
                1.0 / spline(&radial.knots_vig, &radial.vig, r).max(1e-4)
            }
        }
    }

    /// Whether red, green and blue move differently (lateral CA).
    fn splits_planes(&self) -> bool {
        match self {
            Self::Dng(d) => d.splits_planes(),
            Self::Radial { radial, .. } => radial.cor[0] != radial.cor[1] || radial.cor[2] != radial.cor[1],
        }
    }

    /// Position in the uncorrected image that the corrected pixel (x, y) of
    /// `plane` (0 red, 1 green, 2 blue) shows.
    fn source(&self, plane: usize, x: f64, y: f64) -> (f64, f64) {
        match self {
            Self::Dng(d) => d.source(plane, x, y),
            Self::Radial { radial, cx, cy, norm } => {
                let (dx, dy) = (x - cx, y - cy);
                let dr = spline(&radial.knots_dist, &radial.cor[plane], (dx * dx + dy * dy).sqrt() / norm);
                (cx + dr * dx, cy + dr * dy)
            }
        }
    }
}

/// darktable's `_interpolate_linear_spline`: linear between knots, constant
/// outside them.
fn spline(xi: &[f64], yi: &[f64], x: f64) -> f64 {
    if x < xi[0] {
        return yi[0];
    }
    for i in 1..xi.len() {
        if x >= xi[i - 1] && x <= xi[i] {
            let span = xi[i] - xi[i - 1];
            return if span > 0.0 { yi[i - 1] + (x - xi[i - 1]) * (yi[i] - yi[i - 1]) / span } else { yi[i] };
        }
    }
    yi[yi.len() - 1]
}

/// The zoom about the image centre at which every border pixel of every
/// plane maps inside the uncorrected image: above 1 crops in (barrel
/// correction pulls the corners in), below 1 keeps more of the frame. Like
/// the camera, fill the frame rather than show empty corners.
fn fill_zoom(lens: &Prepared, w: f64, h: f64, least: f64) -> f64 {
    const STEPS: usize = 64;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut border = Vec::with_capacity(4 * STEPS);
    for i in 0..STEPS {
        let t = i as f64 / (STEPS - 1) as f64;
        let (x, y) = (0.5 + t * (w - 1.0), 0.5 + t * (h - 1.0));
        border.extend([(x, 0.5), (x, h - 0.5), (0.5, y), (w - 0.5, y)]);
    }
    let inside = |zoom: f64| {
        border.iter().all(|&(x, y)| {
            let (x, y) = (cx + (x - cx) / zoom, cy + (y - cy) / zoom);
            (0..3).all(|plane| {
                let (sx, sy) = lens.source(plane, x, y);
                sx >= 0.5 - 1e-6 && sx <= w - 0.5 + 1e-6 && sy >= 0.5 - 1e-6 && sy <= h - 0.5 + 1e-6
            })
        })
    };
    let (mut lo, mut hi) = (least, 1.5f64);
    if inside(lo) {
        return lo;
    }
    if !inside(hi) {
        return hi;
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if inside(mid) { hi = mid } else { lo = mid }
    }
    hi
}

/// Catmull-Rom weights for a sample at fractional offset `t` from the second
/// of four taps.
fn cubic_weights(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// Bicubic (Catmull-Rom) sample of all three channels at pixel-centre
/// coordinates (x, y), clamped to the image. No tone limit is imposed.
#[inline(always)]
fn sample(data: &[Scalar], w: usize, h: usize, x: f64, y: f64) -> [f32; 3] {
    let fx = (x - 0.5).clamp(0.0, (w - 1) as f64);
    let fy = (y - 0.5).clamp(0.0, (h - 1) as f64);
    let (ix, iy) = (fx as usize, fy as usize);
    let wx = cubic_weights((fx - ix as f64) as f32);
    let wy = cubic_weights((fy - iy as f64) as f32);
    // Weights for the 4 interleaved RGB taps of a row.
    let wx12: [f32; 12] = std::array::from_fn(|k| wx[k / 3]);
    let mut acc = [0.0f32; 12];
    if ix >= 1 && iy >= 1 && ix + 2 < w && iy + 2 < h {
        // Interior: four contiguous runs of 12 values, no clamping.
        for (j, wyj) in wy.iter().enumerate() {
            let at = ((iy + j - 1) * w + ix - 1) * 3;
            let taps: &[Scalar; 12] = data[at..at + 12].try_into().unwrap();
            for k in 0..12 {
                acc[k] += wyj * wx12[k] * to_f32(taps[k]);
            }
        }
    } else {
        for (j, wyj) in wy.iter().enumerate() {
            let row = (iy + j).saturating_sub(1).min(h - 1) * w;
            for i in 0..4 {
                let at = (row + (ix + i).saturating_sub(1).min(w - 1)) * 3;
                for c in 0..3 {
                    acc[i * 3 + c] += wyj * wx12[i * 3] * to_f32(data[at + c]);
                }
            }
        }
    }
    std::array::from_fn(|c| acc[c] + acc[3 + c] + acc[6 + c] + acc[9 + c])
}

fn resample(data: &[Scalar], w: usize, h: usize, lens: &Prepared, zoom: f64) -> Vec<Scalar> {
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let shared = !lens.splits_planes();
    let mut out = vec![from_f32(0.0); w * h * 3];
    out.par_chunks_exact_mut(w * 3).enumerate().for_each(|(y, row)| {
        let yq = cy + (y as f64 + 0.5 - cy) / zoom;
        for (x, px) in row.chunks_exact_mut(3).enumerate() {
            let xq = cx + (x as f64 + 0.5 - cx) / zoom;
            if shared {
                let (sx, sy) = lens.source(1, xq, yq);
                let rgb = sample(data, w, h, sx, sy);
                for (v, s) in px.iter_mut().zip(rgb) {
                    *v = from_f32(s);
                }
            } else {
                for (c, v) in px.iter_mut().enumerate() {
                    let (sx, sy) = lens.source(c, xq, yq);
                    *v = from_f32(sample(data, w, h, sx, sy)[c]);
                }
            }
        }
    });
    out
}

#[cfg(test)]
mod tests;
