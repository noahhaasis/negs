// Stochastic grain generation: upstream experimental's multilayer model.
//
// Port of `spektrafilm/model/grain.py` (`apply_grain` and its helpers) and
// `apply_multiplicative_unsharp_mask` (`model/diffusion.py`) at experimental
// 28bf883. Every emulsion sub-layer of every channel draws a compound
// Poisson-binomial grain field from its own density, blurred by its dye
// clouds; the layers sum to the channel's density. The sum then gets the
// lognormal micro-structure, the pixel blur and the mass-conserving
// multiplicative unsharp mask, and the density floor is removed. A
// single-layer density model is the one-sub-layer case.
//
// Grain is random, so Rust can't match Python pixel for pixel: experimental
// draws from numba's per-thread RNG (`use_fast_stats=True`). The samplers here
// are numpy's exact `rk_poisson` / `rk_binomial` on MT19937 streams seeded
// `ch + 10·sub-layer`, as Python's `use_fast_stats=False` path seeds them.
// `tests/grain_statistics.rs` (spektrafilm-core) checks the statistics against
// Python.

use rayon::prelude::*;
use spektrafilm_math::gaussian;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::numpy_rng::{GaussRng, rk_binomial, rk_poisson};
use spektrafilm_math::precision::{Scalar, ZERO, from_f64, to_f64};
use std::time::Instant;

fn stage_timings_enabled() -> bool {
    std::env::var_os("SPEKTRAFILM_STAGE_TIMINGS").is_some()
}

fn print_stage_timing(enabled: bool, stage: &str, start: Instant) {
    if enabled {
        eprintln!("stage {stage}: {} ms", start.elapsed().as_millis());
    }
}

/// Sub-layers the GPU passes support. Upstream's profiles have three; the
/// Negs-only `kodak_trix400_d76_8min` has four.
pub const MAX_SUB_LAYERS: usize = 4;

/// "RMS granularity" is σ_D × 1000 through a 48 µm circular aperture at net
/// density 1.0 (ISO 6328), upstream `_RMS_APERTURE_*`.
pub const RMS_APERTURE_DIAMETER_UM: f64 = 48.0;
pub const RMS_APERTURE_AREA_UM2: f64 =
    std::f64::consts::PI * (RMS_APERTURE_DIAMETER_UM / 2.0) * (RMS_APERTURE_DIAMETER_UM / 2.0);

/// Grain parameters with every per-stock value resolved (the stock preset
/// applied, B&W stocks broadcast from channel 0). Mirrors upstream
/// `GrainParams` after `digest_params`.
#[derive(Debug, Clone, PartialEq)]
pub struct GrainSettings {
    /// Per channel, σ_D × 1000 at the 48 µm aperture, net density 1.0.
    pub rms_granularity: [f64; 3],
    pub density_min: [f64; 3],
    pub uniformity: [f64; 3],
    /// Relative particle area per sub-layer, coarsest = 1.
    pub particle_scale_sublayers: [f64; 3],
    /// Pixel blur σ in pixels.
    pub blur: f64,
    pub mult_usm_sigma: f64,
    pub mult_usm_amount: f64,
    pub blur_dye_clouds_um: f64,
    /// (blur in µm, clump size in nm).
    pub micro_structure: [f64; 2],
}

/// The density curves the grain reads.
#[derive(Debug, Clone)]
pub struct GrainCurves {
    /// The normalized total curves `develop` maps exposure through
    /// (`density_curves - nanmin`), `[sample][channel]`.
    pub total: Vec<[f64; 3]>,
    pub n_layers: usize,
    /// Per-sub-layer curves, `[sample][sub-layer][channel]` flattened as
    /// `sample * n_layers + layer` (upstream `density_curves_layers`).
    pub layers: Vec<[f64; 3]>,
}

