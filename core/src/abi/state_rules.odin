// The rest of state.rs's C ABI (negs_core.h, "Phase 4a: state rules, edits,
// inspector"): stocks, colour filters, the filter link, the tables, the
// film menus' groups and search, choosing a stock, values as the inspector
// shows them. Stocks cross as negs_stock_t, borrowed for the call; words
// come back static, into the caller's buffer, or as an owned negs_buf_t.
package negs_core

import "base:runtime"
import "core:slice"
import "core:strings"
import "../state"
import "../value"

// Laid out as negs_stock_t: borrowed for the call. `grain` is the preset's
// JSON (read only where a catalog is built).
Stock_C :: struct {
	id, name, target, grain:                                                     Str_C,
	times:                                                                       [^]f64,
	times_len:                                                                   uint,
	paper, positive, bw, fotufilm, manufacturer_data, negs_built, cine_print: u8,
}

// negs_color_filter_t.
Color_Filter_C :: struct {
	key, name, glass: Str_C,
	colour:           u32,
}

@(private)
str_c :: proc "contextless" (s: string) -> Str_C {
	return Str_C{raw_data(s), uint(len(s))}
}

@(private)
str_of :: proc "contextless" (s: Str_C) -> string {
	return host_string(s.data, s.len)
}

// The stock as the core's, borrowing the host's strings; no grain.
@(private)
stock_view :: proc "contextless" (c: ^Stock_C) -> state.Stock {
	return state.Stock {
		id = str_of(c.id),
		name = str_of(c.name),
		target = str_of(c.target),
		times = c.times[:c.times_len] if c.times != nil else nil,
		paper = c.paper != 0,
		positive = c.positive != 0,
		bw = c.bw != 0,
		fotufilm = c.fotufilm != 0,
		manufacturer_data = c.manufacturer_data != 0,
		negs_built = c.negs_built != 0,
		cine_print = c.cine_print != 0,
	}
}

// The host's stocks, borrowed, in the temporary allocator.
@(private)
stock_views :: proc(stocks: [^]Stock_C, count: uint) -> []state.Stock {
	if stocks == nil || count == 0 {
		return nil
	}
	out := make([]state.Stock, count, context.temp_allocator)
	for i in 0 ..< count {
		out[i] = stock_view(&stocks[i])
	}
	return out
}

@(private)
filter_view :: proc "contextless" (c: ^Color_Filter_C) -> state.Color_Filter {
	return state.Color_Filter{key = str_of(c.key), name = str_of(c.name), glass = str_of(c.glass), colour = c.colour}
}

// MARK: Stocks and filters

// `Stock::new` / `ColorFilter::new` on one catalog entry: `out`'s strings
// (and times) point into `storage`, an owned block the caller frees once it
// has copied them.
@(export, link_name = "negs_stock_read")
stock_read :: proc "c" (json: [^]u8, n: uint, out: ^Stock_C, storage: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	if storage != nil {
		storage^ = {}
	}
	v, ok := host_value(json, n)
	if !ok || out == nil || storage == nil {
		return 0
	}
	s := state.stock_from_value(v, context.temp_allocator)
	grain := strings.builder_make(0, 256, context.temp_allocator)
	if s.grain != nil {
		value.write_compact(&grain, s.grain)
	}
	// times first (the block is malloc-aligned), then the strings.
	times_bytes := len(s.times) * size_of(f64)
	total := times_bytes + len(s.id) + len(s.name) + len(s.target) + strings.builder_len(grain)
	block := buf_make(max(total, 1))
	if block.data == nil {
		return 0
	}
	at := 0
	put :: proc(block: Buf, at: ^int, bytes: []u8) -> Str_C {
		start := at^
		if len(bytes) > 0 {
			runtime.mem_copy_non_overlapping(&block.data[start], raw_data(bytes), len(bytes))
		}
		at^ += len(bytes)
		return Str_C{&block.data[start] if len(bytes) > 0 else nil, uint(len(bytes))}
	}
	if len(s.times) > 0 {
		runtime.mem_copy_non_overlapping(block.data, raw_data(s.times), times_bytes)
		out.times = cast([^]f64)block.data
		out.times_len = uint(len(s.times))
		at = times_bytes
	}
	out.id = put(block, &at, transmute([]u8)s.id)
	out.name = put(block, &at, transmute([]u8)s.name)
	out.target = put(block, &at, transmute([]u8)s.target)
	out.grain = put(block, &at, grain.buf[:])
	out.paper = u8(s.paper)
	out.positive = u8(s.positive)
	out.bw = u8(s.bw)
	out.fotufilm = u8(s.fotufilm)
	out.manufacturer_data = u8(s.manufacturer_data)
	out.negs_built = u8(s.negs_built)
	out.cine_print = u8(s.cine_print)
	storage^ = block
	return 1
}

