//! The sheet the photograph lies on in the viewer and the inks the canvas
//! draws with on it (`ViewerSheet.swift`, design/proposals/r27-sheets.html),
//! and the light under a print whose edge sinks into the sheet
//! (`LightTable.swift`, r24 idea 3). Chosen in Settings › Look › Viewer.
//!
//! The title band, the inspector, the strip and Settings follow the theme;
//! only the canvas follows the sheet: its words (caption, label, the empty
//! window's), the trim marks, crop's dial and the pencil.

use std::cell::RefCell;
use std::collections::HashMap;

use super::theme::{self, ThemeName, Tokens, contrast, luminance};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ViewerSheet {
    /// The theme's `bg` (the default).
    #[default]
    Theme,
    /// Darkroom's `bg`, whichever theme is chosen.
    Darkroom,
    Black,
    Charcoal,
    Grey22,
    MiddleGrey,
    LightGrey,
    Paper,
}

impl ViewerSheet {
    pub const ALL: [ViewerSheet; 8] =
        [Self::Theme, Self::Darkroom, Self::Black, Self::Charcoal, Self::Grey22, Self::MiddleGrey, Self::LightGrey, Self::Paper];

    /// The choice as stored, the Mac's `viewerSheet` raw values.
    pub fn key(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Darkroom => "darkroom",
            Self::Black => "black",
            Self::Charcoal => "charcoal",
            Self::Grey22 => "grey22",
            Self::MiddleGrey => "middleGrey",
            Self::LightGrey => "lightGrey",
            Self::Paper => "paper",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|s| s.key() == key).unwrap_or_default()
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Theme => "Theme’s own",
            Self::Darkroom => "Darkroom",
            Self::Black => "Black",
            Self::Charcoal => "Charcoal",
            Self::Grey22 => "22% grey",
            Self::MiddleGrey => "Middle grey",
            Self::LightGrey => "Light grey",
            Self::Paper => "Paper",
        }
    }

    /// 0xRRGGBB, sRGB, under `tokens` (the theme shown).
    pub fn hex_in(self, tokens: &Tokens) -> u32 {
        match self {
            Self::Theme => tokens.hex.bg,
            Self::Darkroom => theme::DARKROOM.hex.bg,
            Self::Black => 0x000000,
            // Lightroom's and Photoshop's Dark Gray, about.
            Self::Charcoal => 0x242424,
            // 22% in Lightroom's readout.
            Self::Grey22 => 0x383838,
            // The 18% grey card, L* 50: ISO 3664's surround for judging prints.
            Self::MiddleGrey => 0x777777,
            // Lightroom's Light Gray, about.
            Self::LightGrey => 0xb4b4b4,
            Self::Paper => theme::DARKROOM.hex.paper,
        }
    }

    /// The sheet nearest a custom viewer background from before the sheets
    /// (Settings' old colour well), so it carries over.
    pub fn nearest(rgb: [u8; 3]) -> Self {
        let hex = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
        let distance = |s: &ViewerSheet| {
            let h = s.hex_in(&theme::DARKROOM);
            (0..3).map(|i| {
                let d = ((h >> (8 * i)) & 0xff) as i32 - ((hex >> (8 * i)) & 0xff) as i32;
                d * d
            }).sum::<i32>()
        };
        Self::ALL[2..].iter().copied().min_by_key(distance).unwrap_or_default()
    }
}

thread_local! {
    static CHOSEN: std::cell::Cell<ViewerSheet> = const { std::cell::Cell::new(ViewerSheet::Theme) };
    static INKS: RefCell<HashMap<(u32, u32, bool), SheetInk>> = RefCell::new(HashMap::new());
}

/// The sheet chosen in Settings.
pub fn chosen() -> ViewerSheet {
    CHOSEN.with(|c| c.get())
}

pub fn set_chosen(sheet: ViewerSheet) {
    CHOSEN.with(|c| c.set(sheet));
}

/// The sheet the settings hold: the stored choice, or for settings from
/// before sheets, the sheet nearest the custom background when it was on.
pub fn from_prefs(prefs: &crate::session::Preferences) -> ViewerSheet {
    if !prefs.viewer_sheet.is_empty() {
        return ViewerSheet::from_key(&prefs.viewer_sheet);
    }
    prefs.custom_viewer_background().map_or(ViewerSheet::Theme, ViewerSheet::nearest)
}

