//! Experimental bounded Bayer development. No native/desktop caller opts in
//! implicitly. The full sensor is still decoded; scaling and demosaic RGB
//! are banded. This is NOT streaming JPEG decompression or export.
use anyhow::{Context, Result, ensure};
use rawler::{
    RawImage,
    imgop::{
        Rect, Point, Dim2,
        develop::Intermediate,
        sensor::{
            Demosaic, SensorType,
            bayer::ppg::PPGDemosaic,
            xtrans::markesteijn::XTransMarkesteijnDemosaic,
        },
    },
    pixarray::{Color2D, PixF32},
    rawimage::{CFAConfig, RawImageData, RawPhotometricInterpretation},
};
use rayon::prelude::*;
use spektrafilm_math::{image::ImageBuf, precision::{from_f32, to_f32}};
use crate::{RawWhiteBalance, white_balance};
use web_time::Instant;

pub const LARGE_RAW_PIXELS: usize = 40_000_000;
/// Mobile `Mode::AutoBayer` bands both 2×2 Bayer and 6×6 X-Trans CFA sensors
/// above this pixel count (e.g. 24 MP Sony, 26–40 MP Fuji X-Trans, 60 MP Leica)
/// so peak WASM memory stays ~200–400 MiB instead of 840–1,330 MiB.
pub const AUTO_BANDED_PIXELS: usize = 16_000_000;
const HALO: usize = 16;
/// 1-pass Markesteijn uses `pad_tile = 12` (`overlap = 24`); a 24 px halo keeps
/// every interior pixel strictly inside the valid demosaic region.
const XTRANS_HALO: usize = 24;
const OUTPUT_BAND_ROWS: usize = 32;
// Also bound source rows when a caller requests a much smaller copy. This
// prototype rejects a single output row spanning more than this, rather than
// accidentally materializing full RGB for e.g. a 60 MP -> 1x1 request.
const MAX_SOURCE_BAND_ROWS: usize = 128;

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Only >40 MP, capped RAWs are candidates. Unsupported large sensors
    /// return an error, never a potentially fatal full-RGB fallback.
    LargeBayer,
    /// Benchmark only: also band small Bayer sensors; unsupported ones fail.
    ForceBayer,
    /// Production mobile default: band >40 MP 2×2 RGB Bayer sensors (e.g.
    /// 60 MP Leica Q3: 406 MiB instead of 1,329 MiB), and seamlessly use the
    /// normal decoder (with bilinear X-Trans above 30 MP) for all other RAWs.
    AutoBayer,
}

pub struct KeptRaw {
    /// Unoriented, linear RGB, with the same cap/rounding as WASM's decode.
    pub image: ImageBuf,
    pub full_size: (u32, u32),
    pub orientation: u16,
    pub bounded: bool,
    pub decode_ms: f64,
    pub develop_ms: f64,
}

/// Oriented, colour-calibrated bounded RAW result for native file loaders.
pub(crate) struct KeptRawPath {
    pub image: ImageBuf,
    pub full_size: (u32, u32),
    pub lens_correction_supported: bool,
    pub decoder: &'static str,
}

