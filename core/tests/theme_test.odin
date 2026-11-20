// The themes (ThemeTests.swift's rules, round 28): in every theme, words
// reach 4.5 : 1 and marks 3 : 1 on what they're read against; Export's words
// 4.5 : 1 on its fill; the panel stands apart from the sheet, a tone darker
// on a light theme; and the spec's own figures.
package tests

import "core:testing"
import "../src/theme"

@(test)
every_theme_meets_contrast :: proc(t: ^testing.T) {
	for id in theme.Id {
		tok := theme.TOKENS[id]
		h, f := tok.hex, tok.film
		check :: proc(t: ^testing.T, id: theme.Id, ink, bg: u32, floor: f64, what: string) {
			testing.expectf(t, theme.contrast(ink, bg) >= floor, "%v: %s %06x on %06x is %.2f", id, what, ink, bg, theme.contrast(ink, bg))
		}
		for ink in ([4]u32{h.dim, h.label, h.paper, h.warn}) {
			for bg in ([3]u32{h.panel, h.bg, h.grid}) {
				check(t, id, ink, bg, 4.5, "words")
			}
		}
		for ink in ([3]u32{f.dim, f.label, f.paper}) {
			check(t, id, ink, h.film, 4.5, "film words")
		}
		for bg in ([3]u32{h.panel, h.bg, h.grid}) {
			check(t, id, h.accent, bg, 3, "pencil")
		}
		check(t, id, f.accent, h.film, 3, "film pencil")
		check(t, id, h.on_fill, h.fill, 4.5, "Export's words")
		testing.expectf(t, h.panel != h.bg, "%v: the panel stands apart from the sheet", id)
		if !tok.dark {
			testing.expectf(t, theme.luminance(h.panel) < theme.luminance(h.bg), "%v: the panel is a tone darker", id)
		}
	}
	testing.expect(t, theme.TOKENS[.Darkroom].hex == theme.TOKENS[.Darkroom].film)
	testing.expect_value(t, theme.TOKENS[.Contact_Sheet].film.accent, 0xe2492f)
}

@(test)
the_specs_figures :: proc(t: ^testing.T) {
	d := theme.DARKROOM_HEX
	near :: proc(a, b: f64) -> bool {return abs(a - b) <= 0.1}
	testing.expect(t, near(theme.contrast(d.dim, d.panel), 5.3))
	testing.expect(t, near(theme.contrast(d.paper, d.bg), 16.6))
	testing.expect(t, near(theme.contrast(d.warn, d.panel), 7.9))
	testing.expect_value(t, theme.contrast(0xffffff, 0x000000), 21.0)
	testing.expect_value(t, theme.contrast(0x777777, 0x777777), 1.0)
	testing.expect_value(t, theme.mix(0x000000, 0xffffff, 0.5), 0x808080)
	testing.expect_value(t, theme.mix(0x102030, 0x102030, 0.3), 0x102030)
}

@(test)
names_parse_as_windows_reads_them :: proc(t: ^testing.T) {
	cases := [?]struct {
		text: string,
		id:   theme.Id,
		ok:   bool,
	} {
		{"darkroom", .Darkroom, true},
		{"Contact sheet", .Contact_Sheet, true},
		{"contactSheet", .Contact_Sheet, true},
		{"LIGHT-TABLE", .Light_Table, true},
		{"Plain dark", .Plain_Dark, true},
		{"plain dark", .Plain_Dark, true},
		{"plain", .Plain, true},
		{"neon", .Darkroom, false},
		{"", .Darkroom, false},
		{"plaín", .Darkroom, false},
	}
	for c in cases {
		id, ok := theme.parse(c.text)
		testing.expectf(t, ok == c.ok && (!ok || id == c.id), "%q: %v %v", c.text, id, ok)
	}
}