/// The canvas's sheet and inks now: the chosen sheet under the theme
/// shown. High contrast keeps the theme's own.
pub fn ink() -> SheetInk {
    let t = theme::tokens();
    let sheet = if theme::current().high_contrast { ViewerSheet::Theme } else { chosen() };
    let hex = sheet.hex_in(&t);
    INKS.with(|inks| *inks.borrow_mut().entry((hex, t.hex.bg, t.highlighter)).or_insert_with(|| SheetInk::new(hex, &t)))
}

/// The inks the canvas draws with on a sheet, 0xRRGGBB.
///
/// Each role keeps its warm hue and chroma in OKLCH and moves only in
/// lightness, to the contrast with the sheet that it has on `bg`. A sheet
/// that can't reach `paper`'s 16.6 : 1 squeezes the ramp in log-contrast,
/// with words held at 4.5 : 1 where the sheet allows it. Light ink when
/// white stands out from the sheet more than black does. The pencil stays
/// yellow while it holds 3 : 1; on a light sheet it turns the red of a
/// grease pencil. On a theme's own `bg` the inks are that theme's exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetInk {
    pub sheet: u32,
    pub paper: u32,
    pub label: u32,
    pub dim: u32,
    pub track: u32,
    pub line: u32,
    pub accent: u32,
    /// Light ink, on a dark sheet.
    pub light: bool,
    /// The mark under a word is the theme's highlighter.
    pub highlighter: bool,
}

/// Each role's contrast with `bg`, OKLCH chroma and hue, and whether it's words.
const ROLES: [(f64, f64, f64, bool); 5] =
    [(16.58, 0.020, 85.0, true), (9.78, 0.023, 83.0, true), (5.74, 0.022, 85.0, true), (2.02, 0.012, 88.0, false), (1.35, 0.008, 85.0, false)];
/// The grease pencil's red.
const GREASE: u32 = 0xc83122;
const WORDS_FLOOR: f64 = 4.5;
const MARK_FLOOR: f64 = 3.0;

impl SheetInk {
    pub fn new(sheet: u32, palette: &Tokens) -> Self {
        let own = if palette.hex.bg == sheet {
            Some(*palette)
        } else {
            ThemeName::ALL.iter().map(|t| *t.tokens()).find(|t| t.hex.bg == sheet)
        };
        if let Some(own) = own {
            let t = own.hex;
            let accent = if contrast(sheet, palette.hex.accent) >= MARK_FLOOR { palette.hex.accent } else { t.accent };
            return Self {
                sheet,
                paper: t.paper,
                label: t.label,
                dim: t.dim,
                track: t.track,
                line: t.line,
                accent,
                light: own.dark,
                highlighter: palette.highlighter && !own.dark,
            };
        }
        let light = contrast(sheet, 0xffffff) >= contrast(sheet, 0x000000);
        let reach = contrast(sheet, if light { 0xffffff } else { 0x000000 });
        let squeeze = (reach.ln() / ROLES[0].0.ln()).min(1.0);
        let mut inks = [0u32; 5];
        for (ink, &(target, chroma, hue, words)) in inks.iter_mut().zip(ROLES.iter()) {
            let mut target = (target.ln() * squeeze).exp();
            if words {
                target = target.max(WORDS_FLOOR.min(reach * 0.995));
            }
            *ink = oklch::solve(sheet, target, chroma, hue, light);
        }
        let darkroom = theme::DARKROOM.hex.accent;
        let accent = if contrast(sheet, palette.hex.accent) >= MARK_FLOOR {
            palette.hex.accent
        } else if contrast(sheet, darkroom) >= MARK_FLOOR {
            darkroom
        } else if light {
            oklch::solve(sheet, MARK_FLOOR, 0.162, 91.0, true)
        } else if contrast(sheet, GREASE) >= MARK_FLOOR {
            GREASE
        } else {
            oklch::solve(sheet, MARK_FLOOR, 0.19, 30.0, false)
        };
        Self {
            sheet,
            paper: inks[0],
            label: inks[1],
            dim: inks[2],
            track: inks[3],
            line: inks[4],
            accent,
            light,
            highlighter: palette.highlighter && !light,
        }
    }
}

/// Björn Ottosson's OKLab in polar form, for inks that keep their hue.
mod oklch {
    use super::contrast;

