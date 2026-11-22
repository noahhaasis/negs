// Between serde_json's Value (core/src/value) and the library's JSON
// (core/src/library: the macOS app's JSONValue, numbers as doubles). Both
// keep objects sorted by key bytes, so only numbers change:
// - to the library, every number becomes a double: a u64 or i64 past 2^53
//   rounds, and 1 and 1.0 become the same;
// - from the library, a double becomes what `json::num` makes of it: a
//   whole number below 9e15 an integer (u64, or i64 if negative; -0.0 is
//   0), others a double, and one that isn't finite null.
// So a Value survives the trip there and back when its doubles aren't
// whole, its integers are below 9e15, and it has no -0.0; a library value
// survives always (its doubles come back equal, if not with the same kind).
package value_bridge

import "core:strings"
import lib "../../library"
import "../../value"

to_library :: proc(v: value.Value, allocator := context.allocator) -> lib.Json {
	switch c in v {
	case nil:
		return nil
	case bool:
		return c
	case u64:
		return f64(c)
	case i64:
		return f64(c)
	case f64:
		return c
	case string:
		return strings.clone(c, allocator)
	case value.Array:
		out := make(lib.Json_Array, len(c), len(c), allocator)
		for item, i in c {
			out[i] = to_library(item, allocator)
		}
		return out
	case value.Object:
		out := make(lib.Json_Object, len(c), len(c), allocator)
		for field, i in c {
			out[i] = lib.Json_Field{strings.clone(field.key, allocator), to_library(field.value, allocator)}
		}
		return out
	}
	return nil
}

from_library :: proc(j: lib.Json, allocator := context.allocator) -> value.Value {
	switch c in j {
	case nil:
		return nil
	case bool:
		return c
	case f64:
		return value.num(c)
	case string:
		return strings.clone(c, allocator)
	case lib.Json_Array:
		out := make(value.Array, len(c), len(c), allocator)
		for item, i in c {
			out[i] = from_library(item, allocator)
		}
		return out
	case lib.Json_Object:
		out := make(value.Object, len(c), len(c), allocator)
		for field, i in c {
			out[i] = value.Field{strings.clone(field.key, allocator), from_library(field.value, allocator)}
		}
		return out
	}
	return nil
}
