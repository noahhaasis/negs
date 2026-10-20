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

use crate::film35::{Renderer, Strip, linear_to_srgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    /// Plain white mat, as shown (but not exported) by the Python GUI.
    White,
    /// Scanned 120-film frame: black with an irregular edge.
    Film,
    /// White mat with a thin black line around the photograph.
    Keyline,
    /// A 35mm negative strip with the film stock's edge print, for 3:2
    /// (full frame) and 3:4 (half-frame) photographs.
    #[serde(rename = "35mm")]
    Film35,
}

impl FromStr for BorderStyle {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "white" => Self::White,
            "film" => Self::Film,
            "keyline" => Self::Keyline,
            "35mm" => Self::Film35,
            _ => bail!("unknown border style: {s} (expected white, film, keyline or 35mm)"),
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

/// The 3:4 portrait canvas Instagram shows whole, for a `width` × `height`
/// photograph with a `style` border: the film borders get a white margin
/// first, then white extends the short way as little as it can.
pub fn instagram_canvas(width: u32, height: u32, style: BorderStyle) -> (u32, u32) {
    let margin = match style {
        BorderStyle::White | BorderStyle::Keyline => 0,
        BorderStyle::Film | BorderStyle::Film35 => white_margin(width, height),
    };
    let (w, h) = (width + 2 * margin, height + 2 * margin);
    if w * 4 > h * 3 {
        (w, (w * 4).div_ceil(3))
    } else {
        ((h * 3).div_ceil(4), h)
    }
}

/// `bordered`, a photograph with a `style` border, in the middle of its
/// white Instagram canvas.
pub fn extend_for_instagram(bordered: &ImageBuf, style: BorderStyle) -> ImageBuf {
    let (w, h) = instagram_canvas(bordered.width, bordered.height, style);
    centered(bordered, w, h, 1.0)
}

/// The 35mm frame a photograph fills, chosen by its aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilmFormat {
    /// 36 × 24 mm, 3:2.
    Full,
    /// 18 × 24 mm, 3:4.
    Half,
}

impl FilmFormat {
    /// The format whose frame matches a `width` × `height` photograph in
    /// either orientation, to within 1%. The border hides the difference.
    pub fn of(width: u32, height: u32) -> Option<Self> {
        let ratio = width.max(height) as f64 / width.min(height).max(1) as f64;
        [(Self::Full, 1.5), (Self::Half, 4.0 / 3.0)]
            .into_iter()
            .find(|&(_, frame)| (ratio / frame - 1.0).abs() <= 0.01)
            .map(|(format, _)| format)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Half => "half",
        }
    }
}

/// `image` with a `style` border. `encoded` says whether the values are
/// sRGB-encoded (otherwise linear), which only matters for the textures.
/// `film` picks the 35mm border's edge print.
pub fn add_border(
    image: &ImageBuf,
    style: BorderStyle,
    film: &str,
    encoded: bool,
    data_dir: &Path,
) -> Result<ImageBuf> {
    let (w, h) = (image.width, image.height);
    Ok(match style {
        BorderStyle::White => framed(image, white_margin(w, h), 0, 1.0),
        BorderStyle::Keyline => framed(image, white_margin(w, h), keyline_width(w, h), 1.0),
        BorderStyle::Film => {
            let path = data_dir.join("borders/film.png");
            let mut texture = image::open(&path)
                .with_context(|| format!("loading border texture {}", path.display()))?
                .to_rgba8();
            if h > w {
                texture = imageops::rotate270(&texture);
            }
            let (inset, pad) = film_geometry(w, h);
            let mut out = framed(image, inset, 0, 0.0);
            overlay_nine_patch(&mut out, texture, pad, encoded);
            out
        }
        BorderStyle::Film35 => {
            let format = FilmFormat::of(w, h).with_context(|| {
                format!("The 35mm border needs a 3:2 or 3:4 photograph, not {w} × {h}")
            })?;
            let strip = Strip::load(data_dir, film, format)?;
            let (cw, ch) = strip.canvas(w, h);
            let mut out = centered(image, cw, ch, 0.0);
            overlay_strip(&mut out, &strip.renderer(cw, ch, strip.turned(w, h)), encoded);
            out
        }
    })
}

/// Draws the 35mm strip over `out`, which is the strip's canvas, at the
/// canvas's own resolution.
fn overlay_strip(out: &mut ImageBuf, renderer: &Renderer, encoded: bool) {
    let width = out.width as usize;
    out.data
        .par_chunks_exact_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let mut strip = vec![[0.0; 4]; width];
            renderer.row(y as u32, &mut strip);
            for (pixel, &[r, g, b, alpha]) in row.chunks_exact_mut(3).zip(&strip) {
                if alpha <= 0.0 {
                    continue;
                }
                for (value, c) in pixel.iter_mut().zip([r, g, b]) {
                    let c = if encoded { linear_to_srgb(c) } else { c };
                    *value = from_f32(c * alpha + to_f32(*value) * (1.0 - alpha));
                }
            }
        });
}

