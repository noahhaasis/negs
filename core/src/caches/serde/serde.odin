// serde's derives with serde_json, for the batch journal, the match cache
// and the rendered preview's header (core/src/state does the edit state):
//
// - Reading a struct: an object (fields by name; unknown ones skipped as
//   `IgnoredAny` skips them; a known field twice is an error; a missing
//   Option is none, a missing other field an error unless it has a
//   default) or an array (fields in declaration order; a missing field is
//   an error unless it has a default, Options included). Numbers: u32,
//   u64, usize and i32 fields take integers that fit (never a double, not
//   even 1.0 or -0); f64 fields any number. An Option is none for null.
//   Unit enums are their name, or `{"name": null}`.
// - Writing: compact, fields in declaration order, none as null, doubles
//   that aren't finite as null.
package serde

import "core:strings"
import "../../value"

Parser :: value.Parser

// The fields a struct read took, and whether it was the array form.
Fields :: struct {
	seen: bit_set[0 ..< 32],
	seq:  bool,
}

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

// serde_json's deserialize_number: '-' or a digit, else the wrong type.
@(private)
read_number :: proc(p: ^Parser) -> (value.Value, bool) {
	c, any := value.peek_ws(p)
	if !any {
		return nil, value.fail(p, .Eof)
	}
	if c != '-' && (c < '0' || c > '9') {
		return nil, value.fail(p, .Invalid_Type)
	}
	return value.parse_number(p)
}

// `deserialize_f64`: any JSON number, as a double.
read_f64 :: proc(p: ^Parser) -> (n: f64, ok: bool) {
	v := read_number(p) or_return
	n, _ = value.as_f64(v)
	return n, true
}

// `deserialize_u64`: a non-negative integer (a negative one is an invalid
// value, a double the wrong type).
read_u64 :: proc(p: ^Parser) -> (out: u64, ok: bool) {
	v := read_number(p) or_return
	#partial switch n in v {
	case u64:
		return n, true
	case i64:
		return 0, value.fail(p, .Invalid_Value)
	}
	return 0, value.fail(p, .Invalid_Type)
}

read_u32 :: proc(p: ^Parser) -> (out: u32, ok: bool) {
	n := read_u64(p) or_return
	if n > u64(max(u32)) {
		return 0, value.fail(p, .Invalid_Value)
	}
	return u32(n), true
}

read_i32 :: proc(p: ^Parser) -> (out: i32, ok: bool) {
	v := read_number(p) or_return
	#partial switch n in v {
	case u64:
		if n <= u64(max(i32)) {
			return i32(n), true
		}
		return 0, value.fail(p, .Invalid_Value)
	case i64:
		if n >= i64(min(i32)) {
			return i32(n), true
		}
		return 0, value.fail(p, .Invalid_Value)
	}
	return 0, value.fail(p, .Invalid_Type)
}

// `deserialize_option`: true for null (taken), false otherwise.
read_null :: proc(p: ^Parser) -> (is_null: bool, ok: bool) {
	c, any := value.peek_ws(p)
	if any && c == 'n' {
		p.i += 1
		return true, value.parse_ident(p, "ull")
	}
	return false, true
}

// A `(f64, f64)` or `[f64; 2]`: an array of exactly two numbers.
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

// A unit enum: `"name"`, or `{"name": null}`; the index of its name.
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
	expect :: proc(p: ^Parser, want: u8, err: value.Error) -> bool {
		d, more := value.peek_ws(p)
		if !more {
			return value.fail(p, .Eof)
		}
		if d != want {
			return value.fail(p, err)
		}
		p.i += 1
		return true
	}
	switch c {
	case '"':
		p.i += 1
		return find(p, names)
	case '{':
		value.enter(p) or_return
		p.i += 1
		expect(p, '"', .Invalid_Type) or_return
		index = find(p, names) or_return
		expect(p, ':', .Expected_Colon) or_return
		// The unit variant's content: `()`, which is null.
		expect(p, 'n', .Invalid_Type) or_return
		value.parse_ident(p, "ull") or_return
		value.leave(p)
		expect(p, '}', .Expected_Value) or_return
		return index, true
	}
	return 0, value.fail(p, .Expected_Value)
}

// A struct's fields as a derive's visit_map or visit_seq takes them.
// `field` reads field `index` into `data`.
read_struct :: proc(
	p: ^Parser,
	names: []string,
	data: rawptr,
	field: proc(p: ^Parser, index: int, data: rawptr) -> bool,
) -> (
	f: Fields,
	ok: bool,
) {
	c, any := value.peek_ws(p)
	if !any {
		return f, value.fail(p, .Eof)
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
			if index in f.seen {
				return f, value.fail(p, .Duplicate_Field)
			}
			field(p, index, data) or_return
			f.seen += {index}
		}
		value.leave(p)
		return f, value.map_end(p)
	case '[':
		f.seq = true
		value.enter(p) or_return
		p.i += 1
		first := true
		for i in 0 ..< len(names) {
			more := value.seq_next(p, &first) or_return
			if !more {
				break
			}
			field(p, i, data) or_return
			f.seen += {i}
		}
		value.leave(p)
		return f, value.seq_end(p)
	}
	return f, value.fail(p, .Invalid_Type)
}

// Whether the fields a derive needs are there: `required` in either form,
// and in the array form `options` too (an Option without a default isn't
// none when its element is missing).
has_fields :: proc(p: ^Parser, f: Fields, required, options: bit_set[0 ..< 32]) -> bool {
	need := required + options if f.seq else required
	if need <= f.seen {
		return true
	}
	return value.fail(p, .Invalid_Length if f.seq else .Missing_Field)
}

// MARK: Writing

// `"name":` after a comma unless it is the first field.
key :: proc(b: ^strings.Builder, name: string, first := false) {
	if !first {
		strings.write_byte(b, ',')
	}
	strings.write_byte(b, '"')
	strings.write_string(b, name)
	strings.write_string(b, `":`)
}

write_str :: proc(b: ^strings.Builder, s: string) {
	value.write_string(b, s)
}

write_opt_str :: proc(b: ^strings.Builder, s: Maybe(string)) {
	if text, ok := s.?; ok {
		value.write_string(b, text)
	} else {
		strings.write_string(b, "null")
	}
}

write_bool :: proc(b: ^strings.Builder, v: bool) {
	strings.write_string(b, "true" if v else "false")
}

write_u64 :: proc(b: ^strings.Builder, v: u64) {
	value.write_u64(b, v)
}

write_i64 :: proc(b: ^strings.Builder, v: i64) {
	value.write_i64(b, v)
}

// serde_json's f64: zmij's text, null when not finite.
write_f64 :: proc(b: ^strings.Builder, v: f64) {
	value.write_f64(b, v)
}
