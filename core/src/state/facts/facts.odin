// What the inspector reads from the editor (windows/src/inspector_view/
// facts.rs's `InspectorFacts`, `Params`, `ColorFilters`, `Brightness`,
// `Prefs`), as the core's own input: the catalog (defaults, stocks, colour
// filters) once per catalog, the edit state, and the editor's flags and
// preferences as plain data. The edit lists (state/edits) and the
// inspector's view model (Phase 4d) read it.
//
// A host that keeps the state itself can answer the state's questions
// instead (`Host`): the edit lists then ask it, so nothing is converted.
package facts

import "base:runtime"
import "core:strings"
import crop "../../rules/crop"
import "../../value"
import ".."

// Set once per catalog: the engine's defaults, the stocks and filters the
// editor built from it, and each profile's colour filter stops.
Catalog :: struct {
	defaults: value.Value,
	stocks:   []state.Stock,
	filters:  []state.Color_Filter,
	// A profile's `color_filter_stops`, by its id: the first profile with
	// the id, as `find` reads them.
	stops:    map[string]value.Value,
}

// `InspectorFacts::prefs` (`Preferences`).
Prefs :: struct {
	lab_all_controls:   bool,
	match_new_photos:   bool,
	// The modules and sections whose "N more" was clicked, comma-separated.
	more_adjustments:   string,
	color_space:        string,
	output_format:      string,
	filter_shift_link:  string,
	tone_graph_channel: string,
	jpeg_quality:       u32,
	png_bit_depth:      u32,
	tiff_bit_depth:     u32,
}

// Why the last brightness match failed: the photograph's generation, the
// inputs it failed for (none: it can't run for this photograph), the
// message.
Failure :: struct {
	generation:  u64,
	has_inputs:  bool,
	// The inputs' JSON, read when first asked.
	inputs_text: string,
	message:     string,
}

Base_Status :: struct {
	found, picked, by_hand, from_roll: bool,
	edge:                              Maybe(string),
}

Flat_Field :: struct {
	path:   string,
	used:   Maybe(bool),
	reason: Maybe(string),
}

// `NegativeFacts`.
Negative :: struct {
	sentence:        string,
	warning:         Maybe(string),
	base:            Maybe(Base_Status),
	flat_field:      Flat_Field,
	busy:            bool,
	backlight_shown: bool,
}

// Everything but the state and the catalog.
Input :: struct {
	prefs:              Prefs,
	is_raw:             bool,
	lens_supported:     bool,
	has_source:         bool,
	photo_size:         crop.Size,
	source_size:        [2]u32,
	preview_scale:      f64,
	image_shown:        bool,
	size_error:         Maybe(string),
	custom_film_format: bool,
	balancing_print:    bool,
	can_balance_print:  bool,
	can_export:         bool,
	cropping:           bool,
	failure:            Maybe(Failure),
	generation:         u64,
	matching:           bool,
	negative:           Maybe(Negative),
}

// What a host answers about the state it keeps, for the edit lists.
// Laid out as negs_facts_host_t.
Host :: struct {
	user:        rawptr,
	// facts.rs's `is_modified`.
	is_modified: proc "c" (user: rawptr, path: [^]u8, len: uint) -> u8,
	// `value(path)` as JSON into `out` (`cap` bytes); returns its length.
	value:       proc "c" (user: rawptr, path: [^]u8, len: uint, out: [^]u8, cap: uint) -> uint,
	// HOST_* bits.
	flags:       proc "c" (user: rawptr) -> u32,
	// The brightness match's exposure: `out` = {offset, modified}.
	exposure:    proc "c" (user: rawptr, out: ^Exposure),
	// The chosen colour filter's name, borrowed for the call; 0 for none.
	color_filter: proc "c" (user: rawptr, name: ^Str) -> u8,
}

HOST_FILM_BW :: 1
HOST_FILM_POSITIVE :: 2
HOST_BORDER :: 4
HOST_MATCHES :: 8

Exposure :: struct {
	offset:   f64,
	modified: u8,
}

