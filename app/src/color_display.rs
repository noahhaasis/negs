//! Color-managed, exact-fit Lanczos-3 photographic display pipeline for GPUI.
//!
//! Solves both photographic quality hazards of stock GPUI:
//! 1. **1:1 Downscaling Sharpness:** Uses `negs_app::exact_fit::coefficients`
//!    (Lanczos-3 in linear light) to resample the photo to exact physical
//!    screen pixels (`(viewport_dips * scale_factor).round()`), eliminating
//!    bilinear grain shimmer and moiré.
//! 2. **Unified Color Management (macOS Display P3 & Windows/Linux ICC):**
//!    Transforms linear sRGB or unclipped Rec. 2020 proxies through the active
//!    display's ICC 3×3 matrix + 4096-entry tone-curve LUT into GPUI's BGRA8
//!    `RenderImage` format (`[b, g, r, 255]`).

use std::sync::Arc;

use gpui::{App, RenderImage, Window};
use negs_app::border::{BorderColor, BorderSettings, BorderStyle};
use negs_app::crop::{self, CropSettings, Orientation, Size as CropSize};
use negs_app::exact_fit;
use negs_app::icc;
use negs_app::picture::Picture;
use smallvec::smallvec;

/// Active monitor's color transform and human-readable profile name.
#[derive(Clone, Debug)]
pub struct DisplayColorProfile {
    pub transform: Option<Arc<icc::Transform>>,
    pub label: String,
    /// Platform display identifier (changes when the window moves monitors).
    pub display_key: u64,
}

impl Default for DisplayColorProfile {
    fn default() -> Self {
        Self {
            transform: None,
            label: "sRGB".to_string(),
            display_key: 0,
        }
    }
}

/// Queries the ICC profile for the display the GPUI `window` currently resides on.
pub fn active_display_profile(window: &Window, cx: &App) -> DisplayColorProfile {
    let display_id = window.display(cx).map(|d| u32::from(d.id()));
    let display_key = display_id.map(u64::from).unwrap_or(0);

    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::HWND;

        let hwnd = HasWindowHandle::window_handle(window)
            .ok()
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(win) => Some(HWND(win.hwnd.get() as *mut _)),
                _ => None,
            });
        if let Some(hwnd) = hwnd {
            if let Some(path) = icc::monitor_profile(hwnd) {
                let bytes = std::fs::read(&path).ok();
                let name = bytes
                    .as_deref()
                    .and_then(icc::profile_name)
                    .or_else(|| {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "ICC".to_string());
                let transform = bytes
                    .as_deref()
                    .and_then(icc::parse)
                    .and_then(|p| p.transform(&path))
                    .map(Arc::new);
                let label = if transform.is_some() {
                    name
                } else {
                    format!("{name} (sRGB)")
                };
                return DisplayColorProfile {
                    transform,
                    label,
                    display_key,
                };
            }
        }
        let (transform, label) = icc::describe_display(None);
        return DisplayColorProfile {
            transform: transform.map(Arc::new),
            label,
            display_key,
        };
    }

    #[cfg(not(windows))]
    {
        let (transform, label) = icc::describe_display(display_id);
        DisplayColorProfile {
            transform: transform.map(Arc::new),
            label,
            display_key,
        }
    }
}

/// A pixel rectangle within the oriented, cropped source photograph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Ready-to-paint BGRA8 display image, plus the source region it covers (for zoomed tiles).
#[derive(Clone)]
pub struct PreparedImage {
    pub render_image: Arc<RenderImage>,
    /// If `Some`, this image is a native-resolution crop of the oriented photo
    /// covering `region` (in oriented photo pixels); otherwise it covers the full photo/document.
    pub region: Option<SourceRegion>,
}

/// Converts a raw BGRA8 buffer (`width * height * 4`) into a GPUI `RenderImage`.
pub fn bgra_to_render_image(bgra: Vec<u8>, width: u32, height: u32) -> Arc<RenderImage> {
    let img = image::RgbaImage::from_raw(width, height, bgra)
        .expect("BGRA buffer length must match width * height * 4");
    Arc::new(RenderImage::new(smallvec![image::Frame::new(img)]))
}

