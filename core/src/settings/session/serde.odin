// The serde_json pieces session.rs's derives use: typed scalars as
// `deserialize_u32`, `deserialize_i32`, `deserialize_f64`, `deserialize_bool`
// and `deserialize_string` (a PathBuf too) take them, options (`null` is
// none), lists and fixed arrays. Only accepting or refusing has to match
// serde_json; the reasons are for people.
package session

import "../../value"

@(private)
Parser :: value.Parser

@(private)
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

@(private)
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

@(private)
read_f64 :: proc(p: ^Parser) -> (n: f64, ok: bool) {
	v := read_number(p) or_return
	n, _ = value.as_f64(v)
	return n, true
}

// An integer in [lo, hi]; a double is the wrong type.
@(private)
read_int :: proc(p: ^Parser, lo, hi: i64) -> (out: i64, ok: bool) {
	v := read_number(p) or_return
	#partial switch n in v {
	case u64:
		if n <= u64(hi) {
			return i64(n), true
		}
		return 0, value.fail(p, .Invalid_Value)
	case i64:
		if n >= lo {
			return n, true
		}
		return 0, value.fail(p, .Invalid_Value)
	}
	return 0, value.fail(p, .Invalid_Type)
}

@(private)
read_u32 :: proc(p: ^Parser) -> (out: u32, ok: bool) {
	n := read_int(p, 0, i64(max(u32))) or_return
	return u32(n), true
}

// `deserialize_option`: true for null (taken), false otherwise.
@(private)
read_null :: proc(p: ^Parser) -> (is_null: bool, ok: bool) {
	c, any := value.peek_ws(p)
	if any && c == 'n' {
		p.i += 1
		return true, value.parse_ident(p, "ull")
	}
	return false, true
}

// `Vec<String>` or `Vec<PathBuf>`.
@(private)
read_strings :: proc(p: ^Parser) -> (list: []string, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return nil, value.fail(p, .Eof)
	}
	if c != '[' {
		return nil, value.fail(p, .Invalid_Type)
	}
	value.enter(p) or_return
	p.i += 1
	out := make([dynamic]string, 0, 8, p.allocator)
	first := true
	for {
		more := value.seq_next(p, &first) or_return
		if !more {
			break
		}
		s := read_string(p) or_return
		append(&out, s)
	}
	value.leave(p)
	value.seq_end(p) or_return
	return out[:], true
}

// `[i32; N]`: exactly `n` integers.
@(private)
read_ints :: proc(p: ^Parser, n: int) -> (out: [5]i32, ok: bool) {
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
	for k in 0 ..< n {
		more := value.seq_next(p, &first) or_return
		if !more {
			return out, value.fail(p, .Invalid_Length)
		}
		out[k] = i32(read_int(p, i64(min(i32)), i64(max(i32))) or_return)
	}
	value.leave(p)
	return out, value.seq_end(p)
}

// A struct's fields as a derive's visit_map or visit_seq takes them: an
// object (by name; unknown ones skipped as IgnoredAny skips them; a known
// one twice is an error) or an array (in declaration order). `field` reads
// field `index` into `data`; `seen` says which were there.
@(private)
read_struct :: proc(
	p: ^Parser,
	names: []string,
	data: rawptr,
	field: proc(p: ^Parser, index: int, data: rawptr) -> bool,
) -> (
	seen: bit_set[0 ..< 64],
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
			key := value.map_key(p) or_return
			index := -1
			for n, i in names {
				if n == key {
					index = i
					break
				}
			}
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
