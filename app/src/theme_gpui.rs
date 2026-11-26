//! Embedded Newsreader typography, editorial type scale, tabular figures,
//! Plain-theme substitution, and all 6 Negs themes mapped from
//! `negs_app::theme::Tokens` to GPUI `Hsla` colors.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use gpui::{App, Font, FontFeatures, FontStyle, FontWeight, Hsla, SharedString, rgb};
use negs_app::theme::{HIGHLIGHTER_INK, HIGHLIGHTER_OPACITY, ThemeName, Tokens, mix};

const NEWSREADER_REGULAR: &[u8] = include_bytes!("../../macos/Fonts/Newsreader-Negs.ttf");
const NEWSREADER_ITALIC: &[u8] = include_bytes!("../../macos/Fonts/Newsreader-Negs-Italic.ttf");

/// Internal TTF family name of `macos/Fonts/Newsreader-Negs*.ttf`.
pub const NEWSREADER_FAMILY: &str = "Newsreader 16pt";

/// Plain-theme size multiplier for system UI sans replacing Newsreader (`L2`,
/// matching SwiftUI `Theme.swift:192` and Win32 `d2d.rs:412`).
pub const PLAIN_SCALE: f32 = 0.95;

static DETECTED_UI_FONT: OnceLock<&'static str> = OnceLock::new();

/// Editorial type scale matching SwiftUI (`Theme.swift`, `InkControls.swift`)
/// and Win32 (`ink.rs`, `titlebar.rs`, `view_switch.rs`) 1:1 (`L4`).
#[allow(dead_code)]
pub mod scale {
    pub const TAB: f32 = 21.0;
    pub const EXPORT_BUTTON: f32 = 18.0;
    pub const VIEW_SWITCH: f32 = 17.0;
    pub const MODULE_TITLE: f32 = 16.0;
    pub const TITLE_NAME: f32 = 15.0;
    pub const VALUE: f32 = 14.5;
    pub const WORD_BUTTON: f32 = 14.0;
    pub const EDIT_COUNT: f32 = 14.0;
    pub const EXPORT_SUMMARY: f32 = 14.0;
    pub const OUTLINED_BUTTON: f32 = 13.5;
    pub const HINT: f32 = 13.0;
    pub const ROW_LABEL: f32 = 12.0;
    pub const PANE_HEADER: f32 = 12.0;
    pub const MENU_BAR: f32 = 12.0;
    pub const SECTION_HEAD: f32 = 11.0;
    pub const SECTION_TRACKING_EM: f32 = 0.13;
    pub const SCOPE_WORD: f32 = 11.0;
    pub const STATUS: f32 = 11.0;
    pub const TAB_COUNT: f32 = 10.5;
    pub const SCOPE_NOTE: f32 = 10.0;
}

/// Computes per-gap tracking so a medium-weight tab word matches the bold
/// word's width without shifting neighbors (fixes Win32 `ink::tab_width`'s
/// off-by-one `n` vs `n - 1` multiplier).
#[allow(dead_code)]
pub fn tab_tracking(bold_width: f32, medium_width: f32, chars: usize) -> f32 {
    if chars <= 1 {
        return 0.0;
    }
    ((bold_width - medium_width) / (chars - 1) as f32).max(0.0)
}

/// OpenType tabular + lining figures (`tnum` + `lnum`, `L5`), so numbers in
/// sliders, captions, and readouts never jitter horizontally while changing.
pub fn tabular_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![
        ("tnum".to_string(), 1),
        ("lnum".to_string(), 1),
    ]))
}

