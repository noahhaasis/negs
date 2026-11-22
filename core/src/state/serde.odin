// EditState, BorderSettings, CropSettings, BrightnessMatch and Roll read
// and written as their serde derives do with serde_json:
//
// - Reading (`serde_json::from_slice::<EditState>`): a struct is an object
//   (fields by their camelCase names; unknown ones skipped as `IgnoredAny`
//   skips them; a known field twice is an error) or an array (fields in
//   declaration order). A missing `#[serde(default)]` field takes its
//   default (BorderSettings and CropSettings default every field); a
//   missing other one is an error, except an Option, which is none. An
//   Option is none for null. Enums are their name as a string, or
//   `{"name": null}`. Nesting counts toward serde_json's 128 levels.
// - Writing (`serde_json::to_string(&state)`): fields in declaration
//   order, `brightnessMatch`, `roll` and the roll's `paper` left out when
//   none, doubles that aren't finite as null. `sorted` writes what
//   `serde_json::to_value(&state)` holds instead (what `json!` embeds):
//   the same fields, each object's keys sorted.
package state

import "core:slice"
import "core:strings"
import border "../rules/border"
import crop "../rules/crop"
import "../value"

@(private = "file")
Parser :: value.Parser

// MARK: Reading

@(private = "file")
read_string :: proc(p: ^Parser) -> (string, bool) {
	c, any := value.peek_ws(p)
	if !any {
		return "", value.fail(p, .Eof)
	}
	if c != '"' {
		return "", value.fail(p, .Invalid_Type)
	}
	p.i += 1
	return value.parse_str(p)
}

@(private = "file")
read_bool :: proc(p: ^Parser) -> (bool, bool) {
	c, any := value.peek_ws(p)
	if !any {
		return false, value.fail(p, .Eof)
	}
	switch c {
	case 't':
		p.i += 1
		return true, value.parse_ident(p, "rue")
	case 'f':
		p.i += 1
		return false, value.parse_ident(p, "alse")
	}
	return false, value.fail(p, .Invalid_Type)
}

// `deserialize_f64`: any JSON number, as a double.
@(private = "file")
read_f64 :: proc(p: ^Parser) -> (n: f64, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return 0, value.fail(p, .Eof)
	}
	if c != '-' && (c < '0' || c > '9') {
		return 0, value.fail(p, .Invalid_Type)
	}
	v := value.parse_number(p) or_return
	n, _ = value.as_f64(v)
	return n, true
}

// `deserialize_option`: none for null, else `read`.
@(private = "file")
read_null :: proc(p: ^Parser) -> (is_null: bool, ok: bool) {
	c, any := value.peek_ws(p)
	if any && c == 'n' {
		p.i += 1
		return true, value.parse_ident(p, "ull")
	}
	return false, true
}

// `[f64; 2]`: an array of exactly two numbers.
@(private = "file")
read_pair :: proc(p: ^Parser) -> (out: [2]f64, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return out, value.fail(p, .Eof)
	}
	if c != '[' {
		return out, value.fail(p, .Invalid_Type)
	}
	value.enter(p) or_return
	p.i += 1
	first := true
	for k in 0 ..< 2 {
		more := value.seq_next(p, &first) or_return
		if !more {
			return out, value.fail(p, .Invalid_Length)
		}
		out[k] = read_f64(p) or_return
	}
	value.leave(p)
	return out, value.seq_end(p)
}

// CropSettings' rect, `[[x, y], [width, height]]`.
@(private = "file")
read_rect :: proc(p: ^Parser) -> (out: crop.Rect, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return out, value.fail(p, .Eof)
	}
	if c != '[' {
		return out, value.fail(p, .Invalid_Type)
	}
	value.enter(p) or_return
	p.i += 1
	first := true
	pairs: [2][2]f64
	for k in 0 ..< 2 {
		more := value.seq_next(p, &first) or_return
		if !more {
			return out, value.fail(p, .Invalid_Length)
		}
		pairs[k] = read_pair(p) or_return
	}
	value.leave(p)
	value.seq_end(p) or_return
	return crop.Rect{pairs[0][0], pairs[0][1], pairs[1][0], pairs[1][1]}, true
}