impl GrainCurves {
    /// `layers[sample][sub-layer]`, one entry per sample of `total`.
    pub fn new(total: Vec<[f64; 3]>, layers: &[Vec<[f64; 3]>]) -> Self {
        assert_eq!(layers.len(), total.len(), "one layer row per curve sample");
        let n_layers = layers.first().map_or(0, Vec::len);
        assert!(n_layers > 0 && layers.iter().all(|row| row.len() == n_layers));
        Self { total, n_layers, layers: layers.concat() }
    }

    /// A single-layer density model: the total curve is the only sub-layer
    /// (upstream `density_curves[:, None, :]`).
    pub fn single_layer(total: Vec<[f64; 3]>) -> Self {
        let layers = total.clone();
        Self { total, n_layers: 1, layers }
    }

    pub fn layer(&self, sample: usize, layer: usize) -> [f64; 3] {
        self.layers[sample * self.n_layers + layer]
    }

    pub fn n_samples(&self) -> usize {
        self.total.len()
    }
}

/// Per (sub-layer, channel) grain parameters (upstream `_layer_grain_params`).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerGrainParams {
    /// Density floor per sub-layer, `fraction · density_min`.
    pub density_min_layers: Vec<[f64; 3]>,
    /// Absolute (floor-inclusive) maximum density per sub-layer.
    pub density_max_layers: Vec<[f64; 3]>,
    /// Grains per pixel. Infinite when the channel's RMS granularity is 0:
    /// the noise-free limit, which develops the layer's mean density.
    pub n_particles_per_pixel: Vec<[f64; 3]>,
    /// Coarsest-sub-layer particle area in µm² (upstream
    /// `_coarsest_area_from_curves`).
    pub coarsest_area_um2: [f64; 3],
    pub particle_scale_sublayers: Vec<f64>,
}

impl LayerGrainParams {
    pub fn n_layers(&self) -> usize {
        self.density_max_layers.len()
    }

    /// Optical density of one grain, `Dmax / N`.
    pub fn od_particle(&self, layer: usize, ch: usize) -> f64 {
        let npp = self.n_particles_per_pixel[layer][ch];
        if npp.is_finite() && npp > 0.0 {
            self.density_max_layers[layer][ch] / npp
        } else {
            0.0
        }
    }

    /// Dye-cloud blur σ in pixels. Upstream blurs each sub-layer by
    /// `blur_dye_clouds_um · sqrt(od_particle)` (as written there, in pixels).
    pub fn dye_cloud_sigma(&self, blur_dye_clouds_um: f64, layer: usize, ch: usize) -> f64 {
        blur_dye_clouds_um * self.od_particle(layer, ch).sqrt()
    }
}

/// Upstream truncates `particle_scale_sublayers` to the model's sub-layers.
/// For models with more sub-layers than the three scales (the Negs-only
/// four-layer Tri-X), the extra layers repeat the finest scale.
pub fn sublayer_scales(scales: [f64; 3], n_layers: usize) -> Vec<f64> {
    (0..n_layers).map(|l| scales[l.min(2)]).collect()
}

fn nan_to_num(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else if v == f64::INFINITY {
        f64::MAX
    } else if v == f64::NEG_INFINITY {
        f64::MIN
    } else {
        v
    }
}

/// Upstream `_coarsest_area_from_curves`: the coarsest-sub-layer particle
/// area for which the realised multilayer peak RMS, maximised over the
/// curves, equals `rms_granularity`.
fn coarsest_area_from_curves(
    curves: &GrainCurves,
    density_min_layers: &[[f64; 3]],
    density_max_layers: &[[f64; 3]],
    fractions: &[[f64; 3]],
    scales: &[f64],
    uniformity: [f64; 3],
    rms_granularity: [f64; 3],
) -> [f64; 3] {
    let mut area = [0.0; 3];
    for ch in 0..3 {
        let mut peak = f64::NEG_INFINITY;
        for k in 0..curves.n_samples() {
            let mut s = 0.0;
            for l in 0..curves.n_layers {
                // A sub-layer without density has no grain (upstream would
                // produce NaN here; no shipped profile has one).
                if fractions[l][ch] == 0.0 {
                    continue;
                }
                let d_abs = nan_to_num(curves.layer(k, l)[ch]) + density_min_layers[l][ch];
                let weight = scales[l] / fractions[l][ch];
                s += weight * d_abs * (density_max_layers[l][ch] - uniformity[ch] * d_abs);
            }
            if s > peak {
                peak = s;
            }
        }
        let sigma_in = rms_granularity[ch] / 1000.0;
        area[ch] = sigma_in * sigma_in * RMS_APERTURE_AREA_UM2 / peak.max(1e-9);
    }
    area
}

