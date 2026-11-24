// edits.rs's own tests on the core (core/src/state/edits and facts), with
// fixture.rs's small catalog. core/diff/tests/edits.rs holds every edits
// function to the frozen Rust on random states. Run by `build.py test`.
package state_edits_tests

import "base:runtime"
import "core:strings"
import "core:testing"
import "../../../src/state"
import "../../../src/state/edits"
import facts "../../../src/state/facts"
import "../../../src/value"

DEFAULTS :: `{
	"camera": {"lens_blur_um": 0.0, "auto_exposure": true, "auto_exposure_method": "center_weighted", "exposure_compensation_ev": 0.0, "film_format_mm": 36.0},
	"scanner": {"lens_blur": 0.0},
	"enlarger": {"print_exposure": 1.0, "preflash_exposure": 0.0, "m_filter_shift": 0.0, "y_filter_shift": 0.0},
	"io": {"output_cctf_encoding": true, "scan_film": false, "input_interpretation": {"mode": "auto"}, "raw_white_balance": {"mode": "as_shot"}},
	"prep": {"exposure_ev": 0.0, "highlights": 0.0, "shadows": 0.0},
	"film_render": {"grain": {"active": true}}
}`

Fixture :: struct {
	arena:   runtime.Arena,
	catalog: facts.Catalog,
	state:   state.Edit_State,
	facts:   facts.Facts,
}

stock :: proc(id, name: string, paper, bw: bool) -> state.Stock {
	return state.Stock{id = id, name = name, paper = paper, bw = bw, manufacturer_data = true}
}

// fixture.rs's `Fixture::new`: a fresh model's facts, everything in one arena.
fixture_init :: proc(x: ^Fixture) {
	_ = runtime.arena_init(&x.arena, 64 * 1024, context.allocator)
	a := runtime.arena_allocator(&x.arena)
	defaults, _ := value.parse_string(DEFAULTS, a)
	stocks := make([]state.Stock, 3, a)
	stocks[0] = stock("portra", "Portra 400", false, false)
	stocks[1] = stock("trix", "Tri-X 400", false, true)
	stocks[2] = stock("endura", "Endura", true, false)
	x.catalog = facts.Catalog{defaults = defaults, stocks = stocks}
	facts.catalog_index(&x.catalog, nil, a)
	x.state = state.default_state(a)
	x.state.params = value.clone(defaults, a)
	x.state.film = "portra"
	x.state.paper = "endura"
	facts.facts_init(&x.facts, a)
	x.facts.catalog = &x.catalog
	x.facts.state = &x.state
	x.facts.input.prefs = facts.Prefs {
		match_new_photos   = true,
		color_space        = "sRGB",
		output_format      = "png",
		jpeg_quality       = 95,
		png_bit_depth      = 8,
		tiff_bit_depth     = 16,
		tone_graph_channel = "rgb",
	}
	x.facts.input.has_source = true
	x.facts.input.preview_scale = 1
	x.facts.input.can_balance_print = true
	x.facts.input.can_export = true
}

fixture_destroy :: proc(x: ^Fixture) {
	runtime.arena_destroy(&x.arena)
}

set :: proc(x: ^Fixture, path: string, v: value.Value) {
	value.set(&x.state.params, path, v, runtime.arena_allocator(&x.arena))
	_ = facts.begin_update(&x.facts)
}

@(test)
counts_match_the_lists :: proc(t: ^testing.T) {
	x: Fixture
	fixture_init(&x)
	defer fixture_destroy(&x)
	// A fresh model already differs from new photos' brightness match.
	before := edits.count(.Prep, &x.facts)
	testing.expect_value(t, before, 1)
	set(&x, "prep.highlights", value.num(12))
	set(&x, "film_render.grain.active", false)
	x.state.border.enabled = true
	for tab in edits.Tab {
		list := edits.edits(tab, &x.facts, '.', context.temp_allocator)
		testing.expect_value(t, len(list), edits.count(tab, &x.facts))
	}
	testing.expect_value(t, edits.count(.Prep, &x.facts), before + 1)
	found := false
	for e in edits.edits(.Prep, &x.facts, '.', context.temp_allocator) {
		found ||= e.value == "+12"
	}
	testing.expect(t, found)
	testing.expect_value(t, edits.count(.Output, &x.facts), 1)
	lab := edits.edits(.Lab, &x.facts, ',', context.temp_allocator)
	if testing.expect_value(t, len(lab), 1) {
		testing.expect_value(t, lab[0].title, "Grain")
		testing.expect_value(t, lab[0].value, "Off")
	}
	free_all(context.temp_allocator)
}

