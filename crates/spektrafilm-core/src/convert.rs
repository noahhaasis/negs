//! Reading a scan of a real colour negative back to film density: a port of
//! upstream experimental's `model/convert.py` and
//! `runtime/stages/converting.py` (28bf883), the "convert-film" front end.
//!
//! A scanned negative arrives as linear RGB in the input colour space. The
//! scan model is the Beer–Lambert sandwich the film scan uses:
//!
//! ```text
//! D(λ)  = base(λ) + Σ_c cmy_c · dye_c(λ)
//! L(λ)  = illuminant(λ) · 10^(−D(λ))
//! XYZ   = ∫ L(λ) · CMF(λ) dλ / ∫ illuminant · ȳ
//! RGB   = M · XYZ          (M: CAT02 to the input space's white, then XYZ → RGB)
//! ```
//!
//! [`FilmConverter`] inverts it per pixel exactly, by upstream's box-bounded
//! damped Gauss–Newton from an analytic log-linear seed, so the film density
//! that would have produced each pixel goes on to the print as filming's
//! output would. Two differences from upstream, neither a change of maths:
//! it works in f64 where upstream's default is f32 (both converge to the
//! same root, upstream to about 1e-4 in density), and each pixel stops on
//! its own residual rather than all pixels on the worst one.
//!
//! [`fit_base_params`] is upstream's "Detect base": it fits the film base
//! tuning and the exposure so the model's clear film matches the scan's.
//! [`base_verdict`] is Negs' own: the clear film found along the frame's
//! edges ([`crate::film_edge`]), or upstream's guess from the thinnest part
//! of the picture when there is none.

use rayon::prelude::*;
use spektrafilm_math::colorspace::{chromatic_adaptation_matrix_f64, xy_to_xyz_f64, RgbColourspace};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f64, to_f64};
use spektrafilm_math::spectral::{CMF_X_F64, CMF_Y_F64, CMF_Z_F64, N_WAVELENGTHS};

use crate::params::{FilmBaseParams, RuntimeParams};
use crate::profile::Profile;

const LN10: f64 = std::f64::consts::LN_10;
/// Upstream's `_invert` defaults.
const MAX_ITER: usize = 8;
const TOL: f64 = 1e-6;
const LAMBDA: f64 = 1e-7;

type Mat3 = [[f64; 3]; 3];
const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// `parse_calibration_matrix`: nine numbers, row-major, separated by spaces,
/// commas, semicolons or brackets; the identity for anything else.
pub fn parse_calibration_matrix(text: &str) -> Mat3 {
    let numbers: Option<Vec<f64>> = text
        .split(|c: char| c.is_whitespace() || ",;[]()".contains(c))
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().ok())
        .collect();
    match numbers {
        Some(n) if n.len() == 9 && n.iter().all(|v| v.is_finite()) => {
            [[n[0], n[1], n[2]], [n[3], n[4], n[5]], [n[6], n[7], n[8]]]
        }
        _ => IDENTITY,
    }
}

/// The scan model's fixed parts: the light through each wavelength that
/// counts, its colour matching and the XYZ → input RGB operator.
struct ScanModel {
    /// Per kept wavelength: the illuminant, zero where the dyes or base
    /// aren't defined (upstream's `illum_valid`, with those dropped).
    illum: Vec<f64>,
    cmf: Vec<[f64; 3]>,
    dye: Vec<[f64; 3]>,
    /// The kept wavelengths' indices into the profile's spectra.
    index: Vec<usize>,
    norm: f64,
    m: Mat3,
}

