// JSON as the macOS app's `JSONValue` holds it (`Parameters.swift`):
// numbers are doubles, objects keep their keys sorted (bytewise), which is
// the order every file is written in. Reading follows Foundation's
// `JSONDecoder` (json_read.odin) and writing its `JSONEncoder`
// (json_write.odin), byte for byte. crates/negs-library/src/json.rs is the
// oracle.
//
// A tree lives in one allocator: build it there, json_clone it into
// another, json_destroy it with the one it was made in (or let an arena go).
package library

import "core:slice"
import "core:strings"

// nil is JSON's null.
Json :: union {
	bool,
	f64,
	string,
	Json_Array,
	Json_Object,
}

Json_Array :: distinct [dynamic]Json
// Sorted by key, keys unique.
Json_Object :: distinct [dynamic]Json_Field

Json_Field :: struct {
	key:   string,
	value: Json,
}

json_object :: proc(allocator := context.allocator) -> Json {
	return make(Json_Object, 0, 0, allocator)
}

json_array :: proc(allocator := context.allocator) -> Json {
	return make(Json_Array, 0, 0, allocator)
}

// The text as a JSON string, copied into `allocator`.
json_string :: proc(text: string, allocator := context.allocator) -> Json {
	return strings.clone(text, allocator)
}

json_number :: proc(v: f64) -> Json {
	return v
}

as_object :: proc(v: Json) -> (Json_Object, bool) {
	o, ok := v.(Json_Object)
	return o, ok
}

as_array :: proc(v: Json) -> (Json_Array, bool) {
	a, ok := v.(Json_Array)
	return a, ok
}

as_str :: proc(v: Json) -> (string, bool) {
	s, ok := v.(string)
	return s, ok
}

as_f64 :: proc(v: Json) -> (f64, bool) {
	n, ok := v.(f64)
	return n, ok
}

as_bool :: proc(v: Json) -> (bool, bool) {
	b, ok := v.(bool)
	return b, ok
}

is_null :: proc(v: Json) -> bool {
	return v == nil
}

// A whole number below 10^15, as `JSONValue.integer`.
as_integer :: proc(v: Json) -> (i64, bool) {
	n, ok := v.(f64)
	if !ok || round_half_away(n) != n || abs(n) >= 1e15 {
		return 0, false
	}
	return i64(n), true
}

// Rust's f64::round: halves away from zero.
round_half_away :: proc(v: f64) -> f64 {
	if v != v || v == 0 || abs(v) >= 4503599627370496.0 {
		return v
	}
	t := f64(i64(v)) // truncated toward zero
	d := v - t
	if d >= 0.5 {
		return t + 1
	}
	if d <= -0.5 {
		return t - 1
	}
	if t == 0 && v < 0 {
		return -0.0
	}
	return t
}

// Where `key` is, or where it would go.
object_search :: proc(o: Json_Object, key: string) -> (index: int, found: bool) {
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

object_find :: proc(o: Json_Object, key: string) -> (Json, bool) {
	i, found := object_search(o, key)
	if !found {
		return nil, false
	}
	return o[i].value, true
}

object_has :: proc(o: Json_Object, key: string) -> bool {
	_, found := object_search(o, key)
	return found
}

// Sets `key` to `value`, both already in the object's allocator. A value it
// replaces is destroyed; a key that is there already keeps its string and
// `key` is freed.
object_put :: proc(o: ^Json_Object, key: string, value: Json) {
	allocator := o.allocator
	i, found := object_search(o^, key)
	if found {
		json_destroy(o[i].value, allocator)
		o[i].value = value
		delete(key, allocator)
		return
	}
	inject_at(o, i, Json_Field{key, value})
}

// object_put with the key copied.
object_set :: proc(o: ^Json_Object, key: string, value: Json) {
	object_put(o, strings.clone(key, o.allocator), value)
}

object_remove :: proc(o: ^Json_Object, key: string) {
	i, found := object_search(o^, key)
	if !found {
		return
	}
	delete(o[i].key, o.allocator)
	json_destroy(o[i].value, o.allocator)
	ordered_remove(o, i)
}

// The value under `key` of an object; null when there is none.
json_get :: proc(v: Json, key: string) -> Json {
	o, ok := v.(Json_Object)
	if !ok {
		return nil
	}
	found, _ := object_find(o, key)
	return found
}

// The value at a dotted path (`"prep.highlights"`, array indices as
// numbers), null when there is none: `JSONValue`'s subscript.
json_at :: proc(v: Json, path: string) -> Json {
	value := v
	rest := path
	for key in strings.split_iterator(&rest, ".") {
		if key == "" {
			continue
		}
		#partial switch c in value {
		case Json_Object:
			found, ok := object_find(c, key)
			if !ok {
				return nil
			}
			value = found
		case Json_Array:
			index, ok := parse_usize(key)
			if !ok || index >= len(c) {
				return nil
			}
			value = c[index]
		case:
			return nil
		}
	}
	return value
}

