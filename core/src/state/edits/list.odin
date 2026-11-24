// The edit lists (edits.rs's `edits`): each tab's controls off their
// defaults, in the tab's order, with their values as the list shows them.
// Always as many as `count`.
package edits

import "base:runtime"
import "core:strings"
import "../../value"
import "../facts"
import ".."

@(private)
List :: struct {
	f:       ^facts.Facts,
	out:     ^[dynamic]Edit,
	decimal: rune,
	a:       runtime.Allocator,
}

@(private)
push :: proc(l: ^List, title: string, text: string, target: Target_Kind, path: cstring = nil) {
	append(l.out, Edit{strings.clone(title, l.a), text, target, path})
}

// A parameter's row when it is modified: its value as `display` writes it.
@(private)
param :: proc(l: ^List, title: string, path: cstring, unit := "") {
	if !facts.is_modified(l.f, string(path)) {
		return
	}
	b := strings.builder_make(0, 16, l.a)
	write_display(&b, facts.value_at(l.f, string(path)), l.decimal)
	strings.write_string(&b, unit)
	push(l, title, strings.to_string(b), .Param, path)
}

@(private)
text :: proc(l: ^List, s: string) -> string {
	return strings.clone(s, l.a)
}

@(private)
number_text :: proc(l: ^List, v: f64) -> string {
	b := strings.builder_make(0, 16, l.a)
	write_number(&b, v, l.decimal)
	return strings.to_string(b)
}

@(private)
signed_text :: proc(l: ^List, v: f64) -> string {
	b := strings.builder_make(0, 16, l.a)
	write_signed(&b, v, l.decimal)
	return strings.to_string(b)
}

@(private)
append_control :: proc(l: ^List, c: state.Adjustment, group: string) {
	if !control_modified(c, l.f) {
		return
	}
	title := string(c.title)
	#partial switch c.kind {
	case .Slider:
		if facts.film_is_bw(l.f) {
			if t := state.bw_title(string(c.path)); t != nil {
				title = string(t)
			}
		}
	}
	b := strings.builder_make(0, 32, l.a)
	if len(group) > 0 {
		strings.write_string(&b, group)
		strings.write_string(&b, " · ")
	}
	strings.write_string(&b, title)
	titled := strings.to_string(b)
	v := facts.value_at(l.f, string(c.path))
	#partial switch c.kind {
	case .Slider:
		n := value.number(v)
		append(l.out, Edit{titled, signed_text(l, n) if c.low < 0.0 else number_text(l, n), .Param, c.path})
	case .Toggle:
		append(l.out, Edit{titled, text(l, on_off(value.boolean(v))), .Param, c.path})
	case .Choice:
		append(l.out, Edit{titled, state.option_title(value.string_of(v), l.a), .Param, c.path})
	}
}

on_off :: proc "contextless" (on: bool) -> string {
	return "On" if on else "Off"
}