impl ScanModel {
    fn new(film: &Profile, illuminant: &[f64], colourspace: &RgbColourspace) -> Result<Self, String> {
        let n = N_WAVELENGTHS.min(illuminant.len()).min(film.data.channel_density.len());
        if n == 0 {
            return Err("the film has no dye spectra".into());
        }
        let cmf_at = |w: usize| [CMF_X_F64[w], CMF_Y_F64[w], CMF_Z_F64[w]];
        let norm: f64 = (0..n).map(|w| illuminant[w] * CMF_Y_F64[w]).sum();
        let mut white = [0.0; 3];
        for w in 0..n {
            for k in 0..3 {
                white[k] += illuminant[w] * cmf_at(w)[k];
            }
        }
        let white = white.map(|v| v / norm);
        let sum = white[0] + white[1] + white[2];
        let illuminant_xyz = xy_to_xyz_f64(white[0] / sum, white[1] / sum);
        let cat = chromatic_adaptation_matrix_f64(illuminant_xyz, colourspace.whitepoint_xyz);
        let m = mul(&colourspace.xyz_to_rgb, &cat);
        let mut model = Self { illum: Vec::new(), cmf: Vec::new(), dye: Vec::new(), index: Vec::new(), norm, m };
        for w in 0..n {
            let row = &film.data.channel_density[w];
            let dye = [0, 1, 2].map(|c| row.get(c).copied().unwrap_or(f64::NAN));
            if dye.iter().all(|v| v.is_finite()) {
                model.illum.push(illuminant[w]);
                model.cmf.push(cmf_at(w));
                model.dye.push(dye);
                model.index.push(w);
            }
        }
        Ok(model)
    }

    /// The tuned base at the kept wavelengths; a wavelength whose base isn't
    /// defined lets no light through, as upstream's NaN handling does.
    fn base_at(&self, base: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut illum = self.illum.clone();
        let offset = self
            .index
            .iter()
            .enumerate()
            .map(|(i, &w)| match base.get(w).copied().filter(|v| v.is_finite()) {
                Some(v) => v,
                None => {
                    illum[i] = 0.0;
                    0.0
                }
            })
            .collect();
        (illum, offset)
    }

    /// The RGB of film with no dye: base and fog only, under `illum`.
    fn clear_rgb(&self, illum: &[f64], base: &[f64]) -> [f64; 3] {
        let mut xyz = [0.0; 3];
        for (i, cmf) in self.cmf.iter().enumerate() {
            let light = illum[i] * (-LN10 * base[i]).exp();
            for k in 0..3 {
                xyz[k] += light * cmf[k];
            }
        }
        apply(&self.m, xyz.map(|v| v / self.norm))
    }
}

/// The exact inverse of the film scan: input RGB → film CMY density.
pub struct FilmConverter {
    model: ScanModel,
    illum: Vec<f64>,
    base: Vec<f64>,
    x_hi: [f64; 3],
    gain: f64,
    calibration: Mat3,
    cmy0: [f64; 3],
    logrgb0: [f64; 3],
    ginv: Mat3,
}

impl FilmConverter {
    /// The converter for `film` (as the pipeline resolved it, film curves
    /// morphed) with `params`' base tuning and `film_render.convert`.
    pub fn new(film: &Profile, params: &RuntimeParams) -> Result<Self, String> {
        let convert = &params.film_render.convert;
        let illuminant = crate::illuminants::standard_illuminant_f64(&convert.scan_illuminant)?;
        let colourspace = RgbColourspace::input(&params.io.input_color_space);
        let model = ScanModel::new(film, &illuminant, colourspace)?;
        let tuned = crate::bases::tuned_film_base(&film.data.base_density, &params.film_render.base);
        let (illum, base) = model.base_at(&tuned);
        // The cmy domain: each channel's density range on the film's curves,
        // what develop puts out.
        let curves = film.density_curves_f64();
        let mut x_hi = [0.0; 3];
        for c in 0..3 {
            let values = curves.iter().map(|row| row[c]).filter(|v| v.is_finite());
            let lo = values.clone().fold(f64::INFINITY, f64::min);
            let hi = values.fold(f64::NEG_INFINITY, f64::max);
            x_hi[c] = if hi > lo { hi - lo } else { 3.0 };
        }
        let mut converter = Self {
            model,
            illum,
            base,
            x_hi,
            gain: 2f64.powf(convert.exposure_compensation_ev),
            calibration: parse_calibration_matrix(&convert.calibration),
            cmy0: [0.0; 3],
            logrgb0: [0.0; 3],
            ginv: IDENTITY,
        };
        converter.refresh_seed();
        Ok(converter)
    }

