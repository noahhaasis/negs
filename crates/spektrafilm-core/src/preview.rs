//! Shared preview calibration and resolution policy for desktop clients.
use crate::params::RuntimeParams;
use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};

pub fn preview_pipeline_cache_key(
    film_name: &str,
    print_name: &str,
    params: &RuntimeParams,
) -> String {
    serde_json::json!({
        "film": film_name,
        "print": print_name,
        "film_dev": params.film_render.development_time,
        "print_dev": params.print_render.development_time,
        "scan_film": params.io.scan_film,
        "input_color_space": params.io.input_color_space,
        "input_cctf_decoding": params.io.input_cctf_decoding,
        "input_gamut": params.io.input_gamut_compress,
        "output_color_space": params.io.output_color_space,
        "output_gamut": params.io.output_gamut_compress,
        "settings": {
            "rgb_to_raw_method": params.settings.rgb_to_raw_method,
            "apply_hanatos2025_adaptation_window": params.settings.apply_hanatos2025_adaptation_window,
            "apply_hanatos2025_adaptation_surface": params.settings.apply_hanatos2025_adaptation_surface,
            "spectral_gaussian_blur": params.settings.spectral_gaussian_blur,
            "lut_resolution": params.settings.lut_resolution,
            "neutral_print_filters_from_database": params.settings.neutral_print_filters_from_database,
            "use_cat16": params.settings.use_cat16,
        },
        "enlarger": {
            "illuminant": params.enlarger.illuminant,
            "c_filter_neutral": params.enlarger.c_filter_neutral,
            "m_filter_neutral": params.enlarger.m_filter_neutral,
            "y_filter_neutral": params.enlarger.y_filter_neutral,
            "m_filter_shift": params.enlarger.m_filter_shift,
            "y_filter_shift": params.enlarger.y_filter_shift,
            "preflash_exposure": params.enlarger.preflash_exposure,
            "preflash_m_filter_shift": params.enlarger.preflash_m_filter_shift,
            "preflash_y_filter_shift": params.enlarger.preflash_y_filter_shift,
            "normalize_print_exposure": params.enlarger.normalize_print_exposure,
            "print_exposure_compensation": params.enlarger.print_exposure_compensation,
        },
    })
    .to_string()
}

pub const PREVIEW_PIXEL_LIMIT: u64 = 12_000_000;

pub fn preview_dimensions(width: u32, height: u32, factor: f32, limited: bool) -> (u32, u32) {
    // Preserve the existing working-resolution behavior, then impose the
    // preview-only cap. A smaller working image must never be enlarged.
    let (width, height) = if factor <= 0.0 || (factor - 1.0).abs() < 1e-6 {
        (width, height)
    } else {
        (
            ((width as f32 * factor).round() as u32).max(1),
            ((height as f32 * factor).round() as u32).max(1),
        )
    };
    preview_cap((width, height), limited)
}

/// The preview of a `working` frame: the frame itself, or with `limited`,
/// at most [`PREVIEW_PIXEL_LIMIT`] pixels with its aspect ratio. Never larger
/// than the working frame, and never the size an export develops at.
pub fn preview_cap((width, height): (u32, u32), limited: bool) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if !limited || pixels <= PREVIEW_PIXEL_LIMIT {
        return (width, height);
    }
    let scale = (PREVIEW_PIXEL_LIMIT as f64 / pixels as f64).sqrt();
    // Floor so rounding cannot exceed the cap. Bound each axis for the
    // degenerate case of a one-pixel-high or one-pixel-wide panorama.
    (
        ((width as f64 * scale).floor() as u32).clamp(1, PREVIEW_PIXEL_LIMIT as u32),
        ((height as f64 * scale).floor() as u32).clamp(1, PREVIEW_PIXEL_LIMIT as u32),
    )
}

/// The frame rendered while a slider moves: `scale` (screen pixels per
/// photograph pixel, below 1) of the `photo` frame, reduced further as needed
/// to fit inside the `source`, so a drag never enlarges the source and
/// [`downsample_area`] always applies. Image Size enlarges only the settled
/// frame. Keeps the aspect ratio.
pub fn interactive_dimensions(photo: (u32, u32), scale: f32, source: (u32, u32)) -> (u32, u32) {
    let frame = (
        ((photo.0 as f32 * scale).round() as u32).max(1),
        ((photo.1 as f32 * scale).round() as u32).max(1),
    );
    if frame.0 <= source.0 && frame.1 <= source.1 {
        return frame;
    }
    let fit = (f64::from(source.0) / f64::from(frame.0)).min(f64::from(source.1) / f64::from(frame.1));
    (
        ((f64::from(frame.0) * fit).floor() as u32).clamp(1, source.0),
        ((f64::from(frame.1) * fit).floor() as u32).clamp(1, source.1),
    )
}

