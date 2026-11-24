// The inspector's facts' C ABI (negs_core.h, "Phase 4a"): a catalog handle
// set once per catalog, a facts handle updated per sync with the state (a
// negs_state_t, borrowed) and the editor's flags and preferences (a plain
// struct), and the readers facts.rs has. A host that keeps the state
// itself answers the edit lists' questions instead (negs_facts_host_t).
package negs_core

import "base:runtime"
import "core:strings"
import "../state"
import facts "../state/facts"
import "../value"

// MARK: The catalog

Catalog_Handle :: struct {
	arena:   runtime.Arena,
	catalog: facts.Catalog,
}

// The engine's defaults, the stocks and colour filters the host built from
// its catalog, and the catalog itself (for each profile's
// `color_filter_stops`); all copied. NULL when a JSON text doesn't read
// (empty texts read as null).
@(export, link_name = "negs_catalog_new")
catalog_new :: proc "c" (
	defaults: [^]u8,
	defaults_len: uint,
	stocks: [^]Stock_C,
	stock_count: uint,
	filters: [^]Color_Filter_C,
	filter_count: uint,
	catalog: [^]u8,
	catalog_len: uint,
) -> ^Catalog_Handle {
	context = core_context()
	h := new(Catalog_Handle)
	_ = runtime.arena_init(&h.arena, 64 * 1024, context.allocator)
	a := runtime.arena_allocator(&h.arena)
	read :: proc(text: [^]u8, n: uint, a: runtime.Allocator) -> (value.Value, bool) {
		if text == nil || n == 0 {
			return nil, true
		}
		v, err := value.parse(text[:n], a)
		return v, err == .None
	}
	d, d_ok := read(defaults, defaults_len, a)
	c, c_ok := read(catalog, catalog_len, a)
	ok := d_ok && c_ok
	h.catalog.defaults = d
	h.catalog.stocks = make([]state.Stock, stock_count, a)
	for i in 0 ..< stock_count {
		s := stock_view(&stocks[i])
		s.id = strings.clone(s.id, a)
		s.name = strings.clone(s.name, a)
		s.target = strings.clone(s.target, a)
		s.times = make([]f64, len(s.times), a)
		copy(s.times, stock_view(&stocks[i]).times)
		g := stocks[i].grain
		if g.len > 0 {
			grain, err := value.parse(g.data[:g.len], a)
			ok &&= err == .None
			s.grain = grain
		}
		h.catalog.stocks[i] = s
	}
	h.catalog.filters = make([]state.Color_Filter, filter_count, a)
	for i in 0 ..< filter_count {
		f := filter_view(&filters[i])
		h.catalog.filters[i] = state.Color_Filter{strings.clone(f.key, a), strings.clone(f.name, a), strings.clone(f.glass, a), f.colour}
	}
	facts.catalog_index(&h.catalog, c, a)
	if !ok {
		catalog_free(h)
		return nil
	}
	return h
}

@(export, link_name = "negs_catalog_free")
catalog_free :: proc "c" (h: ^Catalog_Handle) {
	context = core_context()
	if h == nil {
		return
	}
	runtime.arena_destroy(&h.arena)
	free(h)
}

// MARK: The facts

// negs_facts_prefs_t.
Prefs_C :: struct {
	more_adjustments, color_space, output_format, filter_shift_link, tone_graph_channel: Str_C,
	jpeg_quality, png_bit_depth, tiff_bit_depth:                                         u32,
	lab_all_controls, match_new_photos:                                                  u8,
}

// negs_facts_negative_t.
Negative_C :: struct {
	sentence, warning, edge, flat_path, flat_reason:                                                    Str_C,
	has_warning, has_base, base_found, base_picked, base_by_hand, base_from_roll, has_edge:             u8,
	has_flat_used, flat_used, has_flat_reason, busy, backlight_shown:                                   u8,
}

// negs_facts_input_t.
Input_C :: struct {
	prefs:                                                                                      Prefs_C,
	photo_size:                                                                                 [2]f64,
	preview_scale:                                                                              f64,
	generation:                                                                                 u64,
	failure_generation:                                                                         u64,
	failure_inputs, failure_message, size_error:                                                Str_C,
	negative:                                                                                   Negative_C,
	source_width, source_height:                                                                u32,
	is_raw, lens_supported, has_source, image_shown, has_size_error, custom_film_format:        u8,
	balancing_print, can_balance_print, can_export, cropping, has_failure, failure_has_inputs: u8,
	matching, has_negative:                                                                     u8,
}

