//! Experimental bounded Bayer development. No native/desktop caller opts in
//! implicitly. The full sensor is still decoded; scaling and demosaic RGB
//! are banded. This is NOT streaming JPEG decompression or export.
use anyhow::{Result, ensure};
use rawler::{
    RawImage,
    imgop::{Rect, Point, Dim2, develop::Intermediate, sensor::{Demosaic, SensorType, bayer::ppg::PPGDemosaic}},
    pixarray::PixF32,
    rawimage::{RawImageData, RawPhotometricInterpretation},
};
use rayon::prelude::*;
use spektrafilm_math::{image::ImageBuf, precision::{from_f32, to_f32}};
use crate::{RawWhiteBalance, white_balance};
use web_time::Instant;

pub const LARGE_RAW_PIXELS: usize = 40_000_000;
const HALO: usize = 16;
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
    let selected = matches!(mode, Mode::ForceBayer) || pixels > LARGE_RAW_PIXELS;
    let (image, full_size) = if selected {
        let geometry = geometry(&raw)?;
        let full = (geometry.1.d.w as u32, geometry.1.d.h as u32);
        let target = kept_size(full.0, full.1, max_dim, max_pixels);
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            develop_bands(raw, wb, target, OUTPUT_BAND_ROWS)
        })).map_err(|p| anyhow::anyhow!("bounded RAW develop panicked: {}", crate::panic_message(&p)))??;
        (image, full)
    } else {
        let image = crate::develop_decoded(raw, wb)?;
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

fn downsample_reference(image: &ImageBuf, target: (u32, u32)) -> ImageBuf {
    let mut output = ImageBuf::from_data(target.0, target.1, vec![from_f32(0.0); target.0 as usize * target.1 as usize * 3]);
    reduce_rows(image, 0, 0, &spans(image.width as usize, target.0 as usize), &spans(image.height as usize, target.1 as usize), &mut output.data);
    output
}

fn develop_bands(mut raw: RawImage, wb: RawWhiteBalance, target: (u32, u32), band_rows: usize) -> Result<ImageBuf> {
    let (active, crop) = geometry(&raw)?;
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
        let start_y = (crop.p.y + rows[first].0).saturating_sub(HALO).max(active.p.y);
        let end_y = (crop.p.y + last.0 + last.1.len() + HALO).min(active.p.y + active.d.h);
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
        // PPG shifts by the active ROI. Full active width preserves true
        // left/right borders. 16-row halos exclude artificial top/bottom
        // interpolation except at the true active-area boundaries.
        let cfa = config.cfa.shift(0, scale_start);
        let rgb = PPGDemosaic::new().demosaic(&sensor, &cfa, &config.colors, roi);
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
