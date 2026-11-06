//! Negative scan input: print a real colour negative instead of a simulated one.
//!
//! The usual pipeline films the scene (filming stage), then prints and scans
//! the developed negative. A scan of a real negative already *is* the
//! developed negative, so this mode replaces the filming stage with a
//! measurement of it:
//!
//! 1. **Transmittance.** The input's linear RGB is read as the light the
//!    negative let through, one value per scan channel.
//! 2. **Base.** The film base + fog (the orange mask) is the thinnest film in
//!    the frame: its brightest pixels, unless [`NegativeScanParams::base`]
//!    names it. Dividing by it leaves density *above base*, which is what
//!    `develop` produces (the film's curves are normalized to their minimum).
//! 3. **Dye unmixing.** Each scan channel reads some of every dye. The film
//!    profile's spectral dye densities, seen through a model of the scan
//!    channels, give the 3×3 density each channel reads per unit of each dye;
//!    its inverse turns channel densities into C/M/Y dye amounts.
//! 4. **Reference.** The enlarger is calibrated on the film's own mid-grey dye
//!    amounts, as for a simulated negative. `auto_density` pulls that reference
//!    toward this frame's median; `auto_color` shifts each dye so the frame's
//!    median prints neutral, like dialling in a CC filter pack.
//!
//! The dyes then go through the printing and scanning stages unchanged, so
//! paper, enlarger filtration, print exposure and the scan all apply.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f64, to_f64};

use crate::profile::Profile;

/// `io.negative_scan`: read the input as a scan of a real colour negative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NegativeScanParams {
    /// Treat the input as a negative scan: skip filming, measure the dyes.
    pub active: bool,
    /// The film base's linear transmittance per input channel (the scan of
    /// clear, unexposed film: the rebate). `None` estimates it from the
    /// thinnest part of the frame.
    pub base: Option<[f32; 3]>,
    /// 0 prints the negative as a correctly exposed one (the film's mid-grey
    /// density prints mid-grey); 1 prints this frame's median as mid-grey.
    pub auto_density: f32,
    /// 0 keeps the colour the base and the stock give; 1 balances the frame's
    /// median to neutral.
    pub auto_color: f32,
    /// Dye unmixing strength: 1 corrects each channel's reading of the other
    /// two dyes, 0 reads each channel as one dye.
    pub unmix: f32,
}

impl Default for NegativeScanParams {
    fn default() -> Self {
        Self { active: false, base: None, auto_density: 0.0, auto_color: 1.0, unmix: 1.0 }
    }
}

/// Scan channel model: Gaussian sensitivities (peak, sigma in nm) for the
/// input's R, G, B. Inputs reach the engine as linear sRGB (or the colour
/// space a manual interpretation names), whose primaries separate the dyes
/// about this well.
const SCAN_CHANNELS: [(f64, f64); 3] = [(605.0, 22.0), (535.0, 25.0), (455.0, 20.0)];

/// Transmittance floor, so clipped-black or negative linear values (possible
/// after a colour space conversion) give a finite density.
const MIN_TRANSMITTANCE: f64 = 1e-6;

/// What a frame measures to: everything [`NegativeModel::apply`] needs.
#[derive(Debug, Clone, PartialEq)]
pub struct NegativeModel {
    /// Linear transmittance of base + fog per input channel.
    pub base: [f64; 3],
    /// Channel density → dye amount.
    pub unmix_inverse: [[f64; 3]; 3],
    /// Added to every pixel's dye amounts (auto colour).
    pub shift: [f64; 3],
    /// Dye amounts that print as mid-grey, for the enlarger calibration.
    pub midgray: [f64; 3],
}

