//! The six built-in Negs themes (Darkroom, Contact sheet, Light table,
//! Daylight, Plain, Plain dark), their sRGB hex tokens, and WCAG contrast
//! helpers. Pure Rust with zero UI-toolkit or OS dependencies.

/// A built-in theme. Darkroom is the default. On Windows, Plain follows
/// Windows' light or dark setting; Plain dark is dark always.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeName {
    Darkroom,
    ContactSheet,
    LightTable,
    Daylight,
    Plain,
    PlainDark,
}

impl ThemeName {
    pub const ALL: [ThemeName; 6] = [Self::Darkroom, Self::ContactSheet, Self::LightTable, Self::Daylight, Self::Plain, Self::PlainDark];

    /// The stored name, as the macOS app's `AppTheme` raw value.
    pub fn key(self) -> &'static str {
        match self {
            Self::Darkroom => "darkroom",
            Self::ContactSheet => "contactSheet",
            Self::LightTable => "lightTable",
            Self::Daylight => "daylight",
            Self::Plain => "plain",
            Self::PlainDark => "plainDark",
        }
    }

    /// A stored name or a title, any case, with or without spaces.
    pub fn parse(text: &str) -> Option<Self> {
        let squashed: String = text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
        Self::ALL.into_iter().find(|t| t.key().to_ascii_lowercase() == squashed)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Darkroom => "Darkroom",
            Self::ContactSheet => "Contact sheet",
            Self::LightTable => "Light table",
            Self::Daylight => "Daylight",
            Self::Plain => "Plain",
            Self::PlainDark => "Plain dark",
        }
    }

    /// The line under the swatches in Settings › Look.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Darkroom => "Warm black, cream ink and the yellow pencil.",
            Self::ContactSheet => "Ivory paper and the red grease pencil; the strip stays black film.",
            Self::LightTable => "Lit white, graphite type and a yellow highlighter.",
            Self::Daylight => "Soft grey with an amber pencil, the calmest of the light ones.",
            Self::Plain => "Black on light grey in the system font, with plain blue marks. The easiest to read. The default when Windows is light.",
            Self::PlainDark => "Plain in dark greys: light words in the system font, blue marks. The default when Windows is dark.",
        }
    }

    pub fn tokens(self) -> &'static Tokens {
        match self {
            Self::Darkroom => &DARKROOM,
            Self::ContactSheet => &CONTACT_SHEET,
            Self::LightTable => &LIGHT_TABLE,
            Self::Daylight => &DAYLIGHT,
            Self::Plain => &PLAIN,
            Self::PlainDark => &PLAIN_DARK,
        }
    }
}

/// One theme's colours as 0xRRGGBB, sRGB (Swift's `Palette.Hex`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hex {
    /// The sheet the photograph lies on, and the title band.
    pub bg: u32,
    /// The inspector, Settings and the alert.
    pub panel: u32,
    /// The strip, the tray and the leader.
    pub film: u32,
    /// Behind the grid.
    pub grid: u32,
    /// Rules between sections and modules.
    pub line: u32,
    /// Slider track, outlines, separators, outlined buttons.
    pub track: u32,
    /// Values at their default, hints, secondary words.
    pub dim: u32,
    /// Control labels.
    pub label: u32,
    /// What's being read: names, changed values, the focus box.
    pub paper: u32,
    /// The pencil's thin marks: rings, dots, fills, lines. Never words.
    pub accent: u32,
    /// The Export and Done buttons, and the words on them.
    pub fill: u32,
    pub on_fill: u32,
    /// Errors and warnings, always as words.
    pub warn: u32,
    /// A tile not yet developed.
    pub blank: u32,
}

