//! Post adjustments: finishing edits on the scanned photograph, as an editor
//! would make them to a lab scan. They run on the scan clipped to [0, 1]
//! (`pipeline::clip_to_display`), as an integer file holds it, so they shape
//! the tones the film produced but cannot recover a clipped highlight; that
//! belongs to camera and print exposure.
//!
//! Pixels are decoded with the output space's own transfer function and
//! luminance is that space's Y, so the same settings make the same change in
//! every output space. Tones are shaped on that luminance in sRGB-encoded
//! (perceptual) space, and each pixel keeps its colour ratios, so contrast
//! doesn't shift the film's hues.
//!
//! The arithmetic is f32 even in the f64 build: the tables already limit
//! accuracy to about 2e-7, far below a 16-bit output step, and the f64
//! build's purpose, parity with the Python reference, doesn't cover Post.
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::colorspace::RgbColourspace;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};

/// Each control runs from -100 to 100; 0 leaves the photograph unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PostParams {
    /// Lifts or darkens the midtones, keeping black and white.
    #[serde(default)]
    pub brightness: f32,
    /// An S-curve about middle grey.
    #[serde(default)]
    pub contrast: f32,
    /// Moves the white point: up clips the brightest tones to white, down dims white.
    #[serde(default)]
    pub whites: f32,
    /// Moves the black point: down crushes the darkest tones, up lifts black.
    #[serde(default)]
    pub blacks: f32,
    /// Scales colourfulness at constant luminance; -100 is monochrome.
    #[serde(default)]
    pub saturation: f32,
    /// Tones this print pins, as L\* after the knobs above → printed L\*: a
    /// curve that can set the black and white points (see [`AnchorCurve`]).
    #[serde(default)]
    pub tone_anchors: Vec<ToneAnchor>,
}

/// One pin of the tone curve, in CIE Lab lightness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToneAnchor {
    /// The tone as [`measured_lstar`] read it, L\* 0…100.
    pub input: f32,
    /// Where it should print, L\* 0…100.
    pub target: f32,
    /// Where the tone was measured, for the app to outline; the pipeline
    /// never reads it.
    #[serde(default)]
    pub source: Option<AnchorSource>,
}

/// The circle on the uncropped photograph that a pin averaged.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnchorSource {
    /// The circle's centre, in fractions of the width and height, from the
    /// top left.
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Its radius, as a fraction of the photograph's shorter side.
    #[serde(default)]
    pub radius: f32,
}

impl PostParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// sRGB-encoded middle grey (18% reflectance), the contrast pivot.
const PIVOT: f64 = 0.461_356;
/// How far Whites and Blacks at ±100 move their end point.
const LEVELS_RANGE: f64 = 0.15;
const LUT_SIZE: usize = 4096;

/// Luminance weights of an output colour space: the Y row of its RGB → XYZ
/// matrix. Unknown names fall back to sRGB, as the scan's output does.
pub(crate) fn luminance_weights(color_space: &str) -> [f32; 3] {
    RgbColourspace::output(color_space).rgb_to_xyz[1].map(|v| v as f32)
}

