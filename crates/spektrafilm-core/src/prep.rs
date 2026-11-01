//! Scene preparation before film exposure, ported from Sawmill's Resolve 21.1
//! CameraRawAdjustFilter reconstruction. The recovered controls operate in
//! Rec.709 primaries / Rec.709 OETF; boundaries remain signed scene-linear RGB.
//! This is control-math parity, not decoder or complete Resolve-render parity.
//! The self-guided base needs the complete frame, never independently cropped tiles.
//! Arithmetic stays f32 in reference builds, matching Sawmill's recovered core.
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::{
    colorspace::{RgbColourspace, SRGB, icc_rgb_to_rgb, mat3_vec_f64},
    image::ImageBuf,
    precision::{from_f32, from_f64, to_f64},
};
mod guided;

/// Zero is an exact bypass. Exposure is scene placement in stops; Highlights
/// has Sawmill's extended negative travel and the other controls use -100..100.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrepParams {
    /// Absolute scene placement after camera metering/EV, before recovered
    /// controls. Unlike camera EV, this is excluded from print compensation.
    pub exposure_ev: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub contrast: f32,
    pub lift: f32,
    pub gain: f32,
    pub color_boost: f32,
    pub saturation: f32,
    pub midtone_detail: f32,
}

impl PrepParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.exposure_ev.is_finite() || !(-3.0..=3.0).contains(&self.exposure_ev) {
            return Err("Invalid Prep exposure_ev; expected a finite value from -3 to 3".into());
        }
        for (name, value, min) in [
            ("highlights", self.highlights, -180.),
            ("shadows", self.shadows, -100.),
            ("contrast", self.contrast, -100.),
            ("lift", self.lift, -100.),
            ("gain", self.gain, -100.),
            ("color_boost", self.color_boost, -100.),
            ("saturation", self.saturation, -100.),
            ("midtone_detail", self.midtone_detail, -100.),
        ] {
            if !value.is_finite() || !(min..=100.).contains(&value) {
                return Err(format!(
                    "Invalid Prep {name}; expected a finite value from {min} to 100"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct Settings {
    highlights: f32,
    shadows: f32,
    boost: f32,
    saturation: f32,
    lift: f32,
    gain: f32,
    contrast: f32,
    detail: f32,
}
impl From<&PrepParams> for Settings {
    fn from(p: &PrepParams) -> Self {
        Self {
            highlights: p.highlights,
            shadows: p.shadows,
            boost: p.color_boost,
            saturation: p.saturation,
            lift: p.lift,
            gain: p.gain,
            contrast: p.contrast,
            detail: p.midtone_detail,
        }
    }
}

// Exact recovered Rec.709 gamma enum 2. Negative input follows the linear branch.
fn encode709(x: f32) -> f32 {
    if x <= 0.018 {
        4.5 * x
    } else {
        1.099 * x.powf(0.45) - 0.099
    }
}
fn decode709(x: f32) -> f32 {
    if x <= 0.081 {
        x / 4.5
    } else {
        ((x + 0.099) / 1.099).powf(1. / 0.45)
    }
}
fn luma(p: [f32; 3]) -> f32 {
    p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722
}

// 0x1063cf3cc: Catmull-Rom replacement in two bounded intervals of the tone table.
fn smooth_interval(lut: &mut [f32; 1024], start: usize, step: usize, p: [f32; 4]) {
    let a = (-0.5f32).mul_add(p[0], 0.5 * p[2]);
    let b = (-0.5f32).mul_add(p[3], 2f32.mul_add(p[2], (-2.5f32).mul_add(p[1], p[0])));
    let c = 0.5f32.mul_add(
        p[3],
        (-1.5f32).mul_add(p[2], (-0.5f32).mul_add(p[0], 1.5 * p[1])),
    );
    for j in 1..step {
        let t = j as f32 / step as f32;
        lut[start + j] = (c * t * t).mul_add(t, (b * t).mul_add(t, a.mul_add(t, p[1])));
    }
}
// 0x1063cf8e0, called with count1024, h=1.5*Highlights/100,
// s=1.5*Shadows/200, extent2, final boolean false in the RAW unpacker.
fn tone_lut(highlights: f32, shadows: f32) -> [f32; 1024] {
    let he = (1.5 * (highlights / 100.)).exp2();
    let se = (-(shadows / 200.) * 1.5).exp2();
    let denominator = 0.5f32.powf(se);
    let mut lut = [0.; 1024];
    let mut x = 0f32;
    let dx = 1. / 1023.;
    for v in &mut lut {
        let h = (1. - x).powf(he);
        let s = if x > 0.5 {
            x
        } else {
            0.5 * (x.powf(se) / denominator)
        };
        *v = s.mul_add(1. - x, x * (1. - h));
        x += dx;
    }
    let points = [lut[416], lut[480], lut[544], lut[608]];
    smooth_interval(&mut lut, 480, 64, points);
    let points = [lut[895], lut[959], lut[1023], lut[1023] + 64. / 1024.];
    smooth_interval(&mut lut, 959, 64, points);
    lut
}
// Normalized Metal linear texture coordinates, clamp-to-edge. The shader samples
// the 1024-entry texture at X directly; it does not use X*(N-1) addressing.
fn sample_lut(lut: &[f32; 1024], x: f32) -> f32 {
    let coordinate = (x * 1024. - 0.5).clamp(0., 1023.);
    let i = coordinate.floor() as usize;
    let j = (i + 1).min(1023);
    (lut[j] - lut[i]).mul_add(coordinate - i as f32, lut[i])
}
fn tone_sample(lut: &[f32; 1024], x: f32) -> f32 {
    if x > 1. {
        let y = sample_lut(lut, 1.);
        y + (y - sample_lut(lut, 0.875)) / 0.125 * (x - 1.)
    } else if x < 0. {
        let y = sample_lut(lut, 0.);
        y + (sample_lut(lut, 0.125) - y) / 0.125 * x
    } else {
        sample_lut(lut, x)
    }
}
// 0x1063cfa80. Lift and Gain are not an additive exposure surrogate.
fn affine(lift: f32, gain: f32) -> (f32, f32) {
    let top = 1. / 10f32.powf(gain / 100.).max(f32::from_bits(0x3a83126f));
    let lift = (2. * (lift / 100.)).min(f32::from_bits(0x3f7fbe77));
    let bottom = (lift / (lift - 1.)).min(top + f32::from_bits(0xbba3d70a));
    let span = top - bottom;
    (1. / span, -bottom / span)
}
fn core(
    rgb: [f32; 3],
    base: f32,
    s: Settings,
    lut: &[f32; 1024],
    affine: (f32, f32),
    contrast: f32,
) -> [f32; 3] {
    let p = rgb.map(|x| x * 0.5);
    let mut y = luma(p);
    let a = p[0] * 0.5 - p[1] * 0.25 - p[2] * 0.25;
    let b = p[1] * 0.433012701892219 - p[2] * 0.433012701892219;
    let mut c = (a * a + b * b).sqrt();
    let mut hue = b.atan2(a) / (2. * std::f32::consts::PI);
    if hue < 0. {
        hue += 1.;
    }
    let base = base * 0.5;
    let detail = y - base;
    let mapped = tone_sample(lut, base);
    let ratio = if base.abs() > 0.00001 {
        (mapped / base).clamp(1., 4.)
    } else {
        1.
    };
    y = mapped + ratio * detail;
    c *= ratio;
    y += detail * (s.detail / 100.) * (y * (1. - y)).max(0.) * 4.;
    let boost = s.boost / 100.;
    let weight = if boost >= 0. { 0.7 * boost } else { boost };
    c = (c + c.powf(0.4) * weight * 0.25).max(0.);
    c = (c * (1. + s.saturation / 100.)).max(0.);
    let mut out = [y; 3];
    if c > 0. {
        let a = c * (hue * 2. * std::f32::consts::PI).cos();
        let b = c * (hue * 2. * std::f32::consts::PI).sin();
        out = [
            y + a * 4. / 3.,
            y - a * 2. / 3. + b * 1.154700538379252,
            y - a * 2. / 3. - b * 1.154700538379252,
        ];
        let correction = y - luma(out);
        for x in &mut out {
            *x += correction;
        }
    }
    out.map(|v| ((2. * v - 0.5) * contrast + 0.5) * affine.0 + affine.1)
}

/// Apply Prep to linear pixels in `input_color_space`, returning to that same
/// space. The bridge uses Bradford adaptation (e.g. D50 ProPhoto to D65 Rec.709)
/// and exact matrix inverses. No gamut clamp or display transform is applied.
/// Call after decoding the input transfer and exposure, before the film model.
/// Neutral settings do not allocate, convert, quantize, or change a single bit.
pub fn apply_linear(
    image: &mut ImageBuf,
    params: &PrepParams,
    input_color_space: &str,
) -> Result<(), String> {
    params.validate()?;
    if params.is_identity() {
        return Ok(());
    }
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0 || h == 0 || w.checked_mul(h).and_then(|n| n.checked_mul(3)) != Some(image.data.len())
    {
        return Err("Invalid Prep image dimensions".into());
    }
    if params.exposure_ev != 0.0 {
        let gain = from_f64((params.exposure_ev as f64).exp2());
        image.data.par_iter_mut().try_for_each(|v| {
            *v *= gain;
            if v.is_finite() {
                Ok(())
            } else {
                Err("Prep exposure produced a non-finite result".to_owned())
            }
        })?;
    }
    // Placement alone must not round-trip through the recovered f32 control
    // space: a uniform gain is exact in any linear RGB primaries.
    if (PrepParams {
        exposure_ev: 0.0,
        ..*params
    })
    .is_identity()
    {
        return Ok(());
    }
    let space = RgbColourspace::input(input_color_space);
    let to709 = icc_rgb_to_rgb(space, &SRGB);
    let from709 = icc_rgb_to_rgb(&SRGB, space);
    let same_space = space.name == SRGB.name;
    let encoded: Vec<[f32; 3]> = image
        .par_pixels()
        .map(|p| {
            let rgb = [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])];
            let rgb = if same_space {
                rgb
            } else {
                mat3_vec_f64(&to709, rgb)
            };
            rgb.map(|v| encode709(v as f32))
        })
        .collect();
    let luminance: Vec<f32> = encoded.par_iter().copied().map(luma).collect();
    let base = guided::base_luma(&luminance, w, h, None)?;
    let s = Settings::from(params);
    let lut = tone_lut(s.highlights, s.shadows);
    let affine = affine(s.lift, s.gain);
    let contrast = 1.5f32.powf(s.contrast / 100.);
    image
        .data
        .par_chunks_mut(3)
        .enumerate()
        .try_for_each(|(i, p)| -> Result<(), String> {
            let rgb = core(encoded[i], base[i], s, &lut, affine, contrast).map(decode709);
            let result = if same_space {
                rgb.map(from_f32)
            } else {
                mat3_vec_f64(&from709, rgb.map(|v| v as f64)).map(from_f64)
            };
            if result.iter().any(|v| !v.is_finite()) {
                return Err("Prep adjustments produced a non-finite result".into());
            }
            p.copy_from_slice(&result);
            Ok(())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Generated from the unchanged shipped shader core, compiled as an independent
    // C++ oracle. See rawcontrols-trace-oracle.py and its provenance receipt.
    #[test]
    fn controls_match_shipped_source_goldens() {
        let cases: [([f32; 8], [f32; 3]); 16] = [
            (
                [-70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.0745500326_f32, 0.49454987_f32, 1.25454986_f32],
            ),
            (
                [70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.141301692_f32, 0.635856271_f32, 1.53076482_f32],
            ),
            (
                [0_f32, -70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.0149120986_f32, 0.434911877_f32, 1.19491184_f32],
            ),
            (
                [0_f32, 70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.147525907_f32, 0.663865328_f32, 1.59819412_f32],
            ),
            (
                [0_f32, 0_f32, -70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.271212071_f32, 0.525894344_f32, 0.986748159_f32],
            ),
            (
                [0_f32, 0_f32, 70_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.0132041276_f32, 0.548926234_f32, 1.51832855_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, -70_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [0.389348537_f32, 0.515348494_f32, 0.743348479_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 70_f32, 0_f32, 0_f32, 0_f32, 0_f32],
                [-0.150463283_f32, 0.563536465_f32, 1.85553646_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, -70_f32, 0_f32, 0_f32, 0_f32],
                [-1.1133374_f32, -0.105337858_f32, 1.71866179_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 70_f32, 0_f32, 0_f32, 0_f32],
                [0.999119461_f32, 0.999539435_f32, 1.00029945_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, -70_f32, 0_f32, 0_f32],
                [0.0238319524_f32, 0.107632942_f32, 0.259272844_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 70_f32, 0_f32, 0_f32],
                [0.598631561_f32, 2.7036171_f32, 6.51263952_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, -70_f32, 0_f32],
                [0.213479191_f32, 0.529696226_f32, 1.10189855_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 70_f32, 0_f32],
                [-0.00545662642_f32, 0.552387655_f32, 1.56182039_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, -70_f32],
                [0.0794987679_f32, 0.499498576_f32, 1.25949848_f32],
            ),
            (
                [0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 0_f32, 70_f32],
                [0.159386605_f32, 0.579386413_f32, 1.33938634_f32],
            ),
        ];
        for (p, expected) in cases {
            let s = Settings {
                highlights: p[0],
                shadows: p[1],
                boost: p[2],
                saturation: p[3],
                lift: p[4],
                gain: p[5],
                contrast: p[6],
                detail: p[7],
            };
            let lut = tone_lut(s.highlights, s.shadows);
            let actual = core(
                [0.12, 0.54, 1.3],
                0.43,
                s,
                &lut,
                affine(s.lift, s.gain),
                1.5f32.powf(s.contrast / 100.),
            );
            for i in 0..3 {
                assert!(
                    (actual[i] - expected[i]).abs() <= 3e-6 * expected[i].abs().max(1.0),
                    "settings {p:?}, channel {i}: {} != {}",
                    actual[i],
                    expected[i]
                );
            }
        }
    }
}
