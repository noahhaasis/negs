//! Shared preview calibration and resolution policy for desktop clients.
use crate::params::RuntimeParams;
use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32, to_f32};

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
        "exposure_compensation_ev": if params.enlarger.print_exposure_compensation {
            params.camera.exposure_compensation_ev
        } else {
            0.0
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

pub fn rescale_preview_input_fast(image: &ImageBuf, (new_w, new_h): (u32, u32)) -> ImageBuf {
    // The normal f32 GUI can resize directly from the retained source,
    // avoiding another full-resolution RGB allocation just to downsample.
    #[cfg(not(feature = "precision-f64"))]
    let f32buf = image.data.as_slice();
    #[cfg(feature = "precision-f64")]
    let f32buf: Vec<f32> = image.data.par_iter().map(|&v| to_f32(v)).collect();
    let src = image::ImageBuffer::<image::Rgb<f32>, _>::from_raw(image.width, image.height, f32buf)
        .expect("ImageBuf dims match its data length");
    let dst = image::imageops::resize(&src, new_w, new_h, image::imageops::FilterType::CatmullRom);
    let data: Vec<Scalar> = dst.into_raw().into_par_iter().map(from_f32).collect();
    ImageBuf::from_data(new_w, new_h, data)
}

/// An RGBA8 display proxy of `image`, fitted within `max_dim` on its longer
/// side. Bilinear and parallel, writing bytes directly: a general-purpose
/// resampler would add a float copy and, being serial, seconds at 60 MP.
/// `encoded` says whether `image` is already sRGB-encoded; if not, each
/// interpolated value is encoded here. `image` itself is never modified.
pub fn display_rgba(image: &ImageBuf, max_dim: usize, encoded: bool) -> (usize, usize, Vec<u8>) {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let max_src_dim = src_w.max(src_h).max(1);
    let scale_num = max_dim.min(max_src_dim);
    let w = ((src_w * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let h = ((src_h * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let mut rgba = vec![0u8; w * h * 4];
    rgba.par_chunks_exact_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let gy = if h == src_h {
                y as f32
            } else {
                (y as f32 + 0.5) * (src_h as f32 / h as f32) - 0.5
            };
            let y0 = gy.floor().clamp(0.0, (src_h - 1) as f32) as usize;
            let y1 = (y0 + 1).min(src_h - 1);
            let ty = (gy - y0 as f32).clamp(0.0, 1.0);

            for x in 0..w {
                let gx = if w == src_w {
                    x as f32
                } else {
                    (x as f32 + 0.5) * (src_w as f32 / w as f32) - 0.5
                };
                let x0 = gx.floor().clamp(0.0, (src_w - 1) as f32) as usize;
                let x1 = (x0 + 1).min(src_w - 1);
                let tx = (gx - x0 as f32).clamp(0.0, 1.0);
                let p00 = (y0 * src_w + x0) * 3;
                let p10 = (y0 * src_w + x1) * 3;
                let p01 = (y1 * src_w + x0) * 3;
                let p11 = (y1 * src_w + x1) * 3;
                let dst = x * 4;

                for c in 0..3 {
                    let v00 = to_f32(image.data[p00 + c]);
                    let v10 = to_f32(image.data[p10 + c]);
                    let v01 = to_f32(image.data[p01 + c]);
                    let v11 = to_f32(image.data[p11 + c]);
                    let top = v00 + (v10 - v00) * tx;
                    let bot = v01 + (v11 - v01) * tx;
                    let mut v = top + (bot - top) * ty;
                    if !encoded {
                        v = if v <= 0.0031308 {
                            v * 12.92
                        } else {
                            1.055 * v.powf(1.0 / 2.4) - 0.055
                        };
                    }
                    row[dst + c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                row[dst + 3] = 255;
            }
        });
    (w, h, rgba)
}
