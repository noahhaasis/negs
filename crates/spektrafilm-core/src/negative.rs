//! Printing a scan of a real negative: Negs' own steps around upstream's
//! convert-film route ([`crate::convert`]).
//!
//! - [`detect_base`]: upstream's "Detect base" (sample the clear film, fit
//!   the base tuning and the exposure), with a verdict on whether clear film
//!   was really found, or the clear film picked from a point on the rebate.
//! - [`auto_print`]: a frame's print, as a minilab's auto printer makes it:
//!   the enlarger's magenta and yellow and the print exposure, from the
//!   frame metered in zones on the film's neutral axis ([`meter`]). Written
//!   to the visible enlarger settings, never applied hidden.
//! - [`meter_print_exposure`]: the print exposure alone, metered the same
//!   way, for a roll that shares its filtration (the darkroom way).
//! - [`neutral_print`]: the magenta and yellow that print one point of the
//!   frame neutral, keeping the exposure (a grey picker).
//!
//! The image-independent balance, for the film, base and paper alone, is
//! [`crate::print_balance`], unchanged.

use std::path::Path;

use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f64, to_f64};

use crate::convert::{base_verdict, fit_base_params, levenberg_marquardt_with_step, sample_point_rgb, BaseVerdict};
use crate::params::{FilmBaseParams, RuntimeParams};
use crate::pipeline::Pipeline;
use crate::print_balance::MIDGRAY;
use crate::profile::Profile;
use crate::tone_curve::STOPS;

/// The square a click samples, as a fraction of the frame's long edge.
pub const POINT_SIZE: f64 = 0.015;
/// Without the picture's bounds, the frame's outer tenth on each side
/// isn't metered: the film holder, the rebate and light leaks live there.
pub const AUTO_MARGIN: f64 = 0.1;
/// Film this thin is clear film (the rebate), not picture.
const CLEAR_DENSITY: f64 = 0.06;

/// The longest side [`analysis_frame`] keeps: enough for a thin rebate.
pub const ANALYSIS_SIDE: u32 = 1024;

/// The whole photograph, reduced for detecting the base and the auto print.
pub fn analysis_frame(image: &ImageBuf) -> ImageBuf {
    let long = image.width.max(image.height);
    if long <= ANALYSIS_SIDE {
        return image.clone();
    }
    let scale = ANALYSIS_SIDE as f64 / long as f64;
    let size = |v: u32| ((v as f64 * scale).round() as u32).max(1);
    crate::preview::downsample_area(image, (size(image.width), size(image.height)))
}

/// The base, as [`detect_base`] found it.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedBase {
    pub base: FilmBaseParams,
    /// `film_render.convert.exposure_compensation_ev`.
    pub exposure_ev: f64,
    pub verdict: BaseVerdict,
    /// Picked from a point rather than found.
    pub picked: bool,
    /// The scan looks like a colour negative ([`looks_negative`]).
    pub looks_negative: bool,
}

/// Finds the clear film in `frame` (the whole photograph, small, still in
/// its input CCTF), or reads it at `point` (fractions of the frame), and
/// fits the base to it. `film` is the resolved film profile.
pub fn detect_base(film: &Profile, params: &RuntimeParams, frame: &ImageBuf, point: Option<(f64, f64)>) -> Result<DetectedBase, String> {
    let mut linear = frame.clone();
    crate::stages::filming::decode_input_cctf(&mut linear, params);
    let verdict = base_verdict(&linear, params.film_render.convert.base_percentile);
    let target = match point {
        Some((x, y)) => sample_point_rgb(&linear, x, y, POINT_SIZE),
        None => verdict.rgb,
    };
    if !target.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err("The clear film is black: there's no light to read the base from.".into());
    }
    // Fitted from neutral, so detecting again gives the same base.
    let mut start = params.clone();
    start.film_render.base = FilmBaseParams { active: true, cyan: 1.0, magenta: 1.0, yellow: 1.0, ..params.film_render.base.clone() };
    let (base, exposure_ev) = fit_base_params(target, film, &start)?;
    let mut fitted = start;
    fitted.film_render.base = base.clone();
    fitted.film_render.convert.exposure_compensation_ev = exposure_ev;
    let converter = crate::convert::FilmConverter::new(film, &fitted)?;
    let looks_negative = looks_negative(&converter, target, &linear);
    Ok(DetectedBase { base, exposure_ev, verdict, picked: point.is_some(), looks_negative })
}

/// The clear film is orange at least this much: blue under red (log10) ...
const ORANGE_RED_BLUE: f64 = 0.2;
/// ... and green between them by this.
const ORANGE_STEP: f64 = 0.05;
/// A pixel the read film scans back off by more than this (log10) is
/// outside the film's colours ...
const GAMUT_OFF: f64 = 0.02;
/// ... and a negative has few of those.
const GAMUT_SHARE: f64 = 0.1;

