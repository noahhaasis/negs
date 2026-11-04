//! One-shot scene exposure placement through the currently selected film.
//! The verified camera preview supplies luminance intent, not colour or a
//! fitted curve. The result is an absolute Prep exposure, independent of the
//! previous placement and excluded from camera/print exposure compensation.
//! From yosoyelchris's Negs fork (`prep-raw-scopes`, d2a144d); see docs/prep.md.
use crate::{
    pipeline::{Pipeline, clip_to_display},
    preview,
    stages::{filming, scanning},
};
use serde::Serialize;
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_math::{
    image::ImageBuf,
    precision::{from_f64, to_f64},
};

pub const POLICY: &str = "negs-camera-preview-placement-v1";
const EDGE: u32 = 256;
const MAX_EV: f64 = 3.;
const MIN_SAMPLES: usize = 128;
const QUANTILES: [f64; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
const EXTRA_WHITE_AREA: f64 = 0.01;

#[derive(Debug, Clone, Serialize)]
pub struct PlacementAnalysis {
    pub policy: &'static str,
    pub exposure_ev: f64,
    pub requested_exposure_ev: f64,
    pub highlight_limited: bool,
    pub before_error_ev: f64,
    pub after_error_ev: f64,
    pub reference_samples: usize,
    pub midtone_quantiles: usize,
    pub baseline_near_white_fraction: f64,
    pub reference_near_white_fraction: f64,
    pub after_near_white_fraction: f64,
    pub near_white_budget: f64,
    pub white_thresholds: WhiteThresholds,
    pub preview_dimensions: [u32; 2],
}

// Recovered from Sawmill's filmic_auto_exposure pure meter. Keep its robust
// quantile/white-budget policy independent of renderer and decoder selection.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhiteThresholds {
    pub waveform: f64,
    pub channels: [f64; 3],
}
#[derive(Clone, Debug)]
struct Distribution {
    quantiles: [f64; 9],
    near_white_fraction: f64,
    samples: usize,
}
fn inverse_srgb(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(x: f64) -> f64 {
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}
fn y(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn median(values: &mut [f64]) -> f64 {
    let middle = values.len() / 2;
    *values.select_nth_unstable_by(middle, f64::total_cmp).1
}
fn distribution(
    pixels: &[[f32; 4]],
    width: u32,
    height: u32,
    thresholds: WhiteThresholds,
) -> Result<Distribution, String> {
    if width == 0
        || height == 0
        || (width as usize).checked_mul(height as usize) != Some(pixels.len())
    {
        return Err("Invalid Auto exposure analysis dimensions".into());
    }
    let mut luminances = Vec::new();
    let mut near_white = 0;
    // Fixed central 80% region reduces camera-vignette/crop differences. Use
    // distributions, not unregistered per-pixel correspondences.
    let (mx, my) = (width / 10, height / 10);
    for row in my..height - my {
        for col in mx..width - mx {
            let p = pixels[(row * width + col) as usize];
            if p.iter().any(|v| !v.is_finite()) || p[3] < 0.95 {
                continue;
            }
            let rgb = [p[0] as f64, p[1] as f64, p[2] as f64];
            let luma = y(rgb.map(inverse_srgb));
            if !luma.is_finite() || luma < 0. {
                continue;
            }
            luminances.push(luma);
            if y(rgb) >= thresholds.waveform || (0..3).any(|c| rgb[c] >= thresholds.channels[c]) {
                near_white += 1;
            }
        }
    }
    if luminances.len() < MIN_SAMPLES {
        return Err("Insufficient opaque finite pixels for Auto exposure".into());
    }
    luminances.sort_unstable_by(f64::total_cmp);
    Ok(Distribution {
        quantiles: QUANTILES
            .map(|q| luminances[((luminances.len() - 1) as f64 * q).round() as usize]),
        near_white_fraction: near_white as f64 / luminances.len() as f64,
        samples: luminances.len(),
    })
}
fn calibration_thresholds(white: [f64; 3]) -> Result<WhiteThresholds, String> {
    if white.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("Selected film has no usable print-white calibration".into());
    }
    // Relative to the selected paper/finishing white, so a sub-1 paper white
    // cannot evade protection simply because it never digitally clips.
    Ok(WhiteThresholds {
        waveform: (0.97 * y(white)).min(0.97),
        channels: white.map(|v| (0.985 * v).min(0.985)),
    })
}
/// A pending trial render; boxed so the solve's steps can await it.
pub type Rendering<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + 'a>>;

/// How [`place_exposure_with`] renders its trials: `pipeline` on `image`,
/// as [`Pipeline::process`] does. A browser awaits the GPU's readback.
pub trait Renderer {
    fn render<'a>(&'a mut self, pipeline: &'a Pipeline, image: ImageBuf) -> Rendering<'a, ImageBuf>;
}

/// Renders on `backend` at once, as [`place_exposure`] does.
struct Now<'b>(&'b dyn ComputeBackend);
impl Renderer for Now<'_> {
    fn render<'a>(&'a mut self, pipeline: &'a Pipeline, image: ImageBuf) -> Rendering<'a, ImageBuf> {
        Box::pin(std::future::ready(Ok(pipeline.process(image, self.0))))
    }
}

