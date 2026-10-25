//! s023 coupled-gamma print-curve morph.
//!
//! Port of upstream `spektrafilm/utils/morph_curves.py`. Rebuilds a print
//! profile's density curves from its parametric sum-of-CDFs model
//! (`DensityCurvesModel`) while morphing the per-layer gamma. Each channel's
//! density is `sum_i A_i * Phi((x - mu_i) / sigma_i)`, where `Phi` is the
//! standard-normal CDF (sign-flipped for positive stocks) optionally blended
//! toward a Gumbel-max CDF for the developer-exhaustion control.
//!
//! Layers are ordered by grain speed (ascending center): fast = lowest center,
//! slow = highest. A coupled gamma scaling `sigma' = sigma/g, mu' = mu/g`
//! (amplitudes fixed) sets the per-band slope; effective gamma per channel is
//! `gamma_factor * gamma_factor_{fast,slow} * gamma_factor_{r,g,b}`. Developer
//! exhaustion blends every sub-layer toward the matched Gumbel CDF and then
//! shifts all sub-layers of a channel by a common offset (solved so D(0) is
//! preserved), so it does not move midgray.
//!
//! Properties preserved by construction: per-layer amplitudes, D(0), and
//! D_max = sum A_i; identity at all defaults reproduces the fitted model.

use crate::params::CurvesMorphParams;
use crate::profile::DensityCurvesModel;

/// `NormCdfsFitConfig.sigma_floor` in the profile-creator.
const SIGMA_FLOOR: f64 = 0.05;

/// Standard-normal CDF — cephes `ndtr` (the implementation behind
/// `scipy.stats.norm.cdf`), piecewise on `erf`/`erfc` for precision.
pub(crate) fn norm_cdf(a: f64) -> f64 {
    const SQRTH: f64 = std::f64::consts::FRAC_1_SQRT_2; // sqrt(1/2)
    let x = a * SQRTH;
    let z = x.abs();
    if z < SQRTH {
        0.5 + 0.5 * libm::erf(x)
    } else {
        let y = 0.5 * libm::erfc(z);
        if x > 0.0 { 1.0 - y } else { y }
    }
}

#[inline]
fn signed_z(z: f64, positive: bool) -> f64 {
    if positive { -z } else { z }
}

fn gumbel_matched_cdf(z: f64) -> f64 {
    // location = -ln(ln 2), width = 0.5 * ln(2) * sqrt(2*pi)
    let location = -(2.0_f64.ln().ln());
    let width = 0.5 * 2.0_f64.ln() * (2.0 * std::f64::consts::PI).sqrt();
    (-(-(z / width + location)).exp()).exp()
}

fn layer_cdf(z: f64, positive: bool, gumbel_mix: f64) -> f64 {
    let sz = signed_z(z, positive);
    let cdf = norm_cdf(sz);
    if gumbel_mix > 0.0 {
        (1.0 - gumbel_mix) * cdf + gumbel_mix * gumbel_matched_cdf(sz)
    } else {
        cdf
    }
}

/// Evaluate one channel's density at every `log_exposure` sample.
fn evaluate_channel_density(
    log_exposure: &[f64],
    centers: &[f64],
    amplitudes: &[f64],
    sigmas: &[f64],
    positive: bool,
    gumbel_mix_per_layer: &[f64],
) -> Vec<f64> {
    let mut out = vec![0.0f64; log_exposure.len()];
    for i in 0..centers.len() {
        let mix = gumbel_mix_per_layer[i];
        let (mu, a, s) = (centers[i], amplitudes[i], sigmas[i]);
        for (o, &x) in out.iter_mut().zip(log_exposure) {
            *o += a * layer_cdf((x - mu) / s, positive, mix);
        }
    }
    out
}