/// Upstream `_layer_grain_params` on the curve-based path (`apply_grain`
/// always passes `density_curves_layers`).
pub fn layer_grain_params(
    curves: &GrainCurves,
    settings: &GrainSettings,
    pixel_size_um: f64,
) -> LayerGrainParams {
    let n_layers = curves.n_layers;
    // np.nanmax over the curve samples.
    let mut dmax_curves = vec![[f64::NEG_INFINITY; 3]; n_layers];
    for k in 0..curves.n_samples() {
        for (l, dmax) in dmax_curves.iter_mut().enumerate() {
            let v = curves.layer(k, l);
            for ch in 0..3 {
                if !v[ch].is_nan() && v[ch] > dmax[ch] {
                    dmax[ch] = v[ch];
                }
            }
        }
    }
    let mut total = [0.0; 3];
    for dmax in &dmax_curves {
        for ch in 0..3 {
            total[ch] += dmax[ch];
        }
    }
    let fractions: Vec<[f64; 3]> = dmax_curves
        .iter()
        .map(|d| std::array::from_fn(|ch| d[ch] / total[ch]))
        .collect();
    let density_min_layers: Vec<[f64; 3]> = fractions
        .iter()
        .map(|f| std::array::from_fn(|ch| f[ch] * settings.density_min[ch]))
        .collect();
    let density_max_layers: Vec<[f64; 3]> = dmax_curves
        .iter()
        .zip(&density_min_layers)
        .map(|(d, m)| std::array::from_fn(|ch| d[ch] + m[ch]))
        .collect();
    let scales = sublayer_scales(settings.particle_scale_sublayers, n_layers);
    let coarsest = coarsest_area_from_curves(
        curves,
        &density_min_layers,
        &density_max_layers,
        &fractions,
        &scales,
        settings.uniformity,
        settings.rms_granularity,
    );
    let pixel_area = pixel_size_um * pixel_size_um;
    let n_particles_per_pixel = fractions
        .iter()
        .zip(&scales)
        .map(|(f, &scale)| std::array::from_fn(|ch| pixel_area * f[ch] / (coarsest[ch] * scale)))
        .collect();
    LayerGrainParams {
        density_min_layers,
        density_max_layers,
        n_particles_per_pixel,
        coarsest_area_um2: coarsest,
        particle_scale_sublayers: scales,
    }
}

/// Maps a channel's density onto its sub-layer densities: upstream
/// `interp_density_cmy_layers_channel`, a `fast_interp` from the total curve
/// (negated for positives, so it rises) to each sub-layer curve.
pub struct LayerInterp {
    xs: Vec<f64>,
    inv_dx: Vec<f64>,
    sign: f64,
}

/// Where a density falls on the total curve.
#[derive(Clone, Copy)]
pub enum CurvePosition {
    First,
    Last,
    Between(usize, f64),
}

impl LayerInterp {
    pub fn new(curves: &GrainCurves, ch: usize, positive: bool) -> Self {
        let sign = if positive { -1.0 } else { 1.0 };
        let xs: Vec<f64> = curves.total.iter().map(|row| sign * row[ch]).collect();
        let inv_dx = xs
            .windows(2)
            .map(|w| {
                let dx = w[1] - w[0];
                if dx != 0.0 { 1.0 / dx } else { 0.0 }
            })
            .collect();
        Self { xs, inv_dx, sign }
    }

