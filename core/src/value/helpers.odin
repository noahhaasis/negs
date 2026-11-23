// windows/src/json.rs's helpers, with the same behaviour: dotted paths
// into the engine's parameters, numbers as the macOS app writes them,
// equality with numbers as doubles, and saved values restored onto today's
// defaults.
package value

import "core:strings"

// Rust's `str::parse::<usize>`: ASCII digits after an optional '+', no
// overflow.
parse_index :: proc "contextless" (text: string) -> (int, bool) {
	s := text
	if len(s) > 0 && s[0] == '+' {
		s = s[1:]
	}
	if len(s) == 0 {
		return 0, false
	}
	n: u64 = 0
	for i in 0 ..< len(s) {
		c := s[i]
		if c < '0' || c > '9' {
			return 0, false
		}
		d := u64(c - '0')
		if n > (max(u64) - d) / 10 {
			return 0, false
		}
		n = n * 10 + d
	}
	if n > u64(max(int)) {
		return 0, false
	}
	return int(n), true
}

// `json::get`: the value at a dotted path such as
// `film_render.grain.active`, array elements by index
// (`scanner.unsharp_mask.0`); null when absent. Every piece between dots
// counts, empty ones too. Borrowed from `v`.
get :: proc(v: Value, path: string) -> Value {
	current := v
	rest := path
	for {
		dot := strings.index_byte(rest, '.')
		key := rest if dot < 0 else rest[:dot]
		#partial switch c in current {
		case Object:
			found, ok := object_get(c, key)
			current = found if ok else nil
		case Array:
			i, ok := parse_index(key)
			current = c[i] if ok && i < len(c) else nil
		case:
			current = nil
		}
		if dot < 0 {
			return current
		}
		rest = rest[dot + 1:]
	}
}

// `json::set`: sets the value at a dotted path, making objects on the way.
// An array index out of range, or a path through a scalar, changes nothing
// past the objects already made. `new` must be in the tree's allocator; it
// is taken (destroyed when nothing takes it).
set :: proc(v: ^Value, path: string, new: Value, allocator := context.allocator) {
	if !set_at(v, path, new, allocator) {
		destroy(new, allocator)
	}
}

@(private)
set_at :: proc(v: ^Value, path: string, new: Value, allocator: Allocator) -> bool {
	current := v
	rest := path
	for {
		dot := strings.index_byte(rest, '.')
		key := rest if dot < 0 else rest[:dot]
		#partial switch &c in current^ {
		case Object:
			i, found := object_search(c, key)
			if !found {
				inject_at(&c, i, Field{strings.clone(key, allocator), make_object(0, allocator)})
			}
			current = &c[i].value
		case Array:
			i, ok := parse_index(key)
			if !ok || i >= len(c) {
				return false
			}
			current = &c[i]
		case:
			return false
		}
		if dot < 0 {
			destroy(current^, allocator)
			current^ = new
			return true
		}
		rest = rest[dot + 1:]
	}
}

// `json::number`: a number as a double; 0 for anything else.
number :: proc "contextless" (v: Value) -> f64 {
	n, _ := as_f64(v)
	return n
}

// `json::boolean`: false for anything but true.
boolean :: proc "contextless" (v: Value) -> bool {
	b, _ := as_bool(v)
	return b
}

// `json::string`: "" for anything but a string. Borrowed from `v`.
string_of :: proc "contextless" (v: Value) -> string {
	s, _ := as_str(v)
	return s
}

// `json::num`: a number as the macOS app's JSONEncoder writes it: whole
// numbers below 9e15 as integers (so integer parameters accept them),
// others as doubles; null for a double that isn't finite.
num :: proc "contextless" (v: f64) -> Value {
	if abs(v) < 9.0e15 && f64(i64(v)) == v {
		return from_i64(i64(v))
	}
	return from_f64(v)
}

// `json::same`: equality with numbers compared as doubles, like the macOS
// app's `JSONValue` (1 and 1.0 are the same value).
same :: proc(a, b: Value) -> bool {
	if is_number(a) && is_number(b) {
		x, _ := as_f64(a)
		y, _ := as_f64(b)
		return x == y
	}
	#partial switch x in a {
	case Object:
		y, ok := b.(Object)
		if !ok || len(x) != len(y) {
			return false
		}
		for field in x {
			w, found := object_get(y, field.key)
			if !found || !same(field.value, w) {
				return false
			}
		}
		return true
	case Array:
		y, ok := b.(Array)
		if !ok || len(x) != len(y) {
			return false
		}
		for item, i in x {
			if !same(item, y[i]) {
				return false
			}
		}
		return true
	}
	return equal(a, b)
}

// `json::restoring`: saved values merged into the current defaults, in
// `allocator`: new engine fields keep their defaults, known fields are
// restored, removed fields are dropped. A saved null stays where
// `nullable` (the catalog's `nullable_paths`, `[*]` for every element)
// says it is a value.
restoring :: proc(defaults, saved: Value, nullable: []string, allocator := context.allocator) -> Value {
	path := strings.builder_make(0, 128)
	defer strings.builder_destroy(&path)
	return restoring_at(defaults, saved, nullable, &path, allocator)
}

@(private)
restoring_at :: proc(defaults, saved: Value, nullable: []string, path: ^strings.Builder, allocator: Allocator) -> Value {
	if saved == nil {
		here := strings.to_string(path^)
		for p in nullable {
			if p == here {
				return nil
			}
		}
	}
	#partial switch d in defaults {
	case Object:
		if s, ok := saved.(Object); ok {
			out := make(Object, len(d), len(d), allocator)
			mark := len(path.buf)
			for field, i in d {
				child := field.value
				if w, found := object_get(s, field.key); found {
					if mark > 0 {
						strings.write_byte(path, '.')
					}
					strings.write_string(path, field.key)
					child = restoring_at(field.value, w, nullable, path, allocator)
					resize(&path.buf, mark)
				} else {
					child = clone(child, allocator)
				}
				out[i] = Field{strings.clone(field.key, allocator), child}
			}
			return out
		}
	case Array:
		if s, ok := saved.(Array); ok && len(s) == len(d) {
			out := make(Array, len(d), len(d), allocator)
			mark := len(path.buf)
			strings.write_string(path, "[*]")
			for item, i in d {
				out[i] = restoring_at(item, s[i], nullable, path, allocator)
			}
			resize(&path.buf, mark)
			return out
		}
	}
	keeps_saved := defaults == nil
	if !keeps_saved {
		#partial switch _ in defaults {
		case u64, i64, f64:
			keeps_saved = is_number(saved)
		case bool:
			_, keeps_saved = saved.(bool)
		case string:
			_, keeps_saved = saved.(string)
		}
	}
	return clone(saved if keeps_saved else defaults, allocator)
}
