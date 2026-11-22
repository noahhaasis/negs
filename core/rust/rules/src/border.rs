//! windows/src/border.rs's API with its rules in the shared Odin core
//! (core/src/rules/border). Presentation borders: drawn around the
//! developed photograph, so changing one never re-renders.
//!
//! What runs where:
//! - In Odin: the styles' words, which border a photograph shows, the
//!   colour's hex, the canvas, Instagram's, the strip's turn, and the
//!   exporter's arguments. The engine's sizes (spektrafilm-io's `margin`,
//!   `keyline_width`, `film_geometry`, `instagram_canvas`,
//!   `FilmFormat::of` and the 35mm strip's canvas) are ported into the core
//!   and held to the engine bit for bit (core/diff/tests/border.rs).
//! - In Rust: the types and their serde (sidecars and sessions); the
//!   engine's `BorderStyle` and `FilmFormat`, re-exported; `options`, the
//!   helper's JSON (serde values for the engine's `BorderOptions`).
//!   `FilmStripTexture` (a PNG read with `image`) stays in
//!   windows/src/border.rs.

use negs_core_sys::picture as sys;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub use spektrafilm_io::border::{BorderStyle, FilmFormat};

use crate::crop::static_str;

pub const STYLES: [BorderStyle; 4] = [BorderStyle::Solid, BorderStyle::Film, BorderStyle::Keyline, BorderStyle::Film35];

/// The core's id (`NEGS_BORDER_*`).
fn id(style: BorderStyle) -> u32 {
    match style {
        BorderStyle::Solid => sys::NEGS_BORDER_SOLID,
        BorderStyle::Film => sys::NEGS_BORDER_FILM,
        BorderStyle::Keyline => sys::NEGS_BORDER_KEYLINE,
        BorderStyle::Film35 => sys::NEGS_BORDER_FILM35,
    }
}

pub fn title(style: BorderStyle) -> &'static str {
    static_str(unsafe { sys::negs_border_style_title(id(style)) })
}

pub fn help(style: BorderStyle) -> &'static str {
    static_str(unsafe { sys::negs_border_style_help(id(style)) })
}

pub fn raw_value(style: BorderStyle) -> &'static str {
    static_str(unsafe { sys::negs_border_style_raw_value(id(style)) })
}

/// Whether the style fits a photograph of `size`: the 35mm strip only holds
/// 3:2 and 3:4 photographs.
pub fn fits(style: BorderStyle, width: u32, height: u32) -> bool {
    unsafe { sys::negs_border_fits(id(style), width, height) != 0 }
}

/// An 8-bit sRGB colour, saved and sent to the exporter as "#rrggbb", so the
/// preview shows exactly what the exporter fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BorderColor(pub [u8; 3]);

impl BorderColor {
    pub const WHITE: Self = Self([255; 3]);
    pub const BLACK: Self = Self([0; 3]);

    pub fn from_hex(hex: &str) -> Option<Self> {
        let mut out = [0u8; 3];
        (unsafe { sys::negs_border_color_from_hex(hex.as_ptr(), hex.len(), &mut out) } != 0).then_some(Self(out))
    }

    pub fn hex(self) -> String {
        let mut out = [0u8; 8];
        unsafe { sys::negs_border_color_hex(&self.0, &mut out) };
        String::from_utf8_lossy(&out[..7]).into_owned()
    }

    /// Win32's COLORREF (0x00BBGGRR): the Windows app's own packing.
    pub fn colorref(self) -> u32 {
        self.0[0] as u32 | (self.0[1] as u32) << 8 | (self.0[2] as u32) << 16
    }

    pub fn from_colorref(c: u32) -> Self {
        Self([c as u8, (c >> 8) as u8, (c >> 16) as u8])
    }

    pub fn rgba(self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        unsafe { sys::negs_border_color_rgba(&self.0, &mut out) };
        out
    }
}

impl Serialize for BorderColor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.hex().serialize(s)
    }
}

impl<'de> Deserialize<'de> for BorderColor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(d)?;
        Self::from_hex(&hex).ok_or_else(|| serde::de::Error::custom(format!("Not a #rrggbb colour: {hex}")))
    }
}

/// Sessions saved before a setting existed restore it at its default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BorderSettings {
    pub enabled: bool,
    pub style: BorderStyle,
    pub export: bool,
    /// Extends the border to Instagram's 3:4 portrait: with the mat's colour
    /// around a mat, with white around film.
    pub instagram: bool,
    /// The solid and keyline mat on every side, a fraction of the
    /// photograph's long edge. Both styles share it, and the colour.
    pub width: f64,
    /// Black around the film frame, a fraction of the long edge.
    pub film_width: f64,
    /// The keyline's line, a fraction of the long edge.
    pub line: f64,
    pub color: BorderColor,
    pub line_color: BorderColor,
}