impl Hex {
    /// Every token with its Swift name, for swatches and checks.
    pub fn named(&self) -> [(&'static str, u32); 14] {
        [
            ("bg", self.bg),
            ("panel", self.panel),
            ("film", self.film),
            ("grid", self.grid),
            ("line", self.line),
            ("track", self.track),
            ("dim", self.dim),
            ("label", self.label),
            ("paper", self.paper),
            ("accent", self.accent),
            ("fill", self.fill),
            ("onFill", self.on_fill),
            ("warn", self.warn),
            ("blank", self.blank),
        ]
    }
}

/// A theme: its colours, the inks on its film, and how its marks are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub hex: Hex,
    /// Light ink on dark: menus and pop-ups are drawn dark.
    pub dark: bool,
    /// The pencil under a word is a highlighter swiped behind its lower
    /// half (Light table); strikes stay a pencil line in `accent`.
    pub highlighter: bool,
    /// The system font for the serif, and straight, upright pencil marks.
    pub plain: bool,
    /// The words and marks on the film (strip, tray, leader): the theme's
    /// own, except where the film is dark under light panels (Contact sheet).
    pub film: Hex,
}

/// The T2 · Label tokens (IMPLEMENTATION.md §2.1).
const DARKROOM_HEX: Hex = Hex {
    bg: 0x0c0b0a,
    panel: 0x161513,
    film: 0x000000,
    grid: 0x000000,
    line: 0x2b2925,
    track: 0x47443d,
    dim: 0x918a7c,
    label: 0xbeb6a7,
    paper: 0xf2ebdd,
    accent: 0xffd23f,
    fill: 0xffd23f,
    on_fill: 0x14120c,
    warn: 0xff8a66,
    blank: 0x1c1b18,
};

pub const DARKROOM: Tokens = Tokens { hex: DARKROOM_HEX, dark: true, highlighter: false, plain: false, film: DARKROOM_HEX };

/// Ivory paper and the red grease pencil of a Magnum contact sheet; the
/// strip stays black film, its inks Darkroom's with a brighter red.
pub const CONTACT_SHEET: Tokens = Tokens {
    hex: Hex {
        bg: 0xf7f3ea,
        panel: 0xe9e2d3,
        film: 0x0c0b0a,
        grid: 0xf7f3ea,
        line: 0xd6cdbb,
        track: 0xb3aa99,
        dim: 0x655c4e,
        label: 0x463f35,
        paper: 0x1d1a15,
        accent: 0xc63a26,
        fill: 0xc63a26,
        on_fill: 0xfff8ee,
        warn: 0x9a2f18,
        blank: 0xe2dccd,
    },
    dark: false,
    highlighter: false,
    plain: false,
    film: Hex { accent: 0xe2492f, ..DARKROOM_HEX },
};

const LIGHT_TABLE_HEX: Hex = Hex {
    bg: 0xffffff,
    panel: 0xededeb,
    film: 0xf3f3f1,
    grid: 0xf3f3f1,
    line: 0xdcdcd9,
    track: 0xbdbdba,
    dim: 0x666663,
    label: 0x41413f,
    paper: 0x151514,
    accent: 0xad7b00,
    fill: 0xffd23f,
    on_fill: 0x14120c,
    warn: 0xb4381e,
    blank: 0xe4e4e1,
};

/// A lit white table, graphite type; the yellow stays as a highlighter and
/// in the Export button, ochre where it's a thin line.
pub const LIGHT_TABLE: Tokens = Tokens { hex: LIGHT_TABLE_HEX, dark: false, highlighter: true, plain: false, film: LIGHT_TABLE_HEX };

const DAYLIGHT_HEX: Hex = Hex {
    bg: 0xe8e6e1,
    panel: 0xd8d5cf,
    film: 0xc9c6bf,
    grid: 0xc9c6bf,
    line: 0xc6c2bb,
    track: 0xa29e96,
    dim: 0x57524b,
    label: 0x3a3631,
    paper: 0x161411,
    accent: 0xa35000,
    fill: 0xa35000,
    on_fill: 0xffffff,
    warn: 0x912a16,
    blank: 0xd0cdc7,
};

/// Soft warm grey with an amber pencil.
pub const DAYLIGHT: Tokens = Tokens { hex: DAYLIGHT_HEX, dark: false, highlighter: false, plain: false, film: DAYLIGHT_HEX };

