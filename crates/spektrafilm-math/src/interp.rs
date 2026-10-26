use crate::image::ImageBuf;
use crate::precision::{Scalar, from_f32, from_f64};
/// Fast 1D linear interpolation with binary search.
///
/// Port of Python `fast_interp.py`. Supports both common x-axis (same for all channels)
/// and per-channel x-axes.
use rayon::prelude::*;

/// Interpolate a single value from sorted arrays.
#[inline]
pub fn interp_1d(x: &[f32], y: &[f32], xq: f32) -> f32 {
    debug_assert_eq!(x.len(), y.len());
    debug_assert!(!x.is_empty());

    let n = x.len();
    if n == 1 {
        return y[0];
    }
    if xq <= x[0] {
        return y[0];
    }
    if xq >= x[n - 1] {
        return y[n - 1];
    }

    let i = match x.binary_search_by(|v| v.partial_cmp(&xq).unwrap()) {
        Ok(i) => return y[i],
        Err(i) => i - 1,
    };

    let t = (xq - x[i]) / (x[i + 1] - x[i]);
    y[i] + t * (y[i + 1] - y[i])
}

/// `np.interp(x, xp, fp)` in f64, step for step as numpy's `arr_interp`
/// (`numpy/_core/src/multiarray/compiled_base.c`): the guessed binary search,
/// the precomputed slopes, the exact-hit and NaN fallbacks. Matching the
/// search also matches numpy on an `xp` that isn't monotonic, which a pushed
/// DIR `amount` can produce.
pub fn np_interp(x: &[f64], xp: &[f64], fp: &[f64]) -> Vec<f64> {
    assert_eq!(xp.len(), fp.len());
    assert!(!xp.is_empty());
    let n = xp.len();
    if n == 1 {
        return x
            .iter()
            .map(|v| if v.is_nan() { *v } else { fp[0] })
            .collect();
    }
    let (left, right) = (fp[0], fp[n - 1]);
    let slopes: Option<Vec<f64>> = (n <= x.len()).then(|| {
        (0..n - 1)
            .map(|i| (fp[i + 1] - fp[i]) / (xp[i + 1] - xp[i]))
            .collect()
    });
    let mut j = 0isize;
    x.iter()
        .map(|&xv| {
            if xv.is_nan() {
                return xv;
            }
            j = np_binary_search_with_guess(xv, xp, j);
            if j == -1 {
                return left;
            }
            let ju = j as usize;
            if ju == n {
                return right;
            }
            if ju == n - 1 || xp[ju] == xv {
                return fp[ju];
            }
            let slope = match &slopes {
                Some(s) => s[ju],
                None => (fp[ju + 1] - fp[ju]) / (xp[ju + 1] - xp[ju]),
            };
            let mut v = slope * (xv - xp[ju]) + fp[ju];
            if v.is_nan() {
                v = slope * (xv - xp[ju + 1]) + fp[ju + 1];
                if v.is_nan() && fp[ju] == fp[ju + 1] {
                    v = fp[ju];
                }
            }
            v
        })
        .collect()
}

/// numpy's `binary_search_with_guess`: the index `i` with
/// `arr[i] <= key < arr[i + 1]`, -1 below the range and `len` above it.
fn np_binary_search_with_guess(key: f64, arr: &[f64], guess: isize) -> isize {
    const LIKELY_IN_CACHE_SIZE: isize = 8;
    let len = arr.len() as isize;
    let at = |i: isize| arr[i as usize];
    if key > at(len - 1) {
        return len;
    } else if key < at(0) {
        return -1;
    }
    if len <= 4 {
        let mut i = 1;
        while i < len && key >= at(i) {
            i += 1;
        }
        return i - 1;
    }
    let guess = guess.min(len - 3).max(1);
    let (mut imin, mut imax) = (0, len);
    if key < at(guess) {
        if key < at(guess - 1) {
            imax = guess - 1;
            if guess > LIKELY_IN_CACHE_SIZE && key >= at(guess - LIKELY_IN_CACHE_SIZE) {
                imin = guess - LIKELY_IN_CACHE_SIZE;
            }
        } else {
            return guess - 1;
        }
    } else if key < at(guess + 1) {
        return guess;
    } else if key < at(guess + 2) {
        return guess + 1;
    } else {
        imin = guess + 2;
        if guess < len - LIKELY_IN_CACHE_SIZE - 1 && key < at(guess + LIKELY_IN_CACHE_SIZE) {
            imax = guess + LIKELY_IN_CACHE_SIZE;
        }
    }
    while imin < imax {
        let imid = imin + ((imax - imin) >> 1);
        if key >= at(imid) {
            imin = imid + 1;
        } else {
            imax = imid;
        }
    }
    imin - 1
}