#[inline]
pub fn srgb_decode(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn srgb_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode_lut() -> [f32; 256] {
    std::array::from_fn(|i| srgb_decode(i as f32 / 255.0))
}

/// Converts linear RGB (sRGB primaries, or Rec. 2020 primaries if `wide`)
/// into color-managed BGRA8 (`[b, g, r, 255]`) for GPUI `RenderImage`.
#[inline]
pub fn color_manage_linear_to_bgra(
    mut rgb: [f32; 3],
    wide: bool,
    cm: Option<&icc::Transform>,
) -> [u8; 4] {
    if wide {
        let m = icc::REC2020_TO_SRGB;
        let s = rgb;
        // Unclipped Rec. 2020 -> linear sRGB so wide-gamut displays keep out-of-sRGB colors.
        rgb = [
            m[0][0] * s[0] + m[0][1] * s[1] + m[0][2] * s[2],
            m[1][0] * s[0] + m[1][1] * s[1] + m[1][2] * s[2],
            m[2][0] * s[0] + m[2][1] * s[1] + m[2][2] * s[2],
        ];
    }
    let encoded = match cm {
        Some(t) => {
            let max_idx = t.lut.len() - 1;
            let last = max_idx as f32;
            std::array::from_fn(|c| {
                let m = t.matrix[c][0] * rgb[0]
                    + t.matrix[c][1] * rgb[1]
                    + t.matrix[c][2] * rgb[2];
                let p = srgb_encode(m) * last;
                let i = (p.floor() as usize).min(max_idx);
                let j = (i + 1).min(max_idx);
                let frac = p - i as f32;
                t.lut[i][c] + (t.lut[j][c] - t.lut[i][c]) * frac
            })
        }
        None => rgb.map(srgb_encode),
    };
    let r = (encoded[0].clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (encoded[1].clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (encoded[2].clamp(0.0, 1.0) * 255.0).round() as u8;
    [b, g, r, 255]
}

/// Converts a single sRGB 8-bit color (such as a border mat) through the display ICC transform.
pub fn color_manage_srgb_to_bgra(rgb8: [u8; 3], cm: Option<&icc::Transform>) -> [u8; 4] {
    let linear = [
        srgb_decode(rgb8[0] as f32 / 255.0),
        srgb_decode(rgb8[1] as f32 / 255.0),
        srgb_decode(rgb8[2] as f32 / 255.0),
    ];
    color_manage_linear_to_bgra(linear, false, cm)
}

/// Converts a color-managed sRGB border color to GPUI `Hsla` (for zoomed-in mat backgrounds).
pub fn color_manage_border_hsla(color: BorderColor, cm: Option<&icc::Transform>) -> gpui::Hsla {
    let bgra = color_manage_srgb_to_bgra(color.0, cm);
    let hex = ((bgra[2] as u32) << 16) | ((bgra[1] as u32) << 8) | (bgra[0] as u32);
    gpui::rgb(hex).into()
}

/// Computes the oriented, cropped dimensions `(width, height)` in source pixels.
pub fn oriented_cropped_dimensions(
    src_w: u32,
    src_h: u32,
    orientation: Orientation,
    crop: &CropSettings,
) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (1, 1);
    }
    let px_rect = crop.pixels(CropSize::new(src_w as f64, src_h as f64));
    let cw = (px_rect.width.round() as u32).clamp(1, src_w);
    let ch = (px_rect.height.round() as u32).clamp(1, src_h);
    if orientation.turns.rem_euclid(2) == 1 {
        (ch, cw)
    } else {
        (cw, ch)
    }
}

/// Computes the axis-aligned crop box `(x0, y0, cw, ch)` in unrotated source pixels.
fn axis_crop_box(src_w: u32, src_h: u32, crop: &CropSettings) -> (u32, u32, u32, u32) {
    if !crop.cropped() || crop.angle != 0.0 {
        return (0, 0, src_w, src_h);
    }
    let r = crop.pixels(CropSize::new(src_w as f64, src_h as f64));
    let x0 = (r.x.round() as i64).clamp(0, (src_w - 1) as i64) as u32;
    let y0 = (r.y.round() as i64).clamp(0, (src_h - 1) as i64) as u32;
    let cw = (r.width.round() as u32).clamp(1, src_w - x0);
    let ch = (r.height.round() as u32).clamp(1, src_h - y0);
    (x0, y0, cw, ch)
}

/// Maps an oriented cropped pixel `(ox, oy)` back to an unrotated, unflipped source pixel `(sx, sy)`.
#[inline]
pub fn oriented_to_source(
    ox: u32,
    oy: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    orientation: Orientation,
) -> (u32, u32) {
    let turns = orientation.turns.rem_euclid(4);
    let (fx, fy) = match turns {
        0 => (ox, oy),
        1 => (oy, ch - 1 - ox),
        2 => (cw - 1 - ox, ch - 1 - oy),
        _ => (cw - 1 - oy, ox),
    };
    let ux = if orientation.flip { cw - 1 - fx } else { fx };
    (cx0 + ux, cy0 + fy)
}

/// Fused linear-light Lanczos-3 downscale + orientation + crop + color management -> BGRA8.
///
/// Produces exact `target_w × target_h` physical pixels packed as `[b, g, r, 255]`.
pub fn resample_and_color_manage_bgra(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    wide: bool,
    orientation: Orientation,
    crop: &CropSettings,
    target_w: u32,
    target_h: u32,
    cm: Option<&icc::Transform>,
) -> Vec<u8> {
    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    let lut = srgb_decode_lut();

    if crop.angle.abs() > 1e-6 {
        return render_straightened_bgra(
            rgba,
            src_w,
            src_h,
            wide,
            orientation,
            crop,
            target_w,
            target_h,
            cm,
            &lut,
        );
    }

    let (cx0, cy0, cw, ch) = axis_crop_box(src_w, src_h, crop);
    let turns = orientation.turns.rem_euclid(4);
    let (oriented_w, oriented_h) = if turns % 2 == 1 { (ch, cw) } else { (cw, ch) };

    // Fast path: exact 1:1 size with no resampling needed.
    if target_w == oriented_w && target_h == oriented_h {
        return extract_region_color_managed_bgra(
            rgba,
            src_w,
            src_h,
            wide,
            orientation,
            crop,
            SourceRegion {
                x: 0,
                y: 0,
                width: oriented_w,
                height: oriented_h,
            },
            cm,
        );
    }

    let across = exact_fit::coefficients(oriented_w, target_w);
    let down = exact_fit::coefficients(oriented_h, target_h);
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16);

    // Pass 1 (Horizontal): decode sRGB -> linear light, resample across to `target_w × oriented_h`.
    let mut middle = vec![0.0f32; target_w as usize * oriented_h as usize * 3];
    let rows_each = (oriented_h as usize).div_ceil(threads).max(1);
    let identity_orient = turns == 0 && !orientation.flip;

    std::thread::scope(|scope| {
        for (chunk, out_chunk) in middle
            .chunks_mut(rows_each * target_w as usize * 3)
            .enumerate()
        {
            let across = &across;
            let lut = &lut;
            scope.spawn(move || {
                for (r, row) in out_chunk.chunks_mut(target_w as usize * 3).enumerate() {
                    let oy = (chunk * rows_each + r) as u32;
                    if identity_orient {
                        let src_row_off = (cy0 + oy) as usize * src_w as usize * 4;
                        for (x, (start, weights)) in across.iter().enumerate() {
                            let mut sum = [0.0f32; 3];
                            let base_x = (cx0 + *start) as usize;
                            for (k, &weight) in weights.iter().enumerate() {
                                let p = src_row_off + (base_x + k) * 4;
                                sum[0] += weight * lut[rgba[p] as usize];
                                sum[1] += weight * lut[rgba[p + 1] as usize];
                                sum[2] += weight * lut[rgba[p + 2] as usize];
                            }
                            let dst = x * 3;
                            row[dst] = sum[0];
                            row[dst + 1] = sum[1];
                            row[dst + 2] = sum[2];
                        }
                    } else {
                        for (x, (start, weights)) in across.iter().enumerate() {
                            let mut sum = [0.0f32; 3];
                            for (k, &weight) in weights.iter().enumerate() {
                                let ox = *start + k as u32;
                                let (sx, sy) =
                                    oriented_to_source(ox, oy, cx0, cy0, cw, ch, orientation);
                                let p = (sy as usize * src_w as usize + sx as usize) * 4;
                                sum[0] += weight * lut[rgba[p] as usize];
                                sum[1] += weight * lut[rgba[p + 1] as usize];
                                sum[2] += weight * lut[rgba[p + 2] as usize];
                            }
                            let dst = x * 3;
                            row[dst] = sum[0];
                            row[dst + 1] = sum[1];
                            row[dst + 2] = sum[2];
                        }
                    }
                }
            });
        }
    });

    // Pass 2 (Vertical + Color Management + BGRA8 pack): `target_w × target_h`.
    let mut out = vec![0u8; target_w as usize * target_h as usize * 4];
    let out_rows_each = (target_h as usize).div_ceil(threads).max(1);

    std::thread::scope(|scope| {
        for (chunk, out_chunk) in out
            .chunks_mut(out_rows_each * target_w as usize * 4)
            .enumerate()
        {
            let down = &down;
            let middle = &middle;
            scope.spawn(move || {
                let tw = target_w as usize;
                for (r, row) in out_chunk.chunks_mut(tw * 4).enumerate() {
                    let (start, weights) = &down[chunk * out_rows_each + r];
                    let start = *start as usize;
                    for x in 0..tw {
                        let mut sum = [0.0f32; 3];
                        for (k, &weight) in weights.iter().enumerate() {
                            let p = ((start + k) * tw + x) * 3;
                            sum[0] += weight * middle[p];
                            sum[1] += weight * middle[p + 1];
                            sum[2] += weight * middle[p + 2];
                        }
                        let bgra = color_manage_linear_to_bgra(sum, wide, cm);
                        let dst = x * 4;
                        row[dst..dst + 4].copy_from_slice(&bgra);
                    }
                }
            });
        }
    });

    out
}

