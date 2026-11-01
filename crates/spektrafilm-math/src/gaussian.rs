use crate::image::ImageBuf;
use crate::precision::{Scalar, ZERO, from_f32, from_f64};
use rayon::prelude::*;

/// Separable Gaussian blur on an ImageBuf.
///
/// Uses FIR convolution for small sigma (<= 3.0) and recursive IIR
/// (Young-van Vliet) for larger sigma. Each channel is processed independently.
/// Processes rows in parallel via rayon.
/// Single-channel 2D Gaussian blur. Same separable FIR (σ ≤ 3) +
/// Young-van Vliet IIR (σ > 3) dispatch as `gaussian_blur`, just on a
/// `Vec<Scalar>` of length `w * h`. Used by callers that need
/// per-channel σ — extracting channels and processing each
/// independently avoids the 3x-redundant work of blurring a 3-channel
/// ImageBuf where all channels are identical.
pub fn gaussian_blur_channel(data: &[Scalar], w: u32, h: u32, sigma: f32) -> Vec<Scalar> {
    let wu = w as usize;
    let hu = h as usize;
    assert_eq!(data.len(), wu * hu);
    if sigma <= 0.0 {
        return data.to_vec();
    }
    let sigma_s = from_f32(sigma);
    let mut chan = data.to_vec();
    let mut tmp = vec![ZERO; wu * hu];
    blur_1d_parallel(&chan, &mut tmp, wu, hu, sigma_s, true);
    blur_1d_parallel(&tmp, &mut chan, wu, hu, sigma_s, false);
    chan
}

/// Sum-of-3-Gaussians approximation of an isotropic 2D exponential PSF
/// `exp(-r / decay) / (2π · decay²)`. Matches Python's
/// `fast_exponential_filter` with `n_gaussians=3` bit-for-bit (same
/// amplitudes and σ/λ ratios; relies on `gaussian_blur_channel`'s
/// FIR/IIR dispatch matching Python's). Used for the scatter-tail
/// component of halation.
pub fn exponential_filter_channel(
    data: &[Scalar],
    w: u32,
    h: u32,
    decay_constant: f32,
) -> Vec<Scalar> {
    // Python `_EXPONENTIAL_GAUSSIAN_FITS[3]` — amplitude, σ / decay.
    const FIT: [(f64, f64); 3] = [(0.1633, 0.5360), (0.6496, 1.5236), (0.1870, 2.7684)];
    let n = data.len();
    let mut result = vec![ZERO; n];
    let decay_f64 = decay_constant as f64;
    for &(amp, ratio) in &FIT {
        let sigma_k = (ratio * decay_f64) as f32;
        let component = gaussian_blur_channel(data, w, h, sigma_k);
        let amp_s = from_f64(amp);
        for (r, &v) in result.iter_mut().zip(component.iter()) {
            *r += amp_s * v;
        }
    }
    result
}

pub fn gaussian_blur(img: &ImageBuf, sigma: f32) -> ImageBuf {
    if sigma <= 0.0 {
        return img.clone();
    }

    let channels = blurred_channels(img, sigma);
    let mut out = ImageBuf::new(img.width, img.height);
    for c in 0..3 {
        out.write_channel(c, &channels[c]);
    }
    out
}

/// Blur an owned image, reusing its interleaved RGB allocation for the result.
pub fn gaussian_blur_owned(mut img: ImageBuf, sigma: f32) -> ImageBuf {
    if sigma <= 0.0 {
        return img;
    }

    let channels = blurred_channels(&img, sigma);
    for c in 0..3 {
        img.write_channel(c, &channels[c]);
    }
    img
}

fn blurred_channels(img: &ImageBuf, sigma: f32) -> Vec<Vec<Scalar>> {
    let w = img.width as usize;
    let h = img.height as usize;
    let sigma_s = from_f32(sigma);

    // Process each channel separately for cache-friendly access
    let mut channels: Vec<Vec<Scalar>> = (0..3).map(|c| img.extract_channel(c)).collect();

    channels.par_iter_mut().for_each(|chan| {
        // Horizontal pass
        let mut tmp = vec![ZERO; w * h];
        blur_1d_parallel(chan, &mut tmp, w, h, sigma_s, true);
        // Vertical pass
        blur_1d_parallel(&tmp, chan, w, h, sigma_s, false);
    });

    channels
}

/// 1D blur pass — either horizontal (along rows) or vertical (along columns).
fn blur_1d_parallel(
    src: &[Scalar],
    dst: &mut [Scalar],
    w: usize,
    h: usize,
    sigma: Scalar,
    horizontal: bool,
) {
    // Python `_dispatch_2d`: `sigma >= SMALL_SIGMA_MAX (=3.0) → IIR`,
    // strictly less → FIR. We use the same `>=` boundary so σ exactly
    // at 3.0 doesn't pick a different branch than Python.
    if sigma < from_f64(3.0) {
        fir_blur_1d(src, dst, w, h, sigma, horizontal);
    } else {
        iir_blur_1d(src, dst, w, h, sigma, horizontal);
    }
}