/// Bounded RAW decode from a file path (no extra full-file byte copy in RAM).
/// Supports LibRaw fallback, lens corrections (scaled to the kept image), and
/// EXIF orientation applied to the small kept copy.
pub(crate) fn load_raw_path_kept(
    path: &std::path::Path,
    wb: RawWhiteBalance,
    lens: crate::LensCorrection,
    max_dim: u32,
    max_pixels: f64,
    mode: Mode,
) -> Result<KeptRawPath> {
    wb.validate()?;
    ensure!(max_pixels.is_finite() && max_pixels > 0.0, "bounded RAW requires a positive pixel cap");
    let src = rawler::rawsource::RawSource::new(path)
        .with_context(|| format!("opening RAW file: {}", path.display()))?;
    let params = rawler::decoders::RawDecodeParams::default();
    let (raw, libraw) = match rawler::decode(&src, &params) {
        Ok(raw) => (raw, None),
        #[cfg(all(feature = "libraw", not(target_arch = "wasm32")))]
        Err(e) => match crate::libraw::unpack(&src, crate::libraw::Metadata::Best) {
            Ok(unpacked) => (unpacked.raw, Some((unpacked.version, unpacked.orientation))),
            Err(other) => anyhow::bail!("RAW decode failed: {e:?}; {other}"),
        },
        #[cfg(not(all(feature = "libraw", not(target_arch = "wasm32"))))]
        Err(e) => anyhow::bail!("RAW decode failed: {e:?}"),
    };
    let profile = if libraw.is_none() { crate::lens::read_profile_from(&src) } else { None };
    let (decoder, orientation) = match libraw {
        #[cfg(all(feature = "libraw", not(target_arch = "wasm32")))]
        Some((_, Some(orientation))) => (crate::libraw::NAME, orientation),
        #[cfg(all(feature = "libraw", not(target_arch = "wasm32")))]
        Some((_, None)) => (crate::libraw::NAME, crate::raw_exif_orientation(&src, &params)),
        #[cfg(not(all(feature = "libraw", not(target_arch = "wasm32"))))]
        Some(_) => unreachable!(),
        None => ("rawler", crate::raw_exif_orientation(&src, &params)),
    };
    // Release the file source before allocating sensor/band buffers.
    drop(src);
    let area = crate::lens_area(&raw);
    let estimate = raw.crop_area.or(raw.active_area).map(|r| r.d).unwrap_or(raw.dim());
    let pixels = estimate.w.saturating_mul(estimate.h).max(raw.width.saturating_mul(raw.height));
    let selected = match mode {
        Mode::ForceBayer => true,
        Mode::LargeBayer => pixels > LARGE_RAW_PIXELS,
        Mode::AutoBayer => pixels > AUTO_BANDED_PIXELS && cfa_geometry(&raw).is_ok(),
    };
    let (mut image, full) = if selected {
        let (_active, crop, is_xtrans) = if matches!(mode, Mode::AutoBayer) {
            cfa_geometry(&raw)?
        } else {
            let (a, c) = geometry(&raw)?;
            (a, c, false)
        };
        let full = (crop.d.w as u32, crop.d.h as u32);
        let target = kept_size(full.0, full.1, max_dim, max_pixels);
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            develop_bands_cfa(raw, wb, target, OUTPUT_BAND_ROWS, is_xtrans)
        }))
        .map_err(|p| anyhow::anyhow!("bounded RAW develop panicked: {}", crate::panic_message(&p)))??;
        (image, full)
    } else {
        let xtrans = if matches!(mode, Mode::AutoBayer) && pixels > 30_000_000 {
            crate::XTransDemosaic::Bilinear
        } else {
            crate::XTransDemosaic::Markesteijn
        };
        let image = crate::develop_decoded_with(raw, wb, xtrans)?;
        let full = (image.width, image.height);
        let target = kept_size(full.0, full.1, max_dim, max_pixels);
        let image = if target == full { image } else { downsample_reference(&image, target) };
        (image, full)
    };
    if lens.enabled && let Some(profile) = &profile {
        let scaled_area = area
            .filter(|a| a.developed == (full.0 as usize, full.1 as usize))
            .map(|a| {
                let sx = image.width as f64 / full.0 as f64;
                let sy = image.height as f64 / full.1 as f64;
                [a.area[0] * sx, a.area[1] * sy, a.area[2] * sx, a.area[3] * sy]
            });
        crate::lens::apply(&mut image, profile, scaled_area);
    }
    let image = image.oriented(orientation);
    let full_size = if (5..=8).contains(&orientation) { (full.1, full.0) } else { full };
    Ok(KeptRawPath {
        image,
        full_size,
        lens_correction_supported: profile.is_some(),
        decoder,
    })
}

