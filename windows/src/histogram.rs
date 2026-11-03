//! The developed photograph's tones: 256 bins per channel of the cropped
//! photograph. The border is drawn on the canvas, never in the preview, so it
//! never counts. With it, the RGB waveform: signal level against horizontal
//! position in the shown crop. Mirrors `Histogram.swift`.

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
    /// Counts the crop of an RGBA preview, sampled on an even grid of at most
    /// `max_samples` pixels (`Scopes::of`).
    #[cfg(test)]
    pub fn of(rgba: &[u8], width: usize, height: usize, crop: &CropSettings, max_samples: usize) -> Histogram {
        Scopes::of(rgba, width, height, crop, 0, max_samples).histogram
    }

    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    pub fn peak(&self) -> u32 {
        let channels = [&self.red, &self.green, &self.blue];
        let inner = channels.iter().map(|c| c[1..255].iter().copied().max().unwrap_or(0)).max().unwrap_or(0);
        if inner > 0 { inner } else { channels.iter().map(|c| c.iter().copied().max().unwrap_or(0)).max().unwrap_or(0) }
    }
}

/// RGB signal level against horizontal position in the shown crop. Bins are
/// row-major, with level 0 in row 0 and 255 in row 255.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    pub width: usize,
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub samples: usize,
}

impl Default for Waveform {
    fn default() -> Self {
        Self { width: 1, red: vec![0; 256], green: vec![0; 256], blue: vec![0; 256], samples: 0 }
    }
}

impl Waveform {
    /// A small raster, `width` x 256 with level 255 in the top row, as
    /// premultiplied red, green, blue and alpha: a log scale of the counts
    /// so faint levels show. Drawing then doesn't depend on the photo's size.
    pub fn raster(&self) -> Vec<[u8; 4]> {
        let peak = self.red.iter().chain(&self.green).chain(&self.blue).copied().max().unwrap_or(0);
        let mut pixels = vec![[0u8; 4]; self.width * 256];
        if peak == 0 {
            return pixels;
        }
        let normalizer = (peak as f64).ln_1p();
        let intensity = |count: u32| if count == 0 { 0 } else { (255.0 * (0.18 + 0.82 * (count as f64).ln_1p() / normalizer)).round() as u8 };
        for level in 0..256 {
            for x in 0..self.width {
                let bin = level * self.width + x;
                let (r, g, b) = (intensity(self.red[bin]), intensity(self.green[bin]), intensity(self.blue[bin]));
                pixels[(255 - level) * self.width + x] = [r, g, b, r.max(g).max(b)];
            }
        }
        pixels
    }
}

/// Both scopes from one bounded grid over the crop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scopes {
    pub histogram: Histogram,
    pub waveform: Waveform,
}