fn srgb_encode(x: f64) -> f64 {
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(x: f64) -> f64 {
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// An output space's transfer function, with sRGB's exactly as before.
struct Transfer(&'static RgbColourspace);

impl Transfer {
    fn decode(&self, v: f64) -> f64 {
        if self.0.name == "sRGB" { srgb_decode(v) } else { self.0.cctf.decode(v) }
    }
    fn encode(&self, v: f64) -> f64 {
        if self.0.name == "sRGB" { srgb_encode(v) } else { self.0.cctf.encode(v) }
    }
}

/// The tone curve on encoded luminance, [0, 1] → [0, 1], non-decreasing.
fn tone(params: &PostParams, mut e: f64) -> f64 {
    // Brightness: a rational curve with finite slope at both ends, so lifting
    // the midtones doesn't amplify the grain in the deepest shadows.
    let k = 2f64.powf(1.25 * params.brightness as f64 / 100.0);
    e = k * e / (1.0 + (k - 1.0) * e);
    // Contrast: blend toward (or away from) a quadratic S-curve that keeps
    // black and white. The slope at the pivot is 1 + a, so a ≥ -0.75 keeps
    // the curve rising.
    let c = params.contrast as f64 / 100.0;
    let a = if c < 0.0 { 0.75 * c } else { c };
    let s = if e < PIVOT {
        PIVOT * (e / PIVOT).powi(2)
    } else {
        1.0 - (1.0 - PIVOT) * ((1.0 - e) / (1.0 - PIVOT)).powi(2)
    };
    e += a * (s - e);
    // Whites and blacks: levels on the end points.
    let black = -LEVELS_RANGE * params.blacks as f64 / 100.0;
    let white = 1.0 - LEVELS_RANGE * params.whites as f64 / 100.0;
    ((e - black) / (white - black)).clamp(0.0, 1.0)
}

/// Linear interpolation in a table sampled on [0, 1].
fn lookup(table: &[f32], x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0) * (table.len() - 1) as f32;
    let i = (t as usize).min(table.len() - 2);
    let f = t - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

/// CIE Lab lightness of a linear luminance, and its inverse.
fn lstar_of(y: f64) -> f64 {
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        y * 24389.0 / 27.0
    }
}

fn luminance_of(value: f64) -> f64 {
    if value > 8.0 {
        ((value + 16.0) / 116.0).powi(3)
    } else {
        value * 27.0 / 24389.0
    }
}

/// The pins as a rising curve in L\*. The brightest pin aimed at 0 is the
/// black point and the darkest aimed at 100 the white point, whatever order
/// they were made in; the others bend the curve between them. Tones outside
/// the ends clamp, which is what setting a black or white point means. With
/// no pins it is the identity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnchorCurve {
    black: f64,
    white: f64,
    /// Interior pins, strictly between the ends and rising.
    points: Vec<(f64, f64)>,
}

impl AnchorCurve {
    pub fn new(anchors: &[ToneAnchor]) -> Self {
        // Only the target matters, never the order: every pin aimed at black
        // must print black, so the black point is the brightest of them;
        // every pin aimed at white must print white, so the white point is
        // the darkest of them. Tones outside clamp.
        let (mut black, mut white) = (0.0f64, 100.0f64);
        let mut interior = Vec::new();
        for anchor in anchors {
            if !anchor.input.is_finite() || !anchor.target.is_finite() {
                continue;
            }
            let (input, target) = (anchor.input as f64, anchor.target.clamp(0.0, 100.0) as f64);
            if !(0.0..100.0).contains(&input) {
                continue;
            }
            if target <= 0.0 {
                black = black.max(input);
            } else if target >= 100.0 {
                white = white.min(input);
            } else {
                interior.push((input, target));
            }
        }
        // Sorted by input, then by target, so ties resolve the same way
        // whatever order the pins were made in.
        interior.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        // One pin per zone: two pins aimed at the same target would demand the
        // same output from two inputs, flattening the curve between them. The
        // lowest input wins, deterministically.
        interior.dedup_by(|next, kept| (next.1 - kept.1).abs() < 0.5);
        // A black point at or past the white point leaves no room; keep the
        // curve a curve.
        let white = white.max(black + 0.5);
        let mut points = Vec::with_capacity(interior.len());
        for (input, target) in interior {
            let inside = input > black + 0.5 && input < white - 0.5;
            let rising = points
                .last()
                .is_none_or(|&(i, t)| input - i >= 0.5 && target - t >= 0.5);
            if inside && rising {
                points.push((input, target));
            }
        }
        Self { black, white, points }
    }

    pub fn is_identity(&self) -> bool {
        self.black <= 0.0 && self.white >= 100.0 && self.points.is_empty()
    }

    /// The printed L\* of a measured one.
    pub fn map(&self, value: f64) -> f64 {
        if self.is_identity() {
            return value;
        }
        if !value.is_finite() || value <= self.black {
            return 0.0;
        }
        if value >= self.white {
            return 100.0;
        }
        let mut previous = (self.black, 0.0);
        for &(input, target) in &self.points {
            if value <= input {
                return previous.1 + (target - previous.1) * (value - previous.0) / (input - previous.0);
            }
            previous = (input, target);
        }
        previous.1 + (100.0 - previous.1) * (value - previous.0) / (self.white - previous.0)
    }

    /// The same curve on a linear luminance.
    pub fn map_luminance(&self, y: f64) -> f64 {
        if self.is_identity() {
            return y;
        }
        luminance_of(self.map(lstar_of(y)).clamp(0.0, 100.0))
    }
}

/// Post adjustments prepared for per-pixel use. Every curve is tabulated, so
/// applying them costs a few lookups per pixel.
pub struct PostProcess {
    saturation: f32,
    /// The output space's luminance weights.
    luma: [f32; 3],
    /// Whether pixels arrive, and leave, encoded with the output space's
    /// transfer function rather than linear.
    encoded: bool,
    /// Encoded → linear.
    decode: Vec<f32>,
    /// Linear → encoded, sampled on √x for resolution near black.
    encode: Vec<f32>,
    /// Linear luminance → adjusted linear luminance, sampled on √Y.
    tone: Vec<f32>,
}

impl PostProcess {
    /// `encoded` and `color_space` describe the scan's output, as
    /// `io.output_cctf_encoding` and `io.output_color_space`.
    pub fn new(params: &PostParams, encoded: bool, color_space: &str) -> Self {
        let space = Transfer(RgbColourspace::output(color_space));
        let samples = |f: &dyn Fn(f64) -> f64| -> Vec<f32> {
            (0..LUT_SIZE)
                .map(|i| f(i as f64 / (LUT_SIZE - 1) as f64) as f32)
                .collect()
        };
        Self {
            saturation: params.saturation / 100.0,
            luma: luminance_weights(color_space),
            encoded,
            decode: samples(&|v| space.decode(v)),
            encode: samples(&|s| space.encode(s * s)),
            // The pins bend what the knobs produced.
            tone: {
                let curve = AnchorCurve::new(&params.tone_anchors);
                samples(&|s| curve.map_luminance(srgb_decode(tone(params, srgb_encode(s * s)))))
            },
        }
    }

    /// Adjusts one pixel. Input channels are expected in [0, 1], clipped like
    /// a display; so is the result.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut c = if self.encoded {
            rgb.map(|v| lookup(&self.decode, v))
        } else {
            rgb.map(|v| v.clamp(0.0, 1.0))
        };
        let y = self.luma[0] * c[0] + self.luma[1] * c[1] + self.luma[2] * c[2];
        let y2 = lookup(&self.tone, y.max(0.0).sqrt());
        if y <= 1e-9 {
            c = [y2; 3];
        } else {
            c = c.map(|v| v * (y2 / y));
            // Above white, give up saturation rather than luminance or hue.
            let max = c[0].max(c[1]).max(c[2]);
            if max > 1.0 {
                let k = (1.0 - y2) / (max - y2);
                c = c.map(|v| y2 + (v - y2) * k);
            }
        }
        let mut f = 1.0 + self.saturation;
        if f > 1.0 {
            // Stop at the gamut boundary, keeping the hue.
            for v in c {
                let d = v - y2;
                if d > 0.0 {
                    f = f.min((1.0 - y2) / d);
                } else if d < 0.0 {
                    f = f.min(y2 / -d);
                }
            }
        }
        c = c.map(|v| (y2 + (v - y2) * f).clamp(0.0, 1.0));
        if self.encoded {
            c.map(|v| lookup(&self.encode, v.sqrt()))
        } else {
            c
        }
    }

    /// These adjustments for the GPU display pass, which applies them as
    /// [`Self::apply_rgb`] does.
    pub fn display(&self) -> spektrafilm_gpu::DisplayPost {
        spektrafilm_gpu::DisplayPost {
            luma: self.luma,
            saturation: self.saturation,
            encoded: self.encoded,
            decode: self.decode.clone(),
            encode: self.encode.clone(),
            tone: self.tone.clone(),
        }
    }

    pub fn apply(&self, image: &mut ImageBuf) {
        image.data.par_chunks_exact_mut(3).for_each(|px| {
            let out = self.apply_rgb([to_f32(px[0]), to_f32(px[1]), to_f32(px[2])]);
            px[0] = from_f32(out[0]);
            px[1] = from_f32(out[1]);
            px[2] = from_f32(out[2]);
        });
    }

    pub fn applied(&self, image: &ImageBuf) -> ImageBuf {
        let mut image = image.clone();
        self.apply(&mut image);
        image
    }
}

