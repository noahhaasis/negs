//! Print balance: solve the enlarger's M and Y filter shifts so a mid-grey
//! prints neutral, a port of upstream experimental's
//! `runtime/print_balance.solve_neutral_filter_shifts` (28bf883).
//!
//! The neutral filter database is keyed to each film's own base, so a tuned
//! film base (or any other change to the film, paper or enlarger) leaves the
//! print off-balance. The solver films a uniform mid-grey patch with the
//! current settings, prints and scans it, and finds the M/Y shifts (C fixed)
//! that make the linear output R = G = B. It doesn't depend on the image.
//!
//! Like upstream it prints even when the edit scans the film, reads the
//! enlarger directly (no LUTs), and measures linear output without the output
//! gamut compressor. Two deliberate differences: grain and glare are off,
//! because they are random and would make the residual noisy (glare only
//! adds viewing-white light), and the crop, working size and post
//! adjustments don't apply to the 2 × 2 patch. Upstream's least-squares
//! Levenberg–Marquardt is replaced by a damped Newton iteration on the same
//! two residuals; both converge to the same root.

use std::path::Path;

use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::from_f64;

use crate::params::RuntimeParams;
use crate::pipeline::Pipeline;
use crate::profile::Profile;

/// Upstream's mid-grey patch value.
pub const MIDGRAY: f64 = 0.184;

/// The M and Y filter shifts that print `midgray` neutral, starting from the
/// shifts in `params`. `film` and `print` are the profiles as loaded.
pub fn solve_neutral_filter_shifts(
    film: Profile,
    print: Profile,
    params: &RuntimeParams,
    data_dir: &Path,
    midgray: f64,
) -> Result<(f64, f64), String> {
    let mut p = params.clone();
    p.io.scan_film = false;
    p.settings.use_enlarger_lut = false;
    p.settings.use_scanner_lut = false;
    p.io.output_cctf_encoding = false;
    p.io.output_gamut_compress.algorithm = "off".into();
    p.io.output_gamut_compress.lightness_compression = None;
    p.film_render.grain.active = false;
    p.film_render.glare.active = false;
    p.print_render.glare.active = false;
    p.io.crop = false;
    p.io.photo_long_side_px = None;
    p.io.upscale_factor = 1.0;
    p.post = Default::default();
    let x0 = [
        p.enlarger.m_filter_shift as f64,
        p.enlarger.y_filter_shift as f64,
    ];
    let mut pipeline = Pipeline::new_with_spectral(film, print, p, data_dir)?;
    let gray = ImageBuf::from_data(2, 2, vec![from_f64(midgray); 12]);
    static CPU: spektrafilm_gpu::cpu_backend::CpuBackend = spektrafilm_gpu::cpu_backend::CpuBackend;

    let mut residual = |x: [f64; 2]| -> [f64; 2] {
        pipeline.set_print_filter_shifts(x[0], x[1]);
        let rgb = pipeline.process(gray.clone(), &CPU);
        let mut mean = [0.0f64; 3];
        for px in rgb.data.chunks_exact(3) {
            for c in 0..3 {
                mean[c] += px[c] as f64;
            }
        }
        let n = (rgb.data.len() / 3) as f64;
        let [r, g, b] = mean.map(|v| v / n);
        let gd = g.max(1e-6);
        [(r - g) / gd, (b - g) / gd]
    };
    let [m, y] = solve_2x2(&mut residual, x0);
    Ok((m, y))
}

/// Damped Newton (Levenberg–Marquardt) on two residuals in two unknowns,
/// with a central-difference Jacobian. The step is wide enough for an f32
/// build's rounding and narrow enough that the Jacobian's own error only
/// slows convergence; the root is set by the residuals alone.
fn solve_2x2(f: &mut dyn FnMut([f64; 2]) -> [f64; 2], x0: [f64; 2]) -> [f64; 2] {
    const H: f64 = 0.05;
    const MAX_ITER: usize = 50;
    let norm2 = |r: [f64; 2]| r[0] * r[0] + r[1] * r[1];
    let mut x = x0;
    let mut r = f(x);
    let mut lambda = 1e-3;
    for _ in 0..MAX_ITER {
        if norm2(r) < 1e-28 {
            break;
        }
        let mut jac = [[0.0f64; 2]; 2];
        for k in 0..2 {
            let (mut hi, mut lo) = (x, x);
            hi[k] += H;
            lo[k] -= H;
            let (rh, rl) = (f(hi), f(lo));
            for i in 0..2 {
                jac[i][k] = (rh[i] - rl[i]) / (2.0 * H);
            }
        }
        // Normal equations, (JᵀJ + λ diag JᵀJ) δ = -Jᵀr.
        let mut a = [[0.0f64; 2]; 2];
        let mut g = [0.0f64; 2];
        for i in 0..2 {
            for k in 0..2 {
                a[i][k] = jac[0][i] * jac[0][k] + jac[1][i] * jac[1][k];
            }
            g[i] = jac[0][i] * r[0] + jac[1][i] * r[1];
        }
        let mut improved = false;
        while lambda < 1e12 {
            let m = [
                [a[0][0] * (1.0 + lambda), a[0][1]],
                [a[1][0], a[1][1] * (1.0 + lambda)],
            ];
            let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
            if det == 0.0 || !det.is_finite() {
                lambda *= 10.0;
                continue;
            }
            let step = [
                -(m[1][1] * g[0] - m[0][1] * g[1]) / det,
                -(m[0][0] * g[1] - m[1][0] * g[0]) / det,
            ];
            let trial = [x[0] + step[0], x[1] + step[1]];
            let rt = f(trial);
            if norm2(rt) < norm2(r) {
                let tiny = step[0].abs().max(step[1].abs()) < 1e-10;
                x = trial;
                r = rt;
                lambda = (lambda / 10.0).max(1e-12);
                improved = !tiny;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::solve_2x2;

    #[test]
    fn solves_a_coupled_nonlinear_system() {
        // Root at (3, -2); smooth and monotone like the print residuals.
        let mut f = |x: [f64; 2]| {
            [
                (0.02 * (x[0] - 3.0)).tanh() + 0.005 * (x[1] + 2.0),
                0.03 * (x[1] + 2.0) + 0.001 * (x[0] - 3.0).powi(3) * 1e-2,
            ]
        };
        let x = solve_2x2(&mut f, [0.0, 0.0]);
        assert!(
            (x[0] - 3.0).abs() < 1e-9 && (x[1] + 2.0).abs() < 1e-9,
            "{x:?}"
        );
    }
}
