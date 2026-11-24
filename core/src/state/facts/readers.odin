// facts.rs's readers: values with their grain-lane defaults, whether one is
// modified, the film, the colour filters, the preferences. Where a host
// answers (`Facts.host`), the readers the edit lists use ask it.
package facts

import "core:strings"
import "../../value"
import negative "../../rules/negative"
import crop "../../rules/crop"
import ".."

// output.rs's COLOR_SPACES: the colour spaces a save offers.
@(rodata)
COLOR_SPACES := [?]cstring{"sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"}

INPUT_MODE :: "io.input_interpretation.mode"
UNAVAILABLE :: "Matching reads the file’s own colour tags. Set Input Colour to Automatic to use it."

// MARK: Params

// The film stock (`Params::film_stock`): the first film with the state's
// film id; nil for none.
film_stock :: proc(f: ^Facts) -> ^state.Stock {
	if f.catalog == nil || f.state == nil {
		return nil
	}
	if f.film == -2 {
		f.film = -1
		for &s, i in f.catalog.stocks {
			if !s.paper && s.id == f.state.film {
				f.film = i
				break
			}
		}
	}
	return &f.catalog.stocks[f.film] if f.film >= 0 else nil
}

film_is_bw :: proc(f: ^Facts) -> bool {
	if f.host != nil {
		return f.host.flags(f.host.user) & HOST_FILM_BW != 0
	}
	s := film_stock(f)
	return s != nil && s.bw
}

// The film is a slide (edits' `scan_film_modified`).
film_positive :: proc(f: ^Facts) -> bool {
	if f.host != nil {
		return f.host.flags(f.host.user) & HOST_FILM_POSITIVE != 0
	}
	s := film_stock(f)
	return s != nil && s.positive
}

border_enabled :: proc(f: ^Facts) -> bool {
	if f.host != nil {
		return f.host.flags(f.host.user) & HOST_BORDER != 0
	}
	return f.state != nil && f.state.border.enabled
}

// The film's preset for a grain lane (`json::get` on "<field>.<lane>").
@(private)
grain_preset :: proc(f: ^Facts, field: int, lane: u64) -> value.Value {
	s := film_stock(f)
	if s == nil {
		return nil
	}
	buf: [64]u8
	b := strings.builder_from_bytes(buf[:])
	strings.write_string(&b, string(state.GRAIN_STOCK_FIELDS[field]))
	strings.write_byte(&b, '.')
	strings.write_u64(&b, lane)
	return value.get(s.grain, strings.to_string(b))
}

// `Params::value`: the state's value; a null grain lane is the film's
// preset. Borrowed (in host mode, parsed into the call's scratch).
value_at :: proc(f: ^Facts, path: string) -> value.Value {
	if f.host != nil {
		buf: [256]u8
		n := f.host.value(f.host.user, raw_data(path), uint(len(path)), raw_data(buf[:]), len(buf))
		text := buf[:min(n, len(buf))]
		if n > len(buf) {
			big := make([]u8, n, scratch_allocator(f))
			_ = f.host.value(f.host.user, raw_data(path), uint(len(path)), raw_data(big), n)
			text = big
		}
		v, _ := value.parse(text, scratch_allocator(f))
		return v
	}
	if f.state == nil {
		return nil
	}
	v := value.get(f.state.params, path)
	if v == nil {
		if field, lane, ok := state.grain_lane(path); ok {
			return grain_preset(f, field, lane)
		}
	}
	return v
}

// `Params::default_value`: a grain lane's default is the film's preset,
// not the engine's null.
default_value :: proc(f: ^Facts, path: string) -> value.Value {
	if field, lane, ok := state.grain_lane(path); ok {
		return grain_preset(f, field, lane)
	}
	if f.catalog == nil {
		return nil
	}
	return value.get(f.catalog.defaults, path)
}

// `Params::is_modified`: numbers within a millionth (Rust defaults arrive
// as widened f32 values), anything else by `json::same`.
is_modified :: proc(f: ^Facts, path: string) -> bool {
	if f.host != nil {
		return f.host.is_modified(f.host.user, raw_data(path), uint(len(path))) != 0
	}
	current, original := value_at(f, path), default_value(f, path)
	a, a_ok := value.as_f64(current)
	b, b_ok := value.as_f64(original)
	if a_ok && b_ok {
		return abs(a - b) > 1e-6 * state.fmax(abs(b), 1.0)
	}
	return !value.same(current, original)
}