/// `(i_fast, i_mid, i_slow)` by ascending center (grain-speed order). Real
/// profiles have three distinct centers per channel, so tie-ordering (where
/// this `total_cmp` sort and `np.argsort`'s stable sort could differ) does not
/// arise.
fn speed_layer_indices(centers: &[f64]) -> (usize, usize, usize) {
    let n = centers.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| centers[a].total_cmp(&centers[b]));
    (order[0], order[n / 2], order[n - 1])
}

fn channel_gamma_factor(p: &CurvesMorphParams, channel: usize) -> f64 {
    [
        p.gamma_factor_red,
        p.gamma_factor_green,
        p.gamma_factor_blue,
    ][channel]
}

/// Solve for the common center offset that keeps `D(0)` unchanged once the
/// Gumbel-max blend is applied (developer exhaustion does not move midgray).
/// Mirrors `_developer_exhaustion_center_offset`: bracket-expand around
/// `±0.25` then `brentq` with `xtol = 1e-10`.
fn developer_exhaustion_center_offset(
    centers: &[f64],
    amplitudes: &[f64],
    sigmas: &[f64],
    positive: bool,
    gumbel_mix_per_layer: &[f64],
) -> f64 {
    if gumbel_mix_per_layer.iter().all(|&m| m.abs() <= 1e-8) {
        return 0.0;
    }

    let zeros = vec![0.0f64; gumbel_mix_per_layer.len()];
    let zero_exposure = [0.0f64];
    let target_d0 = evaluate_channel_density(
        &zero_exposure,
        centers,
        amplitudes,
        sigmas,
        positive,
        &zeros,
    )[0];

    let residual = |center_offset: f64| -> f64 {
        let shifted: Vec<f64> = centers.iter().map(|&c| c + center_offset).collect();
        let d0 = evaluate_channel_density(
            &zero_exposure,
            &shifted,
            amplitudes,
            sigmas,
            positive,
            gumbel_mix_per_layer,
        )[0];
        d0 - target_d0
    };

    if residual(0.0).abs() <= 1e-12 {
        return 0.0;
    }

    let mut lo = -0.25;
    let mut hi = 0.25;
    let mut r_lo = residual(lo);
    let mut r_hi = residual(hi);
    for _ in 0..12 {
        if r_lo == 0.0 {
            return lo;
        }
        if r_hi == 0.0 {
            return hi;
        }
        if r_lo * r_hi < 0.0 {
            return brentq(&residual, lo, hi, 1e-10);
        }
        lo *= 2.0;
        hi *= 2.0;
        r_lo = residual(lo);
        r_hi = residual(hi);
    }
    0.0
}

/// Brent's method root finder — faithful port of scipy's `brentq`
/// (`rtol = 4*eps`, `maxiter = 100`).
fn brentq(f: &dyn Fn(f64) -> f64, xa: f64, xb: f64, xtol: f64) -> f64 {
    const RTOL: f64 = 4.0 * f64::EPSILON;
    const MAXITER: usize = 100;

    let (mut xpre, mut xcur) = (xa, xb);
    let mut xblk = 0.0;
    let mut fpre = f(xpre);
    let mut fcur = f(xcur);
    let mut fblk = 0.0;
    let mut spre = 0.0;
    let mut scur = 0.0;

    if fpre * fcur > 0.0 {
        return 0.0;
    }
    if fpre == 0.0 {
        return xpre;
    }
    if fcur == 0.0 {
        return xcur;
    }

    for _ in 0..MAXITER {
        if fpre * fcur < 0.0 {
            xblk = xpre;
            fblk = fpre;
            spre = xcur - xpre;
            scur = xcur - xpre;
        }
        if fblk.abs() < fcur.abs() {
            xpre = xcur;
            xcur = xblk;
            xblk = xpre;
            fpre = fcur;
            fcur = fblk;
            fblk = fpre;
        }

        let delta = (xtol + RTOL * xcur.abs()) / 2.0;
        let sbis = (xblk - xcur) / 2.0;
        if fcur == 0.0 || sbis.abs() < delta {
            return xcur;
        }

        if spre.abs() > delta && fcur.abs() < fpre.abs() {
            let stry = if xpre == xblk {
                // interpolate
                -fcur * (xcur - xpre) / (fcur - fpre)
            } else {
                // extrapolate
                let dpre = (fpre - fcur) / (xpre - xcur);
                let dblk = (fblk - fcur) / (xblk - xcur);
                -fcur * (fblk * dblk - fpre * dpre) / (dblk * dpre * (fblk - fpre))
            };
            if 2.0 * stry.abs() < spre.abs().min(3.0 * sbis.abs() - delta) {
                // good short step
                spre = scur;
                scur = stry;
            } else {
                // bisect
                spre = sbis;
                scur = sbis;
            }
        } else {
            // bisect
            spre = sbis;
            scur = sbis;
        }

        xpre = xcur;
        fpre = fcur;
        if scur.abs() > delta {
            xcur += scur;
        } else {
            xcur += if sbis > 0.0 { delta } else { -delta };
        }
        fcur = f(xcur);
    }
    xcur
}

