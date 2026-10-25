//! Resampling. Export sizes ([`fit`]) apply to the finished print, like a
//! border: the photograph is developed at its working resolution, cropped and
//! bordered, and only then resampled, so a smaller file shows the same grain
//! and halation as a full-size one, only with fewer pixels. Image Size
//! ([`catmull_rom`]) is the opposite: it resizes the source before the film,
//! so grain and halation are developed at the new size.

use std::f64::consts::PI;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32, from_f64, srgb_decode, srgb_encode, to_f32, to_f64};

/// The edge an export is fitted to, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    LongEdge(u32),
    ShortEdge(u32),
}

impl Fit {
    /// The size of a `width` × `height` image fitted to this edge, keeping
    /// its aspect ratio. Never larger than the image: exports are not enlarged.
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        let (edge, target) = match self {
            Fit::LongEdge(px) => (width.max(height), px),
            Fit::ShortEdge(px) => (width.min(height), px),
        };
        if target == 0 || target >= edge {
            return (width, height);
        }
        let scale = f64::from(target) / f64::from(edge);
        let side = |v: u32| {
            if v == edge {
                target
            } else {
                ((f64::from(v) * scale).round() as u32).max(1)
            }
        };
        (side(width), side(height))
    }
}

/// `image` fitted to `fit`, or `None` if it already fits. Resamples with a
/// Lanczos-3 filter in linear light: an `encoded` (sRGB) image is decoded
/// first and encoded again after, so fine grain averages to the same
/// brightness it has at full size.
pub fn fit(image: &ImageBuf, fit: Fit, encoded: bool) -> Option<ImageBuf> {
    let (width, height) = fit.dimensions(image.width, image.height);
    if (width, height) == (image.width, image.height) {
        return None;
    }
    let decode = |v: Scalar| to_f64(if encoded { srgb_decode(v) } else { v });
    let encode = |v: f64| {
        let v = from_f64(v);
        if encoded { srgb_encode(v) } else { v }
    };
    let (sw, dw) = (image.width as usize, width as usize);
    let (columns, rows) = (taps(sw, dw), taps(image.height as usize, height as usize));

    // Across: each source row to `dw` linear pixels.
    let mut across = vec![0f64; dw * image.height as usize * 3];
    across
        .par_chunks_mut(dw * 3)
        .zip(image.data.par_chunks(sw * 3))
        .for_each(|(line, row)| {
            let row: Vec<f64> = row.iter().map(|&v| decode(v)).collect();
            for (x, (first, weights)) in columns.iter().enumerate() {
                let mut px = [0f64; 3];
                for (k, &w) in weights.iter().enumerate() {
                    for c in 0..3 {
                        px[c] += w * row[(first + k) * 3 + c];
                    }
                }
                line[x * 3..x * 3 + 3].copy_from_slice(&px);
            }
        });

    // Down: each output row from the rows it covers.
    let mut data = vec![from_f64(0.0); dw * height as usize * 3];
    data.par_chunks_mut(dw * 3)
        .zip(rows.par_iter())
        .for_each(|(line, (first, weights))| {
            let mut sum = vec![0f64; dw * 3];
            for (k, &w) in weights.iter().enumerate() {
                let row = &across[(first + k) * dw * 3..(first + k + 1) * dw * 3];
                for (s, &v) in sum.iter_mut().zip(row) {
                    *s += w * v;
                }
            }
            for (out, s) in line.iter_mut().zip(sum) {
                *out = encode(s.max(0.0));
            }
        });
    Some(ImageBuf::from_data(width, height, data))
}

/// Per output pixel along an axis: the first source pixel it reads and the
/// normalised Lanczos-3 weights from there, stretched by the reduction so
/// the filter also low-passes.
fn taps(src: usize, dst: usize) -> Vec<(usize, Vec<f64>)> {
    let scale = (src as f64 / dst as f64).max(1.0);
    let support = 3.0 * scale;
    (0..dst)
        .map(|i| {
            let center = (i as f64 + 0.5) * src as f64 / dst as f64;
            let first = (center - support).floor().max(0.0) as usize;
            let last = ((center + support).ceil() as usize).min(src);
            let mut weights: Vec<f64> = (first..last)
                .map(|j| lanczos3((j as f64 + 0.5 - center) / scale))
                .collect();
            let total: f64 = weights.iter().sum();
            weights.iter_mut().for_each(|w| *w /= total);
            (first, weights)
        })
        .collect()
}

