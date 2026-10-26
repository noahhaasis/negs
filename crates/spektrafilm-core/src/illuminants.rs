//! Light sources by name. Port of Python `model/illuminants.py`
//! `standard_illuminant` (the same on upstream main 3bb2c2d and experimental
//! 28bf883): spectra on the 380–780 nm / 5 nm grid, normalised to mean 1.
//!
//! Names: the CIE daylights `D50`, `D55` and `D65`; `T` (incandescent, the
//! reference of the tungsten Vision3 stocks); `K75P` (Kinoton 75P projector,
//! the viewing light of the 2383/2393 print films); the enlarger lamps
//! `TH-KG3` and `TH-KG3-L`; and `BB<temperature>` blackbodies, e.g. `BB3200`.
//! Python also accepts every other colour-science illuminant name; those
//! aren't ported. Any other name is an error, as in Python (`KeyError`).

use std::borrow::Cow;

use spektrafilm_math::spectral::{self, N_WAVELENGTHS};

use crate::enlarger;
use crate::params::RuntimeParams;
use crate::profile::Profile;

/// The names [`standard_illuminant_f64`] accepts, for error messages.
pub const SUPPORTED: &str = "D50, D55, D65, T, K75P, TH-KG3, TH-KG3-L, BB<temperature>";

/// Light-source spectrum by name at full f64 precision.
pub fn standard_illuminant_f64(name: &str) -> Result<Cow<'static, [f64]>, String> {
    Ok(match name {
        "D50" => Cow::Borrowed(&spectral::ILLUMINANT_D50_F64[..]),
        "D55" => Cow::Borrowed(&spectral::ILLUMINANT_D55_F64[..]),
        "D65" => Cow::Borrowed(&spectral::ILLUMINANT_D65_F64[..]),
        "T" => Cow::Borrowed(&spectral::ILLUMINANT_T_F64[..]),
        "K75P" => Cow::Borrowed(&spectral::ILLUMINANT_K75P_F64[..]),
        "TH-KG3" => Cow::Borrowed(&enlarger::ILLUMINANT_TH_KG3_F64[..]),
        "TH-KG3-L" => Cow::Borrowed(&enlarger::ILLUMINANT_TH_KG3_L_F64[..]),
        _ => Cow::Owned(black_body_spectrum(blackbody_temperature(name)?)),
    })
}

/// Light-source spectrum by name in f32, for the f32 interfaces (tc LUT
/// lookup, GPU uniforms). The D-series and `T`/`K75P` are the baked f32
/// tables, which `spectral::illuminant_xyz_f64` recognises to return the
/// exact Python white.
pub fn standard_illuminant(name: &str) -> Result<Cow<'static, [f32]>, String> {
    Ok(match name {
        "D50" => Cow::Borrowed(&spectral::ILLUMINANT_D50[..]),
        "D55" => Cow::Borrowed(&spectral::ILLUMINANT_D55[..]),
        "D65" => Cow::Borrowed(&spectral::ILLUMINANT_D65[..]),
        "T" => Cow::Borrowed(&spectral::ILLUMINANT_T[..]),
        "K75P" => Cow::Borrowed(&spectral::ILLUMINANT_K75P[..]),
        "TH-KG3" => Cow::Borrowed(&enlarger::ILLUMINANT_TH_KG3[..]),
        "TH-KG3-L" => Cow::Borrowed(&enlarger::ILLUMINANT_TH_KG3_L[..]),
        _ => Cow::Owned(
            black_body_spectrum(blackbody_temperature(name)?)
                .into_iter()
                .map(|v| v as f32)
                .collect(),
        ),
    })
}

/// [`standard_illuminant`] for names already checked by [`check_names`].
pub(crate) fn select_illuminant(name: &str) -> Cow<'static, [f32]> {
    standard_illuminant(name).unwrap_or_else(|e| panic!("{e}"))
}

/// [`standard_illuminant_f64`] for names already checked by [`check_names`].
pub(crate) fn select_illuminant_f64(name: &str) -> Cow<'static, [f64]> {
    standard_illuminant_f64(name).unwrap_or_else(|e| panic!("{e}"))
}

