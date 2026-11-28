//! RAW import white balance. See docs/raw-white-balance.md for upstream
//! provenance and the differences between LibRaw and rawler development.

use anyhow::{Result, bail, ensure};
use rawler::{RawImage, imgop::develop::Intermediate};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawWhiteBalanceMode {
    #[default]
    AsShot,
    Daylight,
    Tungsten,
    Custom,
}

impl RawWhiteBalanceMode {
    pub const ALL: [Self; 4] = [Self::AsShot, Self::Daylight, Self::Tungsten, Self::Custom];

    /// Stable command-line spelling, matching the serialized mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AsShot => "as_shot",
            Self::Daylight => "daylight",
            Self::Tungsten => "tungsten",
            Self::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AsShot => "As shot",
            Self::Daylight => "Daylight",
            Self::Tungsten => "Tungsten (2850 K)",
            Self::Custom => "Custom",
        }
    }
}

/// Settings shared by preview and export. Inactive temperature/tint values
/// are retained for the UI but are ignored unless the mode is Custom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawWhiteBalance {
    pub mode: RawWhiteBalanceMode,
    pub temperature: f64,
    pub tint: f64,
}

impl Default for RawWhiteBalance {
    fn default() -> Self {
        Self {
            mode: RawWhiteBalanceMode::AsShot,
            temperature: 5500.0,
            tint: 1.0,
        }
    }
}

impl RawWhiteBalance {
    /// What the decoder uses: the temperature and tint only in Custom mode.
    /// Two settings with the same effective value decode identically.
    pub fn effective(self) -> Self {
        if self.mode == RawWhiteBalanceMode::Custom {
            self
        } else {
            Self { mode: self.mode, ..Self::default() }
        }
    }

    pub fn validate(self) -> Result<()> {
        if self.mode == RawWhiteBalanceMode::Custom {
            ensure!(
                self.temperature.is_finite() && (1667.0..=25000.0).contains(&self.temperature),
                "RAW WB temperature must be between 1667 and 25000 K"
            );
            ensure!(
                self.tint.is_finite() && self.tint > 0.0 && self.tint <= 4.0,
                "RAW WB tint must be greater than 0 and at most 4"
            );
        }
        Ok(())
    }

    fn adjustment(self) -> Option<(f64, f64)> {
        match self.mode {
            RawWhiteBalanceMode::AsShot | RawWhiteBalanceMode::Daylight => None,
            RawWhiteBalanceMode::Tungsten => Some((2850.0, 1.0)),
            RawWhiteBalanceMode::Custom => Some((self.temperature, self.tint)),
        }
    }
}

/// Leave As-shot entirely untouched (including rawler's missing-WB fallback).
/// Other modes start with a camera-matrix-derived daylight development.
pub(crate) fn prepare(raw: &mut RawImage, settings: RawWhiteBalance) -> Result<()> {
    settings.validate()?;
    if settings.mode == RawWhiteBalanceMode::AsShot || is_monochrome(raw) {
        return Ok(());
    }
    use rawler::imgop::{chromatic_adaption::adapt_bradford, xyz::Illuminant};
    // Match RawDevelop's matrix preference and illuminant adaptation exactly.
    let (illuminant, mut matrix) = raw
        .color_matrix_find_first([
            Illuminant::D65,
            Illuminant::A,
            Illuminant::B,
            Illuminant::C,
            Illuminant::D50,
            Illuminant::D55,
            Illuminant::D75,
            Illuminant::Daylight,
            Illuminant::Flash,
        ])
        .ok_or_else(|| {
            anyhow::anyhow!("RAW has no usable camera matrix for daylight white balance")
        })?;
    if illuminant != Illuminant::D65 {
        ensure!(
            matrix.len() == 9,
            "RAW WB requires a 3-channel matrix for non-D65 calibration"
        );
        let m = std::array::from_fn(|i| std::array::from_fn(|j| matrix[i * 3 + j]));
        matrix = adapt_bradford(&illuminant, &Illuminant::D65, &m)
            .as_flattened()
            .to_vec();
    }
    let tag = u16::from(rawler::tags::DngTag::AnalogBalance);
    let balance = analog_balance(raw.dng_tags.get(&tag), matrix.len() / 3);
    raw.wb_coeffs = daylight_coefficients(&matrix, balance)?;
    Ok(())
}

