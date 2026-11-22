// Presentation borders (windows/src/border.rs, Border.swift): drawn around
// the developed photograph, so changing one never re-renders. The styles'
// words, the colour's hex, which border a photograph shows, and the canvas
// the exporter makes around it.
package border

import "core:strings"

STYLES := [4]Style{.Solid, .Film, .Keyline, .Film35}

TITLES := [Style]cstring {
	.Solid   = "Solid",
	.Film    = "Film",
	.Keyline = "Keyline",
	.Film35  = "35mm",
}

HELPS := [Style]cstring {
	.Solid   = "A plain mat around the photograph.",
	.Film    = "A scanned 120 film frame with an irregular black edge.",
	.Keyline = "A mat with a thin line around the photograph.",
	.Film35  = "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph.",
}

// The saved and exported names.
RAW_VALUES := [Style]cstring {
	.Solid   = "white",
	.Film    = "film",
	.Keyline = "keyline",
	.Film35  = "35mm",
}

// Whether the style fits a photograph of `width` × `height`: the 35mm
// strip only holds 3:2 and 3:4 photographs.
fits :: proc "contextless" (style: Style, width, height: u32) -> bool {
	if style != .Film35 {
		return true
	}
	_, ok := format_of(width, height).?
	return width > 0 && height > 0 && ok
}

// An 8-bit sRGB colour.
Color :: [3]u8

WHITE :: Color{255, 255, 255}
BLACK :: Color{0, 0, 0}

DEFAULT_WIDTH: f64 : 0.03
DEFAULT_LINE: f64 : 0.004

// Laid out as negs_border_settings_t.
Settings :: struct {
	// The solid and keyline mat on every side, a fraction of the long edge.
	width:      f64,
	// Black around the film frame, a fraction of the long edge.
	film_width: f64,
	// The keyline's line, a fraction of the long edge.
	line:       f64,
	style:      Style,
	color:      Color,
	line_color: Color,
	enabled:    bool,
	export:     bool,
	// Extends the border to Instagram's 3:4 portrait.
	instagram:  bool,
	reserved:   [3]u8,
}

DEFAULT_SETTINGS :: Settings {
	style      = .Solid,
	export     = true,
	width      = DEFAULT_WIDTH,
	line       = DEFAULT_LINE,
	color      = WHITE,
	line_color = BLACK,
}

// The border around a photograph of `width` × `height`, if the style fits it.
shown :: proc "contextless" (s: Settings, width, height: u32) -> bool {
	return s.enabled && fits(s.style, width, height)
}

exported :: proc "contextless" (s: Settings, width, height: u32) -> bool {
	return s.export && shown(s, width, height)
}

// The style's width setting: the mat, or the black around film.
style_width :: proc "contextless" (s: Settings) -> Maybe(f64) {
	switch s.style {
	case .Solid, .Keyline:
		return s.width
	case .Film:
		return s.film_width
	case .Film35:
		return nil
	}
	return nil
}

keyline :: proc "contextless" (s: Settings, width, height: u32) -> u32 {
	return keyline_width(width, height, s.line)
}

// The canvas around a photograph of `width` × `height` pixels, as the
// exporter makes it.
canvas :: proc "contextless" (s: Settings, width, height: u32) -> [2]u32 {
	inset: u32
	switch s.style {
	case .Solid:
		inset = margin(width, height, s.width)
	case .Keyline:
		inset = margin(width, height, s.width) + keyline(s, width, height)
	case .Film:
		inset = film_geometry(width, height)[0] + margin(width, height, s.film_width)
	case .Film35:
		if format, ok := format_of(width, height).?; ok {
			return strip_canvas(format, width, height)
		}
		return {width, height}
	}
	return {width + 2 * inset, height + 2 * inset}
}

// The 3:4 portrait canvas Instagram shows whole, around the bordered canvas.
instagram :: proc "contextless" (s: Settings, c: [2]u32) -> [2]u32 {
	return instagram_canvas(c[0], c[1], s.style)
}

// The whole document around a photograph: the border, extended for
// Instagram if asked.
document :: proc "contextless" (s: Settings, width, height: u32) -> [2]u32 {
	c := canvas(s, width, height)
	return instagram(s, c) if s.instagram else c
}

// Whether the 35mm strip turns to hold a `width` × `height` photograph.
turned :: proc "contextless" (width, height: u32) -> bool {
	format, ok := format_of(width, height).?
	return ok && strip_turned(format, width, height)
}

// "#rrggbb", lower case.
hex :: proc "contextless" (c: Color) -> [7]u8 {
	DIGITS := "0123456789abcdef"
	out: [7]u8
	out[0] = '#'
	for v, i in c {
		out[1 + 2 * i] = DIGITS[v >> 4]
		out[2 + 2 * i] = DIGITS[v & 15]
	}
	return out
}

@(private = "file")
hex_value :: proc "contextless" (b: u8) -> (u8, bool) {
	switch b {
	case '0' ..= '9':
		return b - '0', true
	case 'a' ..= 'f':
		return b - 'a' + 10, true
	case 'A' ..= 'F':
		return b - 'A' + 10, true
	}
	return 0, false
}

// "#rrggbb" or "rrggbb", either case.
from_hex :: proc "contextless" (text: string) -> Maybe(Color) {
	digits := text[1:] if len(text) > 0 && text[0] == '#' else text
	if len(digits) != 6 {
		return nil
	}
	c: Color
	for i in 0 ..< 3 {
		hi, ok1 := hex_value(digits[2 * i])
		lo, ok2 := hex_value(digits[2 * i + 1])
		if !ok1 || !ok2 {
			return nil
		}
		c[i] = hi << 4 | lo
	}
	return c
}

rgba :: proc "contextless" (c: Color) -> [4]f32 {
	return {f32(c[0]) / 255.0, f32(c[1]) / 255.0, f32(c[2]) / 255.0, 1.0}
}

// The options as `spektrafilm process` arguments, each NUL-terminated, the
// numbers as Rust's `{}` writes them (the helper's own formatting).
arguments :: proc(s: Settings, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 128, allocator)
	arg :: proc(b: ^strings.Builder, text: string) {
		strings.write_string(b, text)
		strings.write_byte(b, 0)
	}
	arg(&b, "--border")
	arg(&b, string(RAW_VALUES[s.style]))
	arg(&b, "--keyline-width")
	write_rust_display(&b, s.line)
	strings.write_byte(&b, 0)
	color, line_color := hex(s.color), hex(s.line_color)
	arg(&b, "--border-color")
	arg(&b, string(color[:]))
	arg(&b, "--keyline-color")
	arg(&b, string(line_color[:]))
	if width, ok := style_width(s).?; ok {
		arg(&b, "--border-width")
		write_rust_display(&b, width)
		strings.write_byte(&b, 0)
	}
	if s.instagram {
		arg(&b, "--instagram")
	}
	return strings.to_string(b)
}
