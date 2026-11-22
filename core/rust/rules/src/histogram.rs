//! windows/src/histogram.rs's API on the shared Odin core
//! (core/src/rules/histogram). The developed photograph's tones: 256 bins
//! per channel of the cropped photograph. The border is drawn on the
//! canvas, never in the preview, so it never counts. With it, the RGB
//! waveform: signal level against horizontal position in the shown crop.
//!
//! The counting, the peak and the waveform's raster run in Odin; the bins
//! cross through the caller's buffers. The types stay Rust.

use negs_core_sys::picture as sys;

use crate::crop::{CropSettings, Orientation};

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
    pub fn of(rgba: &[u8], width: usize, height: usize, crop: &CropSettings, max_samples: usize) -> Histogram {
        Scopes::of(rgba, width, height, crop, 0, max_samples).histogram
    }

    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    pub fn peak(&self) -> u32 {
        let h = sys::negs_histogram_t { red: self.red, green: self.green, blue: self.blue };
        unsafe { sys::negs_histogram_peak(&h) }
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
        let n = self.width * 256;
        let mut pixels = vec![[0u8; 4]; n];
        // Bins shorter than width x 256 (a hand-made Waveform) draw nothing.
        if n > 0 {
            let len = self.red.len().min(self.green.len()).min(self.blue.len());
            unsafe { sys::negs_waveform_raster(self.width, self.red.as_ptr(), self.green.as_ptr(), self.blue.as_ptr(), len, pixels.as_mut_ptr(), n) };
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
        Self::shown(rgba, width, height, crop, Orientation::new(rotation, false), max_samples)
    }

    /// As `of`, the waveform across the photograph mirrored too if
    /// `orientation.flip`, before its quarter turns.
    pub fn shown(rgba: &[u8], width: usize, height: usize, crop: &CropSettings, orientation: Orientation, max_samples: usize) -> Scopes {
        let c = crop.c();
        let o = orientation.c();
        if rgba.len() < width.wrapping_mul(height).wrapping_mul(4) {
            return Scopes::default();
        }
        let wave_width = unsafe { sys::negs_scopes_wave_width(width, height, &c, o, max_samples) };
        if wave_width == 0 {
            return Scopes::default();
        }
        let n = wave_width * 256;
        let (mut red, mut green, mut blue) = (vec![0u32; n], vec![0u32; n], vec![0u32; n]);
        let mut h = sys::negs_histogram_t::default();
        let samples = unsafe {
            sys::negs_scopes_count(rgba.as_ptr(), rgba.len(), width, height, &c, o, max_samples, &mut h, red.as_mut_ptr(), green.as_mut_ptr(), blue.as_mut_ptr(), n)
        };
        Scopes {
            histogram: Histogram { red: h.red, green: h.green, blue: h.blue },
            waveform: Waveform { width: wave_width, red, green, blue, samples },
        }
    }
}
