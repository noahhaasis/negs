//! DNG `GainMap` opcodes (`OpcodeList2` tag 51009 and `OpcodeList3` tag 51022;
//! DNG 1.6 specification, chapter 7, opcode 9).
//!
//! Android phones (Pixel, Samsung, Xiaomi, etc.) and Android's `DngCreator`
//! store their per-channel lens shading correction as `GainMap` opcodes in
//! `OpcodeList2` (one map per CFA site, row/column pitch 2), applied to the
//! black/white-level-scaled sensor data before demosaicing. Without them, phone
//! DNGs develop 1–1.6 EV dark in the corners with a yellow/green colour cast.
//!
//! Unlike user-toggleable geometric/vignette lens corrections (`crate::lens`),
//! `GainMap` opcodes are part of the sensor's radiometric linearization in the
//! DNG pipeline and are therefore applied unconditionally across all decode
//! paths (full-frame, banded preview, and compact tiled export), with zero
//! extra full-frame RAM overhead on banded and compact paths.

use rawler::{
    RawImage,
    decoders::WellKnownIFD,
    formats::tiff::Value,
    imgop::{Dim2, Point, Rect},
    pixarray::Color2D,
    rawimage::{RawImageData, RawPhotometricInterpretation},
    rawsource::RawSource,
};

const OPCODE_LIST2: u16 = 51009;
const OPCODE_LIST3: u16 = 51022;
const OPCODE_GAIN_MAP: u32 = 9;
const MAX_OPCODES: usize = 256;
const MAX_GAIN_COUNT: usize = 1 << 20;

/// Which DNG opcode list a [`GainMap`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpcodeStage {
    /// `OpcodeList2` (51009): applied after linearization/black-white scaling,
    /// before demosaicing (CFA 1-plane or `LinearRaw` 3-plane).
    List2,
    /// `OpcodeList3` (51022): applied after demosaicing (3-plane camera RGB
    /// before colour calibration, or `LinearRaw` 3-plane).
    List3,
}

/// A parsed and validated DNG `GainMap` opcode (DNG 1.6 §7, opcode 9).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GainMap {
    pub(crate) stage: OpcodeStage,
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
    plane: usize,
    planes: usize,
    row_pitch: usize,
    col_pitch: usize,
    points_v: usize,
    points_h: usize,
    spacing_v: f64,
    spacing_h: f64,
    origin_v: f64,
    origin_h: f64,
    map_planes: usize,
    gains: Vec<f32>,
}

/// Read all valid `GainMap` opcodes from a DNG's `OpcodeList2` and
/// `OpcodeList3` tags.
///
/// Returns an empty `Vec` immediately for non-DNG files (zero overhead on
/// NEF/ARW/CR3/RAF/ORF/RW2) and never panics on malformed files.
pub(crate) fn read_from(src: &RawSource) -> Vec<GainMap> {
    if !crate::raw::is_dng(src.buf()) {
        return Vec::new();
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Ok(decoder) = rawler::get_decoder(src) else {
            return Vec::new();
        };
        let Ok(Some(tags)) = decoder.ifd(WellKnownIFD::VirtualDngRawTags) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (tag, stage) in [
            (OPCODE_LIST2, OpcodeStage::List2),
            (OPCODE_LIST3, OpcodeStage::List3),
        ] {
            if let Some(entry) = tags.get_entry(tag)
                && let Value::Undefined(bytes) | Value::Byte(bytes) = &entry.value
            {
                out.extend(parse_stage(bytes, stage));
            }
        }
        out
    }))
    .unwrap_or_default()
}

/// Returns `true` if `path` is a CFA DNG containing `OpcodeList2` `GainMap`
/// opcodes. Used on macOS so `AppleRawDecoder` can defer to `rawler` instead
/// of letting `CIRAWFilter` silently drop Android per-CFA GainMaps.
///
/// Performs a tiny header-only TIFF check first so non-DNG files are rejected
/// without reading the file body.
#[cfg(all(target_os = "macos", feature = "apple-raw"))]
pub(crate) fn has_cfa_gain_maps(path: &std::path::Path) -> bool {
    if !crate::raw::is_dng_file(path) {
        return false;
    }
    let Ok(src) = RawSource::new(path) else {
        return false;
    };
    let maps = read_from(&src);
    if !maps.iter().any(|m| m.stage == OpcodeStage::List2 && m.plane == 0) {
        return false;
    }
    let is_linear_raw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let decoder = rawler::get_decoder(&src).ok()?;
        let raw_ifd = decoder.ifd(WellKnownIFD::Raw).ok()??;
        let photo = match &raw_ifd.get_entry(262)?.value {
            Value::Short(v) => v.first().copied().map(u32::from),
            Value::Long(v) => v.first().copied(),
            _ => None,
        }?;
        Some(photo == 34892)
    }))
    .ok()
    .flatten()
    .unwrap_or(false);
    !is_linear_raw
}

fn u32_raw_at(b: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_be_bytes(b.get(at..end)?.try_into().ok()?))
}

fn usize_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_raw_at(b, at)?).ok()
}

fn f64_at(b: &[u8], at: usize) -> Option<f64> {
    let end = at.checked_add(8)?;
    let v = f64::from_be_bytes(b.get(at..end)?.try_into().ok()?);
    v.is_finite().then_some(v)
}

fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    let end = at.checked_add(4)?;
    let v = f32::from_be_bytes(b.get(at..end)?.try_into().ok()?);
    (v.is_finite() && v >= 0.0).then_some(v)
}