/// Negs' check that a scan looks like a colour negative: its clear film
/// (`base_rgb`, linear) is orange, red over green over blue, and the
/// picture (`linear`, the frame less its margin) reads back as film
/// density the film can hold, scanning back to what was read.
///
/// The orange shows when the scan is white-balanced for the light (or
/// daylight); a scan white-balanced on the film base itself reads as not
/// a negative.
pub fn looks_negative(converter: &crate::convert::FilmConverter, base_rgb: [f64; 3], linear: &ImageBuf) -> bool {
    let l = base_rgb.map(|v| v.max(1e-9).log10());
    let orange = l[0] - l[2] >= ORANGE_RED_BLUE && l[0] - l[1] >= ORANGE_STEP && l[1] - l[2] >= ORANGE_STEP;
    if !orange {
        return false;
    }
    let (w, h) = (linear.width, linear.height);
    let (mx, my) = ((w as f64 * AUTO_MARGIN) as u32, (h as f64 * AUTO_MARGIN) as u32);
    // About 8000 pixels, evenly.
    let step = ((((w - 2 * mx) as f64 * (h - 2 * my) as f64) / 8000.0).sqrt().floor() as u32).max(1);
    let (mut n, mut off) = (0usize, 0usize);
    for y in (my..h - my).step_by(step as usize) {
        for x in (mx..w - mx).step_by(step as usize) {
            let p = linear.get(x, y);
            let rgb = [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])];
            if !rgb.iter().all(|v| v.is_finite()) {
                continue;
            }
            let wanted = converter.pretransform(rgb);
            let back = converter.forward(converter.convert(rgb));
            let worst = (0..3).map(|c| (back[c].max(1e-9).log10() - wanted[c].max(1e-9).log10()).abs()).fold(0.0, f64::max);
            n += 1;
            off += (worst > GAMUT_OFF) as usize;
        }
    }
    n > 0 && (off as f64) <= GAMUT_SHARE * n as f64
}

/// The enlarger's magenta and yellow shifts and its print exposure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrintSettings {
    pub m_filter_shift: f64,
    pub y_filter_shift: f64,
    pub print_exposure: f64,
}

/// The pipeline the balances print with: as [`crate::print_balance`]'s,
/// the enlarger read directly, linear output with no gamut compressor, and
/// grain, glare and the crop and post edits off.
fn balance_pipeline(film: Profile, print: Profile, params: &RuntimeParams, data_dir: &Path) -> Result<Pipeline, String> {
    let mut p = params.clone();
    p.io.scan_film = false;
    p.settings.use_enlarger_lut = false;
    p.settings.use_scanner_lut = false;
    p.io.output_cctf_encoding = false;
    p.io.output_gamut_compress.algorithm = "off".into();
    p.io.output_gamut_compress.lightness_compression = None;
    p.film_render.grain.active = false;
    p.film_render.glare.active = false;
    p.print_render.glare.active = false;
    p.io.crop = false;
    p.io.photo_long_side_px = None;
    p.io.upscale_factor = 1.0;
    p.post = Default::default();
    Pipeline::new_with_spectral(film, print, p, data_dir)
}

/// `frame` read back to film density.
fn frame_density(pipeline: &Pipeline, frame: &ImageBuf) -> ImageBuf {
    let mut density = frame.clone();
    pipeline.convert_film(&mut density);
    density
}

/// The picture inside a scan, as fractions of the whole frame. A rebate
/// finder gives it; without one it's the frame less [`AUTO_MARGIN`] all
/// round, where the film holder, the rebate and light leaks live.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureBounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Default for PictureBounds {
    fn default() -> Self {
        Self { left: AUTO_MARGIN, top: AUTO_MARGIN, right: 1.0 - AUTO_MARGIN, bottom: 1.0 - AUTO_MARGIN }
    }
}

impl PictureBounds {
    /// From `[left, top, right, bottom]`, checked.
    pub fn new([left, top, right, bottom]: [f64; 4]) -> Result<Self, String> {
        let ok = [left, top, right, bottom].iter().all(|v| (0.0..=1.0).contains(v)) && left < right && top < bottom;
        if !ok {
            return Err("The picture's bounds aren't inside the photograph".into());
        }
        Ok(Self { left, top, right, bottom })
    }

    /// The pixel rows and columns of a `width` × `height` frame inside the
    /// bounds; at least one of each.
    fn pixels(&self, width: usize, height: usize) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let span = |lo: f64, hi: f64, n: usize| {
            let a = ((lo * n as f64).round() as usize).min(n.saturating_sub(1));
            let b = ((hi * n as f64).round() as usize).clamp(a + 1, n.max(a + 1));
            a..b
        };
        (span(self.left, self.right, width), span(self.top, self.bottom, height))
    }
}

/// How finely the film's greys are sampled, in stops.
const AXIS_STEP: f64 = 0.125;

/// The film's neutral axis: the density each dye takes for a grey scene,
/// from [`STOPS`]'s darkest to its brightest, as this film is filmed and
/// developed (its curves, couplers and halation). It reads a negative's
/// density back as the exposure that made it, in stops over the film's
/// mid-grey: a grey reads the same in the three layers whatever their
/// contrasts, and 0 is the grey Balance Print prints neutral.
#[derive(Debug, Clone)]
pub struct NeutralAxis {
    density: Vec<[f64; 3]>,
}

impl NeutralAxis {
    /// Films greys with `pipeline`'s film and settings, without the camera:
    /// a negative's exposure is in the film already.
    pub fn new(pipeline: &Pipeline) -> Self {
        let mut params = pipeline.params.clone();
        params.camera.auto_exposure = false;
        params.camera.exposure_compensation_ev = 0.0;
        params.prep = Default::default();
        params.io.input_cctf_decoding = false;
        params.io.crop = false;
        params.io.photo_long_side_px = None;
        params.io.upscale_factor = 1.0;
        params.film_render.grain.active = false;
        params.film_render.glare.active = false;
        let filming = pipeline.clone().with_params(params);
        let count = ((STOPS.1 - STOPS.0) / AXIS_STEP).round() as usize + 1;
        let mut density: Vec<[f64; 3]> = (0..count)
            .map(|i| {
                // Each grey its own patch: halation and the couplers' spread
                // would mix neighbouring greys in a ramp.
                let grey = from_f64(MIDGRAY * (STOPS.0 + i as f64 * AXIS_STEP).exp2());
                mean_rgb(&filming.film_scene(ImageBuf::from_data(2, 2, vec![grey; 12])))
            })
            .collect();
        // Never thinner for more light, so it inverts.
        for i in 1..count {
            for c in 0..3 {
                density[i][c] = density[i][c].max(density[i - 1][c]);
            }
        }
        Self { density }
    }