/// `image` resampled to `width` × `height` with a Catmull–Rom cubic, before
/// the film: the working image of Image Size, the legacy working-resolution
/// factor, and the 12 MP preview.
///
/// The arithmetic is `image::imageops::resize` with `CatmullRom` in f32,
/// down the columns then across the rows, with the same weights, pixel
/// centres, edges and order of accumulation. Unlike it, values above 1 are
/// kept: RAW highlights reach halation and auto exposure unclipped. Only the
/// undershoot below zero is floored, where `image` clamps to [0, 1]. Each
/// output row is filtered down and then across in one go, so no
/// intermediate image is allocated.
///
/// Both sizes must be at least one pixel; [`crate::image_size::resolve`]
/// validates a requested size before anything is allocated.
pub fn catmull_rom(image: &ImageBuf, (width, height): (u32, u32)) -> ImageBuf {
    assert!(image.width > 0 && image.height > 0 && width > 0 && height > 0);
    let (sw, dw) = (image.width as usize, width as usize);
    let (columns, rows) = (catmull_rom_taps(image.width, width), catmull_rom_taps(image.height, height));
    let mut data = vec![from_f64(0.0); dw * height as usize * 3];
    data.par_chunks_mut(dw * 3)
        .zip(rows.par_iter())
        .for_each_init(
            || vec![0f32; sw * 3],
            |down, (line, (first, weights))| {
                // Down: this output row from the source rows it covers.
                down.fill(0.0);
                for (k, &w) in weights.iter().enumerate() {
                    let row = &image.data[(first + k) * sw * 3..(first + k + 1) * sw * 3];
                    for (d, &v) in down.iter_mut().zip(row) {
                        *d += to_f32(v) * w;
                    }
                }
                // Across, flooring the undershoot only.
                for (x, (first, weights)) in columns.iter().enumerate() {
                    let mut px = [0f32; 3];
                    for (k, &w) in weights.iter().enumerate() {
                        for c in 0..3 {
                            px[c] += down[(first + k) * 3 + c] * w;
                        }
                    }
                    for c in 0..3 {
                        line[x * 3 + c] = from_f32(if px[c] < 0.0 { 0.0 } else { px[c] });
                    }
                }
            },
        );
    ImageBuf::from_data(width, height, data)
}

/// Per output pixel along an axis: the first source pixel it reads and the
/// normalised Catmull–Rom weights from there, computed in f32 exactly as
/// `image` 0.25 does (`horizontal_sample`/`vertical_sample`).
fn catmull_rom_taps(src: u32, dst: u32) -> Vec<(usize, Vec<f32>)> {
    let ratio = src as f32 / dst as f32;
    let sratio = if ratio < 1.0 { 1.0 } else { ratio };
    let support = 2.0 * sratio;
    (0..dst)
        .map(|out| {
            let input = (out as f32 + 0.5) * ratio;
            let left = ((input - support).floor() as i64).clamp(0, i64::from(src) - 1);
            let right = ((input + support).ceil() as i64).clamp(left + 1, i64::from(src));
            // The kernel puts a pixel's centre at zero.
            let input = input - 0.5;
            let mut weights = Vec::with_capacity((right - left) as usize);
            let mut sum = 0.0f32;
            for i in left..right {
                let w = bc_cubic((i as f32 - input) / sratio, 0.0, 0.5);
                weights.push(w);
                sum += w;
            }
            for w in &mut weights {
                *w /= sum;
            }
            (left as usize, weights)
        })
        .collect()
}

/// Mitchell–Netravali cubic; B = 0, C = 0.5 is Catmull–Rom. As in `image`.
fn bc_cubic(x: f32, b: f32, c: f32) -> f32 {
    let a = x.abs();
    let k = if a < 1.0 {
        (12.0 - 9.0 * b - 6.0 * c) * a.powi(3) + (-18.0 + 12.0 * b + 6.0 * c) * a.powi(2) + (6.0 - 2.0 * b)
    } else if a < 2.0 {
        (-b - 6.0 * c) * a.powi(3)
            + (6.0 * b + 30.0 * c) * a.powi(2)
            + (-12.0 * b - 48.0 * c) * a
            + (8.0 * b + 24.0 * c)
    } else {
        0.0
    };
    k / 6.0
}