/// FIR Gaussian blur (truncated kernel). Good for small sigma. Borders
/// reflect with the edge sample repeated, as in Python's `_fir_2d_fused`.
fn fir_blur_1d(
    src: &[Scalar],
    dst: &mut [Scalar],
    w: usize,
    h: usize,
    sigma: Scalar,
    horizontal: bool,
) {
    // Python `_gaussian_kernel_1d`: `radius = int(truncate * sigma + 0.5)`
    // with `truncate=3.0`. `int(x)` on a non-negative float truncates
    // toward zero, which for `truncate·sigma + 0.5` is the standard
    // "round half up" rule. `(x + 0.5) as usize` produces the same
    // value as Python's `int(x + 0.5)` for all positive inputs we
    // care about. Rust used to use `.ceil()` here, which biases the
    // radius up by 1 when `truncate·sigma` lands at a half-integer
    // and changes the FIR kernel size — measurable parity drift.
    let radius = (from_f64(3.0) * sigma + from_f64(0.5)) as usize;
    let kernel = make_gaussian_kernel(sigma, radius);
    let r = radius as isize;

    if horizontal {
        dst.par_chunks_exact_mut(w)
            .enumerate()
            .for_each(|(y, row_dst)| {
                let row_src = &src[y * w..(y + 1) * w];
                for (x, out) in row_dst.iter_mut().enumerate() {
                    let mut sum = ZERO;
                    // Only the outer `radius` pixels on each side read past
                    // the border.
                    if x >= radius && x + radius < w {
                        for (&s, &kv) in row_src[x - radius..=x + radius].iter().zip(&kernel) {
                            sum += s * kv;
                        }
                    } else {
                        for (ki, &kv) in kernel.iter().enumerate() {
                            sum += row_src[reflect_index(x as isize + ki as isize - r, w)] * kv;
                        }
                    }
                    *out = sum;
                }
            });
    } else {
        dst.par_chunks_exact_mut(w)
            .enumerate()
            .for_each(|(y, row_dst)| {
                // Source row of each tap, reflected at the top and bottom.
                let rows: Vec<usize> = (0..kernel.len())
                    .map(|ki| reflect_index(y as isize + ki as isize - r, h) * w)
                    .collect();
                for (x, out) in row_dst.iter_mut().enumerate() {
                    let mut sum = ZERO;
                    for (&row, &kv) in rows.iter().zip(&kernel) {
                        sum += src[row + x] * kv;
                    }
                    *out = sum;
                }
            });
    }
}

/// Python `_reflect` in `fast_gaussian_filter.py`, i.e. scipy.ndimage
/// `mode='reflect'` (numpy `symmetric`): `d c b a | a b c d | d c b a`, the
/// edge sample repeated. The pattern has period `2n`, so a kernel wider than
/// the image keeps folding back and forth across it.
#[inline]
fn reflect_index(i: isize, n: usize) -> usize {
    let n = n as isize;
    let i = i.rem_euclid(2 * n);
    (if i < n { i } else { 2 * n - 1 - i }) as usize
}

/// Young-van Vliet recursive IIR Gaussian blur. Good for large sigma.
///
/// Reference: I.T. Young, L.J. van Vliet, "Recursive implementation of the
/// Gaussian filter", Signal Processing 44 (1995) 139-151.
fn iir_blur_1d(
    src: &[Scalar],
    dst: &mut [Scalar],
    w: usize,
    h: usize,
    sigma: Scalar,
    horizontal: bool,
) {
    // Keep coefficients AND recurrence state in f64 even for f32 pixels.
    // The poles approach 1 at large sigma: cancellation in 1-b1-b2-b3
    // and accumulated f32 roundoff bias even a constant image (notably the
    // wide DIR tail). Different native/WASM libm rounding then amplified
    // into visible differences. Pixel buffers remain in Scalar precision.
    let sigma = sigma as f64;
    let q = if sigma >= 2.5 {
        0.98711 * sigma - 0.96330
    } else {
        3.97156 - 4.14554 * (1.0 - 0.26891 * sigma).sqrt()
    };
    let q2 = q * q;
    let q3 = q2 * q;
    let b0 = 1.57825 + 2.44413 * q + 1.4281 * q2 + 0.422205 * q3;
    let b1 = (2.44413 * q + 2.85619 * q2 + 1.26661 * q3) / b0;
    let b2 = -(1.4281 * q2 + 1.26661 * q3) / b0;
    let b3 = (0.422205 * q3) / b0;
    let a = 1.0 - b1 - b2 - b3;

    if horizontal {
        dst.par_chunks_exact_mut(w)
            .enumerate()
            .for_each(|(y, row_dst)| {
                let row_src = &src[y * w..(y + 1) * w];
                iir_filter_row(row_src, row_dst, a, b1, b2, b3);
            });
    } else {
        // Vertical recursive filtering has a true dependency down each column,
        // so columns are independent work items. Store them column-major first
        // to keep parallel writes contiguous, then transpose back to row-major.
        let mut col_major = vec![ZERO; w * h];
        col_major
            .par_chunks_exact_mut(h)
            .enumerate()
            .for_each(|(x, col_dst)| {
                let col_src: Vec<Scalar> = (0..h).map(|y| src[y * w + x]).collect();
                iir_filter_row(&col_src, col_dst, a, b1, b2, b3);
            });
        dst.par_chunks_exact_mut(w)
            .enumerate()
            .for_each(|(y, row_dst)| {
                for x in 0..w {
                    row_dst[x] = col_major[x * h + y];
                }
            });
    }
}