impl NegativeModel {
    /// Measure the base and the frame's median from `metering`, a small
    /// *linear* frame of the whole photograph (so a zoomed or reduced preview
    /// and the export agree).
    pub fn measure(film: &Profile, params: &NegativeScanParams, metering: &ImageBuf) -> Self {
        let pixels: Vec<[f64; 3]> = metering
            .data
            .chunks_exact(3)
            .map(|p| [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])])
            .collect();
        let base = match params.base {
            Some(b) => b.map(|v| (v as f64).max(MIN_TRANSMITTANCE)),
            None => estimate_base(&pixels),
        };
        let unmix_inverse = invert3(scan_matrix(film, params.unmix as f64));
        let film_mid = film_midgray(film);

        // The frame's median dye amounts, over its central 80%.
        let (w, h) = (metering.width as usize, metering.height as usize);
        let mut columns: [Vec<f64>; 3] = Default::default();
        for y in h / 10..(h * 9 / 10).max(h / 10 + 1).min(h) {
            for x in w / 10..(w * 9 / 10).max(w / 10 + 1).min(w) {
                let dye = to_dye(pixels[y * w + x], &base, &unmix_inverse);
                for c in 0..3 {
                    columns[c].push(dye[c]);
                }
            }
        }
        let median = columns.map(|mut v| median(&mut v));

        let auto_color = params.auto_color.clamp(0.0, 1.0) as f64;
        let auto_density = params.auto_density.clamp(0.0, 1.0) as f64;
        let mean = |v: [f64; 3]| (v[0] + v[1] + v[2]) / 3.0;
        // Auto colour moves each dye's median onto the film's grey, keeping
        // the overall density (that is auto density's job).
        let offset = [0, 1, 2].map(|c| film_mid[c] - median[c]);
        let neutral = mean(offset);
        let shift = offset.map(|o| auto_color * (o - neutral));
        let density_pull = auto_density * (mean(median) - mean(film_mid));
        let midgray = film_mid.map(|v| v + density_pull);
        Self { base, unmix_inverse, shift, midgray }
    }

    /// Linear transmittance → dye amounts above base, in place.
    pub fn apply(&self, image: &mut ImageBuf) {
        image.data.par_chunks_exact_mut(3).for_each(|p| {
            let dye = to_dye([to_f64(p[0]), to_f64(p[1]), to_f64(p[2])], &self.base, &self.unmix_inverse);
            for c in 0..3 {
                p[c] = from_f64((dye[c] + self.shift[c]).max(0.0));
            }
        });
    }
}

fn to_dye(t: [f64; 3], base: &[f64; 3], inverse: &[[f64; 3]; 3]) -> [f64; 3] {
    let d = [0, 1, 2].map(|c| (-(t[c].max(MIN_TRANSMITTANCE) / base[c]).log10()).max(0.0));
    [0, 1, 2].map(|i| (inverse[i][0] * d[0] + inverse[i][1] * d[1] + inverse[i][2] * d[2]).max(0.0))
}

/// The thinnest film: the mean of the brightest 0.2% of pixels (by summed
/// transmittance), skipping pixels clipped in any channel.
fn estimate_base(pixels: &[[f64; 3]]) -> [f64; 3] {
    let mut lit: Vec<&[f64; 3]> = pixels.iter().filter(|p| p.iter().all(|&v| v.is_finite() && v < 0.98)).collect();
    if lit.is_empty() {
        return [1.0; 3];
    }
    let sum = |p: &[f64; 3]| p[0] + p[1] + p[2];
    lit.sort_by(|a, b| sum(b).total_cmp(&sum(a)));
    let k = (lit.len() / 500).max(1).min(lit.len());
    let mut acc = [0.0; 3];
    for p in &lit[..k] {
        for c in 0..3 {
            acc[c] += p[c];
        }
    }
    acc.map(|v| (v / k as f64).max(MIN_TRANSMITTANCE))
}

/// Density each scan channel (rows R, G, B) reads per unit of each dye
/// (columns C, M, Y), the off-diagonal crosstalk scaled by `unmix`.
pub fn scan_matrix(film: &Profile, unmix: f64) -> [[f64; 3]; 3] {
    let cd = &film.data.channel_density;
    let n = cd.len().min(spektrafilm_math::spectral::N_WAVELENGTHS);
    let dye = |wl: usize, c: usize| cd[wl].get(c).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
    let mut m = [[0.0; 3]; 3];
    for (k, &(peak, sigma)) in SCAN_CHANNELS.iter().enumerate() {
        let s: Vec<f64> = (0..n)
            .map(|wl| {
                let l = spektrafilm_math::spectral::WAVELENGTH_MIN as f64 + spektrafilm_math::spectral::WAVELENGTH_STEP as f64 * wl as f64;
                (-(l - peak).powi(2) / (2.0 * sigma * sigma)).exp()
            })
            .collect();
        let total: f64 = s.iter().sum();
        for c in 0..3 {
            let t: f64 = (0..n).map(|wl| s[wl] * 10f64.powf(-dye(wl, c))).sum();
            let d = -(t / total).log10();
            m[k][c] = if k == c { d } else { unmix.clamp(0.0, 1.0) * d };
        }
    }
    m
}

/// The film's mid-grey dye amounts: its normalized curves at log exposure 0,
/// where a grey card lands in the filming stage.
pub fn film_midgray(film: &Profile) -> [f64; 3] {
    let le = film.log_exposure_f64();
    let curves = spektrafilm_model::density_curves::normalize_density_curves_f64(&film.density_curves_f64());
    if le.len() < 2 || curves.len() != le.len() {
        return [0.7; 3];
    }
    let j = le.partition_point(|&x| x < 0.0).clamp(1, le.len() - 1);
    let f = ((0.0 - le[j - 1]) / (le[j] - le[j - 1])).clamp(0.0, 1.0);
    [0, 1, 2].map(|c| {
        let (a, b) = (curves[j - 1][c], curves[j][c]);
        if a.is_finite() && b.is_finite() { a + f * (b - a) } else { 0.7 }
    })
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, |a, b| a.total_cmp(b)).1
}