/// Check every illuminant name a render resolves: the film's reference and
/// viewing light, the print's viewing light and the enlarger lamp.
/// `Pipeline::new_with_spectral` calls this first, so the stages can resolve
/// names without returning errors.
pub fn check_names(film: &Profile, print: &Profile, params: &RuntimeParams) -> Result<(), String> {
    for (what, name) in [
        ("film reference_illuminant", &film.info.reference_illuminant),
        ("film viewing_illuminant", &film.info.viewing_illuminant),
        ("print viewing_illuminant", &print.info.viewing_illuminant),
        ("enlarger.illuminant", &params.enlarger.illuminant),
    ] {
        standard_illuminant_f64(name).map_err(|e| format!("{what}: {e}"))?;
    }
    Ok(())
}

/// The temperature of a `BB<temperature>` name (Python: `np.double(name[2:])`).
fn blackbody_temperature(name: &str) -> Result<f64, String> {
    name.strip_prefix("BB")
        .and_then(|t| t.parse::<f64>().ok())
        .filter(|t| t.is_finite() && *t > 0.0)
        .ok_or_else(|| format!("unknown illuminant {name:?}; supported: {SUPPORTED}"))
}

/// Python `black_body_spectrum` (colour's `planck_law` with its CIE constants
/// and n = 1), normalised like `standard_illuminant`.
fn black_body_spectrum(temperature: f64) -> Vec<f64> {
    const C1: f64 = 3.741771e-16;
    const C2: f64 = 1.4388e-2;
    let values: Vec<f64> = (0..N_WAVELENGTHS)
        .map(|i| {
            let wavelength = (spectral::WAVELENGTH_MIN as f64
                + i as f64 * spectral::WAVELENGTH_STEP as f64)
                * 1e-9;
            let d = 1.0 / (C2 / (wavelength * temperature)).exp_m1();
            (C1 * wavelength.powf(-5.0) / std::f64::consts::PI) * d
        })
        .collect();
    let normalization = numpy_sum(&values) / N_WAVELENGTHS as f64;
    values.into_iter().map(|v| v / normalization).collect()
}

/// `np.sum` of a contiguous f64 array shorter than 128: eight interleaved
/// partial sums, combined pairwise, then the remainder in order.
fn numpy_sum(xs: &[f64]) -> f64 {
    debug_assert!(xs.len() <= 128);
    if xs.len() < 8 {
        return xs.iter().fold(-0.0, |acc, &v| acc + v);
    }
    let mut r = [0.0f64; 8];
    r.copy_from_slice(&xs[..8]);
    let blocks = xs.len() - xs.len() % 8;
    for chunk in xs[8..blocks].chunks_exact(8) {
        for (acc, &v) in r.iter_mut().zip(chunk) {
            *acc += v;
        }
    }
    let mut sum = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
    for &v in &xs[blocks..] {
        sum += v;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_are_errors() {
        for name in [
            "",
            "d55",
            "A",
            "BB",
            "BBwarm",
            "BB-3200",
            "BB0",
            "BBinf",
            "Kinoton 75P",
        ] {
            let err = standard_illuminant_f64(name).unwrap_err();
            assert!(err.contains(SUPPORTED), "{name:?}: {err}");
            assert!(standard_illuminant(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn f32_tables_round_the_f64_tables() {
        for name in [
            "D50", "D55", "D65", "T", "K75P", "TH-KG3", "TH-KG3-L", "BB3200",
        ] {
            let f64s = standard_illuminant_f64(name).unwrap();
            let f32s = standard_illuminant(name).unwrap();
            assert_eq!(f64s.len(), N_WAVELENGTHS);
            for (a, b) in f64s.iter().zip(f32s.iter()) {
                // The older D-series and TH-KG3 f32 tables are printed to 6–8 decimals.
                assert!((*a as f32 - b).abs() <= 1e-6, "{name}: {a} vs {b}");
            }
        }
    }
}
