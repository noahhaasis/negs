// Phase 4b's session ABI (negs_core.h, "Phase 4b: presets, session"):
// session.rs's folders, write_atomic, EditorSession, SessionStore,
// Preferences and CatalogCache. Sessions and preferences cross as the
// JSON serde_json writes for them. Every call works in its own arena (its
// allocator too, inside the blocks marked so); results are owned
// negs_buf_t.
package negs_core

import "base:runtime"
import "core:strings"
import fs "../platform/fs"
import "../settings/session"
import "../value"

@(export, link_name = "negs_settings_path")
settings_path :: proc "c" (which: u32, test: u8, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if which > u32(max(session.Place)) {
		return 0
	}
	path: string
	{
		context.allocator = context.temp_allocator
		path = session.place(session.Place(which), test != 0)
	}
	store_text(out, path)
	return 1
}

@(export, link_name = "negs_settings_write_atomic")
settings_write_atomic :: proc "c" (path: [^]u8, path_len: uint, data: [^]u8, data_len: uint, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	err, ok := fs.write_atomic(host_string(path, path_len), host_bytes(data, data_len), context.temp_allocator)
	if !ok {
		store_text(error, err)
		return 0
	}
	return 1
}

// 0 invalid, 1 valid, 2 not a session the core reads.
@(export, link_name = "negs_session_validate")
session_validate :: proc "c" (text: [^]u8, n: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	context.allocator = context.temp_allocator
	s, ok := session.parse_session(host_bytes(text, n))
	if !ok {
		return 2
	}
	return u8(session.validate(s))
}

@(export, link_name = "negs_session_migrate_state")
session_migrate_state :: proc "c" (text: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		v, err := value.parse(host_bytes(text, n))
		if err != .None {
			return 0
		}
		if o, is_object := &v.(value.Object); is_object {
			if params := value.object_get_ptr(o, "params"); params != nil {
				session.migrate_params(params)
			}
		}
		b := strings.builder_make(0, 4096)
		value.write_compact(&b, v)
		result = b.buf[:]
	}
	store_buf(out, buf_from(result))
	return 1
}

// The session as the core writes it (`serde_json::to_vec`), for the
// differential tests: 1, or 0 when it doesn't read.
@(export, link_name = "negs_session_rewrite")
session_rewrite :: proc "c" (text: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		s, ok := session.parse_session(host_bytes(text, n))
		if !ok {
			return 0
		}
		result = session.session_text(s)
	}
	store_buf(out, buf_from(result))
	return 1
}

@(export, link_name = "negs_session_load")
session_load :: proc "c" (file: [^]u8, file_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: session.Load_Result
	text: []u8
	{
		context.allocator = context.temp_allocator
		result, text = session.load(host_string(file, file_len))
	}
	if result != .None {
		store_buf(out, buf_from(text))
	}
	return u8(result)
}

@(export, link_name = "negs_session_save")
session_save :: proc "c" (file: [^]u8, file_len: uint, text: [^]u8, n: uint, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: session.Save_Result
	err: string
	{
		context.allocator = context.temp_allocator
		result, err = session.save(host_string(file, file_len), host_bytes(text, n))
	}
	if result == .Error {
		store_text(error, err)
	}
	return u8(result)
}

// Preferences as the core writes them: NEGS_JSON_PRETTY for to_vec_pretty,
// else to_vec. 1, or 0 when they don't read.
@(export, link_name = "negs_preferences_rewrite")
preferences_rewrite :: proc "c" (text: [^]u8, n: uint, flags: u32, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		prefs, ok := session.parse_preferences(host_bytes(text, n))
		if !ok {
			return 0
		}
		result = session.preferences_text(prefs, flags & VALUE_PRETTY != 0)
	}
	store_buf(out, buf_from(result))
	return 1
}

@(export, link_name = "negs_preferences_default")
preferences_default :: proc "c" (flags: u32) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		result = session.preferences_text(session.default_preferences(), flags & VALUE_PRETTY != 0)
	}
	return buf_from(result)
}

@(export, link_name = "negs_preferences_load")
preferences_load :: proc "c" (file: [^]u8, file_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		prefs, ok := session.load_preferences(host_string(file, file_len))
		if !ok {
			return 0
		}
		result = session.preferences_text(prefs, false)
	}
	store_buf(out, buf_from(result))
	return 1
}

@(export, link_name = "negs_preferences_save")
preferences_save :: proc "c" (file: [^]u8, file_len: uint, text: [^]u8, n: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	context.allocator = context.temp_allocator
	return u8(session.save_preferences(host_string(file, file_len), host_bytes(text, n)))
}

@(export, link_name = "negs_preferences_list_set")
preferences_list_set :: proc "c" (list: [^]u8, list_len: uint, item: [^]u8, item_len: uint, on: u8, sorted: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: string
	{
		context.allocator = context.temp_allocator
		result = session.list_set(host_string(list, list_len), host_string(item, item_len), on != 0, sorted != 0)
	}
	return buf_from(transmute([]u8)result)
}

@(export, link_name = "negs_preferences_list_has")
preferences_list_has :: proc "c" (list: [^]u8, list_len: uint, item: [^]u8, item_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return u8(session.list_has(host_string(list, list_len), host_string(item, item_len)))
}

@(export, link_name = "negs_preferences_tab")
preferences_tab :: proc "c" (tab: [^]u8, tab_len: uint, chosen: u8) -> u8 {
	context = core_context()
	return u8(session.tab(host_string(tab, tab_len), chosen != 0))
}

@(export, link_name = "negs_catalog_load")
catalog_load :: proc "c" (file: [^]u8, file_len: uint, helper: [^]u8, helper_len: uint, data: [^]u8, data_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	result: []u8
	{
		context.allocator = context.temp_allocator
		catalog, ok := session.catalog_load(host_string(file, file_len), host_string(helper, helper_len), host_string(data, data_len))
		if !ok {
			return 0
		}
		b := strings.builder_make(0, 4096)
		value.write_compact(&b, catalog)
		result = b.buf[:]
	}
	store_buf(out, buf_from(result))
	return 1
}

// The catalog crosses as JSON; nothing is saved when it doesn't parse.
@(export, link_name = "negs_catalog_save")
catalog_save :: proc "c" (file: [^]u8, file_len: uint, catalog: [^]u8, catalog_len: uint, helper: [^]u8, helper_len: uint, data: [^]u8, data_len: uint) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	context.allocator = context.temp_allocator
	v, err := value.parse(host_bytes(catalog, catalog_len))
	if err != .None {
		return
	}
	session.catalog_save(host_string(file, file_len), v, host_string(helper, helper_len), host_string(data, data_len))
}