    /// The exposure, in stops, that gives each dye density `cmy` on a grey;
    /// clamped to [`STOPS`].
    pub fn stops(&self, cmy: [f64; 3]) -> [f64; 3] {
        let last = self.density.len() - 1;
        [0, 1, 2].map(|c| {
            let d = cmy[c];
            if !(d > self.density[0][c]) {
                return STOPS.0;
            }
            if d >= self.density[last][c] {
                return STOPS.1;
            }
            let i = self.density.partition_point(|row| row[c] < d).max(1);
            let (d0, d1) = (self.density[i - 1][c], self.density[i][c]);
            let t = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
            STOPS.0 + (i as f64 - 1.0 + t) * AXIS_STEP
        })
    }

    /// Each dye's density on a grey exposed `stops` (per layer) over mid-grey.
    pub fn density(&self, stops: [f64; 3]) -> [f64; 3] {
        let last = self.density.len() - 1;
        [0, 1, 2].map(|c| {
            let x = ((stops[c] - STOPS.0) / AXIS_STEP).clamp(0.0, last as f64);
            let i = (x.floor() as usize).min(last - 1);
            let t = x - i as f64;
            self.density[i][c] * (1.0 - t) + self.density[i + 1][c] * t
        })
    }

    /// `density` read as exposure, in stops per layer, inside `bounds`,
    /// leaving out clear film and film too dense to read: what the tone
    /// graph's histogram counts on a negative.
    pub fn picture_stops(&self, density: &ImageBuf, density_range: [f64; 3], bounds: &PictureBounds) -> Vec<[f32; 3]> {
        let (w, h) = (density.width as usize, density.height as usize);
        let (xs, ys) = bounds.pixels(w, h);
        let mut stops = Vec::with_capacity(xs.len() * ys.len());
        for y in ys {
            for x in xs.clone() {
                let i = (y * w + x) * 3;
                let cmy = [0, 1, 2].map(|c| to_f64(density.data[i + c]));
                if picture_density(cmy, density_range) {
                    stops.push(self.stops(cmy).map(|v| v as f32));
                }
            }
        }
        stops
    }
}

/// The tone graph's histogram on a negative: `frame` (small, in its input
/// CCTF) read back to film density by `pipeline`'s converter, and each
/// pixel's density read on the film's neutral axis as the exposure that
/// made it, in stops over mid-grey per layer, as
/// [`crate::tone_curve::raw_stops`] gives them for a photograph. The clear
/// film and the holder aren't counted. On the CPU.
pub fn film_stops(pipeline: &Pipeline, frame: &ImageBuf) -> Vec<[f32; 3]> {
    let axis = NeutralAxis::new(pipeline);
    let density = frame_density(pipeline, frame);
    let whole = PictureBounds { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };
    axis.picture_stops(&density, pipeline.film_converter().density_range(), &whole)
}

/// Film with some density and still letting light through: not the clear
/// film of the rebate and not the dark holder (read as the densest film).
fn picture_density(cmy: [f64; 3], density_range: [f64; 3]) -> bool {
    cmy.iter().all(|v| v.is_finite())
        && cmy.iter().any(|v| *v > CLEAR_DENSITY)
        && (0..3).all(|c| cmy[c] < density_range[c] - OPAQUE_MARGIN)
}

// How the auto printer meters a frame ([`meter`]): Negs' own numbers,
// chosen on round trips of known scenes (normal, a stop under, under warm
// and under green light) through Portra 400; ticket 33 §9 item 7 asks for
// more negatives before they're final.
/// Zones along the picture's long side (a scanning printer's points).
const ZONES_LONG_SIDE: usize = 24;
/// How far from grey, in stops, a zone still counts fully for the colour:
/// one this far counts 0.6, twice as far 0.14, a lawn (1.5) almost nothing.
const COLOUR_SPREAD: f64 = 0.6;
/// The share of the frame's colour departure the auto printer corrects
/// (a lowered correction level, after Bartleson and Huboi, 1956).
const COLOUR_CORRECTION: f64 = 0.75;
/// The share of the frame's exposure departure it corrects: all of it
/// would print a dark interior or a snow scene mid-grey.
const DENSITY_CORRECTION: f64 = 0.75;
/// Zones further than this, in stops, from the frame's middle tone count
/// at this distance for the exposure: a window or a deep shadow can't
/// carry the frame.
const KEY_RANGE: f64 = 2.0;
/// Density units under the densest film the converter reads.
const OPAQUE_MARGIN: f64 = 0.02;

/// What the auto printer reads in a frame, in film exposure stops over
/// the film's mid-grey ([`NeutralAxis`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metered {
    /// The frame's middle tone.
    pub key: f64,
    /// Its colour away from grey, per layer (summing to 0).
    pub colour: [f64; 3],
    /// The zones read.
    pub zones: usize,
}

