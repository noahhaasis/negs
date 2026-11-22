// The border rules' own tests (core/src/rules/border), with
// windows/src/border.rs's values. core/diff/tests/border.rs holds them to
// the Rust and the engine. Run by `python3 core/build.py test`.
package border_tests

import "core:testing"
import border "../../../src/rules/border"

style :: proc(s: border.Style) -> border.Settings {
	b := border.DEFAULT_SETTINGS
	b.enabled = true
	b.style = s
	return b
}

@(test)
geometry_matches_the_exporter :: proc(t: ^testing.T) {
	testing.expect_value(t, border.canvas(style(.Keyline), 1000, 500), [2]u32{1068, 568})
	testing.expect_value(t, border.keyline(style(.Keyline), 1000, 500), 4)
	testing.expect_value(t, border.canvas(style(.Film), 6000, 4000), [2]u32{6024, 4024})
	testing.expect_value(t, border.film_geometry(6000, 4000)[1], 100)
	b := style(.Solid)
	b.width = 0.1
	testing.expect_value(t, border.canvas(b, 200, 100), [2]u32{240, 140})
	b = style(.Film)
	b.film_width = 0.05
	testing.expect_value(t, border.canvas(b, 600, 400), [2]u32{662, 462})
}

@(test)
instagram_canvas_matches_the_exporter :: proc(t: ^testing.T) {
	testing.expect_value(t, border.instagram(style(.Keyline), {1068, 568}), [2]u32{1068, 1424})
	testing.expect_value(t, border.instagram(style(.Solid), {1000, 2000}), [2]u32{1500, 2000})
	testing.expect_value(t, border.instagram(style(.Solid), {301, 400}), [2]u32{301, 402})
	testing.expect_value(t, border.instagram(style(.Film), {1000, 400}), [2]u32{1060, 1414})
	testing.expect_value(t, border.instagram(style(.Film35), {300, 400}), [2]u32{324, 432})
}

@(test)
colors_are_srgb_hex :: proc(t: ^testing.T) {
	hex := border.hex({255, 128, 0})
	testing.expect_value(t, string(hex[:]), "#ff8000")
	c, ok := border.from_hex("#FF8000").?
	testing.expect(t, ok && c == border.Color{255, 128, 0})
	_, bad := border.from_hex("#ff800").?
	testing.expect(t, !bad)
	_, plus := border.from_hex("#+1+2+3").?
	testing.expect(t, !plus)
}

@(test)
the_35mm_border_only_shows_on_film_frames :: proc(t: ^testing.T) {
	testing.expect(t, border.fits(.Film35, 6000, 4000))
	testing.expect(t, border.fits(.Film35, 3000, 4000))
	testing.expect(t, !border.fits(.Film35, 4000, 4000))
	testing.expect(t, border.fits(.Solid, 4000, 4000))
}

@(test)
numbers_as_rust_writes_them :: proc(t: ^testing.T) {
	cases := [?]struct {
		value: f64,
		text:  string,
	}{{0.03, "0.03"}, {1, "1"}, {-0.0, "-0"}, {1e21, "1000000000000000000000"}, {1e-7, "0.0000001"}, {0.30000000000000004, "0.30000000000000004"}}
	for c in cases {
		s := border.rust_display(c.value)
		defer delete(s)
		testing.expect_value(t, s, c.text)
	}
}

@(test)
exporter_arguments :: proc(t: ^testing.T) {
	b := style(.Keyline)
	b.width = 0.05
	b.line = 0.01
	b.color = border.BLACK
	b.line_color = border.WHITE
	b.instagram = true
	args := border.arguments(b)
	defer delete(args)
	want := "--border\x00keyline\x00--keyline-width\x000.01\x00--border-color\x00#000000\x00--keyline-color\x00#ffffff\x00--border-width\x000.05\x00--instagram\x00"
	testing.expect_value(t, args, want)
}
