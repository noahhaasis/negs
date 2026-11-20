// The built-in themes (round 28, design/proposals/IMPLEMENTATION-THEMES.md):
// their names, their colours as 0xRRGGBB sRGB tokens, the inks on their
// film, how their marks are drawn, and the WCAG contrast the tests hold
// them to. One table for every platform: macos/Sources/NegsApp/AppTheme.swift
// and windows/src/ui/theme.rs carried their own copies of it.
package theme

import "core:c/libc"
import "core:unicode"

Id :: enum u32 {
	Darkroom      = 0,
	Contact_Sheet = 1,
	Light_Table   = 2,
	Daylight      = 3,
	Plain         = 4,
	Plain_Dark    = 5,
}

Platform :: enum u32 {
	MacOS   = 0,
	Windows = 1,
	Linux   = 2,
}

// One theme's colours (Swift's Palette.Hex).
Hex :: struct {
	bg:      u32, // the sheet the photograph lies on, and the title band
	panel:   u32, // the inspector, Settings and the alert
	film:    u32, // the strip, the tray and the leader
	grid:    u32, // behind the grid
	line:    u32, // rules between sections and modules
	track:   u32, // slider track, outlines, separators, outlined buttons
	dim:     u32, // values at their default, hints, secondary words
	label:   u32, // control labels
	paper:   u32, // what's being read: names, changed values, the focus box
	accent:  u32, // the pencil's thin marks: rings, dots, fills, lines; never words
	fill:    u32, // the Export and Done buttons
	on_fill: u32, // and the words on them
	warn:    u32, // errors and warnings, always as words
	blank:   u32, // a tile not yet developed
}

Tokens :: struct {
	hex:         Hex,
	// The words and marks on the film (strip, tray, leader): the theme's own,
	// except where the film is dark under light panels (Contact sheet).
	film:        Hex,
	// Light ink on dark: menus and pop-overs are drawn dark.
	dark:        bool,
	// The pencil under a word is a highlighter swiped behind its lower half
	// (Light table); strikes stay a pencil line in `accent`.
	highlighter: bool,
	// The system font for the serif, and straight, upright marks (Plain).
	plain:       bool,
}

// The stored names, as the macOS app's AppTheme raw values.
KEYS := [Id]cstring {
	.Darkroom      = "darkroom",
	.Contact_Sheet = "contactSheet",
	.Light_Table   = "lightTable",
	.Daylight      = "daylight",
	.Plain         = "plain",
	.Plain_Dark    = "plainDark",
}

TITLES := [Id]cstring {
	.Darkroom      = "Darkroom",
	.Contact_Sheet = "Contact sheet",
	.Light_Table   = "Light table",
	.Daylight      = "Daylight",
	.Plain         = "Plain",
	.Plain_Dark    = "Plain dark",
}

// The line under the swatches in Settings › Look. The platforms' defaults
// differ (macOS: Darkroom; Windows: Plain or Plain dark, after the system),
// so the hints that name them do too. Linux has no theme choice yet.
HINTS := [Platform][Id]cstring {
	.MacOS = {
		.Darkroom      = "Warm black, cream ink and the yellow pencil. The default.",
		.Contact_Sheet = "Ivory paper and the red grease pencil; the strip stays black film.",
		.Light_Table   = "Lit white, graphite type and a yellow highlighter.",
		.Daylight      = "Soft grey with an amber pencil, the calmest of the light ones.",
		.Plain         = "Black on light grey in the system font, with plain blue marks. The easiest to read.",
		.Plain_Dark    = "Plain in macOS’s dark greys: light words in the system font, blue marks.",
	},
	.Windows = {
		.Darkroom      = "Warm black, cream ink and the yellow pencil.",
		.Contact_Sheet = "Ivory paper and the red grease pencil; the strip stays black film.",
		.Light_Table   = "Lit white, graphite type and a yellow highlighter.",
		.Daylight      = "Soft grey with an amber pencil, the calmest of the light ones.",
		.Plain         = "Black on light grey in the system font, with plain blue marks. The easiest to read. The default when Windows is light.",
		.Plain_Dark    = "Plain in dark greys: light words in the system font, blue marks. The default when Windows is dark.",
	},
	.Linux = {
		.Darkroom      = "Warm black, cream ink and the yellow pencil.",
		.Contact_Sheet = "Ivory paper and the red grease pencil; the strip stays black film.",
		.Light_Table   = "Lit white, graphite type and a yellow highlighter.",
		.Daylight      = "Soft grey with an amber pencil, the calmest of the light ones.",
		.Plain         = "Black on light grey in the system font, with plain blue marks. The easiest to read.",
		.Plain_Dark    = "Plain in dark greys: light words in the system font, blue marks.",
	},
}