@(export, link_name = "negs_color_filter_read")
color_filter_read :: proc "c" (json: [^]u8, n: uint, out: ^Color_Filter_C, storage: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	if storage != nil {
		storage^ = {}
	}
	v, ok := host_value(json, n)
	if !ok || out == nil || storage == nil {
		return 0
	}
	f := state.color_filter_from_value(v, context.temp_allocator)
	block := buf_make(max(len(f.key) + len(f.name) + len(f.glass), 1))
	if block.data == nil {
		return 0
	}
	at := uint(0)
	for text, i in ([3]string{f.key, f.name, f.glass}) {
		s := Str_C{nil, uint(len(text))}
		if len(text) > 0 {
			runtime.mem_copy_non_overlapping(&block.data[at], raw_data(text), len(text))
			s.data = &block.data[at]
		}
		at += uint(len(text))
		switch i {
		case 0:
			out.key = s
		case 1:
			out.name = s
		case 2:
			out.glass = s
		}
	}
	out.colour = f.colour
	storage^ = block
	return 1
}

@(export, link_name = "negs_stock_is_motion_picture_print")
stock_is_motion_picture_print :: proc "c" (s: ^Stock_C) -> u8 {
	context = core_context()
	return u8(s != nil && state.is_motion_picture_print(stock_view(s)))
}

@(export, link_name = "negs_stock_lists_negs_built")
stock_lists_negs_built :: proc "c" (s: ^Stock_C, shown: u8) -> u8 {
	context = core_context()
	return u8(s != nil && state.lists_negs_built(stock_view(s), shown != 0))
}

@(export, link_name = "negs_fotufilm_from_key")
fotufilm_from_key :: proc "c" (key: [^]u8, n: uint) -> u32 {
	return u32(state.fotufilm_from_key(host_string(key, n)))
}

// 0 the key, 1 the title. Static; NULL past the choices.
@(export, link_name = "negs_fotufilm_word")
fotufilm_word :: proc "c" (choice: u32, which: u32) -> cstring {
	if choice > u32(max(state.Fotufilm_Films)) {
		return nil
	}
	f := state.Fotufilm_Films(choice)
	switch which {
	case 0:
		return state.FOTUFILM_KEYS[f]
	case 1:
		return state.FOTUFILM_TITLES[f]
	}
	return nil
}

@(export, link_name = "negs_fotufilm_lists")
fotufilm_lists :: proc "c" (choice: u32, s: ^Stock_C) -> u8 {
	if s == nil || choice > u32(max(state.Fotufilm_Films)) {
		return 0
	}
	return u8(state.fotufilm_lists(state.Fotufilm_Films(choice), stock_view(s)))
}

@(export, link_name = "negs_color_filter_effect")
color_filter_effect :: proc "c" (key: [^]u8, n: uint) -> cstring {
	return state.color_filter_effect(host_string(key, n))
}

@(export, link_name = "negs_color_filter_hint")
color_filter_hint :: proc "c" (filter: ^Color_Filter_C, has_stops: u8, stops: f64, film: [^]u8, film_len: uint, metered: u8, decimal: u32, out: ^Buf) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	f: state.Color_Filter
	chosen: ^state.Color_Filter
	if filter != nil {
		f = filter_view(filter)
		chosen = &f
	}
	s: Maybe(f64)
	if has_stops != 0 {
		s = stops
	}
	text := state.color_filter_hint(chosen, s, host_string(film, film_len), metered != 0, decimal_rune(decimal), context.temp_allocator)
	store_buf(out, buf_from(transmute([]u8)text))
}

