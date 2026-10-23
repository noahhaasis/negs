//! Presentation borders. They are drawn around the developed photograph, so
//! changing one never re-renders. Every size comes from
//! `spektrafilm-io::border` and its 35mm strip, the code the exporter adds
//! the same border with. Mirrors `Border.swift` in the macOS app.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub use spektrafilm_io::border::{BorderStyle, FilmFormat};
use spektrafilm_io::border::{film_geometry, instagram_canvas, keyline_width, margin};
use spektrafilm_io::film35::Strip;

pub const STYLES: [BorderStyle; 4] = [BorderStyle::Solid, BorderStyle::Film, BorderStyle::Keyline, BorderStyle::Film35];

pub fn title(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Solid => "Solid",
        BorderStyle::Film => "Film",
        BorderStyle::Keyline => "Keyline",
        BorderStyle::Film35 => "35mm",
    }
}

pub fn help(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Solid => "A plain mat around the photograph.",
        BorderStyle::Film => "A scanned 120 film frame with an irregular black edge.",
        BorderStyle::Keyline => "A mat with a thin line around the photograph.",
        BorderStyle::Film35 => "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph.",
    }
}

pub fn raw_value(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Solid => "white",
        BorderStyle::Film => "film",
        BorderStyle::Keyline => "keyline",
        BorderStyle::Film35 => "35mm",
    }
}

/// Whether the style fits a photograph of `size`: the 35mm strip only holds
/// 3:2 and 3:4 photographs.
pub fn fits(style: BorderStyle, width: u32, height: u32) -> bool {
    style != BorderStyle::Film35 || (width > 0 && height > 0 && FilmFormat::of(width, height).is_some())
}

/// An 8-bit sRGB colour, saved and sent to the exporter as "#rrggbb", so the
/// preview shows exactly what the exporter fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BorderColor(pub [u8; 3]);

impl BorderColor {
    pub const WHITE: Self = Self([255; 3]);
    pub const BLACK: Self = Self([0; 3]);

    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits = hex.strip_prefix('#').unwrap_or(hex);
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(digits, 16).ok()?;
        Some(Self([(value >> 16) as u8, (value >> 8) as u8, value as u8]))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    pub fn colorref(self) -> u32 {
        self.0[0] as u32 | (self.0[1] as u32) << 8 | (self.0[2] as u32) << 16
    }

    pub fn from_colorref(c: u32) -> Self {
        Self([c as u8, (c >> 8) as u8, (c >> 16) as u8])
    }