/// Explicit opt-in API, decoding the sensor only once even on the small-file
/// legacy route. Callers must orient the SMALL image after this returns.
pub fn load_raw_bytes_kept(bytes: &[u8], wb: RawWhiteBalance, max_dim: u32, max_pixels: f64, mode: Mode) -> Result<KeptRaw> {
    wb.validate()?;
    ensure!(max_pixels.is_finite() && max_pixels > 0.0, "bounded RAW requires a positive pixel cap");
    let src = rawler::rawsource::RawSource::new_from_slice(bytes);
    let params = Default::default();
    let start = Instant::now();
    let raw = rawler::decode(&src, &params).map_err(|e| anyhow::anyhow!("RAW decode failed: {e:?}"))?;
    let decode_ms = start.elapsed().as_secs_f64() * 1000.0;
    let orientation = crate::raw_exif_orientation(&src, &params);
    // Metadata no longer needs the copied file. In particular do not keep it
    // alongside the sensor + output or the small legacy control's resample.
    drop(src);
    let start = Instant::now();
    let estimate = raw.crop_area.or(raw.active_area).map(|r| r.d).unwrap_or(raw.dim());
    // A tiny default crop must not hide an expensive full sensor/active ROI.
    let pixels = estimate.w.saturating_mul(estimate.h).max(raw.width.saturating_mul(raw.height));
    let selected = match mode {
        Mode::ForceBayer => true,
        Mode::LargeBayer => pixels > LARGE_RAW_PIXELS,
        Mode::AutoBayer => pixels > AUTO_BANDED_PIXELS && cfa_geometry(&raw).is_ok(),
    };
    let (image, full_size) = if selected {
        let (_active, crop, is_xtrans) = if matches!(mode, Mode::AutoBayer) {
            cfa_geometry(&raw)?
        } else {
            let (a, c) = geometry(&raw)?;
            (a, c, false)
        };
        let full = (crop.d.w as u32, crop.d.h as u32);
        let target = kept_size(full.0, full.1, max_dim, max_pixels);
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            develop_bands_cfa(raw, wb, target, OUTPUT_BAND_ROWS, is_xtrans)
        })).map_err(|p| anyhow::anyhow!("bounded RAW develop panicked: {}", crate::panic_message(&p)))??;
        (image, full)
    } else {
        let xtrans = if matches!(mode, Mode::AutoBayer) && pixels > 30_000_000 {
            crate::XTransDemosaic::Bilinear
        } else {
            crate::XTransDemosaic::Markesteijn
        };
        let image = crate::develop_decoded_with(raw, wb, xtrans)?;
        let full = (image.width, image.height);
        let target = kept_size(full.0, full.1, max_dim, max_pixels);
        let image = if target == full { image } else { downsample_reference(&image, target) };
        (image, full)
    };
    let develop_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(KeptRaw { image, full_size, orientation, bounded: selected, decode_ms, develop_ms })
}

/// Kept-size policy copied deliberately, not a refactor of the default path.
/// Regression comparisons check it against the existing core/WASM policy.
pub fn kept_size(width: u32, height: u32, max_dim: u32, max_pixels: f64) -> (u32, u32) {
    let (long, short) = (width.max(height) as f64, width.min(height) as f64);
    let mut side = if max_dim == 0 { long } else { long.min(max_dim as f64) };
    if max_pixels > 0.0 && side * side * short / long > max_pixels {
        side = (max_pixels * long / short).sqrt().floor();
    }
    let scale = (side / long).min(1.0);
    (((width as f64 * scale).round() as u32).clamp(1, width),
     ((height as f64 * scale).round() as u32).clamp(1, height))
}

fn contained(inner: Rect, outer: Rect) -> bool {
    inner.d.w > 0 && inner.d.h > 0 && inner.p.x >= outer.p.x && inner.p.y >= outer.p.y
        && inner.p.x.checked_add(inner.d.w).is_some_and(|x| x <= outer.p.x + outer.d.w)
        && inner.p.y.checked_add(inner.d.h).is_some_and(|y| y <= outer.p.y + outer.d.h)
}