@(export, link_name = "negs_color_filter_help")
color_filter_help :: proc "c" (filter: ^Color_Filter_C, out: ^Buf) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	f: state.Color_Filter
	chosen: ^state.Color_Filter
	if filter != nil {
		f = filter_view(filter)
		chosen = &f
	}
	text := state.color_filter_help(chosen, context.temp_allocator)
	store_buf(out, buf_from(transmute([]u8)text))
}

// A `char` the host passes: an invalid scalar reads as '.'.
@(private)
decimal_rune :: proc "contextless" (c: u32) -> rune {
	if c > 0x10FFFF || (c >= 0xD800 && c <= 0xDFFF) {
		return '.'
	}
	return rune(c)
}

// MARK: The filter link

@(export, link_name = "negs_filter_link_from_key")
filter_link_from_key :: proc "c" (key: [^]u8, n: uint) -> u32 {
	return u32(state.filter_link_from_key(host_string(key, n)))
}

// 0 the key, 1 the title. Static; NULL past the links.
@(export, link_name = "negs_filter_link_word")
filter_link_word :: proc "c" (link: u32, which: u32) -> cstring {
	if link > u32(max(state.Filter_Link)) {
		return nil
	}
	l := state.Filter_Link(link)
	switch which {
	case 0:
		return state.FILTER_LINK_KEYS[l]
	case 1:
		return state.FILTER_LINK_TITLES[l]
	}
	return nil
}

@(export, link_name = "negs_filter_link_links")
filter_link_links :: proc "c" (path: [^]u8, n: uint) -> u8 {
	return u8(state.filter_link_links(host_string(path, n)))
}

// The two shifts (magenta, yellow) into `out`.
@(export, link_name = "negs_filter_link_shifts")
filter_link_shifts :: proc "c" (link: u32, path: [^]u8, n: uint, v, magenta, yellow: f64, out: ^[2]f64) {
	l := state.Filter_Link(link) if link <= u32(max(state.Filter_Link)) else .Separate
	m, y := state.filter_link_shifts(l, host_string(path, n), v, magenta, yellow)
	if out != nil {
		out^ = {m, y}
	}
}

// MARK: Paths

// grain_lane: the field's place in GRAIN_STOCK_FIELDS and the lane.
@(export, link_name = "negs_grain_lane")
grain_lane :: proc "c" (path: [^]u8, n: uint, field: ^u32, lane: ^u64) -> u8 {
	f, l, ok := state.grain_lane(host_string(path, n))
	if !ok {
		return 0
	}
	if field != nil {
		field^ = u32(f)
	}
	if lane != nil {
		lane^ = l
	}
	return 1
}

@(export, link_name = "negs_color_only")
color_only :: proc "c" (path: [^]u8, n: uint) -> u8 {
	return u8(state.color_only(host_string(path, n)))
}

@(export, link_name = "negs_bw_title")
bw_title :: proc "c" (path: [^]u8, n: uint) -> cstring {
	return state.bw_title(host_string(path, n))
}

// MARK: Words and values

@(export, link_name = "negs_option_title")
option_title :: proc "c" (option: [^]u8, n: uint, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [256]u8
	b := strings.builder_from_bytes(buf[:])
	text := host_string(option, n)
	if len(text) * 3 + 8 > len(buf) {
		// Upper-casing can triple a letter's bytes.
		arena: runtime.Arena
		b = strings.builder_make(0, len(text) * 3 + 8, runtime.arena_allocator(call_begin(&arena)))
		state.write_option_title(&b, text)
		return into(b.buf[:], out, cap)
	}
	state.write_option_title(&b, text)
	return into(b.buf[:], out, cap)
}

@(export, link_name = "negs_snap")
snap :: proc "c" (v, low, high, step: f64) -> f64 {
	return state.snap(v, low, high, step)
}

@(export, link_name = "negs_decimals")
decimals :: proc "c" (step: f64) -> uint {
	return uint(state.decimals(step))
}