fn lanczos3(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else if x.abs() < 3.0 {
        let px = PI * x;
        3.0 * px.sin() * (px / 3.0).sin() / (px * px)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_fit_the_edge_and_never_enlarge() {
        assert_eq!(Fit::LongEdge(2048).dimensions(6000, 4000), (2048, 1365));
        assert_eq!(Fit::LongEdge(2048).dimensions(4000, 6000), (1365, 2048));
        assert_eq!(Fit::ShortEdge(1080).dimensions(6000, 4000), (1620, 1080));
        assert_eq!(Fit::LongEdge(1440).dimensions(3000, 4000), (1080, 1440));
        assert_eq!(Fit::LongEdge(8000).dimensions(6000, 4000), (6000, 4000));
        assert_eq!(Fit::ShortEdge(4000).dimensions(6000, 4000), (6000, 4000));
        assert_eq!(Fit::LongEdge(100).dimensions(10_000, 10), (100, 1));
    }

    #[test]
    fn flat_images_stay_flat() {
        let image = ImageBuf::from_data(300, 200, vec![from_f64(0.5); 300 * 200 * 3]);
        for encoded in [false, true] {
            let small = fit(&image, Fit::LongEdge(64), encoded).unwrap();
            assert_eq!((small.width, small.height), (64, 43));
            assert!(small.data.iter().all(|&v| (to_f64(v) - 0.5).abs() < 1e-5));
        }
        assert!(fit(&image, Fit::LongEdge(300), true).is_none());
    }

    /// The old early resize: `image`'s Catmull–Rom, clamped to [0, 1].
    fn image_crate(image: &ImageBuf, (w, h): (u32, u32)) -> Vec<f32> {
        let data: Vec<f32> = image.data.iter().map(|&v| to_f32(v)).collect();
        let src = ::image::ImageBuffer::<::image::Rgb<f32>, _>::from_raw(image.width, image.height, data).unwrap();
        ::image::imageops::resize(&src, w, h, ::image::imageops::FilterType::CatmullRom).into_raw()
    }

    fn noise(width: u32, height: u32, scale: f64, seed: u64) -> ImageBuf {
        let mut state = seed | 1;
        let data = (0..width * height * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                from_f64((state >> 11) as f64 / (1u64 << 53) as f64 * scale)
            })
            .collect();
        ImageBuf::from_data(width, height, data)
    }

    /// Where `image` didn't clamp, the result is the same to the bit; where it
    /// clamped at 1, the value is kept, and at 0 both floor the undershoot.
    #[test]
    fn catmull_rom_matches_the_image_crate_wherever_it_did_not_clip() {
        let sizes = [(37, 23, 101, 59), (101, 59, 37, 23), (64, 48, 64, 97), (64, 48, 13, 48), (5, 3, 1, 1), (1, 1, 7, 4), (40, 30, 120, 90)];
        for (i, &(sw, sh, dw, dh)) in sizes.iter().enumerate() {
            for scale in [0.98, 2.5] {
                let image = noise(sw, sh, scale, 7 + i as u64);
                let old = image_crate(&image, (dw, dh));
                let new = catmull_rom(&image, (dw, dh));
                assert_eq!((new.width, new.height), (dw, dh));
                let mut kept = 0;
                for (k, (&a, &b)) in old.iter().zip(&new.data).enumerate() {
                    let b = to_f32(b);
                    if a < 1.0 {
                        assert_eq!(a.to_bits(), b.to_bits(), "{sw}x{sh}->{dw}x{dh} at {k}: {a} vs {b}");
                    } else {
                        assert!(b >= 1.0, "{b}");
                        kept += usize::from(b > 1.0);
                    }
                }
                // With sources up to 2.5 the clamp was active and values survive.
                // (Below 1, cubic overshoot can also exceed 1 when enlarging.)
                if scale > 1.0 {
                    assert!(kept > 0, "{sw}x{sh}->{dw}x{dh}");
                }
            }
        }
    }

    #[test]
    fn catmull_rom_keeps_highlights_and_floors_undershoot() {
        for value in [0.0, 0.18, 1.0, 2.5] {
            let flat = ImageBuf::from_data(9, 7, vec![from_f64(value); 9 * 7 * 3]);
            for size in [(27, 21), (4, 3)] {
                let out = catmull_rom(&flat, size);
                assert!(out.data.iter().all(|&v| (to_f64(v) - value).abs() < 1e-6), "{value} at {size:?}");
            }
        }
        // A bright impulse above 1 keeps its energy past the clip, and the
        // cubic's negative lobes floor at zero rather than going negative.
        let mut impulse = ImageBuf::from_data(9, 9, vec![from_f64(0.0); 9 * 9 * 3]);
        impulse.set(4, 4, [from_f64(40.0); 3]);
        let out = catmull_rom(&impulse, (27, 27));
        let peak = out.data.iter().map(|&v| to_f64(v)).fold(0.0, f64::max);
        assert!(peak > 30.0, "{peak}");
        assert!(out.data.iter().all(|&v| to_f64(v) >= 0.0));
        // Overshoot of a [0, 1] edge rises above 1: the old resize clipped it.
        let edge: Vec<Scalar> = (0..16 * 3).map(|i| from_f64(if i / 3 < 8 { 0.0 } else { 1.0 })).collect();
        let edge = ImageBuf::from_data(16, 1, edge);
        let up = catmull_rom(&edge, (48, 1));
        assert!(up.data.iter().any(|&v| to_f64(v) > 1.0));
        assert!(image_crate(&edge, (48, 1)).iter().all(|&v| v <= 1.0));
    }

    #[test]
    fn halving_averages_in_linear_light() {
        // Alternating black and white columns average to half the light,
        // which sRGB encodes brighter than 0.5.
        let data = (0..4 * 64 * 3)
            .map(|i| from_f64(((i / 3) % 2) as f64))
            .collect();
        let image = ImageBuf::from_data(64, 4, data);
        let half = fit(&image, Fit::LongEdge(32), true).unwrap();
        let mid = to_f64(half.data[(32 + 16) * 3]);
        assert!(
            (mid - to_f64(srgb_encode(from_f64(0.5)))).abs() < 0.02,
            "{mid}"
        );
    }
}