    pub fn rgba(self) -> [f32; 4] {
        [self.0[0] as f32 / 255.0, self.0[1] as f32 / 255.0, self.0[2] as f32 / 255.0, 1.0]
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

impl BorderSettings {
    /// The border around a photograph of `size`, if the style fits it.
    pub fn shown(&self, width: u32, height: u32) -> Option<BorderSettings> {
        (self.enabled && fits(self.style, width, height)).then_some(*self)
    }

    pub fn exported(&self, width: u32, height: u32) -> Option<BorderSettings> {
        if self.export { self.shown(width, height) } else { None }
    }

    /// The style's width setting: the mat, or the black around film.
    pub fn style_width(&self) -> Option<f64> {
        match self.style {
            BorderStyle::Solid | BorderStyle::Keyline => Some(self.width),
            BorderStyle::Film => Some(self.film_width),
            BorderStyle::Film35 => None,
        }
    }

    pub fn keyline(&self, width: u32, height: u32) -> u32 {
        keyline_width(width, height, self.line)
    }

    /// The canvas around a photograph of `width` × `height` pixels, as the
    /// exporter makes it. `film` picks the 35mm strip.
    pub fn canvas(&self, width: u32, height: u32, film: &str) -> (u32, u32) {
        let inset = match self.style {
            BorderStyle::Solid => margin(width, height, self.width),
            BorderStyle::Keyline => margin(width, height, self.width) + self.keyline(width, height),
            BorderStyle::Film => film_geometry(width, height).0 + margin(width, height, self.film_width),
            BorderStyle::Film35 => {
                return match FilmFormat::of(width, height) {
                    Some(format) => Strip::new(film, format).canvas(width, height),
                    None => (width, height),
                };
            }
        };
        (width + 2 * inset, height + 2 * inset)
    }

    /// The 3:4 portrait canvas Instagram shows whole, around the bordered
    /// `canvas`.
    pub fn instagram_canvas(&self, canvas: (u32, u32)) -> (u32, u32) {
        instagram_canvas(canvas.0, canvas.1, self.style)
    }

    /// The whole document around a photograph: the border, extended for
    /// Instagram if asked.
    pub fn document(&self, width: u32, height: u32, film: &str) -> (u32, u32) {
        let canvas = self.canvas(width, height, film);
        if self.instagram { self.instagram_canvas(canvas) } else { canvas }
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
        let mut arguments: Vec<String> = vec![
            "--border".into(),
            raw_value(self.style).into(),
            "--keyline-width".into(),
            format!("{}", self.line),
            "--border-color".into(),
            self.color.hex(),
            "--keyline-color".into(),
            self.line_color.hex(),
        ];
        if let Some(width) = self.style_width() {
            arguments.extend(["--border-width".into(), format!("{width}")]);
        }
        if self.instagram {
            arguments.push("--instagram".into());
        }
        arguments
    }
}

/// A 35mm strip texture from `data/borders/35mm`, laid out with the strip
/// horizontal. The exporter draws the same strip at full resolution.
pub struct FilmStripTexture {
    pub film: String,
    pub format: FilmFormat,
    pub image: image::RgbaImage,
}

impl FilmStripTexture {
    /// The film's strip, or the format's generic one.
    pub fn load(data: &Path, film: &str, format: FilmFormat) -> Option<Self> {
        let dir = data.join("borders/35mm").join(format.name());
        let mut path = dir.join(format!("{film}.png"));
        if !path.exists() {
            path = dir.join("generic.png");
        }
        let image = image::open(&path).ok()?.to_rgba8();
        Some(Self { film: film.into(), format, image })
    }
}

/// Whether the strip turns to hold a `width` × `height` photograph.
pub fn strip_turned(film: &str, width: u32, height: u32) -> bool {
    FilmFormat::of(width, height).is_some_and(|format| Strip::new(film, format).turned(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(style: BorderStyle) -> BorderSettings {
        BorderSettings { style, ..Default::default() }
    }

    /// The same sizes as the exporter's tests in `spektrafilm-io::border`
    /// and the macOS app's `BorderTests`.
    #[test]
    fn geometry_matches_the_exporter() {
        assert_eq!(style(BorderStyle::Solid).canvas(200, 100, ""), (212, 112));
        assert_eq!(style(BorderStyle::Keyline).canvas(1000, 500, ""), (1068, 568));
        assert_eq!(style(BorderStyle::Keyline).keyline(1000, 500), 4);
        assert_eq!(style(BorderStyle::Film).canvas(6000, 4000, ""), (6024, 4024));
        assert_eq!(film_geometry(6000, 4000).1, 100);
        assert_eq!(BorderSettings { width: 0.1, ..style(BorderStyle::Solid) }.canvas(200, 100, ""), (240, 140));
        assert_eq!(BorderSettings { width: 0.01, line: 0.02, ..style(BorderStyle::Keyline) }.canvas(1000, 500, ""), (1060, 560));
        assert_eq!(BorderSettings { film_width: 0.05, ..style(BorderStyle::Film) }.canvas(600, 400, ""), (662, 462));
    }

    #[test]
    fn instagram_canvas_matches_the_exporter() {
        assert_eq!(style(BorderStyle::Keyline).instagram_canvas((1068, 568)), (1068, 1424));
        assert_eq!(style(BorderStyle::Solid).instagram_canvas((1000, 2000)), (1500, 2000));
        assert_eq!(style(BorderStyle::Solid).instagram_canvas((300, 400)), (300, 400));
        assert_eq!(style(BorderStyle::Solid).instagram_canvas((301, 400)), (301, 402));
        assert_eq!(style(BorderStyle::Film).instagram_canvas((1000, 400)), (1060, 1414));
        assert_eq!(style(BorderStyle::Film35).instagram_canvas((300, 400)), (324, 432));
    }

    #[test]
    fn settings_saved_before_instagram_restore() {
        let border: BorderSettings = serde_json::from_str(r#"{"enabled":true,"style":"film","export":false}"#).unwrap();
        assert_eq!(border, BorderSettings { enabled: true, style: BorderStyle::Film, export: false, ..Default::default() });
        let white: BorderSettings = serde_json::from_str(r#"{"enabled":true,"style":"white","export":true,"instagram":false}"#).unwrap();
        assert_eq!(white, BorderSettings { enabled: true, ..Default::default() });
        assert_eq!(white.width, 0.03);
        let keyline = BorderSettings { style: BorderStyle::Keyline, color: BorderColor([32, 64, 96]), ..Default::default() };
        let saved: BorderSettings = serde_json::from_str(&serde_json::to_string(&keyline).unwrap()).unwrap();
        assert_eq!(saved.color.hex(), "#204060");
        assert!(serde_json::to_string(&keyline).unwrap().contains(r##""lineColor":"#000000""##));
    }

    #[test]
    fn colors_are_srgb_hex() {
        assert_eq!(BorderColor::from_hex("#204060"), Some(BorderColor([0x20, 0x40, 0x60])));
        assert_eq!(BorderColor::from_hex("FF8000").unwrap().hex(), "#ff8000");
        assert_eq!(BorderColor::from_hex("#ff80"), None);
        assert_eq!(BorderColor::from_hex("#gg8000"), None);
        let c = BorderColor([12, 200, 99]);
        assert_eq!(BorderColor::from_colorref(c.colorref()), c);
    }

    /// The options the exporter reads; see `BorderOptions` in `spektrafilm-io::border`.
    #[test]
    fn exporter_options() {
        let keyline = BorderSettings {
            style: BorderStyle::Keyline,
            instagram: true,
            width: 0.05,
            line: 0.01,
            color: BorderColor::BLACK,
            line_color: BorderColor::WHITE,
            ..Default::default()
        };
        assert_eq!(keyline.options(), json!({"width": 0.05, "line": 0.01, "color": "#000000", "line_color": "#ffffff"}));
        assert_eq!(
            keyline.arguments(),
            ["--border", "keyline", "--keyline-width", "0.01", "--border-color", "#000000", "--keyline-color", "#ffffff", "--border-width", "0.05", "--instagram"]
        );
        assert_eq!(BorderSettings { film_width: 0.02, ..style(BorderStyle::Film) }.options()["width"], json!(0.02));
        assert!(style(BorderStyle::Film35).options().get("width").is_none());
        let parsed: spektrafilm_io::border::BorderOptions = serde_json::from_value(keyline.options()).unwrap();
        assert_eq!(parsed.width, Some(0.05));
    }

    #[test]
    fn the_35mm_border_only_shows_on_film_frames() {
        let border = BorderSettings { enabled: true, style: BorderStyle::Film35, ..Default::default() };
        assert_eq!(border.shown(6000, 4000).map(|b| b.style), Some(BorderStyle::Film35));
        assert!(border.shown(1920, 1080).is_none());
        assert!(border.exported(1920, 1080).is_none());
        let (cw, ch) = border.canvas(600, 400, "kodak_portra_400");
        assert!((cw as f64 / 600.0 - 38.0 / 36.0).abs() < 0.01 && (ch as f64 / 400.0 - 35.0 / 24.0).abs() < 0.01);
        assert_eq!(border.canvas(1920, 1080, "kodak_portra_400"), (1920, 1080));
    }

    #[test]
    fn strip_textures_load() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let strip = FilmStripTexture::load(&data, "not_a_film", FilmFormat::Full).unwrap();
        assert_eq!(strip.image.dimensions(), (3800, 3500));
    }
}