/// The DNG's AnalogBalance, which the stored values already carry (DNG
/// spec: camera neutral = AnalogBalance · CameraCalibration · ColorMatrix ·
/// XYZ white). iPhone 18 ProRAW stores pre-balanced values (AnalogBalance
/// about 2.5, 1, 1.7 with AsShotNeutral 1, 1, 1); without it the daylight
/// coefficients apply that balance a second time and the photograph turns
/// magenta. All ones without the tag, and for a malformed one: a value per
/// colour plane of the matrix (`channels`), each finite and positive. A
/// 3-plane camera's fourth (second green) entry follows green.
fn analog_balance(tag: Option<&rawler::formats::tiff::Value>, channels: usize) -> [f64; 4] {
    let ones = [1.0; 4];
    let Some(value) = tag else {
        return ones;
    };
    if !matches!(channels, 3 | 4) || value.count() != channels {
        return ones;
    }
    let mut balance = ones;
    for (i, v) in balance.iter_mut().take(channels).enumerate() {
        match value.get_f32(i) {
            Ok(Some(x)) if x.is_finite() && x > 0.0 => *v = f64::from(x),
            _ => return ones,
        }
    }
    if channels == 3 {
        balance[3] = balance[1];
    }
    balance
}

pub(crate) fn is_monochrome(raw: &RawImage) -> bool {
    // Monochrome DNGs can be tagged LinearRaw with one sample per pixel,
    // which rawler's is_monochrome() (BlackIsZero only) doesn't recognize.
    raw.is_monochrome()
        || (raw.cpp == 1
            && !matches!(
                raw.photometric,
                rawler::rawimage::RawPhotometricInterpretation::Cfa(_)
            ))
}

fn daylight_coefficients(matrix: &[f32], analog_balance: [f64; 4]) -> Result<[f32; 4]> {
    ensure!(
        matches!(matrix.len(), 9 | 12),
        "RAW WB requires a 3- or 4-channel XYZ-to-camera matrix"
    );
    let white = whitepoint(6504.0);
    let mut response = [0.0; 4];
    for (i, row) in matrix.chunks_exact(3).enumerate() {
        response[i] = row
            .iter()
            .zip(white)
            .map(|(&m, w)| f64::from(m) * w)
            .sum::<f64>();
        ensure!(
            response[i].is_finite() && response[i] > 0.0,
            "RAW camera matrix gives an invalid daylight response for channel {i}"
        );
    }
    if matrix.len() == 9 {
        response[3] = response[1];
    }
    for (r, balance) in response.iter_mut().zip(analog_balance) {
        *r *= balance;
    }
    let wb = response.map(|v| (response[1] / v) as f32);
    ensure!(
        wb.iter().all(|v| v.is_finite() && *v > 0.0),
        "Invalid RAW daylight coefficients"
    );
    Ok(wb)
}

type Matrix = [[f64; 3]; 3];
const IDENTITY: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
const CAT02: Matrix = [
    [0.7328, 0.4296, -0.1624],
    [-0.7036, 1.6975, 0.0061],
    [0.0030, 0.0136, 0.9834],
];
const SRGB_TO_XYZ: Matrix = [
    [0.4124, 0.3576, 0.1805],
    [0.2126, 0.7152, 0.0722],
    [0.0193, 0.1192, 0.9505],
];
const ACES_TO_XYZ: Matrix = [
    [0.9525523959, 0.0, 0.0000936786],
    [0.3439664498, 0.7281660966, -0.0721325464],
    [0.0, 0.0, 1.0088251844],
];

fn mv(m: Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r.iter().zip(v).map(|(a, b)| a * b).sum())
}

fn mm(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn inverse(m: Matrix) -> Matrix {
    let c: Matrix = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3]
                - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3]
        })
    });
    let det: f64 = (0..3).map(|j| m[0][j] * c[0][j]).sum();
    std::array::from_fn(|i| std::array::from_fn(|j| c[j][i] / det))
}