/// Extracts a native-resolution `region` of the oriented, cropped photograph and applies
/// linear-light color management into BGRA8 (`[b, g, r, 255]`). Used for zoom >= 100%.
pub fn extract_region_color_managed_bgra(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    wide: bool,
    orientation: Orientation,
    crop: &CropSettings,
    region: SourceRegion,
    cm: Option<&icc::Transform>,
) -> Vec<u8> {
    let lut = srgb_decode_lut();
    let (cx0, cy0, cw, ch) = axis_crop_box(src_w, src_h, crop);
    let turns = orientation.turns.rem_euclid(4);
    let (oriented_w, oriented_h) = if turns % 2 == 1 { (ch, cw) } else { (cw, ch) };

    let rx = region.x.min(oriented_w.saturating_sub(1));
    let ry = region.y.min(oriented_h.saturating_sub(1));
    let rw = region.width.clamp(1, oriented_w - rx);
    let rh = region.height.clamp(1, oriented_h - ry);

    let mut out = vec![0u8; rw as usize * rh as usize * 4];
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16);
    let rows_each = (rh as usize).div_ceil(threads).max(1);
    let identity_orient = turns == 0 && !orientation.flip;

    std::thread::scope(|scope| {
        for (chunk, out_chunk) in out.chunks_mut(rows_each * rw as usize * 4).enumerate() {
            let lut = &lut;
            scope.spawn(move || {
                for (r, row) in out_chunk.chunks_mut(rw as usize * 4).enumerate() {
                    let oy = ry + (chunk * rows_each + r) as u32;
                    if identity_orient {
                        let src_row_off = (cy0 + oy) as usize * src_w as usize * 4;
                        let base_x = (cx0 + rx) as usize;
                        for tx in 0..rw as usize {
                            let p = src_row_off + (base_x + tx) * 4;
                            let rgb = [
                                lut[rgba[p] as usize],
                                lut[rgba[p + 1] as usize],
                                lut[rgba[p + 2] as usize],
                            ];
                            let bgra = color_manage_linear_to_bgra(rgb, wide, cm);
                            let dst = tx * 4;
                            row[dst..dst + 4].copy_from_slice(&bgra);
                        }
                    } else {
                        for tx in 0..rw as u32 {
                            let ox = rx + tx;
                            let (sx, sy) =
                                oriented_to_source(ox, oy, cx0, cy0, cw, ch, orientation);
                            let p = (sy as usize * src_w as usize + sx as usize) * 4;
                            let rgb = [
                                lut[rgba[p] as usize],
                                lut[rgba[p + 1] as usize],
                                lut[rgba[p + 2] as usize],
                            ];
                            let bgra = color_manage_linear_to_bgra(rgb, wide, cm);
                            let dst = tx as usize * 4;
                            row[dst..dst + 4].copy_from_slice(&bgra);
                        }
                    }
                }
            });
        }
    });

    out
}