/// Parse all valid `GainMap` opcodes from a big-endian DNG opcode list buffer.
/// Unknown opcodes and malformed maps are skipped safely (overflow-checked for
/// 32-bit WASM targets).
pub(crate) fn parse_stage(b: &[u8], stage: OpcodeStage) -> Vec<GainMap> {
    let mut out = Vec::new();
    let Some(count) = usize_at(b, 0) else {
        return out;
    };
    let mut at = 4usize;
    for _ in 0..count.min(MAX_OPCODES) {
        let Some(id) = u32_raw_at(b, at) else {
            break;
        };
        let Some(size_off) = at.checked_add(12) else {
            break;
        };
        let Some(size) = usize_at(b, size_off) else {
            break;
        };
        let Some(body_start) = at.checked_add(16) else {
            break;
        };
        let Some(body_end) = body_start.checked_add(size) else {
            break;
        };
        let Some(body) = b.get(body_start..body_end) else {
            break;
        };
        at = body_end;
        if id == OPCODE_GAIN_MAP
            && let Some(map) = parse_gain_map(body, stage)
        {
            out.push(map);
        }
    }
    out
}

fn parse_gain_map(b: &[u8], stage: OpcodeStage) -> Option<GainMap> {
    let top = usize_at(b, 0)?;
    let left = usize_at(b, 4)?;
    let bottom = usize_at(b, 8)?;
    let right = usize_at(b, 12)?;
    let plane = usize_at(b, 16)?;
    let planes = usize_at(b, 20)?;
    let row_pitch = usize_at(b, 24)?;
    let col_pitch = usize_at(b, 28)?;
    let points_v = usize_at(b, 32)?;
    let points_h = usize_at(b, 36)?;
    let spacing_v = f64_at(b, 40)?;
    let spacing_h = f64_at(b, 48)?;
    let origin_v = f64_at(b, 56)?;
    let origin_h = f64_at(b, 64)?;
    let map_planes = usize_at(b, 72)?;

    if top >= bottom
        || left >= right
        || planes == 0
        || row_pitch == 0
        || col_pitch == 0
        || points_v == 0
        || points_h == 0
        || map_planes == 0
        || (map_planes != 1 && map_planes != planes)
    {
        return None;
    }
    // Spacing must be strictly positive when an axis has >1 point; allow 0.0
    // (or any non-negative finite spacing) on a 1-point axis.
    if (points_v > 1 && spacing_v <= 0.0)
        || (points_v == 1 && spacing_v < 0.0)
        || (points_h > 1 && spacing_h <= 0.0)
        || (points_h == 1 && spacing_h < 0.0)
    {
        return None;
    }

    let n = points_v.checked_mul(points_h)?.checked_mul(map_planes)?;
    if n == 0 || n > MAX_GAIN_COUNT {
        return None;
    }
    let gains_bytes = n.checked_mul(4)?;
    let gains_end = 76usize.checked_add(gains_bytes)?;
    if b.len() < gains_end {
        return None;
    }

    let mut gains = Vec::with_capacity(n);
    for i in 0..n {
        gains.push(f32_at(b, 76 + i * 4)?);
    }

    Some(GainMap {
        stage,
        top,
        left,
        bottom,
        right,
        plane,
        planes,
        row_pitch,
        col_pitch,
        points_v,
        points_h,
        spacing_v,
        spacing_h,
        origin_v,
        origin_h,
        map_planes,
        gains,
    })
}

/// Map a normalized coordinate `rel` into `(i0, i1, frac)` clamped to
/// `[0, points - 1]`. Safe against non-finite intermediates.
#[inline(always)]
fn axis_index(rel: f64, origin: f64, spacing: f64, points: usize) -> (usize, usize, f32) {
    if points <= 1 {
        return (0, 0, 0.0);
    }
    let max_idx = (points - 1) as f64;
    let t = (rel - origin) / spacing;
    if !t.is_finite() {
        return if t == f64::INFINITY {
            (points - 1, points - 1, 0.0)
        } else {
            (0, 0, 0.0)
        };
    }
    if t <= 0.0 {
        return (0, 0, 0.0);
    }
    if t >= max_idx {
        return (points - 1, points - 1, 0.0);
    }
    let i = (t.floor() as usize).min(points - 2);
    (i, i + 1, (t - i as f64) as f32)
}

impl GainMap {
    /// Clamped opcode rectangle `[top..bottom, left..right]` inside `active`.
    #[inline(always)]
    fn clamped_bounds(&self, active: Rect) -> Option<(usize, usize, usize, usize)> {
        let bottom = self.bottom.min(active.d.h);
        let right = self.right.min(active.d.w);
        (self.top < bottom && self.left < right).then_some((self.top, self.left, bottom, right))
    }

    /// Bilinear interpolation at precomputed row/col indices for `map_plane`.
    #[inline(always)]
    fn sample_at(
        &self,
        r0: usize,
        r1: usize,
        fr: f32,
        c0: usize,
        c1: usize,
        fc: f32,
        map_plane: usize,
    ) -> f32 {
        let mp = self.map_planes;
        let row0 = r0 * self.points_h * mp + map_plane;
        let row1 = r1 * self.points_h * mp + map_plane;
        let one_minus_fc = 1.0 - fc;
        let top = self.gains[row0 + c0 * mp] * one_minus_fc + self.gains[row0 + c1 * mp] * fc;
        let bot = self.gains[row1 + c0 * mp] * one_minus_fc + self.gains[row1 + c1 * mp] * fc;
        top * (1.0 - fr) + bot * fr
    }
}

/// Active-area rectangle for `raw` (falls back to the full sensor rect).
#[inline]
pub(crate) fn active_rect(raw: &RawImage) -> Rect {
    raw.active_area
        .unwrap_or_else(|| Rect::new(Point::new(0, 0), Dim2::new(raw.width, raw.height)))
}

/// Whether any map in `maps` applies to `raw` right after black/white scaling
/// (before demosaicing for CFA, or all `List2` + `List3` maps for `LinearRaw`).
pub(crate) fn has_pre_demosaic(maps: &[GainMap], raw: &RawImage) -> bool {
    if maps.is_empty() {
        return false;
    }
    match &raw.photometric {
        RawPhotometricInterpretation::Cfa(_) if raw.cpp == 1 => {
            maps.iter().any(|m| m.stage == OpcodeStage::List2 && m.plane == 0)
        }
        RawPhotometricInterpretation::LinearRaw if raw.cpp == 3 => {
            maps.iter().any(|m| m.plane < 3)
        }
        _ => false,
    }
}