// A unit enum: `"name"`, or `{"name": null}`; the index of its name in
// `names`.
@(private = "file")
read_variant :: proc(p: ^Parser, names: []string) -> (index: int, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return 0, value.fail(p, .Eof)
	}
	find :: proc(p: ^Parser, names: []string) -> (int, bool) {
		name, ok := value.parse_str(p)
		defer delete(name, p.allocator)
		if !ok {
			return 0, false
		}
		for n, i in names {
			if n == name {
				return i, true
			}
		}
		return 0, value.fail(p, .Unknown_Variant)
	}
	switch c {
	case '"':
		p.i += 1
		return find(p, names)
	case '{':
		value.enter(p) or_return
		p.i += 1
		d, more := value.peek_ws(p)
		if !more {
			return 0, value.fail(p, .Eof)
		}
		if d != '"' {
			return 0, value.fail(p, .Invalid_Type)
		}
		p.i += 1
		index = find(p, names) or_return
		d, more = value.peek_ws(p)
		if !more {
			return 0, value.fail(p, .Eof)
		}
		if d != ':' {
			return 0, value.fail(p, .Expected_Colon)
		}
		p.i += 1
		// The unit variant's content: `()`, which is null.
		d, more = value.peek_ws(p)
		if !more {
			return 0, value.fail(p, .Eof)
		}
		if d != 'n' {
			return 0, value.fail(p, .Invalid_Type)
		}
		p.i += 1
		value.parse_ident(p, "ull") or_return
		value.leave(p)
		d, more = value.peek_ws(p)
		if !more {
			return 0, value.fail(p, .Eof)
		}
		if d != '}' {
			return 0, value.fail(p, .Expected_Value)
		}
		p.i += 1
		return index, true
	}
	return 0, value.fail(p, .Expected_Value)
}

// A struct's fields, as a derive's visit_map or visit_seq takes them: an
// object (by name) or an array (in order). `field` reads field `index`
// into `data`; `seen` says which were there.
@(private = "file")
read_struct :: proc(
	p: ^Parser,
	names: []string,
	data: rawptr,
	field: proc(p: ^Parser, index: int, data: rawptr) -> bool,
) -> (
	seen: bit_set[0 ..< 16],
	ok: bool,
) {
	c, any := value.peek_ws(p)
	if !any {
		return seen, value.fail(p, .Eof)
	}
	switch c {
	case '{':
		value.enter(p) or_return
		p.i += 1
		first := true
		for {
			more := value.map_next(p, &first) or_return
			if !more {
				break
			}
			key, good := value.map_key(p)
			index := -1
			for n, i in names {
				if n == key {
					index = i
					break
				}
			}
			delete(key, p.allocator)
			good or_return
			if index < 0 {
				value.ignore_value(p) or_return
				continue
			}
			if index in seen {
				return seen, value.fail(p, .Duplicate_Field)
			}
			field(p, index, data) or_return
			seen += {index}
		}
		value.leave(p)
		return seen, value.map_end(p)
	case '[':
		value.enter(p) or_return
		p.i += 1
		first := true
		for i in 0 ..< len(names) {
			more := value.seq_next(p, &first) or_return
			if !more {
				break
			}
			field(p, i, data) or_return
			seen += {i}
		}
		value.leave(p)
		return seen, value.seq_end(p)
	}
	return seen, value.fail(p, .Invalid_Type)
}

BORDER_FIELDS := [?]string{"enabled", "style", "export", "instagram", "width", "filmWidth", "line", "color", "lineColor"}
BORDER_STYLE_NAMES := [?]string{"white", "film", "keyline", "35mm"}
CROP_FIELDS := [?]string{"rect", "aspect", "custom", "angle"}
CROP_ASPECT_NAMES := [?]string{"original", "freeform", "square", "16:9", "3:2", "4:3", "5:4", "7:5", "custom"}
MATCH_FIELDS := [?]string{"ev", "inputs", "highlightLimited"}
ROLL_FIELDS := [?]string{"id", "magenta", "yellow", "paper"}
STATE_FIELDS := [?]string{"params", "film", "paper", "limitPreview", "border", "crop", "brightnessMatch", "roll"}

// `#rrggbb` or `rrggbb`, either case (BorderColor's Deserialize).
@(private = "file")
read_color :: proc(p: ^Parser) -> (c: border.Color, ok: bool) {
	text := read_string(p) or_return
	defer delete(text, p.allocator)
	found, is_hex := border.from_hex(text).?
	if !is_hex {
		return {}, value.fail(p, .Invalid_Value)
	}
	return found, true
}

read_border :: proc(p: ^Parser) -> (border.Settings, bool) {
	out := border.DEFAULT_SETTINGS
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		s := (^border.Settings)(data)
		switch index {
		case 0:
			s.enabled = read_bool(p) or_return
		case 1:
			i := read_variant(p, BORDER_STYLE_NAMES[:]) or_return
			s.style = border.Style(i)
		case 2:
			s.export = read_bool(p) or_return
		case 3:
			s.instagram = read_bool(p) or_return
		case 4:
			s.width = read_f64(p) or_return
		case 5:
			s.film_width = read_f64(p) or_return
		case 6:
			s.line = read_f64(p) or_return
		case 7:
			s.color = read_color(p) or_return
		case 8:
			s.line_color = read_color(p) or_return
		}
		return true
	}
	_, ok := read_struct(p, BORDER_FIELDS[:], &out, field)
	return out, ok
}