/// Meters `density` (a frame read back to film density) inside `bounds`.
///
/// After what minilab auto printers do, Negs' own sums: the picture is
/// measured in zones (a scanning printer's measuring points), each zone's
/// light added up as the printer's cells would (large-area transmission
/// density), and each zone read on the film's [`NeutralAxis`] as the
/// exposure that made it. The frame's middle tone is the mean of the
/// zones' exposures (a log average), with zones far from it held at
/// [`KEY_RANGE`]: a sky or a deep shadow weighs no more than that. A
/// zone's tone is its luminance. The colour is the zones' mean colour,
/// each zone weighed by how near grey it is ([`COLOUR_SPREAD`]), as Fuji's
/// printers weigh saturated points down (US 4,641,959): strongly coloured
/// subjects, a lawn or a blue sky, hardly move it (subject-failure
/// suppression), while a cast from the light, which shifts every zone,
/// still does. On the neutral axis a cast is the same shift in every zone
/// whatever its density, which it isn't in density.
pub fn meter(axis: &NeutralAxis, density: &ImageBuf, density_range: [f64; 3], bounds: &PictureBounds) -> Result<Metered, String> {
    let (w, h) = (density.width as usize, density.height as usize);
    let (xs, ys) = bounds.pixels(w, h);
    let side = (xs.len().max(ys.len()) / ZONES_LONG_SIDE).max(1);
    let (nx, ny) = (xs.len().div_ceil(side), ys.len().div_ceil(side));
    let mut light = vec![([0.0f64; 3], 0usize, 0usize); nx * ny];
    for y in ys.clone() {
        for x in xs.clone() {
            let i = (y * w + x) * 3;
            let cmy = [0, 1, 2].map(|c| to_f64(density.data[i + c]));
            let zone = &mut light[((y - ys.start) / side) * nx + (x - xs.start) / side];
            zone.2 += 1;
            if picture_density(cmy, density_range) {
                for c in 0..3 {
                    zone.0[c] += 10f64.powf(-cmy[c]);
                }
                zone.1 += 1;
            }
        }
    }
    // A zone half clear film or holder is left out.
    let zones: Vec<[f64; 3]> = light
        .iter()
        .filter(|(_, n, all)| *n > 0 && 2 * *n >= *all)
        .map(|(t, n, _)| axis.stops(t.map(|t| -(t / *n as f64).log10())))
        .collect();
    if zones.is_empty() {
        return Err("There's no picture to meter: the frame is clear film or black.".into());
    }
    // A zone's tone is its brightness, the exposures added up as light
    // weighed as the eye weighs them (Rec. 709 luminance): a green lawn is
    // as bright as it looks, not as dark as its red and blue.
    let tone = |s: &[f64; 3]| luminance(s[0].exp2(), s[1].exp2(), s[2].exp2()).log2();
    let mut tones: Vec<f64> = zones.iter().map(tone).collect();
    let mid = tones.len() / 2;
    let middle = *tones.select_nth_unstable_by(mid, f64::total_cmp).1;
    let key = zones.iter().map(|s| tone(s).clamp(middle - KEY_RANGE, middle + KEY_RANGE)).sum::<f64>() / zones.len() as f64;
    let (mut colour, mut weight) = ([0.0; 3], 0.0);
    for s in &zones {
        let grey = (s[0] + s[1] + s[2]) / 3.0;
        let off = s.map(|v| v - grey);
        let distance = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
        let wz = (-0.5 * (distance / COLOUR_SPREAD).powi(2)).exp();
        for c in 0..3 {
            colour[c] += wz * off[c];
        }
        weight += wz;
    }
    let colour = colour.map(|v| if weight > 0.0 { v / weight } else { 0.0 });
    Ok(Metered { key, colour, zones: zones.len() })
}

/// The neutral axis and the frame's density for `pipeline`.
fn read_frame(pipeline: &Pipeline, frame: &ImageBuf) -> (NeutralAxis, ImageBuf, [f64; 3]) {
    (NeutralAxis::new(pipeline), frame_density(pipeline, frame), pipeline.film_converter().density_range())
}

fn patch(cmy: [f64; 3]) -> ImageBuf {
    ImageBuf::from_data(2, 2, [cmy; 4].iter().flatten().map(|v| from_f64(*v)).collect())
}

fn mean_rgb(image: &ImageBuf) -> [f64; 3] {
    let mut acc = [0.0; 3];
    for p in image.data.chunks_exact(3) {
        for c in 0..3 {
            acc[c] += to_f64(p[c]);
        }
    }
    let n = (image.data.len() / 3).max(1) as f64;
    acc.map(|v| v / n)
}

/// The grey the auto printer prints as mid-grey for `metered`: on the
/// film's neutral axis, moved by the corrected share of the frame's
/// exposure and colour.
fn printed_grey(axis: &NeutralAxis, metered: &Metered, colour: bool) -> [f64; 3] {
    let k = if colour { COLOUR_CORRECTION } else { 0.0 };
    axis.density([0, 1, 2].map(|c| DENSITY_CORRECTION * metered.key + k * metered.colour[c]))
}