/// Morphed `(centers, amplitudes, sigmas, gumbel_mix_per_layer)` for one channel.
fn morph_channel_params(
    model: &DensityCurvesModel,
    p: &CurvesMorphParams,
    channel: usize,
    positive: bool,
) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut centers = model.centers[channel].clone();
    let amplitudes = model.amplitudes[channel].clone();
    let mut sigmas = model.sigmas[channel].clone();

    let (i_fast, i_mid, i_slow) = speed_layer_indices(&centers);

    let base = p.gamma_factor * channel_gamma_factor(p, channel);
    let g_fast = base * p.gamma_factor_fast;
    // Upstream couples both mid and slow sub-layers to gamma_factor_slow.
    let g_mid = base * p.gamma_factor_slow;
    let g_slow = base * p.gamma_factor_slow;

    sigmas[i_fast] = (sigmas[i_fast] / g_fast).max(SIGMA_FLOOR);
    centers[i_fast] /= g_fast;
    sigmas[i_mid] = (sigmas[i_mid] / g_mid).max(SIGMA_FLOOR);
    centers[i_mid] /= g_mid;
    sigmas[i_slow] = (sigmas[i_slow] / g_slow).max(SIGMA_FLOOR);
    centers[i_slow] /= g_slow;

    let gumbel_mix_per_layer = vec![p.developer_exhaustion; centers.len()];
    let offset = developer_exhaustion_center_offset(
        &centers,
        &amplitudes,
        &sigmas,
        positive,
        &gumbel_mix_per_layer,
    );
    for c in &mut centers {
        *c += offset;
    }

    (centers, amplitudes, sigmas, gumbel_mix_per_layer)
}

/// Evaluate the print density curves from the fitted model, returning
/// `[n_samples][3]` curves ready for `density_curve_interp`. Mirrors upstream
/// `apply_print_curves_morph`: when `p.active` is false the fitted model is
/// evaluated as-is (`_evaluate_fitted_density` — the morph parameters are
/// never read); when active, the coupled-gamma morph is applied (which at
/// identity params also reproduces the fitted model).
///
/// Returns `Err` on an unsupported model type, a model that is not three RGB
/// channels with consistent per-channel array shapes, and — when active — a
/// non-positive gamma factor or out-of-range developer exhaustion (mirrors
/// the upstream validation, and keeps the per-channel indexing panic-free).
pub fn morph_density_curves(
    log_exposure: &[f64],
    model: &DensityCurvesModel,
    p: &CurvesMorphParams,
    positive: bool,
) -> Result<Vec<[f64; 3]>, String> {
    morph_density_curves_with_layers(log_exposure, model, p, positive).map(|(total, _)| total)
}