/// `image` in the middle of a `width` × `height` canvas of `fill`.
fn centered(image: &ImageBuf, width: u32, height: u32, fill: f32) -> ImageBuf {
    let (w, h) = (image.width as usize, image.height as usize);
    let (cw, ch) = (width as usize, height as usize);
    let (x0, y0) = ((cw - w) / 2, (ch - h) / 2);
    let mut data = vec![from_f32(fill); cw * ch * 3];
    data.par_chunks_exact_mut(cw * 3)
        .skip(y0)
        .take(h)
        .zip(image.data.par_chunks_exact(w * 3))
        .for_each(|(row, src)| row[x0 * 3..(x0 + w) * 3].copy_from_slice(src));
    ImageBuf::from_data(width, height, data)
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
/// With no `pad` it scales the whole texture.
fn overlay_nine_patch(out: &mut ImageBuf, mut texture: RgbaImage, pad: u32, encoded: bool) {
    let (dw, dh) = (out.width, out.height);
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
        let out = add_border(&photo(200, 100), BorderStyle::White, "", true, &data_dir()).unwrap();
        assert_eq!((out.width, out.height), (212, 112));
        assert_eq!(out.get(0, 0), [from_f32(1.0); 3]);
        assert_eq!(out.get(6, 6), [from_f32(0.5); 3]);
        assert_eq!(out.get(5, 6), [from_f32(1.0); 3]);
    }

    #[test]
    fn keyline_surrounds_the_photo() {
        let out = add_border(
            &photo(1000, 500),
            BorderStyle::Keyline,
            "",
            true,
            &data_dir(),
        )
        .unwrap();
        // 30 px of white, a 4 px black line, then the photograph.
        assert_eq!((out.width, out.height), (1068, 568));
        assert_eq!(out.get(29, 100), [from_f32(1.0); 3]);
        assert_eq!(out.get(30, 100), [from_f32(0.0); 3]);
        assert_eq!(out.get(33, 33), [from_f32(0.0); 3]);
        assert_eq!(out.get(34, 100), [from_f32(0.5); 3]);
    }

    #[test]
    fn instagram_extends_the_short_way_to_3_by_4() {
        use BorderStyle::*;
        // Landscape grows down, a tall portrait grows sideways, and a
        // canvas that's already 3:4 stays as it is.
        assert_eq!(instagram_canvas(1068, 568, Keyline), (1068, 1424));
        assert_eq!(instagram_canvas(1000, 2000, White), (1500, 2000));
        assert_eq!(instagram_canvas(300, 400, White), (300, 400));
        assert_eq!(instagram_canvas(301, 400, White), (301, 402));
        // The film borders get a 3% white margin first.
        assert_eq!(instagram_canvas(1000, 400, Film), (1060, 1414));
        assert_eq!(instagram_canvas(300, 400, Film35), (324, 432));

        let bordered = add_border(&photo(200, 100), White, "", true, &data_dir()).unwrap();
        let out = extend_for_instagram(&bordered, White);
        assert_eq!((out.width, out.height), (212, 283));
        assert_eq!(out.get(106, 141), [from_f32(0.5); 3]);
        assert_eq!(out.get(106, 10), [from_f32(1.0); 3]);

        let bordered = add_border(&photo(600, 400), Film, "", true, &data_dir()).unwrap();
        let out = extend_for_instagram(&bordered, Film);
        assert_eq!((out.width, out.height), instagram_canvas(602, 402, Film));
        assert_eq!(out.get(0, out.height / 2), [from_f32(1.0); 3]);
        assert!(out.get(18, out.height / 2)[0] < from_f32(0.01));
    }

    #[test]
    fn film_format_follows_the_aspect_ratio() {
        assert_eq!(FilmFormat::of(6000, 4000), Some(FilmFormat::Full));
        assert_eq!(FilmFormat::of(4000, 6000), Some(FilmFormat::Full));
        assert_eq!(FilmFormat::of(7952, 5304), Some(FilmFormat::Full));
        assert_eq!(FilmFormat::of(3000, 4000), Some(FilmFormat::Half));
        assert_eq!(FilmFormat::of(4032, 3024), Some(FilmFormat::Half));
        // 1.52 is more than 1% off 3:2.
        assert_eq!(FilmFormat::of(1520, 1000), None);
        assert_eq!(FilmFormat::of(1920, 1080), None);
        assert_eq!(FilmFormat::of(100, 100), None);
    }

    #[test]
    fn every_film_profile_has_a_35mm_edge_print() {
        use crate::film35::{Kind, STOCKS};
        let data = data_dir();
        let mut films = Vec::new();
        for entry in std::fs::read_dir(data.join("profiles")).unwrap() {
            let path = entry.unwrap().path();
            let profile: serde_json::Value =
                serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
            let info = &profile["info"];
            if info["support"] != "film" {
                continue;
            }
            let stock = path.file_stem().unwrap().to_str().unwrap().to_owned();
            // Tri-X Reversal 7266 isn't made in 35mm: use the generic strip.
            if stock == "kodak_trix" {
                assert!(STOCKS.iter().all(|(id, ..)| *id != stock));
                continue;
            }
            let (_, kind, _) = STOCKS
                .iter()
                .find(|(id, ..)| *id == stock)
                .unwrap_or_else(|| panic!("no edge print for {stock}"));
            let expected = if info["type"] == "positive" {
                Kind::Slide
            } else if info["channel_model"] == "bw" {
                Kind::BlackAndWhite
            } else {
                Kind::Negative
            };
            assert_eq!(*kind, expected, "{stock}");
            films.push(stock);
        }
        assert_eq!(films.len(), STOCKS.len());
        // The macOS preview shows prerendered strips.
        for stock in STOCKS.iter().map(|(id, ..)| *id).chain(["generic"]) {
            for format in ["full", "half"] {
                let texture = data.join(format!("borders/35mm/{format}/{stock}.png"));
                assert!(texture.exists(), "missing {}", texture.display());
            }
        }
    }

    #[test]
    fn film35_preview_window_matches_the_renderer() {
        let frames: serde_json::Value = serde_json::from_slice(
            &std::fs::read(data_dir().join("borders/35mm/frames.json")).unwrap(),
        )
        .unwrap();
        for format in [FilmFormat::Full, FilmFormat::Half] {
            let window = &frames[format.name()]["window"];
            for (i, v) in Strip::window(format).iter().enumerate() {
                assert!((window[i].as_f64().unwrap() - v).abs() < 1e-5, "{}", format.name());
            }
        }
    }

    #[test]
    fn film35_strip_surrounds_the_photo() {
        // Full frame in both orientations, then half-frame in both.
        for (w, h) in [(600, 400), (400, 600), (300, 400), (400, 300)] {
            let out = add_border(
                &photo(w, h),
                BorderStyle::Film35,
                "kodak_portra_400",
                false,
                &data_dir(),
            )
            .unwrap();
            let strip = Strip::new("kodak_portra_400", FilmFormat::of(w, h).unwrap());
            assert_eq!((out.width, out.height), strip.canvas(w, h));
            // A full frame lies along the strip and a half-frame across it:
            // the 35 mm film is 24 mm of picture and 11 mm of rebate.
            let full = w.max(h) * 2 == w.min(h) * 3;
            let (along, across) = if full == (w > h) {
                (out.width as f64 / w as f64, out.height as f64 / h as f64)
            } else {
                (out.height as f64 / h as f64, out.width as f64 / w as f64)
            };
            // 38 mm of strip per 36 mm frame, 19 mm per 18 mm half-frame.
            assert!((along - 19.0 / 18.0).abs() < 0.01, "{w}x{h}: {along}");
            assert!((across - 35.0 / 24.0).abs() < 0.01, "{w}x{h}: {across}");
            assert_eq!(out.get(out.width / 2, out.height / 2), [from_f32(0.5); 3]);
            assert!(out.get(0, 0)[0] < from_f32(0.03));
        }
    }

    #[test]
    fn film35_hides_a_slightly_wider_photo_under_the_rebate() {
        let out = add_border(
            &photo(604, 400),
            BorderStyle::Film35,
            "kodak_gold_200",
            false,
            &data_dir(),
        )
        .unwrap();
        let strip = Strip::new("kodak_gold_200", FilmFormat::Full);
        assert_eq!((out.width, out.height), strip.canvas(604, 400));
        // Just outside the window, at mid-height, is rebate, not photograph.
        let edge = (Strip::window(FilmFormat::Full)[0] * out.width as f64) as u32;
        assert!(out.get(edge.saturating_sub(2), out.height / 2)[0] < from_f32(0.05));
    }

    #[test]
    fn film35_needs_a_film_frame_ratio() {
        let err = add_border(
            &photo(160, 90),
            BorderStyle::Film35,
            "kodak_gold_200",
            true,
            &data_dir(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("3:2"), "{err}");
    }

    #[test]
    fn film35_falls_back_to_the_generic_strip() {
        let out = add_border(
            &photo(600, 400),
            BorderStyle::Film35,
            "not_a_film",
            true,
            &data_dir(),
        )
        .unwrap();
        assert!(out.width > 600);
    }

    #[test]
    fn film_frame_is_black_at_the_edges_and_clear_in_the_middle() {
        for (w, h) in [(600, 400), (400, 600), (6000, 4000)] {
            let out = add_border(&photo(w, h), BorderStyle::Film, "", false, &data_dir()).unwrap();
            let (inset, _) = film_geometry(w, h);
            assert_eq!((out.width, out.height), (w + 2 * inset, h + 2 * inset));
            assert!(out.get(0, out.height / 2)[0] < from_f32(0.01));
            assert!(out.get(out.width / 2, out.height - 1)[0] < from_f32(0.01));
            assert_eq!(out.get(out.width / 2, out.height / 2), [from_f32(0.5); 3]);
        }
    }
}
