//! Presentation borders. They are drawn around the developed photograph, so
//! changing one never re-renders. Every size comes from
//! `spektrafilm-io::border` and its 35mm strip, the code the exporter adds
//! the same border with. Mirrors `Border.swift` in the macOS app.
//!
//! The shared core's (core/src/rules/border, through negs_core_rules): the
//! settings and their serde stay Rust, the rules and the engine's sizes
//! run in Odin (core/diff/tests/border.rs). The 35mm strip's texture stays
//! here: it is image loading.

use std::path::Path;

pub use negs_core_rules::border::*;

// What the tests below name from the Rust this module was.
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use spektrafilm_io::border::film_geometry;

/// A 35mm strip texture from `data/borders/35mm`, laid out with the strip
/// horizontal. The exporter draws the same strip at full resolution.
pub struct FilmStripTexture {
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
        Some(Self { image })
    }
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
