//! Presentation crops of a rendered photograph.
//!
//! Like borders, a crop is applied after the film pipeline, so changing one
//! never re-renders: the film is exposed and developed whole, and the print
//! is trimmed. The rectangle is in the photograph's unrotated orientation, so
//! it turns with the photograph. A straightened crop turns the photograph by
//! a few degrees about its centre first; the rectangle is then in that turned
//! frame. The macOS preview shows the same rectangle
//! (`macos/Sources/Spektrafilm/Crop.swift`); keep the rounding in sync.

use std::str::FromStr;

use anyhow::{Result, bail, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f64, to_f64};

/// The furthest a photograph can be straightened, in degrees either way.
pub const MAX_ANGLE: f64 = 45.0;

/// A rectangle as fractions of the unrotated photograph, origin top left,
/// and a straightening angle in degrees, clockwise as shown.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub angle: f64,
}

impl Crop {
    /// Fails unless the rectangle lies inside the photograph. A
    /// straightened rectangle may reach past the unturned edges, where the
    /// turned photograph's corners are.
    pub fn validate(&self) -> Result<()> {
        let values = [self.x, self.y, self.width, self.height, self.angle];
        ensure!(
            values.iter().all(|v| v.is_finite()) && self.angle.abs() <= MAX_ANGLE,
            "The crop must lie inside the photograph"
        );
        let (low, high) = if self.angle == 0.0 {
            (0.0, 1.0 + 1e-9)
        } else {
            (-0.5, 1.5)
        };
        ensure!(
            self.x >= low
                && self.y >= low
                && self.width > 0.0
                && self.height > 0.0
                && self.x + self.width <= high
                && self.y + self.height <= high,
            "The crop must lie inside the photograph"
        );
        Ok(())
    }

    /// The pixel rectangle (x, y, width, height) in a `width` × `height`
    /// image: edges round to the nearest pixel, and at least one remains.
    pub fn pixels(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let edge = |fraction: f64, size: u32| {
            (fraction * size as f64).round().clamp(0.0, size as f64) as u32
        };
        let (x0, y0) = (edge(self.x, width), edge(self.y, height));
        let x1 = edge(self.x + self.width, width).max(x0 + 1).min(width);
        let y1 = edge(self.y + self.height, height).max(y0 + 1).min(height);
        let (x0, y0) = (x0.min(x1 - 1), y0.min(y1 - 1));
        (x0, y0, x1 - x0, y1 - y0)
    }

    /// The straightened crop's size in pixels: its sides round to the
    /// nearest pixel, keeping at least one.
    pub fn straightened_size(&self, width: u32, height: u32) -> (u32, u32) {
        (
            ((self.width * width as f64).round() as u32).max(1),
            ((self.height * height as f64).round() as u32).max(1),
        )
    }

    /// The part of `image` inside the crop.
    pub fn apply(&self, image: &ImageBuf) -> ImageBuf {
        if self.angle != 0.0 {
            return self.straighten(image);
        }
        let (x, y, w, h) = self.pixels(image.width, image.height);
        if (x, y, w, h) == (0, 0, image.width, image.height) {
            return image.clone();
        }
        let row = image.width as usize * 3;
        let mut data = Vec::with_capacity(w as usize * h as usize * 3);
        for line in image
            .data
            .chunks_exact(row)
            .skip(y as usize)
            .take(h as usize)
        {
            data.extend_from_slice(&line[x as usize * 3..(x + w) as usize * 3]);
        }
        ImageBuf::from_data(w, h, data)
    }

    /// Turns `image` about its centre and samples the rectangle bilinearly.
    /// Rounding at the rectangle's edge can reach just past the photograph;
    /// those samples repeat its edge pixels.
    fn straighten(&self, image: &ImageBuf) -> ImageBuf {
        let (w, h) = self.straightened_size(image.width, image.height);
        let (iw, ih) = (image.width as f64, image.height as f64);
        let (cx, cy) = (iw / 2.0, ih / 2.0);
        let (origin_x, origin_y) = (self.x * iw, self.y * ih);
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let src = &image.data;
        let (max_x, max_y) = (image.width as usize - 1, image.height as usize - 1);
        let mut data = vec![from_f64(0.0); w as usize * h as usize * 3];
        data.par_chunks_exact_mut(w as usize * 3)
            .enumerate()
            .for_each(|(v, row)| {
                for u in 0..w as usize {
                    // Turn back by the angle (y down, so clockwise is positive).
                    let (dx, dy) = (
                        origin_x + u as f64 + 0.5 - cx,
                        origin_y + v as f64 + 0.5 - cy,
                    );
                    let sx = (cx + dx * cos + dy * sin - 0.5).clamp(0.0, max_x as f64);
                    let sy = (cy - dx * sin + dy * cos - 0.5).clamp(0.0, max_y as f64);
                    let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                    let (x1, y1) = ((x0 + 1).min(max_x), (y0 + 1).min(max_y));
                    let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
                    let at = |x: usize, y: usize, c: usize| {
                        to_f64(src[(y * image.width as usize + x) * 3 + c])
                    };
                    for c in 0..3 {
                        let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                        let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                        row[u * 3 + c] = from_f64(top * (1.0 - fy) + bottom * fy);
                    }
                }
            });
        ImageBuf::from_data(w, h, data)
    }
}