@(test)
more_lines_reveal_and_fold_back :: proc(t: ^testing.T) {
	x: Fixture
	fixture_init(&x)
	defer fixture_destroy(&x)
	index := -1
	for g, i in state.ADJUSTMENT_GROUPS {
		if g.basic > 0 && g.basic < len(g.controls) {
			index = i
			break
		}
	}
	group := state.ADJUSTMENT_GROUPS[index]
	more, ok := edits.module_more(&x.facts, index, context.temp_allocator)
	testing.expect(t, ok)
	testing.expect(t, strings.has_suffix(more.text, "more control…") || strings.has_suffix(more.text, "more controls…"), more.text)
	testing.expect_value(t, more.action, edits.More_Action.Reveal)
	testing.expect_value(t, more.key, string(group.title))
	x.facts.input.prefs.more_adjustments = string(group.title)
	for _, i in group.controls {
		testing.expect(t, edits.control_shows(&x.facts, index, i))
	}
	more, ok = edits.module_more(&x.facts, index, context.temp_allocator)
	testing.expect(t, ok && strings.has_prefix(more.text, "Hide "), more.text)
	testing.expect_value(t, more.action, edits.More_Action.Conceal)
	section, has := edits.section_more(&x.facts, .Camera, context.temp_allocator)
	testing.expect(t, has && strings.contains(section.text, "Lens blur"), section.text)
	x.facts.input.prefs.lab_all_controls = true
	_, has = edits.section_more(&x.facts, .Camera, context.temp_allocator)
	testing.expect(t, !has)
	_, has = edits.module_more(&x.facts, index, context.temp_allocator)
	testing.expect(t, !has)
	free_all(context.temp_allocator)
}

@(test)
words_read_like_the_mac :: proc(t: ^testing.T) {
	testing.expect_value(t, edits.words("as_shot", context.temp_allocator), "As shot")
	testing.expect_value(t, edits.words("io.input_interpretation.mode", context.temp_allocator), "Mode")
	testing.expect_value(t, edits.words("", context.temp_allocator), "")
	free_all(context.temp_allocator)
}

@(test)
numbers_keep_two_decimals_and_a_sign :: proc(t: ^testing.T) {
	text :: proc(v: f64, signed: bool) -> string {
		b := strings.builder_make(context.temp_allocator)
		if signed {
			edits.write_signed(&b, v, ',')
		} else {
			edits.write_number(&b, v, ',')
		}
		return strings.to_string(b)
	}
	testing.expect_value(t, text(1.5, false), "1,5")
	testing.expect_value(t, text(2.0, false), "2")
	testing.expect_value(t, text(12.0, true), "+12")
	testing.expect_value(t, text(-0.25, true), "-0,25")
	testing.expect_value(t, text(0.0, true), "0")
	testing.expect_value(t, text(0.001, true), "0")
	testing.expect_value(t, text(-0.001, true), "0")
	free_all(context.temp_allocator)
}

@(test)
brightness_notes :: proc(t: ^testing.T) {
	x: Fixture
	fixture_init(&x)
	defer fixture_destroy(&x)
	a := runtime.arena_allocator(&x.arena)
	_, has := facts.note(&x.facts, context.temp_allocator)
	testing.expect(t, !has)
	x.state.brightness_match = state.Brightness_Match {
		ev = -0.5,
	}
	_ = facts.begin_update(&x.facts)
	// No inputs solved for yet: pending.
	n, _ := facts.note(&x.facts, context.temp_allocator)
	testing.expect_value(t, n, "Matching…")
	m := x.state.brightness_match.?
	m.has_inputs = true
	m.inputs = state.match_inputs(x.state, a)
	m.highlight_limited = true
	x.state.brightness_match = m
	_ = facts.begin_update(&x.facts)
	n, _ = facts.note(&x.facts, context.temp_allocator)
	testing.expect_value(t, n, "Starts at −0.50 EV to match the original, held back to protect highlights.")
	testing.expect_value(t, facts.matched_ev(&x.facts), -0.5)
	testing.expect_value(t, facts.exposure_offset(&x.facts), 0.5)
	set(&x, "io.input_interpretation.mode", "manual")
	n, _ = facts.note(&x.facts, context.temp_allocator)
	testing.expect_value(t, n, facts.UNAVAILABLE)
	testing.expect_value(t, facts.matched_ev(&x.facts), 0)
	free_all(context.temp_allocator)
}