/// Match RawDevelop's active ROI, then its default crop/adapt rules. Reject
/// nonstandard metadata instead of guessing phase/crops or allocating full RGB.
fn geometry(raw: &RawImage) -> Result<(Rect, Rect)> {
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
        anyhow::bail!("bounded RAW supports only three-colour Bayer CFA (not linear RAW)");
    };
    ensure!(raw.cpp == 1 && config.sensor == SensorType::Bayer && config.cfa.is_rgb()
        && config.cfa.width == 2 && config.cfa.height == 2 && raw.fuji_rotation_width.is_none(),
        "bounded RAW supports only unrotated 2x2 RGB Bayer CFA");
    let sensor = Rect::new(Point::new(0, 0), raw.dim());
    let active = raw.active_area.unwrap_or(sensor);
    ensure!(contained(active, sensor) && active.d.w >= 7 && active.d.h >= 7, "unsupported bounded RAW active area");
    let crop = raw.crop_area.or(raw.active_area).unwrap_or(active);
    ensure!(contained(crop, active), "unsupported bounded RAW default crop");
    // RawDevelop skips a crop whose dimensions already equal the active ROI.
    // Containment above ensures that in that case its origin is equal too.
    Ok((active, crop))
}

/// Geometry check supporting both unrotated 2×2 RGB Bayer and 6×6 RGB X-Trans
/// sensors. Returns `(active, crop, is_xtrans)`.
fn cfa_geometry(raw: &RawImage) -> Result<(Rect, Rect, bool)> {
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
        anyhow::bail!("bounded RAW supports only three-colour Bayer/X-Trans CFA");
    };
    if config.sensor == SensorType::Bayer {
        let (active, crop) = geometry(raw)?;
        return Ok((active, crop, false));
    }
    ensure!(
        raw.cpp == 1
            && config.sensor == SensorType::Xtrans
            && config.cfa.is_rgb()
            && config.cfa.width == 6
            && config.cfa.height == 6
            && raw.fuji_rotation_width.is_none(),
        "bounded RAW supports only unrotated 2x2 RGB Bayer or 6x6 RGB X-Trans CFA"
    );
    let sensor = Rect::new(Point::new(0, 0), raw.dim());
    let active = raw.active_area.unwrap_or(sensor);
    ensure!(
        contained(active, sensor) && active.d.w >= 64 && active.d.h >= 64,
        "unsupported bounded X-Trans active area"
    );
    let crop = raw.crop_area.or(raw.active_area).unwrap_or(active);
    ensure!(contained(crop, active), "unsupported bounded X-Trans default crop");
    Ok((active, crop, true))
}

fn demosaic_cfa(sensor: &PixF32, config: &CFAConfig, scale_start: usize, roi: Rect, is_xtrans: bool) -> Color2D<f32, 3> {
    let cfa = config.cfa.shift(0, scale_start);
    if is_xtrans {
        XTransMarkesteijnDemosaic::new_pass_1().demosaic(sensor, &cfa, &config.colors, roi)
    } else {
        PPGDemosaic::new().demosaic(sensor, &cfa, &config.colors, roi)
    }
}

type Spans = Vec<(usize, Vec<f32>)>;
fn spans(src: usize, dst: usize) -> Spans {
    let scale = src as f64 / dst as f64;
    (0..dst).map(|i| {
        let (a, b) = (i as f64 * scale, ((i + 1) as f64 * scale).min(src as f64));
        let first = a.floor() as usize;
        let weights = (first..(b.ceil() as usize).min(src))
            .map(|j| ((b.min(j as f64 + 1.0) - a.max(j as f64)) / scale) as f32).collect();
        (first, weights)
    }).collect()
}

/// The accumulation order is important: vertical sum in f32 first, THEN
/// horizontal sum, with exactly preview::downsample_area's fractional spans.
fn reduce_rows(image: &ImageBuf, x: usize, y_origin: usize, columns: &Spans, rows: &[(usize, Vec<f32>)], output: &mut [spektrafilm_math::precision::Scalar]) {
    let sw = columns.last().map(|(x, w)| x + w.len()).unwrap();
    output.par_chunks_mut(columns.len() * 3).zip(rows.par_iter()).for_each(|(line, (y0, weights))| {
        let mut sum = vec![0f32; sw * 3];
        for (k, &w) in weights.iter().enumerate() {
            let start = ((y0 + k - y_origin) * image.width as usize + x) * 3;
            for (s, &v) in sum.iter_mut().zip(&image.data[start..start + sw * 3]) {
                *s += w * to_f32(v);
            }
        }
        for (x, (x0, weights)) in columns.iter().enumerate() {
            let mut px = [0f32; 3];
            for (k, &w) in weights.iter().enumerate() {
                for c in 0..3 { px[c] += w * sum[(x0 + k) * 3 + c]; }
            }
            for c in 0..3 { line[x * 3 + c] = from_f32(px[c]); }
        }
    });
}