    fn refresh_seed(&mut self) {
        self.cmy0 = self.x_hi.map(|v| 0.5 * v);
        let (rgb0, j0) = self.forward_jac(self.cmy0);
        let clipped = rgb0.map(|v| v.max(1e-12));
        self.logrgb0 = clipped.map(f64::log10);
        let mut g = [[0.0; 3]; 3];
        for i in 0..3 {
            for c in 0..3 {
                g[i][c] = -(j0[i][c] / (clipped[i] * LN10));
            }
        }
        self.ginv = invert3(&g).unwrap_or(IDENTITY);
    }

    fn seed(&self, rgb: [f64; 3]) -> [f64; 3] {
        let d = [0, 1, 2].map(|j| self.logrgb0[j] - rgb[j].max(1e-12).log10());
        [0, 1, 2].map(|i| {
            let v = self.cmy0[i] + self.ginv[i][0] * d[0] + self.ginv[i][1] * d[1] + self.ginv[i][2] * d[2];
            v.clamp(0.0, self.x_hi[i])
        })
    }

    /// The scan of film density `cmy`, and its Jacobian `J[i][c] = ∂rgb_i/∂cmy_c`.
    fn forward_jac(&self, cmy: [f64; 3]) -> ([f64; 3], Mat3) {
        let m = &self.model;
        let mut xyz = [0.0; 3];
        // dxyz[k][c] = Σ L·dye_c·cmf_k
        let mut dxyz = [[0.0; 3]; 3];
        for i in 0..m.illum.len() {
            let dye = m.dye[i];
            let d = self.base[i] + cmy[0] * dye[0] + cmy[1] * dye[1] + cmy[2] * dye[2];
            let mut light = self.illum[i] * (-LN10 * d).exp();
            if !light.is_finite() {
                light = if light.is_nan() { 0.0 } else { 1e12 };
            }
            let cmf = m.cmf[i];
            for k in 0..3 {
                let lk = light * cmf[k];
                xyz[k] += lk;
                for c in 0..3 {
                    dxyz[k][c] += lk * dye[c];
                }
            }
        }
        let xyz = xyz.map(|v| v / m.norm);
        let rgb = apply(&m.m, xyz);
        let scale = -LN10 / m.norm;
        let mut j = [[0.0; 3]; 3];
        for i in 0..3 {
            for c in 0..3 {
                j[i][c] = scale * (m.m[i][0] * dxyz[0][c] + m.m[i][1] * dxyz[1][c] + m.m[i][2] * dxyz[2][c]);
            }
        }
        (rgb, j)
    }

    /// Each dye's highest density, where the inverse stops: film this dense
    /// let no light through (the holder, or nothing scanned).
    pub fn density_range(&self) -> [f64; 3] {
        self.x_hi
    }

    /// The scan RGB of film density `cmy` (the forward model).
    pub fn forward(&self, cmy: [f64; 3]) -> [f64; 3] {
        self.forward_jac(cmy).0
    }

    /// `pretransform`: scanned RGB → the gained, calibrated RGB the model inverts.
    pub fn pretransform(&self, rgb: [f64; 3]) -> [f64; 3] {
        let c = &self.calibration;
        [0, 1, 2].map(|j| (rgb[0] * c[0][j] + rgb[1] * c[1][j] + rgb[2] * c[2][j]) * self.gain)
    }

    /// `_invert`: the film density whose scan is `y`.
    pub fn invert(&self, y: [f64; 3]) -> [f64; 3] {
        let mut x = self.seed(y);
        for _ in 0..MAX_ITER {
            let (f, j) = self.forward_jac(x);
            let r = [f[0] - y[0], f[1] - y[1], f[2] - y[2]];
            if r.iter().all(|v| v.abs() < TOL) || r.iter().any(|v| !v.is_finite()) {
                break;
            }
            let mut jtj = [[0.0; 3]; 3];
            let mut g = [0.0; 3];
            for c in 0..3 {
                for k in 0..3 {
                    jtj[c][k] = j[0][c] * j[0][k] + j[1][c] * j[1][k] + j[2][c] * j[2][k];
                }
                jtj[c][c] += LAMBDA;
                g[c] = j[0][c] * r[0] + j[1][c] * r[1] + j[2][c] * r[2];
            }
            let Some(inverse) = invert3(&jtj) else { break };
            let dx = apply(&inverse, g);
            x = [0, 1, 2].map(|c| (x[c] - dx[c]).clamp(0.0, self.x_hi[c]));
        }
        x
    }