read_crop :: proc(p: ^Parser) -> (crop.Settings, bool) {
	out := crop.DEFAULT_SETTINGS
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		s := (^crop.Settings)(data)
		switch index {
		case 0:
			s.rect = read_rect(p) or_return
		case 1:
			i := read_variant(p, CROP_ASPECT_NAMES[:]) or_return
			s.aspect = crop.Aspect(i)
		case 2:
			pair := read_pair(p) or_return
			s.custom = crop.Size{pair[0], pair[1]}
		case 3:
			s.angle = read_f64(p) or_return
		}
		return true
	}
	_, ok := read_struct(p, CROP_FIELDS[:], &out, field)
	return out, ok
}

read_match :: proc(p: ^Parser) -> (out: Brightness_Match, ok: bool) {
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		m := (^Brightness_Match)(data)
		switch index {
		case 0:
			if is_null := read_null(p) or_return; !is_null {
				m.ev = read_f64(p) or_return
			}
		case 1:
			if is_null := read_null(p) or_return; !is_null {
				m.inputs = value.parse_value(p) or_return
				m.has_inputs = true
			}
		case 2:
			m.highlight_limited = read_bool(p) or_return
		}
		return true
	}
	_, ok = read_struct(p, MATCH_FIELDS[:], &out, field)
	if !ok {
		value.destroy(out.inputs, p.allocator)
		return {}, false
	}
	return out, true
}

read_roll :: proc(p: ^Parser) -> (out: Roll, ok: bool) {
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		r := (^Roll)(data)
		switch index {
		case 0:
			r.id = read_string(p) or_return
		case 1:
			r.magenta = read_f64(p) or_return
		case 2:
			r.yellow = read_f64(p) or_return
		case 3:
			if is_null := read_null(p) or_return; !is_null {
				r.paper = read_string(p) or_return
			}
		}
		return true
	}
	seen: bit_set[0 ..< 16]
	seen, ok = read_struct(p, ROLL_FIELDS[:], &out, field)
	if ok && !({0, 1, 2} <= seen) {
		ok = value.fail(p, .Missing_Field)
	}
	if !ok {
		delete(out.id, p.allocator)
		if paper, has := out.paper.?; has {
			delete(paper, p.allocator)
		}
		return {}, false
	}
	return out, true
}

// An EditState at the parser's place.
read_state :: proc(p: ^Parser) -> (out: Edit_State, ok: bool) {
	out.border = border.DEFAULT_SETTINGS
	out.crop = crop.DEFAULT_SETTINGS
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		s := (^Edit_State)(data)
		switch index {
		case 0:
			s.params = value.parse_value(p) or_return
		case 1:
			s.film = read_string(p) or_return
		case 2:
			s.paper = read_string(p) or_return
		case 3:
			s.limit_preview = read_bool(p) or_return
		case 4:
			s.border = read_border(p) or_return
		case 5:
			s.crop = read_crop(p) or_return
		case 6:
			if is_null := read_null(p) or_return; !is_null {
				s.brightness_match = read_match(p) or_return
			}
		case 7:
			if is_null := read_null(p) or_return; !is_null {
				s.roll = read_roll(p) or_return
			}
		}
		return true
	}
	seen: bit_set[0 ..< 16]
	seen, ok = read_struct(p, STATE_FIELDS[:], &out, field)
	if ok && !({0, 1, 2} <= seen) {
		ok = value.fail(p, .Missing_Field)
	}
	if !ok {
		destroy(out, p.allocator)
		return {}, false
	}
	return out, true
}

// `serde_json::from_slice::<EditState>`: the state in `allocator`, or why
// not.
parse :: proc(text: []u8, allocator := context.allocator) -> (Edit_State, value.Error) {
	p := value.parser_make(text, allocator)
	s, ok := read_state(&p)
	if ok && value.finish(&p) {
		return s, .None
	}
	if ok {
		destroy(s, allocator)
	}
	return {}, p.err if p.err != .None else .Invalid_Type
}

// MARK: Writing

// The structs as a Value in which each struct's object keeps its fields in
// declaration order (`sorted` false) or sorted (true): value.write_compact
// and write_pretty write objects in the order they hold. `params` and the
// inputs are borrowed, not copied: free the result with `free_encoded`.
@(private = "file")
Encoder :: struct {
	sorted:    bool,
	allocator: value.Allocator,
}

@(private = "file")
obj :: proc(e: ^Encoder, n: int) -> value.Object {
	return value.make_object(n, e.allocator)
}

@(private = "file")
put :: proc(o: ^value.Object, key: string, v: value.Value) {
	append(o, value.Field{key, v})
}

@(private = "file")
done :: proc(e: ^Encoder, o: value.Object) -> value.Value {
	if e.sorted {
		slice.sort_by(o[:], proc(a, b: value.Field) -> bool {return a.key < b.key})
	}
	return o
}