/// Straightened (non-zero `crop.angle`) resample + color management in linear light.
#[allow(clippy::too_many_arguments)]
fn render_straightened_bgra(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    wide: bool,
    orientation: Orientation,
    crop: &CropSettings,
    target_w: u32,
    target_h: u32,
    cm: Option<&icc::Transform>,
    lut: &[f32; 256],
) -> Vec<u8> {
    let turned = if orientation.turns.rem_euclid(2) == 1 {
        CropSize::new(src_h as f64, src_w as f64)
    } else {
        CropSize::new(src_w as f64, src_h as f64)
    };
    let mut out = vec![0u8; target_w as usize * target_h as usize * 4];
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16);
    let rows_each = (target_h as usize).div_ceil(threads).max(1);

    std::thread::scope(|scope| {
        for (chunk, out_chunk) in out
            .chunks_mut(rows_each * target_w as usize * 4)
            .enumerate()
        {
            scope.spawn(move || {
                for (r, row) in out_chunk.chunks_mut(target_w as usize * 4).enumerate() {
                    let y = chunk * rows_each + r;
                    let v = (y as f64 + 0.5) / target_h as f64;
                    for x in 0..target_w as usize {
                        let u = (x as f64 + 0.5) / target_w as f64;
                        let rgb = match crop::unshown_point(orientation, crop, turned, [u, v]) {
                            Some(pt) => sample_bilinear_linear(
                                rgba,
                                src_w,
                                src_h,
                                pt.x as f32,
                                pt.y as f32,
                                lut,
                            ),
                            None => [0.0, 0.0, 0.0],
                        };
                        let bgra = color_manage_linear_to_bgra(rgb, wide, cm);
                        let dst = x * 4;
                        row[dst..dst + 4].copy_from_slice(&bgra);
                    }
                }
            });
        }
    });

    out
}