    /// `convert`: scanned RGB → film CMY density.
    pub fn convert(&self, rgb: [f64; 3]) -> [f64; 3] {
        self.invert(self.pretransform(rgb))
    }

    /// The convert front pass of the GPU-resident chain: the same solve,
    /// with each wavelength's illuminant, colour matching and XYZ → RGB
    /// folded into one weight.
    pub fn gpu_params(&self) -> spektrafilm_gpu::ConvertGpuParams {
        let m = &self.model;
        let c = &self.calibration;
        let pretransform = [0, 1, 2].map(|j| [0, 1, 2].map(|i| c[i][j] * self.gain));
        let weights = (0..m.illum.len())
            .map(|i| {
                let light = m.cmf[i].map(|v| self.illum[i] * v / m.norm);
                apply(&m.m, light)
            })
            .collect();
        let dye_base = (0..m.illum.len()).map(|i| [m.dye[i][0], m.dye[i][1], m.dye[i][2], self.base[i]]).collect();
        spektrafilm_gpu::ConvertGpuParams {
            pretransform,
            cmy0: self.cmy0,
            logrgb0: self.logrgb0,
            ginv: self.ginv,
            x_hi: self.x_hi,
            dye_base,
            weights,
        }
    }

    /// Converts a linear input image to film density, in place.
    pub fn convert_image(&self, image: &mut ImageBuf) {
        image.data.par_chunks_mut(3 * 64).for_each(|chunk| {
            for p in chunk.chunks_exact_mut(3) {
                let cmy = self.convert([to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]);
                for c in 0..3 {
                    p[c] = from_f64(cmy[c]);
                }
            }
        });
    }
}

/// `sample_base_rgb`: the mean RGB of the brightest pixels, by Rec. 709
/// luminance at or above the `percentile` (numpy's linear quantile): the
/// clear film transmits the most light.
pub fn sample_base_rgb(pixels: &[[f64; 3]], percentile: f64) -> [f64; 3] {
    let (bright, _) = brightest(pixels, percentile);
    let set: Vec<[f64; 3]> = if bright.is_empty() {
        pixels.iter().copied().filter(|p| p.iter().all(|v| v.is_finite())).collect()
    } else {
        bright.iter().map(|&i| pixels[i]).collect()
    };
    mean(&set)
}