// Rust's `str::parse::<usize>`: ASCII digits, an optional leading '+'.
parse_usize :: proc(text: string) -> (int, bool) {
	s := text
	if len(s) > 0 && s[0] == '+' {
		s = s[1:]
	}
	if len(s) == 0 {
		return 0, false
	}
	n: u64 = 0
	for c in transmute([]u8)s {
		if c < '0' || c > '9' {
			return 0, false
		}
		next := n * 10 + u64(c - '0')
		if next / 10 != n {
			return 0, false
		}
		n = next
	}
	if n > u64(max(int)) {
		return 0, false
	}
	return int(n), true
}

// Sets the value at a dotted path, making objects on the way as
// `JSONValue`'s subscript does. `new` must be in the tree's allocator; it
// is taken (destroyed when nothing takes it).
json_set_at :: proc(v: ^Json, path: string, new: Json, allocator := context.allocator) {
	keys := make([dynamic]string, 0, 8, context.temp_allocator)
	rest := path
	for key in strings.split_iterator(&rest, ".") {
		if key != "" {
			append(&keys, key)
		}
	}
	if !set_keys(v, keys[:], new, allocator) {
		json_destroy(new, allocator)
	}
}

@(private)
set_keys :: proc(v: ^Json, keys: []string, new: Json, allocator: Allocator) -> bool {
	if len(keys) == 0 {
		json_destroy(v^, allocator)
		v^ = new
		return true
	}
	key := keys[0]
	#partial switch &c in v^ {
	case Json_Object:
		i, found := object_search(c, key)
		if !found {
			inject_at(&c, i, Json_Field{strings.clone(key, allocator), json_object(allocator)})
		}
		return set_keys(&c[i].value, keys[1:], new, allocator)
	case Json_Array:
		index, ok := parse_usize(key)
		if ok && index < len(c) {
			return set_keys(&c[index], keys[1:], new, allocator)
		}
	}
	return false
}

// A deep copy in `allocator`.
json_clone :: proc(v: Json, allocator := context.allocator) -> Json {
	switch c in v {
	case bool:
		return c
	case f64:
		return c
	case string:
		return strings.clone(c, allocator)
	case Json_Array:
		out := make(Json_Array, len(c), len(c), allocator)
		for item, i in c {
			out[i] = json_clone(item, allocator)
		}
		return out
	case Json_Object:
		out := make(Json_Object, len(c), len(c), allocator)
		for field, i in c {
			out[i] = Json_Field{strings.clone(field.key, allocator), json_clone(field.value, allocator)}
		}
		return out
	}
	return nil
}

// Frees a tree made in `allocator`. A container made in another allocator
// (a temporary tree's view of a kept one) is left alone, with all in it.
json_destroy :: proc(v: Json, allocator := context.allocator) {
	same :: proc(a, b: Allocator) -> bool {
		return a.procedure == b.procedure && a.data == b.data
	}
	switch c in v {
	case bool, f64:
	case string:
		delete(c, allocator)
	case Json_Array:
		if !same(c.allocator, allocator) {
			return
		}
		for item in c {
			json_destroy(item, allocator)
		}
		delete(c)
	case Json_Object:
		if !same(c.allocator, allocator) {
			return
		}
		for field in c {
			delete(field.key, allocator)
			json_destroy(field.value, allocator)
		}
		delete(c)
	}
}

// Rust's derived PartialEq: numbers compare as doubles.
json_equal :: proc(a, b: Json) -> bool {
	switch x in a {
	case nil:
		return b == nil
	case bool:
		y, ok := b.(bool)
		return ok && x == y
	case f64:
		y, ok := b.(f64)
		return ok && x == y
	case string:
		y, ok := b.(string)
		return ok && x == y
	case Json_Array:
		y, ok := b.(Json_Array)
		if !ok || len(x) != len(y) {
			return false
		}
		for item, i in x {
			if !json_equal(item, y[i]) {
				return false
			}
		}
		return true
	case Json_Object:
		y, ok := b.(Json_Object)
		if !ok || len(x) != len(y) {
			return false
		}
		for field, i in x {
			if field.key != y[i].key || !json_equal(field.value, y[i].value) {
				return false
			}
		}
		return true
	}
	return false
}

// `self` with `changes` laid over it: objects merge key by key, anything
// else is replaced whole (`JSONValue.overlaid`). A new tree in `allocator`.
json_overlaid :: proc(base, changes: Json, allocator := context.allocator) -> Json {
	_, base_is_object := base.(Json_Object)
	c, changes_are_object := changes.(Json_Object)
	if !base_is_object || !changes_are_object {
		return json_clone(changes, allocator)
	}
	out := json_clone(base, allocator).(Json_Object)
	for field in c {
		i, found := object_search(out, field.key)
		if found {
			merged := json_overlaid(out[i].value, field.value, allocator)
			json_destroy(out[i].value, allocator)
			out[i].value = merged
		} else {
			inject_at(&out, i, Json_Field{strings.clone(field.key, allocator), json_clone(field.value, allocator)})
		}
	}
	return out
}