    pub fn locate(&self, density: f64) -> CurvePosition {
        let x = self.sign * density;
        let k = self.xs.len();
        if x <= self.xs[0] {
            CurvePosition::First
        } else if x >= self.xs[k - 1] {
            CurvePosition::Last
        } else {
            // np.searchsorted(xs, x, side='right') - 1
            let low = self.xs.partition_point(|&v| v <= x) - 1;
            CurvePosition::Between(low, (x - self.xs[low]) * self.inv_dx[low])
        }
    }
}

/// The sub-layer density at a located position.
pub fn layer_density(
    curves: &GrainCurves,
    position: CurvePosition,
    layer: usize,
    ch: usize,
) -> f64 {
    match position {
        CurvePosition::First => curves.layer(0, layer)[ch],
        CurvePosition::Last => curves.layer(curves.n_samples() - 1, layer)[ch],
        CurvePosition::Between(low, t) => {
            let y0 = curves.layer(low, layer)[ch];
            let y1 = curves.layer(low + 1, layer)[ch];
            y0 + t * (y1 - y0)
        }
    }
}

/// Blur one plane in place (upstream `fast_gaussian_filter`; σ ≤ 0 is a copy).
fn blur_plane(plane: &mut Vec<Scalar>, width: u32, height: u32, sigma: f64) {
    if sigma > 0.0 {
        *plane = gaussian::gaussian_blur_channel(plane, width, height, sigma as f32);
    }
}

/// One sub-layer's grain from its absolute (floor-inclusive) density, in
/// place: upstream `layer_particle_model`, then the dye-cloud blur.
///
/// The compound Poisson-binomial draw: N_s ~ Poisson(N / sat) sensitised
/// grains, each developed with probability D / Dmax, scaled back by the
/// grain's optical density and the saturation factor. Like numpy, every
/// Poisson count of the plane is drawn before any binomial count.
#[allow(clippy::too_many_arguments)]
pub fn layer_particle_model(
    mut density: Vec<Scalar>,
    width: u32,
    height: u32,
    density_max: f64,
    n_particles_per_pixel: f64,
    grain_uniformity: f64,
    seed: u32,
    dye_cloud_sigma_px: f64,
) -> Vec<Scalar> {
    let dmax = density_max;
    let npp = n_particles_per_pixel;
    let probability_and_saturation = |d: Scalar| {
        let p = (to_f64(d) / dmax).clamp(1e-6, 1.0 - 1e-6);
        (p, 1.0 - p * grain_uniformity * (1.0 - 1e-6))
    };
    if !(npp.is_finite() && npp > 0.0) {
        // RMS granularity 0: infinitely many grains develop the mean exactly.
        for d in &mut density {
            *d = from_f64(probability_and_saturation(*d).0 * dmax);
        }
        return density;
    }
    let od_particle = dmax / npp;
    let mut rng = rand_mt::Mt::new(seed);
    // Counts stay far below u32::MAX (λ is at most a few thousand).
    let seeds: Vec<u32> = density
        .iter()
        .map(|&d| {
            let (_, sat) = probability_and_saturation(d);
            rk_poisson(&mut rng, npp / sat).min(u32::MAX as u64) as u32
        })
        .collect();
    for (d, &n) in density.iter_mut().zip(&seeds) {
        let (p, sat) = probability_and_saturation(*d);
        let developed = rk_binomial(&mut rng, n as u64, p);
        *d = from_f64((developed as f64) * od_particle * sat);
    }
    drop(seeds);
    blur_plane(&mut density, width, height, dye_cloud_sigma_px);
    density
}