/// The indices of the pixels at or above the luminance `percentile`, and the threshold.
fn brightest(pixels: &[[f64; 3]], percentile: f64) -> (Vec<usize>, f64) {
    let lum = |p: &[f64; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let finite: Vec<usize> = (0..pixels.len()).filter(|&i| pixels[i].iter().all(|v| v.is_finite())).collect();
    if finite.is_empty() {
        return (Vec::new(), f64::NAN);
    }
    let mut sorted: Vec<f64> = finite.iter().map(|&i| lum(&pixels[i])).collect();
    sorted.sort_by(f64::total_cmp);
    let q = percentile.clamp(0.0, 100.0) / 100.0 * (sorted.len() - 1) as f64;
    let (lo, frac) = (q.floor() as usize, q - q.floor());
    let threshold = if lo + 1 < sorted.len() { sorted[lo] + frac * (sorted[lo + 1] - sorted[lo]) } else { sorted[lo] };
    (finite.into_iter().filter(|&i| lum(&pixels[i]) >= threshold).collect(), threshold)
}

fn mean(set: &[[f64; 3]]) -> [f64; 3] {
    let mut acc = [0.0; 3];
    for p in set {
        for c in 0..3 {
            acc[c] += p[c];
        }
    }
    let n = set.len().max(1) as f64;
    acc.map(|v| v / n)
}

/// `fit_base_params`: the film base tuning (cyan, magenta, yellow) and the
/// exposure that make the model's clear film (no dye) the measured clear
/// film `target`, so unexposed film reads as density 0. Upstream's
/// least-squares Levenberg–Marquardt on the same residuals (and the same
/// ridge toward 1) converges to the same fit.
pub fn fit_base_params(target: [f64; 3], film: &Profile, params: &RuntimeParams) -> Result<(FilmBaseParams, f64), String> {
    const W_RIDGE: f64 = 0.02;
    let convert = &params.film_render.convert;
    let illuminant = crate::illuminants::standard_illuminant_f64(&convert.scan_illuminant)?;
    let model = ScanModel::new(film, &illuminant, RgbColourspace::input(&params.io.input_color_space))?;
    let start = &params.film_render.base;
    let tuned = |x: &[f64]| FilmBaseParams { active: true, cyan: x[0], magenta: x[1], yellow: x[2], ..start.clone() };
    let forward_zero = |x: &[f64]| {
        let base = crate::bases::tuned_film_base(&film.data.base_density, &tuned(x));
        let (illum, offset) = model.base_at(&base);
        model.clear_rgb(&illum, &offset)
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let gain_for = |f: [f64; 3]| {
        let denominator = dot(target, target);
        if denominator > 0.0 { dot(f, target) / denominator } else { 1.0 }
    };
    let residual = |x: &[f64]| -> Vec<f64> {
        let f = forward_zero(x);
        let g = gain_for(f);
        vec![
            f[0] - g * target[0],
            f[1] - g * target[1],
            f[2] - g * target[2],
            W_RIDGE * (x[0] - 1.0),
            W_RIDGE * (x[1] - 1.0),
            W_RIDGE * (x[2] - 1.0),
        ]
    };
    let x = levenberg_marquardt(residual, vec![start.cyan, start.magenta, start.yellow], 1e-6, 200);
    let ev = gain_for(forward_zero(&x)).max(1e-9).log2();
    Ok((tuned(&x), ev))
}

/// Where the clear film was found, in the analysed frame's own orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }
}

/// What detecting the base found.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseVerdict {
    /// The clear film's mean RGB: measured on the clear film round the
    /// picture when it was found, else upstream's [`sample_base_rgb`].
    pub rgb: [f64; 3],
    /// Clear film was found along the frame's edges (the rebate or the
    /// gaps between frames, [`crate::film_edge`]). Otherwise the base is
    /// upstream's guess from the thinnest part of the picture.
    pub found: bool,
    /// The edge it was measured on; `None` when on more than one.
    pub edge: Option<Edge>,
    /// Every edge it was measured on.
    pub edges: Vec<Edge>,
    /// The picture's rectangle inside the clear film, when its edges were found.
    pub picture: Option<crate::film_edge::Picture>,
}

/// Finds the clear film round the picture ([`crate::film_edge`], Negs'
/// own) in `image` (linear, the whole frame), falling back to upstream's
/// brightest-`percentile` sample when there is none.
pub fn base_verdict(image: &ImageBuf, percentile: f64) -> BaseVerdict {
    let found = crate::film_edge::find(image);
    match found.clear_film {
        Some(clear) => BaseVerdict {
            rgb: clear.rgb,
            found: true,
            edge: (clear.edges.len() == 1).then(|| clear.edges[0]),
            edges: clear.edges,
            picture: found.picture,
        },
        None => {
            let pixels: Vec<[f64; 3]> = image.data.chunks_exact(3).map(|p| [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]).collect();
            BaseVerdict { rgb: sample_base_rgb(&pixels, percentile), found: false, edge: None, edges: Vec::new(), picture: None }
        }
    }
}

