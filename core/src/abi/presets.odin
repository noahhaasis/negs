// Phase 4b's presets ABI (negs_core.h, "Phase 4b: presets, session"):
// presets.rs's folder operations and name rules. Every call works in its
// own arena; results are owned negs_buf_t or negs_preset_list_t.
package negs_core

import "base:runtime"
import "core:mem"
import "core:strings"
import "../settings/presets"

// negs_preset_file_t: a preset as listed. `path` is borrowed from the list
// that holds it (or the caller's, going in).
Preset_File_C :: struct {
	path:        [^]u8,
	path_len:    uint,
	size:        u64,
	modified_s:  i64,
	modified_ns: u32,
}

// negs_preset_list_t: `files` and their paths in one block, freed with
// negs_preset_list_free.
Preset_List_C :: struct {
	files: [^]Preset_File_C,
	count: uint,
}

@(private)
preset_in :: proc "contextless" (f: ^Preset_File_C) -> presets.Preset_File {
	if f == nil {
		return {}
	}
	return {host_string(f.path, f.path_len), f.size, f.modified_s, f.modified_ns}
}

@(private)
preset_list :: proc(list: []presets.Preset_File) -> Preset_List_C {
	if len(list) == 0 {
		return {}
	}
	head := len(list) * size_of(Preset_File_C)
	total := head
	for f in list {
		total += len(f.path)
	}
	block, err := mem.alloc_bytes(total, align_of(Preset_File_C))
	if err != nil {
		return {}
	}
	files := ([^]Preset_File_C)(raw_data(block))
	at := head
	for f, i in list {
		copy(block[at:], f.path)
		files[i] = {raw_data(block[at:]), uint(len(f.path)), f.size, f.seconds, f.nanos}
		at += len(f.path)
	}
	return {files, uint(len(list))}
}

@(private)
store_list :: proc(out: ^Preset_List_C, list: []presets.Preset_File) {
	l := preset_list(list)
	if out != nil {
		out^ = l
	} else if l.files != nil {
		mem.free(l.files)
	}
}

@(private)
store_text :: proc(out: ^Buf, s: string) {
	store_buf(out, buf_from(transmute([]u8)s))
}

@(export, link_name = "negs_preset_list_free")
preset_list_free :: proc "c" (list: Preset_List_C) {
	context = core_context()
	if list.files != nil {
		mem.free(list.files)
	}
}

@(export, link_name = "negs_settings_text_functions")
settings_text_functions :: proc "c" (nfc, lowercase: presets.Text_Fn, user: rawptr) {
	presets.set_text_functions(nfc, lowercase, user)
}

@(export, link_name = "negs_presets_limit")
presets_limit :: proc "c" (which: u32) -> u64 {
	switch which {
	case 0:
		return presets.MAX_LENGTH
	case 1:
		return presets.MAX_FILE_SIZE
	}
	return 0
}

@(export, link_name = "negs_presets_list")
presets_list :: proc "c" (folder: [^]u8, folder_len: uint, out: ^Preset_List_C, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, err, ok := presets.list(host_string(folder, folder_len))
	if !ok {
		store_text(error, err)
		return 0
	}
	store_list(out, list)
	return 1
}

@(export, link_name = "negs_presets_add")
presets_add :: proc "c" (folder: [^]u8, folder_len: uint, data: [^]u8, data_len: uint, name: [^]u8, name_len: uint, out: ^Preset_List_C, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	f, err, ok := presets.add(host_string(folder, folder_len), host_bytes(data, data_len), host_string(name, name_len))
	if !ok {
		store_text(error, err)
		return 0
	}
	store_list(out, {f})
	return 1
}

@(export, link_name = "negs_presets_rename")
presets_rename :: proc "c" (folder: [^]u8, folder_len: uint, preset: ^Preset_File_C, name: [^]u8, name_len: uint, out: ^Preset_List_C, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	f, err, ok := presets.rename(host_string(folder, folder_len), preset_in(preset), host_string(name, name_len))
	if !ok {
		store_text(error, err)
		return 0
	}
	store_list(out, {f})
	return 1
}

@(export, link_name = "negs_presets_delete")
presets_delete :: proc "c" (folder: [^]u8, folder_len: uint, preset: ^Preset_File_C, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	err, ok := presets.delete_preset(host_string(folder, folder_len), preset_in(preset))
	if !ok {
		store_text(error, err)
		return 0
	}
	return 1
}

@(export, link_name = "negs_presets_read")
presets_read :: proc "c" (preset: ^Preset_File_C, out: ^Buf, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	data, err, ok := presets.read(preset_in(preset))
	if !ok {
		store_text(error, err)
		return 0
	}
	store_buf(out, buf_from(data))
	return 1
}

@(export, link_name = "negs_presets_read_file")
presets_read_file :: proc "c" (path: [^]u8, path_len: uint, out: ^Buf, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	data, err, ok := presets.read_file(host_string(path, path_len))
	if !ok {
		store_text(error, err)
		return 0
	}
	store_buf(out, buf_from(data))
	return 1
}

@(export, link_name = "negs_presets_set_order")
presets_set_order :: proc "c" (folder: [^]u8, folder_len: uint, list: [^]Preset_File_C, count: uint, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	files := make([]presets.Preset_File, int(count), context.temp_allocator)
	for i in 0 ..< int(count) {
		files[i] = preset_in(&list[i])
	}
	err, ok := presets.set_order(host_string(folder, folder_len), files)
	if !ok {
		store_text(error, err)
		return 0
	}
	return 1
}

@(export, link_name = "negs_presets_name_problem")
presets_name_problem :: proc "c" (name: [^]u8, name_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	problem, bad := presets.problem(host_string(name, name_len))
	if bad {
		store_text(out, problem)
	}
	return u8(bad)
}

@(export, link_name = "negs_presets_name_key")
presets_name_key :: proc "c" (name: [^]u8, name_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_from(transmute([]u8)presets.key(host_string(name, name_len)))
}

@(export, link_name = "negs_presets_name_unique")
presets_name_unique :: proc "c" (name: [^]u8, name_len: uint, taken: [^]Str_C, count: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	keys := make(map[string]bool, int(count) + 1, context.temp_allocator)
	for i in 0 ..< int(count) {
		keys[strings.clone(host_string(taken[i].data, taken[i].len), context.temp_allocator)] = true
	}
	return buf_from(transmute([]u8)presets.unique(host_string(name, name_len), keys))
}

@(export, link_name = "negs_presets_name_from_file_name")
presets_name_from_file_name :: proc "c" (name: [^]u8, name_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_from(transmute([]u8)presets.from_file_name(host_string(name, name_len)))
}

@(export, link_name = "negs_presets_text_start")
presets_text_start :: proc "c" (data: [^]u8, n: uint) -> i64 {
	return i64(presets.text_start(host_bytes(data, n)))
}