/// The value of `future`, which must be ready on its first poll: a solve
/// whose renders all are.
fn now<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => unreachable!("a synchronous placement render was pending"),
    }
}

/// The solve's trials: the distribution of the output at an exposure.
trait Trials {
    fn render(&mut self, ev: f64) -> Rendering<'_, Distribution>;
}
impl<F: FnMut(f64) -> Result<Distribution, String>> Trials for F {
    fn render(&mut self, ev: f64) -> Rendering<'_, Distribution> {
        Box::pin(std::future::ready(self(ev)))
    }
}

#[derive(Debug)]
struct Solution {
    ev: f64,
    requested: f64,
    limited: bool,
    before: f64,
    after: f64,
    before_white: f64,
    after_white: f64,
    budget: f64,
    used_quantiles: usize,
}
#[cfg(test)]
fn solve(
    target: &Distribution,
    mut render: impl FnMut(f64) -> Result<Distribution, String>,
) -> Result<Solution, String> {
    now(solve_with(target, &mut render))
}
async fn solve_with(target: &Distribution, trials: &mut dyn Trials) -> Result<Solution, String> {
    // Target-defined eligibility never changes as exposure is tested. Camera
    // night scenes are not driven to a universal middle-grey histogram.
    let selected: Vec<_> = target
        .quantiles
        .iter()
        .enumerate()
        .filter(|(_, v)| **v >= inverse_srgb(0.15) && **v <= inverse_srgb(0.85))
        .map(|(i, _)| i)
        .collect();
    // The app matches "midtone quantiles" and "±3 EV" below to word its note
    // (CameraBrightnessReference.failure); keep those phrases.
    if selected.len() < 3 {
        return Err(
            "Camera preview has too few usable midtone quantiles; exposure unchanged".into(),
        );
    }
    let residual = |d: &Distribution| -> f64 {
        let mut v: Vec<_> = selected
            .iter()
            .map(|i| (d.quantiles[*i].max(1e-9) / target.quantiles[*i]).log2())
            .collect();
        median(&mut v)
    };
    let baseline = trials.render(0.).await?;
    let before = residual(&baseline);
    let budget =
        (baseline.near_white_fraction.max(target.near_white_fraction) + EXTRA_WHITE_AREA).min(1.);
    let mut points = Vec::new();
    for i in 0..=12 {
        let ev = -MAX_EV + i as f64 * 0.5;
        let d = trials.render(ev).await?;
        points.push((ev, residual(&d)));
    }
    // A robust residual must actually be bracketed and monotone here. Never
    // blindly bisect pathological/extreme material settings.
    if points.windows(2).any(|p| p[1].1 + 0.025 < p[0].1) {
        return Err("Selected film response is not reliably monotone for Auto exposure".into());
    }
    if points[0].1 > 0. || points[12].1 < 0. {
        return Err("Camera brightness requires exposure outside the supported ±3 EV range".into());
    }
    let (mut lo, mut hi) = (-MAX_EV, MAX_EV);
    for pair in points.windows(2) {
        if pair[0].1 <= 0. && pair[1].1 >= 0. {
            lo = pair[0].0;
            hi = pair[1].0;
            break;
        }
    }
    for _ in 0..8 {
        let mid = (lo + hi) * 0.5;
        if residual(&trials.render(mid).await?) < 0. {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let requested = (((lo + hi) * 0.5) * 100.).round() / 100.;
    let mut ev = requested;
    let mut after = trials.render(ev).await?;
    let limited = ev > 0. && after.near_white_fraction > budget;
    if limited {
        let (mut lo, mut hi) = (0., ev);
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            if trials.render(mid).await?.near_white_fraction <= budget {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        ev = (lo * 100.).floor() / 100.;
        after = trials.render(ev).await?;
    }
    Ok(Solution {
        ev,
        requested,
        limited,
        before,
        after: residual(&after),
        before_white: baseline.near_white_fraction,
        after_white: after.near_white_fraction,
        budget,
        used_quantiles: selected.len(),
    })
}

fn valid_dimensions(image: &ImageBuf) -> bool {
    image.width > 0
        && image.height > 0
        && (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|n| n.checked_mul(3))
            == Some(image.data.len())
}
fn proxy(image: &ImageBuf) -> ImageBuf {
    let scale = (EDGE as f64 / image.width.max(image.height) as f64).min(1.0);
    let size = (
        (image.width as f64 * scale).round().max(1.) as u32,
        (image.height as f64 * scale).round().max(1.) as u32,
    );
    preview::downsample_area(image, size)
}
fn rgba(image: &ImageBuf) -> Vec<[f32; 4]> {
    image
        .pixels()
        .map(|p| {
            [
                to_f64(p[0]) as f32,
                to_f64(p[1]) as f32,
                to_f64(p[2]) as f32,
                1.,
            ]
        })
        .collect()
}
fn display_srgb(mut image: ImageBuf, pipeline: &Pipeline) -> ImageBuf {
    let encoded = pipeline.params.io.output_cctf_encoding;
    clip_to_display(&mut image, encoded);
    scanning::convert_output(
        &mut image,
        &pipeline.params.io.output_color_space,
        "sRGB",
        encoded,
    );
    if !encoded {
        for v in &mut image.data {
            *v = from_f64(srgb(to_f64(*v)));
        }
    }
    image
}
fn deterministic(pipeline: &mut Pipeline) {
    crate::image_size::clear(&mut pipeline.params);
    pipeline.params.film_render.grain.active = false;
    pipeline.params.film_render.halation.active = false;
    pipeline.params.camera.diffusion_filter.active = false;
    pipeline.params.enlarger.diffusion_filter.active = false;
    pipeline.params.camera.lens_blur_um = 0.;
    pipeline.params.enlarger.lens_blur = 0.;
    pipeline.params.scanner.lens_blur = 0.;
    pipeline.params.scanner.unsharp_mask = [0., 0.];
    // Keep the film's mean viewing glare, remove its random spatial field.
    pipeline.params.film_render.glare.roughness = 0.;
    pipeline.params.print_render.glare.roughness = 0.;
    pipeline.params.film_render.glare.blur = 0.;
    pipeline.params.print_render.glare.blur = 0.;
}

/// Solve one absolute `prep.exposure_ev` against an oriented, full-frame camera
/// reference in **linear sRGB**. `source` follows the pipeline's input colour
/// space/transfer settings. The caller must verify that the reference belongs
/// to this RAW and has trustworthy colour metadata; never pass a RAW decode as
/// its own camera-reference substitute.
///
/// Source metering is frozen before any trial. The current calibrated film,
/// print, Prep controls and Post intent are retained, with grain/scattering and
/// spatial sharpening disabled on a <=256px proxy. No pipeline is rebuilt per
/// trial, and neither source nor caller's pipeline/previous placement changes.
/// Returned EV replaces the saved placement, never adds to it. Unreliable,
/// nonmonotonic or unbracketed references return an error without a guessed gain.
pub fn place_exposure(
    pipeline: &Pipeline,
    source: &ImageBuf,
    reference: &ImageBuf,
    backend: &dyn ComputeBackend,
) -> Result<PlacementAnalysis, String> {
    now(place_exposure_with(pipeline, source, reference, &mut Now(backend)))
}

/// [`place_exposure`] with its trials rendered by `renderer`, which may
/// await them: a browser's GPU reads back between tasks.
pub async fn place_exposure_with(
    pipeline: &Pipeline,
    source: &ImageBuf,
    reference: &ImageBuf,
    renderer: &mut dyn Renderer,
) -> Result<PlacementAnalysis, String> {
    pipeline.params.prep.validate()?;
    if !valid_dimensions(source) || !valid_dimensions(reference) {
        return Err("Invalid exposure placement image dimensions".into());
    }
    let aspect = |i: &ImageBuf| i.width as f64 / i.height as f64;
    if (aspect(source) / aspect(reference) - 1.).abs() > 0.03 {
        return Err("Camera preview aspect ratio does not match the full source".into());
    }
    let fixed_ev = pipeline.frame_autoexposure_ev(source);
    if !fixed_ev.is_finite() {
        return Err("Source auto exposure is not finite".into());
    }
    let mut meter = pipeline.clone().with_autoexposure_ev(fixed_ev);
    deterministic(&mut meter);
    meter.params.prep.exposure_ev = 0.;
    // Average in linear light, while retaining the original frame's AE value.
    let decoded = if pipeline.params.io.input_cctf_decoding {
        let mut image = source.clone();
        filming::decode_input_cctf(&mut image, &pipeline.params);
        Some(image)
    } else {
        None
    };
    let source = proxy(decoded.as_ref().unwrap_or(source));
    if source.data.iter().any(|v| !v.is_finite()) {
        return Err("Source has non-finite pixels for exposure placement".into());
    }
    meter.params.io.input_cctf_decoding = false;
    let mut reference = proxy(reference);
    for v in &mut reference.data {
        *v = from_f64(srgb(to_f64(*v)));
    }

    // An overexposed uniform patch measures this film/paper/finishing white,
    // including sub-digital whites that a fixed clipping threshold would miss.
    let white_source = ImageBuf::from_data(16, 16, vec![from_f64(65536.); 16 * 16 * 3]);
    let white = display_srgb(renderer.render(&meter, white_source).await?, &meter);
    let white = std::array::from_fn(|c| white.pixels().map(|p| to_f64(p[c])).fold(0., f64::max));
    let thresholds = calibration_thresholds(white)?;
    let target = distribution(
        &rgba(&reference),
        reference.width,
        reference.height,
        thresholds,
    )?;
    let mut trials = Placement { meter, source, thresholds, cache: Default::default(), renderer };
    let solution = solve_with(&target, &mut trials).await?;
    let source = trials.source;
    Ok(PlacementAnalysis {
        policy: POLICY,
        exposure_ev: solution.ev,
        requested_exposure_ev: solution.requested,
        highlight_limited: solution.limited,
        before_error_ev: solution.before,
        after_error_ev: solution.after,
        reference_samples: target.samples,
        midtone_quantiles: solution.used_quantiles,
        baseline_near_white_fraction: solution.before_white,
        reference_near_white_fraction: target.near_white_fraction,
        after_near_white_fraction: solution.after_white,
        near_white_budget: solution.budget,
        white_thresholds: thresholds,
        preview_dimensions: [source.width, source.height],
    })
}
/// The solve's trials through the selected film: the proxy at each
/// exposure, displayed and measured, each exposure once.
struct Placement<'r> {
    meter: Pipeline,
    source: ImageBuf,
    thresholds: WhiteThresholds,
    cache: std::collections::HashMap<u64, Distribution>,
    renderer: &'r mut dyn Renderer,
}
impl Trials for Placement<'_> {
    fn render(&mut self, ev: f64) -> Rendering<'_, Distribution> {
        Box::pin(async move {
            // Prep's stored value is f32; use that same value in every trial/cache.
            let ev = ev as f32;
            let key = (ev as f64).to_bits();
            if let Some(d) = self.cache.get(&key) {
                return Ok(d.clone());
            }
            self.meter.params.prep.exposure_ev = ev;
            let output = self.renderer.render(&self.meter, self.source.clone()).await?;
            let output = display_srgb(output, &self.meter);
            let d = distribution(&rgba(&output), output.width, output.height, self.thresholds)?;
            self.cache.insert(key, d.clone());
            Ok(d)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn threshold() -> WhiteThresholds {
        calibration_thresholds([1.; 3]).unwrap()
    }
    fn simple_frame(ev: f64) -> Vec<[f32; 4]> {
        (0..1024)
            .map(|i| {
                let x = (0.018 + 0.30 * (i % 32) as f64 / 31.) * ev.exp2();
                let v = srgb(x.min(1.)) as f32;
                [v, v, v, 1.]
            })
            .collect()
    }
    #[test]
    fn scalar_fit_recovers_camera_brightness_deterministically_and_without_mutation() {
        let source = simple_frame(0.);
        let original = source.clone();
        for expected in [-1.2, 0., 0.73, 1.1] {
            let target = distribution(&simple_frame(expected), 32, 32, threshold()).unwrap();
            let run = || {
                solve(&target, |ev| {
                    distribution(&simple_frame(ev), 32, 32, threshold())
                })
                .unwrap()
            };
            let a = run();
            let b = run();
            assert!((a.ev - expected).abs() <= 0.01, "{expected}: {a:?}");
            assert_eq!(a.ev, b.ev);
            assert!(!a.limited);
            assert!(a.after.abs() < 0.02);
        }
        assert_eq!(source, original);
    }
    #[test]
    fn selected_paper_white_guard_operates_below_digital_clipping() {
        let t = calibration_thresholds([0.90, 0.94, 0.86]).unwrap();
        let dim = vec![[0.75, 0.75, 0.75, 1.]; 1024];
        let white = vec![[0.89, 0.93, 0.85, 1.]; 1024];
        assert_eq!(
            distribution(&dim, 32, 32, t).unwrap().near_white_fraction,
            0.
        );
        assert_eq!(
            distribution(&white, 32, 32, t).unwrap().near_white_fraction,
            1.
        );
        assert!(white.iter().all(|p| p[..3].iter().all(|v| *v < 0.97)));
    }
    #[test]
    fn highlight_backoff_preserves_reference_budget_and_reports_residual() {
        let target = Distribution {
            quantiles: [0.2; 9],
            near_white_fraction: 0.,
            samples: 1024,
        };
        let a = solve(&target, |ev| {
            Ok(Distribution {
                quantiles: [0.1 * ev.exp2(); 9],
                near_white_fraction: (ev - 0.4).max(0.) * 0.2,
                samples: 1024,
            })
        })
        .unwrap();
        assert!((a.requested - 1.).abs() < 0.01);
        assert!(a.limited);
        assert!((a.ev - 0.45).abs() < 0.011);
        assert!(a.after_white <= a.budget);
        assert!(a.after < -0.5);
    }
    #[test]
    fn missing_midtones_unbracketed_and_nonmonotonic_responses_are_unavailable() {
        let dark = Distribution {
            quantiles: [0.0001; 9],
            near_white_fraction: 0.,
            samples: 1024,
        };
        assert!(solve(&dark, |_| Ok(dark.clone())).is_err());
        let target = Distribution {
            quantiles: [0.2; 9],
            ..dark
        };
        assert!(
            solve(&target, |_| Ok(Distribution {
                quantiles: [0.001; 9],
                ..target.clone()
            }))
            .is_err()
        );
        assert!(
            solve(&target, |ev| Ok(Distribution {
                quantiles: [0.2 * (-ev).exp2(); 9],
                ..target.clone()
            }))
            .is_err()
        );
        assert!(distribution(&[[f32::NAN; 4]; 1024], 32, 32, threshold()).is_err());
    }
    #[test]
    fn small_geometry_shifts_and_highlight_outliers_do_not_change_midtone_anchor() {
        let mut frame = simple_frame(0.7);
        let a = distribution(&frame, 32, 32, threshold()).unwrap();
        // A cyclic shift of rows preserves central distributions for this ramp.
        frame.rotate_left(32);
        let b = distribution(&frame, 32, 32, threshold()).unwrap();
        assert_eq!(a.quantiles, b.quantiles);
        let target = distribution(&frame, 32, 32, threshold()).unwrap();
        let result = solve(&target, |ev| {
            distribution(&simple_frame(ev), 32, 32, threshold())
        })
        .unwrap();
        assert!((result.ev - 0.7).abs() < 0.01);
    }
    #[test]
    fn calibrated_film_recovers_absolute_placement_with_print_compensation_and_post() {
        use crate::{params::RuntimeParams, profile};
        use spektrafilm_gpu::cpu_backend::CpuBackend;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut params = RuntimeParams::default();
        params.io.input_color_space = "sRGB".into();
        params.io.input_cctf_decoding = false;
        params.io.output_color_space = "Display P3".into();
        params.io.output_cctf_encoding = false;
        params.post.brightness = 12.;
        let pipeline = Pipeline::new_with_spectral(
            profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
            profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
            params,
            &dir,
        )
        .unwrap();
        assert!(pipeline.params.enlarger.normalize_print_exposure);
        assert!(pipeline.params.enlarger.print_exposure_compensation);
        let source = ImageBuf::from_data(
            32,
            24,
            (0..32 * 24)
                .flat_map(|i| {
                    let x = 0.015 + 0.35 * (i % 32) as f64 / 31.;
                    [x * 1.1, x, x * 0.8].map(from_f64)
                })
                .collect(),
        );
        let source_bits: Vec<_> = source.data.iter().map(|v| v.to_bits()).collect();
        let mut target_pipeline = pipeline
            .clone()
            .with_autoexposure_ev(pipeline.frame_autoexposure_ev(&source));
        deterministic(&mut target_pipeline);
        target_pipeline.params.prep.exposure_ev = 0.75;
        let mut reference = display_srgb(
            target_pipeline.process(source.clone(), &CpuBackend),
            &target_pipeline,
        );
        for v in &mut reference.data {
            *v = from_f64(inverse_srgb(to_f64(*v)));
        }
        let a = place_exposure(&pipeline, &source, &reference, &CpuBackend).unwrap();
        assert!((a.exposure_ev - 0.75).abs() <= 0.011, "{a:?}");
        assert!(!a.highlight_limited, "{a:?}");
        assert!(
            a.before_error_ev < -0.1 && a.after_error_ev.abs() < 0.02,
            "{a:?}"
        );
        // Starting from a previously placed photo must replace, never compound
        // the result; updating Prep must not alter grey-card print calibration.
        let mut changed_params = pipeline.params.clone();
        changed_params.prep.exposure_ev = 1.7;
        let changed = pipeline.clone().with_params(changed_params);
        assert_eq!(
            pipeline.print_exposure_factor(),
            changed.print_exposure_factor()
        );
        let b = place_exposure(&changed, &source, &reference, &CpuBackend).unwrap();
        assert_eq!(a.exposure_ev, b.exposure_ev);
        assert_eq!(a.before_error_ev, b.before_error_ev);
        assert_eq!(changed.params.prep.exposure_ev, 1.7);
        assert_eq!(
            source_bits,
            source.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
}