/// The mean RGB in a square around `(x, y)` (fractions of the frame), its
/// side `size` of the frame's long edge: a click on the rebate.
pub fn sample_point_rgb(image: &ImageBuf, x: f64, y: f64, size: f64) -> [f64; 3] {
    let (w, h) = (image.width as f64, image.height as f64);
    let half = (size * w.max(h) / 2.0).max(1.0);
    let (cx, cy) = (x.clamp(0.0, 1.0) * w, y.clamp(0.0, 1.0) * h);
    let x0 = ((cx - half).floor().max(0.0) as u32).min(image.width - 1);
    let x1 = ((cx + half).ceil().min(w) as u32).max(x0 + 1);
    let y0 = ((cy - half).floor().max(0.0) as u32).min(image.height - 1);
    let y1 = ((cy + half).ceil().min(h) as u32).max(y0 + 1);
    let mut set = Vec::new();
    for yy in y0..y1 {
        for xx in x0..x1 {
            let p = image.get(xx, yy);
            let p = [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])];
            if p.iter().all(|v| v.is_finite()) {
                set.push(p);
            }
        }
    }
    mean(&set)
}

/// Levenberg–Marquardt with a forward-difference Jacobian, MINPACK's
/// `lmdif` in outline: small problems only (a few unknowns).
pub(crate) fn levenberg_marquardt(
    f: impl FnMut(&[f64]) -> Vec<f64>,
    x0: Vec<f64>,
    tol: f64,
    max_iter: usize,
) -> Vec<f64> {
    levenberg_marquardt_with_step(f, x0, tol, max_iter, None)
}

/// [`levenberg_marquardt`], with a central-difference Jacobian of step `h`
/// when given: residuals that go through an f32 render need a step wide
/// enough for its rounding (as [`crate::print_balance`]'s solver does).
pub(crate) fn levenberg_marquardt_with_step(
    mut f: impl FnMut(&[f64]) -> Vec<f64>,
    x0: Vec<f64>,
    tol: f64,
    max_iter: usize,
    h: Option<f64>,
) -> Vec<f64> {
    let n = x0.len();
    let mut x = x0;
    let mut r = f(&x);
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut c = cost(&r);
    let mut lambda = 1e-3;
    for _ in 0..max_iter {
        let m = r.len();
        let mut jac = vec![vec![0.0; n]; m];
        for k in 0..n {
            match h {
                None => {
                    let step = f64::EPSILON.sqrt() * x[k].abs().max(1.0);
                    let mut xs = x.clone();
                    xs[k] += step;
                    let rs = f(&xs);
                    for i in 0..m {
                        jac[i][k] = (rs[i] - r[i]) / step;
                    }
                }
                Some(step) => {
                    let (mut hi, mut lo) = (x.clone(), x.clone());
                    hi[k] += step;
                    lo[k] -= step;
                    let (rh, rl) = (f(&hi), f(&lo));
                    for i in 0..m {
                        jac[i][k] = (rh[i] - rl[i]) / (2.0 * step);
                    }
                }
            }
        }
        let mut a = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for i in 0..n {
            for k in 0..n {
                a[i][k] = (0..m).map(|row| jac[row][i] * jac[row][k]).sum();
            }
            g[i] = (0..m).map(|row| jac[row][i] * r[row]).sum();
        }
        let mut improved = false;
        while lambda < 1e12 {
            let mut damped = a.clone();
            for i in 0..n {
                damped[i][i] += lambda * a[i][i].max(1e-12);
            }
            let Some(delta) = solve(damped, g.iter().map(|v| -v).collect()) else {
                lambda *= 10.0;
                continue;
            };
            let candidate: Vec<f64> = x.iter().zip(&delta).map(|(a, b)| a + b).collect();
            let rc = f(&candidate);
            let cc = cost(&rc);
            if cc.is_finite() && cc < c {
                let step: f64 = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
                let size: f64 = x.iter().map(|v| v * v).sum::<f64>().sqrt();
                x = candidate;
                r = rc;
                let relative = (c - cc) / c.max(1e-300);
                c = cc;
                lambda = (lambda / 10.0).max(1e-12);
                improved = true;
                if step <= tol * (size + tol) || relative < 1e-15 {
                    return x;
                }
                break;
            }
            lambda *= 10.0;
        }
        if !improved || c < 1e-30 {
            break;
        }
    }
    x
}

/// Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let factor = a[row][col] / a[col][col];
            for k in col..n {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn invert3(m: &Mat3) -> Option<Mat3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !det.is_finite() || det.abs() < 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portra() -> Profile {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        crate::profile::load_profile_by_name(&data, "kodak_portra_400").unwrap()
    }

    #[test]
    fn calibration_parses_nine_numbers_or_is_the_identity() {
        assert_eq!(parse_calibration_matrix("1 0 0  0 1 0  0 0 1"), IDENTITY);
        assert_eq!(parse_calibration_matrix("[[2, 0, 0], [0, 1, 0], [0, 0, 1]]")[0][0], 2.0);
        assert_eq!(parse_calibration_matrix("1 2 3"), IDENTITY);
        assert_eq!(parse_calibration_matrix("a b c d e f g h i"), IDENTITY);
    }

    #[test]
    fn converting_inverts_the_scan_model() {
        let film = portra();
        let params = RuntimeParams::default();
        let converter = FilmConverter::new(&film, &params).unwrap();
        for cmy in [[0.0, 0.0, 0.0], [0.3, 0.6, 0.9], [1.2, 1.0, 0.8], [0.05, 1.5, 0.4]] {
            // Upstream stops at a scan residual of 1e-6, which dense film
            // magnifies in density.
            let rgb = converter.forward(cmy);
            let back = converter.convert(rgb);
            let again = converter.forward(back);
            for c in 0..3 {
                assert!((back[c] - cmy[c]).abs() < 1e-5, "{cmy:?} came back as {back:?}");
                assert!((again[c] - rgb[c]).abs() < 1e-6, "{rgb:?} scanned back as {again:?}");
            }
        }
    }

    #[test]
    fn a_fitted_base_reads_the_clear_film_as_density_zero() {
        let film = portra();
        let mut params = RuntimeParams::default();
        // A scan whose clear film is a darker, cooler orange than the model's.
        let target = [0.42, 0.21, 0.09];
        let (base, ev) = fit_base_params(target, &film, &params).unwrap();
        params.film_render.base = base;
        params.film_render.convert.exposure_compensation_ev = ev;
        let converter = FilmConverter::new(&film, &params).unwrap();
        let cmy = converter.convert(target);
        assert!(cmy.iter().all(|v| v.abs() < 0.02), "clear film read as {cmy:?}");
    }

    #[test]
    fn the_base_is_found_on_an_even_strip_at_an_edge() {
        let (w, h) = (100u32, 60u32);
        let mut image = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                let rgb = if x < 6 { [0.8, 0.45, 0.25] } else { [0.3 + 0.002 * x as f64, 0.15, 0.05 + 0.001 * y as f64] };
                for c in 0..3 {
                    image.data[i + c] = from_f64(rgb[c]);
                }
            }
        }
        let verdict = base_verdict(&image, 99.0);
        assert!(verdict.found);
        assert_eq!(verdict.edge, Some(Edge::Left));
        assert!((verdict.rgb[0] - 0.8).abs() < 1e-6);

        // No strip: the thinnest part of the picture, a guess.
        let mut picture = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                let t = ((x as f64 - 50.0).powi(2) + (y as f64 - 30.0).powi(2)).sqrt() / 60.0;
                let rgb = [0.6 - 0.3 * t, 0.3 - 0.1 * t * (x as f64 / 100.0), 0.1 + 0.05 * (y as f64 / 60.0)];
                for c in 0..3 {
                    picture.data[i + c] = from_f64(rgb[c]);
                }
            }
        }
        assert!(!base_verdict(&picture, 99.0).found);
    }

    #[test]
    fn levenberg_marquardt_finds_a_least_squares_minimum() {
        let x = levenberg_marquardt(|x| vec![x[0] - 2.0, 10.0 * (x[1] + 1.0), x[0] * x[1] + 2.0], vec![0.0, 0.0], 1e-12, 200);
        assert!((x[0] - 2.0).abs() < 1e-6 && (x[1] + 1.0).abs() < 1e-6, "{x:?}");
    }
}