/// The source resampled for a settled preview, with the same Catmull–Rom
/// as exports (`spektrafilm_io::resize::catmull_rom`): parallel, and
/// highlights above 1 are kept.
pub fn rescale_preview_input_fast(image: &ImageBuf, dimensions: (u32, u32)) -> ImageBuf {
    spektrafilm_io::resize::catmull_rom(image, dimensions)
}

/// The source reduced for an interactive preview: each output pixel is the
/// mean of the source area it covers. Parallel over output rows and cheaper
/// than the cubic resample above: a 60 MP source takes tens of milliseconds.
/// Only for frames shown while a slider moves; see [`interactive_dimensions`].
pub fn downsample_area(image: &ImageBuf, (new_w, new_h): (u32, u32)) -> ImageBuf {
    assert!(new_w >= 1 && new_h >= 1 && new_w <= image.width && new_h <= image.height);
    let (sw, sh) = (image.width as usize, image.height as usize);
    let (dw, dh) = (new_w as usize, new_h as usize);
    // Per output column or row: the first source index it covers and the
    // covered fraction of each source pixel from there, summing to one.
    let spans = |src: usize, dst: usize| -> Vec<(usize, Vec<f32>)> {
        let scale = src as f64 / dst as f64;
        (0..dst)
            .map(|i| {
                let (a, b) = (i as f64 * scale, ((i + 1) as f64 * scale).min(src as f64));
                let first = a.floor() as usize;
                let weights = (first..(b.ceil() as usize).min(src))
                    .map(|j| ((b.min(j as f64 + 1.0) - a.max(j as f64)) / scale) as f32)
                    .collect();
                (first, weights)
            })
            .collect()
    };
    let (columns, rows) = (spans(sw, dw), spans(sh, dh));
    let mut data = vec![from_f32(0.0); dw * dh * 3];
    data.par_chunks_mut(dw * 3)
        .zip(rows.par_iter())
        .for_each(|(line, (y0, row_weights))| {
            let mut sum = vec![0f32; sw * 3];
            for (k, &w) in row_weights.iter().enumerate() {
                let row = &image.data[(y0 + k) * sw * 3..(y0 + k + 1) * sw * 3];
                for (s, &v) in sum.iter_mut().zip(row) {
                    *s += w * to_f32(v);
                }
            }
            for (x, (x0, column_weights)) in columns.iter().enumerate() {
                let mut px = [0f32; 3];
                for (k, &w) in column_weights.iter().enumerate() {
                    for c in 0..3 {
                        px[c] += w * sum[(x0 + k) * 3 + c];
                    }
                }
                for c in 0..3 {
                    line[x * 3 + c] = from_f32(px[c]);
                }
            }
        });
    ImageBuf::from_data(new_w, new_h, data)
}

/// An RGBA8 display proxy of `image`, fitted within `max_dim` on its longer
/// side. Bilinear and parallel, writing bytes directly: a general-purpose
/// resampler would add a float copy and, being serial, seconds at 60 MP.
/// `encoded` says whether `image` is already sRGB-encoded; if not, each
/// interpolated value is encoded here. `image` itself is never modified.
pub fn display_rgba(image: &ImageBuf, max_dim: usize, encoded: bool) -> (usize, usize, Vec<u8>) {
    display_rgba_with_post(image, max_dim, encoded, None)
}