fn invert3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let (a, b) = ((j + 1) % 3, (j + 2) % 3);
            let (c, d) = ((i + 1) % 3, (i + 2) % 3);
            r[i][j] = (m[a][c] * m[b][d] - m[a][d] * m[b][c]) / det;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film() -> Profile {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        crate::profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap()
    }

    /// A synthetic scan of known dyes: transmittance = base · 10^(−M·dye).
    fn scan_of(dyes: &[[f64; 3]], base: [f64; 3], m: &[[f64; 3]; 3]) -> ImageBuf {
        let mut img = ImageBuf::new(dyes.len() as u32, 1);
        for (i, dye) in dyes.iter().enumerate() {
            for k in 0..3 {
                let d: f64 = (0..3).map(|c| m[k][c] * dye[c]).sum();
                img.data[i * 3 + k] = from_f64(base[k] * 10f64.powf(-d));
            }
        }
        img
    }

    #[test]
    fn defaults_are_off_and_parse_from_partial_json() {
        assert!(!NegativeScanParams::default().active);
        let p: NegativeScanParams = serde_json::from_str(r#"{"active": true}"#).unwrap();
        assert!(p.active);
        assert_eq!(p.unmix, 1.0);
    }

    #[test]
    fn recovers_dyes_from_a_synthetic_scan() {
        let film = film();
        let m = scan_matrix(&film, 1.0);
        let base = [0.8, 0.55, 0.35]; // an orange-ish mask
        let dyes = [[0.0, 0.0, 0.0], [0.5, 0.6, 0.7], [1.2, 0.3, 0.9], [0.1, 1.5, 0.4]];
        let mut img = scan_of(&dyes, base, &m);
        let params = NegativeScanParams { active: true, base: Some(base.map(|v| v as f32)), auto_color: 0.0, ..Default::default() };
        let model = NegativeModel::measure(&film, &params, &img);
        model.apply(&mut img);
        for (i, dye) in dyes.iter().enumerate() {
            for c in 0..3 {
                let got = to_f64(img.data[i * 3 + c]);
                assert!((got - dye[c]).abs() < 1e-3, "pixel {i} dye {c}: {got} vs {}", dye[c]);
            }
        }
    }

    #[test]
    fn estimates_the_base_from_the_thinnest_film() {
        let film = film();
        let m = scan_matrix(&film, 1.0);
        let base = [0.7, 0.5, 0.3];
        let mut dyes: Vec<[f64; 3]> = (0..999).map(|i| [0.3 + (i % 7) as f64 * 0.1, 0.4, 0.5 + (i % 3) as f64 * 0.1]).collect();
        // A sliver of clear film: more than the brightest 0.2% averaged.
        dyes.extend([[0.0; 3]; 5]);
        let img = scan_of(&dyes, base, &m);
        let model = NegativeModel::measure(&film, &NegativeScanParams { active: true, ..Default::default() }, &img);
        for c in 0..3 {
            assert!((model.base[c] - base[c]).abs() < 1e-4, "{:?}", model.base);
        }
    }

    #[test]
    fn auto_color_neutralizes_the_median_and_keeps_its_density() {
        let film = film();
        let m = scan_matrix(&film, 1.0);
        let base = [0.8, 0.6, 0.4];
        let dyes: Vec<[f64; 3]> = (0..400).map(|_| [0.4, 0.7, 0.5]).collect();
        let img = scan_of(&dyes, base, &m);
        let params = NegativeScanParams { active: true, base: Some(base.map(|v| v as f32)), auto_color: 1.0, ..Default::default() };
        let model = NegativeModel::measure(&film, &params, &img);
        let mid = film_midgray(&film);
        let shifted = [0.4 + model.shift[0], 0.7 + model.shift[1], 0.5 + model.shift[2]];
        // The colour now matches the film's grey; the mean density is unchanged.
        let rel = |v: [f64; 3]| { let m = (v[0] + v[1] + v[2]) / 3.0; v.map(|x| x - m) };
        for c in 0..3 {
            assert!((rel(shifted)[c] - rel(mid)[c]).abs() < 1e-3);
        }
        assert!((model.shift.iter().sum::<f64>()).abs() < 1e-9);
        assert_eq!(model.midgray, mid);
    }
}