impl Scopes {
    /// Samples the crop of an RGBA preview on an even grid of at most
    /// `max_samples` pixels, which keeps the shape and takes a few
    /// milliseconds at any size; the budget also bounds very thin panoramas.
    /// The waveform runs across the photograph as shown, after `rotation`
    /// quarter turns clockwise.
    pub fn of(rgba: &[u8], width: usize, height: usize, crop: &CropSettings, rotation: i32, max_samples: usize) -> Scopes {
        let size = Size::new(width as f64, height as f64);
        let area = crop.pixels(size);
        if area.width < 1.0 || area.height < 1.0 || max_samples == 0 || rgba.len() < width * height * 4 {
            return Scopes::default();
        }
        let (aw, ah) = (area.width as usize, area.height as usize);
        let columns = aw.min(max_samples).min(((max_samples as f64 * aw as f64 / ah as f64).sqrt() as usize).max(1));
        let rows = ah.min((max_samples / columns).max(1));
        let turn = rotation.rem_euclid(4);
        let shown_columns = if turn % 2 == 0 { columns } else { rows };
        let wave_width = shown_columns.min(256);
        let mut histogram = Histogram::default();
        let (mut red, mut green, mut blue) = (vec![0u32; wave_width * 256], vec![0u32; wave_width * 256], vec![0u32; wave_width * 256]);
        let mut samples = 0;
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph.
        let (sin, cos) = (-crop.angle * std::f64::consts::PI / 180.0).sin_cos();
        let (cx, cy) = (size.width / 2.0, size.height / 2.0);
        for row in 0..rows {
            for column in 0..columns {
                let dx = area.x + (column as f64 + 0.5) * area.width / columns as f64 - cx;
                let dy = area.y + (row as f64 + 0.5) * area.height / rows as f64 - cy;
                let (x, y) = ((cx + dx * cos - dy * sin).floor(), (cy + dx * sin + dy * cos).floor());
                if x < 0.0 || y < 0.0 || x as usize >= width || y as usize >= height {
                    continue;
                }
                let p = (y as usize * width + x as usize) * 4;
                let (r, g, b) = (rgba[p] as usize, rgba[p + 1] as usize, rgba[p + 2] as usize);
                histogram.red[r] += 1;
                histogram.green[g] += 1;
                histogram.blue[b] += 1;
                let shown = match turn {
                    1 => rows - 1 - row,
                    2 => columns - 1 - column,
                    3 => row,
                    _ => column,
                };
                let wx = shown * wave_width / shown_columns;
                red[r * wave_width + wx] += 1;
                green[g * wave_width + wx] += 1;
                blue[b * wave_width + wx] += 1;
                samples += 1;
            }
        }
        Scopes { histogram, waveform: Waveform { width: wave_width, red, green, blue, samples } }
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

    /// An RGBA preview whose left half is black and right half white, with a
    /// red column down the middle (`HistogramTests.swift`).
    fn halves(width: usize, height: usize) -> Vec<u8> {
        let mut rgba = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let p = (y * width + x) * 4;
                let c: [u8; 3] = if x == width / 2 { [200, 0, 0] } else if x < width / 2 { [0, 0, 0] } else { [255, 255, 255] };
                rgba[p..p + 3].copy_from_slice(&c);
                rgba[p + 3] = 255;
            }
        }
        rgba
    }

    #[test]
    fn counts_every_pixel_and_turns_straightened_crops_back() {
        let image = halves(100, 60);
        let whole = Histogram::of(&image, 100, 60, &CropSettings::default(), 1 << 20);
        assert_eq!(whole.red.iter().sum::<u32>(), 6000);
        assert_eq!((whole.red[0], whole.red[200], whole.red[255]), (50 * 60, 60, 49 * 60));
        let left = Rect::new(0.0, 0.25, 0.4, 0.5);
        let turned = Histogram::of(&image, 100, 60, &CropSettings { rect: left, angle: 180.0, ..Default::default() }, 1 << 20);
        assert_eq!(turned.red[255], turned.red.iter().sum::<u32>());
        let sampled = Histogram::of(&image, 100, 60, &CropSettings::default(), 1500);
        let total = sampled.red.iter().sum::<u32>() as f64;
        assert!(total <= 1500.0 && (sampled.red[0] as f64 / total - 0.5).abs() < 0.05);
    }

    #[test]
    fn waveform_keeps_position_and_levels() {
        let scopes = Scopes::of(&halves(100, 60), 100, 60, &CropSettings::default(), 0, 1 << 20);
        let w = &scopes.waveform;
        assert_eq!((w.width, w.samples), (100, 6000));
        assert_eq!(w.red[0], 60, "Black is at the bottom of the left column");
        assert_eq!(w.red[200 * 100 + 50], 60, "The red column keeps its level and position");
        assert_eq!(w.blue[255 * 100 + 99], 60, "White is at the top of the right column");
        assert_eq!(w.red.iter().sum::<u32>(), scopes.histogram.red.iter().sum::<u32>());
        let raster = w.raster();
        assert!(raster[255 * 100][0] > 0 && raster[99][0] > 0, "Black draws at the bottom, white at the top");
    }

    #[test]
    fn waveform_follows_the_crop_straightening_and_quarter_turns() {
        let image = halves(100, 60);
        let right = CropSettings { rect: Rect::new(0.5, 0.0, 0.5, 1.0), ..Default::default() };
        let w = Scopes::of(&image, 100, 60, &right, 0, 1 << 20).waveform;
        assert_eq!((w.width, w.red[200 * 50], w.red[255 * 50 + 49]), (50, 60, 60));
        let turned = Scopes::of(&image, 100, 60, &CropSettings { angle: 180.0, ..Default::default() }, 0, 1 << 20).waveform;
        assert_eq!((turned.red[255 * 100], turned.red[99]), (60, 60));
        let half = Scopes::of(&image, 100, 60, &CropSettings::default(), 2, 1 << 20).waveform;
        assert_eq!((half.red[255 * 100], half.red[99]), (60, 60));
        for rotation in [1, 3, -1] {
            let w = Scopes::of(&image, 100, 60, &CropSettings::default(), rotation, 1 << 20).waveform;
            assert_eq!(w.width, 60);
            for x in 0..60 {
                assert_eq!((w.red[x], w.red[200 * 60 + x], w.red[255 * 60 + x]), (50, 1, 49), "rotation {rotation}");
            }
        }
    }

    #[test]
    fn the_budget_bounds_thin_panoramas_and_nothing_is_empty() {
        for (width, height) in [(10000, 1), (1, 10000), (101, 61)] {
            let scopes = Scopes::of(&halves(width, height), width, height, &CropSettings::default(), 0, 127);
            assert!(scopes.waveform.samples <= 127 && scopes.waveform.samples > 0 && scopes.waveform.width <= 256);
            assert_eq!(scopes.histogram.red.iter().sum::<u32>() as usize, scopes.waveform.samples);
        }
        let empty = Scopes::of(&halves(100, 60), 100, 60, &CropSettings::default(), 0, 0);
        assert_eq!((empty.histogram.peak(), empty.waveform.samples), (0, 0));
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
