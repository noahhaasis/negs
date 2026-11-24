// The rest of state.rs's own tests, on the core (core/src/state's Phase 4a
// rules): snapping, values, grain lanes, film formats, groups, the film
// search and Rust's fixed-point text. core/diff/tests/state_rules.rs holds
// every rule to the frozen Rust on random inputs. Run by `build.py test`.
package state_rules_tests

import "core:strings"
import "core:testing"
import "../../../src/state"
import "../../../src/value"

@(test)
snap_steps_and_clamps :: proc(t: ^testing.T) {
	testing.expect_value(t, state.snap(0.123456, 0.0, 2.0, 0.01), 0.12)
	testing.expect_value(t, state.snap(2.7, 1.0, 4.0, 1.0), 3.0)
	testing.expect_value(t, state.snap(99.0, -5.0, 5.0, 0.1), 5.0)
	testing.expect_value(t, state.snap(0.0251, 0.0, 0.2, 0.001), 0.025)
	testing.expect_value(t, state.snap(0.33, 0.1, 1.0, 0.05), 0.35)
	testing.expect_value(t, state.snap(-1.234, -1.5, 1.5, 0.01), -1.23)
}

expect_text :: proc(t: ^testing.T, got: string, want: string, loc := #caller_location) {
	testing.expect_value(t, got, want, loc = loc)
	delete(got)
}

@(test)
values_show_the_step_decimals :: proc(t: ^testing.T) {
	expect_text(t, state.format_value(0.5, 0.01), "0.5")
	expect_text(t, state.format_value(-0.0001, 0.1), "0")
	expect_text(t, state.format_value(12.0, 1.0), "12")
	expect_text(t, state.format_value(0.025, 0.001), "0.025")
}

@(test)
fixed_rounds_the_exact_value_to_even :: proc(t: ^testing.T) {
	expect_text(t, state.fixed(0.5, 0), "0")
	expect_text(t, state.fixed(1.5, 0), "2")
	expect_text(t, state.fixed(2.5, 0), "2")
	expect_text(t, state.fixed(-0.5, 0), "-0")
	expect_text(t, state.fixed(0.125, 2), "0.12")
	expect_text(t, state.fixed(0.375, 2), "0.38")
	expect_text(t, state.fixed(0.045, 2), "0.04")
	expect_text(t, state.fixed(-0.0001, 2), "-0.00")
	expect_text(t, state.fixed(1e21, 1), "1000000000000000000000.0")
	expect_text(t, state.fixed(5e-324, 6), "0.000000")
	big := state.fixed(1e300, 0)
	testing.expect(t, strings.has_prefix(big, "1000000000000000052504760255204420248704468581108159154915854115511802457988908195786371375080447864043704443832883878176942523235360430575644792184786706982848387200926575803737830233794788090059368953234970799945081119038967640880074652742780142494579258788820056842838115669472196386865459400540160"))
	testing.expect_value(t, len(big), 301)
	delete(big)
}

@(test)
grain_lanes_follow_the_stock_preset :: proc(t: ^testing.T) {
	field, lane, ok := state.grain_lane("film_render.grain.rms_granularity.2")
	testing.expect(t, ok && field == 0 && lane == 2)
	_, _, ok = state.grain_lane("film_render.grain.micro_structure.0")
	testing.expect(t, !ok)
	_, _, ok = state.grain_lane("film_render.grain.blur")
	testing.expect(t, !ok)
	testing.expect(t, state.color_only("film_render.grain.uniformity.1"))
	testing.expect(t, !state.color_only("film_render.grain.uniformity.0"))
	testing.expect(t, !state.color_only("film_render.grain.particle_scale_sublayers.2"))
	lanes := 0
	for g in state.ADJUSTMENT_GROUPS {
		for c in g.controls {
			if _, _, has := state.grain_lane(state.adjustment_path(c)); has {
				lanes += 1
			}
		}
	}
	testing.expect_value(t, lanes, 9)
}

@(test)
film_formats_read_back_from_the_long_edge :: proc(t: ^testing.T) {
	testing.expect_value(t, state.FILM_FORMATS[state.film_format(35.0)].name, "35 mm (135)")
	testing.expect_value(t, state.FILM_FORMATS[state.film_format(56.0)].name, "6×4.5 / 6×6")
	testing.expect_value(t, state.FILM_FORMATS[state.film_format(f64(f32(24.0)))].name, "Half frame")
	testing.expect_value(t, state.film_format(45.0), -1)
	testing.expect_value(t, state.film_format(36.0), -1)
}

stock :: proc(id, name: string, paper, positive, bw: bool, target := "") -> state.Stock {
	return state.Stock{id = id, name = name, paper = paper, positive = positive, bw = bw, target = target, manufacturer_data = true}
}

ids :: proc(groups: [dynamic]state.Stock_Group, stocks: []state.Stock) -> string {
	b := strings.builder_make(context.temp_allocator)
	for g in groups {
		for i in g.stocks {
			if strings.builder_len(b) > 0 {
				strings.write_byte(&b, ' ')
			}
			strings.write_string(&b, stocks[i].id)
		}
	}
	return strings.to_string(b)
}