/// `x,y,width,height[,angle]`: fractions, then degrees.
impl FromStr for Crop {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let expected = "expected x,y,width,height as fractions, then an optional angle in degrees";
        let values: Vec<f64> = s
            .split(',')
            .map(|v| v.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| anyhow::anyhow!("invalid crop: {s} ({expected})"))?;
        let (x, y, width, height, angle) = match values[..] {
            [x, y, width, height] => (x, y, width, height, 0.0),
            [x, y, width, height, angle] => (x, y, width, height, angle),
            _ => bail!("invalid crop: {s} ({expected})"),
        };
        let crop = Self {
            x,
            y,
            width,
            height,
            angle,
        };
        crop.validate()?;
        Ok(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spektrafilm_math::precision::from_f32;

    fn crop(x: f64, y: f64, width: f64, height: f64) -> Crop {
        Crop {
            x,
            y,
            width,
            height,
            angle: 0.0,
        }
    }

    #[test]
    fn crop_keeps_the_pixels_inside() {
        let data = (0..4 * 3 * 3).map(|v| from_f32(v as f32)).collect();
        let image = ImageBuf::from_data(4, 3, data);
        let out = crop(0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0).apply(&image);
        assert_eq!((out.width, out.height), (2, 2));
        assert_eq!(out.get(0, 0), image.get(1, 1));
        assert_eq!(out.get(1, 1), image.get(2, 2));
    }

    #[test]
    fn pixels_round_edges_and_keep_one_pixel() {
        assert_eq!(
            crop(0.1004, 0.0, 0.5, 1.0).pixels(1000, 800),
            (100, 0, 500, 800)
        );
        assert_eq!(crop(0.999, 0.0, 0.0001, 1.0).pixels(10, 10), (9, 0, 1, 10));
    }

    #[test]
    fn straightening_turns_the_photograph_clockwise() {
        // A bright pixel right of centre on a 41 × 41 photograph.
        let mut image = ImageBuf::from_data(41, 41, vec![from_f32(0.0); 41 * 41 * 3]);
        image.set(30, 20, [from_f32(1.0); 3]);
        let mut turned = crop(0.0, 0.0, 1.0, 1.0);
        turned.angle = 90.0 / 2.0;
        let out = turned.apply(&image);
        assert_eq!((out.width, out.height), (41, 41));
        // Ten pixels right of centre turns 45° clockwise: down and to the right.
        let d = (10.0 * std::f64::consts::FRAC_1_SQRT_2).round() as u32;
        let brightest = (0..41 * 41)
            .max_by(|&a, &b| {
                let (pa, pb) = (out.get(a % 41, a / 41)[0], out.get(b % 41, b / 41)[0]);
                pa.partial_cmp(&pb).unwrap()
            })
            .unwrap();
        assert_eq!((brightest % 41, brightest / 41), (20 + d, 20 + d));
        // A straightened crop keeps its rounded size.
        let mut small = crop(0.2, 0.1, 0.5, 0.25);
        small.angle = -3.0;
        assert_eq!(small.apply(&image).width, 21);
        assert_eq!(small.straightened_size(1000, 800), (500, 200));
    }

    #[test]
    fn parses_fractions_and_rejects_crops_outside() {
        let parsed: Crop = "0.1, 0.2, 0.5, 0.25".parse().unwrap();
        assert_eq!(parsed, crop(0.1, 0.2, 0.5, 0.25));
        let straightened: Crop = "0.1,0.2,0.5,0.25,-2.5".parse().unwrap();
        assert_eq!(straightened.angle, -2.5);
        assert!("0.6,0,0.5,1".parse::<Crop>().is_err());
        assert!("0,0,1".parse::<Crop>().is_err());
        assert!("0,0,0,1".parse::<Crop>().is_err());
        assert!("0,0,1,1,50".parse::<Crop>().is_err());
    }
}
