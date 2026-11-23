// JSON as serde_json's `Value` holds it (features `float_roundtrip`, no
// `preserve_order`), for the Windows app's state, presets, sessions and
// caches (windows/src/json.rs and its users). Numbers keep serde's three
// kinds: a non-negative integer (u64), a negative one (i64, always below
// zero) and a double (f64, always finite); objects are sorted by key bytes
// like a BTreeMap<String, Value>, each key once. Reading follows
// `serde_json::from_slice` (parse.odin), writing `to_string` and
// `to_string_pretty` (write.odin), byte for byte; numbers are written as
// zmij and itoa write them (number.odin), with Rust's `{}` for f64.
// core/diff/tests/value.rs holds all of it to serde_json.
//
// The library's JSON (core/src/library/json*.odin) is the macOS app's
// JSONValue, doubles only; value/bridge converts between the two.
//
// A tree lives in one allocator: build it there, clone it into another,
// destroy it with the one it was made in (or let an arena go).
package value

import "base:runtime"
import "core:strings"

Allocator :: runtime.Allocator

// nil is JSON's null.
Value :: union {
	bool,
	u64,
	i64,
	f64,
	string,
	Array,
	Object,
}

Array :: distinct [dynamic]Value
// Sorted by key bytes, each key once.
Object :: distinct [dynamic]Field

Field :: struct {
	key:   string,
	value: Value,
}

// serde_json's `Value::from(i64)`: a non-negative one is a u64.
from_i64 :: proc "contextless" (v: i64) -> Value {
	if v < 0 {
		return v
	}
	return u64(v)
}

// `Value::from(f64)`: null unless finite.
from_f64 :: proc "contextless" (v: f64) -> Value {
	if v - v != 0 {
		return nil
	}
	return v
}

make_object :: proc(capacity := 0, allocator := context.allocator) -> Object {
	return make(Object, 0, capacity, allocator)
}

make_array :: proc(capacity := 0, allocator := context.allocator) -> Array {
	return make(Array, 0, capacity, allocator)
}

// A string copied into `allocator`.
make_string :: proc(text: string, allocator := context.allocator) -> Value {
	return strings.clone(text, allocator)
}

is_null :: proc "contextless" (v: Value) -> bool {
	return v == nil
}

is_number :: proc "contextless" (v: Value) -> bool {
	#partial switch _ in v {
	case u64, i64, f64:
		return true
	}
	return false
}

// `Value::as_f64`: any number as a double (a u64 or i64 rounded).
as_f64 :: proc "contextless" (v: Value) -> (f64, bool) {
	#partial switch n in v {
	case u64:
		return f64(n), true
	case i64:
		return f64(n), true
	case f64:
		return n, true
	}
	return 0, false
}

// `Value::as_u64`: a u64 only.
as_u64 :: proc "contextless" (v: Value) -> (u64, bool) {
	n, ok := v.(u64)
	return n, ok
}

// `Value::as_i64`: a u64 up to i64::MAX, or an i64.
as_i64 :: proc "contextless" (v: Value) -> (i64, bool) {
	#partial switch n in v {
	case u64:
		if n <= u64(max(i64)) {
			return i64(n), true
		}
	case i64:
		return n, true
	}
	return 0, false
}

as_bool :: proc "contextless" (v: Value) -> (bool, bool) {
	b, ok := v.(bool)
	return b, ok
}

as_str :: proc "contextless" (v: Value) -> (string, bool) {
	s, ok := v.(string)
	return s, ok
}

// Where `key` is in `o`, or where it would go.
object_search :: proc "contextless" (o: Object, key: string) -> (index: int, found: bool) {
	lo, hi := 0, len(o)
	for lo < hi {
		mid := (lo + hi) / 2
		if o[mid].key < key {
			lo = mid + 1
		} else {
			hi = mid
		}
	}
	return lo, lo < len(o) && o[lo].key == key
}

// The value under `key`, if there is one.
object_get :: proc "contextless" (o: Object, key: string) -> (Value, bool) {
	i, found := object_search(o, key)
	if !found {
		return nil, false
	}
	return o[i].value, true
}

// A pointer to the value under `key`, if there is one.
object_get_ptr :: proc "contextless" (o: ^Object, key: string) -> ^Value {
	i, found := object_search(o^, key)
	if !found {
		return nil
	}
	return &o[i].value
}

// `BTreeMap::insert`: sets `key` to `v`, both already in the object's
// allocator. A value it replaces is destroyed; a key already there keeps
// its own string and `key` is freed.
object_insert :: proc(o: ^Object, key: string, v: Value) {
	allocator := o.allocator
	i, found := object_search(o^, key)
	if found {
		destroy(o[i].value, allocator)
		o[i].value = v
		delete(key, allocator)
		return
	}
	inject_at(o, i, Field{key, v})
}

// object_insert with the key copied.
object_set :: proc(o: ^Object, key: string, v: Value) {
	object_insert(o, strings.clone(key, o.allocator), v)
}

// Appends a field whose key sorts after every key there (a writer's fast
// path); falls back to object_insert otherwise.
object_push :: proc(o: ^Object, key: string, v: Value) {
	if len(o) == 0 || o[len(o) - 1].key < key {
		append(o, Field{key, v})
		return
	}
	object_insert(o, key, v)
}

object_remove :: proc(o: ^Object, key: string) {
	i, found := object_search(o^, key)
	if !found {
		return
	}
	delete(o[i].key, o.allocator)
	destroy(o[i].value, o.allocator)
	ordered_remove(o, i)
}

// A deep copy in `allocator`.
clone :: proc(v: Value, allocator := context.allocator) -> Value {
	switch c in v {
	case bool:
		return c
	case u64:
		return c
	case i64:
		return c
	case f64:
		return c
	case string:
		return strings.clone(c, allocator)
	case Array:
		out := make(Array, len(c), len(c), allocator)
		for item, i in c {
			out[i] = clone(item, allocator)
		}
		return out
	case Object:
		out := make(Object, len(c), len(c), allocator)
		for field, i in c {
			out[i] = Field{strings.clone(field.key, allocator), clone(field.value, allocator)}
		}
		return out
	}
	return nil
}

// Frees a tree made in `allocator`.
destroy :: proc(v: Value, allocator := context.allocator) {
	switch c in v {
	case bool, u64, i64, f64:
	case string:
		delete(c, allocator)
	case Array:
		for item in c {
			destroy(item, allocator)
		}
		delete(c)
	case Object:
		for field in c {
			delete(field.key, allocator)
			destroy(field.value, allocator)
		}
		delete(c)
	}
}

// serde_json's `PartialEq` for Value: numbers equal only within a kind
// (1 is not 1.0), doubles with `==` (so 0.0 == -0.0).
equal :: proc(a, b: Value) -> bool {
	switch x in a {
	case nil:
		return b == nil
	case bool:
		y, ok := b.(bool)
		return ok && x == y
	case u64:
		y, ok := b.(u64)
		return ok && x == y
	case i64:
		y, ok := b.(i64)
		return ok && x == y
	case f64:
		y, ok := b.(f64)
		return ok && x == y
	case string:
		y, ok := b.(string)
		return ok && x == y
	case Array:
		y, ok := b.(Array)
		if !ok || len(x) != len(y) {
			return false
		}
		for item, i in x {
			if !equal(item, y[i]) {
				return false
			}
		}
		return true
	case Object:
		y, ok := b.(Object)
		if !ok || len(x) != len(y) {
			return false
		}
		for field, i in x {
			if field.key != y[i].key || !equal(field.value, y[i].value) {
				return false
			}
		}
		return true
	}
	return false
}
