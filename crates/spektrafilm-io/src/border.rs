//! Presentation borders added around a rendered photograph.
//!
//! Borders are composited after the film pipeline and rotation, so changing
//! one never re-renders. The macOS preview draws the same geometry with Core
//! Animation (`macos/Sources/Spektrafilm/Border.swift`); keep the two in sync.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use image::{RgbaImage, imageops};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, srgb_decode, to_f32};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    /// Plain white mat, as shown (but not exported) by the Python GUI.
    White,
    /// Scanned 120-film frame: black with an irregular edge.
    Film,
    /// White mat with a thin black line around the photograph.
    Keyline,
}

impl FromStr for BorderStyle {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "white" => Self::White,
            "film" => Self::Film,
            "keyline" => Self::Keyline,
            _ => bail!("unknown border style: {s} (expected white, film or keyline)"),
        })
    }
}

/// White margin on every side: 3% of the long edge, as in the Python GUI.
pub fn white_margin(width: u32, height: u32) -> u32 {
    (width.max(height) as f64 * 0.03).floor() as u32
}

/// Width of the keyline's black line.
pub fn keyline_width(width: u32, height: u32) -> u32 {
    ((width.max(height) as f64 * 0.004).round() as u32).max(1)
}

/// The film frame's (inset, nine-patch size). The inset grows the canvas
/// a little; the frame's irregular edge still overlaps the photograph.
pub fn film_geometry(width: u32, height: u32) -> (u32, u32) {
    let short = width.min(height) as f64;
    (
        ((short * 0.003).round() as u32).max(1),
        ((short * 0.025).round() as u32).max(2),
    )
}

/// `image` with a `style` border. `encoded` says whether the values are
/// sRGB-encoded (otherwise linear), which only matters for the film texture.
pub fn add_border(
    image: &ImageBuf,
    style: BorderStyle,
    encoded: bool,
    data_dir: &Path,
) -> Result<ImageBuf> {
    let (w, h) = (image.width, image.height);
    Ok(match style {
        BorderStyle::White => framed(image, white_margin(w, h), 0, 1.0),
        BorderStyle::Keyline => framed(image, white_margin(w, h), keyline_width(w, h), 1.0),
        BorderStyle::Film => {
            let path = data_dir.join("borders/film.png");
            let texture = image::open(&path)
                .with_context(|| format!("loading border texture {}", path.display()))?
                .to_rgba8();
            let (inset, pad) = film_geometry(w, h);
            let mut out = framed(image, inset, 0, 0.0);
            overlay_nine_patch(&mut out, texture, pad, encoded);
            out
        }
    })
}

/// `image` inside a `line`-wide ring of black, then a `margin` of `fill`.
fn framed(image: &ImageBuf, margin: u32, line: u32, fill: f32) -> ImageBuf {
    let offset = (margin + line) as usize;
    let (w, h) = (image.width as usize, image.height as usize);
    let (cw, ch) = (w + 2 * offset, h + 2 * offset);
    let (fill, black) = (from_f32(fill), from_f32(0.0));
    let mut data = vec![fill; cw * ch * 3];
    let ring = margin as usize..ch - margin as usize;
    data.par_chunks_exact_mut(cw * 3)
        .enumerate()
        .for_each(|(y, row)| {
            if line > 0 && ring.contains(&y) {
                row[margin as usize * 3..(cw - margin as usize) * 3].fill(black);
            }
            if (offset..offset + h).contains(&y) {
                let src = &image.data[(y - offset) * w * 3..][..w * 3];
                row[offset * 3..(offset + w) * 3].copy_from_slice(src);
            }
        });
    ImageBuf::from_data(cw as u32, ch as u32, data)
}