/// Registers the embedded Newsreader regular and italic fonts with GPUI's text
/// system, and detects Windows 11's `Segoe UI Variable` when available (`L5`).
pub fn register_fonts(cx: &App) -> anyhow::Result<()> {
    let res = cx.text_system().add_fonts(vec![
        Cow::Borrowed(NEWSREADER_REGULAR),
        Cow::Borrowed(NEWSREADER_ITALIC),
    ]);
    #[cfg(target_os = "windows")]
    {
        let names = cx.text_system().all_font_names();
        let chosen = if names.iter().any(|n| n.eq_ignore_ascii_case("Segoe UI Variable")) {
            "Segoe UI Variable"
        } else if names.iter().any(|n| n.eq_ignore_ascii_case("Segoe UI Variable Text")) {
            "Segoe UI Variable Text"
        } else {
            "Segoe UI"
        };
        let _ = DETECTED_UI_FONT.set(chosen);
    }
    res
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

/// Resolves the initial theme from `NEGS_THEME`, saved preferences, or Darkroom default (`L1`).
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

/// Computes the GPUI `Font` and effective size for an editorial serif text role,
/// applying Plain-theme substitution (`0.95×` size, upright, weight-remapped, `L2`)
/// and `tnum` + `lnum` OpenType features (`L5`).
pub fn serif_font_spec(
    plain: bool,
    ui_family: &SharedString,
    size: f32,
    weight: f32,
    italic: bool,
) -> (Font, f32) {
    let features = tabular_features();
    if plain {
        let mapped = if weight >= 650.0 {
            FontWeight::SEMIBOLD
        } else if weight >= 500.0 {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        };
        let font = Font {
            family: ui_family.clone(),
            features,
            fallbacks: None,
            weight: mapped,
            style: FontStyle::Normal,
        };
        (font, size * PLAIN_SCALE)
    } else {
        let font = Font {
            family: SharedString::from(NEWSREADER_FAMILY),
            features,
            fallbacks: None,
            weight: FontWeight(weight.clamp(400.0, 700.0)),
            style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        };
        (font, size)
    }
}

/// All GPUI colors and typography choices derived from a `negs_app::theme::ThemeName`.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct GpuiTheme {
    pub name: ThemeName,
    pub tokens: Tokens,
    /// Sheet behind the photograph and titlebar ground (`tokens.hex.bg`).
    pub canvas: Hsla,
    /// Panel/inspector/Settings surface (`tokens.hex.panel`).
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
    /// Film-strip / tray inks (`tokens.film.*`; darkroom cream + red accent on Contact Sheet).
    pub film_line: Hsla,
    pub film_track: Hsla,
    pub film_dim: Hsla,
    pub film_label: Hsla,
    pub film_paper: Hsla,
    pub film_accent: Hsla,
    /// Light Table highlighter ink (`#ffd23f @ 0.85`).
    pub highlighter_ink: Hsla,
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
    /// Shared OpenType tabular + lining features (`tnum` + `lnum`).
    pub tabular_features: FontFeatures,
}

impl GpuiTheme {
    pub fn from_name(name: ThemeName) -> Self {
        let tokens = *name.tokens();
        let h = tokens.hex;
        let f = tokens.film;
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
            film_line: hex(f.line),
            film_track: hex(f.track),
            film_dim: hex(f.dim),
            film_label: hex(f.label),
            film_paper: hex(f.paper),
            film_accent: hex(f.accent),
            highlighter_ink: hex_alpha(HIGHLIGHTER_INK, HIGHLIGHTER_OPACITY),
            subtle_hover,
            subtle_active,
            badge_bg: hex_alpha(0x0c0b0a, 0.78),
            on_photo: hex(tokens.on_photo()),
            serif_family,
            ui_family,
            tabular_features: tabular_features(),
        }
    }

    /// Editorial serif font + size (automatically applying Plain-theme `0.95×`,
    /// upright, weight-remapped substitution and `tnum`+`lnum` features).
    pub fn serif_spec(&self, size: f32, weight: f32, italic: bool) -> (Font, f32) {
        serif_font_spec(self.tokens.plain, &self.ui_family, size, weight, italic)
    }

    /// System UI sans font with tabular + lining figures (`tnum` + `lnum`).
    pub fn ui_font(&self, weight: FontWeight) -> Font {
        Font {
            family: self.ui_family.clone(),
            features: self.tabular_features.clone(),
            fallbacks: None,
            weight,
            style: FontStyle::Normal,
        }
    }
}

pub fn system_ui_font() -> &'static str {
    if let Some(detected) = DETECTED_UI_FONT.get() {
        return detected;
    }
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