#[inline]
fn sample_bilinear_linear(
    rgba: &[u8],
    w: u32,
    h: u32,
    u: f32,
    v: f32,
    lut: &[f32; 256],
) -> [f32; 3] {
    let fx = (u * w as f32 - 0.5).clamp(0.0, (w.saturating_sub(1)) as f32);
    let fy = (v * h as f32 - 0.5).clamp(0.0, (h.saturating_sub(1)) as f32);
    let x0 = fx.floor() as u32;
    let y0 = fy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let mut sum = [0.0f32; 3];
    let weights = [
        (x0, y0, (1.0 - tx) * (1.0 - ty)),
        (x1, y0, tx * (1.0 - ty)),
        (x0, y1, (1.0 - tx) * ty),
        (x1, y1, tx * ty),
    ];
    for (sx, sy, weight) in weights {
        let p = (sy as usize * w as usize + sx as usize) * 4;
        sum[0] += weight * lut[rgba[p] as usize];
        sum[1] += weight * lut[rgba[p + 1] as usize];
        sum[2] += weight * lut[rgba[p + 2] as usize];
    }
    sum
}

/// Composites a Solid or Keyline border around an already color-managed BGRA8 photo
/// at exact physical dimensions `(doc_tw, doc_th)`.
fn composite_solid_border(
    photo_bgra: &[u8],
    photo_tw: u32,
    photo_th: u32,
    doc_tw: u32,
    doc_th: u32,
    border: BorderSettings,
    oriented_w: u32,
    oriented_h: u32,
    cm: Option<&icc::Transform>,
) -> Vec<u8> {
    let mat = color_manage_srgb_to_bgra(border.color.0, cm);
    let mut out = vec![0u8; doc_tw as usize * doc_th as usize * 4];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&mat);
    }

    let pad_x = (doc_tw.saturating_sub(photo_tw)) / 2;
    let pad_y = (doc_th.saturating_sub(photo_th)) / 2;

    if border.style == BorderStyle::Keyline {
        let keyline_photo = border.keyline(oriented_w, oriented_h) as f64;
        let scale = photo_tw as f64 / oriented_w.max(1) as f64;
        let k_px = (keyline_photo * scale).round().max(1.0) as u32;
        let line_col = color_manage_srgb_to_bgra(border.line_color.0, cm);
        let kx0 = pad_x.saturating_sub(k_px);
        let ky0 = pad_y.saturating_sub(k_px);
        let kx1 = (pad_x + photo_tw + k_px).min(doc_tw);
        let ky1 = (pad_y + photo_th + k_px).min(doc_th);
        for y in ky0..ky1 {
            let row_off = y as usize * doc_tw as usize * 4;
            for x in kx0..kx1 {
                let dst = row_off + x as usize * 4;
                out[dst..dst + 4].copy_from_slice(&line_col);
            }
        }
    }

    for y in 0..photo_th.min(doc_th.saturating_sub(pad_y)) {
        let src_row = y as usize * photo_tw as usize * 4;
        let dst_row = ((pad_y + y) as usize * doc_tw as usize + pad_x as usize) * 4;
        let copy_len = photo_tw.min(doc_tw.saturating_sub(pad_x)) as usize * 4;
        out[dst_row..dst_row + copy_len].copy_from_slice(&photo_bgra[src_row..src_row + copy_len]);
    }
    out
}

