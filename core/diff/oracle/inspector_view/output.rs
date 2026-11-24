//! The export's choices the inspector shows (the Output tab's sliders and
//! the Export foot), moved here from `model.rs`, which re-exports them.

use crate::border::BorderSettings;

/// A reduction such as 68%; below 10% with one decimal.
pub fn percent(fraction: f64) -> String {
    format!("{:.*}%", if fraction < 0.1 { 1 } else { 0 }, fraction * 100.0)
}

/// The image format a save or export writes; the engine picks the encoder
/// from the file extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 3] = [OutputFormat::Png, OutputFormat::Jpeg, OutputFormat::Tiff];
    pub fn key(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
        }
    }
    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|f| f.key() == key).unwrap_or(Self::Png)
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Png => "PNG (lossless)",
            Self::Jpeg => "JPEG (smaller)",
            Self::Tiff => "TIFF (lossless)",
        }
    }
    pub fn pattern(self) -> &'static str {
        match self {
            Self::Png => "*.png",
            Self::Jpeg => "*.jpg;*.jpeg",
            Self::Tiff => "*.tif;*.tiff",
        }
    }
}

/// The colour spaces a save offers, as colour names them.
pub const COLOR_SPACES: [&str; 4] = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"];
/// ProPhoto RGB bands at 8 bits, so it needs 16; Adobe RGB stands in.
pub const PROPHOTO: usize = 3;
pub const PROPHOTO_FALLBACK: usize = 2;
pub const QUALITY_RANGE: (u32, u32) = (1, 100);

/// The colour space a save with `bit_depth` uses when `chosen` was picked
/// (`SaveSettings::effective_space`).
pub fn effective_space(chosen: usize, bit_depth: u32) -> usize {
    let chosen = chosen.min(COLOR_SPACES.len() - 1);
    if chosen == PROPHOTO && bit_depth == 8 { PROPHOTO_FALLBACK } else { chosen }
}

/// The three border width settings, in percent of the long edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderWidth {
    Mat,
    Line,
    Film,
}

impl BorderWidth {
    pub fn range(self) -> (f64, f64, f64) {
        match self {
            BorderWidth::Mat | BorderWidth::Film => (0.0, 15.0, 0.1),
            BorderWidth::Line => (0.05, 2.0, 0.05),
        }
    }
    pub fn get(self, b: &BorderSettings) -> f64 {
        match self {
            BorderWidth::Mat => b.width,
            BorderWidth::Line => b.line,
            BorderWidth::Film => b.film_width,
        }
    }
    pub fn set(self, b: &mut BorderSettings, value: f64) {
        match self {
            BorderWidth::Mat => b.width = value,
            BorderWidth::Line => b.line = value,
            BorderWidth::Film => b.film_width = value,
        }
    }
}