/// Upstream `add_micro_structure` on one channel: multiplicative lognormal
/// clumping with mean 1, when the clumps are at least 5 % of a pixel.
fn add_micro_structure(
    plane: &mut [Scalar],
    width: u32,
    height: u32,
    micro_structure: [f64; 2],
    pixel_size_um: f64,
    seed: u32,
) {
    let Some((sigma, blur_px)) = micro_structure_on(micro_structure, pixel_size_um) else {
        return;
    };
    let sigma2 = (sigma * sigma).ln_1p();
    let s = sigma2.sqrt();
    let mut rng = GaussRng::new(seed);
    let mut clumping: Vec<Scalar> = (0..plane.len())
        .map(|_| from_f64((rng.gauss() * s - 0.5 * sigma2).exp()))
        .collect();
    if blur_px > 0.4 {
        blur_plane(&mut clumping, width, height, blur_px);
    }
    for (v, c) in plane.iter_mut().zip(&clumping) {
        *v *= *c;
    }
}

/// A parallel f64 sum whose rounding doesn't depend on rayon's work
/// splitting: fixed chunks, then their sums in order. (A plain `par_iter().sum()`
/// regroups the additions from run to run.)
fn deterministic_sum(values: &[Scalar]) -> f64 {
    let chunks: Vec<f64> = values.par_chunks(1 << 14).map(|c| c.iter().map(|&v| to_f64(v)).sum::<f64>()).collect();
    chunks.iter().sum()
}

/// Upstream `apply_multiplicative_unsharp_mask` on one channel, in place:
/// `D' = D · (D / G_σ*D)^amount`, then one scale factor so the channel's
/// total density (its dye mass) is unchanged. The sums are image-global.
pub fn multiplicative_unsharp_mask(
    plane: &mut Vec<Scalar>,
    width: u32,
    height: u32,
    sigma: f64,
    amount: f64,
) {
    if sigma <= 0.0 || amount <= 0.0 {
        return;
    }
    const EPS: f64 = 1e-6;
    for v in plane.iter_mut() {
        *v = v.max(ZERO);
    }
    let mut blur = plane.clone();
    blur_plane(&mut blur, width, height, sigma);
    let mass = deterministic_sum(plane);
    let (eps, a, a1) = (from_f64(EPS), from_f64(amount), from_f64(1.0 + amount));
    blur.par_iter_mut().zip(plane.par_iter()).for_each(|(g, &d)| {
        *g = if d <= ZERO { ZERO } else { d.max(eps).powf(a1) / g.max(eps).powf(a) };
    });
    let sharpened = deterministic_sum(&blur);
    if sharpened > 0.0 {
        let scale = from_f64(mass / sharpened);
        blur.par_iter_mut().for_each(|v| *v *= scale);
    }
    *plane = blur;
}