// The host's input as the core's, borrowing its strings.
@(private)
input_view :: proc "contextless" (c: ^Input_C) -> facts.Input {
	in_: facts.Input
	if c == nil {
		return in_
	}
	in_.prefs = facts.Prefs {
		lab_all_controls   = c.prefs.lab_all_controls != 0,
		match_new_photos   = c.prefs.match_new_photos != 0,
		more_adjustments   = str_of(c.prefs.more_adjustments),
		color_space        = str_of(c.prefs.color_space),
		output_format      = str_of(c.prefs.output_format),
		filter_shift_link  = str_of(c.prefs.filter_shift_link),
		tone_graph_channel = str_of(c.prefs.tone_graph_channel),
		jpeg_quality       = c.prefs.jpeg_quality,
		png_bit_depth      = c.prefs.png_bit_depth,
		tiff_bit_depth     = c.prefs.tiff_bit_depth,
	}
	in_.is_raw = c.is_raw != 0
	in_.lens_supported = c.lens_supported != 0
	in_.has_source = c.has_source != 0
	in_.photo_size = {c.photo_size[0], c.photo_size[1]}
	in_.source_size = {c.source_width, c.source_height}
	in_.preview_scale = c.preview_scale
	in_.image_shown = c.image_shown != 0
	if c.has_size_error != 0 {
		in_.size_error = str_of(c.size_error)
	}
	in_.custom_film_format = c.custom_film_format != 0
	in_.balancing_print = c.balancing_print != 0
	in_.can_balance_print = c.can_balance_print != 0
	in_.can_export = c.can_export != 0
	in_.cropping = c.cropping != 0
	if c.has_failure != 0 {
		in_.failure = facts.Failure{c.failure_generation, c.failure_has_inputs != 0, str_of(c.failure_inputs), str_of(c.failure_message)}
	}
	in_.generation = c.generation
	in_.matching = c.matching != 0
	if c.has_negative != 0 {
		n := c.negative
		neg := facts.Negative {
			sentence        = str_of(n.sentence),
			busy            = n.busy != 0,
			backlight_shown = n.backlight_shown != 0,
		}
		if n.has_warning != 0 {
			neg.warning = str_of(n.warning)
		}
		if n.has_base != 0 {
			b := facts.Base_Status{found = n.base_found != 0, picked = n.base_picked != 0, by_hand = n.base_by_hand != 0, from_roll = n.base_from_roll != 0}
			if n.has_edge != 0 {
				b.edge = str_of(n.edge)
			}
			neg.base = b
		}
		neg.flat_field.path = str_of(n.flat_path)
		if n.has_flat_used != 0 {
			neg.flat_field.used = n.flat_used != 0
		}
		if n.has_flat_reason != 0 {
			neg.flat_field.reason = str_of(n.flat_reason)
		}
		in_.negative = neg
	}
	return in_
}

Facts_Handle :: struct {
	f:    facts.Facts,
	host:  facts.Host,
}

FACTS_BORROW :: 1 // NEGS_FACTS_BORROW

// A facts handle reading `catalog` (borrowed: it must outlive the handle;
// NULL for none). The caller owns it until negs_facts_free.
@(export, link_name = "negs_facts_new")
facts_new :: proc "c" (catalog: ^Catalog_Handle) -> ^Facts_Handle {
	context = core_context()
	h := new(Facts_Handle)
	facts.facts_init(&h.f, context.allocator)
	if catalog != nil {
		h.f.catalog = &catalog.catalog
	}
	return h
}

@(export, link_name = "negs_facts_free")
facts_free :: proc "c" (h: ^Facts_Handle) {
	context = core_context()
	if h == nil {
		return
	}
	facts.facts_destroy(&h.f)
	free(h)
}

// A new sync: the state (borrowed until the next update or the handle's
// end; NULL for none) and the input (copied, or with NEGS_FACTS_BORROW
// its strings borrowed until the next update).
@(export, link_name = "negs_facts_update")
facts_update :: proc "c" (h: ^Facts_Handle, s: ^State_Handle, input: ^Input_C, flags: u32) {
	context = core_context()
	if h == nil {
		return
	}
	a := facts.begin_update(&h.f)
	h.f.state = &s.state if s != nil else nil
	in_ := input_view(input)
	h.f.input = in_ if flags & FACTS_BORROW != 0 else facts.clone_input(in_, a)
}