/// [`morph_density_curves`] that also returns the per-sub-layer curves,
/// `(total [n_le][3], layers [n_le][n_layers][3])`. Port of upstream
/// `apply_print_curves_morph_with_layers`, which bakes the film chemistry
/// into the film profile so the grain sub-layers stay consistent with the
/// morph. The total is the in-order sum of the layers, which is also how
/// upstream's `apply_print_curves_morph` accumulates it, so both agree
/// bit for bit.
#[allow(clippy::type_complexity)]
pub fn morph_density_curves_with_layers(
    log_exposure: &[f64],
    model: &DensityCurvesModel,
    p: &CurvesMorphParams,
    positive: bool,
) -> Result<(Vec<[f64; 3]>, Vec<Vec<[f64; 3]>>), String> {
    // The port implements only the Gaussian-CDF model: upstream 0.3.4 calls
    // it "cdfs", upstream dev renames it "norm_cdfs" (identical formula —
    // dev's `_GAUSS_MODEL_TYPES = ('norm_cdfs',)`). The skewed
    // "sept_norm_cdfs" variant would evaluate wrongly as Gaussian — refuse
    // rather than emit silently-wrong densities.
    if model.model_type != "cdfs" && model.model_type != "norm_cdfs" {
        return Err(format!(
            "unsupported density_curves_model type {:?} (expected \"cdfs\"/\"norm_cdfs\")",
            model.model_type
        ));
    }
    if model.n_layers() == 0 {
        return Err("s023 morph requires a fitted density_curves_model".into());
    }
    if model.n_channels() != 3 {
        return Err(format!(
            "s023 morph expects 3 channels, got {}",
            model.n_channels()
        ));
    }
    let n_layers = model.n_layers();
    for (name, rows) in [
        ("centers", &model.centers),
        ("amplitudes", &model.amplitudes),
        ("sigmas", &model.sigmas),
    ] {
        if rows.len() != 3 || rows.iter().any(|r| r.len() != n_layers) {
            return Err(format!("density_curves_model.{name} must be 3×{n_layers}"));
        }
    }
    // Upstream short-circuits when inactive: the fitted model is evaluated
    // as-is and the morph parameters — including invalid ones — are never
    // read.
    if p.active {
        for (name, v) in [
            ("gamma_factor", p.gamma_factor),
            ("gamma_factor_fast", p.gamma_factor_fast),
            ("gamma_factor_slow", p.gamma_factor_slow),
            ("gamma_factor_red", p.gamma_factor_red),
            ("gamma_factor_green", p.gamma_factor_green),
            ("gamma_factor_blue", p.gamma_factor_blue),
        ] {
            if v <= 0.0 {
                return Err(format!("{name} must be strictly positive (got {v})"));
            }
        }
        if !(0.0..=1.0).contains(&p.developer_exhaustion) {
            return Err(format!(
                "developer_exhaustion must be in [0, 1] (got {})",
                p.developer_exhaustion
            ));
        }
    }

    let mut total = vec![[0.0f64; 3]; log_exposure.len()];
    let mut layers = vec![vec![[0.0f64; 3]; n_layers]; log_exposure.len()];
    for channel in 0..3 {
        let (centers, amplitudes, sigmas, mix) = if p.active {
            morph_channel_params(model, p, channel, positive)
        } else {
            (
                model.centers[channel].clone(),
                model.amplitudes[channel].clone(),
                model.sigmas[channel].clone(),
                vec![0.0f64; n_layers],
            )
        };
        for (i, &x) in log_exposure.iter().enumerate() {
            let mut sum = 0.0f64;
            for l in 0..n_layers {
                let d = amplitudes[l] * layer_cdf((x - centers[l]) / sigmas[l], positive, mix[l]);
                layers[i][l][channel] = d;
                sum += d;
            }
            total[i][channel] = sum;
        }
    }
    Ok((total, layers))
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("data")
    }

    /// s023 print-curve morph regression guard. Morphed density curves for the
    /// kodak_portra_endura fitted model are cross-checked against upstream
    /// experimental 28bf883's `morph_curves.py` (scipy `norm.cdf` + `brentq`)
    /// with a non-identity setting that exercises every control, including the
    /// developer-exhaustion offset solve.
    #[test]
    fn morph_matches_python_reference() {
        let dir = data_dir();
        let print = crate::profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
        let model = print.data.density_curves_model.as_ref().unwrap();
        let log_exposure = print.log_exposure_f64();
        let positive = print.is_positive();
        assert!(!positive, "endura is a negative-type print paper");

        let p = CurvesMorphParams {
            active: true,
            gamma_factor: 1.1,
            gamma_factor_fast: 0.9,
            gamma_factor_slow: 1.2,
            gamma_factor_red: 1.05,
            gamma_factor_green: 0.95,
            gamma_factor_blue: 1.0,
            developer_exhaustion: 0.3,
        };
        let morphed = morph_density_curves(&log_exposure, model, &p, positive).unwrap();

        // (sample_index, R, G, B) from experimental 28bf883's
        // `apply_print_curves_morph` on the same (28bf883) model.
        let expect: [(usize, [f64; 3]); 8] = [
            (0, [6.04374181357351e-32, 1.8626071677241133e-26, 5.133309699665567e-29]),
            (32, [1.0587496409351388e-16, 5.213366461788462e-14, 2.4555235615139012e-15]),
            (64, [9.939369217475997e-07, 7.439137710938368e-06, 2.241644080123102e-06]),
            (100, [0.1912862823336439, 0.18531041493200068, 0.15164796399974384]),
            (128, [2.2414834969040576, 1.7608119481223927, 1.7480868974096948]),
            (160, [2.45869493442771, 2.0652999342370912, 1.8206426676953955]),
            (200, [2.4597462787423585, 2.067618475281535, 1.8210696107660937]),
            (255, [2.4597509970155245, 2.067634980096055, 1.8210719974895442]),
        ];
        for (i, want) in expect {
            for c in 0..3 {
                let got = morphed[i][c];
                assert!(
                    (got - want[c]).abs() <= 1e-9 + 1e-9 * want[c].abs(),
                    "row {i} ch {c}: {got} vs {}",
                    want[c]
                );
            }
        }
    }

    /// Identity params reproduce the fitted model (the morph is a no-op at
    /// defaults except `active`).
    #[test]
    fn morph_identity_reproduces_model() {
        let dir = data_dir();
        let print = crate::profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
        let model = print.data.density_curves_model.as_ref().unwrap();
        let log_exposure = print.log_exposure_f64();
        let positive = print.is_positive();

        let identity = CurvesMorphParams {
            active: true,
            ..Default::default()
        };
        let morphed = morph_density_curves(&log_exposure, model, &identity, positive).unwrap();

        // Direct evaluation of the unmorphed model.
        for (i, &x) in log_exposure.iter().enumerate() {
            for c in 0..3 {
                let mut d = 0.0;
                for l in 0..model.n_layers() {
                    let z = (x - model.centers[c][l]) / model.sigmas[c][l];
                    d += model.amplitudes[c][l] * norm_cdf(signed_z(z, positive));
                }
                assert!((morphed[i][c] - d).abs() < 1e-15, "row {i} ch {c}");
            }
        }
    }

    /// The port only implements the Gaussian `cdfs` model; any other
    /// `model_type` must be refused rather than evaluated as Gaussian.
    #[test]
    fn morph_rejects_unknown_model_type() {
        let model = DensityCurvesModel {
            model_type: "sept_norm_cdfs".into(),
            centers: vec![vec![0.0; 3]; 3],
            amplitudes: vec![vec![1.0; 3]; 3],
            sigmas: vec![vec![1.0; 3]; 3],
            alphas: None,
        };
        let p = CurvesMorphParams {
            active: true,
            ..Default::default()
        };
        let err = morph_density_curves(&[0.0, 1.0], &model, &p, false).unwrap_err();
        assert!(err.contains("unsupported"), "{err}");
    }
}
