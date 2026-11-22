// The tray rules' own tests (core/src/rules/tray), some of
// windows/src/tray_rules.rs's tests ported. The differential tests
// (core/diff/tests/tray_rules.rs) hold the rest to the Rust. Run by
// `python3 core/build.py test`.
package tray_tests

import "core:testing"
import tray "../../../src/rules/tray"

// Favourites (Portra, Trix), Color Negative (Gold, Portra, Ektar), Black &
// White (HP5, Trix): windows' `row()`.
Fixture :: struct {
	favourites: [2]tray.Entry,
	colour:     [3]tray.Entry,
	black:      [2]tray.Entry,
	sections:   [3]tray.Section,
}

fixture :: proc(f: ^Fixture) {
	f.favourites = {{"portra", "PORTRA"}, {"trix", "TRIX"}}
	f.colour = {{"gold", "GOLD"}, {"portra", "PORTRA"}, {"ektar", "EKTAR"}}
	f.black = {{"hp5", "HP5"}, {"trix", "TRIX"}}
	f.sections = {
		{tray.FAVOURITES, f.favourites[:], true},
		{"Color Negative", f.colour[:], false},
		{"Black & White", f.black[:], false},
	}
}

@(test)
favourites_lead_unless_searching :: proc(t: ^testing.T) {
	counts := [3]uint{3, 0, 2}
	kept: [3]uint
	n, lead := tray.sections(counts[:], 2, "", kept[:])
	testing.expect_value(t, n, 2)
	testing.expect(t, lead)
	testing.expect_value(t, [2]uint{kept[0], kept[1]}, [2]uint{0, 2})
	_, lead = tray.sections(counts[:], 2, "port", kept[:])
	testing.expect(t, !lead, "searching hides them")
	_, lead = tray.sections(counts[:], 2, " 　", kept[:])
	testing.expect(t, lead, "spaces aren't a search")
	_, lead = tray.sections(counts[:], 0, "", kept[:])
	testing.expect(t, !lead)
	testing.expect(t, tray.with_scan(false, false, ""))
	testing.expect(t, !tray.with_scan(true, false, ""), "papers")
	testing.expect(t, !tray.with_scan(false, true, ""), "read as")
}

@(test)
stepping_goes_through_the_favourites_and_on_into_the_groups :: proc(t: ^testing.T) {
	f: Fixture
	fixture(&f)
	places: [7]tray.Place
	testing.expect_value(t, tray.places(f.sections[:], places[:]), 7)
	stock :: proc(places: []tray.Place, i: int) -> (string, bool) {
		return places[i].stock, places[i].favourite
	}
	s, fav := stock(places[:], tray.step("portra", true, 1, places[:]))
	testing.expect(t, s == "trix" && fav)
	s, fav = stock(places[:], tray.step("trix", true, 1, places[:]))
	testing.expect(t, s == "gold" && !fav)
	s, fav = stock(places[:], tray.step("gold", false, -1, places[:]))
	testing.expect(t, s == "trix" && fav)
	s, _ = stock(places[:], tray.step("portra", false, 1, places[:]))
	testing.expect_value(t, s, "ektar")
	s, _ = stock(places[:], tray.step("velvia", false, 1, places[:]))
	testing.expect_value(t, s, "portra")
	testing.expect_value(t, tray.step("x", false, 1, nil), -1)
}

