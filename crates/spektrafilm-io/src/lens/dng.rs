//! DNG `OpcodeList3` lens corrections (DNG 1.6 specification, chapter 7):
//! `WarpRectilinear` (opcode 1: radial and tangential distortion, per plane,
//! so also lateral chromatic aberration) and `FixVignetteRadial` (opcode 3).
//! Both are defined on the linear reference image (the active area) with
//! radii normalised to the distance from the optical centre to the farthest
//! corner. `GainMap` (`OpcodeList2`, applied to sensor data before
//! demosaicing) is not handled.

use rawler::decoders::WellKnownIFD;
use rawler::formats::tiff::Value;
use rawler::rawsource::RawSource;

use super::{Area, Model, Profile, Source};

const OPCODE_LIST3: u16 = 51022;
const WARP_RECTILINEAR: u32 = 1;
const FIX_VIGNETTE_RADIAL: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Opcodes {
    pub warp: Option<Warp>,
    pub vignette: Option<Vignette>,
}

/// Per plane `[kr0, kr1, kr2, kr3, kt0, kt1]`; one plane applies to all.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Warp {
    pub planes: Vec<[f64; 6]>,
    /// Optical centre, normalised to the area.
    pub centre: (f64, f64),
}

/// Gain `1 + k0 r² + k1 r⁴ + k2 r⁶ + k3 r⁸ + k4 r¹⁰`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Vignette {
    pub k: [f64; 5],
    pub centre: (f64, f64),
}

pub(super) fn read(src: &RawSource) -> Option<Profile> {
    let decoder = rawler::get_decoder(src).ok()?;
    let tags = decoder.ifd(WellKnownIFD::VirtualDngRawTags).ok()??;
    let bytes = match &tags.get_entry(OPCODE_LIST3)?.value {
        Value::Undefined(b) | Value::Byte(b) => b.clone(),
        _ => return None,
    };
    let opcodes = parse(&bytes)?;
    Some(Profile { source: Source::Dng, model: Model::Dng(opcodes) })
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be_f64(b: &[u8], at: usize) -> Option<f64> {
    let v = f64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?);
    v.is_finite().then_some(v)
}

fn centre(b: &[u8], at: usize) -> Option<(f64, f64)> {
    let c = (be_f64(b, at)?, be_f64(b, at + 8)?);
    ((0.0..=1.0).contains(&c.0) && (0.0..=1.0).contains(&c.1)).then_some(c)
}

/// Parse a big-endian opcode list, keeping the first warp and vignette. An
/// opcode that does not parse is skipped, as are unknown ones.
pub(super) fn parse(b: &[u8]) -> Option<Opcodes> {
    let count = be_u32(b, 0)?;
    let mut at = 4;
    let mut out = Opcodes { warp: None, vignette: None };
    for _ in 0..count.min(256) {
        let id = be_u32(b, at)?;
        let size = be_u32(b, at + 12)? as usize;
        let body = b.get(at + 16..at + 16 + size)?;
        at += 16 + size;
        match id {
            WARP_RECTILINEAR if out.warp.is_none() => out.warp = warp(body),
            FIX_VIGNETTE_RADIAL if out.vignette.is_none() => out.vignette = vignette(body),
            _ => {}
        }
    }
    (out.warp.is_some() || out.vignette.is_some()).then_some(out)
}

fn warp(b: &[u8]) -> Option<Warp> {
    let n = be_u32(b, 0)? as usize;
    if n != 1 && n != 3 {
        return None;
    }
    let mut planes = Vec::with_capacity(n);
    for p in 0..n {
        let mut k = [0.0; 6];
        for (i, v) in k.iter_mut().enumerate() {
            *v = be_f64(b, 4 + 8 * (6 * p + i))?;
        }
        planes.push(k);
    }
    let centre = centre(b, 4 + 8 * 6 * n)?;
    let identity = planes.iter().all(|k| *k == [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    (!identity).then_some(Warp { planes, centre })
}

fn vignette(b: &[u8]) -> Option<Vignette> {
    let mut k = [0.0; 5];
    for (i, v) in k.iter_mut().enumerate() {
        *v = be_f64(b, 8 * i)?;
    }
    let centre = centre(b, 40)?;
    (k != [0.0; 5]).then_some(Vignette { k, centre })
}

/// Optical centre in image pixels and the normalising distance to the
/// farthest corner of `area`.
fn frame(c: (f64, f64), area: Area) -> (f64, f64, f64) {
    let [left, top, w, h] = area;
    let (cx, cy) = (left + c.0 * w, top + c.1 * h);
    let mx = (cx - left).abs().max((left + w - cx).abs());
    let my = (cy - top).abs().max((top + h - cy).abs());
    (cx, cy, mx.hypot(my).max(1e-9))
}

pub(super) struct Placed<'a> {
    warp: Option<(&'a Warp, f64, f64, f64)>,
    vignette: Option<(&'a Vignette, f64, f64, f64)>,
}

impl Opcodes {
    pub(super) fn place(&self, area: Area) -> Placed<'_> {
        Placed {
            warp: self.warp.as_ref().map(|w| {
                let (cx, cy, m) = frame(w.centre, area);
                (w, cx, cy, m)
            }),
            vignette: self.vignette.as_ref().map(|v| {
                let (cx, cy, m) = frame(v.centre, area);
                (v, cx, cy, m)
            }),
        }
    }
}

impl Placed<'_> {
    pub(super) fn gain(&self, x: f64, y: f64) -> f64 {
        let Some((v, cx, cy, m)) = self.vignette else { return 1.0 };
        let r2 = ((x - cx) / m).powi(2) + ((y - cy) / m).powi(2);
        let k = &v.k;
        1.0 + r2 * (k[0] + r2 * (k[1] + r2 * (k[2] + r2 * (k[3] + r2 * k[4]))))
    }

    pub(super) fn splits_planes(&self) -> bool {
        self.warp.is_some_and(|(w, ..)| w.planes.windows(2).any(|p| p[0] != p[1]))
    }

    pub(super) fn source(&self, plane: usize, x: f64, y: f64) -> (f64, f64) {
        let Some((w, cx, cy, m)) = self.warp else { return (x, y) };
        let k = &w.planes[plane.min(w.planes.len() - 1)];
        let (dx, dy) = ((x - cx) / m, (y - cy) / m);
        let r2 = dx * dx + dy * dy;
        let f = k[0] + r2 * (k[1] + r2 * (k[2] + r2 * k[3]));
        let tx = k[4] * 2.0 * dx * dy + k[5] * (r2 + 2.0 * dx * dx);
        let ty = k[5] * 2.0 * dx * dy + k[4] * (r2 + 2.0 * dy * dy);
        (cx + m * (f * dx + tx), cy + m * (f * dy + ty))
    }
}