pub const DEFAULT_WIDTH: f64 = 0.03;
pub const DEFAULT_LINE: f64 = 0.004;

impl Default for BorderSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            style: BorderStyle::Solid,
            export: true,
            instagram: false,
            width: DEFAULT_WIDTH,
            film_width: 0.0,
            line: DEFAULT_LINE,
            color: BorderColor::WHITE,
            line_color: BorderColor::BLACK,
        }
    }
}

fn pair(f: impl FnOnce(*mut [u32; 2])) -> (u32, u32) {
    let mut out = [0u32; 2];
    f(&mut out);
    (out[0], out[1])
}

impl BorderSettings {
    fn c(&self) -> sys::negs_border_settings_t {
        sys::negs_border_settings_t {
            width: self.width,
            film_width: self.film_width,
            line: self.line,
            style: id(self.style),
            color: self.color.0,
            line_color: self.line_color.0,
            enabled: self.enabled as u8,
            export_border: self.export as u8,
            instagram: self.instagram as u8,
            reserved: [0; 3],
        }
    }

    /// The border around a photograph of `size`, if the style fits it.
    pub fn shown(&self, width: u32, height: u32) -> Option<BorderSettings> {
        (unsafe { sys::negs_border_shown(&self.c(), width, height) } != 0).then_some(*self)
    }

    pub fn exported(&self, width: u32, height: u32) -> Option<BorderSettings> {
        (unsafe { sys::negs_border_exported(&self.c(), width, height) } != 0).then_some(*self)
    }

    /// The style's width setting: the mat, or the black around film.
    pub fn style_width(&self) -> Option<f64> {
        let mut out = 0.0;
        (unsafe { sys::negs_border_style_width(&self.c(), &mut out) } != 0).then_some(out)
    }

    pub fn keyline(&self, width: u32, height: u32) -> u32 {
        unsafe { sys::negs_border_keyline(&self.c(), width, height) }
    }

    /// The canvas around a photograph of `width` × `height` pixels, as the
    /// exporter makes it. `film` picks the 35mm strip, whose edge print
    /// differs by film but whose size doesn't.
    pub fn canvas(&self, width: u32, height: u32, film: &str) -> (u32, u32) {
        let _ = film;
        pair(|out| unsafe { sys::negs_border_canvas(&self.c(), width, height, out) })
    }

    /// The 3:4 portrait canvas Instagram shows whole, around the bordered
    /// `canvas`.
    pub fn instagram_canvas(&self, canvas: (u32, u32)) -> (u32, u32) {
        pair(|out| unsafe { sys::negs_border_instagram(&self.c(), canvas.0, canvas.1, out) })
    }

    /// The whole document around a photograph: the border, extended for
    /// Instagram if asked.
    pub fn document(&self, width: u32, height: u32, film: &str) -> (u32, u32) {
        let _ = film;
        pair(|out| unsafe { sys::negs_border_document(&self.c(), width, height, out) })
    }

    /// The helper's `border_options`, as `BorderOptions` in
    /// `spektrafilm-io::border` reads them.
    pub fn options(&self) -> Value {
        let mut options = json!({"line": self.line, "color": self.color.hex(), "line_color": self.line_color.hex()});
        if let Some(width) = self.style_width() {
            options["width"] = json!(width);
        }
        options
    }

    /// The same options as `spektrafilm process` arguments.
    pub fn arguments(&self) -> Vec<String> {
        let bytes = negs_core_sys::take_buf(unsafe { sys::negs_border_arguments(&self.c()) });
        let mut arguments: Vec<String> = bytes.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        // The last NUL ends the last argument.
        arguments.pop();
        arguments
    }
}

/// Whether the strip turns to hold a `width` × `height` photograph.
pub fn strip_turned(film: &str, width: u32, height: u32) -> bool {
    let _ = film;
    unsafe { sys::negs_border_turned(width, height) != 0 }
}

/// A double as Rust's `{}` writes it, from the core (the arguments' numbers).
pub fn rust_display(v: f64) -> String {
    String::from_utf8_lossy(&negs_core_sys::take_buf(unsafe { sys::negs_rust_display_f64(v) })).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_styles_are_the_cores() {
        let mut ids = [0u32; 4];
        unsafe { sys::negs_border_styles(&mut ids) };
        assert_eq!(ids, STYLES.map(id));
        assert_eq!(STYLES.map(raw_value), ["white", "film", "keyline", "35mm"]);
        assert_eq!(STYLES.map(title), ["Solid", "Film", "Keyline", "35mm"]);
    }
}