// What `value` changes from `base`, compared exactly: objects recurse, any
// other value is kept whole when it differs. false when nothing does
// (`JSONValue.changes(from:)`).
json_changes_from :: proc(value, base: Json, allocator := context.allocator) -> (Json, bool) {
	values, a := value.(Json_Object)
	defaults, b := base.(Json_Object)
	if a && b {
		changed := make(Json_Object, 0, 0, allocator)
		for field in values {
			fallback, found := object_find(defaults, field.key)
			if found {
				if change, ok := json_changes_from(field.value, fallback, allocator); ok {
					append(&changed, Json_Field{strings.clone(field.key, allocator), change})
				}
			} else {
				append(&changed, Json_Field{strings.clone(field.key, allocator), json_clone(field.value, allocator)})
			}
		}
		if len(changed) == 0 {
			delete(changed)
			return nil, false
		}
		return changed, true
	}
	if json_equal(value, base) {
		return nil, false
	}
	return json_clone(value, allocator), true
}

// The parts of `value`, a stored change, under keys none of `known` has at
// that place: what a newer version stored, to be written back unchanged
// (`JSONValue.unknown(to:)`).
json_unknown_to :: proc(value: Json, known: []Json, allocator := context.allocator) -> (Json, bool) {
	values, ok := value.(Json_Object)
	if !ok {
		return nil, false
	}
	kept := make(Json_Object, 0, 0, allocator)
	children := make([dynamic]Json, 0, len(known), context.temp_allocator)
	for field in values {
		clear(&children)
		for node in known {
			if o, is_object := node.(Json_Object); is_object {
				if child, found := object_find(o, field.key); found {
					append(&children, child)
				}
			}
		}
		if len(children) == 0 {
			append(&kept, Json_Field{strings.clone(field.key, allocator), json_clone(field.value, allocator)})
		} else if deeper, has := json_unknown_to(field.value, children[:], allocator); has {
			append(&kept, Json_Field{strings.clone(field.key, allocator), deeper})
		}
	}
	if len(kept) == 0 {
		delete(kept)
		return nil, false
	}
	return kept, true
}

// Keeps the engine's current defaults (`defaults`) and restores the saved
// values it knows, dropping the ones it doesn't (`JSONValue.restoring`). A
// saved null stays where `nullable` (the catalog's `nullable_paths`, `[*]`
// for array elements) says it's a value.
json_restoring :: proc(defaults, saved: Json, nullable: []string, allocator := context.allocator) -> Json {
	return restoring_at(defaults, saved, nullable, "", allocator)
}

@(private)
restoring_at :: proc(self, saved: Json, nullable: []string, path: string, allocator: Allocator) -> Json {
	if saved == nil && slice.contains(nullable, path) {
		return nil
	}
	#partial switch d in self {
	case Json_Object:
		if values, ok := saved.(Json_Object); ok {
			result := json_clone(d, allocator).(Json_Object)
			for field in values {
				fallback, found := object_find(d, field.key)
				if !found {
					continue
				}
				child := field.key if path == "" else strings.concatenate({path, ".", field.key}, context.temp_allocator)
				restored := restoring_at(fallback, field.value, nullable, child, allocator)
				i, _ := object_search(result, field.key)
				json_destroy(result[i].value, allocator)
				result[i].value = restored
			}
			return result
		}
	case Json_Array:
		if values, ok := saved.(Json_Array); ok && len(d) == len(values) {
			child := strings.concatenate({path, "[*]"}, context.temp_allocator)
			out := make(Json_Array, len(d), len(d), allocator)
			for item, i in d {
				out[i] = restoring_at(item, values[i], nullable, child, allocator)
			}
			return out
		}
	}
	same_kind := false
	switch _ in self {
	case nil:
		same_kind = true
	case f64:
		_, same_kind = saved.(f64)
	case bool:
		_, same_kind = saved.(bool)
	case string:
		_, same_kind = saved.(string)
	case Json_Array, Json_Object:
	}
	if same_kind {
		return json_clone(saved, allocator)
	}
	return json_clone(self, allocator)
}

// Builds an object from pairs whose values are already in `allocator`.
json_from_pairs :: proc(pairs: []Json_Field, allocator := context.allocator) -> Json {
	out := make(Json_Object, 0, len(pairs), allocator)
	for p in pairs {
		object_put(&out, strings.clone(p.key, allocator), p.value)
	}
	return out
}