pub(crate) fn downsample_reference(image: &ImageBuf, target: (u32, u32)) -> ImageBuf {
    let mut output = ImageBuf::from_data(target.0, target.1, vec![from_f32(0.0); target.0 as usize * target.1 as usize * 3]);
    reduce_rows(image, 0, 0, &spans(image.width as usize, target.0 as usize), &spans(image.height as usize, target.1 as usize), &mut output.data);
    output
}

#[cfg(test)]
fn develop_bands(raw: RawImage, wb: RawWhiteBalance, target: (u32, u32), band_rows: usize) -> Result<ImageBuf> {
    develop_bands_cfa(raw, wb, target, band_rows, false)
}

fn develop_bands_cfa(mut raw: RawImage, wb: RawWhiteBalance, target: (u32, u32), band_rows: usize, allow_xtrans: bool) -> Result<ImageBuf> {
    let (active, crop, is_xtrans) = if allow_xtrans {
        cfa_geometry(&raw)?
    } else {
        let (a, c) = geometry(&raw)?;
        (a, c, false)
    };
    let halo = if is_xtrans { XTRANS_HALO } else { HALO };
    ensure!(band_rows > 0 && target.0 > 0 && target.1 > 0 && target.0 as usize <= crop.d.w && target.1 as usize <= crop.d.h, "invalid bounded RAW target");
    white_balance::prepare(&mut raw, wb)?;
    let black = raw.blacklevel.as_bayer_array();
    let white = raw.whitelevel.as_bayer_array();
    let sensor_len = match &raw.data { RawImageData::Integer(v) => v.len(), RawImageData::Float(v) => v.len() };
    ensure!(sensor_len == raw.width * raw.height, "invalid bounded RAW mosaic length");
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else { unreachable!() };
    let columns = spans(crop.d.w, target.0 as usize);
    let rows = spans(crop.d.h, target.1 as usize);
    ensure!(rows.iter().all(|(_, weights)| weights.len() <= MAX_SOURCE_BAND_ROWS),
        "bounded RAW prototype does not support this extreme reduction ratio");
    let mut output = ImageBuf::from_data(target.0, target.1, vec![from_f32(0.0); target.0 as usize * target.1 as usize * 3]);
    let mut first = 0;
    while first < rows.len() {
        let mut end = (first + band_rows).min(rows.len());
        while end > first + 1 && rows[end - 1].0 + rows[end - 1].1.len() - rows[first].0 > MAX_SOURCE_BAND_ROWS {
            end -= 1;
        }
        let last = &rows[end - 1];
        let start_y = (crop.p.y + rows[first].0).saturating_sub(halo).max(active.p.y);
        let end_y = (crop.p.y + last.0 + last.1.len() + halo).min(active.p.y + active.d.h);
        // Scale full sensor-width row PAIRS, anchored to the original even
        // row. This reuses rawler's kernel exactly, including its untouched
        // odd trailing column/row. Cropping before scaling would change both
        // black-level phase and that legacy odd-dimension behavior.
        let scale_start = start_y & !1;
        let scale_end = ((end_y + 1) & !1).min(raw.height);
        let range = scale_start * raw.width..scale_end * raw.width;
        let data: Vec<f32> = match &raw.data {
            RawImageData::Integer(data) => data[range].iter().map(|&v| v as f32).collect(),
            RawImageData::Float(data) => data[range].to_vec(),
        };
        let mut sensor = PixF32::new_with(data, raw.width, scale_end - scale_start);
        rawler::imgop::raw::correct_blacklevel_cfa(&mut sensor.data, raw.width, scale_end - scale_start, &black, &white);
        let roi = Rect::new(Point::new(active.p.x, start_y - scale_start), Dim2::new(active.d.w, end_y - start_y));
        // Original phase (adjusted for the strip's sensor-row origin), then
        // demosaic shifts by the active ROI. Full active width preserves true
        // left/right borders. Halos exclude artificial top/bottom interpolation
        // except at the true active-area boundaries.
        let rgb = demosaic_cfa(&sensor, config, scale_start, roi, is_xtrans);
        drop(sensor);
        let mut rgb = crate::calibrate(&raw, Intermediate::ThreeColor(rgb));
        white_balance::finish(&mut rgb, wb, false)?;
        let image = crate::intermediate_to_image_buf(rgb);
        // Coordinates below are relative to the cropped image's source rows.
        let shifted_rows: Spans = rows[first..end].iter().map(|(y, w)| (crop.p.y + y - start_y, w.clone())).collect();
        reduce_rows(&image, crop.p.x - active.p.x, 0, &columns, &shifted_rows,
            &mut output.data[first * target.0 as usize * 3..end * target.0 as usize * 3]);
        first = end;
    }
    Ok(output)
}