/// Synchronizes macOS `NSWindow.appearance` (and `NSApp.appearance`) with the
/// chosen Negs theme (`L6`, fixing SwiftUI's missing appearance sync so native
/// scrollbars, sheets, and dialogs match Darkroom vs Light Table).
#[cfg(target_os = "macos")]
pub fn sync_window_appearance(window: &gpui::Window, dark: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::{CStr, c_char, c_void};

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    unsafe {
        type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
        type Msg1 = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
        type MsgVoid1 = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);
        type MsgStr = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void;

        let msg0: Msg0 = std::mem::transmute(objc_msgSend as *const ());
        let msg1: Msg1 = std::mem::transmute(objc_msgSend as *const ());
        let msg_void1: MsgVoid1 = std::mem::transmute(objc_msgSend as *const ());
        let msg_str: MsgStr = std::mem::transmute(objc_msgSend as *const ());

        let ns_string_cls = objc_getClass(c"NSString".as_ptr());
        let sel_utf8 = sel_registerName(c"stringWithUTF8String:".as_ptr());
        let name: &CStr = if dark {
            c"NSAppearanceNameDarkAqua"
        } else {
            c"NSAppearanceNameAqua"
        };
        let ns_name = msg_str(ns_string_cls, sel_utf8, name.as_ptr());
        if ns_name.is_null() {
            return;
        }
        let ns_appearance_cls = objc_getClass(c"NSAppearance".as_ptr());
        let sel_named = sel_registerName(c"appearanceNamed:".as_ptr());
        let appearance = msg1(ns_appearance_cls, sel_named, ns_name);
        if appearance.is_null() {
            return;
        }

        let sel_set_app = sel_registerName(c"setAppearance:".as_ptr());
        let ns_app_cls = objc_getClass(c"NSApplication".as_ptr());
        let sel_shared = sel_registerName(c"sharedApplication".as_ptr());
        let ns_app = msg0(ns_app_cls, sel_shared);
        if !ns_app.is_null() {
            msg_void1(ns_app, sel_set_app, appearance);
        }

        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
                let ns_view = appkit.ns_view.as_ptr();
                let sel_window = sel_registerName(c"window".as_ptr());
                let ns_window = msg0(ns_view, sel_window);
                if !ns_window.is_null() {
                    msg_void1(ns_window, sel_set_app, appearance);
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn sync_window_appearance(_window: &gpui::Window, _dark: bool) {}

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
    fn first_launch_defaults_to_darkroom() {
        assert_eq!(initial_theme(None), ThemeName::Darkroom);
        assert_eq!(initial_theme(Some("")), ThemeName::Darkroom);
        assert_eq!(initial_theme(Some("lighttable")), ThemeName::LightTable);
    }

    #[test]
    fn plain_theme_scales_strips_italic_and_remaps_weights() {
        let darkroom = GpuiTheme::from_name(ThemeName::Darkroom);
        let (f, sz) = darkroom.serif_spec(17.0, 400.0, true);
        assert_eq!(f.family.as_ref(), NEWSREADER_FAMILY);
        assert_eq!(f.style, FontStyle::Italic);
        assert_eq!(f.weight, FontWeight(400.0));
        assert_eq!(sz, 17.0);
        assert_eq!(
            f.features.tag_value_list(),
            &[("tnum".to_string(), 1), ("lnum".to_string(), 1)]
        );

        let plain = GpuiTheme::from_name(ThemeName::Plain);
        let (pf, psz) = plain.serif_spec(17.0, 400.0, true);
        assert_eq!(pf.family, plain.ui_family);
        assert_eq!(pf.style, FontStyle::Normal, "Plain drops italic");
        assert_eq!(pf.weight, FontWeight::NORMAL);
        assert!((psz - 17.0 * 0.95).abs() < 1e-4);

        assert_eq!(plain.serif_spec(21.0, 700.0, false).0.weight, FontWeight::SEMIBOLD);
        assert_eq!(plain.serif_spec(15.0, 650.0, false).0.weight, FontWeight::SEMIBOLD);
        assert_eq!(plain.serif_spec(21.0, 500.0, false).0.weight, FontWeight::MEDIUM);
        assert_eq!(plain.serif_spec(14.5, 400.0, false).0.weight, FontWeight::NORMAL);
    }

    #[test]
    fn contact_sheet_exposes_dark_film_inks() {
        let cs = GpuiTheme::from_name(ThemeName::ContactSheet);
        assert_eq!(cs.film_accent, hex(0xe2492f));
        assert_eq!(cs.film_paper, hex(0xf2ebdd));
    }

    #[test]
    fn tab_tracking_uses_exact_gap_count() {
        assert_eq!(tab_tracking(44.0, 41.0, 4), 1.0); // 3 gaps between 4 chars
        assert_eq!(tab_tracking(40.0, 42.0, 4), 0.0);
        assert_eq!(tab_tracking(44.0, 41.0, 1), 0.0);
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