// negs_str_t.
Str :: struct {
	data: [^]u8,
	len:  uint,
}

Facts :: struct {
	catalog:      ^Catalog,
	state:        ^state.Edit_State,
	input:        Input,
	host:         ^Host,
	// What one update holds: the input's strings and the caches.
	update:       runtime.Arena,
	// One call's answers (borrowed until the next call).
	scratch:      runtime.Arena,
	// Caches, for this update.
	film:         int, // -2 not looked up, -1 none
	failed:       int, // -1 not worked out
	match_inputs: Maybe(value.Value),
	failure_inputs: Maybe(value.Value),
}

catalog_destroy :: proc(c: ^Catalog) {
	delete(c.stops)
}

// The stops map: each profile id's first `color_filter_stops`. Borrows
// from `catalog`.
catalog_index :: proc(c: ^Catalog, catalog: value.Value, allocator := context.allocator) {
	c.stops = make(map[string]value.Value, 64, allocator)
	profiles, ok := state.field(catalog, "profiles").(value.Array)
	if !ok {
		return
	}
	for p in profiles {
		id, is_string := state.field(p, "id").(string)
		if !is_string || id in c.stops {
			continue
		}
		c.stops[id] = state.field(p, "color_filter_stops")
	}
}

facts_init :: proc(f: ^Facts, allocator := context.allocator) {
	_ = runtime.arena_init(&f.update, 4 * 1024, allocator)
	_ = runtime.arena_init(&f.scratch, 4 * 1024, allocator)
	f.film = -2
	f.failed = -1
}

facts_destroy :: proc(f: ^Facts) {
	runtime.arena_destroy(&f.update)
	runtime.arena_destroy(&f.scratch)
}

update_allocator :: proc(f: ^Facts) -> runtime.Allocator {
	return runtime.arena_allocator(&f.update)
}

scratch_allocator :: proc(f: ^Facts) -> runtime.Allocator {
	return runtime.arena_allocator(&f.scratch)
}

// A new state and input (strings already in the update arena, see
// `begin_update`); the caches start again.
begin_update :: proc(f: ^Facts) -> runtime.Allocator {
	runtime.arena_free_all(&f.update)
	f.film = -2
	f.failed = -1
	f.match_inputs = nil
	f.failure_inputs = nil
	return update_allocator(f)
}

// The start of a call that answers into the scratch arena.
begin_call :: proc(f: ^Facts) -> runtime.Allocator {
	runtime.arena_free_all(&f.scratch)
	return scratch_allocator(f)
}

clone_input :: proc(in_: Input, allocator: runtime.Allocator) -> Input {
	c := in_
	s :: proc(text: string, a: runtime.Allocator) -> string {return strings.clone(text, a)}
	c.prefs.more_adjustments = s(in_.prefs.more_adjustments, allocator)
	c.prefs.color_space = s(in_.prefs.color_space, allocator)
	c.prefs.output_format = s(in_.prefs.output_format, allocator)
	c.prefs.filter_shift_link = s(in_.prefs.filter_shift_link, allocator)
	c.prefs.tone_graph_channel = s(in_.prefs.tone_graph_channel, allocator)
	if e, ok := in_.size_error.?; ok {
		c.size_error = s(e, allocator)
	}
	if fl, ok := in_.failure.?; ok {
		fl.inputs_text = s(fl.inputs_text, allocator)
		fl.message = s(fl.message, allocator)
		c.failure = fl
	}
	if n, ok := in_.negative.?; ok {
		n.sentence = s(n.sentence, allocator)
		if w, has := n.warning.?; has {
			n.warning = s(w, allocator)
		}
		if b, has := n.base.?; has {
			if e, has_edge := b.edge.?; has_edge {
				b.edge = s(e, allocator)
			}
			n.base = b
		}
		n.flat_field.path = s(n.flat_field.path, allocator)
		if r, has := n.flat_field.reason.?; has {
			n.flat_field.reason = s(r, allocator)
		}
		c.negative = n
	}
	return c
}