@(export, link_name = "negs_format_value")
format_value :: proc "c" (v, step: f64, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [400]u8
	b := strings.builder_from_bytes(buf[:])
	state.write_value(&b, v, step)
	return into(b.buf[:], out, cap)
}

// The format's place in FILM_FORMATS; -1 for Custom.
@(export, link_name = "negs_film_format")
film_format :: proc "c" (mm: f64) -> i32 {
	return i32(state.film_format(mm))
}

@(export, link_name = "negs_ratio_name")
ratio_name :: proc "c" (width, height: f64, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	text := state.ratio_name(width, height, context.temp_allocator)
	return into(transmute([]u8)text, out, cap)
}

// MARK: Tables

// negs_adjustment_group_t.
Adjustment_Group_C :: struct {
	title:        cstring,
	section:      u32,
	enable:       cstring,
	basic:        uint,
	controls:     [^]state.Adjustment,
	controls_len: uint,
}

@(export, link_name = "negs_adjustment_group_count")
adjustment_group_count :: proc "c" () -> uint {
	context = core_context()
	return uint(len(state.ADJUSTMENT_GROUPS))
}

// ADJUSTMENT_GROUPS[index] (static); 0 past the end.
@(export, link_name = "negs_adjustment_group")
adjustment_group :: proc "c" (index: uint, out: ^Adjustment_Group_C) -> u8 {
	context = core_context()
	if index >= uint(len(state.ADJUSTMENT_GROUPS)) || out == nil {
		return 0
	}
	g := state.ADJUSTMENT_GROUPS[index]
	out^ = Adjustment_Group_C{g.title, u32(g.section), g.enable if len(g.enable) > 0 else nil, uint(g.basic), raw_data(g.controls), uint(len(g.controls))}
	return 1
}

// The loose controls and Prep's lists: 0 PREP_TONE, 1 PREP_COLOR,
// 2 PREP_DETAIL, 3 NEGATIVE_EXPOSURE, 4 LENS_BLUR, 5 WB_TEMPERATURE,
// 6 WB_TINT, 7 FILM_FORMAT. Static; NULL past them.
@(export, link_name = "negs_adjustments")
adjustments :: proc "c" (which: u32, count: ^uint) -> [^]state.Adjustment {
	context = core_context()
	list: []state.Adjustment
	switch which {
	case 0:
		list = state.PREP_TONE
	case 1:
		list = state.PREP_COLOR
	case 2:
		list = state.PREP_DETAIL
	case 3:
		list = ([^]state.Adjustment)(&state.NEGATIVE_EXPOSURE)[:1]
	case 4:
		list = ([^]state.Adjustment)(&state.LENS_BLUR)[:1]
	case 5:
		list = ([^]state.Adjustment)(&state.WB_TEMPERATURE)[:1]
	case 6:
		list = ([^]state.Adjustment)(&state.WB_TINT)[:1]
	case 7:
		list = ([^]state.Adjustment)(&state.FILM_FORMAT)[:1]
	case:
		if count != nil {
			count^ = 0
		}
		return nil
	}
	if count != nil {
		count^ = uint(len(list))
	}
	return raw_data(list)
}

// The word lists: 0 METERING, 1 INPUT_MODES, 2 ASSUMED_SPACES,
// 3 MANUAL_SPACES, 4 WHITE_BALANCE, 5 GRAIN_STOCK_FIELDS,
// 6 DIR_HALATION_STOCK_PATHS, 7 the choice paths (negs_choice_t's bits).
// Static; NULL past them.
@(export, link_name = "negs_state_words")
state_words :: proc "c" (which: u32, count: ^uint) -> [^]cstring {
	list: []cstring
	switch which {
	case 0:
		list = state.METERING[:]
	case 1:
		list = state.INPUT_MODES[:]
	case 2:
		list = state.ASSUMED_SPACES[:]
	case 3:
		list = state.MANUAL_SPACES[:]
	case 4:
		list = state.WHITE_BALANCE[:]
	case 5:
		list = state.GRAIN_STOCK_FIELDS[:]
	case 6:
		list = state.DIR_HALATION_STOCK_PATHS[:]
	case 7:
		list = slice.enumerated_array(&state.CHOICE_PATHS)
	}
	if count != nil {
		count^ = uint(len(list))
	}
	return raw_data(list) if len(list) > 0 else nil
}