/// Interpolate a uniformly-spaced table.
#[inline]
pub fn interp_uniform(x_min: f32, x_max: f32, y: &[f32], xq: f32) -> f32 {
    let n = y.len();
    if n == 1 {
        return y[0];
    }

    let step = (x_max - x_min) / (n as f32 - 1.0);
    let t = (xq - x_min) / step;

    if t <= 0.0 {
        return y[0];
    }
    if t >= (n - 1) as f32 {
        return y[n - 1];
    }

    let i = t as usize;
    let frac = t - i as f32;
    y[i] + frac * (y[i + 1] - y[i])
}

/// Batch interpolation: uniform table for 3 channels.
#[inline]
pub fn interp_uniform_3ch(x_min: f32, x_max: f32, table: &[[f32; 3]], xq: f32) -> [f32; 3] {
    let n = table.len();
    if n == 1 {
        return table[0];
    }

    let step = (x_max - x_min) / (n as f32 - 1.0);
    let t = (xq - x_min) / step;

    if t <= 0.0 {
        return table[0];
    }
    if t >= (n - 1) as f32 {
        return table[n - 1];
    }

    let i = t as usize;
    let frac = t - i as f32;
    [
        table[i][0] + frac * (table[i + 1][0] - table[i][0]),
        table[i][1] + frac * (table[i + 1][1] - table[i][1]),
        table[i][2] + frac * (table[i + 1][2] - table[i][2]),
    ]
}

/// Fast image interpolation: for each pixel (HxWx3), look up each channel
/// independently in a common x-axis → y-values table.
///
/// This is the direct Rust port of the Python `fast_interp` Numba kernel.
/// `x_axis`: sorted 1D array of length K.
/// `y_vals`: [K][3] array of y-values per channel.
pub fn fast_interp_image(img: &ImageBuf, x_axis: &[f32], y_vals: &[[f32; 3]]) -> ImageBuf {
    let k = x_axis.len();
    assert_eq!(y_vals.len(), k);

    // Promote constant data to Scalar once (no-op in f32 mode, widening in f64 mode).
    let xa: Vec<Scalar> = x_axis.iter().map(|&v| from_f32(v)).collect();
    let ya: Vec<[Scalar; 3]> = y_vals
        .iter()
        .map(|r| [from_f32(r[0]), from_f32(r[1]), from_f32(r[2])])
        .collect();
    let inv_dx: Vec<Scalar> = (0..k - 1)
        .map(|i| {
            let dx = xa[i + 1] - xa[i];
            if dx != 0.0 { 1.0 / dx } else { 0.0 }
        })
        .collect();

    let mut out = img.clone();
    out.par_pixels_mut().for_each(|px| {
        for c in 0..3 {
            let x = px[c];
            if x <= xa[0] {
                px[c] = ya[0][c];
            } else if x >= xa[k - 1] {
                px[c] = ya[k - 1][c];
            } else {
                let idx = xa.partition_point(|&v| v < x);
                let low = if idx > 0 { idx - 1 } else { 0 };
                let t = (x - xa[low]) * inv_dx[low];
                px[c] = ya[low][c] + t * (ya[low + 1][c] - ya[low][c]);
            }
        }
    });
    out
}