size_mode :: proc(f: ^Facts) -> state.Size_Mode {
	return state.size_mode(f.state.params) if f.state != nil else .Original
}

// Whether the Lab shows the long-edge slider: Custom… was picked, or the
// length isn't one of the stills formats.
film_format_custom :: proc(f: ^Facts) -> bool {
	return f.input.custom_film_format || state.film_format(value.number(value_at(f, state.adjustment_path(state.FILM_FORMAT)))) < 0
}

is_negative :: proc(f: ^Facts) -> bool {
	return f.state != nil && negative.is_route(value.string_of(value.get(f.state.params, negative.ROUTE_PATH)))
}

// MARK: Colour filters

// The filter in front of the lens, or "none".
color_filter_key :: proc(f: ^Facts) -> string {
	k := value.string_of(value_at(f, state.COLOR_FILTER_PATH))
	return k if len(k) > 0 else state.COLOR_FILTER_NONE
}

// The catalog's filter in front of the lens; nil for none.
chosen_color_filter :: proc(f: ^Facts) -> ^state.Color_Filter {
	if f.catalog == nil {
		return nil
	}
	key := color_filter_key(f)
	for &c in f.catalog.filters {
		if c.key == key {
			return &c
		}
	}
	return nil
}

// The chosen filter's name (edits), or ok false.
chosen_color_filter_name :: proc(f: ^Facts) -> (string, bool) {
	if f.host != nil {
		name: Str
		if f.host.color_filter(f.host.user, &name) == 0 {
			return "", false
		}
		return string(name.data[:name.len]) if name.len > 0 else "", true
	}
	c := chosen_color_filter(f)
	if c == nil {
		return "", false
	}
	return c.name, true
}

// How many stops the filter takes on the current film; none for colour
// films and for a film the glass leaves blind.
color_filter_stops :: proc(f: ^Facts, key: string) -> Maybe(f64) {
	if f.catalog == nil || f.state == nil {
		return nil
	}
	stops, found := f.catalog.stops[f.state.film]
	if !found {
		return nil
	}
	n, ok := value.as_f64(state.field(stops, key))
	if !ok {
		return nil
	}
	return n
}

// The module shows for B&W films, or while a filter is set.
color_filter_shown :: proc(f: ^Facts) -> bool {
	return f.catalog != nil && len(f.catalog.filters) > 0 && (film_is_bw(f) || color_filter_key(f) != state.COLOR_FILTER_NONE)
}

// MARK: Preferences

// Whether a module's or section's "N more" was clicked (`split(',')`, so
// an empty key is revealed by an empty list).
revealed :: proc(f: ^Facts, key: string) -> bool {
	rest := f.input.prefs.more_adjustments
	for {
		i := strings.index_byte(rest, ',')
		piece := rest if i < 0 else rest[:i]
		if piece == key {
			return true
		}
		if i < 0 {
			return false
		}
		rest = rest[i + 1:]
	}
}

filter_link :: proc(f: ^Facts) -> state.Filter_Link {
	return state.filter_link_from_key(f.input.prefs.filter_shift_link)
}

// The colour space the preview develops in: the one Save and Export last
// used. Static.
preview_space :: proc(chosen: string) -> cstring {
	for s in COLOR_SPACES {
		if string(s) == chosen {
			return s
		}
	}
	return COLOR_SPACES[0]
}

facts_preview_space :: proc(f: ^Facts) -> cstring {
	return preview_space(f.input.prefs.color_space)
}

// MARK: InspectorFacts

has_catalog :: proc(f: ^Facts) -> bool {
	return f.catalog != nil && len(f.catalog.stocks) > 0
}

// The development times the film's (or paper's) profile was measured at.
stock_times :: proc(f: ^Facts, paper: bool) -> []f64 {
	if f.catalog == nil || f.state == nil {
		return nil
	}
	id := f.state.paper if paper else f.state.film
	for s in f.catalog.stocks {
		if s.paper == paper && s.id == id {
			return s.times
		}
	}
	return nil
}

// The photograph's size in whole pixels; ok false before one opens.
photo_pixels :: proc(f: ^Facts) -> (w, h: u32, ok: bool) {
	s := f.input.photo_size
	if crop.is_zero(s) {
		return
	}
	return state.saturating_u32(s.width), state.saturating_u32(s.height), true
}

negative_busy :: proc(f: ^Facts) -> bool {
	n, ok := f.input.negative.?
	return ok && n.busy
}
