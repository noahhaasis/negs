//! Post adjustments: finishing edits on the scanned photograph, as an editor
//! would make them to a lab scan. They run on the scan clipped to [0, 1]
//! (`pipeline::clip_to_display`), as an integer file holds it, so they shape
//! the tones the film produced but cannot recover a clipped highlight; that
//! belongs to camera and print exposure.
//!
//! Tones are shaped on luminance in sRGB-encoded (perceptual) space, and each
//! pixel keeps its colour ratios, so contrast doesn't shift the film's hues.
//!
//! The arithmetic is f32 even in the f64 build: the tables already limit
//! accuracy to about 2e-7, far below a 16-bit output step, and the f64
//! build's purpose, parity with the Python reference, doesn't cover Post.
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::colorspace::SRGB_TO_XYZ;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};
use spektrafilm_math::spectral::colorspace_to_xyz;

/// Each control runs from -100 to 100; 0 leaves the photograph unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
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
fn luminance_weights(color_space: &str) -> [f32; 3] {
    match color_space {
        "ProPhoto RGB" | "Rec. 2020" | "Rec2020" | "ITU-R BT.2020" | "ACES2065-1" => {
            colorspace_to_xyz(color_space)[1]
        }
        _ => SRGB_TO_XYZ[1],
    }
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

/// Post adjustments prepared for per-pixel use. Every curve is tabulated, so
/// applying them costs a few lookups per pixel.
pub struct PostProcess {
    saturation: f32,
    /// The output space's luminance weights.
    luma: [f32; 3],
    /// Whether pixels arrive, and leave, sRGB-encoded rather than linear.
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
        let samples = |f: &dyn Fn(f64) -> f64| -> Vec<f32> {
            (0..LUT_SIZE)
                .map(|i| f(i as f64 / (LUT_SIZE - 1) as f64) as f32)
                .collect()
        };
        Self {
            saturation: params.saturation / 100.0,
            luma: luminance_weights(color_space),
            encoded,
            decode: samples(&srgb_decode),
            encode: samples(&|s| srgb_encode(s * s)),
            tone: samples(&|s| srgb_decode(tone(params, srgb_encode(s * s)))),
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
        let prophoto = colorspace_to_xyz("ProPhoto RGB")[1];
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
}