@(test)
stocks_group_like_the_mac :: proc(t: ^testing.T) {
	films := []state.Stock {
		stock("gold", "Gold", false, false, false, "endura"),
		stock("vision", "Vision3", false, false, false, "print2383"),
		stock("velvia", "Velvia", false, true, false),
		stock("trix", "Tri-X", false, false, true),
	}
	papers := []state.Stock{stock("endura", "Endura", true, false, false), stock("print2383", "2383 Print Film", true, false, false)}
	g := state.film_groups(films, papers)
	defer state.groups_destroy(g)
	testing.expect_value(t, len(g), 4)
	testing.expect_value(t, g[2].title, "Motion Picture")
	p := state.paper_groups(papers)
	defer state.groups_destroy(p)
	testing.expect_value(t, len(p), 2)
	testing.expect_value(t, p[1].title, "Motion Picture Print")
	expect_text(t, state.option_title("some_new_option"), "Some New Option")
	expect_text(t, state.option_title("über_ßa"), "Über SSa")
	expect_text(t, state.ratio_name(1920.0, 1080.0), "16:9")
	expect_text(t, state.ratio_name(1000.0, 1300.0), "1.30:1")
}

@(test)
film_search_matches_every_word :: proc(t: ^testing.T) {
	films := []state.Stock {
		stock("kodak_portra_400", "Portra 400", false, false, false),
		stock("kodak_portra_160", "Portra 160", false, false, false),
		stock("fujifilm_velvia_100", "Velvia 100", false, true, false),
		stock("kodak_trix400_d76_8min", "Tri-X 400 (D-76, 8 min)", false, false, true),
	}
	search :: proc(query: string, films: []state.Stock) -> string {
		g := state.film_groups(films, nil)
		defer state.groups_destroy(g)
		state.matching(query, &g, films)
		return ids(g, films)
	}
	testing.expect_value(t, search("portra\t160", films), "kodak_portra_160")
	testing.expect_value(t, search("400 portra", films), "kodak_portra_400")
	testing.expect_value(t, search("kodak", films), "kodak_portra_400 kodak_portra_160 kodak_trix400_d76_8min")
	testing.expect_value(t, search("portra_400", films), "")
	testing.expect_value(t, search("&", films), "kodak_trix400_d76_8min")
	testing.expect_value(t, search("  ", films), "kodak_portra_400 kodak_portra_160 fujifilm_velvia_100 kodak_trix400_d76_8min")
	free_all(context.temp_allocator)
}

@(test)
choosing_a_film_brings_its_paper :: proc(t: ^testing.T) {
	stocks := []state.Stock {
		stock("vision", "Vision3", false, false, false, "print2383"),
		stock("velvia", "Velvia", false, true, false, "endura"),
		stock("endura", "Endura", true, false, false),
		stock("print2383", "2383 Print Film", true, false, false),
	}
	s := state.default_state()
	defer state.destroy(s)
	state.choosing(&s, "vision", false, false, stocks)
	testing.expect_value(t, s.film, "vision")
	testing.expect_value(t, s.paper, "print2383")
	testing.expect(t, !value.boolean(value.get(s.params, "io.scan_film")) && value.get(s.params, "io.scan_film") != nil)
	testing.expect(t, value.get(s.params, "film_render.grain.uniformity") == nil)
	state.choosing(&s, "velvia", false, true, stocks)
	testing.expect_value(t, s.paper, "print2383")
	testing.expect(t, value.boolean(value.get(s.params, "io.scan_film")))
	state.choosing(&s, "endura", true, false, stocks)
	testing.expect_value(t, s.film, "velvia")
	testing.expect_value(t, s.paper, "endura")
}

@(test)
filters_and_links :: proc(t: ^testing.T) {
	testing.expect_value(t, state.parse_hex_colour("#4dc53f"), 0x4dc53f)
	testing.expect_value(t, state.parse_hex_colour("##+ff"), 0xff)
	testing.expect_value(t, state.parse_hex_colour("+"), 0x888888)
	testing.expect_value(t, state.parse_hex_colour("1ffffffff"), 0x888888)
	f := state.Color_Filter{key = "hoya_y2", name = "Yellow", glass = "Hoya Y2", colour = 0xffff00}
	expect_text(t, state.color_filter_hint(&f, 1.34, "HP5", true, ','), "Darkens blue sky a touch so clouds stand out. The everyday filter. Takes 1,3 stops on HP5; auto exposure gives it.")
	expect_text(t, state.color_filter_hint(&f, 1.0, "HP5", false, ','), "Darkens blue sky a touch so clouds stand out. The everyday filter. Takes 1 stop on HP5.")
	expect_text(t, state.color_filter_hint(nil, 1.0, "HP5", false, ','), "No glass in front of the lens.")
	expect_text(t, state.color_filter_help(&f), "Yellow · Hoya Y2. Darkens blue sky a touch so clouds stand out. The everyday filter.")
	m, y := state.filter_link_shifts(.Together, state.FILTER_LINK_MAGENTA, 10.0, 0.0, 45.0)
	testing.expect_value(t, m, 5.0)
	testing.expect_value(t, y, 50.0)
	m, y = state.filter_link_shifts(.Opposite, state.FILTER_LINK_YELLOW, 10.0, 0.0, 0.0)
	testing.expect_value(t, m, -10.0)
	testing.expect_value(t, y, 10.0)
}
