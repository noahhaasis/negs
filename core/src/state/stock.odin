// The catalog's films and papers (state.rs's `Stock`, `Stock.swift`), the
// Settings that hide some of them from the lists (Fotufilm films, Show
// Negs-built films), and the camera colour filters (`ColorFilter`).
package state

import "core:strings"
import "../value"

Stock :: struct {
	id:                string,
	name:              string,
	paper:             bool,
	positive:          bool,
	bw:                bool,
	target:            string,
	times:             []f64,
	// The resolved grain preset, per field of GRAIN_STOCK_FIELDS.
	grain:             value.Value,
	// Ported from Fotufilm; and, for those, from the manufacturer's data.
	fotufilm:          bool,
	manufacturer_data: bool,
	// Prepared by Negs itself from datasheets.
	negs_built:        bool,
	// A motion-picture print film (catalog `cine_print`).
	cine_print:        bool,
}

// `value[key]`: an object's field, null for anything else.
field :: proc(v: value.Value, key: string) -> value.Value {
	if o, ok := v.(value.Object); ok {
		f, _ := value.object_get(o, key)
		return f
	}
	return nil
}

// `Stock::new`: a catalog profile's fields; a catalog from before the
// Fotufilm and Negs-built fields lists every film. In `allocator`.
stock_from_value :: proc(v: value.Value, allocator := context.allocator) -> Stock {
	s := Stock {
		id                = strings.clone(value.string_of(field(v, "id")), allocator),
		name              = strings.clone(value.string_of(field(v, "name")), allocator),
		paper             = value.boolean(field(v, "paper")),
		positive          = value.boolean(field(v, "positive")),
		bw                = value.boolean(field(v, "bw")),
		target            = strings.clone(value.string_of(field(v, "target_print")), allocator),
		grain             = value.clone(field(v, "grain"), allocator),
		fotufilm          = value.boolean(field(v, "fotufilm")),
		negs_built        = value.boolean(field(v, "negs_built")),
		cine_print        = value.boolean(field(v, "cine_print")),
	}
	b, is_bool := field(v, "manufacturer_data").(bool)
	s.manufacturer_data = !(is_bool && !b)
	if times, ok := field(v, "development_times").(value.Array); ok {
		s.times = make([]f64, len(times), allocator)
		for t, i in times {
			s.times[i] = value.number(t)
		}
	}
	return s
}

stock_destroy :: proc(s: Stock, allocator := context.allocator) {
	delete(s.id, allocator)
	delete(s.name, allocator)
	delete(s.target, allocator)
	delete(s.times, allocator)
	value.destroy(s.grain, allocator)
}

is_motion_picture_print :: proc(s: Stock) -> bool {
	// A catalog from before `cine_print` still knows Kodak's by name.
	return s.paper && (s.cine_print || strings.contains(s.name, "Vision") || strings.contains(s.name, "Print Film"))
}

// Settings › Show Negs-built films: the films Negs prepared itself stay out
// of the lists until shown.
lists_negs_built :: proc "contextless" (s: Stock, shown: bool) -> bool {
	return shown || !s.negs_built
}

NEGS_BUILT_HINT :: "Films Negs prepared itself from Fujifilm’s datasheets, still being checked: the Eterna cine films; Pro 160S, 160C and 800Z, Superia 1600 and True Definition 400; the Velvia 100F, Provia 400F and 400X, Sensia and 64T slides; and the Eterna-CP print films. Photos already using one keep it."

// Settings › Fotufilm films: which of the films ported from Fotufilm the
// lists show. An edit or preset with a hidden film keeps it.
Fotufilm_Films :: enum u32 {
	All,
	Datasheets,
	Off,
}

// As the Mac stores it; anything else is All.
fotufilm_from_key :: proc "contextless" (key: string) -> Fotufilm_Films {
	switch key {
	case "datasheets":
		return .Datasheets
	case "off":
		return .Off
	}
	return .All
}

FOTUFILM_KEYS := [Fotufilm_Films]cstring {
	.All        = "all",
	.Datasheets = "datasheets",
	.Off        = "off",
}

FOTUFILM_TITLES := [Fotufilm_Films]cstring {
	.All        = "All",
	.Datasheets = "From datasheets",
	.Off        = "None",
}

fotufilm_lists :: proc "contextless" (f: Fotufilm_Films, s: Stock) -> bool {
	switch f {
	case .Datasheets:
		return !s.fotufilm || s.manufacturer_data
	case .Off:
		return !s.fotufilm
	case .All:
	}
	return true
}

// MARK: Colour filters

// A camera colour filter from the catalog (issue #21): its params key, the
// colour it's named by, the measured glass, and the glass's colour.
Color_Filter :: struct {
	key:    string,
	name:   string,
	glass:  string,
	colour: u32,
}