// The controls on `tab` that are off their defaults, in the tab's order,
// in `allocator`.
edits :: proc(tab: Tab, f: ^facts.Facts, decimal: rune, allocator := context.allocator) -> [dynamic]Edit {
	out := make([dynamic]Edit, 0, 8, allocator)
	l := List{f, &out, decimal, allocator}
	switch tab {
	case .Prep:
		if f.input.is_raw {
			param(&l, "White balance", WHITE_BALANCE_PATHS[0])
			param(&l, "Temperature (K)", WHITE_BALANCE_PATHS[1])
			param(&l, "Tint", WHITE_BALANCE_PATHS[2])
		}
		if facts.matches(f) != f.input.prefs.match_new_photos {
			push(&l, "Match camera brightness", text(&l, on_off(facts.matches(f))), .Match_Brightness)
		}
		if facts.exposure_offset_modified(f) {
			push(&l, "Exposure", signed_text(&l, facts.exposure_offset(f)), .Exposure)
		}
		param(&l, "Auto exposure", AUTO_EXPOSURE[0])
		param(&l, "Metering", AUTO_EXPOSURE[1])
		for list in ([3][]state.Adjustment{state.PREP_TONE, state.PREP_COLOR, state.PREP_DETAIL}) {
			for c in list {
				append_control(&l, c, "")
			}
		}
		if f.input.is_raw {
			if f.input.lens_supported {
				param(&l, "Lens corrections", state.LENS_CORRECTION)
			}
		} else {
			for p in INPUT_PATHS {
				b := strings.builder_make(0, 32, context.temp_allocator)
				strings.write_string(&b, "Input ")
				write_words(&b, string(p))
				param(&l, strings.to_string(b), p)
			}
		}
	case .Lab:
		for section in SECTIONS {
			switch section {
			case .Camera:
				param(&l, "Exposure compensation", CAMERA_LOOSE[0])
				format := state.FILM_FORMAT.path
				if facts.is_modified(f, string(format)) {
					mm := value.number(facts.value_at(f, string(format)))
					i := state.film_format(mm)
					v: string
					if i >= 0 {
						v = text(&l, string(state.FILM_FORMATS[i].name))
					} else {
						b := strings.builder_make(0, 16, allocator)
						write_number(&b, mm, decimal)
						strings.write_string(&b, " mm")
						v = strings.to_string(b)
					}
					push(&l, "Film format", v, .Film_Format)
				}
				param(&l, "Lens blur (µm)", CAMERA_LOOSE[2])
				if facts.is_modified(f, state.COLOR_FILTER_PATH) {
					name, ok := facts.chosen_color_filter_name(f)
					push(&l, "Color filter", text(&l, name if ok else "None"), .Param, state.COLOR_FILTER_PATH)
				}
			case .Film:
				param(&l, "Film development", FILM_DEVELOPMENT, " min")
			case .Print:
				if scan_film_modified(f) {
					push(&l, "Scan film directly", text(&l, on_off(value.boolean(facts.value_at(f, state.SCAN_FILM)))), .Scan_Film)
				}
				param(&l, "Print development", PRINT_DEVELOPMENT, " min")
			case .Scan:
				space := facts.facts_preview_space(f)
				if space != facts.COLOR_SPACES[0] {
					push(&l, "Colour space", text(&l, string(space)), .Preview_Space)
				}
				param(&l, "Encode with the colour space’s curve", CCTF)
			}
			for g in state.ADJUSTMENT_GROUPS {
				if g.section != section {
					continue
				}
				if len(g.enable) > 0 && facts.is_modified(f, string(g.enable)) {
					push(&l, string(g.title), text(&l, on_off(value.boolean(facts.value_at(f, string(g.enable))))), .Param, g.enable)
				}
				for c in g.controls {
					append_control(&l, c, string(g.title))
				}
			}
		}
	case .Output:
		if facts.is_modified(f, state.SIZE_TARGET) {
			b := strings.builder_make(0, 16, allocator)
			write_number(&b, value.number(facts.value_at(f, state.SIZE_TARGET)), decimal)
			strings.write_string(&b, " px")
			push(&l, "Image size", strings.to_string(b), .Param, state.SIZE_TARGET)
		}
		if facts.border_enabled(f) {
			push(&l, "Border", text(&l, "On"), .Border)
		}
	}
	return out
}

// MARK: Words and numbers

// "as_shot" as "As shot"; a path's last part likewise.
write_words :: proc(b: ^strings.Builder, s: string) {
	last := s
	if i := strings.last_index_byte(s, '.'); i >= 0 {
		last = s[i + 1:]
	}
	spaced := make([]u8, len(last), context.temp_allocator)
	for c, i in transmute([]u8)last {
		spaced[i] = ' ' if c == '_' else c
	}
	state.write_capitalised(b, string(spaced))
}

words :: proc(s: string, allocator := context.allocator) -> string {
	b := strings.builder_make(0, len(s) + 4, allocator)
	write_words(&b, s)
	return strings.to_string(b)
}

// Up to two decimals, without grouping, with the locale's point.
write_number :: proc(b: ^strings.Builder, v: f64, decimal: rune) {
	buf: [400]u8
	t := strings.builder_from_bytes(buf[:])
	state.write_fixed(&t, v, 2)
	s := strings.trim_right(strings.trim_right(strings.to_string(t), "0"), ".")
	if s == "-0" {
		s = "0"
	}
	for c in transmute([]u8)s {
		if c == '.' {
			strings.write_rune(b, decimal)
		} else {
			strings.write_byte(b, c)
		}
	}
}

// With its sign, except at zero.
write_signed :: proc(b: ^strings.Builder, v: f64, decimal: rune) {
	start := strings.builder_len(b^)
	write_number(b, v, decimal)
	if v > 0.0 && string(b.buf[start:]) != "0" {
		inject_at(&b.buf, start, '+')
	}
}

write_display :: proc(b: ^strings.Builder, v: value.Value, decimal: rune) {
	#partial switch x in v {
	case u64, i64, f64:
		write_number(b, value.number(v), decimal)
	case bool:
		strings.write_string(b, on_off(x))
	case string:
		write_words(b, x)
	case:
		strings.write_string(b, "Changed")
	}
}