#[cfg(test)]
mod tests;

/// Inverse of `ImageBuf::oriented(orientation)`: maps an upright coordinate
/// `(dx, dy)` back to the unoriented `(w × h)` coordinate `(sx, sy)`.
#[inline]
fn upright_to_unoriented(dx: usize, dy: usize, w: usize, h: usize, orientation: u16) -> (usize, usize) {
    match orientation {
        2 => (w - 1 - dx, dy),
        3 => (w - 1 - dx, h - 1 - dy),
        4 => (dx, h - 1 - dy),
        5 => (dy, dx),
        6 => (dy, h - 1 - dx),
        7 => (w - 1 - dy, h - 1 - dx),
        8 => (w - 1 - dy, dx),
        _ => (dx, dy),
    }
}

/// A decoded 2×2 RGB Bayer or 6×6 RGB X-Trans RAW kept as its compact CFA
/// mosaic (`u16` = 2 B/px, e.g. 52 MiB for 26 MP Fuji X-Trans or 115 MiB for
/// 60 MP Leica Q3 instead of 310–840 MiB f32 RGB/Markesteijn buffers), so
/// arbitrary upright rectangles can be demosaiced on demand for tiled
/// full-resolution export without ever materializing a full-frame f32 image.
pub struct BayerSensor {
    raw: RawImage,
    active: Rect,
    crop: Rect,
    wb: RawWhiteBalance,
    black: [f32; 4],
    white: [f32; 4],
    orientation: u16,
    is_xtrans: bool,
}

impl BayerSensor {
    /// Decode a Bayer or X-Trans RAW if it is a supported RGB CFA sensor (and
    /// at or above `min_pixels`). Returns `Ok(Some(sensor))` when handled as a
    /// compact sensor, or `Ok(None)` when the caller should use the standard
    /// full-frame RAW decoder (e.g. linear DNG, monochrome, or smaller files).
    pub fn try_open(bytes: &[u8], wb: RawWhiteBalance, min_pixels: usize) -> Result<Option<Self>> {
        wb.validate()?;
        let src = rawler::rawsource::RawSource::new_from_slice(bytes);
        let params = Default::default();
        let mut raw = rawler::decode(&src, &params).map_err(|e| anyhow::anyhow!("RAW decode failed: {e:?}"))?;
        let orientation = crate::raw_exif_orientation(&src, &params);
        drop(src);
        let estimate = raw.crop_area.or(raw.active_area).map(|r| r.d).unwrap_or(raw.dim());
        let pixels = estimate.w.saturating_mul(estimate.h).max(raw.width.saturating_mul(raw.height));
        if pixels < min_pixels {
            return Ok(None);
        }
        let Ok((active, crop, is_xtrans)) = cfa_geometry(&raw) else {
            return Ok(None);
        };
        let sensor_len = match &raw.data {
            RawImageData::Integer(v) => v.len(),
            RawImageData::Float(v) => v.len(),
        };
        if sensor_len != raw.width * raw.height {
            return Ok(None);
        }
        white_balance::prepare(&mut raw, wb)?;
        let black = raw.blacklevel.as_bayer_array();
        let white = raw.whitelevel.as_bayer_array();
        Ok(Some(Self { raw, active, crop, wb, black, white, orientation, is_xtrans }))
    }