/// The most pixels [`measured_lstar`] reads; a larger circle is sampled on an
/// even grid.
const MEASURE_SAMPLES: f64 = 65_536.0;

/// The mean L\* of the pixels in a circle, as the pins read it: after the
/// four knobs, before any pin. `centre` is in fractions of the image and
/// `radius` a fraction of its shorter side; 0 is the pixel under the centre.
/// None outside the image.
pub fn measured_lstar(
    image: &ImageBuf,
    post: &PostParams,
    encoded: bool,
    color_space: &str,
    centre: (f64, f64),
    radius: f64,
) -> Option<f64> {
    let (width, height) = (image.width as f64, image.height as f64);
    if !(0.0..=1.0).contains(&centre.0) || !(0.0..=1.0).contains(&centre.1) || width < 1.0 || height < 1.0 {
        return None;
    }
    let (cx, cy) = (centre.0 * width, centre.1 * height);
    let reach = radius.max(0.0) * width.min(height);
    let knobs = PostProcess::new(&PostParams { tone_anchors: Vec::new(), ..post.clone() }, encoded, color_space);
    let transfer = Transfer(RgbColourspace::output(color_space));
    let luma = luminance_weights(color_space);
    let (lo, hi) = crate::pipeline::display_range(encoded);
    let lstar = |x: u32, y: u32| {
        let rgb = knobs.apply_rgb(image.get(x, y).map(|v| to_f32(v.clamp(lo, hi))));
        let linear = rgb.map(|v| if encoded { transfer.decode(f64::from(v)) } else { f64::from(v) });
        lstar_of((0..3).map(|c| f64::from(luma[c]) * linear[c]).sum())
    };
    let last = (image.width - 1, image.height - 1);
    if reach < 0.5 {
        return Some(lstar((cx as u32).min(last.0), (cy as u32).min(last.1)));
    }
    let span = |c: f64, end: u32| ((c - reach).max(0.0) as u32, ((c + reach).ceil() as u32).min(end));
    let (x0, x1) = span(cx, last.0);
    let (y0, y1) = span(cy, last.1);
    let area = f64::from(x1 - x0 + 1) * f64::from(y1 - y0 + 1);
    let stride = (area / MEASURE_SAMPLES).sqrt().ceil().max(1.0) as usize;
    let (mut sum, mut count) = (0.0, 0u32);
    for y in (y0..=y1).step_by(stride) {
        for x in (x0..=x1).step_by(stride) {
            let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
            if dx * dx + dy * dy <= reach * reach {
                sum += lstar(x, y);
                count += 1;
            }
        }
    }
    (count > 0).then(|| sum / f64::from(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(f: impl FnOnce(&mut PostParams)) -> PostParams {
        let mut p = PostParams::default();
        f(&mut p);
        p
    }

    #[test]
    fn neutral_settings_keep_the_photograph() {
        let post = PostProcess::new(&PostParams::default(), true, "sRGB");
        for rgb in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.2, 0.5, 0.9],
            [0.8, 0.1, 0.3],
        ] {
            let out = post.apply_rgb(rgb);
            for c in 0..3 {
                assert!((out[c] - rgb[c]).abs() < 2e-4, "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn tone_curves_rise_and_keep_their_ends() {
        for (b, c) in [
            (100.0, 0.0),
            (-100.0, 0.0),
            (0.0, 100.0),
            (0.0, -100.0),
            (60.0, -40.0),
        ] {
            let p = params(|p| {
                p.brightness = b;
                p.contrast = c;
            });
            assert!(tone(&p, 0.0).abs() < 1e-12 && (tone(&p, 1.0) - 1.0).abs() < 1e-12);
            let values: Vec<f64> = (0..=100).map(|i| tone(&p, i as f64 / 100.0)).collect();
            assert!(values.windows(2).all(|w| w[1] >= w[0]), "b {b} c {c}");
        }
        assert!(tone(&params(|p| p.brightness = 50.0), PIVOT) > PIVOT);
        assert!(tone(&params(|p| p.contrast = 50.0), 0.3) < 0.3);
        assert!(tone(&params(|p| p.contrast = 50.0), 0.7) > 0.7);
    }

    #[test]
    fn whites_and_blacks_move_the_end_points() {
        assert_eq!(tone(&params(|p| p.whites = 100.0), 1.0 - LEVELS_RANGE), 1.0);
        assert!(tone(&params(|p| p.whites = -100.0), 1.0) < 0.9);
        assert_eq!(tone(&params(|p| p.blacks = -100.0), LEVELS_RANGE), 0.0);
        assert!(tone(&params(|p| p.blacks = 100.0), 0.0) > 0.1);
    }

    #[test]
    fn saturation_keeps_luminance_and_greys() {
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let mono = PostProcess::new(&params(|p| p.saturation = -100.0), false, "sRGB");
        let out = mono.apply_rgb([0.6, 0.2, 0.1]);
        assert!((out[0] - out[1]).abs() < 1e-6 && (out[1] - out[2]).abs() < 1e-6);
        assert!((luma(out) - luma([0.6, 0.2, 0.1])).abs() < 1e-3);
        let vivid = PostProcess::new(&params(|p| p.saturation = 100.0), false, "sRGB");
        assert_eq!(
            vivid.apply_rgb([0.4, 0.4, 0.4]).map(|v| (v * 1e4).round()),
            [4000.0; 3]
        );
        let out = vivid.apply_rgb([0.9, 0.3, 0.1]);
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!((luma(out) - luma([0.9, 0.3, 0.1])).abs() < 1e-3);
        assert!(out[0] - out[2] > 0.8);
    }

    #[test]
    fn brightening_a_bright_colour_stays_in_gamut() {
        let post = PostProcess::new(
            &params(|p| {
                p.brightness = 100.0;
                p.whites = 100.0;
            }),
            true,
            "sRGB",
        );
        let out = post.apply_rgb([1.0, 0.9, 0.2]);
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)), "{out:?}");
    }

    #[test]
    fn luminance_follows_the_output_space() {
        let prophoto = RgbColourspace::output("ProPhoto RGB").rgb_to_xyz[1].map(|v| v as f32);
        let mono = PostProcess::new(&params(|p| p.saturation = -100.0), false, "ProPhoto RGB");
        for (rgb, y) in [
            ([1.0, 0.0, 0.0], prophoto[0]),
            ([0.0, 0.0, 1.0], prophoto[2]),
        ] {
            let out = mono.apply_rgb(rgb);
            assert!(
                out.iter().all(|v| (v - y).abs() < 1e-4),
                "{rgb:?} -> {out:?}, want {y}"
            );
        }
        assert_eq!(luminance_weights("unknown"), luminance_weights("sRGB"));
    }

    /// The same settings change a colour the same way whatever space it is
    /// encoded in: adjust in sRGB, or in ProPhoto RGB and convert.
    #[test]
    fn adjustments_agree_across_output_spaces() {
        use crate::stages::scanning::convert_output;
        // Tone only: a saturation boost stops at each space's own gamut
        // boundary, which is the point of a wider output space.
        let edit = params(|p| {
            p.brightness = 30.0;
            p.contrast = 25.0;
            p.whites = -10.0;
        });
        let colours = [[0.8f32, 0.3, 0.2], [0.2, 0.5, 0.7], [0.46, 0.46, 0.46], [0.9, 0.85, 0.1], [0.05, 0.04, 0.03]];
        let srgb = ImageBuf::from_data(5, 1, colours.iter().flatten().map(|&v| from_f32(v)).collect());
        let mut prophoto = srgb.clone();
        convert_output(&mut prophoto, "sRGB", "ProPhoto RGB", true);
        let a = PostProcess::new(&edit, true, "sRGB").applied(&srgb);
        let mut b = PostProcess::new(&edit, true, "ProPhoto RGB").applied(&prophoto);
        convert_output(&mut b, "ProPhoto RGB", "sRGB", true);
        for (x, y) in a.data.iter().zip(&b.data) {
            // f32 tables and arithmetic, and two conversions.
            assert!((to_f32(*x) - to_f32(*y)).abs() < 2e-3, "{:?} vs {:?}", a.data, b.data);
        }
    }
    // MARK: The pinned tone curve

    /// Pins bend the tones the four knobs produced, so a pin that holds its
    /// tone leaves a photograph with Post edits exactly as it was.
    #[test]
    fn a_holding_pin_keeps_the_knobs_edit() {
        let knobs = params(|p| p.brightness = 40.0);
        let plain = PostProcess::new(&knobs, true, "sRGB");
        let tone = measured_lstar(
            &ImageBuf::from_data(1, 1, vec![from_f32(0.4); 3]), &knobs, true, "sRGB", (0.5, 0.5), 0.0,
        )
        .unwrap() as f32;
        let mut pinned = knobs.clone();
        pinned.tone_anchors = vec![pin(tone, tone)];
        let pinned = PostProcess::new(&pinned, true, "sRGB");
        for v in [0.2f32, 0.4, 0.6, 0.8] {
            let (a, b) = (plain.apply_rgb([v; 3]), pinned.apply_rgb([v; 3]));
            assert!((a[0] - b[0]).abs() < 2e-3, "{v}: {a:?} vs {b:?}");
        }
    }

    /// A pin's input is the tone before the pins: the curve it joins must not
    /// change what it reads.
    #[test]
    fn measuring_ignores_the_pins_already_on_the_curve() {
        let image = ImageBuf::from_data(2, 1, vec![from_f32(0.2), from_f32(0.2), from_f32(0.2), from_f32(0.6), from_f32(0.6), from_f32(0.6)]);
        let mut pinned = PostParams::default();
        let before = measured_lstar(&image, &pinned, true, "sRGB", (0.25, 0.5), 0.0).unwrap();
        pinned.tone_anchors = vec![pin(70.0, 20.0)];
        let after = measured_lstar(&image, &pinned, true, "sRGB", (0.25, 0.5), 0.0).unwrap();
        assert!((before - after).abs() < 1e-6, "{before} vs {after}");
        // sRGB 0.2 is L* 21.2.
        assert!((before - 21.2).abs() < 0.2, "{before}");
    }

    #[test]
    fn measuring_averages_the_circle_and_stays_on_the_photograph() {
        let mut image = ImageBuf::new(40, 40);
        for y in 0..40 { for x in 0..40 {
            let v = from_f32(if x < 20 { 0.2 } else { 0.6 });
            image.set(x, y, [v, v, v]);
        } }
        let post = PostParams::default();
        let dark = measured_lstar(&image, &post, true, "sRGB", (0.1, 0.5), 0.0).unwrap();
        let light = measured_lstar(&image, &post, true, "sRGB", (0.9, 0.5), 0.0).unwrap();
        let both = measured_lstar(&image, &post, true, "sRGB", (0.5, 0.5), 0.25).unwrap();
        assert!((both - (dark + light) / 2.0).abs() < 1.0, "{dark} {light} {both}");
        assert!(measured_lstar(&image, &post, true, "sRGB", (1.5, 0.5), 0.0).is_none());
        assert!(measured_lstar(&image, &post, true, "sRGB", (0.5, -0.5), 0.0).is_none());
    }

    fn pin(input: f32, target: f32) -> ToneAnchor {
        ToneAnchor { input, target, source: None }
    }

    #[test]
    fn no_pins_leave_the_tones_alone() {
        let curve = AnchorCurve::new(&[]);
        assert!(curve.is_identity());
        for value in [0.0, 5.0, 50.0, 99.0, 100.0] {
            assert_eq!(curve.map(value), value);
        }
    }

    #[test]
    fn a_pin_lands_on_its_target_without_moving_the_ends() {
        let curve = AnchorCurve::new(&[pin(50.0, 30.0)]);
        assert!((curve.map(50.0) - 30.0).abs() < 1e-9);
        assert_eq!(curve.map(0.0), 0.0);
        assert!((curve.map(100.0) - 100.0).abs() < 1e-9);
    }

    /// A pin aimed at black sets the black point, and a later pin elsewhere
    /// leaves it exactly where it was.
    #[test]
    fn a_black_point_pin_stays_put() {
        let curve = AnchorCurve::new(&[pin(30.0, 0.0), pin(60.0, 50.0)]);
        assert_eq!(curve.map(10.0), 0.0);
        assert_eq!(curve.map(30.0), 0.0);
        assert!((curve.map(60.0) - 50.0).abs() < 1e-9);
        assert!((curve.map(100.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_white_point_pin_stays_put() {
        let curve = AnchorCurve::new(&[pin(80.0, 100.0), pin(40.0, 30.0)]);
        assert!((curve.map(80.0) - 100.0).abs() < 1e-9);
        assert!((curve.map(95.0) - 100.0).abs() < 1e-9);
        assert!((curve.map(40.0) - 30.0).abs() < 1e-9);
    }

    /// Only the zones matter, never the order: two pins aimed at black hold
    /// the brightest of them, whichever came first.
    #[test]
    fn pin_order_never_changes_the_curve() {
        let a = AnchorCurve::new(&[pin(30.0, 0.0), pin(10.0, 0.0), pin(60.0, 50.0)]);
        let b = AnchorCurve::new(&[pin(60.0, 50.0), pin(10.0, 0.0), pin(30.0, 0.0)]);
        assert_eq!(a, b);
        assert_eq!(a.map(20.0), 0.0, "everything under the brightest black pin is black");
    }

    /// One pin per zone: two aimed at the same target collapse to one, so the
    /// curve keeps rising instead of flattening.
    #[test]
    fn one_pin_per_zone() {
        let curve = AnchorCurve::new(&[pin(40.0, 50.0), pin(70.0, 50.0)]);
        assert!(curve.points.len() == 1);
        let mut previous = -1.0;
        for step in 0..=100 {
            let value = curve.map(step as f64);
            assert!(value >= previous, "L* {step}");
            previous = value;
        }
    }

    #[test]
    fn pins_keep_the_curve_rising_and_inside() {
        let curve = AnchorCurve::new(&[pin(25.0, 60.0), pin(50.0, 20.0), pin(75.0, 80.0)]);
        let mut previous = -1.0;
        for step in 0..=100 {
            let value = curve.map(step as f64);
            assert!((0.0..=100.0).contains(&value));
            assert!(value >= previous, "L* {step}: {value} < {previous}");
            previous = value;
        }
    }
}