const PLAIN_HEX: Hex = Hex {
    bg: 0xf7f7f7,
    panel: 0xececec,
    film: 0xe2e2e2,
    grid: 0xf7f7f7,
    line: 0xd2d2d2,
    track: 0xa6a6a6,
    dim: 0x4a4a4a,
    label: 0x1d1d1d,
    paper: 0x000000,
    accent: 0x0058d0,
    fill: 0x0062d9,
    on_fill: 0xffffff,
    warn: 0xb0160a,
    blank: 0xe0e0e0,
};

/// Near native: neutral greys, near-black words (7 : 1 and up on every
/// ground), a plain blue mark and the system font.
pub const PLAIN: Tokens = Tokens { hex: PLAIN_HEX, dark: false, highlighter: false, plain: true, film: PLAIN_HEX };

const PLAIN_DARK_HEX: Hex = Hex {
    bg: 0x1e1e1e,
    panel: 0x282828,
    film: 0x141414,
    grid: 0x1e1e1e,
    line: 0x3a3a3a,
    track: 0x5e5e5e,
    dim: 0xb4b4b4,
    label: 0xdcdcdc,
    paper: 0xf5f5f5,
    accent: 0x0091ff,
    fill: 0x0059d1,
    on_fill: 0xffffff,
    warn: 0xff6961,
    blank: 0x2e2e2e,
};

/// Plain in dark greys: words 7 : 1 and up on every ground, a lighter blue
/// for marks, a readable dark red.
pub const PLAIN_DARK: Tokens = Tokens { hex: PLAIN_DARK_HEX, dark: true, highlighter: false, plain: true, film: PLAIN_DARK_HEX };

/// The highlighter's yellow and strength (Light table).
pub const HIGHLIGHTER_INK: u32 = 0xffd23f;
pub const HIGHLIGHTER_OPACITY: f32 = 0.85;
/// Text selection: the pencil at this opacity, never the system blue (§4.9).
pub const SELECTION_OPACITY: f32 = 0.32;
/// Outside the crop box the photograph darkens with this, at `CROP_SHADE_OPACITY` (§4.10).
pub const CROP_SHADE: u32 = 0x080706;
pub const CROP_SHADE_OPACITY: f32 = 0.6;
/// Disabled controls fade to this opacity (§2.1: 0.35–0.38).
pub const DISABLED_OPACITY: f32 = 0.36;
/// Strip frames other than the current one (§4.4).
pub const STRIP_DIM: f32 = 0.42;

impl Tokens {
    /// What dims the window under an alert: black at 0.28 on a dark theme, a
    /// warm brown at 0.12 on a light one (r28-light.html).
    pub fn dimming(&self) -> (u32, f32) {
        // sRGB 0.24, 0.16, 0.08.
        if self.dark { (0x000000, 0.28) } else { (0x3d2914, 0.12) }
    }
    /// Words on a dark badge laid over a photograph: Darkroom's `paper` in
    /// every theme, since the badge stays dark.
    pub fn on_photo(&self) -> u32 {
        DARKROOM.hex.paper
    }
    /// The strength of shadows under alerts and tags: softer on a light theme.
    pub fn shadow_strength(&self) -> f32 {
        if self.dark { 1.0 } else { 0.3 }
    }
}

/// The relative luminance of 0xRRGGBB (WCAG).
pub fn luminance(rgb: u32) -> f64 {
    let channel = |shift: u32| {
        let c = ((rgb >> shift) & 0xff) as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

pub fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Linearly blends two 24-bit packed colours (works identically for 0xRRGGBB or 0xBBGGRR).
pub fn mix(a: u32, b: u32, t: f64) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as f64, ((b >> shift) & 0xff) as f64);
        ((x + (y - x) * t).round() as u32) << shift
    };
    channel(0) | channel(8) | channel(16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_parse_and_contrast() {
        for theme in ThemeName::ALL {
            assert_eq!(ThemeName::parse(theme.key()), Some(theme));
            assert_eq!(ThemeName::parse(theme.title()), Some(theme));
            let t = theme.tokens();
            assert!(contrast(t.hex.paper, t.hex.panel) >= 7.0, "{theme:?}");
        }
        assert!(contrast(0xffffff, 0x000000) > 20.9);
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
    }
}