// The host answering the edit lists' questions (copied; `user` must stay
// valid while it's set). NULL goes back to the state.
@(export, link_name = "negs_facts_set_host")
facts_set_host :: proc "c" (h: ^Facts_Handle, host: ^facts.Host) {
	if h == nil {
		return
	}
	if host == nil {
		h.f.host = nil
		return
	}
	h.host = host^
	h.f.host = &h.host
}

@(private)
value_into :: proc(v: value.Value, out: ^Buf) {
	b := strings.builder_make(0, 64, context.temp_allocator)
	value.write_compact(&b, v)
	store_buf(out, buf_from(b.buf[:]))
}

// `value(path)` as JSON (the grain lanes' presets included). 0 for a NULL
// handle.
@(export, link_name = "negs_facts_value")
facts_value :: proc "c" (h: ^Facts_Handle, path: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	if h == nil {
		store_buf(out, {})
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	value_into(facts.value_at(&h.f, host_string(path, n)), out)
	return 1
}

@(export, link_name = "negs_facts_default_value")
facts_default_value :: proc "c" (h: ^Facts_Handle, path: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	if h == nil {
		store_buf(out, {})
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	value_into(facts.default_value(&h.f, host_string(path, n)), out)
	return 1
}

@(export, link_name = "negs_facts_is_modified")
facts_is_modified :: proc "c" (h: ^Facts_Handle, path: [^]u8, n: uint) -> u8 {
	context = core_context()
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return u8(facts.is_modified(&h.f, host_string(path, n)))
}

FACTS_FILM_BW :: 1
FACTS_FILM_POSITIVE :: 2
FACTS_BORDER :: 4
FACTS_MATCHES :: 8
FACTS_ACTIVE :: 16
FACTS_PENDING :: 32
FACTS_FAILED_HERE :: 64
FACTS_OFFSET_MODIFIED :: 128
FACTS_SHOWS_METER :: 256
FACTS_FILM_FORMAT_CUSTOM :: 512
FACTS_NEGATIVE :: 1024
FACTS_COLOR_FILTER_SHOWN :: 2048
FACTS_HAS_CATALOG :: 4096
FACTS_NEGATIVE_BUSY :: 8192

// The yes-or-no readers, as NEGS_FACTS_* bits.
@(export, link_name = "negs_facts_flags")
facts_flags :: proc "c" (h: ^Facts_Handle) -> u32 {
	context = core_context()
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	f := &h.f
	bits := u32(0)
	set :: proc "contextless" (bits: ^u32, bit: u32, on: bool) {
		if on {
			bits^ |= bit
		}
	}
	set(&bits, FACTS_FILM_BW, facts.film_is_bw(f))
	set(&bits, FACTS_FILM_POSITIVE, facts.film_positive(f))
	set(&bits, FACTS_BORDER, facts.border_enabled(f))
	set(&bits, FACTS_MATCHES, facts.matches(f))
	set(&bits, FACTS_ACTIVE, facts.active(f))
	set(&bits, FACTS_PENDING, facts.pending(f))
	set(&bits, FACTS_FAILED_HERE, facts.failed_here(f))
	set(&bits, FACTS_OFFSET_MODIFIED, facts.exposure_offset_modified(f))
	set(&bits, FACTS_SHOWS_METER, facts.shows_meter(f))
	set(&bits, FACTS_FILM_FORMAT_CUSTOM, facts.film_format_custom(f))
	set(&bits, FACTS_NEGATIVE, facts.is_negative(f))
	set(&bits, FACTS_COLOR_FILTER_SHOWN, facts.color_filter_shown(f))
	set(&bits, FACTS_HAS_CATALOG, facts.has_catalog(f))
	set(&bits, FACTS_NEGATIVE_BUSY, facts.negative_busy(f))
	return bits
}

// negs_facts_brightness_t.
Brightness_C :: struct {
	matched_ev, exposure_offset, range_low, range_high: f64,
}

@(export, link_name = "negs_facts_brightness")
facts_brightness :: proc "c" (h: ^Facts_Handle, out: ^Brightness_C) {
	context = core_context()
	if h == nil || out == nil {
		return
	}
	context.temp_allocator = facts.begin_call(&h.f)
	f := &h.f
	low, high := facts.exposure_offset_range(f)
	out^ = Brightness_C{facts.matched_ev(f), facts.exposure_offset(f), low, high}
}

// The note under Prep's Exposure; borrowed until the next call on `h`.
// 0 for none.
@(export, link_name = "negs_facts_note")
facts_note :: proc "c" (h: ^Facts_Handle, out: ^Str_C) -> u8 {
	context = core_context()
	if out != nil {
		out^ = {}
	}
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	text, ok := facts.note(&h.f, context.temp_allocator)
	if ok && out != nil {
		out^ = str_c(text)
	}
	return u8(ok)
}

// The filter in front of the lens (`key`, borrowed until the next update)
// and its place in the catalog's filters; -1 for none.
@(export, link_name = "negs_facts_color_filter")
facts_color_filter :: proc "c" (h: ^Facts_Handle, key: ^Str_C) -> i64 {
	context = core_context()
	if key != nil {
		key^ = {}
	}
	if h == nil {
		return -1
	}
	context.temp_allocator = facts.begin_call(&h.f)
	f := &h.f
	if key != nil {
		key^ = str_c(facts.color_filter_key(f))
	}
	c := facts.chosen_color_filter(f)
	if c == nil {
		return -1
	}
	return i64(uintptr(c) - uintptr(raw_data(f.catalog.filters))) / size_of(state.Color_Filter)
}

@(export, link_name = "negs_facts_color_filter_stops")
facts_color_filter_stops :: proc "c" (h: ^Facts_Handle, key: [^]u8, n: uint, out: ^f64) -> u8 {
	context = core_context()
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	stops, ok := facts.color_filter_stops(&h.f, host_string(key, n)).?
	if ok && out != nil {
		out^ = stops
	}
	return u8(ok)
}

// The film's place in the catalog's stocks; -1 for none.
@(export, link_name = "negs_facts_film_stock")
facts_film_stock :: proc "c" (h: ^Facts_Handle) -> i64 {
	context = core_context()
	if h == nil {
		return -1
	}
	_ = facts.film_stock(&h.f)
	return i64(h.f.film)
}

@(export, link_name = "negs_facts_revealed")
facts_revealed :: proc "c" (h: ^Facts_Handle, key: [^]u8, n: uint) -> u8 {
	context = core_context()
	return u8(h != nil && facts.revealed(&h.f, host_string(key, n)))
}

// Static.
@(export, link_name = "negs_facts_preview_space")
facts_preview_space :: proc "c" (h: ^Facts_Handle) -> cstring {
	context = core_context()
	if h == nil {
		return facts.COLOR_SPACES[0]
	}
	return facts.facts_preview_space(&h.f)
}

// `preview_space(chosen)`. Static.
@(export, link_name = "negs_preview_space")
preview_space :: proc "c" (chosen: [^]u8, n: uint) -> cstring {
	context = core_context()
	return facts.preview_space(host_string(chosen, n))
}

// 0 SizeMode, 1 FilterLink.
@(export, link_name = "negs_facts_choice")
facts_choice :: proc "c" (h: ^Facts_Handle, which: u32) -> u32 {
	context = core_context()
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	switch which {
	case 0:
		return u32(facts.size_mode(&h.f))
	case 1:
		return u32(facts.filter_link(&h.f))
	}
	return 0
}

// The film's (or paper's) development times, borrowed from the catalog.
@(export, link_name = "negs_facts_stock_times")
facts_stock_times :: proc "c" (h: ^Facts_Handle, paper: u8, count: ^uint) -> [^]f64 {
	context = core_context()
	times: []f64
	if h != nil {
		times = facts.stock_times(&h.f, paper != 0)
	}
	if count != nil {
		count^ = uint(len(times))
	}
	return raw_data(times)
}

@(export, link_name = "negs_facts_photo_pixels")
facts_photo_pixels :: proc "c" (h: ^Facts_Handle, width, height: ^u32) -> u8 {
	context = core_context()
	if h == nil {
		return 0
	}
	w, ht, ok := facts.photo_pixels(&h.f)
	if ok {
		if width != nil {
			width^ = w
		}
		if height != nil {
			height^ = ht
		}
	}
	return u8(ok)
}
