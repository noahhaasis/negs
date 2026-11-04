//! The tone graph's data (ticket 20, prototype): a ramp of greys through
//! film → print → scan, with the film's and the paper's exposure per grey,
//! and the photograph's film exposure as a histogram.
//!
//! Adapted from SpektraMac's `ToneGraph` / `toneGraph(controls:)` (GPL-3.0).
use serde::Serialize;
use spektrafilm_math::{image::ImageBuf, precision::from_f64};

use crate::{params::RuntimeParams, pipeline::Pipeline};

/// The scene's mid-grey, at 0 stops.
pub const MID_GREY: f64 = 0.184;
/// The film exposures the ramp covers, in stops over mid-grey.
pub const STOPS: (f64, f64) = (-8.0, 8.0);
const LOG10_2: f64 = std::f64::consts::LOG10_2;

/// The render's settings for a ramp of greys: per pixel like the LUT
/// export, but keeping what changes a print's tones (the enlarger's filter
/// shifts and preflash, the scanner corrections) and the acceleration LUTs
/// the render uses, so the curve is the render's.
pub fn tone_params(params: &RuntimeParams) -> RuntimeParams {
    let mut tone = crate::lut_export::per_pixel_params(params.clone());
    tone.enlarger.y_filter_shift = params.enlarger.y_filter_shift;
    tone.enlarger.m_filter_shift = params.enlarger.m_filter_shift;
    tone.enlarger.preflash_exposure = params.enlarger.preflash_exposure;
    tone.enlarger.preflash_y_filter_shift = params.enlarger.preflash_y_filter_shift;
    tone.enlarger.preflash_m_filter_shift = params.enlarger.preflash_m_filter_shift;
    tone.scanner.white_correction = params.scanner.white_correction;
    tone.scanner.black_correction = params.scanner.black_correction;
    tone.settings.use_enlarger_lut = params.settings.use_enlarger_lut;
    tone.settings.use_scanner_lut = params.settings.use_scanner_lut;
    // Linear greys in, display-encoded sRGB out (greys are the same in the
    // other output spaces with an sRGB-like curve).
    tone.io.input_color_space = "sRGB".into();
    tone.io.input_cctf_decoding = false;
    tone.io.output_color_space = "sRGB".into();
    tone.io.output_cctf_encoding = true;
    tone
}

#[derive(Serialize)]
pub struct ToneCurve {
    /// Film exposure per grey, in stops over mid-grey, per layer.
    pub film: Vec<[f64; 3]>,
    /// The paper's log10 exposure per grey, per layer; 0 is a mid-grey
    /// negative at print exposure 1. `None` when scanning the film directly.
    pub paper: Option<Vec<[f64; 3]>>,
    /// The scan, display-encoded sRGB, unclipped.
    pub rgb: Vec<[f64; 3]>,
}

fn triples(image: &ImageBuf, scale: f64) -> Vec<[f64; 3]> {
    image.pixels().map(|p| [0, 1, 2].map(|c| p[c] as f64 * scale)).collect()
}

/// `count` greys, evenly spaced in film exposure over [`STOPS`], through
/// `pipeline` (built with [`tone_params`]).
pub fn tone_curve(pipeline: &Pipeline, count: usize) -> ToneCurve {
    let count = count.clamp(2, 4096);
    // Camera exposure compensation scales the film's exposure; the ramp
    // undoes it, so it spans the same film exposures at any compensation.
    // (It stays in the params: the enlarger's exposure compensation reads it.)
    let ev = pipeline.params.camera.exposure_compensation_ev as f64;
    let mut ramp = ImageBuf::new(count as u32, 1);
    for (i, pixel) in ramp.pixels_mut().enumerate() {
        let stops = STOPS.0 + (STOPS.1 - STOPS.0) * i as f64 / (count - 1) as f64;
        let grey = from_f64(MID_GREY * (stops - ev).exp2());
        pixel[..3].fill(grey);
    }
    let (film, paper, rgb) = pipeline.process_taps(ramp);
    ToneCurve {
        film: triples(&film, 1.0 / LOG10_2),
        paper: paper.as_ref().map(|paper| triples(paper, 1.0)),
        rgb: triples(&rgb, 1.0),
    }
}

/// The film raw exposure of a metering frame (`Pipeline::metering_film_raw`)
/// as stops, before the exposure scale.
pub fn raw_stops(raw: &ImageBuf) -> Vec<[f32; 3]> {
    raw.pixels()
        .map(|p| [0, 1, 2].map(|c| ((p[c] as f64).max(1e-10).log2()) as f32))
        .collect()
}

/// Counts film exposures (`raw_stops` shifted by `ev`) in `bins` bins over
/// [`STOPS`], per layer; the ends take what's beyond them.
pub fn film_histogram(stops: &[[f32; 3]], ev: f64, bins: usize) -> [Vec<u32>; 3] {
    let mut counts = [vec![0u32; bins], vec![0u32; bins], vec![0u32; bins]];
    let scale = bins as f64 / (STOPS.1 - STOPS.0);
    for pixel in stops {
        for c in 0..3 {
            let bin = ((pixel[c] as f64 + ev - STOPS.0) * scale).floor();
            counts[c][bin.clamp(0.0, bins as f64 - 1.0) as usize] += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile;

    fn data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn curve(params: RuntimeParams) -> ToneCurve {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
        let pipeline = Pipeline::new_with_spectral(film, print, tone_params(&params), &dir).unwrap();
        tone_curve(&pipeline, 65)
    }

    #[test]
    fn mid_grey_is_at_zero_and_brighter_scenes_print_brighter() {
        let curve = curve(RuntimeParams::default());
        let mid = 32;
        for c in 0..3 {
            assert!(curve.film[mid][c].abs() < 0.05, "film {:?}", curve.film[mid]);
            assert!(curve.paper.as_ref().unwrap()[mid][c].abs() < 0.05, "paper {:?}", curve.paper.as_ref().unwrap()[mid]);
        }
        let luma = |rgb: [f64; 3]| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        assert!(luma(curve.rgb[20]) < luma(curve.rgb[32]) && luma(curve.rgb[32]) < luma(curve.rgb[44]));
        // Brighter scene: a denser negative, less light on the paper.
        assert!(curve.paper.as_ref().unwrap()[44][1] < curve.paper.as_ref().unwrap()[20][1]);
    }

    #[test]
    fn print_exposure_shifts_the_paper_exposure_only() {
        let base = curve(RuntimeParams::default());
        let mut params = RuntimeParams::default();
        params.enlarger.print_exposure = 2.0;
        let brighter = curve(params);
        for (a, b) in base.paper.unwrap().iter().zip(brighter.paper.unwrap()) {
            for c in 0..3 {
                assert!((b[c] - a[c] - 2f64.log10()).abs() < 1e-3, "{a:?} {b:?}");
            }
        }
    }
}
