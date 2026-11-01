//! Self-guided luminance base used by Resolve 21.1 CameraRawAdjustFilter.
//!
//! Recovered call: GuidedFilter(gray, base, gray, 0.5, 0.01, 4, false).
//! Evidence: work/resolve-raw-rea-2026-10-26/rawcontrols-helper-*.asm and the
//! shipped ImageFilters source. This preserves the observed schedule, radius,
//! edge replication and coefficient equations, not GPU/MPS rounding bit-for-bit.
//! It requires the complete frame: using a cropped tile changes its context.
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

fn cancelled(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|v| v.load(Ordering::Relaxed)) {
        Err("Render cancelled".into())
    } else {
        Ok(())
    }
}

fn schedule(w: usize, h: usize) -> (usize, usize, usize) {
    // GuidedFilterPitch 0x106342814..2878: keep both f32 multiplications.
    let radius = ((w.max(h) as f32 * 0.02_f32) * 0.5_f32).ceil() as usize;
    let mut sub = (w / 320).clamp(1, 4);
    if sub > radius || h < sub {
        // The height guard is a host safety extension for degenerate 1-D inputs;
        // real photographs follow the recovered schedule unchanged.
        sub = 1;
    }
    (w / sub, h / sub, (radius / sub).max(1))
}

// Fixed-size (2r+1) box mean, edge pixels replicated. Resolve uses MPSImageBox
// with edgeMode=Clamp or equivalent separable box kernels. f64 running sums
// avoid long-row drift; each directional result is rounded back to f32.
fn box_mean(
    src: &[f32],
    w: usize,
    h: usize,
    r: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<f32>, String> {
    let divisor = (2 * r + 1) as f64;
    let mut horizontal = vec![0.0_f32; src.len()];
    for y in 0..h {
        cancelled(cancel)?;
        let row = &src[y * w..(y + 1) * w];
        let mut sum = row[0] as f64 * (r + 1) as f64;
        sum += row[1..=r.min(w - 1)].iter().map(|&v| v as f64).sum::<f64>();
        sum += row[w - 1] as f64 * r.saturating_sub(w - 1) as f64;
        for x in 0..w {
            horizontal[y * w + x] = (sum / divisor) as f32;
            sum += row[(x + r + 1).min(w - 1)] as f64 - row[x.saturating_sub(r)] as f64;
        }
    }
    let mut sums = vec![0.0_f64; w];
    for x in 0..w {
        sums[x] = horizontal[x] as f64 * (r + 1) as f64
            + horizontal[(h - 1) * w + x] as f64 * r.saturating_sub(h - 1) as f64;
    }
    for y in 1..=r.min(h - 1) {
        for x in 0..w {
            sums[x] += horizontal[y * w + x] as f64;
        }
    }
    let mut output = vec![0.0_f32; src.len()];
    for y in 0..h {
        cancelled(cancel)?;
        let incoming = (y + r + 1).min(h - 1) * w;
        let outgoing = y.saturating_sub(r) * w;
        for x in 0..w {
            output[y * w + x] = (sums[x] / divisor) as f32;
            sums[x] += horizontal[incoming + x] as f64 - horizontal[outgoing + x] as f64;
        }
    }
    Ok(output)
}

#[derive(Clone, Copy)]
struct Axis {
    lo: usize,
    hi: usize,
    t: f32,
}
fn axis(src: usize, dst: usize) -> Vec<Axis> {
    // ResizeParams 0x1063cee78 emits scale=1/dst and offset=.5/dst;
    // normalized linear sampling evaluates u*src-.5, not u*(src-1).
    let scale = 1.0 / dst as f32;
    (0..dst)
        .map(|i| {
            let coordinate =
                ((i as f32 * scale + 0.5 * scale) * src as f32 - 0.5).clamp(0.0, (src - 1) as f32);
            let lo = coordinate.floor() as usize;
            Axis {
                lo,
                hi: (lo + 1).min(src - 1),
                t: coordinate - lo as f32,
            }
        })
        .collect()
}
fn bilinear(src: &[f32], w: usize, x: Axis, y: Axis) -> f32 {
    let a = src[y.lo * w + x.lo];
    let b = src[y.lo * w + x.hi];
    let c = src[y.hi * w + x.lo];
    let d = src[y.hi * w + x.hi];
    let top = a + (b - a) * x.t;
    let bottom = c + (d - c) * x.t;
    top + (bottom - top) * y.t
}
fn resize(
    src: &[f32],
    w: usize,
    h: usize,
    dw: usize,
    dh: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<f32>, String> {
    let xs = axis(w, dw);
    let ys = axis(h, dh);
    let mut dst = vec![0.0; dw * dh];
    for y in 0..dh {
        cancelled(cancel)?;
        for x in 0..dw {
            dst[y * dw + x] = bilinear(src, w, xs[x], ys[y]);
        }
    }
    Ok(dst)
}

pub(super) fn base_luma(
    luma: &[f32],
    w: usize,
    h: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<f32>, String> {
    cancelled(cancel)?;
    if w == 0
        || h == 0
        || w > i32::MAX as usize
        || h > i32::MAX as usize
        || w.checked_mul(h) != Some(luma.len())
    {
        return Err("Invalid RAW guided-base dimensions".into());
    }
    for block in luma.chunks(16384) {
        cancelled(cancel)?;
        if block
            .iter()
            .any(|v| !v.is_finite() || !(*v * *v).is_finite())
        {
            return Err("Non-finite RAW guided-base input".into());
        }
    }
    let (lw, lh, radius) = schedule(w, h);
    let small = if (lw, lh) == (w, h) {
        Cow::Borrowed(luma)
    } else {
        Cow::Owned(resize(luma, w, h, lw, lh, cancel)?)
    };
    let mut b = box_mean(&small, lw, lh, radius, cancel)?;
    let square: Vec<f32> = small.iter().map(|&v| v * v).collect();
    let mut a = box_mean(&square, lw, lh, radius, cancel)?;
    drop(square);
    drop(small);
    for (aa, bb) in a.chunks_mut(16384).zip(b.chunks_mut(16384)) {
        cancelled(cancel)?;
        for (a, b) in aa.iter_mut().zip(bb) {
            // Variance(...,true) selects varianceClampedNegativeKernel.
            let variance = (*a - *b * *b).max(0.0);
            *a = variance / (variance + 0.01).max(0.0001);
            *b = *b - *a * *b;
        }
    }
    let mean_a = box_mean(&a, lw, lh, radius, cancel)?;
    drop(a);
    let mean_b = box_mean(&b, lw, lh, radius, cancel)?;
    drop(b);
    let mut output = vec![0.0; luma.len()];
    if (lw, lh) == (w, h) {
        for y in 0..h {
            cancelled(cancel)?;
            for i in y * w..(y + 1) * w {
                output[i] = mean_a[i] * luma[i] + mean_b[i];
            }
        }
    } else {
        // Sample both small coefficient fields directly, avoiding two full-frame
        // intermediates while preserving the observed upsample-then-combine math.
        let xs = axis(lw, w);
        let ys = axis(lh, h);
        for y in 0..h {
            cancelled(cancel)?;
            for x in 0..w {
                let a = bilinear(&mean_a, lw, xs[x], ys[y]);
                let b = bilinear(&mean_b, lw, xs[x], ys[y]);
                output[y * w + x] = a * luma[y * w + x] + b;
            }
        }
    }
    cancelled(cancel)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn brute_box(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
        (0..src.len())
            .map(|i| {
                let mut sum = 0.0_f64;
                for dy in -(r as isize)..=r as isize {
                    for dx in -(r as isize)..=r as isize {
                        let x = (i as isize % w as isize + dx).clamp(0, w as isize - 1) as usize;
                        let y = (i as isize / w as isize + dy).clamp(0, h as isize - 1) as usize;
                        sum += src[y * w + x] as f64;
                    }
                }
                (sum / ((2 * r + 1) * (2 * r + 1)) as f64) as f32
            })
            .collect()
    }
    #[test]
    fn box_matches_independent_2d_reference_including_degenerate_and_large_radius() {
        for (w, h) in [(1, 1), (1, 9), (9, 1), (11, 7)] {
            let src: Vec<_> = (0..w * h)
                .map(|i| ((i * 29 % 71) as f32 - 20.0) / 13.0)
                .collect();
            for r in [1, 2, 15] {
                let actual = box_mean(&src, w, h, r, None).unwrap();
                for (a, b) in actual.iter().zip(brute_box(&src, w, h, r)) {
                    assert!((a - b).abs() < 5e-7, "{w}x{h}, r{r}: {a} != {b}");
                }
            }
        }
    }
    #[test]
    fn schedule_uses_width_subsampling_and_truncated_sizes() {
        assert_eq!(schedule(1600, 1068), (400, 267, 4));
        assert_eq!(schedule(641, 427), (320, 213, 3));
        assert_eq!(schedule(300, 1600), (300, 1600, 16));
        assert_eq!(schedule(6240, 4160), (1560, 1040, 15));
    }
    #[test]
    fn resize_uses_pixel_centers_not_align_corners() {
        let result = resize(&[0.0, 1.0, 2.0, 3.0], 4, 1, 2, 1, None).unwrap();
        assert_eq!(result, [0.5, 2.5]);
        let result = resize(&[0.0, 1.0], 2, 1, 4, 1, None).unwrap();
        assert_eq!(result, [0.0, 0.25, 0.75, 1.0]);
    }
    #[test]
    fn constants_signed_hdr_and_single_pixel_are_preserved() {
        for (w, h) in [(1, 1), (17, 9), (641, 427)] {
            for v in [-2.0, 0.0, 0.18, 1.0, 8.0] {
                let result = base_luma(&vec![v; w * h], w, h, None).unwrap();
                assert!(result.iter().all(|&x| (x - v).abs() < 2e-6));
            }
        }
    }
    #[test]
    fn guided_equations_match_independent_small_reference() {
        let (w, h) = (23, 17);
        let src: Vec<_> = (0..w * h)
            .map(|i| {
                let x = (i % w) as f32;
                let detail = ((i * 13 % 19) as f32 - 9.0) * 0.002;
                if x < 11.0 {
                    0.1 + x * 0.003 + detail
                } else {
                    0.8 + x * 0.003 + detail
                }
            })
            .collect();
        let mean = brute_box(&src, w, h, 1);
        let square: Vec<_> = src.iter().map(|&v| v * v).collect();
        let variance: Vec<_> = brute_box(&square, w, h, 1)
            .iter()
            .zip(&mean)
            .map(|(&q, &m)| (q - m * m).max(0.0))
            .collect();
        let a: Vec<_> = variance.iter().map(|&v| v / (v + 0.01)).collect();
        let b: Vec<_> = mean.iter().zip(&a).map(|(&m, &a)| m - a * m).collect();
        let ma = brute_box(&a, w, h, 1);
        let mb = brute_box(&b, w, h, 1);
        let actual = base_luma(&src, w, h, None).unwrap();
        for i in 0..src.len() {
            assert!((actual[i] - (ma[i] * src[i] + mb[i])).abs() < 2e-6);
        }
        // This is edge aware, not a global blur: most of the step survives.
        assert!(actual[8 * w + 11] - actual[8 * w + 10] > 0.6);
    }
    #[test]
    fn invalid_dimensions_nonfinite_and_cancellation_are_rejected() {
        assert!(base_luma(&[], 0, 1, None).is_err());
        assert!(base_luma(&[0.0], usize::MAX, 2, None).is_err());
        assert!(base_luma(&[f32::NAN], 1, 1, None).is_err());
        assert!(base_luma(&[f32::MAX], 1, 1, None).is_err());
        assert!(base_luma(&[0.0], 1, 1, Some(&AtomicBool::new(true))).is_err());
    }
}
