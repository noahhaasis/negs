// DIR (Development Inhibitor Release) coupler model.
// Handles same-layer and inter-layer inhibition with spatial diffusion, and
// the Langmuir saturation of upstream experimental (`model/couplers.py` at
// 28bf883): donor-side for negatives, receiver-side for positives.

use rayon::prelude::*;
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::interp::np_interp;
use spektrafilm_math::precision::{Scalar, from_f64};

/// DIR couplers matrix [3][3]. Row = donor layer, Column = receiver layer.
pub fn compute_dir_couplers_matrix(
    gamma_samelayer: [f64; 3],
    gamma_r_to_gb: [f64; 2],
    gamma_g_to_rb: [f64; 2],
    gamma_b_to_rg: [f64; 2],
    inhibition_samelayer: f64,
    inhibition_interlayer: f64,
) -> [[f64; 3]; 3] {
    let mut m = [[0.0f64; 3]; 3];
    m[0][0] = gamma_samelayer[0] * inhibition_samelayer;
    m[1][1] = gamma_samelayer[1] * inhibition_samelayer;
    m[2][2] = gamma_samelayer[2] * inhibition_samelayer;
    m[0][1] = gamma_r_to_gb[0] * inhibition_interlayer;
    m[0][2] = gamma_r_to_gb[1] * inhibition_interlayer;
    m[1][0] = gamma_g_to_rb[0] * inhibition_interlayer;
    m[1][2] = gamma_g_to_rb[1] * inhibition_interlayer;
    m[2][0] = gamma_b_to_rg[0] * inhibition_interlayer;
    m[2][1] = gamma_b_to_rg[1] * inhibition_interlayer;
    m
}

/// Amplitude-matched Langmuir saturation per channel,
/// `S(x) = x (K + x_ref) / (K + x)`. `S(x_ref) = x_ref`, so the calibrated
/// operating point is unchanged; the law rolls off away from it. A channel
/// with an infinite `K` stays linear (`S(x) = x`).
///
/// Python: `langmuir_donor` (`K`, `D_ref`) and `receiver_langmuir`
/// (`Kr`, `c_ref`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Langmuir {
    pub k: [f64; 3],
    pub x_ref: [f64; 3],
}

impl Langmuir {
    pub fn apply(&self, c: usize, x: f64) -> f64 {
        if self.k[c].is_finite() {
            x * (self.k[c] + self.x_ref[c]) / (self.k[c] + x)
        } else {
            x
        }
    }

    /// Per-channel `(K, x_ref, saturating)` at pixel precision.
    fn scalars(&self) -> [(Scalar, Scalar, bool); 3] {
        std::array::from_fn(|c| {
            (
                from_f64(self.k[c]),
                from_f64(self.x_ref[c]),
                self.k[c].is_finite(),
            )
        })
    }
}

#[inline]
fn saturate(x: Scalar, (k, x_ref, saturating): (Scalar, Scalar, bool)) -> Scalar {
    if saturating {
        x * (k + x_ref) / (k + x)
    } else {
        x
    }
}

/// Python `langmuir_donor_params`: `d_max` per channel of the normalized
/// curves, `D_ref = d_max / 2`, `K = langmuir_donor_k_rgb * d_max`.
pub fn langmuir_donor_params(
    density_curves: &[[f64; 3]],
    langmuir_donor_k_rgb: [f64; 3],
) -> Langmuir {
    let d_max = crate::density_curves::max_density_f64(density_curves);
    Langmuir {
        k: std::array::from_fn(|c| langmuir_donor_k_rgb[c] * d_max[c]),
        x_ref: std::array::from_fn(|c| 0.5 * d_max[c]),
    }
}

/// Python `receiver_langmuir_params`: the operating-point inhibitor arriving
/// at each receiver, `c_ref[m] = Σ_k D_ref[k] · M_unit[k][m]`, and the knee
/// `Kr = langmuir_receiver_k_rgb · 2 c_ref`. `matrix_unit` is the couplers
/// matrix before the `amount` scaling, so the knee doesn't move with it.
pub fn receiver_langmuir_params(
    langmuir_d_ref: [f64; 3],
    matrix_unit: &[[f64; 3]; 3],
    langmuir_receiver_k_rgb: [f64; 3],
) -> Langmuir {
    let c_ref: [f64; 3] = std::array::from_fn(|m| {
        langmuir_d_ref[0] * matrix_unit[0][m]
            + langmuir_d_ref[1] * matrix_unit[1][m]
            + langmuir_d_ref[2] * matrix_unit[2][m]
    });
    Langmuir {
        k: std::array::from_fn(|m| langmuir_receiver_k_rgb[m] * (2.0 * c_ref[m])),
        x_ref: c_ref,
    }
}

