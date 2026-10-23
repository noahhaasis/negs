//! The developed photograph's tones: 256 bins per channel of the cropped
//! photograph. The border is drawn on the canvas, never in the preview, so it
//! never counts. Mirrors `Histogram.swift`.

use crate::crop::{CropSettings, Size};

#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
}

impl Default for Histogram {
    fn default() -> Self {
        Self { red: [0; 256], green: [0; 256], blue: [0; 256] }
    }
}

impl Histogram {
    /// Counts the crop of an RGBA preview. A large crop is sampled on an even
    /// grid of at most `max_samples` pixels, which keeps the shape and takes a
    /// few milliseconds at any preview size.
    pub fn of(rgba: &[u8], width: usize, height: usize, crop: &CropSettings, max_samples: usize) -> Histogram {
        let mut histogram = Histogram::default();
        let size = Size::new(width as f64, height as f64);
        let area = crop.pixels(size);
        if area.width < 1.0 || area.height < 1.0 || rgba.len() < width * height * 4 {
            return histogram;
        }
        let stride = ((area.width * area.height / max_samples as f64).sqrt().ceil() as usize).max(1);
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph.
        let (sin, cos) = (-crop.angle * std::f64::consts::PI / 180.0).sin_cos();
        let (cx, cy) = (size.width / 2.0, size.height / 2.0);
        for row in (0..area.height as usize).step_by(stride) {
            for column in (0..area.width as usize).step_by(stride) {
                let (dx, dy) = (area.x + column as f64 + 0.5 - cx, area.y + row as f64 + 0.5 - cy);
                let (x, y) = ((cx + dx * cos - dy * sin).floor(), (cy + dx * sin + dy * cos).floor());
                if x >= 0.0 && y >= 0.0 && (x as usize) < width && (y as usize) < height {
                    let p = (y as usize * width + x as usize) * 4;
                    histogram.red[rgba[p] as usize] += 1;
                    histogram.green[rgba[p + 1] as usize] += 1;
                    histogram.blue[rgba[p + 2] as usize] += 1;
                }
            }
        }
        histogram
    }

    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    pub fn peak(&self) -> u32 {
        let channels = [&self.red, &self.green, &self.blue];
        let inner = channels.iter().map(|c| c[1..255].iter().copied().max().unwrap_or(0)).max().unwrap_or(0);
        if inner > 0 { inner } else { channels.iter().map(|c| c.iter().copied().max().unwrap_or(0)).max().unwrap_or(0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crop::Rect;

    #[test]
    fn counts_only_the_crop() {
        // Left half red, right half blue, 4 × 2.
        let mut rgba = vec![0u8; 4 * 2 * 4];
        for y in 0..2 {
            for x in 0..4 {
                let p = (y * 4 + x) * 4;
                if x < 2 { rgba[p] = 200 } else { rgba[p + 2] = 100 }
                rgba[p + 3] = 255;
            }
        }
        let whole = Histogram::of(&rgba, 4, 2, &CropSettings::default(), 1 << 20);
        assert_eq!((whole.red[200], whole.blue[100], whole.green[0]), (4, 4, 8));
        let left = CropSettings { rect: Rect::new(0.0, 0.0, 0.5, 1.0), ..Default::default() };
        let cropped = Histogram::of(&rgba, 4, 2, &left, 1 << 20);
        assert_eq!((cropped.red[200], cropped.blue[100]), (4, 0));
        let sampled = Histogram::of(&rgba, 4, 2, &CropSettings::default(), 2);
        assert_eq!(sampled.green[0], 2);
    }

    #[test]
    fn peak_ignores_clipping() {
        let mut h = Histogram::default();
        h.red[0] = 1000;
        h.green[255] = 900;
        h.blue[17] = 30;
        assert_eq!(h.peak(), 30);
        let mut clipped = Histogram::default();
        clipped.red[255] = 5;
        assert_eq!(clipped.peak(), 5);
    }
}