    pub fn is_xtrans(&self) -> bool {
        self.is_xtrans
    }

    /// Upright full cropped dimensions `(width, height)`.
    pub fn full_size(&self) -> (u32, u32) {
        let (cw, ch) = (self.crop.d.w as u32, self.crop.d.h as u32);
        if (5..=8).contains(&self.orientation) { (ch, cw) } else { (cw, ch) }
    }

    /// Demosaic an upright rectangle `(ux0, uy0, w, h)` on demand. Coordinates
    /// past the right/bottom edges of `full_size()` are clamped to the edge
    /// pixel, matching tile padding without extra copies.
    pub fn demosaic_upright_rect(&self, ux0: u32, uy0: u32, w: u32, h: u32) -> Result<ImageBuf> {
        let (full_w, full_h) = self.full_size();
        let (full_w, full_h) = (full_w as usize, full_h as usize);
        let (cw, ch) = (self.crop.d.w, self.crop.d.h);
        ensure!(w > 0 && h > 0 && full_w > 0 && full_h > 0, "empty Bayer rect");

        let vx0 = (ux0 as usize).min(full_w - 1);
        let vy0 = (uy0 as usize).min(full_h - 1);
        let vx1 = ((ux0 as usize) + (w as usize) - 1).min(full_w - 1);
        let vy1 = ((uy0 as usize) + (h as usize) - 1).min(full_h - 1);

        let corners = [
            upright_to_unoriented(vx0, vy0, cw, ch, self.orientation),
            upright_to_unoriented(vx1, vy0, cw, ch, self.orientation),
            upright_to_unoriented(vx0, vy1, cw, ch, self.orientation),
            upright_to_unoriented(vx1, vy1, cw, ch, self.orientation),
        ];
        let cx_min = corners.iter().map(|c| c.0).min().unwrap();
        let cx_max = corners.iter().map(|c| c.0).max().unwrap();
        let cy_min = corners.iter().map(|c| c.1).min().unwrap();
        let cy_max = corners.iter().map(|c| c.1).max().unwrap();

        let halo = if self.is_xtrans { XTRANS_HALO } else { HALO };
        let start_y = (self.crop.p.y + cy_min).saturating_sub(halo).max(self.active.p.y);
        let end_y = (self.crop.p.y + cy_max + 1 + halo).min(self.active.p.y + self.active.d.h);
        let start_x = (self.crop.p.x + cx_min).saturating_sub(halo).max(self.active.p.x);
        let end_x = (self.crop.p.x + cx_max + 1 + halo).min(self.active.p.x + self.active.d.w);

        let scale_start = start_y & !1;
        let scale_end = ((end_y + 1) & !1).min(self.raw.height);
        let range = scale_start * self.raw.width..scale_end * self.raw.width;
        let data: Vec<f32> = match &self.raw.data {
            RawImageData::Integer(data) => data[range].iter().map(|&v| v as f32).collect(),
            RawImageData::Float(data) => data[range].to_vec(),
        };
        let mut sensor = PixF32::new_with(data, self.raw.width, scale_end - scale_start);
        rawler::imgop::raw::correct_blacklevel_cfa(
            &mut sensor.data,
            self.raw.width,
            scale_end - scale_start,
            &self.black,
            &self.white,
        );
        let roi = Rect::new(
            Point::new(start_x, start_y - scale_start),
            Dim2::new(end_x - start_x, end_y - start_y),
        );
        let RawPhotometricInterpretation::Cfa(config) = &self.raw.photometric else {
            unreachable!()
        };
        let rgb = demosaic_cfa(&sensor, config, scale_start, roi, self.is_xtrans);
        drop(sensor);
        let mut rgb = crate::calibrate(&self.raw, Intermediate::ThreeColor(rgb));
        white_balance::finish(&mut rgb, self.wb, false)?;
        let band = crate::intermediate_to_image_buf(rgb);
        let bw = band.width as usize;

        let (tw, th) = (w as usize, h as usize);
        let mut out = vec![from_f32(0.0); tw * th * 3];
        let (ox, oy) = (ux0 as usize, uy0 as usize);
        let (cpx, cpy) = (self.crop.p.x, self.crop.p.y);
        let orientation = self.orientation;
        out.par_chunks_exact_mut(tw * 3).enumerate().for_each(|(ly, row)| {
            let gy = (oy + ly).min(full_h - 1);
            for lx in 0..tw {
                let gx = (ox + lx).min(full_w - 1);
                let (cx, cy) = upright_to_unoriented(gx, gy, cw, ch, orientation);
                let rx = cpx + cx - start_x;
                let ry = cpy + cy - start_y;
                let si = (ry * bw + rx) * 3;
                row[lx * 3..lx * 3 + 3].copy_from_slice(&band.data[si..si + 3]);
            }
        });
        Ok(ImageBuf::from_data(w, h, out))
    }