    fn linear(c: f64) -> f64 {
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    fn encoded(c: f64) -> f64 {
        if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
    }

    /// OKLab's lightness of an sRGB colour, 0…1.
    pub fn lightness(hex: u32) -> f64 {
        let c = |shift: u32| linear(((hex >> shift) & 0xff) as f64 / 255.0);
        let (r, g, b) = (c(16), c(8), c(0));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s
    }

    /// Linear sRGB, or None outside its gamut.
    fn linear_rgb(lightness: f64, chroma: f64, hue: f64) -> Option<(f64, f64, f64)> {
        let (a, b) = (chroma * (hue * std::f64::consts::PI / 180.0).cos(), chroma * (hue * std::f64::consts::PI / 180.0).sin());
        let cube = |x: f64| x * x * x;
        let l = cube(lightness + 0.3963377774 * a + 0.2158037573 * b);
        let m = cube(lightness - 0.1055613458 * a - 0.0638541728 * b);
        let s = cube(lightness - 0.0894841775 * a - 1.2914855480 * b);
        let rgb = (
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        );
        [rgb.0, rgb.1, rgb.2].iter().all(|&v| v > -1e-4 && v < 1.0 + 1e-4).then_some(rgb)
    }

    /// The colour at `lightness` and `hue`, with `chroma` or as much as sRGB holds.
    pub fn color(lightness: f64, chroma: f64, hue: f64) -> u32 {
        let byte = |c: f64| ((encoded(c).clamp(0.0, 1.0) * 255.0).round()) as u32;
        let mut c = chroma;
        loop {
            if let Some((r, g, b)) = linear_rgb(lightness, c, hue) {
                return byte(r) << 16 | byte(g) << 8 | byte(b);
            }
            if c < 1e-4 {
                let grey = byte(lightness * lightness * lightness);
                return grey << 16 | grey << 8 | grey;
            }
            c *= 0.85;
        }
    }

    /// The colour nearest the sheet in lightness, lighter or darker, that
    /// reaches `target` contrast with it.
    pub fn solve(sheet: u32, target: f64, chroma: f64, hue: f64, light: bool) -> u32 {
        let (mut low, mut high) = if light { (lightness(sheet), 1.0) } else { (0.0, lightness(sheet)) };
        for _ in 0..40 {
            let mid = (low + high) / 2.0;
            let reaches = contrast(sheet, color(mid, chroma, hue)) >= target;
            if light {
                if reaches { high = mid } else { low = mid }
            } else if reaches {
                low = mid
            } else {
                high = mid
            }
        }
        color(if light { high } else { low }, chroma, hue)
    }
}

// MARK: The light table

/// A soft light under the print, like a light box under negatives, when the
/// print's outer edge would sink into the sheet: a film border or a dark mat
/// on a dark sheet. On a light sheet a light edge gets a soft shade instead.
pub mod light_table {
    use super::*;

    /// The light comes on below this WCAG contrast between edge and sheet.
    pub const MINIMUM_CONTRAST: f64 = 1.5;
    /// How far the light moves the sheet's colour toward Darkroom's `paper`.
    pub const STRENGTH: f64 = 0.07;
    /// How far the shade moves a light sheet's colour toward black.
    pub const SHADE_STRENGTH: f64 = 0.16;
    /// DIPs: the light reaches `SPREAD` past the print's edges before it
    /// fades out over `BLUR`.
    pub const SPREAD: f64 = 14.0;
    pub const BLUR: f64 = 26.0;
    pub const CORNER_RADIUS: f64 = 10.0;

    /// The colour of the print's outermost pixels: None without a border
    /// (the edge is the photograph's own).
    pub fn edge(border: Option<&crate::border::BorderSettings>) -> Option<u32> {
        use crate::border::BorderStyle;
        let border = border?;
        Some(match border.style {
            BorderStyle::Solid | BorderStyle::Keyline => {
                let [r, g, b] = border.color.0;
                (r as u32) << 16 | (g as u32) << 8 | b as u32
            }
            // Instagram's extension puts the film borders on white.
            BorderStyle::Film | BorderStyle::Film35 => {
                if border.instagram { 0xffffff } else { 0x000000 }
            }
        })
    }