/// Apply Young-van Vliet IIR filter to a 1D signal (forward + backward pass).
fn iir_filter_row(
    src: &[Scalar],
    dst: &mut [Scalar],
    a: f64,
    b1: f64,
    b2: f64,
    b3: f64,
) {
    let n = src.len();
    if n == 0 {
        return;
    }

    // Python parity (`_iir_horizontal` in `fast_gaussian_filter.py`):
    //
    //   forward:
    //     w1 = w2 = w3 = src[0]              (sample-replication border)
    //     for j in 0..n:
    //         w = a*src[j] + b1*w1 + b2*w2 + b3*w3
    //         dst[j] = w
    //         w3 = w2; w2 = w1; w1 = w
    //   backward:
    //     w1 = w2 = w3 = dst[n-1]            (sample-replication border)
    //     for j in (0..n).rev():
    //         w = a*dst[j] + b1*w1 + b2*w2 + b3*w3
    //         dst[j] = w
    //         w3 = w2; w2 = w1; w1 = w
    //
    // Crucially, the state is initialized to the FIRST sample (not zero),
    // and the iteration covers EVERY index in both directions (not
    // n-3..0). The previous Rust code zeroed the state and skipped the
    // last three positions in the backward pass — which produced a
    // small but visible edge bias vs. Python at every halation σ.
    let x0 = src[0] as f64;
    let mut w1 = x0;
    let mut w2 = x0;
    let mut w3 = x0;
    for j in 0..n {
        let w = a * src[j] as f64 + b1 * w1 + b2 * w2 + b3 * w3;
        dst[j] = from_f64(w);
        w3 = w2;
        w2 = w1;
        w1 = w;
    }
    let xn = dst[n - 1] as f64;
    w1 = xn;
    w2 = xn;
    w3 = xn;
    for j in (0..n).rev() {
        let w = a * dst[j] as f64 + b1 * w1 + b2 * w2 + b3 * w3;
        dst[j] = from_f64(w);
        w3 = w2;
        w2 = w1;
        w1 = w;
    }
}

/// Create normalized Gaussian kernel of given radius.
fn make_gaussian_kernel(sigma: Scalar, radius: usize) -> Vec<Scalar> {
    let size = 2 * radius + 1;
    let mut kernel = Vec::with_capacity(size);
    let s2 = from_f64(2.0) * sigma * sigma;

    for i in 0..size {
        let x = from_f64(i as f64) - from_f64(radius as f64);
        kernel.push((-x * x / s2).exp());
    }

    let sum: Scalar = kernel.iter().sum();
    for v in &mut kernel {
        *v /= sum;
    }
    kernel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaussian_kernel_normalized() {
        let k = make_gaussian_kernel(from_f64(1.0), 3);
        let sum: Scalar = k.iter().sum();
        assert!((sum - from_f64(1.0)).abs() < from_f64(1e-6));
    }

    /// numpy `np.pad(np.arange(4), 9, mode='symmetric')`, which Python's
    /// `_reflect` reproduces (scipy `mode='reflect'`).
    #[test]
    fn reflect_index_matches_numpy_symmetric() {
        let expected = [
            0, 0, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 3, 3,
        ];
        let got: Vec<usize> = (-9..13).map(|i| reflect_index(i, 4)).collect();
        assert_eq!(got, expected);
        assert!((-5..6).all(|i| reflect_index(i, 1) == 0));
    }

    #[test]
    fn wide_iir_preserves_dc_gain() {
        for sigma in [3.0, 10.0, 30.0, 60.0] {
            let input = vec![from_f64(0.42); 97 * 61];
            let out = gaussian_blur_channel(&input, 97, 61, sigma);
            let worst = out.iter().map(|&x| (x as f64 - input[0] as f64).abs()).fold(0.0, f64::max);
            assert!(worst < 1e-7, "sigma {sigma}: DC error {worst}");
        }
    }

    #[test]
    fn test_gaussian_blur_identity_zero_sigma() {
        let img = ImageBuf::from_data(4, 4, vec![from_f64(0.5); 4 * 4 * 3]);
        let out = gaussian_blur(&img, 0.0);
        assert_eq!(img.data, out.data);
    }

    #[test]
    fn test_gaussian_blur_uniform_image() {
        // Blurring a uniform image should return the same image
        let img = ImageBuf::from_data(8, 8, vec![from_f64(0.42); 8 * 8 * 3]);
        let out = gaussian_blur(&img, 2.0);
        for (a, b) in img.data.iter().zip(out.data.iter()) {
            assert!((a - b).abs() < from_f64(1e-4), "expected {a}, got {b}");
        }
    }
}