/// A minilab's auto print of `frame` (the whole photograph, small, in its
/// input CCTF), metered inside `bounds` ([`meter`]): the magenta, yellow
/// and print exposure that print its metered grey as a neutral mid-grey.
/// Like [`neutral_balance`], it always starts from no filtration, so the
/// same frame on the same paper gets the same print whatever the enlarger
/// was set to: a new paper runs it again (the app), and a frame's history
/// of papers and filters doesn't follow it.
pub fn auto_print(
    film: Profile,
    print: Profile,
    params: &RuntimeParams,
    data_dir: &Path,
    frame: &ImageBuf,
    bounds: &PictureBounds,
) -> Result<PrintSettings, String> {
    let mut pipeline = balance_pipeline(film, print, params, data_dir)?;
    let (axis, density, range) = read_frame(&pipeline, frame);
    let metered = meter(&axis, &density, range, bounds)?;
    let target = patch(printed_grey(&axis, &metered, true));
    Ok(print_grey(&mut pipeline, target, [0.0, 0.0, 1.0]))
}

/// The negative's neutral print: the magenta, yellow and print exposure
/// that print the film's own mid-grey (0 on its [`NeutralAxis`]) as a
/// neutral mid-grey, for this film, base, reading and paper. Unlike
/// [`crate::print_balance`]'s grey card, which keeps the print exposure, the
/// exposure is found with the filtration, so the grey never sits in the
/// paper's toe or shoulder, where the paper's own white or black passes for
/// neutral at almost any filtration. It always starts from no filtration:
/// the answer doesn't depend on the frame's enlarger settings.
pub fn neutral_balance(film: Profile, print: Profile, params: &RuntimeParams, data_dir: &Path) -> Result<PrintSettings, String> {
    let mut pipeline = balance_pipeline(film, print, params, data_dir)?;
    let target = patch(NeutralAxis::new(&pipeline).density([0.0; 3]));
    Ok(print_grey(&mut pipeline, target, [0.0, 0.0, 1.0]))
}

/// The magenta, yellow and print exposure that print film density
/// `target` as a neutral mid-grey, starting from `x0` (magenta, yellow,
/// exposure).
fn print_grey(pipeline: &mut Pipeline, target: ImageBuf, x0: [f64; 3]) -> PrintSettings {
    let x0 = vec![x0[0], x0[1], x0[2].max(1e-3).log2()];
    let x = levenberg_marquardt_with_step(
        |x| {
            pipeline.set_print_filter_shifts(x[0], x[1]);
            pipeline.params.enlarger.print_exposure = x[2].exp2() as f32;
            let [r, g, b] = mean_rgb(&pipeline.print_film_density(target.clone()));
            let g = g.max(1e-6);
            vec![(r - g) / g, (b - g) / g, (g / MIDGRAY).log2()]
        },
        x0,
        1e-6,
        50,
        Some(0.05),
    );
    PrintSettings { m_filter_shift: x[0], y_filter_shift: x[1], print_exposure: x[2].exp2() }
}

/// The print exposure for `frame`, metered inside `bounds` as
/// [`auto_print`] meters it, keeping the enlarger's filtration: a darkroom
/// roll shares its filtration, and each frame gets its own exposure.
pub fn meter_print_exposure(
    film: Profile,
    print: Profile,
    params: &RuntimeParams,
    data_dir: &Path,
    frame: &ImageBuf,
    bounds: &PictureBounds,
) -> Result<f64, String> {
    let mut pipeline = balance_pipeline(film, print, params, data_dir)?;
    let (axis, density, range) = read_frame(&pipeline, frame);
    let metered = meter(&axis, &density, range, bounds)?;
    let target = patch(printed_grey(&axis, &metered, false));
    let x0 = vec![(params.enlarger.print_exposure as f64).max(1e-3).log2()];
    let x = levenberg_marquardt_with_step(
        |x| {
            pipeline.params.enlarger.print_exposure = x[0].exp2() as f32;
            let [r, g, b] = mean_rgb(&pipeline.print_film_density(target.clone()));
            vec![(luminance(r, g, b) / MIDGRAY).max(1e-9).log2()]
        },
        x0,
        1e-6,
        50,
        Some(0.05),
    );
    Ok(x[0].exp2())
}

