//! Export sizes. Like a border, the size applies to the finished print:
//! the photograph is developed at its working resolution, cropped and
//! bordered, and only then resampled, so a smaller file shows the same
//! grain and halation as a full-size one, only with fewer pixels.

use std::f64::consts::PI;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f64, srgb_decode, srgb_encode, to_f64};

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