/// [`display_rgba`] with post adjustments, so a post-only edit redraws the
/// preview without developing the film again. They are applied to the source
/// pixels before downscaling, as an export applies them before any resize, so
/// the preview matches the adjusted image at every size.
pub fn display_rgba_with_post(
    image: &ImageBuf,
    max_dim: usize,
    encoded: bool,
    post: Option<&crate::post::PostProcess>,
) -> (usize, usize, Vec<u8>) {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let max_src_dim = src_w.max(src_h).max(1);
    let scale_num = max_dim.min(max_src_dim);
    let w = ((src_w * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let h = ((src_h * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let mut rgba = vec![0u8; w * h * 4];
    let pixel = |y: usize, x: usize| {
        let p = (y * src_w + x) * 3;
        let rgb = [0, 1, 2].map(|c| to_f32(image.data[p + c]));
        post.map_or(rgb, |post| post.apply_rgb(rgb))
    };
    let write = |out: &mut [u8], rgb: [f32; 3]| {
        for (c, mut v) in rgb.into_iter().enumerate() {
            if !encoded {
                v = if v <= 0.0031308 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
            }
            out[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out[3] = 255;
    };
    if w == src_w && h == src_h {
        rgba.par_chunks_exact_mut(w * 4)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, out) in row.chunks_exact_mut(4).enumerate() {
                    write(out, pixel(y, x));
                }
            });
        return (w, h, rgba);
    }

    // Bilinear. Each band of display rows reads the source rows it needs
    // once, and only the columns it samples, so post adjustments run about
    // once per source pixel rather than once per tap.
    const BAND: usize = 32;
    let columns: Vec<(usize, f32)> = (0..w)
        .map(|x| {
            let gx = if w == src_w {
                x as f32
            } else {
                (x as f32 + 0.5) * (src_w as f32 / w as f32) - 0.5
            };
            let x0 = gx.floor().clamp(0.0, (src_w - 1) as f32) as usize;
            (x0, (gx - x0 as f32).clamp(0.0, 1.0))
        })
        .collect();
    let mut sampled = vec![false; src_w];
    for &(x0, _) in &columns {
        sampled[x0] = true;
        sampled[(x0 + 1).min(src_w - 1)] = true;
    }
    rgba.par_chunks_mut(w * 4 * BAND)
        .enumerate()
        .for_each(|(band, rows)| {
            // The two source rows under the current display row, by index.
            let mut cached = [
                (usize::MAX, vec![[0f32; 3]; src_w]),
                (usize::MAX, vec![[0f32; 3]; src_w]),
            ];
            for (i, row) in rows.chunks_exact_mut(w * 4).enumerate() {
                let y = band * BAND + i;
                let gy = if h == src_h {
                    y as f32
                } else {
                    (y as f32 + 0.5) * (src_h as f32 / h as f32) - 0.5
                };
                let y0 = gy.floor().clamp(0.0, (src_h - 1) as f32) as usize;
                let y1 = (y0 + 1).min(src_h - 1);
                let ty = (gy - y0 as f32).clamp(0.0, 1.0);
                if cached[1].0 == y0 {
                    cached.swap(0, 1);
                }
                for (slot, source_y) in [(0, y0), (1, y1)] {
                    let (index, values) = &mut cached[slot];
                    if *index != source_y {
                        for (x, value) in values.iter_mut().enumerate() {
                            if sampled[x] {
                                *value = pixel(source_y, x);
                            }
                        }
                        *index = source_y;
                    }
                }
                let (top_row, bottom_row) = (&cached[0].1, &cached[1].1);
                for (out, &(x0, tx)) in row.chunks_exact_mut(4).zip(&columns) {
                    let x1 = (x0 + 1).min(src_w - 1);
                    let rgb = [0, 1, 2].map(|c| {
                        let top = top_row[x0][c] + (top_row[x1][c] - top_row[x0][c]) * tx;
                        let bot = bottom_row[x0][c] + (bottom_row[x1][c] - bottom_row[x0][c]) * tx;
                        top + (bot - top) * ty
                    });
                    write(out, rgb);
                }
            }
        });
    (w, h, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post::{PostParams, PostProcess};

    #[test]
    fn area_downsample_averages_the_covered_pixels() {
        // 3 → 2 columns: each output covers 1.5 source pixels.
        let image = ImageBuf::from_data(
            3,
            2,
            [0.0, 3.0, 6.0, 0.0, 3.0, 6.0]
                .iter()
                .flat_map(|&v| [from_f32(v); 3])
                .collect(),
        );
        let out = downsample_area(&image, (2, 1));
        let values: Vec<f32> = out.data.iter().step_by(3).map(|&v| to_f32(v)).collect();
        assert_eq!(values, [1.0, 5.0]);
        let flat = ImageBuf::from_data(7, 5, vec![from_f32(0.25); 7 * 5 * 3]);
        let out = downsample_area(&flat, (3, 2));
        assert!(out.data.iter().all(|&v| (to_f32(v) - 0.25).abs() < 1e-6));
    }

    #[test]
    fn preview_cap_keeps_the_aspect_and_never_enlarges() {
        assert_eq!(preview_cap((12_000, 8000), false), (12_000, 8000));
        let (w, h) = preview_cap((12_000, 8000), true);
        assert!(u64::from(w) * u64::from(h) <= PREVIEW_PIXEL_LIMIT);
        assert_eq!((w, h), (4242, 2828));
        assert_eq!(preview_cap((3000, 2000), true), (3000, 2000));
        assert_eq!(preview_dimensions(6000, 4000, 0.5, true), (3000, 2000));
    }

    #[test]
    fn interactive_frames_fit_inside_the_source() {
        // Zoomed past fit on a 3x enlargement: the old frame was 1.5x the source.
        assert_eq!(interactive_dimensions((18_000, 12_000), 0.5, (6000, 4000)), (6000, 4000));
        assert_eq!(interactive_dimensions((18_000, 12_000), 0.25, (6000, 4000)), (4500, 3000));
        // An odd aspect from rounding the working frame still fits both sides.
        let (w, h) = interactive_dimensions((12_001, 7999), 0.9, (6000, 4000));
        assert!(w <= 6000 && h <= 4000 && w >= 5990, "{w}x{h}");
        let source = ImageBuf::from_data(60, 40, vec![from_f32(0.5); 60 * 40 * 3]);
        let frame = interactive_dimensions((180, 120), 0.75, (60, 40));
        assert_eq!(downsample_area(&source, frame).width, 60);
    }

    /// Per-pixel bilinear sampling, as the display proxy did before it read
    /// source rows in bands. The banded version must give the same bytes.
    fn reference_rgba(image: &ImageBuf, w: usize, h: usize) -> Vec<u8> {
        let (src_w, src_h) = (image.width as usize, image.height as usize);
        let at = |x: usize, y: usize, c: usize| to_f32(image.data[(y * src_w + x) * 3 + c]);
        let mut out = Vec::new();
        for y in 0..h {
            let gy = (y as f32 + 0.5) * (src_h as f32 / h as f32) - 0.5;
            let y0 = gy.floor().clamp(0.0, (src_h - 1) as f32) as usize;
            let (y1, ty) = ((y0 + 1).min(src_h - 1), (gy - y0 as f32).clamp(0.0, 1.0));
            for x in 0..w {
                let gx = (x as f32 + 0.5) * (src_w as f32 / w as f32) - 0.5;
                let x0 = gx.floor().clamp(0.0, (src_w - 1) as f32) as usize;
                let (x1, tx) = ((x0 + 1).min(src_w - 1), (gx - x0 as f32).clamp(0.0, 1.0));
                for c in 0..3 {
                    let top = at(x0, y0, c) + (at(x1, y0, c) - at(x0, y0, c)) * tx;
                    let bot = at(x0, y1, c) + (at(x1, y1, c) - at(x0, y1, c)) * tx;
                    out.push(((top + (bot - top) * ty).clamp(0.0, 1.0) * 255.0).round() as u8);
                }
                out.push(255);
            }
        }
        out
    }

    #[test]
    fn downscaled_display_matches_per_pixel_bilinear() {
        // Taller than one band, with an odd scale so source rows are skipped and shared.
        let (src_w, src_h) = (53, 97);
        let data = (0..src_w * src_h * 3)
            .map(|i| from_f32(((i * 7919) % 1000) as f32 / 999.0))
            .collect();
        let image = ImageBuf::from_data(src_w as u32, src_h as u32, data);
        for max_dim in [96, 61, 40, 7] {
            let (w, h, rgba) = display_rgba(&image, max_dim, true);
            assert_eq!(rgba, reference_rgba(&image, w, h), "max_dim {max_dim}");
        }
    }

    /// Fine detail above the display cap: adjusting then downscaling, as an
    /// export would, must equal the preview's own path.
    #[test]
    fn post_preview_matches_the_adjusted_image_when_downscaled() {
        let stripes = (0..20 * 2)
            .flat_map(|i| [(i % 2) as f32; 3])
            .map(from_f32)
            .collect();
        let stripes = ImageBuf::from_data(20, 2, stripes);
        let noise = (0..53 * 97 * 3)
            .map(|i| from_f32(((i * 7919) % 1000) as f32 / 999.0))
            .collect();
        let noise = ImageBuf::from_data(53, 97, noise);
        let post = PostProcess::new(
            &PostParams {
                brightness: 100.0,
                contrast: 40.0,
                saturation: 50.0,
                ..Default::default()
            },
            true,
            "sRGB",
        );
        for (image, sizes) in [(&stripes, &[7, 20][..]), (&noise, &[97, 61, 40, 7][..])] {
            for &max_dim in sizes {
                assert_eq!(
                    display_rgba_with_post(image, max_dim, true, Some(&post)),
                    display_rgba(&post.applied(image), max_dim, true),
                    "{}x{} at max_dim {max_dim}",
                    image.width,
                    image.height
                );
            }
        }
    }
}