/// Everything the DIR correction derives from the curves and the params
/// before touching pixels: the scaled matrix, the Langmuir terms, `d_max` and
/// the inverted curves. Shared by the CPU develop and the GPU set-up, so both
/// apply the same saturation. Port of the first half of Python
/// `apply_density_correction_dir_couplers`.
#[derive(Debug, Clone)]
pub struct DirCouplersModel {
    /// `couplers_matrix * amount`.
    pub matrix: [[f64; 3]; 3],
    /// Donor saturation (negatives); `None` is the linear donor.
    pub donor: Option<Langmuir>,
    /// Receiver saturation after the diffusion (positives).
    pub receiver: Option<Langmuir>,
    pub density_max: [f64; 3],
    /// Density curves before DIR couplers, on the `log_exposure` axis.
    pub density_curves_0: Vec<[f64; 3]>,
    pub positive: bool,
}

impl DirCouplersModel {
    /// `density_curves` are the normalized (base-subtracted) curves.
    pub fn new(
        density_curves: &[[f64; 3]],
        log_exposure: &[f64],
        matrix_unit: &[[f64; 3]; 3],
        amount: f64,
        langmuir_donor_k_rgb: [f64; 3],
        langmuir_receiver_k_rgb: [f64; 3],
        positive: bool,
    ) -> Self {
        let donor_params = langmuir_donor_params(density_curves, langmuir_donor_k_rgb);
        let (donor, receiver) = if positive {
            // Reversal interimage: linear first-dev silver donor, saturation
            // on the receiver.
            let receiver =
                receiver_langmuir_params(donor_params.x_ref, matrix_unit, langmuir_receiver_k_rgb);
            (None, Some(receiver))
        } else {
            (Some(donor_params), None)
        };
        let matrix = matrix_unit.map(|row| row.map(|v| v * amount));
        let density_curves_0 = compute_curves_before_dir(
            density_curves,
            log_exposure,
            &matrix,
            donor.as_ref(),
            receiver.as_ref(),
            positive,
        );
        Self {
            matrix,
            donor,
            receiver,
            density_max: crate::density_curves::max_density_f64(density_curves),
            density_curves_0,
            positive,
        }
    }
}

/// Apply exposure correction from DIR couplers.
///
/// density_cmy contributes inhibitor to other layers through the couplers
/// matrix, after the donor saturation. The inhibitor diffuses spatially
/// (Gaussian + exponential tail), then the receiver saturation applies.
#[allow(clippy::too_many_arguments)]
pub fn compute_exposure_correction(
    log_raw: ImageBuf,
    density_cmy: ImageBuf,
    density_max: [f64; 3],
    couplers_matrix: &[[f64; 3]; 3],
    donor: Option<&Langmuir>,
    diffusion_size_pixel: f32,
    diffusion_tail_pixel: f32,
    diffusion_tail_weight: f64,
    receiver: Option<&Langmuir>,
    positive: bool,
) -> ImageBuf {
    let mut density_silver = density_cmy;

    if positive {
        let dmax = density_max.map(from_f64);
        density_silver.par_pixels_mut().for_each(|px| {
            for c in 0..3 {
                px[c] = dmax[c] - px[c];
            }
        });
    }
    if let Some(donor) = donor {
        let lm = donor.scalars();
        density_silver.par_pixels_mut().for_each(|px| {
            for c in 0..3 {
                px[c] = saturate(px[c], lm[c]);
            }
        });
    }

    // log_raw_correction = einsum('ijk, km->ijm', donor, couplers_matrix)
    let cm: [[Scalar; 3]; 3] = couplers_matrix.map(|row| row.map(from_f64));
    // Each pixel's three source lanes must be captured before overwriting.
    let mut correction = density_silver;
    correction.data.par_chunks_exact_mut(3).for_each(|out| {
        let px = [out[0], out[1], out[2]];
        for m in 0..3 {
            out[m] = px[0] * cm[0][m] + px[1] * cm[1][m] + px[2] * cm[2][m];
        }
    });

    if diffusion_size_pixel > 0.0 {
        // Python: `(1 - w) * fast_gaussian_filter(corr, σ_size)
        //         + w * fast_exponential_filter(corr, σ_tail)`.
        // The tail kernel is EXPONENTIAL (3-Gaussian mixture), not
        // another Gaussian — using two Gaussians here was a structural
        // diff vs Python.
        use spektrafilm_math::gaussian::{exponential_filter_channel, gaussian_blur_channel};
        let w_img = correction.width;
        let h_img = correction.height;
        let n_pix = (w_img as usize) * (h_img as usize);
        let w = from_f64(diffusion_tail_weight);
        let one = from_f64(1.0);
        for c in 0..3 {
            let ch = correction.extract_channel(c);
            let mut g = gaussian_blur_channel(&ch, w_img, h_img, diffusion_size_pixel);
            let t = exponential_filter_channel(&ch, w_img, h_img, diffusion_tail_pixel);
            for i in 0..n_pix {
                g[i] = (one - w) * g[i] + w * t[i];
            }
            correction.write_channel(c, &g);
        }
    }

    // Receiver-side saturation of the inhibitor that arrives, i.e. after the
    // spatial diffusion. Positives only.
    if let Some(receiver) = receiver {
        let lm = receiver.scalars();
        correction.par_pixels_mut().for_each(|px| {
            for c in 0..3 {
                px[c] = saturate(px[c], lm[c]);
            }
        });
    }

    let mut result = log_raw;
    result
        .data
        .par_iter_mut()
        .zip(correction.data.par_iter())
        .for_each(|(r, c)| {
            *r -= c;
        });
    result
}