@(private = "file")
num :: proc(v: f64) -> value.Value {
	return value.from_f64(v)
}

@(private = "file")
pair :: proc(e: ^Encoder, a, b: f64) -> value.Value {
	out := value.make_array(2, e.allocator)
	append(&out, num(a), num(b))
	return out
}

@(private = "file")
hex :: proc(e: ^Encoder, c: border.Color) -> value.Value {
	h := border.hex(c)
	return strings.clone(string(h[:]), e.allocator)
}

@(private = "file")
encode_border :: proc(e: ^Encoder, s: border.Settings) -> value.Value {
	o := obj(e, 9)
	put(&o, "enabled", s.enabled)
	put(&o, "style", BORDER_STYLE_NAMES[s.style])
	put(&o, "export", s.export)
	put(&o, "instagram", s.instagram)
	put(&o, "width", num(s.width))
	put(&o, "filmWidth", num(s.film_width))
	put(&o, "line", num(s.line))
	put(&o, "color", hex(e, s.color))
	put(&o, "lineColor", hex(e, s.line_color))
	return done(e, o)
}

@(private = "file")
encode_crop :: proc(e: ^Encoder, s: crop.Settings) -> value.Value {
	o := obj(e, 4)
	rect := value.make_array(2, e.allocator)
	append(&rect, pair(e, s.rect.x, s.rect.y), pair(e, s.rect.width, s.rect.height))
	put(&o, "rect", rect)
	aspect := int(s.aspect) if int(s.aspect) < len(CROP_ASPECT_NAMES) else 0
	put(&o, "aspect", CROP_ASPECT_NAMES[aspect])
	put(&o, "custom", pair(e, s.custom.width, s.custom.height))
	put(&o, "angle", num(s.angle))
	return done(e, o)
}

@(private = "file")
encode :: proc(e: ^Encoder, s: Edit_State) -> value.Value {
	o := obj(e, 8)
	put(&o, "params", s.params)
	put(&o, "film", s.film)
	put(&o, "paper", s.paper)
	put(&o, "limitPreview", s.limit_preview)
	put(&o, "border", encode_border(e, s.border))
	put(&o, "crop", encode_crop(e, s.crop))
	if m, ok := s.brightness_match.?; ok {
		mo := obj(e, 3)
		ev, has_ev := m.ev.?
		put(&mo, "ev", num(ev) if has_ev else nil)
		put(&mo, "inputs", m.inputs if m.has_inputs else nil)
		put(&mo, "highlightLimited", m.highlight_limited)
		put(&o, "brightnessMatch", done(e, mo))
	}
	if r, ok := s.roll.?; ok {
		ro := obj(e, 4)
		put(&ro, "id", r.id)
		put(&ro, "magenta", num(r.magenta))
		put(&ro, "yellow", num(r.yellow))
		if paper, has := r.paper.?; has {
			put(&ro, "paper", paper)
		}
		put(&o, "roll", done(e, ro))
	}
	return done(e, o)
}

// Frees what `encode` made, not what it borrowed.
@(private = "file")
free_encoded :: proc(v: value.Value, allocator: value.Allocator) {
	o := v.(value.Object)
	for f in o {
		switch f.key {
		case "border":
			b := f.value.(value.Object)
			for g in b {
				if g.key == "color" || g.key == "lineColor" {
					delete(g.value.(string), allocator)
				}
			}
			delete(b)
		case "crop":
			c := f.value.(value.Object)
			for g in c {
				if a, is_array := g.value.(value.Array); is_array {
					for item in a {
						if inner, nested := item.(value.Array); nested {
							delete(inner)
						}
					}
					delete(a)
				}
			}
			delete(c)
		case "brightnessMatch", "roll":
			delete(f.value.(value.Object))
		}
	}
	delete(o)
}

// `serde_json::to_string(&state)` (or `to_string_pretty`); with `sorted`,
// what `to_value(&state)` writes (as inside `json!`).
write :: proc(b: ^strings.Builder, s: Edit_State, sorted := false, pretty := false, depth := 0) {
	e := Encoder{sorted, context.allocator}
	v := encode(&e, s)
	defer free_encoded(v, context.allocator)
	if pretty {
		value.write_pretty(b, v, depth)
	} else {
		value.write_compact(b, v)
	}
}

to_string :: proc(s: Edit_State, sorted := false, pretty := false, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 1024, allocator)
	write(&b, s, sorted, pretty)
	return strings.to_string(b)
}

// `serde_json::to_value(&state)`: a Value of its own (keys sorted), in
// `allocator`.
to_value :: proc(s: Edit_State, allocator := context.allocator) -> value.Value {
	e := Encoder{true, context.allocator}
	v := encode(&e, s)
	defer free_encoded(v, context.allocator)
	return value.clone(v, allocator)
}