/// Stretches `texture` over `out` as a nine-patch whose corners are `pad`
/// pixels, like upstream's `Border::custom`, and composites it on top.
fn overlay_nine_patch(out: &mut ImageBuf, mut texture: RgbaImage, pad: u32, encoded: bool) {
    let (dw, dh) = (out.width, out.height);
    if dh > dw {
        texture = imageops::rotate270(&texture);
    }
    // Scale with the short side. Shrink once up front, so minification
    // doesn't alias; bilinear sampling is enough for magnification.
    let scale = dw.min(dh) as f64 / texture.width().min(texture.height()) as f64;
    if scale < 1.0 {
        let size = |v: u32| ((v as f64 * scale).round() as u32).max(2 * pad + 1);
        texture = imageops::resize(
            &texture,
            size(texture.width()),
            size(texture.height()),
            imageops::FilterType::Triangle,
        );
    }
    let scale = dw.min(dh) as f64 / texture.width().min(texture.height()) as f64;
    let xs = axis(dw, texture.width(), scale, pad);
    let ys = axis(dh, texture.height(), scale, pad);
    let color: Vec<f32> = (0..=255u8)
        .map(|v| {
            let v = from_f32(v as f32 / 255.0);
            to_f32(if encoded { v } else { srgb_decode(v) })
        })
        .collect();
    let (tw, texels) = (texture.width() as usize, texture.as_raw());
    out.data
        .par_chunks_exact_mut(dw as usize * 3)
        .zip(ys.par_iter())
        .for_each(|(row, &(y0, y1, fy))| {
            for (pixel, &(x0, x1, fx)) in row.chunks_exact_mut(3).zip(&xs) {
                let taps = [
                    ((y0 * tw + x0) * 4, (1.0 - fx) * (1.0 - fy)),
                    ((y0 * tw + x1) * 4, fx * (1.0 - fy)),
                    ((y1 * tw + x0) * 4, (1.0 - fx) * fy),
                    ((y1 * tw + x1) * 4, fx * fy),
                ];
                let alpha: f32 = taps
                    .iter()
                    .map(|&(i, k)| texels[i + 3] as f32 * k)
                    .sum::<f32>()
                    / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                for (c, value) in pixel.iter_mut().enumerate() {
                    let tex: f32 = taps
                        .iter()
                        .map(|&(i, k)| color[texels[i + c] as usize] * k)
                        .sum();
                    *value = from_f32(tex * alpha + to_f32(*value) * (1.0 - alpha));
                }
            }
        });
}

/// For each destination pixel along one axis: the two texels to blend and
/// the weight of the second. The first and last `pad` pixels map 1:1 (at
/// `scale`) to the texture's ends; the middle stretches.
fn axis(dest: u32, tex: u32, scale: f64, pad: u32) -> Vec<(usize, usize, f32)> {
    let (dest, tex, pad) = (dest as f64, tex as f64, pad as f64);
    let tex_pad = pad / scale;
    (0..dest as usize)
        .map(|d| {
            let c = d as f64 + 0.5;
            let src = if c < pad {
                c / scale
            } else if c > dest - pad {
                tex - (dest - c) / scale
            } else {
                tex_pad + (c - pad) * (tex - 2.0 * tex_pad) / (dest - 2.0 * pad).max(1.0)
            };
            let f = (src - 0.5).clamp(0.0, tex - 1.0);
            let i0 = f.floor() as usize;
            (i0, (i0 + 1).min(tex as usize - 1), (f - f.floor()) as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(w: u32, h: u32) -> ImageBuf {
        ImageBuf::from_data(w, h, vec![from_f32(0.5); (w * h * 3) as usize])
    }
    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn white_border_matches_python_padding() {
        let out = add_border(&photo(200, 100), BorderStyle::White, true, &data_dir()).unwrap();
        assert_eq!((out.width, out.height), (212, 112));
        assert_eq!(out.get(0, 0), [from_f32(1.0); 3]);
        assert_eq!(out.get(6, 6), [from_f32(0.5); 3]);
        assert_eq!(out.get(5, 6), [from_f32(1.0); 3]);
    }

    #[test]
    fn keyline_surrounds_the_photo() {
        let out = add_border(&photo(1000, 500), BorderStyle::Keyline, true, &data_dir()).unwrap();
        // 30 px of white, a 4 px black line, then the photograph.
        assert_eq!((out.width, out.height), (1068, 568));
        assert_eq!(out.get(29, 100), [from_f32(1.0); 3]);
        assert_eq!(out.get(30, 100), [from_f32(0.0); 3]);
        assert_eq!(out.get(33, 33), [from_f32(0.0); 3]);
        assert_eq!(out.get(34, 100), [from_f32(0.5); 3]);
    }

    #[test]
    fn film_frame_is_black_at_the_edges_and_clear_in_the_middle() {
        for (w, h) in [(600, 400), (400, 600), (6000, 4000)] {
            let out = add_border(&photo(w, h), BorderStyle::Film, false, &data_dir()).unwrap();
            let (inset, _) = film_geometry(w, h);
            assert_eq!((out.width, out.height), (w + 2 * inset, h + 2 * inset));
            assert!(out.get(0, out.height / 2)[0] < from_f32(0.01));
            assert!(out.get(out.width / 2, out.height - 1)[0] < from_f32(0.01));
            assert_eq!(out.get(out.width / 2, out.height / 2), [from_f32(0.5); 3]);
        }
    }
}
