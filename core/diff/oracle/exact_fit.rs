//! The fitted view's exact-size picture (ticket 38): the developed image
//! resampled once to the very pixels the fit shows, with Lanczos-3 in
//! linear light as Pillow's `Image.LANCZOS` does it, then drawn 1:1 on whole
//! pixels. Trilinear mipmaps between two powers of two lose about half the
//! fine detail; this keeps it. Display only: exports never come here.
//!
//! The GPU runs the same filter in `gpu.rs` (`Gpu::lanczos`); this is the
//! software canvas's, and the reference the tests hold both to.

use std::sync::Arc;

/// Lanczos-3's reach, in input pixels at scale 1.
const SUPPORT: f64 = 3.0;

/// The exact picture's size for a `w` × `h` photograph (its layout size,
/// before crop and turns) shown at `z` physical pixels per photo pixel.
pub fn target(w: f64, h: f64, z: f64) -> (u32, u32) {
    (((w * z).round() as u32).max(1), ((h * z).round() as u32).max(1))
}

/// Whether the fitted view should draw an exact picture: shrinking, and the
/// texture (smaller than the layout with Limit preview) larger than the target.
pub fn worth_it(z: f64, texture: (u32, u32), target: (u32, u32)) -> bool {
    z < 0.999 && (texture.0 > target.0 || texture.1 > target.1)
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

/// Pillow's `lanczos_filter`.
fn lanczos(x: f64) -> f64 {
    if (-SUPPORT..SUPPORT).contains(&x) { sinc(x) * sinc(x / SUPPORT) } else { 0.0 }
}

/// For each of `output` pixels along an axis of `input`, the first input
/// pixel it reads and the normalised weights from there (Pillow's
/// `precompute_coeffs`: the kernel widened by the reduction, cut at the
/// edges and renormalised).
pub fn coefficients(input: u32, output: u32) -> Vec<(u32, Vec<f32>)> {
    let scale = input as f64 / output as f64;
    let filterscale = scale.max(1.0);
    let support = SUPPORT * filterscale;
    let ss = 1.0 / filterscale;
    (0..output)
        .map(|x| {
            let center = (x as f64 + 0.5) * scale;
            // C's `(int)` truncates toward zero, then Pillow clamps.
            let min = ((center - support + 0.5) as i64).max(0) as u32;
            let max = ((center + support + 0.5) as i64).min(input as i64) as u32;
            let mut weights: Vec<f64> = (min..max).map(|i| lanczos((i as f64 - center + 0.5) * ss)).collect();
            let sum: f64 = weights.iter().sum();
            if sum != 0.0 {
                for w in &mut weights {
                    *w /= sum;
                }
            }
            (min, weights.into_iter().map(|w| w as f32).collect())
        })
        .collect()
}

fn decode(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// Resamples sRGB RGBA `w` × `h` to `tw` × `th` in linear light: across,
/// then down, as Pillow does, in floats between. Opaque out.
pub fn resample(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let lut: [f32; 256] = std::array::from_fn(|i| decode(i as f32 / 255.0));
    let across = Arc::new(coefficients(w, tw));
    let down = coefficients(h, th);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    // Across: h rows of tw linear pixels.
    let mut middle = vec![0.0f32; tw as usize * h as usize * 3];
    let rows_each = (h as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk, out) in middle.chunks_mut(rows_each * tw as usize * 3).enumerate() {
            let across = &across;
            let lut = &lut;
            scope.spawn(move || {
                for (r, row) in out.chunks_mut(tw as usize * 3).enumerate() {
                    let y = chunk * rows_each + r;
                    let source = &rgba[y * w as usize * 4..(y + 1) * w as usize * 4];
                    for (x, (start, weights)) in across.iter().enumerate() {
                        let mut sum = [0.0f32; 3];
                        for (k, weight) in weights.iter().enumerate() {
                            let p = (*start as usize + k) * 4;
                            for c in 0..3 {
                                sum[c] += weight * lut[source[p + c] as usize];
                            }
                        }
                        row[x * 3..x * 3 + 3].copy_from_slice(&sum);
                    }
                }
            });
        }
    });
    // Down: th rows of tw pixels, encoded.
    let mut out = vec![255u8; tw as usize * th as usize * 4];
    let rows_each = (th as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk, part) in out.chunks_mut(rows_each * tw as usize * 4).enumerate() {
            let down = &down;
            let middle = &middle;
            scope.spawn(move || {
                for (r, row) in part.chunks_mut(tw as usize * 4).enumerate() {
                    let (start, weights) = &down[chunk * rows_each + r];
                    for x in 0..tw as usize {
                        let mut sum = [0.0f32; 3];
                        for (k, weight) in weights.iter().enumerate() {
                            let p = ((*start as usize + k) * tw as usize + x) * 3;
                            for c in 0..3 {
                                sum[c] += weight * middle[p + c];
                            }
                        }
                        for c in 0..3 {
                            row[x * 4 + c] = (encode(sum[c]) * 255.0).round() as u8;
                        }
                    }
                }
            });
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_round_the_fitted_size() {
        assert_eq!(target(6000.0, 4000.0, 2200.0 / 6000.0), (2200, 1467));
        assert_eq!(target(4896.0, 3264.0, 1035.0 / 3264.0), (1553, 1035));
        assert_eq!(target(333.0, 777.0, 0.5), (167, 389));
    }

    #[test]
    fn only_shrinking_larger_textures() {
        assert!(worth_it(0.317, (4896, 3264), (1553, 1035)));
        assert!(!worth_it(1.0, (4896, 3264), (4896, 3264)));
        // Limit preview: the texture is already smaller than the fit.
        assert!(!worth_it(0.3, (1200, 800), (1553, 1035)));
    }

    #[test]
    fn coefficients_follow_pillow() {
        // 2:1 widens the kernel to 12 taps, symmetric about the centre.
        let c = coefficients(100, 50);
        let (start, weights) = &c[25];
        assert_eq!(*start, 45);
        assert_eq!(weights.len(), 12);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        for k in 0..6 {
            assert!((weights[k] - weights[11 - k]).abs() < 1e-6);
        }
        // At the edge the kernel is cut and renormalised.
        let (start, weights) = &c[0];
        assert_eq!(*start, 0);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn flat_stays_flat_and_detail_survives() {
        let (w, h) = (97, 61);
        let grey = vec![128u8; w * h * 4];
        let out = resample(&grey, w as u32, h as u32, 40, 25);
        assert!(out.chunks(4).all(|p| p[0] == 128 && p[1] == 128 && p[2] == 128 && p[3] == 255));
        // Stripes three pixels wide stay stripes at 0.6.
        let stripes: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| if (x / 3) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] })).collect();
        let out = resample(&stripes, w as u32, h as u32, 58, 37);
        let row: Vec<u8> = out[12 * 58 * 4..13 * 58 * 4].chunks(4).map(|p| p[0]).collect();
        let (lo, hi) = (*row[4..54].iter().min().unwrap(), *row[4..54].iter().max().unwrap());
        assert!(hi > 200 && lo < 60, "{lo}..{hi}");
    }
}