/// f64 variant of fast_interp_image — accepts f64 axes/curves directly
/// to preserve precision (no f32 truncation at the JSON boundary).
pub fn fast_interp_image_f64(img: &ImageBuf, x_axis: &[f64], y_vals: &[[f64; 3]]) -> ImageBuf {
    let k = x_axis.len();
    assert_eq!(y_vals.len(), k);

    // Promote to Scalar once (no-op in f64 mode, narrowing in f32 mode).
    let xa: Vec<Scalar> = x_axis.iter().map(|&v| from_f64(v)).collect();
    let ya: Vec<[Scalar; 3]> = y_vals
        .iter()
        .map(|r| [from_f64(r[0]), from_f64(r[1]), from_f64(r[2])])
        .collect();
    let inv_dx: Vec<Scalar> = (0..k - 1)
        .map(|i| {
            let dx = xa[i + 1] - xa[i];
            if dx != 0.0 { 1.0 / dx } else { 0.0 }
        })
        .collect();

    let mut out = img.clone();
    out.par_pixels_mut().for_each(|px| {
        for c in 0..3 {
            let x = px[c];
            if x <= xa[0] {
                px[c] = ya[0][c];
            } else if x >= xa[k - 1] {
                px[c] = ya[k - 1][c];
            } else {
                // Match numpy's searchsorted(side='right') - 1: include equal values on the left side.
                let idx = xa.partition_point(|&v| v <= x);
                let low = if idx > 0 { idx - 1 } else { 0 };
                let t = (x - xa[low]) * inv_dx[low];
                px[c] = ya[low][c] + t * (ya[low + 1][c] - ya[low][c]);
            }
        }
    });
    out
}

/// f64 variant with per-channel x-axes.
pub fn fast_interp_image_perchannel_f64(
    img: &ImageBuf,
    x_axes: &[[f64; 3]],
    y_vals: &[[f64; 3]],
) -> ImageBuf {
    let k = x_axes.len();
    assert_eq!(y_vals.len(), k);

    let xax: Vec<[Scalar; 3]> = x_axes
        .iter()
        .map(|r| [from_f64(r[0]), from_f64(r[1]), from_f64(r[2])])
        .collect();
    let ya: Vec<[Scalar; 3]> = y_vals
        .iter()
        .map(|r| [from_f64(r[0]), from_f64(r[1]), from_f64(r[2])])
        .collect();
    let per_chan_axes: [Vec<Scalar>; 3] = [
        xax.iter().map(|r| r[0]).collect(),
        xax.iter().map(|r| r[1]).collect(),
        xax.iter().map(|r| r[2]).collect(),
    ];
    let inv_dx: Vec<[Scalar; 3]> = (0..k - 1)
        .map(|i| {
            let mut id = [Scalar::default(); 3];
            for c in 0..3 {
                let dx = xax[i + 1][c] - xax[i][c];
                id[c] = if dx != 0.0 { 1.0 / dx } else { 0.0 };
            }
            id
        })
        .collect();

    let mut out = img.clone();
    out.par_pixels_mut().for_each(|px| {
        for c in 0..3 {
            let x = px[c];
            let x0 = xax[0][c];
            let xn = xax[k - 1][c];
            if x <= x0 {
                px[c] = ya[0][c];
            } else if x >= xn {
                px[c] = ya[k - 1][c];
            } else {
                let xa = &per_chan_axes[c];
                let idx = xa.partition_point(|&v| v <= x);
                let low = if idx > 0 { idx - 1 } else { 0 };
                let t = (x - xax[low][c]) * inv_dx[low][c];
                px[c] = ya[low][c] + t * (ya[low + 1][c] - ya[low][c]);
            }
        }
    });
    out
}

