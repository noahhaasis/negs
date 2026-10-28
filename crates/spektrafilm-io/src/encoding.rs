//! How a finished image's values are encoded: an output colour space, with
//! its transfer function or as linear light. Finishing (resizing, borders,
//! the 35mm strip) needs it, because its own colours are sRGB and its
//! resampling works in linear light. sRGB keeps its exact arithmetic.

use spektrafilm_math::colorspace::{self, RgbColourspace, SRGB};
use spektrafilm_math::precision::{Scalar, from_f64, srgb_decode, srgb_encode, to_f64};

use crate::film35::linear_to_srgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Encoding {
    pub space: &'static RgbColourspace,
    /// The values carry the space's transfer function (else linear light).
    pub encoded: bool,
}

/// sRGB, `encoded` or linear: what finishing assumed before output spaces.
impl From<bool> for Encoding {
    fn from(encoded: bool) -> Self {
        Self { space: &SRGB, encoded }
    }
}

impl Encoding {
    /// `space` as colour names it; unknown names are sRGB, as the scan's
    /// output and the writers treat them.
    pub fn new(space: &str, encoded: bool) -> Self {
        Self { space: RgbColourspace::output(space), encoded }
    }

    fn is_srgb(&self) -> bool {
        self.space.name == SRGB.name
    }

    /// A value → linear light in the space.
    pub fn decode(&self, v: Scalar) -> f64 {
        match (self.encoded, self.is_srgb()) {
            (false, _) => to_f64(v),
            (true, true) => to_f64(srgb_decode(v)),
            (true, false) => self.space.cctf.decode(to_f64(v)),
        }
    }

    /// Linear light in the space → a value.
    pub fn encode(&self, v: f64) -> Scalar {
        match (self.encoded, self.is_srgb()) {
            (false, _) => from_f64(v),
            (true, true) => srgb_encode(from_f64(v)),
            (true, false) => from_f64(self.space.cctf.encode(v)),
        }
    }

    /// Linear sRGB in [0, 1] → this encoding, clipped to its gamut.
    pub fn from_linear_srgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_srgb() {
            return if self.encoded { rgb.map(linear_to_srgb) } else { rgb };
        }
        let m = colorspace::icc_rgb_to_rgb(&SRGB, self.space);
        let lin = colorspace::mat3_vec_f64(&m, rgb.map(f64::from));
        lin.map(|v| {
            let v = v.clamp(0.0, 1.0);
            (if self.encoded { self.space.cctf.encode(v) } else { v }) as f32
        })
    }

    /// An sRGB colour given as this image's values would be in sRGB
    /// (sRGB-encoded when `encoded`, else linear) → this encoding. sRGB
    /// passes through untouched.
    pub fn from_srgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_srgb() {
            return rgb;
        }
        let lin = if self.encoded { rgb.map(|v| to_f64(srgb_decode(from_f64(v as f64))) as f32) } else { rgb };
        self.from_linear_srgb(lin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_passes_through_and_white_stays_white() {
        let srgb = Encoding::from(true);
        assert_eq!(srgb.from_srgb([0.2, 0.5, 0.9]), [0.2, 0.5, 0.9]);
        for space in ["Display P3", "Adobe RGB (1998)", "ProPhoto RGB"] {
            for encoded in [true, false] {
                let e = Encoding::new(space, encoded);
                for v in e.from_srgb([1.0; 3]) {
                    assert!((v - 1.0).abs() < 2e-4, "{space}: {v}");
                }
                let black = e.from_srgb([0.0; 3]);
                assert!(black.iter().all(|v| v.abs() < 1e-6), "{space}");
                // Round trip through the space's own curve.
                for x in [0.0, 0.01, 0.2, 0.7, 1.0] {
                    assert!((e.decode(e.encode(x)) - x).abs() < 1e-5, "{space} {x}");
                }
            }
        }
    }

    #[test]
    fn colours_match_their_srgb_xyz() {
        // An sRGB red in Display P3 is the same colour: same XYZ.
        let e = Encoding::new("Display P3", false);
        let p3 = e.from_srgb([1.0, 0.0, 0.0]).map(f64::from);
        let p3_xyz = colorspace::mat3_vec_f64(&RgbColourspace::output("Display P3").rgb_to_xyz, p3);
        let srgb_xyz = colorspace::mat3_vec_f64(&SRGB.rgb_to_xyz, [1.0, 0.0, 0.0]);
        for c in 0..3 {
            assert!((p3_xyz[c] - srgb_xyz[c]).abs() < 1e-5, "{p3_xyz:?} vs {srgb_xyz:?}");
        }
    }
}