// FILM_FORMATS[index]; 0 past the end.
@(export, link_name = "negs_film_format_row")
film_format_row :: proc "c" (index: uint, name: ^cstring, mm: ^f64) -> u8 {
	if index >= len(state.FILM_FORMATS) {
		return 0
	}
	f := state.FILM_FORMATS[index]
	if name != nil {
		name^ = f.name
	}
	if mm != nil {
		mm^ = f.mm
	}
	return 1
}

// 0 NEGS_BUILT_HINT, 1 FILTER_LINK_HELP, 2 BALANCE_PRINT_HELP,
// 3 ColorFilter::PATH, 4 ColorFilter::NONE, 5 FilterLink::MAGENTA,
// 6 FilterLink::YELLOW, 7 io.scan_film. Static; NULL past them.
@(export, link_name = "negs_state_rules_constant")
state_rules_constant :: proc "c" (which: u32) -> cstring {
	switch which {
	case 0:
		return state.NEGS_BUILT_HINT
	case 1:
		return state.FILTER_LINK_HELP
	case 2:
		return state.BALANCE_PRINT_HELP
	case 3:
		return state.COLOR_FILTER_PATH
	case 4:
		return state.COLOR_FILTER_NONE
	case 5:
		return state.FILTER_LINK_MAGENTA
	case 6:
		return state.FILTER_LINK_YELLOW
	case 7:
		return state.SCAN_FILM
	}
	return nil
}

// 0 PREP_EXPOSURE_RANGE low, 1 high, 2 PREP_EXPOSURE_STEP,
// 3 FilterLink::RANGE low, 4 high; NaN past them.
@(export, link_name = "negs_state_rules_number")
state_rules_number :: proc "c" (which: u32) -> f64 {
	switch which {
	case 0:
		return state.PREP_EXPOSURE_LOW
	case 1:
		return state.PREP_EXPOSURE_HIGH
	case 2:
		return state.PREP_EXPOSURE_STEP
	case 3:
		return state.FILTER_LINK_LOW
	case 4:
		return state.FILTER_LINK_HIGH
	}
	return transmute(f64)u64(0x7FF8000000000000)
}

// MARK: Choosing

// negs_choice_t: what choosing a stock sets. `film` and `paper` borrow
// from the call's inputs (the film, the paper, `id`, or a stock's target).
Choice_C :: struct {
	film, paper: Str_C,
	nulls:       u32,
	sets_scan:   u8,
	scan:        u8,
}

@(private)
choice_c :: proc "contextless" (c: state.Choice) -> Choice_C {
	return Choice_C{str_c(c.film), str_c(c.paper), transmute(u32)c.nulls, u8(c.sets_scan), u8(c.scan)}
}

// `choosing` for a state with this film and paper: what to set.
@(export, link_name = "negs_choosing_plan")
choosing_plan :: proc "c" (film: [^]u8, film_len: uint, paper: [^]u8, paper_len: uint, id: [^]u8, id_len: uint, is_paper, keep_paper: u8, stocks: [^]Stock_C, count: uint, out: ^Choice_C) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	c := state.choosing_plan(host_string(film, film_len), host_string(paper, paper_len), host_string(id, id_len), is_paper != 0, keep_paper != 0, stock_views(stocks, count))
	if out != nil {
		out^ = choice_c(c)
	}
}

// `pair_film`: scanning for slides, and the film's own paper unless
// `paper` is 0.
@(export, link_name = "negs_pair_film_plan")
pair_film_plan :: proc "c" (film: [^]u8, film_len: uint, paper: [^]u8, paper_len: uint, stocks: [^]Stock_C, count: uint, with_paper: u8, out: ^Choice_C) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	c := state.Choice{film = host_string(film, film_len), paper = host_string(paper, paper_len)}
	state.pair_plan(&c, stock_views(stocks, count), with_paper != 0)
	if out != nil {
		out^ = choice_c(c)
	}
}