@(test)
the_row_is_laid_out_as_on_the_mac :: proc(t: ^testing.T) {
	f: Fixture
	fixture(&f)
	testing.expect_value(t, tray.width(2), 18.0 + 9.0 + 2.0 * 212.0 + 8.0)
	starts: [3]f64
	tray.starts(f.sections[:], starts[:])
	testing.expect_value(t, starts[0], tray.INSET)
	testing.expect_value(t, starts[1], tray.INSET + tray.width(2) + tray.GROUP_GAP)
	testing.expect_value(t, tray.content_width(nil), 2 * tray.INSET)
	x, ok := tray.tile_x("ektar", 1, f.sections[:])
	testing.expect(t, ok)
	testing.expect_value(t, x, starts[1] + 27.0 + 2.0 * 220.0)
	_, ok = tray.tile_x("ektar", 0, f.sections[:])
	testing.expect(t, !ok)
	testing.expect_value(t, tray.section_of("portra", f.sections[:], true), 0)
	testing.expect_value(t, tray.section_of("portra", f.sections[:], false), 1)
	testing.expect_value(t, tray.section_of("gold", f.sections[:], true), 1)
	testing.expect_value(t, tray.current(starts[1] - 40, starts[:]), 1)
	testing.expect_value(t, tray.current(starts[1] - 41, starts[:]), 0)

	left := tray.tile_left(starts[1], 1)
	kind, section, index := tray.hit(f.sections[:], left + 10, tray.TOP + 50)
	testing.expect_value(t, [3]uint{uint(kind), section, index}, [3]uint{uint(tray.Hit_Kind.Tile), 1, 1})
	kind, section, index = tray.hit(f.sections[:], left + tray.TILE - 10, tray.TOP + 10)
	testing.expect_value(t, [3]uint{uint(kind), section, index}, [3]uint{uint(tray.Hit_Kind.Star), 1, 1})
}

@(test)
the_wheel_scrolls_sideways_and_groups_glide :: proc(t: ^testing.T) {
	testing.expect_value(t, tray.wheel(200, 120, false, 600, 2000), 200 - tray.WHEEL_NOTCH)
	testing.expect_value(t, tray.wheel(200, -240, false, 600, 2000), 200 + 2 * tray.WHEEL_NOTCH)
	testing.expect_value(t, tray.wheel(200, 120, true, 600, 2000), 200 + tray.WHEEL_NOTCH)
	testing.expect_value(t, tray.wheel(10, 120, false, 600, 2000), 0.0)
	testing.expect_value(t, tray.wheel(1390, -120, false, 600, 2000), 1400.0)
	testing.expect_value(t, tray.glide(100, 500, 0), 100.0)
	testing.expect_value(t, tray.glide(100, 500, tray.GLIDE_MS / 2), 300.0)
	testing.expect_value(t, tray.glide(100, 500, 10 * tray.GLIDE_MS), 500.0)
	testing.expect_value(t, tray.clamp_offset(50, 600, 400), 0.0)
}

@(test)
screen_readers_hear_the_macs_words :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	testing.expect_value(t, tray.star_label("Portra 400", true, ta), "Remove Portra 400 from favourites")
	testing.expect_value(t, tray.star_label("Portra 400", false, ta), "Add Portra 400 to favourites")
	testing.expect_value(t, tray.tile_value(true, false), "Trying")
	testing.expect_value(t, tray.tile_value(false, true), "Before")
	testing.expect_value(t, tray.tile_value(true, true), "")
	testing.expect_value(t, tray.index_label("Black & White", ta), "Show Black & White")
	testing.expect_value(t, tray.no_match(" nothing ", false, ta), "No films match “nothing”")
	testing.expect_value(t, tray.matches(0, ta), "")
	testing.expect_value(t, tray.matches(1, ta), "1 match · return tries the first")
	testing.expect_value(t, tray.matches(3, ta), "3 matches · return tries the first")
}

@(test)
keeping_and_going_back :: proc(t: ^testing.T) {
	testing.expect_value(t, tray.visit_action(false, false), "Change Film")
	testing.expect_value(t, tray.visit_action(false, true), "Change Negative Film")
	testing.expect_value(t, tray.ending(true, true), tray.Ending.Nothing)
	testing.expect_value(t, tray.ending(true, false), tray.Ending.Step)
	testing.expect_value(t, tray.ending(false, false), tray.Ending.Restore)
	testing.expect_value(t, tray.submit(" "), tray.Submit.Keep)
	testing.expect_value(t, tray.cancel(""), tray.Cancel.Go_Back)
	testing.expect_value(t, tray.cancel(" "), tray.Cancel.Clear)
	action, step := tray.tray_key(.Left, false, false)
	testing.expect_value(t, action, tray.Tray_Key.Step)
	testing.expect_value(t, step, -1)
	action, _ = tray.tray_key(.Escape, true, false)
	testing.expect_value(t, action, tray.Tray_Key.Pass)
	testing.expect_value(t, tray.escape({tray = true, grid = true}), tray.Escape.Tray)
	testing.expect_value(t, tray.escape({}), tray.Escape.Nothing)
}