/// Apply experimental's grain to a normalized density image (upstream
/// `apply_grain`).
///
/// `curves` are the film's total and per-sub-layer curves; `positive`
/// reverses the interpolation axis. `monochrome` (B&W, upstream's
/// single-channel emulsion) grains channel 0 and copies it to all three
/// lanes. Every sub-layer of every channel draws from its own MT19937
/// stream, so they all run at once; `serial_channels` instead grains one
/// sub-layer at a time, holding only a channel's running sum and the
/// current sub-layer, to lower peak memory. The result is identical.
#[allow(clippy::too_many_arguments)]
pub fn apply_grain(
    density_cmy: ImageBuf,
    pixel_size_um: f64,
    settings: &GrainSettings,
    curves: &GrainCurves,
    positive: bool,
    monochrome: bool,
    serial_channels: bool,
) -> ImageBuf {
    let stage_timings = stage_timings_enabled();
    let (w, h) = (density_cmy.width, density_cmy.height);
    let lp = layer_grain_params(curves, settings, pixel_size_um);
    let interps: Vec<LayerInterp> =
        (0..3).map(|ch| LayerInterp::new(curves, ch, positive)).collect();

    let sub_layer = |ch: usize, l: usize| -> Vec<Scalar> {
        let t = Instant::now();
        // Sub-layer density offset by its floor, built straight from the
        // shared image so no copy of the channel is held.
        let interp = &interps[ch];
        let dmin_l = from_f64(lp.density_min_layers[l][ch]);
        let layer: Vec<Scalar> = density_cmy
            .data
            .par_chunks_exact(3)
            .map(|px| {
                let pos = interp.locate(to_f64(px[ch]));
                from_f64(layer_density(curves, pos, l, ch)) + dmin_l
            })
            .collect();
        let g = layer_particle_model(
            layer,
            w,
            h,
            lp.density_max_layers[l][ch],
            lp.n_particles_per_pixel[l][ch],
            settings.uniformity[ch],
            (ch + 10 * l) as u32,
            lp.dye_cloud_sigma(settings.blur_dye_clouds_um, l, ch),
        );
        print_stage_timing(stage_timings, "grain.layer_particle_model", t);
        g
    };

    // The sub-layers sum in order, so the rounding is the same whichever
    // way they were scheduled.
    let finalize_channel = |ch: usize, layers: &mut dyn Iterator<Item = Vec<Scalar>>| -> Vec<Scalar> {
        let mut grain = layers.next().expect("at least one sub-layer");
        for g in layers {
            grain.iter_mut().zip(&g).for_each(|(s, v)| *s += *v);
        }

        // Upstream `_finalize_grain`, which is per channel throughout.
        let t = Instant::now();
        add_micro_structure(
            &mut grain,
            w,
            h,
            settings.micro_structure,
            pixel_size_um,
            100 + ch as u32,
        );
        blur_plane(&mut grain, w, h, settings.blur);
        if settings.mult_usm_amount > 0.0 && settings.mult_usm_sigma > 0.0 {
            multiplicative_unsharp_mask(
                &mut grain,
                w,
                h,
                settings.mult_usm_sigma,
                settings.mult_usm_amount,
            );
        }
        let dmin = from_f64(settings.density_min[ch]);
        grain.par_iter_mut().for_each(|v| *v -= dmin);
        print_stage_timing(stage_timings, "grain.finalize", t);
        grain
    };

    let channels: Vec<usize> = if monochrome { vec![0] } else { vec![0, 1, 2] };
    let n_layers = lp.n_layers();
    let mut out_planes: Vec<(usize, Vec<Scalar>)> = if serial_channels {
        channels
            .iter()
            .map(|&ch| (ch, finalize_channel(ch, &mut (0..n_layers).map(|l| sub_layer(ch, l)))))
            .collect()
    } else {
        // One OS thread per stream, not a rayon task: a rayon worker waiting
        // on a stream's inner parallel work steals whole other streams and
        // can't resume its own until they finish, which doubled the slowest.
        let sub_layer = &sub_layer;
        let mut layers: Vec<Vec<Scalar>> = std::thread::scope(|scope| {
            let streams: Vec<_> = channels
                .iter()
                .flat_map(|&ch| (0..n_layers).map(move |l| scope.spawn(move || sub_layer(ch, l))))
                .collect();
            streams.into_iter().map(|s| s.join().expect("grain sub-layer")).collect()
        });
        let mut per_channel: Vec<Vec<Vec<Scalar>>> = Vec::with_capacity(channels.len());
        while !layers.is_empty() {
            let rest = layers.split_off(n_layers);
            per_channel.push(std::mem::replace(&mut layers, rest));
        }
        channels
            .par_iter()
            .zip(per_channel)
            .map(|(&ch, ch_layers)| (ch, finalize_channel(ch, &mut ch_layers.into_iter())))
            .collect()
    };
    // Every channel worker has finished reading the density; reuse it.
    let mut out = density_cmy;
    if monochrome {
        let (_, plane) = out_planes.pop().expect("channel 0");
        for ch in 0..3 {
            out.write_channel(ch, &plane);
        }
    } else {
        for (ch, plane) in out_planes {
            out.write_channel(ch, &plane);
        }
    }
    out
}

/// The micro-structure as `add_micro_structure` applies it: `(lognormal
/// sigma, blur σ px)` when the clumps are at least 5 % of a pixel.
fn micro_structure_on(micro_structure: [f64; 2], pixel_size_um: f64) -> Option<(f64, f64)> {
    let blur_px = micro_structure[0] / pixel_size_um;
    let sigma = micro_structure[1] * 0.001 / pixel_size_um;
    (sigma > 0.05).then_some((sigma, blur_px))
}