    /// Build a small upright preview (`max_side` on the longest side, e.g. 256
    /// px for auto-exposure metering) in horizontal strips without allocating
    /// a full-frame f32 image.
    pub fn small_preview(&self, max_side: u32) -> Result<ImageBuf> {
        let (full_w, full_h) = self.full_size();
        let target = kept_size(full_w, full_h, max_side, 0.0);
        let (cw, ch) = (self.crop.d.w, self.crop.d.h);
        let unoriented_target = if (5..=8).contains(&self.orientation) {
            (target.1, target.0)
        } else {
            target
        };
        let columns = spans(cw, unoriented_target.0 as usize);
        let rows = spans(ch, unoriented_target.1 as usize);
        let mut output = ImageBuf::from_data(
            unoriented_target.0,
            unoriented_target.1,
            vec![from_f32(0.0); unoriented_target.0 as usize * unoriented_target.1 as usize * 3],
        );
        let RawPhotometricInterpretation::Cfa(config) = &self.raw.photometric else {
            unreachable!()
        };
        let halo = if self.is_xtrans { XTRANS_HALO } else { HALO };
        let mut first = 0;
        while first < rows.len() {
            let mut end = (first + OUTPUT_BAND_ROWS).min(rows.len());
            while end > first + 1 && rows[end - 1].0 + rows[end - 1].1.len() - rows[first].0 > MAX_SOURCE_BAND_ROWS {
                end -= 1;
            }
            let last = &rows[end - 1];
            let start_y = (self.crop.p.y + rows[first].0).saturating_sub(halo).max(self.active.p.y);
            let end_y = (self.crop.p.y + last.0 + last.1.len() + halo).min(self.active.p.y + self.active.d.h);
            let scale_start = start_y & !1;
            let scale_end = ((end_y + 1) & !1).min(self.raw.height);
            let range = scale_start * self.raw.width..scale_end * self.raw.width;
            let data: Vec<f32> = match &self.raw.data {
                RawImageData::Integer(data) => data[range].iter().map(|&v| v as f32).collect(),
                RawImageData::Float(data) => data[range].to_vec(),
            };
            let mut sensor = PixF32::new_with(data, self.raw.width, scale_end - scale_start);
            rawler::imgop::raw::correct_blacklevel_cfa(
                &mut sensor.data,
                self.raw.width,
                scale_end - scale_start,
                &self.black,
                &self.white,
            );
            let roi = Rect::new(
                Point::new(self.active.p.x, start_y - scale_start),
                Dim2::new(self.active.d.w, end_y - start_y),
            );
            let rgb = demosaic_cfa(&sensor, config, scale_start, roi, self.is_xtrans);
            drop(sensor);
            let mut rgb = crate::calibrate(&self.raw, Intermediate::ThreeColor(rgb));
            white_balance::finish(&mut rgb, self.wb, false)?;
            let image = crate::intermediate_to_image_buf(rgb);
            let shifted_rows: Spans = rows[first..end].iter().map(|(y, w)| (self.crop.p.y + y - start_y, w.clone())).collect();
            reduce_rows(
                &image,
                self.crop.p.x - self.active.p.x,
                0,
                &columns,
                &shifted_rows,
                &mut output.data[first * unoriented_target.0 as usize * 3..end * unoriented_target.0 as usize * 3],
            );
            first = end;
        }
        Ok(output.oriented(self.orientation))
    }
}