// The T2 · Label tokens (IMPLEMENTATION.md §2.1), unchanged.
DARKROOM_HEX :: Hex {
	bg      = 0x0c0b0a,
	panel   = 0x161513,
	film    = 0x000000,
	grid    = 0x000000,
	line    = 0x2b2925,
	track   = 0x47443d,
	dim     = 0x918a7c,
	label   = 0xbeb6a7,
	paper   = 0xf2ebdd,
	accent  = 0xffd23f,
	fill    = 0xffd23f,
	on_fill = 0x14120c,
	warn    = 0xff8a66,
	blank   = 0x1c1b18,
}

// Ivory paper and the red grease pencil of a Magnum contact sheet; the strip
// stays black film, its inks Darkroom's with a brighter red.
CONTACT_SHEET_HEX :: Hex {
	bg      = 0xf7f3ea,
	panel   = 0xe9e2d3,
	film    = 0x0c0b0a,
	grid    = 0xf7f3ea,
	line    = 0xd6cdbb,
	track   = 0xb3aa99,
	dim     = 0x655c4e,
	label   = 0x463f35,
	paper   = 0x1d1a15,
	accent  = 0xc63a26,
	fill    = 0xc63a26,
	on_fill = 0xfff8ee,
	warn    = 0x9a2f18,
	blank   = 0xe2dccd,
}

// A lit white table, graphite type; the yellow stays as a highlighter and in
// the Export button, ochre where it's a thin line.
LIGHT_TABLE_HEX :: Hex {
	bg      = 0xffffff,
	panel   = 0xededeb,
	film    = 0xf3f3f1,
	grid    = 0xf3f3f1,
	line    = 0xdcdcd9,
	track   = 0xbdbdba,
	dim     = 0x666663,
	label   = 0x41413f,
	paper   = 0x151514,
	accent  = 0xad7b00,
	fill    = 0xffd23f,
	on_fill = 0x14120c,
	warn    = 0xb4381e,
	blank   = 0xe4e4e1,
}

// Soft warm grey with an amber pencil.
DAYLIGHT_HEX :: Hex {
	bg      = 0xe8e6e1,
	panel   = 0xd8d5cf,
	film    = 0xc9c6bf,
	grid    = 0xc9c6bf,
	line    = 0xc6c2bb,
	track   = 0xa29e96,
	dim     = 0x57524b,
	label   = 0x3a3631,
	paper   = 0x161411,
	accent  = 0xa35000,
	fill    = 0xa35000,
	on_fill = 0xffffff,
	warn    = 0x912a16,
	blank   = 0xd0cdc7,
}

// Near native: neutral greys, near-black words (7 : 1 and up on every
// ground), a plain blue mark and the system font.
PLAIN_HEX :: Hex {
	bg      = 0xf7f7f7,
	panel   = 0xececec,
	film    = 0xe2e2e2,
	grid    = 0xf7f7f7,
	line    = 0xd2d2d2,
	track   = 0xa6a6a6,
	dim     = 0x4a4a4a,
	label   = 0x1d1d1d,
	paper   = 0x000000,
	accent  = 0x0058d0,
	fill    = 0x0062d9,
	on_fill = 0xffffff,
	warn    = 0xb0160a,
	blank   = 0xe0e0e0,
}

// Plain in the system's dark greys: the sheet is macOS's dark window
// (1e1e1e), the panel a tone lighter as in Darkroom, the strip darker. Words
// 7 : 1 and up on every ground, the system blue (dark) for marks, Apple's
// readable dark red.
PLAIN_DARK_HEX :: Hex {
	bg      = 0x1e1e1e,
	panel   = 0x282828,
	film    = 0x141414,
	grid    = 0x1e1e1e,
	line    = 0x3a3a3a,
	track   = 0x5e5e5e,
	dim     = 0xb4b4b4,
	label   = 0xdcdcdc,
	paper   = 0xf5f5f5,
	accent  = 0x0091ff,
	fill    = 0x0059d1,
	on_fill = 0xffffff,
	warn    = 0xff6961,
	blank   = 0x2e2e2e,
}

// Contact sheet's film inks: Darkroom's, with a brighter red pencil.
CONTACT_SHEET_FILM :: Hex {
	bg      = DARKROOM_HEX.bg,
	panel   = DARKROOM_HEX.panel,
	film    = DARKROOM_HEX.film,
	grid    = DARKROOM_HEX.grid,
	line    = DARKROOM_HEX.line,
	track   = DARKROOM_HEX.track,
	dim     = DARKROOM_HEX.dim,
	label   = DARKROOM_HEX.label,
	paper   = DARKROOM_HEX.paper,
	accent  = 0xe2492f,
	fill    = DARKROOM_HEX.fill,
	on_fill = DARKROOM_HEX.on_fill,
	warn    = DARKROOM_HEX.warn,
	blank   = DARKROOM_HEX.blank,
}

