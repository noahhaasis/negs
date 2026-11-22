//! The developed photograph's tones: 256 bins per channel of the cropped
//! photograph. The border is drawn on the canvas, never in the preview, so it
//! never counts. With it, the RGB waveform: signal level against horizontal
//! position in the shown crop. Mirrors `Histogram.swift`.
//!
//! The shared core's (core/src/rules/histogram, through negs_core_rules):
//! the types stay Rust, the counting, peak and raster run in Odin
//! (core/diff/tests/histogram.rs).

pub use negs_core_rules::histogram::*;

// What the tests below name from the Rust this module was.
#[cfg(test)]
use crate::crop::CropSettings;

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