/// The GPU pass's parameters, with the same thresholds as `apply_grain`.
pub fn gpu_params(
    curves: &GrainCurves,
    settings: &GrainSettings,
    pixel_size_um: f64,
    positive: bool,
    monochrome: bool,
) -> spektrafilm_gpu::GrainGpuParams {
    let lp = layer_grain_params(curves, settings, pixel_size_um);
    let n = lp.n_layers();
    assert!(n <= MAX_SUB_LAYERS, "{n} sub-layers; the GPU grain supports {MAX_SUB_LAYERS}");
    let f32x3 = |v: [f64; 3]| v.map(|x| x as f32);
    let sign = if positive { -1.0 } else { 1.0 };
    let xs: Vec<[f64; 3]> = curves.total.iter().map(|r| r.map(|v| sign * v)).collect();
    let mut inv_dx: Vec<[f32; 3]> = xs
        .windows(2)
        .map(|w| {
            std::array::from_fn(|ch| {
                let dx = w[1][ch] - w[0][ch];
                if dx != 0.0 { (1.0 / dx) as f32 } else { 0.0 }
            })
        })
        .collect();
    inv_dx.push([0.0; 3]);
    let micro = micro_structure_on(settings.micro_structure, pixel_size_um).map(|(sigma, blur_px)| {
        let sigma2 = (sigma * sigma).ln_1p();
        [sigma2 as f32, if blur_px > 0.4 { blur_px as f32 } else { 0.0 }]
    });
    spektrafilm_gpu::GrainGpuParams {
        interp_x: xs.into_iter().map(f32x3).collect(),
        positive,
        interp_inv_dx: inv_dx,
        curves_layers: curves.layers.iter().copied().map(f32x3).collect(),
        density_min_layers: lp.density_min_layers.iter().copied().map(f32x3).collect(),
        density_max_layers: lp.density_max_layers.iter().copied().map(f32x3).collect(),
        n_particles_per_pixel: lp
            .n_particles_per_pixel
            .iter()
            .map(|v| v.map(|x| if x.is_finite() && x > 0.0 { x as f32 } else { 0.0 }))
            .collect(),
        dye_cloud_sigma_px: (0..n)
            .map(|l| std::array::from_fn(|ch| lp.dye_cloud_sigma(settings.blur_dye_clouds_um, l, ch) as f32))
            .collect(),
        grain_uniformity: f32x3(settings.uniformity),
        density_min: f32x3(settings.density_min),
        micro_structure: micro,
        blur_px: settings.blur.max(0.0) as f32,
        mult_usm: (settings.mult_usm_amount > 0.0 && settings.mult_usm_sigma > 0.0)
            .then_some([settings.mult_usm_sigma as f32, settings.mult_usm_amount as f32]),
        base_seed: 0,
        monochrome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curves() -> GrainCurves {
        // Three sigmoid sub-layers per channel on a 0.05-step grid.
        let xs: Vec<f64> = (0..121).map(|i| -3.0 + 0.05 * i as f64).collect();
        let cdf = |z: f64| 1.0 / (1.0 + (-1.7 * z).exp());
        let layers: Vec<Vec<[f64; 3]>> = xs
            .iter()
            .map(|&x| {
                (0..3)
                    .map(|l| {
                        let (c, a) = (-1.0 + 0.8 * l as f64, 0.8 - 0.15 * l as f64);
                        [a * cdf((x - c) / 0.4), a * cdf((x - c - 0.1) / 0.45), a * cdf((x - c + 0.1) / 0.5)]
                    })
                    .collect()
            })
            .collect();
        let total: Vec<[f64; 3]> = layers
            .iter()
            .map(|row| std::array::from_fn(|ch| row.iter().map(|v| v[ch]).sum()))
            .collect();
        GrainCurves::new(total, &layers)
    }

    fn settings() -> GrainSettings {
        GrainSettings {
            rms_granularity: [9.0, 13.0, 30.0],
            density_min: [0.03, 0.03, 0.03],
            uniformity: [0.97, 0.97, 0.97],
            particle_scale_sublayers: [1.0, 0.4, 0.25],
            blur: 0.89,
            mult_usm_sigma: 0.7,
            mult_usm_amount: 1.5,
            blur_dye_clouds_um: 2.0,
            micro_structure: [0.2, 30.0],
        }
    }

    #[test]
    fn serial_and_parallel_grain_are_bit_identical() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(3).build().unwrap();
        let mut density = ImageBuf::new(11, 7);
        for (i, px) in density.pixels_mut().enumerate() {
            let v = from_f64((i % 17) as f64 * 0.12 - 0.1);
            px.copy_from_slice(&[v, v, v]);
        }
        let curves = curves();
        for monochrome in [false, true] {
            for positive in [false, true] {
                let render = |serial| {
                    apply_grain(density.clone(), 2.0, &settings(), &curves, positive, monochrome, serial)
                };
                let parallel = pool.install(|| render(false));
                let serial = pool.install(|| render(true));
                assert!(serial.data.iter().all(|v| v.is_finite()));
                assert_eq!(
                    parallel.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    serial.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    "monochrome={monochrome}, positive={positive}",
                );
                if monochrome {
                    assert!(serial.pixels().all(|px| px[0] == px[1] && px[1] == px[2]));
                }
            }
        }
    }

    #[test]
    fn sub_layer_floors_and_maxima_add_up_to_the_channel() {
        let curves = curves();
        let lp = layer_grain_params(&curves, &settings(), 6.0);
        for ch in 0..3 {
            let floor: f64 = lp.density_min_layers.iter().map(|d| d[ch]).sum();
            assert!((floor - 0.03).abs() < 1e-15);
        }
        // The finer sub-layers hold more, smaller grains.
        assert!(lp.n_particles_per_pixel[2][0] > lp.n_particles_per_pixel[0][0]);
    }

    #[test]
    fn layer_interpolation_reproduces_the_curve_samples() {
        let curves = curves();
        for positive in [false, true] {
            let mut c = curves.clone();
            if positive {
                c.total.reverse();
                let n = c.n_layers;
                let rows: Vec<Vec<[f64; 3]>> =
                    c.layers.chunks(n).rev().map(|r| r.to_vec()).collect();
                c = GrainCurves::new(c.total.clone(), &rows);
            }
            let interp = LayerInterp::new(&c, 1, positive);
            for k in [5usize, 60, 100] {
                let pos = interp.locate(c.total[k][1]);
                let sum: f64 = (0..3).map(|l| layer_density(&c, pos, l, 1)).sum();
                assert!((sum - c.total[k][1]).abs() < 1e-12, "positive={positive} k={k}");
            }
        }
    }

    #[test]
    fn noise_free_limit_develops_the_mean() {
        let plane = vec![from_f64(0.5); 16];
        let out = layer_particle_model(plane, 4, 4, 2.0, f64::INFINITY, 0.97, 0, 0.0);
        assert!(out.iter().all(|&v| (to_f64(v) - 0.5).abs() < 1e-6));
    }

    #[test]
    fn multiplicative_unsharp_mask_keeps_the_mass() {
        let (w, h) = (9u32, 7u32);
        let mut plane: Vec<Scalar> =
            (0..w * h).map(|i| from_f64(0.2 + ((i * 7) % 11) as f64 * 0.1)).collect();
        let before: f64 = plane.iter().map(|&v| to_f64(v)).sum();
        multiplicative_unsharp_mask(&mut plane, w, h, 0.7, 1.5);
        let after: f64 = plane.iter().map(|&v| to_f64(v)).sum();
        assert!((after - before).abs() < 1e-5 * before);
        assert!(plane.iter().all(|&v| v > ZERO));
    }
}