/// Whether any `OpcodeList3` `GainMap` applies to a CFA image after demosaic.
pub(crate) fn has_post_demosaic_cfa(maps: &[GainMap], raw: &RawImage) -> bool {
    raw.cpp == 1
        && matches!(raw.photometric, RawPhotometricInterpretation::Cfa(_))
        && maps.iter().any(|m| m.stage == OpcodeStage::List3 && m.plane < 3)
}

/// Apply `OpcodeList2` `GainMap`s on-the-fly to a scaled `f32` CFA strip
/// covering sensor rows `strip_y0..strip_y1` (width `sensor_width`) and sensor
/// columns `x_start..x_end`. Vertical interpolation is hoisted outside the
/// inner column loop. Output is left unclamped above 1.0.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_cfa_strip(
    maps: &[GainMap],
    strip: &mut [f32],
    sensor_width: usize,
    strip_y0: usize,
    strip_y1: usize,
    x_start: usize,
    x_end: usize,
    active: Rect,
) {
    if maps.is_empty() || strip_y0 >= strip_y1 || x_start >= x_end || sensor_width == 0 {
        return;
    }
    let strip_h = strip_y1 - strip_y0;
    if strip.len() < strip_h.saturating_mul(sensor_width) {
        return;
    }
    let x_end = x_end.min(sensor_width);

    for map in maps {
        if map.stage != OpcodeStage::List2 || map.plane != 0 {
            continue;
        }
        let Some((top, left, bottom, right)) = map.clamped_bounds(active) else {
            continue;
        };

        let row_lo = strip_y0.saturating_sub(active.p.y).max(top);
        let row_hi = strip_y1.saturating_sub(active.p.y).min(bottom);
        if row_lo >= row_hi {
            continue;
        }
        let rem_r = (row_lo - top) % map.row_pitch;
        let first_row = if rem_r == 0 {
            row_lo
        } else {
            row_lo + (map.row_pitch - rem_r)
        };
        if first_row >= row_hi {
            continue;
        }

        let col_lo = x_start.saturating_sub(active.p.x).max(left);
        let col_hi = x_end.saturating_sub(active.p.x).min(right);
        if col_lo >= col_hi {
            continue;
        }
        let rem_c = (col_lo - left) % map.col_pitch;
        let first_col = if rem_c == 0 {
            col_lo
        } else {
            col_lo + (map.col_pitch - rem_c)
        };
        if first_col >= col_hi {
            continue;
        }

        // DNG 1.6 §7 / Adobe DNG SDK `dng_gain_map_interpolator`: normalized
        // coordinates relative to the opcode's rectangle `[top..bottom, left..right]`.
        let inv_h = 1.0 / (bottom - top) as f64;
        let inv_w = 1.0 / (right - left) as f64;
        let mp = map.map_planes;

        for row in (first_row..row_hi).step_by(map.row_pitch) {
            let local_y = active.p.y + row - strip_y0;
            let row_slice = &mut strip[local_y * sensor_width..(local_y + 1) * sensor_width];
            let v = ((row - top) as f64 + 0.5) * inv_h;
            let (r0, r1, fr) = axis_index(v, map.origin_v, map.spacing_v, map.points_v);
            let row0_base = r0 * map.points_h * mp;
            let row1_base = r1 * map.points_h * mp;
            let one_minus_fr = 1.0 - fr;

            for col in (first_col..col_hi).step_by(map.col_pitch) {
                let h = ((col - left) as f64 + 0.5) * inv_w;
                let (c0, c1, fc) = axis_index(h, map.origin_h, map.spacing_h, map.points_h);
                let one_minus_fc = 1.0 - fc;
                let top_g =
                    map.gains[row0_base + c0 * mp] * one_minus_fc + map.gains[row0_base + c1 * mp] * fc;
                let bot_g =
                    map.gains[row1_base + c0 * mp] * one_minus_fc + map.gains[row1_base + c1 * mp] * fc;
                row_slice[active.p.x + col] *= top_g * one_minus_fr + bot_g * fr;
            }
        }
    }
}

/// Apply 3-channel `GainMap`s (`List2` and `List3` in order, for `LinearRaw`)
/// to an interleaved RGB `f32` rectangle covering sensor coordinates
/// `[rect_x0..rect_x0 + rect_w, rect_y0..rect_y0 + rect_h]`.
pub(crate) fn apply_rgb_rect(
    maps: &[GainMap],
    buf: &mut [f32],
    rect_x0: usize,
    rect_y0: usize,
    rect_w: usize,
    rect_h: usize,
    active: Rect,
) {
    apply_rgb_rect_filtered(maps, buf, rect_x0, rect_y0, rect_w, rect_h, active, None);
}