COLOR_FILTER_PATH :: "camera.color_filter"
COLOR_FILTER_NONE :: "none"
COLOR_FILTER_GREY :: 0x888888

// `ColorFilter::new`. In `allocator`.
color_filter_from_value :: proc(v: value.Value, allocator := context.allocator) -> Color_Filter {
	return Color_Filter {
		key = strings.clone(value.string_of(field(v, "key")), allocator),
		name = strings.clone(value.string_of(field(v, "name")), allocator),
		glass = strings.clone(value.string_of(field(v, "glass")), allocator),
		colour = parse_hex_colour(value.string_of(field(v, "colour"))),
	}
}

color_filter_destroy :: proc(f: Color_Filter, allocator := context.allocator) {
	delete(f.key, allocator)
	delete(f.name, allocator)
	delete(f.glass, allocator)
}

// `u32::from_str_radix(text.trim_start_matches('#'), 16)`, grey when it
// isn't one.
parse_hex_colour :: proc "contextless" (text: string) -> u32 {
	s := text
	for len(s) > 0 && s[0] == '#' {
		s = s[1:]
	}
	if len(s) > 0 && s[0] == '+' && len(s) > 1 {
		s = s[1:]
	}
	if len(s) == 0 {
		return COLOR_FILTER_GREY
	}
	n: u64 = 0
	for i in 0 ..< len(s) {
		c := s[i]
		d: u64
		switch c {
		case '0' ..= '9':
			d = u64(c - '0')
		case 'a' ..= 'f':
			d = u64(c - 'a' + 10)
		case 'A' ..= 'F':
			d = u64(c - 'A' + 10)
		case:
			return COLOR_FILTER_GREY
		}
		n = n * 16 + d
		if n > u64(max(u32)) {
			return COLOR_FILTER_GREY
		}
	}
	return u32(n)
}

// What each filter does to a black-and-white photograph.
color_filter_effect :: proc "contextless" (key: string) -> cstring {
	switch key {
	case "hoya_x1":
		return "Lightens foliage; darkens red lips and skin a little, for a tanned look."
	case "hoya_x0":
		return "Lightens foliage and darkens the sky a little. Kind to skin outdoors."
	case "hoya_y2":
		return "Darkens blue sky a touch so clouds stand out. The everyday filter."
	case "hoya_ya3":
		return "Darker sky, stronger clouds, less haze. Hides freckles and blemishes."
	case "hoya_r1":
		return "Near-black blue sky, bright red and brick, pale skin and lips. Cuts haze hard."
	}
	return "No glass in front of the lens."
}

// What the filter does, then what it costs the film: "… Takes 1,3 stops
// on Ilford HP5 Plus; auto exposure gives it." `decimal` is the locale's
// separator. No filter: the effect of none.
color_filter_hint :: proc(filter: ^Color_Filter, stops: Maybe(f64), film: string, metered: bool, decimal: rune, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 128, allocator)
	if filter == nil {
		strings.write_string(&b, string(color_filter_effect(COLOR_FILTER_NONE)))
		return strings.to_string(b)
	}
	strings.write_string(&b, string(color_filter_effect(filter.key)))
	if s, ok := stops.?; ok {
		rounded := round(s * 10.0) / 10.0
		strings.write_string(&b, " Takes ")
		if rounded - trunc(rounded) == 0.0 {
			write_fixed(&b, rounded, 0)
		} else {
			write_replacing_point(&b, rounded, 1, decimal)
		}
		strings.write_string(&b, " stop")
		if !(abs(s - 1.0) < 0.05) {
			strings.write_byte(&b, 's')
		}
		strings.write_string(&b, " on ")
		strings.write_string(&b, film)
		strings.write_string(&b, "; auto exposure gives it." if metered else ".")
	}
	return strings.to_string(b)
}

// `format!("{x:.places}").replace('.', decimal)`.
write_replacing_point :: proc(b: ^strings.Builder, x: f64, places: int, decimal: rune) {
	buf: [400]u8
	t := strings.builder_from_bytes(buf[:])
	write_fixed(&t, x, places)
	for c in transmute([]u8)strings.to_string(t) {
		if c == '.' {
			strings.write_rune(b, decimal)
		} else {
			strings.write_byte(b, c)
		}
	}
}

// The disc's tooltip.
color_filter_help :: proc(filter: ^Color_Filter, allocator := context.allocator) -> string {
	if filter == nil {
		return strings.clone("No filter in front of the lens.", allocator)
	}
	return strings.concatenate({filter.name, " · ", filter.glass, ". ", string(color_filter_effect(filter.key))}, allocator)
}
