// Phase 4c's C ABI, continued: the match cache (core/src/caches/
// match_cache) and the rendered preview (core/src/caches/preview).
package negs_core

import "base:runtime"
import mc "../caches/match_cache"
import "../caches/preview"
import "../state"

// MARK: Match cache

@(export, link_name = "negs_match_key")
match_key :: proc "c" (
	file: [^]u8,
	file_len: uint,
	film: [^]u8,
	film_len: uint,
	paper: [^]u8,
	paper_len: uint,
	params: [^]u8,
	params_len: uint,
	engine: [^]u8,
	engine_len: uint,
	out: ^Buf,
) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	p, ok := host_value(params, params_len)
	if !ok {
		return 0
	}
	k, has := mc.key(
		host_string(file, file_len),
		host_string(film, film_len),
		host_string(paper, paper_len),
		p,
		host_string(engine, engine_len),
		context.temp_allocator,
	)
	if !has {
		return 0
	}
	store_buf(out, buf_from(transmute([]u8)k))
	return 1
}

@(export, link_name = "negs_match_engine_stamp")
match_engine_stamp :: proc "c" (version: [^]u8, version_len: uint, helper: [^]u8, helper_len: uint, data: [^]u8, data_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	text := mc.engine_stamp(host_string(version, version_len), host_string(helper, helper_len), host_string(data, data_len), context.temp_allocator)
	return buf_from(transmute([]u8)text)
}

@(export, link_name = "negs_match_cache_limit")
match_cache_limit :: proc "c" () -> u64 {
	return mc.LIMIT
}

@(export, link_name = "negs_match_cache_new")
match_cache_new :: proc "c" (file: [^]u8, n: uint, limit: u64) -> ^mc.Cache {
	context = core_context()
	c := new(mc.Cache)
	c^ = mc.cache_make(host_string(file, n), limit)
	return c
}

@(export, link_name = "negs_match_cache_free")
match_cache_free :: proc "c" (c: ^mc.Cache) {
	context = core_context()
	if c != nil {
		mc.cache_destroy(c)
		free(c)
	}
}

@(export, link_name = "negs_match_cache_solved")
match_cache_solved :: proc "c" (c: ^mc.Cache, key: [^]u8, n: uint, now: f64, ev: ^f64, limited: ^u8) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if c == nil {
		return 0
	}
	e, l, found := mc.solved(c, host_string(key, n), now)
	if !found {
		return 0
	}
	if ev != nil {
		ev^ = e
	}
	if limited != nil {
		limited^ = u8(l)
	}
	return 1
}

@(export, link_name = "negs_match_cache_store")
match_cache_store :: proc "c" (c: ^mc.Cache, key: [^]u8, n: uint, ev: f64, limited: u8, now: f64) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if c == nil {
		return 0
	}
	return u8(mc.store(c, host_string(key, n), ev, limited != 0, now))
}

@(export, link_name = "negs_match_cache_flush_bytes")
match_cache_flush_bytes :: proc "c" (c: ^mc.Cache, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	if c == nil {
		return 0
	}
	bytes, write := mc.flush_bytes(c, context.temp_allocator)
	if !write {
		return 0
	}
	store_buf(out, buf_from(bytes))
	return 1
}

@(export, link_name = "negs_match_cache_write")
match_cache_write :: proc "c" (file: [^]u8, file_len: uint, bytes: [^]u8, n: uint) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	mc.write_file(host_string(file, file_len), host_bytes(bytes, n))
}

@(export, link_name = "negs_match_cache_count")
match_cache_count :: proc "c" (c: ^mc.Cache) -> u64 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if c == nil {
		return 0
	}
	return u64(mc.count(c))
}

// MARK: Rendered preview

PREVIEW_RENDERED :: 1 // NEGS_PREVIEW_RENDERED

@(export, link_name = "negs_preview_key")
preview_key :: proc "c" (
	source: [^]u8,
	source_len: uint,
	helper: [^]u8,
	helper_len: uint,
	data: [^]u8,
	data_len: uint,
	state_json: [^]u8,
	state_len: uint,
	flags: u32,
	out: ^Buf,
) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	src, hlp, dat := host_string(source, source_len), host_string(helper, helper_len), host_string(data, data_len)
	text: string
	ok: bool
	if flags & PREVIEW_RENDERED != 0 {
		v, parsed := host_value(state_json, state_len)
		if !parsed {
			return 0
		}
		text, ok = preview.key_of_rendered(src, hlp, dat, v, context.temp_allocator)
	} else {
		s, err := state.parse(host_bytes(state_json, state_len), context.temp_allocator)
		if err != .None {
			return 0
		}
		text, ok = preview.key(src, hlp, dat, s, context.temp_allocator)
	}
	if !ok {
		return 0
	}
	store_buf(out, buf_from(transmute([]u8)text))
	return 1
}

@(export, link_name = "negs_preview_checksum")
preview_checksum :: proc "c" (bytes: [^]u8, n: uint) -> u64 {
	return preview.checksum(host_bytes(bytes, n))
}

@(export, link_name = "negs_preview_read")
preview_read :: proc "c" (
	file: [^]u8,
	file_len: uint,
	key: [^]u8,
	key_len: uint,
	frame: ^preview.Frame,
	display_space: ^Buf,
	file_bytes: ^Buf,
	pixels_offset: ^uint,
) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if display_space != nil {
		display_space^ = {}
	}
	if file_bytes != nil {
		file_bytes^ = {}
	}
	// Read into the core's heap and handed over as they are: the host
	// copies the pixels out once, as the Rust's `to_vec` did.
	loaded, err := preview.read(host_string(file, file_len), host_string(key, key_len), context.allocator)
	if err != .None {
		return u8(err)
	}
	if frame != nil {
		frame^ = loaded.frame
	}
	store_buf(display_space, Buf{raw_data(loaded.display_space), uint(len(loaded.display_space))})
	if pixels_offset != nil {
		pixels_offset^ = uint(loaded.pixels_offset)
	}
	store_buf(file_bytes, Buf{raw_data(loaded.bytes), uint(len(loaded.bytes))})
	return 0
}

@(export, link_name = "negs_preview_write")
preview_write :: proc "c" (
	file: [^]u8,
	file_len: uint,
	key: [^]u8,
	key_len: uint,
	frame: ^preview.Frame,
	display_space: [^]u8,
	display_len: uint,
	pixels: [^]u8,
	n: uint,
) -> i32 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	f := frame^ if frame != nil else preview.Frame{}
	return i32(preview.write(host_string(file, file_len), host_string(key, key_len), f, host_string(display_space, display_len), host_bytes(pixels, n)))
}