/// Apply `OpcodeList3` `GainMap`s to a demosaiced 3-channel CFA `Color2D`
/// covering sensor coordinates `(rect_x0, rect_y0)`.
pub(crate) fn apply_cfa_post_demosaic(
    maps: &[GainMap],
    rgb: &mut Color2D<f32, 3>,
    rect_x0: usize,
    rect_y0: usize,
    active: Rect,
) {
    if !maps.iter().any(|m| m.stage == OpcodeStage::List3 && m.plane < 3) {
        return;
    }
    let rect_w = rgb.width;
    let rect_h = rgb.height;
    let rect_x1 = rect_x0 + rect_w;
    let rect_y1 = rect_y0 + rect_h;

    for map in maps {
        if map.stage != OpcodeStage::List3 || map.plane >= 3 {
            continue;
        }
        let Some((top, left, bottom, right)) = map.clamped_bounds(active) else {
            continue;
        };

        let row_lo = rect_y0.saturating_sub(active.p.y).max(top);
        let row_hi = rect_y1.saturating_sub(active.p.y).min(bottom);
        if row_lo >= row_hi {
            continue;
        }
        let rem_r = (row_lo - top) % map.row_pitch;
        let first_row = if rem_r == 0 {
            row_lo
        } else {
            row_lo + (map.row_pitch - rem_r)
        };
        if first_row >= row_hi {
            continue;
        }

        let col_lo = rect_x0.saturating_sub(active.p.x).max(left);
        let col_hi = rect_x1.saturating_sub(active.p.x).min(right);
        if col_lo >= col_hi {
            continue;
        }
        let rem_c = (col_lo - left) % map.col_pitch;
        let first_col = if rem_c == 0 {
            col_lo
        } else {
            col_lo + (map.col_pitch - rem_c)
        };
        if first_col >= col_hi {
            continue;
        }

        let inv_h = 1.0 / (bottom - top) as f64;
        let inv_w = 1.0 / (right - left) as f64;
        let c_start = map.plane;
        let c_end = (map.plane + map.planes).min(3);

        for row in (first_row..row_hi).step_by(map.row_pitch) {
            let local_y = active.p.y + row - rect_y0;
            let row_base = local_y * rect_w;
            let v = ((row - top) as f64 + 0.5) * inv_h;
            let (r0, r1, fr) = axis_index(v, map.origin_v, map.spacing_v, map.points_v);

            for col in (first_col..col_hi).step_by(map.col_pitch) {
                let local_x = active.p.x + col - rect_x0;
                let h = ((col - left) as f64 + 0.5) * inv_w;
                let (c0, c1, fc) = axis_index(h, map.origin_h, map.spacing_h, map.points_h);
                let px = &mut rgb.data[row_base + local_x];
                for c in c_start..c_end {
                    let mp = if map.map_planes == 1 { 0 } else { c - c_start };
                    px[c] *= map.sample_at(r0, r1, fr, c0, c1, fc, mp);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_rgb_rect_filtered(
    maps: &[GainMap],
    buf: &mut [f32],
    rect_x0: usize,
    rect_y0: usize,
    rect_w: usize,
    rect_h: usize,
    active: Rect,
    only_stage: Option<OpcodeStage>,
) {
    if maps.is_empty() || rect_w == 0 || rect_h == 0 {
        return;
    }
    if buf.len() < rect_w.saturating_mul(rect_h).saturating_mul(3) {
        return;
    }
    let rect_x1 = rect_x0 + rect_w;
    let rect_y1 = rect_y0 + rect_h;

    for map in maps {
        if only_stage.is_some_and(|s| map.stage != s) || map.plane >= 3 {
            continue;
        }
        let Some((top, left, bottom, right)) = map.clamped_bounds(active) else {
            continue;
        };

        let row_lo = rect_y0.saturating_sub(active.p.y).max(top);
        let row_hi = rect_y1.saturating_sub(active.p.y).min(bottom);
        if row_lo >= row_hi {
            continue;
        }
        let rem_r = (row_lo - top) % map.row_pitch;
        let first_row = if rem_r == 0 {
            row_lo
        } else {
            row_lo + (map.row_pitch - rem_r)
        };
        if first_row >= row_hi {
            continue;
        }

        let col_lo = rect_x0.saturating_sub(active.p.x).max(left);
        let col_hi = rect_x1.saturating_sub(active.p.x).min(right);
        if col_lo >= col_hi {
            continue;
        }
        let rem_c = (col_lo - left) % map.col_pitch;
        let first_col = if rem_c == 0 {
            col_lo
        } else {
            col_lo + (map.col_pitch - rem_c)
        };
        if first_col >= col_hi {
            continue;
        }

        let inv_h = 1.0 / (bottom - top) as f64;
        let inv_w = 1.0 / (right - left) as f64;
        let c_start = map.plane;
        let c_end = (map.plane + map.planes).min(3);

        for row in (first_row..row_hi).step_by(map.row_pitch) {
            let local_y = active.p.y + row - rect_y0;
            let row_slice = &mut buf[local_y * rect_w * 3..(local_y + 1) * rect_w * 3];
            let v = ((row - top) as f64 + 0.5) * inv_h;
            let (r0, r1, fr) = axis_index(v, map.origin_v, map.spacing_v, map.points_v);

            for col in (first_col..col_hi).step_by(map.col_pitch) {
                let local_x = active.p.x + col - rect_x0;
                let h = ((col - left) as f64 + 0.5) * inv_w;
                let (c0, c1, fc) = axis_index(h, map.origin_h, map.spacing_h, map.points_h);
                let base = local_x * 3;
                for c in c_start..c_end {
                    let mp = if map.map_planes == 1 { 0 } else { c - c_start };
                    row_slice[base + c] *= map.sample_at(r0, r1, fr, c0, c1, fc, mp);
                }
            }
        }
    }
}

/// Apply 3-channel `GainMap`s (`List2` and `List3` in order) to a single
/// `LinearRaw` pixel at sensor coordinates `(sx, sy)`. Applies each map in
/// sequential multiplication order so rounding is bit-for-bit identical to
/// [`apply_rgb_rect`].
#[inline]
pub(crate) fn apply_rgb_pixel(
    maps: &[GainMap],
    px: &mut [f32; 3],
    sx: usize,
    sy: usize,
    active: Rect,
) {
    if sx < active.p.x || sy < active.p.y {
        return;
    }
    let row = sy - active.p.y;
    let col = sx - active.p.x;
    for map in maps {
        if map.plane >= 3 {
            continue;
        }
        let Some((top, left, bottom, right)) = map.clamped_bounds(active) else {
            continue;
        };
        if row < top
            || row >= bottom
            || col < left
            || col >= right
            || (row - top) % map.row_pitch != 0
            || (col - left) % map.col_pitch != 0
        {
            continue;
        }
        let v = ((row - top) as f64 + 0.5) * (1.0 / (bottom - top) as f64);
        let h = ((col - left) as f64 + 0.5) * (1.0 / (right - left) as f64);
        let (r0, r1, fr) = axis_index(v, map.origin_v, map.spacing_v, map.points_v);
        let (c0, c1, fc) = axis_index(h, map.origin_h, map.spacing_h, map.points_h);
        let c_start = map.plane;
        let c_end = (map.plane + map.planes).min(3);
        for c in c_start..c_end {
            let mp = if map.map_planes == 1 { 0 } else { c - c_start };
            px[c] *= map.sample_at(r0, r1, fr, c0, c1, fc, mp);
        }
    }
}

/// Evaluate the combined 3-channel gain multiplier at sensor coordinates
/// `(sx, sy)` for `LinearRaw` images.
#[allow(dead_code)]
pub(crate) fn gain_rgb(maps: &[GainMap], sx: usize, sy: usize, active: Rect) -> [f32; 3] {
    let mut g = [1.0f32; 3];
    apply_rgb_pixel(maps, &mut g, sx, sy, active);
    g
}

/// Apply pre-demosaic `GainMap`s in place to an already-scaled (`RawImageData::Float`)
/// full-frame `RawImage`.
///
/// - CFA (`cpp == 1`): applies `OpcodeList2` maps (`plane == 0`).
/// - `LinearRaw` (`cpp == 3`): applies `OpcodeList2` and `OpcodeList3` maps in order.
pub(crate) fn apply_full(maps: &[GainMap], raw: &mut RawImage) {
    if maps.is_empty() {
        return;
    }
    let active = active_rect(raw);
    let (w, h) = (raw.width, raw.height);
    let RawImageData::Float(data) = &mut raw.data else {
        return;
    };
    match &raw.photometric {
        RawPhotometricInterpretation::Cfa(_) if raw.cpp == 1 => {
            apply_cfa_strip(maps, data, w, 0, h, 0, w, active);
        }
        RawPhotometricInterpretation::LinearRaw if raw.cpp == 3 => {
            apply_rgb_rect(maps, data, 0, 0, w, h, active);
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    /// Build a raw `GainMap` opcode body (big-endian) for tests.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_gain_map_body(
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
        plane: u32,
        planes: u32,
        row_pitch: u32,
        col_pitch: u32,
        points_v: u32,
        points_h: u32,
        spacing_v: f64,
        spacing_h: f64,
        origin_v: f64,
        origin_h: f64,
        map_planes: u32,
        gains: &[f32],
    ) -> Vec<u8> {
        let mut body = Vec::with_capacity(76 + gains.len() * 4);
        for v in [
            top, left, bottom, right, plane, planes, row_pitch, col_pitch, points_v, points_h,
        ] {
            body.extend(v.to_be_bytes());
        }
        for v in [spacing_v, spacing_h, origin_v, origin_h] {
            body.extend(v.to_be_bytes());
        }
        body.extend(map_planes.to_be_bytes());
        for &g in gains {
            body.extend(g.to_be_bytes());
        }
        body
    }

    /// Wrap raw opcode `(id, body)` pairs into a big-endian DNG OpcodeList.
    pub(crate) fn encode_opcode_list(opcodes: &[(u32, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((opcodes.len() as u32).to_be_bytes());
        for &(id, body) in opcodes {
            out.extend(id.to_be_bytes());
            out.extend(0x0103_0000u32.to_be_bytes()); // DNG 1.3.0.0
            out.extend(0u32.to_be_bytes()); // flags
            out.extend((body.len() as u32).to_be_bytes());
            out.extend(body);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;

    fn cfa_opcode_list(maps: &[(u32, u32, u32, [f32; 4])], extra_opcode: bool) -> Vec<u8> {
        let mut bodies: Vec<(u32, Vec<u8>)> = Vec::new();
        if extra_opcode {
            bodies.push((99, vec![0u8; 8]));
        }
        for &(top, left, pitch, gains) in maps {
            bodies.push((
                OPCODE_GAIN_MAP,
                encode_gain_map_body(
                    top, left, 4, 4, 0, 1, pitch, pitch, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &gains,
                ),
            ));
        }
        let refs: Vec<(u32, &[u8])> = bodies.iter().map(|(id, b)| (*id, b.as_slice())).collect();
        encode_opcode_list(&refs)
    }

    #[test]
    fn parses_gain_maps_and_skips_other_opcodes_and_truncated_tail() {
        let list = cfa_opcode_list(&[(0, 0, 2, [1.0, 2.0, 3.0, 4.0]), (1, 1, 2, [1.5; 4])], true);
        let maps = parse_stage(&list, OpcodeStage::List2);
        assert_eq!(maps.len(), 2);
        assert_eq!((maps[1].top, maps[1].left, maps[1].row_pitch), (1, 1, 2));
        assert_eq!(maps[0].gains, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            parse_stage(&list[..list.len() - 3], OpcodeStage::List2).len(),
            1,
            "a truncated map is dropped without losing earlier valid maps"
        );
    }

    #[test]
    fn rejects_malformed_parameters_and_overflow_sizes() {
        // 1. 32-bit overflow in param_size (safe on wasm32)
        let mut hostile = Vec::new();
        hostile.extend(1u32.to_be_bytes());
        hostile.extend(OPCODE_GAIN_MAP.to_be_bytes());
        hostile.extend(0x0103_0000u32.to_be_bytes());
        hostile.extend(0u32.to_be_bytes());
        hostile.extend(u32::MAX.to_be_bytes());
        hostile.extend([0u8; 32]);
        assert!(parse_stage(&hostile, OpcodeStage::List2).is_empty());

        // 2. Planes == 0
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 0, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.0; 4]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());

        // 3. Zero row_pitch or col_pitch
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 0, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.0; 4]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 1, 0, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.0; 4]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());

        // 4. Negative or NaN gain
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.0, -0.1, 1.0, 1.0]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.0, f32::NAN, 1.0, 1.0]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());

        // 5. MapPlanes mismatch (planes = 3, map_planes = 2)
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 3, 1, 1, 1, 1, 0.0, 0.0, 0.0, 0.0, 2, &[1.0, 1.0]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());

        // 6. spacing == 0.0 rejected when points > 1
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 1, 1, 2, 1, 0.0, 0.0, 0.0, 0.0, 1, &[1.0, 2.0]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());

        // 7. Empty rectangle (top >= bottom)
        let b = encode_gain_map_body(4, 0, 4, 4, 0, 1, 1, 1, 1, 1, 0.0, 0.0, 0.0, 0.0, 1, &[2.0]);
        assert!(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2).is_empty());
    }

    #[test]
    fn allows_zero_spacing_on_one_point_axis() {
        let b = encode_gain_map_body(0, 0, 4, 4, 0, 1, 1, 1, 1, 2, 0.0, 1.0, 0.0, 0.0, 1, &[1.0, 3.0]);
        let maps = parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2);
        assert_eq!(maps.len(), 1);
        let mut strip = vec![1.0f32; 16];
        let active = Rect::new(Point::new(0, 0), Dim2::new(4, 4));
        apply_cfa_strip(&maps, &mut strip, 4, 0, 4, 0, 4, active);
        // Horizontal linear ramp from 1.0 to 3.0 at h = 0.125, 0.375, 0.625, 0.875, constant vertically.
        for y in 0..4 {
            assert!((strip[y * 4] - 1.25).abs() < 1e-6);
            assert!((strip[y * 4 + 1] - 1.75).abs() < 1e-6);
            assert!((strip[y * 4 + 2] - 2.25).abs() < 1e-6);
            assert!((strip[y * 4 + 3] - 2.75).abs() < 1e-6);
        }
    }

    #[test]
    fn applies_per_cfa_position_with_bilinear_gains_unclamped() {
        let maps = parse_stage(&cfa_opcode_list(&[(0, 0, 2, [1.0, 2.0, 3.0, 4.0])], false), OpcodeStage::List2);
        let mut data = vec![1.0f32; 16];
        let active = Rect::new(Point::new(0, 0), Dim2::new(4, 4));
        apply_cfa_strip(&maps, &mut data, 4, 0, 4, 0, 4, active);
        // (row 0, col 0): v = h = 0.125 -> 1 + 0.125*1 + 0.125*2 = 1.375 (unclamped > 1.0)
        assert!((data[0] - 1.375).abs() < 1e-6, "{}", data[0]);
        // (row 2, col 2): v = h = 0.625 -> 1 + 0.625*1 + 0.625*2 = 2.875
        assert!((data[2 * 4 + 2] - 2.875).abs() < 1e-6, "{}", data[10]);
        for i in [1, 3, 4, 5, 6, 7, 9, 11, 12, 13, 14, 15] {
            assert_eq!(data[i], 1.0, "site {i} must be untouched");
        }
    }

    #[test]
    fn honours_active_area_origin_and_sub_rectangle() {
        // 6×6 sensor, 4×4 active area at (2, 2); sub-rect [1..3, 1..3] inside active area.
        let b = encode_gain_map_body(1, 1, 3, 3, 0, 1, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[2.0, 4.0, 2.0, 4.0]);
        let maps = parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b)]), OpcodeStage::List2);
        let mut data = vec![1.0f32; 36];
        let active = Rect::new(Point::new(2, 2), Dim2::new(4, 4));
        apply_cfa_strip(&maps, &mut data, 6, 0, 6, 0, 6, active);
        // Sub-rect maps active rows 1..3, cols 1..3 -> sensor rows 3..5, cols 3..5.
        // At col=1 (first col of sub-rect): h = 0.5 / 2 = 0.25 -> gain = 2.0 + 0.25 * 2.0 = 2.5
        // At col=2 (second col of sub-rect): h = 1.5 / 2 = 0.75 -> gain = 2.0 + 0.75 * 2.0 = 3.5
        for sy in 0..6 {
            for sx in 0..6 {
                let got = data[sy * 6 + sx];
                let want = match (sy, sx) {
                    (3 | 4, 3) => 2.5,
                    (3 | 4, 4) => 3.5,
                    _ => 1.0,
                };
                assert!((got - want).abs() < 1e-6, "at ({sx},{sy}): got {got}, want {want}");
            }
        }
    }

    #[test]
    fn linear_rgb_supports_single_plane_and_three_plane_gain_maps() {
        // Map 1: MapPlanes = 1 across all 3 planes (uniform 1.5×)
        let b1 = encode_gain_map_body(0, 0, 4, 4, 0, 3, 1, 1, 1, 1, 0.0, 0.0, 0.0, 0.0, 1, &[1.5]);
        // Map 2: MapPlanes = 3 across planes 0..3 (per-channel [1.2, 1.0, 2.0])
        let b2 = encode_gain_map_body(0, 0, 4, 4, 0, 3, 1, 1, 1, 1, 0.0, 0.0, 0.0, 0.0, 3, &[1.2, 1.0, 2.0]);
        let mut maps = parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b1)]), OpcodeStage::List2);
        maps.extend(parse_stage(&encode_opcode_list(&[(OPCODE_GAIN_MAP, &b2)]), OpcodeStage::List3));

        let active = Rect::new(Point::new(0, 0), Dim2::new(4, 4));
        let mut buf = vec![0.5f32; 4 * 4 * 3];
        apply_rgb_rect(&maps, &mut buf, 0, 0, 4, 4, active);

        for y in 0..4 {
            for x in 0..4 {
                let i = (y * 4 + x) * 3;
                let mut px = [0.5f32; 3];
                apply_rgb_pixel(&maps, &mut px, x, y, active);
                assert_eq!(&buf[i..i + 3], &px[..]);
                assert!((px[0] - 0.5 * 1.5 * 1.2).abs() < 1e-6);
                assert!((px[1] - 0.5 * 1.5 * 1.0).abs() < 1e-6);
                assert!((px[2] - 0.5 * 1.5 * 2.0).abs() < 1e-6);
                let g = gain_rgb(&maps, x, y, active);
                assert!((g[0] - 1.8).abs() < 1e-6);
                assert!((g[1] - 1.5).abs() < 1e-6);
                assert!((g[2] - 3.0).abs() < 1e-6);
            }
        }
    }

    /// Build a minimal valid uncompressed 16-bit DNG (either RGGB Bayer CFA or
    /// 3-channel LinearRaw) readable by `rawler` and Negs, with optional
    /// `OpcodeList2`.
    fn build_synthetic_dng(
        width: u32,
        height: u32,
        linear_raw: bool,
        pixels: &[u16],
        opcode_list2: Option<&[u8]>,
    ) -> Vec<u8> {
        let cpp: u16 = if linear_raw { 3 } else { 1 };
        assert_eq!(pixels.len(), (width * height * u32::from(cpp)) as usize);

        // TIFF entry: (tag, typ, count, raw_value_bytes)
        // typ: 1=BYTE, 2=ASCII, 3=SHORT, 4=LONG, 5=RATIONAL, 7=UNDEFINED, 10=SRATIONAL
        let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();
        let u16s = |vals: &[u16]| vals.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let u32s = |vals: &[u32]| vals.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let rats = |vals: &[(u32, u32)]| {
            vals.iter()
                .flat_map(|&(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat())
                .collect::<Vec<u8>>()
        };
        let srats = |vals: &[(i32, i32)]| {
            vals.iter()
                .flat_map(|&(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat())
                .collect::<Vec<u8>>()
        };

        entries.push((254, 4, 1, u32s(&[0]))); // NewSubfileType = full-res raw
        entries.push((256, 4, 1, u32s(&[width]))); // ImageWidth
        entries.push((257, 4, 1, u32s(&[height]))); // ImageLength
        if linear_raw {
            entries.push((258, 3, 3, u16s(&[16, 16, 16]))); // BitsPerSample
        } else {
            entries.push((258, 3, 1, u16s(&[16])));
        }
        entries.push((259, 3, 1, u16s(&[1]))); // Compression = Uncompressed
        entries.push((262, 3, 1, u16s(&[if linear_raw { 34892 } else { 32803 }]))); // Photometric
        entries.push((271, 2, 7, b"Google\0".to_vec())); // Make
        entries.push((272, 2, 12, b"Pixel 9 Pro\0".to_vec())); // Model
        entries.push((273, 4, 1, u32s(&[0]))); // StripOffsets (patched below)
        entries.push((274, 3, 1, u16s(&[1]))); // Orientation
        entries.push((277, 3, 1, u16s(&[cpp]))); // SamplesPerPixel
        entries.push((278, 4, 1, u32s(&[height]))); // RowsPerStrip
        entries.push((279, 4, 1, u32s(&[(pixels.len() * 2) as u32]))); // StripByteCounts
        entries.push((284, 3, 1, u16s(&[1]))); // PlanarConfiguration
        if !linear_raw {
            entries.push((33421, 3, 2, u16s(&[2, 2]))); // CFARepeatPatternDim
            entries.push((33422, 1, 4, vec![0, 1, 1, 2])); // CFAPattern RGGB
        }
        entries.push((50706, 1, 4, vec![1, 4, 0, 0])); // DNGVersion 1.4.0.0
        entries.push((50707, 1, 4, vec![1, 3, 0, 0])); // DNGBackwardVersion
        entries.push((50708, 2, 12, b"Pixel 9 Pro\0".to_vec())); // UniqueCameraModel
        if linear_raw {
            entries.push((50714, 3, 3, u16s(&[1024, 1024, 1024]))); // BlackLevel
            entries.push((50717, 4, 3, u32s(&[16383, 16383, 16383]))); // WhiteLevel
        } else {
            entries.push((50714, 3, 1, u16s(&[1024]))); // BlackLevel
            entries.push((50717, 4, 1, u32s(&[16383]))); // WhiteLevel
        }
        // ColorMatrix1 (D65 XYZ -> camera RGB)
        entries.push((
            50721,
            10,
            9,
            srats(&[
                (7000, 10000),
                (-1000, 10000),
                (-500, 10000),
                (-2000, 10000),
                (11000, 10000),
                (1000, 10000),
                (500, 10000),
                (-1500, 10000),
                (9000, 10000),
            ]),
        ));
        entries.push((50728, 5, 3, rats(&[(1000, 1000), (1000, 1000), (1000, 1000)]))); // AsShotNeutral
        entries.push((50778, 3, 1, u16s(&[21]))); // CalibrationIlluminant1 = D65
        entries.push((50829, 4, 4, u32s(&[0, 0, height, width]))); // ActiveArea
        if let Some(op2) = opcode_list2 {
            entries.push((OPCODE_LIST2, 7, op2.len() as u32, op2.to_vec()));
        }
        entries.sort_by_key(|e| e.0);

        let ifd_offset = 8usize;
        let ifd_size = 2 + entries.len() * 12 + 4;
        let mut out = Vec::new();
        out.extend(b"II");
        out.extend(42u16.to_le_bytes());
        out.extend((ifd_offset as u32).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());

        let mut payload = Vec::new();
        let payload_base = ifd_offset + ifd_size;
        let mut strip_entry_val_pos = 0usize;

        for (tag, typ, count, data) in &entries {
            out.extend(tag.to_le_bytes());
            out.extend(typ.to_le_bytes());
            out.extend(count.to_le_bytes());
            if *tag == 273 {
                strip_entry_val_pos = out.len();
                out.extend(0u32.to_le_bytes());
            } else if data.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..data.len()].copy_from_slice(data);
                out.extend(inline);
            } else {
                // Align payload to 4 bytes
                while !payload.len().is_multiple_of(4) {
                    payload.push(0);
                }
                let off = (payload_base + payload.len()) as u32;
                out.extend(off.to_le_bytes());
                payload.extend_from_slice(data);
            }
        }
        out.extend(0u32.to_le_bytes()); // next IFD = 0
        out.extend_from_slice(&payload);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let strip_offset = out.len() as u32;
        out[strip_entry_val_pos..strip_entry_val_pos + 4].copy_from_slice(&strip_offset.to_le_bytes());
        for &p in pixels {
            out.extend(p.to_le_bytes());
        }
        out
    }

    #[test]
    fn end_to_end_synthetic_dng_cfa_and_linear_raw_gain_maps() {
        let (w, h) = (32u32, 24u32);
        // Uniform mid-grey sensor values (black=1024, white=16383 -> ~0.35 linear)
        let cfa_pixels = vec![6400u16; (w * h) as usize];
        // GainMap boosts corners 2.5× (and blue 3.2×) while center stays 1.0×
        let r_body = encode_gain_map_body(0, 0, h, w, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.5, 1.5, 2.5, 1.5, 1.0, 1.5, 2.5, 1.5, 2.5]);
        let g1_body = encode_gain_map_body(0, 1, h, w, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.4, 1.45, 2.4, 1.45, 1.0, 1.45, 2.4, 1.45, 2.4]);
        let g2_body = encode_gain_map_body(1, 0, h, w, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.42, 1.46, 2.42, 1.46, 1.01, 1.46, 2.42, 1.46, 2.42]);
        let b_body = encode_gain_map_body(1, 1, h, w, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[3.2, 1.8, 3.2, 1.8, 1.0, 1.8, 3.2, 1.8, 3.2]);
        let op2 = encode_opcode_list(&[
            (OPCODE_GAIN_MAP, &r_body),
            (OPCODE_GAIN_MAP, &g1_body),
            (OPCODE_GAIN_MAP, &g2_body),
            (OPCODE_GAIN_MAP, &b_body),
        ]);

        let dng_with = build_synthetic_dng(w, h, false, &cfa_pixels, Some(&op2));
        let dng_without = build_synthetic_dng(w, h, false, &cfa_pixels, None);

        let wb = crate::RawWhiteBalance::default();
        let img_with = crate::load_raw_bytes(&dng_with, wb).unwrap();
        let img_without = crate::load_raw_bytes(&dng_without, wb).unwrap();
        // Corner should be substantially brighter with GainMap applied (> 2×).
        let corner_without = img_without.get(1, 1)[1];
        let corner_with = img_with.get(1, 1)[1];
        assert!(corner_with > corner_without * 2.0, "corner_with={corner_with} vs corner_without={corner_without}");

        // Bounded ForceBayer and CompactRawSensor must match load_raw_bytes bit-for-bit!
        let kept = crate::bounded::load_raw_bytes_kept(&dng_with, wb, 0, 1e9, crate::bounded::Mode::ForceBayer).unwrap();
        assert_eq!(kept.image.data, img_with.data, "bounded CFA DNG mismatch");
        let compact = crate::bounded::CompactRawSensor::try_open_bytes(&dng_with, wb, 1).unwrap().unwrap();
        let rect = compact.extract_upright_rect(0, 0, w, h).unwrap();
        assert_eq!(rect.data, img_with.data, "CompactRawSensor CFA DNG mismatch");

        // LinearRaw DNG with 3-plane GainMap
        let lin_pixels = vec![6400u16; (w * h * 3) as usize];
        let lin_body = encode_gain_map_body(0, 0, h, w, 0, 3, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 3,
            &[2.0, 1.5, 2.5,  1.2, 1.1, 1.4,  1.3, 1.1, 1.5,  2.2, 1.6, 2.8]);
        let lin_op2 = encode_opcode_list(&[(OPCODE_GAIN_MAP, &lin_body)]);
        let lin_dng = build_synthetic_dng(w, h, true, &lin_pixels, Some(&lin_op2));
        let lin_full = crate::load_raw_bytes(&lin_dng, wb).unwrap();
        let lin_compact = crate::bounded::CompactRawSensor::try_open_bytes(&lin_dng, wb, 1).unwrap().unwrap();
        let lin_rect = lin_compact.extract_upright_rect(0, 0, w, h).unwrap();
        assert_eq!(lin_rect.data, lin_full.data, "CompactRawSensor LinearRaw DNG mismatch");

        // On macOS, verify AppleRawDecoder declines the CFA GainMap DNG (routing Auto to rawler),
        // while NOT falsely flagging the LinearRaw DNG as a CFA GainMap DNG.
        #[cfg(all(target_os = "macos", feature = "apple-raw"))]
        {
            let tmp = std::env::temp_dir().join("sf_test_gain_map_cfa.dng");
            let tmp_lin = std::env::temp_dir().join("sf_test_gain_map_lin.dng");
            std::fs::write(&tmp, &dng_with).unwrap();
            std::fs::write(&tmp_lin, &lin_dng).unwrap();
            assert!(has_cfa_gain_maps(&tmp));
            assert!(!has_cfa_gain_maps(&tmp_lin));
            let decoded = crate::raw::decode_raw(
                &tmp,
                crate::raw::RawDecodeOptions::default(),
                crate::raw::RawDecoderPreference::Auto,
            )
            .unwrap();
            assert_eq!(decoded.decoder, "rawler");
            assert_eq!(decoded.image.data, img_with.data);
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::remove_file(&tmp_lin);
        }
    }
}