/// Fast image interpolation with per-channel x-axes.
///
/// `x_axes`: [K][3] — different x-axis per channel (e.g. log_exposure / gamma_factor).
/// `y_vals`: [K][3] — y-values per channel.
pub fn fast_interp_image_perchannel(
    img: &ImageBuf,
    x_axes: &[[f32; 3]],
    y_vals: &[[f32; 3]],
) -> ImageBuf {
    let k = x_axes.len();
    assert_eq!(y_vals.len(), k);

    // Promote constant data to Scalar once. Pre-extract per-channel axes for the binary
    // search inside the parallel loop (avoids per-pixel allocation).
    let xax: Vec<[Scalar; 3]> = x_axes
        .iter()
        .map(|r| [from_f32(r[0]), from_f32(r[1]), from_f32(r[2])])
        .collect();
    let ya: Vec<[Scalar; 3]> = y_vals
        .iter()
        .map(|r| [from_f32(r[0]), from_f32(r[1]), from_f32(r[2])])
        .collect();
    let per_chan_axes: [Vec<Scalar>; 3] = [
        xax.iter().map(|r| r[0]).collect(),
        xax.iter().map(|r| r[1]).collect(),
        xax.iter().map(|r| r[2]).collect(),
    ];
    let inv_dx: Vec<[Scalar; 3]> = (0..k - 1)
        .map(|i| {
            let mut id = [Scalar::default(); 3];
            for c in 0..3 {
                let dx = xax[i + 1][c] - xax[i][c];
                id[c] = if dx != 0.0 { 1.0 / dx } else { 0.0 };
            }
            id
        })
        .collect();

    let mut out = img.clone();
    out.par_pixels_mut().for_each(|px| {
        for c in 0..3 {
            let x = px[c];
            let x0 = xax[0][c];
            let xn = xax[k - 1][c];
            if x <= x0 {
                px[c] = ya[0][c];
            } else if x >= xn {
                px[c] = ya[k - 1][c];
            } else {
                let xa = &per_chan_axes[c];
                let idx = xa.partition_point(|&v| v < x);
                let low = if idx > 0 { idx - 1 } else { 0 };
                let t = (x - xax[low][c]) * inv_dx[low][c];
                px[c] = ya[low][c] + t * (ya[low + 1][c] - ya[low][c]);
            }
        }
    });
    out
}

