//! Embedded Newsreader typography and all 6 Negs themes mapped from
//! `negs_app::theme::Tokens` to GPUI `Hsla` colors.

use std::borrow::Cow;

use gpui::{App, Hsla, SharedString, rgb};
use negs_app::theme::{ThemeName, Tokens, mix};

const NEWSREADER_REGULAR: &[u8] = include_bytes!("../../macos/Fonts/Newsreader-Negs.ttf");
const NEWSREADER_ITALIC: &[u8] = include_bytes!("../../macos/Fonts/Newsreader-Negs-Italic.ttf");

/// Internal TTF family name of `macos/Fonts/Newsreader-Negs*.ttf`.
pub const NEWSREADER_FAMILY: &str = "Newsreader 16pt";

/// Registers the embedded Newsreader regular and italic fonts with GPUI's text system.
pub fn register_fonts(cx: &App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(vec![
        Cow::Borrowed(NEWSREADER_REGULAR),
        Cow::Borrowed(NEWSREADER_ITALIC),
    ])
}

/// Converts a packed `0xRRGGBB` sRGB token into a GPUI `Hsla` color.
pub fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// Converts a packed `0xRRGGBB` sRGB token with explicit opacity into a GPUI `Hsla` color.
pub fn hex_alpha(value: u32, alpha: f32) -> Hsla {
    Hsla::from(rgb(value)).opacity(alpha)
}

/// Environment override (`NEGS_THEME`), matching `windows/src/ui/theme.rs`.
pub fn env_theme_override() -> Option<ThemeName> {
    let value = std::env::var("NEGS_THEME").ok()?;
    parse_theme_override(&value)
}

pub fn parse_theme_override(value: &str) -> Option<ThemeName> {
    match value.trim().to_ascii_lowercase().as_str() {
        "light" => Some(ThemeName::Plain),
        "dark" => Some(ThemeName::PlainDark),
        other => ThemeName::parse(other),
    }
}

/// Resolves the initial theme from `NEGS_THEME`, saved preferences, or Darkroom default.
pub fn initial_theme(saved: Option<&str>) -> ThemeName {
    if let Some(forced) = env_theme_override() {
        return forced;
    }
    saved
        .and_then(ThemeName::parse)
        .unwrap_or(ThemeName::Darkroom)
}

/// Cycles to the next theme in `ThemeName::ALL`.
pub fn next_theme(current: ThemeName) -> ThemeName {
    let idx = ThemeName::ALL
        .iter()
        .position(|&t| t == current)
        .unwrap_or(0);
    ThemeName::ALL[(idx + 1) % ThemeName::ALL.len()]
}

/// All GPUI colors and typography choices derived from a `negs_app::theme::ThemeName`.
#[derive(Clone, Debug)]
pub struct GpuiTheme {
    pub name: ThemeName,
    pub tokens: Tokens,
    /// Sheet behind the photograph and titlebar ground (`tokens.hex.bg`).
    pub canvas: Hsla,
    /// Panel/status/titlebar surface (`tokens.hex.panel`).
    pub panel: Hsla,
    /// Film strip / dark tray surface (`tokens.hex.film`).
    pub film: Hsla,
    /// Contact sheet grid ground (`tokens.hex.grid`).
    pub grid: Hsla,
    /// Hairline dividers (`tokens.hex.line`).
    pub line: Hsla,
    /// Control borders and tracks (`tokens.hex.track`).
    pub track: Hsla,
    /// Secondary/dim copy (`tokens.hex.dim`).
    pub dim: Hsla,
    /// Standard control labels (`tokens.hex.label`).
    pub label: Hsla,
    /// Primary readable ink (`tokens.hex.paper`).
    pub paper: Hsla,
    /// Grease-pencil accent (`tokens.hex.accent`).
    pub accent: Hsla,
    /// Primary filled button background (`tokens.hex.fill`).
    pub fill: Hsla,
    /// Text on filled primary buttons (`tokens.hex.on_fill`).
    pub on_fill: Hsla,
    /// Warnings & error text (`tokens.hex.warn`).
    pub warn: Hsla,
    /// Placeholder well (`tokens.hex.blank`).
    pub blank: Hsla,
    /// Subtle hover fill over `panel`/`canvas`.
    pub subtle_hover: Hsla,
    /// Active/pressed subtle fill.
    pub subtle_active: Hsla,
    /// Dark badge overlay over photographs (always readable).
    pub badge_bg: Hsla,
    /// Text on dark photo badges (`tokens.on_photo()`).
    pub on_photo: Hsla,
    /// Primary editorial serif family (Newsreader unless Plain theme).
    pub serif_family: SharedString,
    /// UI sans family.
    pub ui_family: SharedString,
}

impl GpuiTheme {
    pub fn from_name(name: ThemeName) -> Self {
        let tokens = *name.tokens();
        let h = tokens.hex;
        let subtle_hover = hex(mix(h.panel, h.paper, if tokens.dark { 0.08 } else { 0.06 }));
        let subtle_active = hex(mix(h.panel, h.paper, if tokens.dark { 0.14 } else { 0.11 }));
        let ui_family = SharedString::from(system_ui_font());
        let serif_family = if tokens.plain {
            ui_family.clone()
        } else {
            SharedString::from(NEWSREADER_FAMILY)
        };
        Self {
            name,
            tokens,
            canvas: hex(h.bg),
            panel: hex(h.panel),
            film: hex(h.film),
            grid: hex(h.grid),
            line: hex(h.line),
            track: hex(h.track),
            dim: hex(h.dim),
            label: hex(h.label),
            paper: hex(h.paper),
            accent: hex(h.accent),
            fill: hex(h.fill),
            on_fill: hex(h.on_fill),
            warn: hex(h.warn),
            blank: hex(h.blank),
            subtle_hover,
            subtle_active,
            badge_bg: hex_alpha(0x0c0b0a, 0.78),
            on_photo: hex(tokens.on_photo()),
            serif_family,
            ui_family,
        }
    }
}

fn system_ui_font() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        ".AppleSystemUIFont"
    }
    #[cfg(target_os = "windows")]
    {
        "Segoe UI"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "sans-serif"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_six_themes_construct_and_cycle() {
        for name in ThemeName::ALL {
            let t = GpuiTheme::from_name(name);
            assert_eq!(t.name, name);
            if t.tokens.plain {
                assert_eq!(t.serif_family, t.ui_family);
            } else {
                assert_eq!(t.serif_family.as_ref(), NEWSREADER_FAMILY);
            }
        }
        let mut cur = ThemeName::Darkroom;
        for _ in 0..ThemeName::ALL.len() {
            cur = next_theme(cur);
        }
        assert_eq!(cur, ThemeName::Darkroom);
    }

    #[test]
    fn env_override_parsing_matches_windows_shell() {
        assert_eq!(parse_theme_override("light"), Some(ThemeName::Plain));
        assert_eq!(parse_theme_override("DARK"), Some(ThemeName::PlainDark));
        assert_eq!(parse_theme_override("contact sheet"), Some(ThemeName::ContactSheet));
        assert_eq!(parse_theme_override("lightTable"), Some(ThemeName::LightTable));
        assert_eq!(parse_theme_override("daylight"), Some(ThemeName::Daylight));
        assert_eq!(parse_theme_override("darkroom"), Some(ThemeName::Darkroom));
        assert_eq!(parse_theme_override("unknown"), None);
    }
}