/// Rec. 709 luminance of linear RGB.
fn luminance(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The magenta and yellow that print the film at `point` (fractions of
/// `frame`) neutral, keeping the print exposure: something that should be grey.
pub fn neutral_print(
    film: Profile,
    print: Profile,
    params: &RuntimeParams,
    data_dir: &Path,
    frame: &ImageBuf,
    point: (f64, f64),
) -> Result<PrintSettings, String> {
    let mut pipeline = balance_pipeline(film, print, params, data_dir)?;
    let density = frame_density(&pipeline, frame);
    let target = patch(sample_point_rgb(&density, point.0, point.1, POINT_SIZE));
    let x0 = vec![params.enlarger.m_filter_shift as f64, params.enlarger.y_filter_shift as f64];
    let x = levenberg_marquardt_with_step(
        |x| {
            pipeline.set_print_filter_shifts(x[0], x[1]);
            let [r, g, b] = mean_rgb(&pipeline.print_film_density(target.clone()));
            let g = g.max(1e-6);
            vec![(r - g) / g, (b - g) / g]
        },
        x0,
        1e-6,
        50,
        Some(0.05),
    );
    Ok(PrintSettings { m_filter_shift: x[0], y_filter_shift: x[1], print_exposure: params.enlarger.print_exposure as f64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ROUTE_CONVERT_PRINT;

    fn data() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }
    fn load(name: &str) -> Profile {
        crate::profile::load_profile_by_name(&data(), name).unwrap()
    }

    /// A synthetic scan: Portra 400 density through the scan model, with a
    /// strip of clear film down the left.
    fn scanned_negative(params: &RuntimeParams) -> ImageBuf {
        let film = load("kodak_portra_400");
        let converter = crate::convert::FilmConverter::new(&film, params).unwrap();
        let (w, h) = (80u32, 50u32);
        let mut image = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let cmy = if x < 5 {
                    [0.0; 3]
                } else {
                    let t = x as f64 / w as f64;
                    [0.5 + 0.6 * t, 0.7 + 0.3 * (y as f64 / h as f64), 0.9 - 0.2 * t]
                };
                let rgb = converter.forward(cmy).map(|v| v * 0.6);
                let i = ((y * w + x) * 3) as usize;
                for c in 0..3 {
                    image.data[i + c] = from_f64(rgb[c]);
                }
            }
        }
        image
    }

    fn linear_params() -> RuntimeParams {
        let mut params = RuntimeParams::default();
        params.workflow.route = ROUTE_CONVERT_PRINT.into();
        params.io.input_cctf_decoding = false;
        params
    }

    #[test]
    fn detecting_finds_the_strip_and_reads_it_as_clear_film() {
        let params = linear_params();
        let frame = scanned_negative(&params);
        let film = load("kodak_portra_400");
        let found = detect_base(&film, &params, &frame, None).unwrap();
        assert!(found.verdict.found, "{:?}", found.verdict);
        assert_eq!(found.verdict.edge, Some(crate::convert::Edge::Left));
        // The scan was 0.6 of the model's light: about +0.74 EV.
        assert!((found.exposure_ev - (1.0f64 / 0.6).log2()).abs() < 0.02, "{}", found.exposure_ev);
        let picked = detect_base(&film, &params, &frame, Some((0.02, 0.5))).unwrap();
        assert!(picked.picked);
        assert!((picked.exposure_ev - found.exposure_ev).abs() < 0.02);
        // The strip bounds the picture on the left, cut a little inside.
        let picture = found.verdict.picture.expect("picture");
        assert_eq!(picture.edges, vec![crate::convert::Edge::Left]);
        assert!((picture.x0 - (5.0 / 80.0 + crate::film_edge::INSET)).abs() < 0.003, "{picture:?}");
    }

    #[test]
    fn a_scanned_negative_looks_like_one_and_a_photograph_does_not() {
        let params = linear_params();
        let film = load("kodak_portra_400");
        assert!(detect_base(&film, &params, &scanned_negative(&params), None).unwrap().looks_negative);
        // A photograph: white highlights over coloured, darker parts.
        let (w, h) = (80u32, 50u32);
        let mut photo = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (t, s) = (x as f64 / w as f64, y as f64 / h as f64);
                let rgb = if t > 0.9 { [0.95, 0.95, 0.95] } else { [0.05 + 0.6 * t, 0.1 + 0.5 * s, 0.4 - 0.3 * t * s] };
                let i = ((y * w + x) * 3) as usize;
                for c in 0..3 {
                    photo.data[i + c] = from_f64(rgb[c]);
                }
            }
        }
        assert!(!detect_base(&film, &params, &photo, None).unwrap().looks_negative);
    }

    fn portra(params: &RuntimeParams) -> Pipeline {
        balance_pipeline(load("kodak_portra_400"), load("kodak_portra_endura"), params, &data()).unwrap()
    }

    /// A scene (linear RGB per pixel) filmed on Portra 400 and scanned back
    /// through the scan model: a negative whose true scene is known. Clear
    /// film down the left fifth when `rebate`.
    fn negative_of(params: &RuntimeParams, w: u32, h: u32, rebate: bool, scene: impl Fn(u32, u32) -> [f64; 3]) -> ImageBuf {
        let pipeline = portra(params);
        let mut rgb = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                let v = scene(x, y);
                for c in 0..3 {
                    rgb.data[i + c] = from_f64(v[c]);
                }
            }
        }
        let density = pipeline.film_scene(rgb);
        let converter = pipeline.film_converter();
        let mut image = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                let cmy = if rebate && x < w / 5 { [0.0; 3] } else { [0, 1, 2].map(|c| to_f64(density.data[i + c])) };
                let scan = converter.forward(cmy);
                for c in 0..3 {
                    image.data[i + c] = from_f64(scan[c]);
                }
            }
        }
        image
    }

    /// Greys from a stop under to a stop over `stops`, in a checkerboard.
    fn greys(stops: f64) -> impl Fn(u32, u32) -> [f64; 3] {
        move |x, y| [MIDGRAY * (stops + [-1.0, 0.0, 1.0][((x / 4 + y / 4) % 3) as usize]).exp2(); 3]
    }

    fn auto(params: &RuntimeParams, frame: &ImageBuf) -> PrintSettings {
        auto_print(load("kodak_portra_400"), load("kodak_portra_endura"), params, &data(), frame, &PictureBounds::default()).unwrap()
    }

    /// The print's RGB of film density `cmy` with `settings`.
    fn printed(params: &RuntimeParams, settings: &PrintSettings, cmy: [f64; 3]) -> [f64; 3] {
        let mut params = params.clone();
        params.enlarger.m_filter_shift = settings.m_filter_shift as f32;
        params.enlarger.y_filter_shift = settings.y_filter_shift as f32;
        params.enlarger.print_exposure = settings.print_exposure as f32;
        mean_rgb(&portra(&params).print_film_density(patch(cmy)))
    }

    #[test]
    fn the_neutral_axis_reads_a_grey_back_as_its_exposure() {
        let params = linear_params();
        let pipeline = portra(&params);
        let axis = NeutralAxis::new(&pipeline);
        for stops in [-3.0, -1.3, 0.0, 0.7, 2.5] {
            let grey = from_f64(MIDGRAY * f64::exp2(stops));
            let density = mean_rgb(&pipeline.film_scene(ImageBuf::from_data(2, 2, vec![grey; 12])));
            let read = axis.stops(density);
            assert!(read.iter().all(|s| (s - stops).abs() < 0.01), "{stops}: {read:?}");
            let back = axis.density([stops; 3]);
            assert!((0..3).all(|c| (back[c] - density[c]).abs() < 0.005), "{back:?} {density:?}");
        }
        assert_eq!(axis.stops([0.0; 3]), [STOPS.0; 3]);
        assert_eq!(axis.stops([10.0; 3]), [STOPS.1; 3]);
    }

    #[test]
    fn auto_leaves_a_normal_frame_as_the_balanced_print_prints_it() {
        let mut params = linear_params();
        let (m, y) = crate::print_balance::solve_neutral_filter_shifts(
            load("kodak_portra_400"),
            load("kodak_portra_endura"),
            &params,
            &data(),
            MIDGRAY,
        )
        .unwrap();
        params.enlarger.m_filter_shift = m as f32;
        params.enlarger.y_filter_shift = y as f32;
        let frame = negative_of(&params, 60, 40, false, greys(0.0));
        let settings = auto(&params, &frame);
        assert!((settings.m_filter_shift - m).abs() < 0.3 && (settings.y_filter_shift - y).abs() < 0.3, "{settings:?} {m} {y}");
        assert!((settings.print_exposure - 1.0).abs() < 0.03, "{settings:?}");
    }

    #[test]
    fn a_lawn_doesnt_turn_auto_magenta() {
        // A green lawn with a grey card: a printer integrating to grey
        // prints the lawn grey and the card magenta (subject failure).
        let params = linear_params();
        let neutral = auto(&params, &negative_of(&params, 60, 40, false, greys(0.0)));
        let lawn = negative_of(&params, 60, 40, false, |x, y| {
            if (10..20).contains(&x) && (10..20).contains(&y) {
                [MIDGRAY; 3]
            } else {
                let b = [0.7, 1.0, 1.4][((x / 3 + y / 2) % 3) as usize];
                [0.06 * b, 0.16 * b, 0.035 * b]
            }
        });
        let settings = auto(&params, &lawn);
        let card = printed(&params, &settings, NeutralAxis::new(&portra(&params)).density([0.0; 3]));
        let cast = [(card[0] / card[1]).log2(), (card[2] / card[1]).log2()];
        assert!(cast.iter().all(|c| c.abs() < 0.1), "the grey card prints {cast:?} with {settings:?}");
        assert!((settings.print_exposure / neutral.print_exposure).log2().abs() < 0.35, "{settings:?} {neutral:?}");
    }

    #[test]
    fn auto_corrects_most_of_a_cast_from_the_light() {
        let params = linear_params();
        let neutral = auto(&params, &negative_of(&params, 60, 40, false, greys(0.0)));
        let warm = |x, y| {
            let [r, g, b] = greys(0.0)(x, y);
            [r * 1.4, g, b * 0.7]
        };
        let settings = auto(&params, &negative_of(&params, 60, 40, false, warm));
        // A grey card under the warm light, printed as balanced and by Auto.
        let warm_card = [MIDGRAY * 1.4, MIDGRAY, MIDGRAY * 0.7].map(from_f64);
        let card = mean_rgb(&portra(&params).film_scene(ImageBuf::from_data(2, 2, warm_card.repeat(4))));
        let cast = |rgb: [f64; 3]| [(rgb[0] / rgb[1]).log2(), (rgb[2] / rgb[1]).log2()];
        let before = cast(printed(&params, &neutral, card));
        let after = cast(printed(&params, &settings, card));
        assert!(before[0] > 0.2 && before[1] < -0.2, "{before:?}");
        for c in 0..2 {
            assert!(after[c].abs() < 0.5 * before[c].abs(), "{before:?} → {after:?}");
        }
    }

    const PAPERS: [&str; 5] =
        ["kodak_portra_endura", "kodak_endura_premier", "kodak_ultra_endura", "kodak_supra_endura", "fujifilm_crystal_archive_typeii"];

    /// The print's RGB of film density `cmy` on `paper` with `settings`.
    fn printed_on(paper: &str, params: &RuntimeParams, settings: &PrintSettings, cmy: [f64; 3]) -> [f64; 3] {
        let mut params = params.clone();
        params.enlarger.m_filter_shift = settings.m_filter_shift as f32;
        params.enlarger.y_filter_shift = settings.y_filter_shift as f32;
        params.enlarger.print_exposure = settings.print_exposure as f32;
        let pipeline = balance_pipeline(load("kodak_portra_400"), load(paper), &params, &data()).unwrap();
        mean_rgb(&pipeline.print_film_density(patch(cmy)))
    }

    #[test]
    fn the_neutral_balance_prints_the_films_grey_neutral_whatever_the_enlarger_was() {
        // A frame's own exposure can put the grey card of a plain print
        // balance in the paper's shoulder, where its white passes for
        // neutral: there the solve wandered to −55 M −81 Y on Endura Premier.
        let params = linear_params();
        let grey = NeutralAxis::new(&portra(&params)).density([0.0; 3]);
        for paper in PAPERS {
            let neutral = neutral_balance(load("kodak_portra_400"), load(paper), &params, &data()).unwrap();
            let rgb = printed_on(paper, &params, &neutral, grey);
            let cast = [(rgb[0] / rgb[1]).log2(), (rgb[2] / rgb[1]).log2()];
            assert!(cast.iter().all(|c| c.abs() < 0.005), "{paper}: {cast:?} with {neutral:?}");
            assert!(((rgb[1] / MIDGRAY).log2()).abs() < 0.01, "{paper}: {rgb:?}");
            assert!(neutral.m_filter_shift.abs() < 20.0 && neutral.y_filter_shift.abs() < 20.0, "{paper}: {neutral:?}");
            for (m, y, exposure) in [(-60.0, -87.0, 0.51), (84.0, -44.0, 0.64), (0.0, 0.0, 3.0)] {
                let mut from = params.clone();
                (from.enlarger.m_filter_shift, from.enlarger.y_filter_shift, from.enlarger.print_exposure) = (m, y, exposure);
                let again = neutral_balance(load("kodak_portra_400"), load(paper), &from, &data()).unwrap();
                assert_eq!(again, neutral, "{paper} from {m} {y} {exposure}");
            }
        }
    }

    #[test]
    fn auto_prints_a_frame_alike_on_each_paper_whatever_the_enlarger_was() {
        // A new paper runs Auto again: from the last paper's filtration,
        // or a bad one (−60 M −87 Y), it must find the same print, and
        // paper after paper must come back to where it was.
        let params = linear_params();
        let warm = |x, y| {
            let [r, g, b] = greys(0.3)(x, y);
            [r * 1.2, g, b * 0.85]
        };
        let frame = negative_of(&params, 60, 40, false, warm);
        let auto_on = |paper: &str, params: &RuntimeParams| {
            auto_print(load("kodak_portra_400"), load(paper), params, &data(), &frame, &PictureBounds::default()).unwrap()
        };
        for paper in PAPERS {
            let own = auto_on(paper, &params);
            assert!(own.m_filter_shift.abs() < 40.0 && own.y_filter_shift.abs() < 40.0, "{paper}: {own:?}");
            for (m, y, exposure) in [(-60.0, -87.0, 0.51), (84.0, -44.0, 0.64), (0.0, 0.0, 3.0)] {
                let mut from = params.clone();
                (from.enlarger.m_filter_shift, from.enlarger.y_filter_shift, from.enlarger.print_exposure) = (m, y, exposure);
                assert_eq!(auto_on(paper, &from), own, "{paper} from {m} {y} {exposure}");
            }
        }
    }

    #[test]
    fn metering_follows_the_frame_inside_its_bounds() {
        let mut params = linear_params();
        params.enlarger.m_filter_shift = 3.0;
        params.enlarger.y_filter_shift = -2.0;
        let meter = |frame: &ImageBuf, bounds: &PictureBounds| {
            meter_print_exposure(load("kodak_portra_400"), load("kodak_portra_endura"), &params, &data(), frame, bounds).unwrap()
        };
        let at = |frame: &ImageBuf| meter(frame, &PictureBounds::default());
        let normal = negative_of(&params, 60, 40, true, greys(0.0));
        let over = negative_of(&params, 60, 40, true, greys(1.0));
        // A stop over: a denser negative, more print exposure, by the
        // corrected share of the stop as the paper sees it.
        let ratio = (at(&over) / at(&normal)).log2();
        assert!(ratio > 0.4 && ratio < 1.2, "{ratio}");
        // Auto meters the same exposure when the frame has no colour.
        let auto = auto(&params, &normal);
        assert!((auto.print_exposure / at(&normal) - 1.0).abs() < 0.03, "{auto:?} {}", at(&normal));
        // Outside the bounds doesn't count: a bright left half is left out.
        let half = negative_of(&params, 60, 40, false, |x, y| greys(if x < 30 { 3.0 } else { 0.0 })(x, y));
        let plain = negative_of(&params, 60, 40, false, greys(0.0));
        let right = PictureBounds::new([0.55, 0.1, 0.95, 0.9]).unwrap();
        assert!((meter(&half, &right) / meter(&plain, &right) - 1.0).abs() < 1e-6);
        assert!(at(&half) > 1.2 * at(&plain));
        assert!(PictureBounds::new([0.6, 0.1, 0.4, 0.9]).is_err());
        assert!(PictureBounds::new([0.0, 0.0, 1.1, 1.0]).is_err());
    }

    #[test]
    fn the_histogram_reads_a_negative_as_the_exposure_that_made_it() {
        let params = linear_params();
        let pipeline = portra(&params);
        // Greys from three stops under to three over, left to right, after
        // a strip of clear film.
        let (w, h) = (70u32, 10u32);
        let scene_stops = |x: u32| -3.0 + 6.0 * (x.max(w / 5) - w / 5) as f64 / (w - w / 5 - 1) as f64;
        let negative = negative_of(&params, w, h, true, |x, _| [MIDGRAY * scene_stops(x).exp2(); 3]);
        let stops = film_stops(&pipeline, &negative);
        assert_eq!(stops.len(), ((w - w / 5) * h) as usize, "the clear film isn't counted");
        for (i, s) in stops.iter().enumerate() {
            let want = scene_stops(w / 5 + i as u32 % (w - w / 5));
            assert!(s.iter().all(|v| (*v as f64 - want).abs() < 0.02), "{i}: {s:?} want {want}");
        }
    }
}