/// Akima interpolation of `(x, y)` at `xq`, as `scipy.interpolate.
/// Akima1DInterpolator(x, y)(xq)` (method `"akima"`, no extrapolation: NaN
/// outside `[x[0], x[n-1]]`). `x` must be strictly increasing, with at least
/// two points.
pub fn akima_1d(x: &[f64], y: &[f64], xq: &[f64]) -> Vec<f64> {
    let n = x.len();
    assert!(n >= 2 && y.len() == n, "akima_1d needs matching x and y with two points or more");
    let dx: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(dx.iter().all(|&d| d > 0.0), "akima_1d needs strictly increasing x");
    // Breakpoint slopes t, as scipy's Akima1DInterpolator.__init__.
    let t: Vec<f64> = if n == 2 {
        let m = (y[1] - y[0]) / dx[0];
        vec![m, m]
    } else {
        let mut m = vec![0.0; n + 3];
        for i in 0..n - 1 {
            m[i + 2] = (y[i + 1] - y[i]) / dx[i];
        }
        m[1] = 2.0 * m[2] - m[3];
        m[0] = 2.0 * m[1] - m[2];
        m[n + 1] = 2.0 * m[n] - m[n - 1];
        m[n + 2] = 2.0 * m[n + 1] - m[n];
        let dm: Vec<f64> = m.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let f1 = &dm[2..];
        let f2 = &dm[..dm.len() - 2];
        let f12: Vec<f64> = f1.iter().zip(f2).map(|(a, b)| a + b).collect();
        let max = f12.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (0..n)
            .map(|i| {
                if f12[i] > 1e-9 * max {
                    m[i + 1] + (f2[i] / f12[i]) * (m[i + 2] - m[i + 1])
                } else {
                    0.5 * (m[i + 3] + m[i])
                }
            })
            .collect()
    };
    // CubicHermiteSpline coefficients, evaluated like scipy's PPoly.
    xq.iter()
        .map(|&v| {
            if !(v >= x[0] && v <= x[n - 1]) {
                return f64::NAN;
            }
            let i = x.partition_point(|&xi| xi <= v).clamp(1, n - 1) - 1;
            let slope = (y[i + 1] - y[i]) / dx[i];
            let tt = (t[i] + t[i + 1] - 2.0 * slope) / dx[i];
            let c = [tt / dx[i], (slope - t[i]) / dx[i] - tt, t[i], y[i]];
            let s = v - x[i];
            let mut z = 1.0;
            let mut res = 0.0;
            for k in (0..4).rev() {
                res += c[k] * z;
                z *= s;
            }
            res
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::akima_1d;

    #[test]
    fn akima_reproduces_lines_and_nodes() {
        let x = [0.0, 1.0, 2.5, 4.0, 4.5];
        let y = [1.0, 3.0, 6.0, 9.0, 10.0];
        let got = akima_1d(&x, &y, &[0.0, 0.5, 2.5, 4.25, 4.5, 4.6, -0.1]);
        for (g, e) in got.iter().zip([1.0, 2.0, 6.0, 9.5, 10.0]) {
            assert!((g - e).abs() < 1e-12, "{g} vs {e}");
        }
        assert!(got[5].is_nan() && got[6].is_nan());
    }

    use super::*;

    #[test]
    fn test_interp_1d_exact() {
        let x = vec![0.0, 1.0, 2.0, 3.0];
        let y = vec![0.0, 10.0, 20.0, 30.0];
        assert_eq!(interp_1d(&x, &y, 1.0), 10.0);
        assert_eq!(interp_1d(&x, &y, 2.0), 20.0);
    }

    #[test]
    fn test_interp_1d_midpoint() {
        let x = vec![0.0, 1.0, 2.0];
        let y = vec![0.0, 10.0, 20.0];
        let v = interp_1d(&x, &y, 0.5);
        assert!((v - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_interp_1d_clamp() {
        let x = vec![0.0, 1.0];
        let y = vec![5.0, 15.0];
        assert_eq!(interp_1d(&x, &y, -1.0), 5.0);
        assert_eq!(interp_1d(&x, &y, 2.0), 15.0);
    }

    #[test]
    fn test_interp_uniform() {
        let y = vec![0.0, 10.0, 20.0, 30.0];
        let v = interp_uniform(0.0, 3.0, &y, 1.5);
        assert!((v - 15.0).abs() < 1e-6);
    }

    #[test]
    fn test_interp_uniform_3ch() {
        let table = vec![[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]];
        let v = interp_uniform_3ch(0.0, 1.0, &table, 0.5);
        assert!((v[0] - 5.0).abs() < 1e-6);
        assert!((v[1] - 10.0).abs() < 1e-6);
        assert!((v[2] - 15.0).abs() < 1e-6);
    }

    #[test]
    fn test_fast_interp_image() {
        use crate::precision::from_f64;
        let x_axis = vec![0.0_f32, 0.5, 1.0];
        let y_vals = vec![[0.0_f32, 0.0, 0.0], [5.0, 10.0, 15.0], [10.0, 20.0, 30.0]];
        let img = ImageBuf::from_data(
            2,
            1,
            vec![
                from_f64(0.25),
                from_f64(0.25),
                from_f64(0.25),
                from_f64(0.75),
                from_f64(0.75),
                from_f64(0.75),
            ],
        );
        let out = fast_interp_image(&img, &x_axis, &y_vals);
        let p0 = out.get(0, 0);
        assert!((p0[0] - from_f64(2.5)).abs() < from_f64(1e-4));
        assert!((p0[1] - from_f64(5.0)).abs() < from_f64(1e-4));
        assert!((p0[2] - from_f64(7.5)).abs() < from_f64(1e-4));
    }
}