/// CIE daylight locus >=4000 K; Kang 2002 Planckian locus below, as upstream.
fn whitepoint(t: f64) -> [f64; 3] {
    let x = if t >= 4000.0 {
        if t <= 7000.0 {
            -4.607e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 99.11 / t + 0.244063
        } else {
            -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 247.48 / t + 0.237040
        }
    } else {
        -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 0.8776956e3 / t + 0.179910
    };
    let y = if t >= 4000.0 {
        -3.0 * x * x + 2.87 * x - 0.275
    } else if t <= 2222.0 {
        -1.1063814 * x.powi(3) - 1.34811020 * x.powi(2) + 2.18555832 * x - 0.20219683
    } else {
        -0.9549476 * x.powi(3) - 1.37418593 * x.powi(2) + 2.09137015 * x - 0.16748867
    };
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn adaptation(source: [f64; 3], target: [f64; 3]) -> Matrix {
    if source == target {
        return IDENTITY;
    }
    let s = mv(CAT02, source);
    let t = mv(CAT02, target);
    let mut scale = IDENTITY;
    for i in 0..3 {
        scale[i][i] = t[i] / s[i];
    }
    mm(inverse(CAT02), mm(scale, CAT02))
}

fn adjustment_matrix(temperature: f64, tint: f64) -> Matrix {
    if temperature == 6504.0 && tint == 1.0 {
        return IDENTITY;
    }
    let temperature_adjust = adaptation(whitepoint(temperature), whitepoint(6504.0));
    // Upstream operates in ACES2065-1 (D60). Move our linear sRGB (D65)
    // into the same space, apply its XYZ adaptation and ACES green tint,
    // then return to sRGB. colour.RGB_to_RGB defaults to CAT02 adaptation.
    let d65 = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
    let d60 = [0.32168 / 0.33767, 1.0, (1.0 - 0.32168 - 0.33767) / 0.33767];
    let to_aces_xyz = adaptation(d65, d60);
    let mut tint_scale = IDENTITY;
    tint_scale[1][1] = tint;
    let tint_xyz = mm(ACES_TO_XYZ, mm(tint_scale, inverse(ACES_TO_XYZ)));
    mm(
        inverse(SRGB_TO_XYZ),
        mm(
            inverse(to_aces_xyz),
            mm(
                tint_xyz,
                mm(temperature_adjust, mm(to_aces_xyz, SRGB_TO_XYZ)),
            ),
        ),
    )
}

/// The Custom temperature and tint that turn `neutral`, a colour in the
/// Daylight development, into a neutral grey: what As shot shows as grey,
/// read off the Daylight development, gives the Custom settings that develop
/// like As shot. Solved in mireds and log tint, within the controls' ranges;
/// None if no setting comes close.
pub fn custom_for_neutral(neutral: [f64; 3]) -> Option<(f64, f64)> {
    if !neutral.iter().all(|v| v.is_finite() && *v > 0.0) {
        return None;
    }
    let (mired_range, tint_range) = ((1e6 / 25000.0, 1e6 / 1667.0), (0.01f64.ln(), 4f64.ln()));
    let residual = |mired: f64, log_tint: f64| -> Option<[f64; 2]> {
        let out = mv(adjustment_matrix(1e6 / mired, log_tint.exp()), neutral);
        out.iter().all(|v| *v > 0.0).then(|| [(out[0] / out[1]).ln(), (out[2] / out[1]).ln()])
    };
    let (mut mired, mut log_tint) = (1e6 / 6504.0, 0.0);
    for _ in 0..40 {
        let r = residual(mired, log_tint)?;
        if r[0].abs().max(r[1].abs()) < 1e-9 {
            break;
        }
        let h = (1e-3, 1e-5);
        let rm = residual(mired + h.0, log_tint)?;
        let rt = residual(mired, log_tint + h.1)?;
        let j = [[(rm[0] - r[0]) / h.0, (rt[0] - r[0]) / h.1], [(rm[1] - r[1]) / h.0, (rt[1] - r[1]) / h.1]];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if !det.is_finite() || det.abs() < 1e-30 {
            return None;
        }
        let dm = (j[1][1] * r[0] - j[0][1] * r[1]) / det;
        let dt = (j[0][0] * r[1] - j[1][0] * r[0]) / det;
        mired = (mired - dm).clamp(mired_range.0, mired_range.1);
        log_tint = (log_tint - dt).clamp(tint_range.0, tint_range.1);
    }
    let r = residual(mired, log_tint)?;
    // At either end of a range a camera setting past it can't be met: keep
    // the nearest setting while it's close, else give up.
    (r[0].abs().max(r[1].abs()) < 0.05).then(|| (1e6 / mired, log_tint.exp()))
}

/// How `as_shot` relates to `daylight`, the same photograph developed alike
/// but for white balance (interleaved RGB): the linear map `b` with
/// as_shot ≈ b · daylight, fitted over the pixels well clear of clipping, and
/// those pixels' second moments in Daylight (Σ d dᵀ), which weigh colours
/// by how much of the photograph is in them.
pub struct AsShotFit {
    pub b: [[f64; 3]; 3],
    pub moments: [[f64; 3]; 3],
}

pub fn as_shot_fit(as_shot: &[f32], daylight: &[f32]) -> Option<AsShotFit> {
    if as_shot.len() != daylight.len() || as_shot.len() < 3 {
        return None;
    }
    let peak = |data: &[f32]| data.iter().copied().filter(|v| v.is_finite()).fold(0f32, f32::max) as f64;
    // Well below the top: a channel the white balance pushed past the
    // sensor's clip is clipped before the camera matrix mixes it, so a pixel
    // can be clipped without any of its RGB values reaching the top
    // (measured on the web's sample DNGs: at 0.9 the fit missed by up to 6%).
    let (top_a, top_d) = (peak(as_shot) * 0.2, peak(daylight) * 0.2);
    // Normal equations of as_shot = b · daylight: Σ d dᵀ and Σ d aᵀ.
    let (mut dd, mut da) = ([[0.0f64; 3]; 3], [[0.0f64; 3]; 3]);
    let mut count = 0usize;
    for (a, d) in as_shot.chunks_exact(3).zip(daylight.chunks_exact(3)) {
        let a = [a[0] as f64, a[1] as f64, a[2] as f64];
        let d = [d[0] as f64, d[1] as f64, d[2] as f64];
        if !a.iter().chain(&d).all(|v| v.is_finite()) || a.iter().any(|v| *v >= top_a) || d.iter().any(|v| *v >= top_d)
            || d.iter().sum::<f64>() <= 0.0 {
            continue;
        }
        for i in 0..3 {
            for j in 0..3 {
                dd[i][j] += d[i] * d[j];
                da[i][j] += d[i] * a[j];
            }
        }
        count += 1;
    }
    if count < 64 {
        return None;
    }
    let det = dd[0][0] * (dd[1][1] * dd[2][2] - dd[1][2] * dd[2][1]) - dd[0][1] * (dd[1][0] * dd[2][2] - dd[1][2] * dd[2][0])
        + dd[0][2] * (dd[1][0] * dd[2][1] - dd[1][1] * dd[2][0]);
    let scale = dd[0][0].max(dd[1][1]).max(dd[2][2]);
    let b = if det.is_finite() && det.abs() > 1e-9 * scale.powi(3) {
        // bᵀ = (Σ d dᵀ)⁻¹ Σ d aᵀ.
        let bt = mm(inverse(dd), da);
        std::array::from_fn(|i| std::array::from_fn(|j| bt[j][i]))
    } else {
        // Too little colour to fit a full map (a near-grey frame): each
        // channel's gain alone.
        let mut b = [[0.0; 3]; 3];
        for i in 0..3 {
            b[i][i] = da[i][i] / dd[i][i];
        }
        b
    };
    b.iter().flatten().all(|v| v.is_finite()).then_some(AsShotFit { b, moments: dd })
}

/// The Custom temperature and tint that develop like As shot, given how As
/// shot relates to Daylight. Custom's adaptation isn't the camera's, so no
/// setting matches every colour: this one comes closest over the
/// photograph's own colours (least squares, brightness aside), starting
/// from the one that makes As shot's grey grey.
pub fn custom_for_fit(fit: &AsShotFit) -> Option<(f64, f64)> {
    let neutral = mv(inverse(fit.b), [1.0; 3]);
    let start = custom_for_neutral(neutral).unwrap_or((6504.0, 1.0));
    let (mired_range, tint_range) = ((1e6 / 25000.0, 1e6 / 1667.0), (0.01f64.ln(), 4f64.ln()));
    let total: f64 = (0..3).map(|i| (0..3).map(|j| fit.b[i][j] * (0..3).map(|k| fit.moments[j][k] * fit.b[i][k]).sum::<f64>()).sum::<f64>()).sum();
    let cost = |p: [f64; 2]| -> f64 {
        let (mired, log_tint) = (p[0].clamp(mired_range.0, mired_range.1), p[1].clamp(tint_range.0, tint_range.1));
        let adj = adjustment_matrix(1e6 / mired, log_tint.exp());
        // tr(X S Yᵀ) for the matrices X, Y.
        let inner = |x: &Matrix, y: &Matrix| -> f64 {
            (0..3).map(|i| (0..3).map(|j| x[i][j] * (0..3).map(|k| fit.moments[j][k] * y[i][k]).sum::<f64>()).sum::<f64>()).sum()
        };
        let (aa, ab) = (inner(&adj, &adj), inner(&adj, &fit.b));
        if aa <= 0.0 {
            return f64::INFINITY;
        }
        // The best brightness scale k leaves |k·adj − b|² = |b|² − ab²/aa.
        (total - ab * ab / aa) / total
    };
    // Nelder–Mead in mireds and log tint.
    let mut simplex = [[1e6 / start.0, start.1.ln()], [1e6 / start.0 + 5.0, start.1.ln()], [1e6 / start.0, start.1.ln() + 0.02]];
    let mut values = simplex.map(cost);
    for _ in 0..200 {
        let mut order = [0, 1, 2];
        order.sort_by(|&x, &y| values[x].total_cmp(&values[y]));
        let (best, mid, worst) = (order[0], order[1], order[2]);
        if (values[worst] - values[best]).abs() < 1e-14 {
            break;
        }
        let centre = [(simplex[best][0] + simplex[mid][0]) / 2.0, (simplex[best][1] + simplex[mid][1]) / 2.0];
        let at = |t: f64| [centre[0] + t * (simplex[worst][0] - centre[0]), centre[1] + t * (simplex[worst][1] - centre[1])];
        let reflected = at(-1.0);
        let fr = cost(reflected);
        if fr < values[best] {
            let expanded = at(-2.0);
            let fe = cost(expanded);
            (simplex[worst], values[worst]) = if fe < fr { (expanded, fe) } else { (reflected, fr) };
        } else if fr < values[mid] {
            (simplex[worst], values[worst]) = (reflected, fr);
        } else {
            let contracted = at(0.5);
            let fc = cost(contracted);
            if fc < values[worst] {
                (simplex[worst], values[worst]) = (contracted, fc);
            } else {
                for i in [mid, worst] {
                    simplex[i] = [(simplex[i][0] + simplex[best][0]) / 2.0, (simplex[i][1] + simplex[best][1]) / 2.0];
                    values[i] = cost(simplex[i]);
                }
            }
        }
    }
    let best = (0..3).min_by(|&x, &y| values[x].total_cmp(&values[y]))?;
    let p = simplex[best];
    values[best].is_finite().then(|| (1e6 / p[0].clamp(mired_range.0, mired_range.1), p[1].clamp(tint_range.0, tint_range.1).exp()))
}

/// Apply the corrected upstream post-development WB before the loader's
/// final negative clamp. A monochrome sensor has no chromatic balance.
pub(crate) fn finish(
    image: &mut Intermediate,
    settings: RawWhiteBalance,
    monochrome: bool,
) -> Result<()> {
    if monochrome {
        return Ok(());
    }
    let Some((temperature, tint)) = settings.adjustment() else {
        return Ok(());
    };
    if temperature == 6504.0 && tint == 1.0 {
        return Ok(());
    }
    let matrix = adjustment_matrix(temperature, tint);
    match image {
        Intermediate::ThreeColor(pixels) => pixels.data.par_iter_mut().for_each(|p| {
            *p = mv(matrix, p.map(f64::from)).map(|v| v as f32);
        }),
        Intermediate::Monochrome(_) => {}
        Intermediate::FourColor(_) => bail!("RAW WB expected calibrated RGB output"),
    }
    Ok(())
}

/// Apply the same backend-independent adaptation to Apple's D65 linear RGB.
pub(crate) fn finish_rgb(
    rgb: &mut [f32],
    settings: RawWhiteBalance,
    monochrome: bool,
) -> Result<()> {
    settings.validate()?;
    if monochrome {
        return Ok(());
    }
    let Some((temperature, tint)) = settings.adjustment() else {
        return Ok(());
    };
    if temperature == 6504.0 && tint == 1.0 {
        return Ok(());
    }
    let matrix = adjustment_matrix(temperature, tint);
    rgb.par_chunks_exact_mut(3).for_each(|p| {
        let out = mv(matrix, [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
        for (v, result) in p.iter_mut().zip(out) {
            *v = result as f32;
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_and_validation() {
        let mut wb = RawWhiteBalance::default();
        assert_eq!(wb.mode, RawWhiteBalanceMode::AsShot);
        assert_eq!(wb.adjustment(), None);
        wb.mode = RawWhiteBalanceMode::Daylight;
        assert_eq!(wb.adjustment(), None);
        wb.mode = RawWhiteBalanceMode::Tungsten;
        wb.temperature = f64::NAN; // inactive controls are ignored
        wb.tint = -1.0;
        assert_eq!(wb.adjustment(), Some((2850.0, 1.0)));
        assert!(wb.validate().is_ok());
        wb.mode = RawWhiteBalanceMode::Custom;
        assert!(wb.validate().is_err());
        for temperature in [1667.0, 5500.0, 25000.0] {
            wb.temperature = temperature;
            wb.tint = 1.0;
            assert!(wb.validate().is_ok());
        }
        for tint in [0.0, -1.0, f64::INFINITY, f64::NAN, 4.01] {
            wb.tint = tint;
            assert!(wb.validate().is_err());
        }
        wb.tint = 1.0;
        for temperature in [0.0, 1666.0, 25001.0, f64::INFINITY, f64::NAN] {
            wb.temperature = temperature;
            assert!(wb.validate().is_err());
        }
    }

    #[test]
    fn custom_for_neutral_inverts_the_adjustment() {
        for (t, tint) in [(3200.0, 0.85), (5000.0, 1.1), (6504.0, 1.0), (9000.0, 0.9), (2400.0, 1.3)] {
            // The colour this setting makes grey.
            let neutral = mv(inverse(adjustment_matrix(t, tint)), [1.0; 3]);
            let (found_t, found_tint) = custom_for_neutral(neutral).unwrap();
            assert!((found_t - t).abs() < 0.5, "{found_t} != {t}");
            assert!((found_tint - tint).abs() < 1e-4, "{found_tint} != {tint}");
        }
        assert_eq!(custom_for_neutral([0.0, 1.0, 1.0]), None);
        assert_eq!(custom_for_neutral([f64::NAN, 1.0, 1.0]), None);
    }

    #[test]
    fn custom_for_fit_recovers_a_custom_development() {
        // A Daylight development, and an As shot one that is Custom
        // 3800 K / 1.05 itself: the fit finds that setting again.
        let to_as_shot = adjustment_matrix(3800.0, 1.05);
        let mut daylight = Vec::new();
        let mut as_shot = Vec::new();
        let mut seed = 7u32;
        for _ in 0..4000 {
            let mut p = [0.0; 3];
            for v in &mut p {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = 0.01 + 0.15 * f64::from(seed >> 8) / f64::from(1u32 << 24);
            }
            daylight.extend(p.map(|v| v as f32));
            as_shot.extend(mv(to_as_shot, p).map(|v| v as f32));
        }
        // A highlight, which sets where clipping is; it isn't fitted.
        daylight.extend([1.0f32; 3]);
        as_shot.extend(mv(to_as_shot, [1.0; 3]).map(|v| v as f32));
        let fit = as_shot_fit(&as_shot, &daylight).unwrap();
        let (t, tint) = custom_for_fit(&fit).unwrap();
        assert!((t - 3800.0).abs() < 5.0, "{t}");
        assert!((tint - 1.05).abs() < 2e-3, "{tint}");
        assert!(as_shot_fit(&as_shot[..6], &daylight[..6]).is_none());
    }

    #[test]
    fn daylight_coefficients_project_white_and_normalize_green() {
        let m = [0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.75];
        let wb = daylight_coefficients(&m, [1.0; 4]).unwrap();
        let white = whitepoint(6504.0);
        assert!((f64::from(wb[0]) * 0.5 * white[0] - 1.0).abs() < 1e-7);
        assert_eq!(wb[1], 1.0);
        assert!((f64::from(wb[2]) * 0.75 * white[2] - 1.0).abs() < 1e-7);
        assert_eq!(wb[3], wb[1]);
        let mut four = m.to_vec();
        four.extend_from_slice(&[0.0, 0.5, 0.0]);
        assert_eq!(daylight_coefficients(&four, [1.0; 4]).unwrap()[3], 2.0);
        // Stored values already carry AnalogBalance: undo it, not twice.
        let balanced = daylight_coefficients(&m, [2.0, 1.0, 0.5, 1.0]).unwrap();
        assert!((balanced[0] - wb[0] / 2.0).abs() < 1e-6);
        assert!((balanced[2] - wb[2] * 2.0).abs() < 1e-6);
        assert_eq!(balanced[1], 1.0);
        assert!(daylight_coefficients(&[0.0; 9], [1.0; 4]).is_err());
        assert!(daylight_coefficients(&[f32::NAN; 9], [1.0; 4]).is_err());
        assert!(daylight_coefficients(&[1.0; 6], [1.0; 4]).is_err());
    }

    #[test]
    fn analog_balance_needs_one_valid_value_per_plane() {
        use rawler::formats::tiff::{Rational, SRational, Value};
        let r = |v: &[(u32, u32)]| Value::Rational(v.iter().map(|&(n, d)| Rational::new(n, d)).collect());
        assert_eq!(analog_balance(None, 3), [1.0; 4]);
        let three = r(&[(5, 2), (3, 2), (7, 4)]);
        assert_eq!(analog_balance(Some(&three), 3), [2.5, 1.5, 1.75, 1.5]);
        let four = r(&[(5, 2), (3, 2), (7, 4), (1, 2)]);
        assert_eq!(analog_balance(Some(&four), 4), [2.5, 1.5, 1.75, 0.5]);
        // The count must match the matrix's planes.
        assert_eq!(analog_balance(Some(&three), 4), [1.0; 4]);
        assert_eq!(analog_balance(Some(&four), 3), [1.0; 4]);
        assert_eq!(analog_balance(Some(&r(&[(1, 1); 2])), 3), [1.0; 4]);
        // Zero, infinite, NaN, negative and non-numeric values are ignored.
        for bad in [(0, 1), (1, 0), (0, 0)] {
            assert_eq!(analog_balance(Some(&r(&[(1, 1), bad, (1, 1)])), 3), [1.0; 4], "{bad:?}");
        }
        let negative = Value::SRational(vec![SRational::new(1, 1), SRational::new(-1, 1), SRational::new(1, 1)]);
        assert_eq!(analog_balance(Some(&negative), 3), [1.0; 4]);
        assert_eq!(analog_balance(Some(&Value::Undefined(vec![1, 1, 1])), 3), [1.0; 4]);
    }

    #[test]
    fn whitepoints_and_adjustment_match_colour_science_reference() {
        // colour 0.4.x: CCT_to_xy(Kang 2002 / CIE Illuminant D Series),
        // xy_to_XYZ. Reference generated independently in Python.
        for (t, expected) in [
            (2850.0, [1.09784235, 1.0, 0.35530371]),
            (6504.0, [0.95015469, 1.0, 1.08825907]),
        ] {
            for (a, b) in whitepoint(t).into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-8);
            }
        }
        // sRGB -> ACES; upstream's Von Kries/CAT02 XYZ adaptation
        // (3200 -> 6504 K); ACES green * .85; ACES -> sRGB.
        let matrix = adjustment_matrix(3200.0, 0.85);
        for (input, expected) in [
            ([0.12, 0.18, 0.09], [0.05512882, 0.16371677, 0.25686877]),
            ([0.25, 0.05, 0.14], [0.19382519, 0.04054611, 0.35510906]),
        ] {
            for (a, b) in mv(matrix, input).into_iter().zip(expected) {
                // colour uses rounded published sRGB forward/inverse matrices;
                // here the inverse is computed, so allow their rounding error.
                assert!((a - b).abs() < 3e-5, "{a} != {b}");
            }
        }
        assert_eq!(adjustment_matrix(6504.0, 1.0), IDENTITY);
        let warm = mv(adjustment_matrix(2850.0, 1.0), [0.2; 3]);
        assert!(warm[2] > warm[0]); // tungsten WB cools a daylight image
    }
}