// `choosing` on a state: a new handle the caller frees.
@(export, link_name = "negs_choosing")
choosing :: proc "c" (h: ^State_Handle, id: [^]u8, id_len: uint, is_paper, keep_paper: u8, stocks: [^]Stock_C, count: uint) -> ^State_Handle {
	context = core_context()
	if h == nil {
		return nil
	}
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	next := handle_new()
	a := runtime.arena_allocator(&next.arena)
	next.state = state.clone(h.state, a)
	state.choosing(&next.state, host_string(id, id_len), is_paper != 0, keep_paper != 0, stock_views(stocks, count), a)
	return next
}

// MARK: Groups and the search

// negs_stock_group_t: a section of a stock menu: its title (static) and
// its stocks, `count` places from `start` in the call's `places`.
Stock_Group_C :: struct {
	title: cstring,
	start: uint,
	count: uint,
}

@(private)
write_groups :: proc(groups: [dynamic]state.Stock_Group, out: [^]Stock_Group_C, cap: uint, places: [^]u32) -> uint {
	at := uint(0)
	for g, i in groups {
		if uint(i) < cap && out != nil {
			// The titles are GROUP_TITLES' own (static).
			title: cstring
			for t in state.GROUP_TITLES {
				if string(t) == g.title {
					title = t
				}
			}
			out[i] = Stock_Group_C{title, at, uint(len(g.stocks))}
		}
		for s in g.stocks {
			if places != nil {
				places[at] = u32(s)
			}
			at += 1
		}
	}
	return uint(len(groups))
}

// film_groups: up to 4 groups into `out`, their stocks' places in `films`
// into `places` (room for `film_count`). Returns the number of groups.
@(export, link_name = "negs_film_groups")
film_groups :: proc "c" (films: [^]Stock_C, film_count: uint, papers: [^]Stock_C, paper_count: uint, out: [^]Stock_Group_C, cap: uint, places: [^]u32) -> uint {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	groups := state.film_groups(stock_views(films, film_count), stock_views(papers, paper_count), context.temp_allocator)
	return write_groups(groups, out, cap, places)
}

// paper_groups: up to 3 groups, as negs_film_groups.
@(export, link_name = "negs_paper_groups")
paper_groups :: proc "c" (papers: [^]Stock_C, paper_count: uint, out: [^]Stock_Group_C, cap: uint, places: [^]u32) -> uint {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	groups := state.paper_groups(stock_views(papers, paper_count), context.temp_allocator)
	return write_groups(groups, out, cap, places)
}

// `matching`: groups `titles[i]` of `counts[i]` stocks each, their stocks
// one after another in `stocks`. `keep` (one per stock) gets 1 for each
// stock kept; groups left with none drop out. Returns 0 for a query
// without words (everything kept), 1 otherwise.
@(export, link_name = "negs_stock_matching")
stock_matching :: proc "c" (query: [^]u8, query_len: uint, titles: [^]Str_C, counts: [^]uint, group_count: uint, stocks: [^]Stock_C, keep: [^]u8) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	total := uint(0)
	groups := make([dynamic]state.Stock_Group, 0, group_count, context.temp_allocator)
	for i in 0 ..< group_count {
		g := state.Stock_Group{str_of(titles[i]), make([dynamic]int, counts[i], context.temp_allocator)}
		for j in 0 ..< counts[i] {
			g.stocks[j] = int(total + j)
		}
		total += counts[i]
		append(&groups, g)
	}
	all := stock_views(stocks, total)
	if keep != nil {
		for i in 0 ..< total {
			keep[i] = 1
		}
	}
	if len(state.split_whitespace(host_string(query, query_len), context.temp_allocator)) == 0 {
		return 0
	}
	state.matching(host_string(query, query_len), &groups, all)
	if keep != nil {
		for i in 0 ..< total {
			keep[i] = 0
		}
		for g in groups {
			for s in g.stocks {
				keep[s] = 1
			}
		}
	}
	return 1
}

// The host's case- and accent-blind `contains` for the film search (set
// once, before the first search; NULL for a plain byte search).
@(export, link_name = "negs_text_set_contains")
text_set_contains :: proc "c" (contains: state.Contains_Fn) {
	state.contains_fn = contains
}