/// Full DIR coupler density correction pipeline.
///
/// Port of Python `apply_density_correction_dir_couplers`. `log_exposure`
/// and `density_curves` (normalized) are the f64 profile tables.
#[allow(clippy::too_many_arguments)]
pub fn apply_density_correction(
    density_cmy: ImageBuf,
    log_raw: ImageBuf,
    pixel_size_um: f32,
    log_exposure: &[f64],
    model: &DirCouplersModel,
    diffusion_size_um: f64,
    diffusion_tail_um: f64,
    diffusion_tail_weight: f64,
    gamma_factor: f32,
    backend: &dyn ComputeBackend,
) -> ImageBuf {
    let diffusion_size_px = (diffusion_size_um / pixel_size_um as f64) as f32;
    let diffusion_tail_px = (diffusion_tail_um / pixel_size_um as f64) as f32;

    let log_raw_corrected = compute_exposure_correction(
        log_raw,
        density_cmy,
        model.density_max,
        &model.matrix,
        model.donor.as_ref(),
        diffusion_size_px,
        diffusion_tail_px,
        diffusion_tail_weight,
        model.receiver.as_ref(),
        model.positive,
    );

    backend.density_curve_interp(
        &log_raw_corrected,
        log_exposure,
        &model.density_curves_0,
        gamma_factor as f64,
    )
}

/// Compute density curves before DIR coupler effects.
///
/// Port of Python `compute_density_curves_before_dir_couplers`, in f64 like
/// upstream: shift each curve's exposure axis by the inhibitor its own
/// (saturated) density releases, then resample on the original axis.
pub fn compute_curves_before_dir(
    density_curves: &[[f64; 3]],
    log_exposure: &[f64],
    couplers_matrix: &[[f64; 3]; 3],
    donor: Option<&Langmuir>,
    receiver: Option<&Langmuir>,
    positive: bool,
) -> Vec<[f64; 3]> {
    let k = density_curves.len();
    let mut dc_silver = density_curves.to_vec();

    if positive {
        // Silver density is `nanmax(curves) - curves`.
        let max_d = crate::density_curves::max_density_f64(density_curves);
        for row in &mut dc_silver {
            for c in 0..3 {
                row[c] = max_d[c] - row[c];
            }
        }
    }
    if let Some(donor) = donor {
        for row in &mut dc_silver {
            for c in 0..3 {
                row[c] = donor.apply(c, row[c]);
            }
        }
    }

    // couplers_amount = donor @ couplers_matrix
    let mut couplers_amount = vec![[0.0f64; 3]; k];
    for j in 0..k {
        for m in 0..3 {
            couplers_amount[j][m] = dc_silver[j][0] * couplers_matrix[0][m]
                + dc_silver[j][1] * couplers_matrix[1][m]
                + dc_silver[j][2] * couplers_matrix[2][m];
            if let Some(receiver) = receiver {
                couplers_amount[j][m] = receiver.apply(m, couplers_amount[j][m]);
            }
        }
    }

    // log_exposure_0 = log_exposure - couplers_amount, then re-interpolate
    // the curves on the shifted axis (`-np.interp(x, xp, -fp)` for positives).
    let mut corrected = vec![[0.0f64; 3]; k];
    for c in 0..3 {
        let le_shifted: Vec<f64> = (0..k)
            .map(|j| log_exposure[j] - couplers_amount[j][c])
            .collect();
        let column = if positive {
            let neg: Vec<f64> = density_curves.iter().map(|row| -row[c]).collect();
            np_interp(log_exposure, &le_shifted, &neg)
                .into_iter()
                .map(|v| -v)
                .collect()
        } else {
            let col: Vec<f64> = density_curves.iter().map(|row| row[c]).collect();
            np_interp(log_exposure, &le_shifted, &col)
        };
        for j in 0..k {
            corrected[j][c] = column[j];
        }
    }

    corrected
}