/// Prepares an exact-fit (downscaled) color-managed `RenderImage` from a `Picture`.
///
/// `target_w × target_h` are the exact physical screen pixels of the displayed document
/// (including solid/keyline border if enabled).
pub fn prepare_exact_fit(
    picture: &Picture,
    orientation: Orientation,
    crop: &CropSettings,
    border: Option<BorderSettings>,
    film: &str,
    target_w: u32,
    target_h: u32,
    cm: Option<&icc::Transform>,
) -> Option<PreparedImage> {
    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    let (oriented_w, oriented_h) =
        oriented_cropped_dimensions(picture.width, picture.height, orientation, crop);

    let active_border = border.and_then(|b| {
        b.shown(oriented_w, oriented_h)
            .filter(|s| matches!(s.style, BorderStyle::Solid | BorderStyle::Keyline))
    });

    let bgra = picture.with_pixels(|rgba| {
        if let Some(b) = active_border {
            let (doc_w, doc_h) = b.document(oriented_w, oriented_h, film);
            let photo_tw = ((oriented_w as f64 / doc_w.max(1) as f64) * target_w as f64)
                .round()
                .clamp(1.0, target_w as f64) as u32;
            let photo_th = ((oriented_h as f64 / doc_h.max(1) as f64) * target_h as f64)
                .round()
                .clamp(1.0, target_h as f64) as u32;
            let photo_bgra = resample_and_color_manage_bgra(
                rgba,
                picture.width,
                picture.height,
                picture.wide,
                orientation,
                crop,
                photo_tw,
                photo_th,
                cm,
            );
            composite_solid_border(
                &photo_bgra,
                photo_tw,
                photo_th,
                target_w,
                target_h,
                b,
                oriented_w,
                oriented_h,
                cm,
            )
        } else {
            resample_and_color_manage_bgra(
                rgba,
                picture.width,
                picture.height,
                picture.wide,
                orientation,
                crop,
                target_w,
                target_h,
                cm,
            )
        }
    })?;

    Some(PreparedImage {
        render_image: bgra_to_render_image(bgra, target_w, target_h),
        region: None,
    })
}