TOKENS := [Id]Tokens {
	.Darkroom      = {hex = DARKROOM_HEX, film = DARKROOM_HEX, dark = true},
	.Contact_Sheet = {hex = CONTACT_SHEET_HEX, film = CONTACT_SHEET_FILM},
	.Light_Table   = {hex = LIGHT_TABLE_HEX, film = LIGHT_TABLE_HEX, highlighter = true},
	.Daylight      = {hex = DAYLIGHT_HEX, film = DAYLIGHT_HEX},
	.Plain         = {hex = PLAIN_HEX, film = PLAIN_HEX, plain = true},
	.Plain_Dark    = {hex = PLAIN_DARK_HEX, film = PLAIN_DARK_HEX, dark = true, plain = true},
}

// The highlighter's yellow and strength (Light table).
HIGHLIGHTER_INK :: 0xffd23f
HIGHLIGHTER_OPACITY :: 0.85
// Text selection: the pencil at this opacity, never the system blue (§4.9).
SELECTION_OPACITY :: 0.32
// Outside the crop box the photograph darkens with this (§4.10).
CROP_SHADE :: 0x080706
CROP_SHADE_OPACITY :: 0.6
// Disabled controls fade to this opacity (§2.1: 0.35–0.38).
DISABLED_OPACITY :: 0.36
// Strip frames other than the current one (§4.4).
STRIP_DIM :: 0.42
// What dims the window under an alert, as sRGB and opacity: black at 0.28
// on a dark theme, a warm brown at 0.12 on a light one (r28-light.html).
DIMMING_DARK :: [4]f64{0, 0, 0, 0.28}
DIMMING_LIGHT :: [4]f64{0.24, 0.16, 0.08, 0.12}
// The strength of shadows under alerts and tags: softer on a light theme.
SHADOW_DARK :: 1.0
SHADOW_LIGHT :: 0.3

// A stored name or a title, any case, with or without spaces (Windows'
// ThemeName::parse: the letters and digits, lowercased, against each key).
// A letter or figure past ASCII makes it unknown, as no key has one, except
// the Kelvin sign, which lowercases to k; anything else past ASCII is passed
// over. (Rust's is_alphanumeric also counts the few combining marks that are
// Other_Alphabetic; those are passed over here.)
parse :: proc(text: string) -> (Id, bool) {
	squashed: [64]u8
	n := 0
	for r in text {
		c := r
		if c == '\u212A' {
			c = 'k'
		}
		if c >= 'A' && c <= 'Z' {
			c += 'a' - 'A'
		}
		if (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') {
			if n == len(squashed) {
				return .Darkroom, false
			}
			squashed[n] = u8(c)
			n += 1
		} else if c > 0x7f && (unicode.is_letter(c) || unicode.is_number(c)) {
			return .Darkroom, false
		}
	}
	got := string(squashed[:n])
	for id in Id {
		key := string(KEYS[id])
		if len(key) != len(got) {
			continue
		}
		same := true
		for i in 0 ..< len(key) {
			k := key[i]
			if k >= 'A' && k <= 'Z' {
				k += 'a' - 'A'
			}
			if k != got[i] {
				same = false
				break
			}
		}
		if same {
			return id, true
		}
	}
	return .Darkroom, false
}

// The WCAG 2 relative luminance of an sRGB colour, 0…1.
luminance :: proc "contextless" (rgb: u32) -> f64 {
	channel :: proc "contextless" (rgb, shift: u32) -> f64 {
		c := f64((rgb >> shift) & 0xff) / 255.0
		return c / 12.92 if c <= 0.04045 else libc.pow((c + 0.055) / 1.055, 2.4)
	}
	return 0.2126 * channel(rgb, 16) + 0.7152 * channel(rgb, 8) + 0.0722 * channel(rgb, 0)
}

// The WCAG 2 contrast ratio between two sRGB colours, 1…21.
contrast :: proc "contextless" (a, b: u32) -> f64 {
	x, y := luminance(a), luminance(b)
	return (libc.fmax(x, y) + 0.05) / (libc.fmin(x, y) + 0.05)
}

// `a` moved towards `b` by `t`, each channel rounded half away from zero.
mix :: proc "contextless" (a, b: u32, t: f64) -> u32 {
	channel :: proc "contextless" (a, b, shift: u32, t: f64) -> u32 {
		x, y := f64((a >> shift) & 0xff), f64((b >> shift) & 0xff)
		v := libc.round(x + (y - x) * t)
		return (u32(v) if v > 0 else 0) << shift
	}
	return channel(a, b, 0, t) | channel(a, b, 8, t) | channel(a, b, 16, t)
}
