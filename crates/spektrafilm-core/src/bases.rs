//! Film and print base tuning, a port of upstream experimental's
//! `_tuned_base_density` (`model/develop.py`, 28bf883).
//!
//! The tuned base replaces the profile's `base_density` wherever upstream's
//! `compute_density_spectral` adds it: the film base in the print exposure,
//! its mid-grey reference and the film scan; the print base in the print
//! scan. Two places keep the profile's base, as upstream does: the enlarger
//! preflash (it lights the bare film base) and the mean base density of the
//! scanner's black/white exposure correction.
//!
//! Inactive or neutral settings return the profile's base times `scale`, so
//! the defaults are the identity, bit for bit.

use crate::params::{FilmBaseParams, PrintBaseParams, RuntimeParams};
use crate::profile::Profile;
use spektrafilm_math::interp::np_interp;
use spektrafilm_math::spectral::{WAVELENGTH_MIN, WAVELENGTH_STEP};

/// The film's base density as the film stages see it, `film_render.base`
/// applied.
pub fn film_base_density(film: &Profile, params: &RuntimeParams) -> Vec<f64> {
    tuned_film_base(&film.data.base_density, &params.film_render.base)
}

/// The print's base density as the print scan sees it, `print_render.base`
/// applied.
pub fn print_base_density(print: &Profile, params: &RuntimeParams) -> Vec<f64> {
    tuned_print_base(&print.data.base_density, &params.print_render.base)
}

/// `_tuned_base_density(base, params, is_film=True)`.
pub fn tuned_film_base(base: &[f64], p: &FilmBaseParams) -> Vec<f64> {
    if !p.active {
        return base.to_vec();
    }
    let neutral = (p.cyan, p.magenta, p.yellow) == (1.0, 1.0, 1.0) && p.tilt == 0.0;
    let tuned = if neutral {
        base.to_vec()
    } else {
        film_base_tuning(base, p)
    };
    // Upstream scales again here, so a tuned film base takes `scale` twice.
    tuned.iter().map(|v| v * p.scale).collect()
}

/// `_tuned_base_density(base, params, is_film=False)`.
pub fn tuned_print_base(base: &[f64], p: &PrintBaseParams) -> Vec<f64> {
    if !p.active {
        return base.to_vec();
    }
    let neutral = (p.cyan, p.magenta, p.yellow) == (1.0, 1.0, 1.0);
    let tuned = if neutral {
        base.to_vec()
    } else {
        print_base_tuning(base, p)
    };
    tuned.iter().map(|v| v * p.scale).collect()
}

/// The working wavelengths, upstream's `SPECTRAL_SHAPE.wavelengths`.
fn wavelengths(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| WAVELENGTH_MIN as f64 + i as f64 * WAVELENGTH_STEP as f64)
        .collect()
}

/// `base_film_density_tuning`: per-channel density scales interpolated
/// between the status-M peaks and a spectral tilt pivoting at 555 nm, both
/// smoothed with a 35 nm Gaussian, times `scale`.
fn film_base_tuning(base: &[f64], p: &FilmBaseParams) -> Vec<f64> {
    let wl = wavelengths(base.len());
    let sigma = 35.0 / WAVELENGTH_STEP as f64;
    let peaks = [460.0, 555.0, 650.0];
    let shift = np_interp(&wl, &peaks, &[p.yellow, p.magenta, p.cyan]);
    let density_scale = gaussian_filter1d(&shift, sigma);
    let slope = p.tilt / (650.0 - 555.0);
    let tilt: Vec<f64> = wl
        .iter()
        .map(|w| (slope * (w - 555.0) + 1.0).max(0.0))
        .collect();
    let density_tilt = gaussian_filter1d(&tilt, sigma);
    (0..base.len())
        .map(|i| base[i] * p.scale * density_scale[i] * density_tilt[i])
        .collect()
}

/// `base_print_density_tuning`: the base at the status-A peaks, scaled per
/// channel, interpolated back over the spectrum and smoothed with a 20 nm
/// Gaussian. The result replaces the base's spectral shape.
fn print_base_tuning(base: &[f64], p: &PrintBaseParams) -> Vec<f64> {
    let wl = wavelengths(base.len());
    let peaks = [445.0, 530.0, 610.0];
    let at_peaks = np_interp(&peaks, &wl, base);
    let channels = [
        at_peaks[0] * p.yellow,
        at_peaks[1] * p.magenta,
        at_peaks[2] * p.cyan,
    ];
    let spectral_min = np_interp(&wl, &peaks, &channels);
    gaussian_filter1d(&spectral_min, 20.0 / WAVELENGTH_STEP as f64)
}

/// `scipy.ndimage.gaussian_filter1d(x, sigma)` with its defaults: truncate 4,
/// `reflect` borders (`d c b a | a b c d | d c b a`), and the symmetric-kernel
/// summation order of scipy's `NI_Correlate1D`.
fn gaussian_filter1d(x: &[f64], sigma: f64) -> Vec<f64> {
    let radius = (4.0 * sigma + 0.5) as isize;
    let factor = -0.5 / (sigma * sigma);
    let phi: Vec<f64> = (-radius..=radius)
        .map(|k| (factor * (k * k) as f64).exp())
        .collect();
    let sum: f64 = phi.iter().sum();
    let w: Vec<f64> = phi.iter().map(|v| v / sum).collect();
    let n = x.len() as isize;
    let at = |i: isize| {
        // Reflect about the edges, repeating the edge sample.
        let period = 2 * n;
        let i = i.rem_euclid(period);
        x[(if i >= n { period - 1 - i } else { i }) as usize]
    };
    let r = radius as usize;
    (0..n)
        .map(|i| {
            let mut acc = x[i as usize] * w[r];
            for j in -radius..0 {
                acc += (at(i + j) + at(i - j)) * w[(j + radius) as usize];
            }
            acc
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_settings_are_the_identity() {
        let base: Vec<f64> = (0..81).map(|i| 0.1 + 0.002 * i as f64).collect();
        assert_eq!(tuned_film_base(&base, &FilmBaseParams::default()), base);
        assert_eq!(tuned_print_base(&base, &PrintBaseParams::default()), base);
        let off = FilmBaseParams {
            active: false,
            scale: 2.0,
            tilt: 1.0,
            ..Default::default()
        };
        assert_eq!(tuned_film_base(&base, &off), base);
    }

    #[test]
    fn gaussian_filter1d_keeps_constants_and_mass() {
        let flat = vec![0.7; 81];
        for v in gaussian_filter1d(&flat, 7.0) {
            assert!((v - 0.7).abs() < 1e-15);
        }
        // A reflected boundary keeps the sum of a centred impulse.
        let mut impulse = vec![0.0; 81];
        impulse[40] = 1.0;
        let s: f64 = gaussian_filter1d(&impulse, 4.0).iter().sum();
        assert!((s - 1.0).abs() < 1e-14);
    }
}
