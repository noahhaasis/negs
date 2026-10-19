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
    raw.wb_coeffs = daylight_coefficients(&matrix)?;
    Ok(())
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

fn daylight_coefficients(matrix: &[f32]) -> Result<[f32; 4]> {
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
    fn daylight_coefficients_project_white_and_normalize_green() {
        let m = [0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.75];
        let wb = daylight_coefficients(&m).unwrap();
        let white = whitepoint(6504.0);
        assert!((f64::from(wb[0]) * 0.5 * white[0] - 1.0).abs() < 1e-7);
        assert_eq!(wb[1], 1.0);
        assert!((f64::from(wb[2]) * 0.75 * white[2] - 1.0).abs() < 1e-7);
        assert_eq!(wb[3], wb[1]);
        let mut four = m.to_vec();
        four.extend_from_slice(&[0.0, 0.5, 0.0]);
        assert_eq!(daylight_coefficients(&four).unwrap()[3], 2.0);
        assert!(daylight_coefficients(&[0.0; 9]).is_err());
        assert!(daylight_coefficients(&[f32::NAN; 9]).is_err());
        assert!(daylight_coefficients(&[1.0; 6]).is_err());
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