    /// The light under a print whose outer edge is `edge`, on `sheet`; None
    /// when the edge already stands out from the sheet.
    pub fn light(sheet: u32, edge: Option<u32>) -> Option<u32> {
        let edge = edge?;
        if contrast(sheet, edge) >= MINIMUM_CONTRAST {
            return None;
        }
        let (e, s) = (luminance(edge), luminance(sheet));
        let lights = if e == s { s < 0.18 } else { e < s };
        Some(if lights { mix(sheet, theme::DARKROOM.hex.paper, STRENGTH) } else { mix(sheet, 0x000000, SHADE_STRENGTH) })
    }

    pub fn mix(a: u32, b: u32, t: f64) -> u32 {
        let c = |hex: u32, shift: u32| ((hex >> shift) & 0xff) as f64 / 255.0;
        let byte = |shift: u32| (((c(a, shift) + (c(b, shift) - c(a, shift)) * t) * 255.0).round()) as u32;
        byte(16) << 16 | byte(8) << 8 | byte(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme's own sheet keeps that theme's inks exactly.
    #[test]
    fn a_themes_own_sheet_keeps_its_inks() {
        let ink = SheetInk::new(theme::DARKROOM.hex.bg, &theme::DARKROOM);
        let t = theme::DARKROOM.hex;
        assert_eq!((ink.paper, ink.label, ink.dim, ink.track, ink.accent), (t.paper, t.label, t.dim, t.track, t.accent));
        assert!(ink.light);
        // Daylight's sheet under Darkroom: Daylight's inks, Darkroom's yellow only if it holds 3 : 1.
        let day = SheetInk::new(theme::DAYLIGHT.hex.bg, &theme::DARKROOM);
        assert_eq!(day.paper, theme::DAYLIGHT.hex.paper);
        assert!(!day.light);
    }

    /// Greys get light or dark ink with the words' contrast, and the pencil
    /// turns grease-pencil red on a light sheet.
    #[test]
    fn greys_keep_their_words_readable() {
        for sheet in [ViewerSheet::Black, ViewerSheet::Charcoal, ViewerSheet::Grey22, ViewerSheet::MiddleGrey, ViewerSheet::LightGrey, ViewerSheet::Paper] {
            let hex = sheet.hex_in(&theme::DARKROOM);
            let ink = SheetInk::new(hex, &theme::DARKROOM);
            let reach = contrast(hex, if ink.light { 0xffffff } else { 0 });
            for words in [ink.paper, ink.label, ink.dim] {
                assert!(contrast(hex, words) >= WORDS_FLOOR.min(reach * 0.99) - 0.01, "{sheet:?} {words:06x}");
            }
            assert!(contrast(hex, ink.accent) >= MARK_FLOOR - 0.01, "{sheet:?}");
        }
        let paper = SheetInk::new(ViewerSheet::Paper.hex_in(&theme::DARKROOM), &theme::DARKROOM);
        assert!(!paper.light);
        assert_eq!(paper.accent, GREASE);
        let middle = SheetInk::new(0x777777, &theme::DARKROOM);
        assert!(contrast(0x777777, middle.paper) < 16.0);
    }

    /// The light comes on for a black film border on the dark sheet, not for
    /// a white mat; a light sheet shades a white mat.
    #[test]
    fn light_table_follows_the_edge_against_the_sheet() {
        let dark = theme::DARKROOM.hex.bg;
        assert_eq!(light_table::light(dark, Some(0x000000)), Some(light_table::mix(dark, theme::DARKROOM.hex.paper, 0.07)));
        assert_eq!(light_table::light(dark, Some(0xffffff)), None);
        assert_eq!(light_table::light(dark, None), None);
        assert_eq!(light_table::light(0xffffff, Some(0xffffff)), Some(light_table::mix(0xffffff, 0, 0.16)));
        // #333333 reads on Darkroom's bg (about 1.56); #222222 doesn't.
        assert_eq!(light_table::light(dark, Some(0x333333)), None);
        assert!(light_table::light(dark, Some(0x222222)).is_some());
    }

    #[test]
    fn keys_round_trip_and_old_backgrounds_carry_over() {
        for sheet in ViewerSheet::ALL {
            assert_eq!(ViewerSheet::from_key(sheet.key()), sheet);
        }
        assert_eq!(ViewerSheet::from_key("nonsense"), ViewerSheet::Theme);
        assert_eq!(ViewerSheet::nearest([0x24, 0x24, 0x24]), ViewerSheet::Charcoal);
        assert_eq!(ViewerSheet::nearest([0x80, 0x80, 0x80]), ViewerSheet::MiddleGrey);
    }
}