/// Prepares a native-resolution visible tile for zoomed views (`zoom >= 1.0`).
pub fn prepare_zoomed_tile(
    picture: &Picture,
    orientation: Orientation,
    crop: &CropSettings,
    region: SourceRegion,
    cm: Option<&icc::Transform>,
) -> Option<PreparedImage> {
    let (oriented_w, oriented_h) =
        oriented_cropped_dimensions(picture.width, picture.height, orientation, crop);
    let rx = region.x.min(oriented_w.saturating_sub(1));
    let ry = region.y.min(oriented_h.saturating_sub(1));
    let rw = region.width.clamp(1, oriented_w - rx);
    let rh = region.height.clamp(1, oriented_h - ry);
    let clamped = SourceRegion {
        x: rx,
        y: ry,
        width: rw,
        height: rh,
    };

    let bgra = picture.with_pixels(|rgba| {
        if crop.angle.abs() > 1e-6 {
            // For straightened crops at high zoom, render the straightened photo at its oriented size
            // or extract the region directly via straightened sampling.
            let lut = srgb_decode_lut();
            let turned = if orientation.turns.rem_euclid(2) == 1 {
                CropSize::new(picture.height as f64, picture.width as f64)
            } else {
                CropSize::new(picture.width as f64, picture.height as f64)
            };
            let mut out = vec![0u8; rw as usize * rh as usize * 4];
            for ty in 0..rh {
                let v = (ry as f64 + ty as f64 + 0.5) / oriented_h as f64;
                for tx in 0..rw {
                    let u = (rx as f64 + tx as f64 + 0.5) / oriented_w as f64;
                    let rgb = match crop::unshown_point(orientation, crop, turned, [u, v]) {
                        Some(pt) => sample_bilinear_linear(
                            rgba,
                            picture.width,
                            picture.height,
                            pt.x as f32,
                            pt.y as f32,
                            &lut,
                        ),
                        None => [0.0, 0.0, 0.0],
                    };
                    let bgra = color_manage_linear_to_bgra(rgb, picture.wide, cm);
                    let dst = (ty as usize * rw as usize + tx as usize) * 4;
                    out[dst..dst + 4].copy_from_slice(&bgra);
                }
            }
            out
        } else {
            extract_region_color_managed_bgra(
                rgba,
                picture.width,
                picture.height,
                picture.wide,
                orientation,
                crop,
                clamped,
                cm,
            )
        }
    })?;

    Some(PreparedImage {
        render_image: bgra_to_render_image(bgra, rw, rh),
        region: Some(clamped),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::crop::Rect as CropRect;
    use std::path::Path;

    /// Display P3 D50 colorants matching `negs_app::icc::tests`.
    const P3_COLORANTS: [[f64; 3]; 3] = [
        [0.5151, 0.2920, 0.1571],
        [0.2412, 0.6922, 0.0666],
        [-0.0011, 0.0419, 0.7841],
    ];

    fn display_p3_transform() -> icc::Transform {
        let srgb_curve = icc::Curve::Parametric(
            3,
            vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
        );
        let profile = icc::MatrixProfile {
            colorants: P3_COLORANTS,
            curves: [srgb_curve.clone(), srgb_curve.clone(), srgb_curve],
        };
        profile
            .transform(Path::new("Display P3"))
            .expect("Display P3 transform")
    }

    #[test]
    fn bgra_channel_order_on_srgb() {
        // Pure sRGB red [255, 0, 0, 255] -> GPUI BGRA [0, 0, 255, 255].
        let red_rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let out = resample_and_color_manage_bgra(
            &red_rgba,
            4,
            1,
            false,
            Orientation::default(),
            &CropSettings::default(),
            4,
            1,
            None,
        );
        assert_eq!(&out[0..4], &[0, 0, 255, 255], "red must pack as BGRA [0,0,255,255]");
        assert_eq!(&out[4..8], &[0, 255, 0, 255], "green must pack as BGRA [0,255,0,255]");
        assert_eq!(&out[8..12], &[255, 0, 0, 255], "blue must pack as BGRA [255,0,0,255]");
        assert_eq!(&out[12..16], &[255, 255, 255, 255], "white stays white");
    }

    #[test]
    fn display_p3_desaturates_srgb_red() {
        let p3 = display_p3_transform();
        let red_rgba = vec![255, 0, 0, 255];
        let out = resample_and_color_manage_bgra(
            &red_rgba,
            1,
            1,
            false,
            Orientation::default(),
            &CropSettings::default(),
            1,
            1,
            Some(&p3),
        );
        let (b, g, r, a) = (out[0], out[1], out[2], out[3]);
        assert_eq!(a, 255);
        assert!(r < 240 && r > 220, "sRGB red on P3 should have R < 240, got R={r} G={g} B={b}");
        assert!(g > 30 && g < 70, "sRGB red on P3 should add green > 30, got R={r} G={g} B={b}");
    }

    #[test]
    fn rec2020_wide_gamut_on_display_p3_preserves_out_of_srgb_saturation() {
        let p3 = display_p3_transform();
        // Pure Rec. 2020 red is far outside sRGB; unclipped Rec.2020->sRGB->P3
        // must NOT clip at sRGB red (~234) and must saturate P3 red (R > 250, G == 0).
        let rec2020_red = vec![255, 0, 0, 255];
        let out_wide = resample_and_color_manage_bgra(
            &rec2020_red,
            1,
            1,
            true,
            Orientation::default(),
            &CropSettings::default(),
            1,
            1,
            Some(&p3),
        );
        let out_srgb = resample_and_color_manage_bgra(
            &rec2020_red,
            1,
            1,
            false,
            Orientation::default(),
            &CropSettings::default(),
            1,
            1,
            Some(&p3),
        );
        // BGRA: index 2 = R, index 1 = G
        assert!(
            out_wide[2] > out_srgb[2] + 15 && out_wide[2] == 255 && out_wide[1] == 0,
            "Rec.2020 red on P3 should preserve saturation: wide={out_wide:?}, srgb={out_srgb:?}"
        );
    }

    #[test]
    fn exact_fit_lanczos_flat_grey_and_3px_stripe_sharpness() {
        let (w, h) = (97u32, 61u32);
        let grey = vec![128u8; (w * h * 4) as usize];
        let out = resample_and_color_manage_bgra(
            &grey,
            w,
            h,
            false,
            Orientation::default(),
            &CropSettings::default(),
            40,
            25,
            None,
        );
        assert!(
            out.chunks_exact(4)
                .all(|p| p[0] == 128 && p[1] == 128 && p[2] == 128 && p[3] == 255),
            "flat grey must stay flat"
        );

        let stripes: Vec<u8> = (0..h)
            .flat_map(|_| {
                (0..w).flat_map(|x| {
                    if (x / 3) % 2 == 0 {
                        [255, 255, 255, 255]
                    } else {
                        [0, 0, 0, 255]
                    }
                })
            })
            .collect();
        let out = resample_and_color_manage_bgra(
            &stripes,
            w,
            h,
            false,
            Orientation::default(),
            &CropSettings::default(),
            58,
            37,
            None,
        );
        let row: Vec<u8> = out[12 * 58 * 4..13 * 58 * 4]
            .chunks_exact(4)
            .map(|p| p[2])
            .collect();
        let lo = *row[4..54].iter().min().unwrap();
        let hi = *row[4..54].iter().max().unwrap();
        assert!(hi > 200 && lo < 60, "3px stripes must remain crisp: {lo}..{hi}");
    }

    #[test]
    fn orientation_and_crop_match_negs_app_crop_math() {
        // 4x2 source where every pixel has a unique (x, y) signature in R=x*50, G=y*100.
        let (w, h) = (4u32, 2u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 50) as u8, (y * 100) as u8, 200, 255]);
            }
        }

        // Check all 8 orientations (turns 0..4, flip false/true).
        for flip in [false, true] {
            for turns in 0..4 {
                let orient = Orientation::new(turns, flip);
                let (ow, oh) = oriented_cropped_dimensions(w, h, orient, &CropSettings::default());
                let out = resample_and_color_manage_bgra(
                    &rgba,
                    w,
                    h,
                    false,
                    orient,
                    &CropSettings::default(),
                    ow,
                    oh,
                    None,
                );
                for oy in 0..oh {
                    for ox in 0..ow {
                        let shown_pt = crop::Point {
                            x: (ox as f64 + 0.5) / ow as f64,
                            y: (oy as f64 + 0.5) / oh as f64,
                        };
                        let src_pt = orient.unshown(shown_pt);
                        let expected_sx = (src_pt.x * w as f64).floor() as u32;
                        let expected_sy = (src_pt.y * h as f64).floor() as u32;
                        let idx = (oy as usize * ow as usize + ox as usize) * 4;
                        // BGRA: [2] is R (= sx*50), [1] is G (= sy*100)
                        assert_eq!(
                            (out[idx + 2], out[idx + 1]),
                            ((expected_sx * 50) as u8, (expected_sy * 100) as u8),
                            "mismatch at turns={turns} flip={flip} ({ox},{oy})"
                        );
                    }
                }
            }
        }

        // Check crop: right half (x = 0.5..1.0, y = 0.0..1.0) -> 2x2 pixels x in {2, 3}.
        let crop = CropSettings {
            rect: CropRect::new(0.5, 0.0, 0.5, 1.0),
            ..Default::default()
        };
        let (ow, oh) = oriented_cropped_dimensions(w, h, Orientation::default(), &crop);
        assert_eq!((ow, oh), (2, 2));
        let out = resample_and_color_manage_bgra(
            &rgba,
            w,
            h,
            false,
            Orientation::default(),
            &crop,
            ow,
            oh,
            None,
        );
        // Top-left of cropped image should be source (2, 0) -> R=100, G=0.
        assert_eq!((out[2], out[1]), (100, 0));
        // Bottom-right of cropped image should be source (3, 1) -> R=150, G=100.
        let br = (1 * 2 + 1) * 4;
        assert_eq!((out[br + 2], out[br + 1]), (150, 100));
    }
}
