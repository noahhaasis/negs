// The value's C ABI (negs_core.h, "Phase 4 base: value, edit state"):
// serde_json's JSON in and out as text. Every call parses into its own
// arena and frees it before returning; results are owned negs_buf_t.
package negs_core

import "base:runtime"
import "core:strings"
import "../value"

VALUE_PRETTY :: 1 // NEGS_JSON_PRETTY
VALUE_SORTED :: 2 // NEGS_JSON_SORTED

// Laid out as negs_str_t.
Str_C :: struct {
	data: [^]u8,
	len:  uint,
}

@(private)
host_bytes :: proc "contextless" (data: [^]u8, n: uint) -> []u8 {
	if data == nil || n == 0 {
		return nil
	}
	return data[:n]
}

// Text the host passes, parsed as a Value in the temporary allocator.
@(private)
host_value :: proc(data: [^]u8, n: uint) -> (value.Value, bool) {
	v, err := value.parse(host_bytes(data, n), context.temp_allocator)
	return v, err == .None
}

@(private)
value_buf :: proc(v: value.Value, pretty := false) -> Buf {
	b := strings.builder_make(0, 256, context.temp_allocator)
	if pretty {
		value.write_pretty(&b, v)
	} else {
		value.write_compact(&b, v)
	}
	return buf_from(b.buf[:])
}

@(private)
store_buf :: proc(out: ^Buf, b: Buf) {
	if out != nil {
		out^ = b
	} else {
		buf_free_in_context(b)
	}
}

// Into the caller's buffer, as 3a's words: returns the whole length.
@(private)
into :: proc "contextless" (text: []u8, out: [^]u8, cap: uint) -> uint {
	if out != nil {
		n := min(uint(len(text)), cap)
		for i in 0 ..< n {
			out[i] = text[i]
		}
	}
	return uint(len(text))
}

@(export, link_name = "negs_value_rewrite")
value_rewrite :: proc "c" (text: [^]u8, n: uint, flags: u32, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	v, ok := host_value(text, n)
	if !ok {
		return 0
	}
	store_buf(out, value_buf(v, flags & VALUE_PRETTY != 0))
	return 1
}

@(export, link_name = "negs_value_write_f64")
value_write_f64 :: proc "c" (v: f64, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [64]u8
	b := strings.builder_from_bytes(buf[:])
	value.write_f64(&b, v)
	return into(b.buf[:], out, cap)
}

@(export, link_name = "negs_value_display_f64")
value_display_f64 :: proc "c" (v: f64, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [400]u8
	b := strings.builder_from_bytes(buf[:])
	value.write_display(&b, v)
	return into(b.buf[:], out, cap)
}

@(export, link_name = "negs_json_get")
json_get :: proc "c" (v: [^]u8, n: uint, path: [^]u8, path_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	tree, ok := host_value(v, n)
	if !ok {
		return 0
	}
	store_buf(out, value_buf(value.get(tree, host_string(path, path_len))))
	return 1
}

@(export, link_name = "negs_json_set")
json_set :: proc "c" (v: [^]u8, n: uint, path: [^]u8, path_len: uint, new: [^]u8, new_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	tree, ok := host_value(v, n)
	item, item_ok := host_value(new, new_len)
	if !ok || !item_ok {
		return 0
	}
	value.set(&tree, host_string(path, path_len), item, context.temp_allocator)
	store_buf(out, value_buf(tree))
	return 1
}

@(export, link_name = "negs_json_number")
json_number :: proc "c" (v: [^]u8, n: uint) -> f64 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(v, n)
	return value.number(tree)
}

@(export, link_name = "negs_json_boolean")
json_boolean :: proc "c" (v: [^]u8, n: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(v, n)
	return u8(value.boolean(tree))
}

@(export, link_name = "negs_json_string")
json_string :: proc "c" (v: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	tree, ok := host_value(v, n)
	if !ok {
		return 0
	}
	store_buf(out, buf_from(transmute([]u8)value.string_of(tree)))
	return 1
}

@(export, link_name = "negs_json_num")
json_num :: proc "c" (v: f64, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [64]u8
	b := strings.builder_from_bytes(buf[:])
	value.write_scalar(&b, value.num(v))
	return into(b.buf[:], out, cap)
}

// 1 or 0; 2 when either isn't JSON.
@(export, link_name = "negs_json_same")
json_same :: proc "c" (a: [^]u8, a_len: uint, b: [^]u8, b_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	x, ok_x := host_value(a, a_len)
	y, ok_y := host_value(b, b_len)
	if !ok_x || !ok_y {
		return 2
	}
	return u8(value.same(x, y))
}

@(private)
host_strings :: proc(list: [^]Str_C, count: uint) -> []string {
	out := make([]string, int(count), context.temp_allocator)
	for i in 0 ..< int(count) {
		out[i] = host_string(list[i].data, list[i].len)
	}
	return out
}

@(export, link_name = "negs_json_restoring")
json_restoring :: proc "c" (defaults: [^]u8, d_len: uint, saved: [^]u8, s_len: uint, nullable: [^]Str_C, count: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	d, ok_d := host_value(defaults, d_len)
	s, ok_s := host_value(saved, s_len)
	if !ok_d || !ok_s {
		return 0
	}
	r := value.restoring(d, s, host_strings(nullable, count), context.temp_allocator)
	store_buf(out, value_buf(r))
	return 1
}
