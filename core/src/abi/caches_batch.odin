// Phase 4c's C ABI (negs_core.h, "Phase 4c: batch, match cache, rendered
// preview"): std::path's rules and the batch (core/src/caches/batch).
// Every call works in its own arena; results are owned negs_buf_t.
package negs_core

import "base:runtime"
import "core:strings"
import "../caches/batch"
import "../caches/files"
import "../state"
import "../value"

// Laid out as negs_batch_save_t.
Batch_Save_C :: batch.Save

// Laid out as negs_batch_run_t.
Batch_Run_C :: struct {
	statuses:  [^]u8,
	count:     uint,
	running:   u8,
	cancelled: u8,
}

// Laid out as negs_batch_counts_t.
Batch_Counts_C :: struct {
	total:           u64,
	remaining:       u64,
	finished:        u64,
	exported:        u64,
	failed:          u64,
	skipped:         u64,
	fraction:        f64,
	exported_part:   f64,
	other_part:      f64,
	needs_attention: u8,
}

@(private = "file")
style_of :: proc "contextless" (style: u32) -> files.Style {
	switch style {
	case 1:
		return .Unix
	case 2:
		return .Windows
	}
	return files.STYLE
}

@(private = "file")
text_buf :: proc(s: string) -> Buf {
	return buf_from(transmute([]u8)s)
}

@(export, link_name = "negs_caches_path")
caches_path :: proc "c" (op, style: u32, path: [^]u8, path_len: uint, arg: [^]u8, arg_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	p := host_string(path, path_len)
	a := host_string(arg, arg_len)
	s := style_of(style)
	result: string
	ok := true
	switch op {
	case 0:
		result, ok = files.file_name(p, s)
	case 1:
		result, ok = files.file_stem(p, s)
	case 2:
		result, ok = files.extension(p, s)
	case 3:
		result, ok = files.parent(p, s)
	case 4:
		result = files.with_extension(p, a, context.temp_allocator, s)
	case 5:
		result = files.join(p, a, context.temp_allocator, s)
	case 6:
		result = files.display(p, context.temp_allocator)
	case:
		return 0
	}
	if !ok {
		return 0
	}
	store_buf(out, text_buf(result))
	return 1
}

@(export, link_name = "negs_caches_secs_f64")
caches_secs_f64 :: proc "c" (secs: u64, nanos: u32) -> f64 {
	return files.secs_f64({secs, nanos})
}

// MARK: Kinds

@(export, link_name = "negs_batch_key")
batch_key :: proc "c" (table, index: u32) -> cstring {
	@(static, rodata) NAMING := [?]cstring{"nameFilm", "name", "custom"}
	@(static, rodata) NAMING_TITLES := [?]cstring{"Name_Film", "Name", "Custom"}
	@(static, rodata) EXISTING := [?]cstring{"number", "replace", "skip"}
	@(static, rodata) EXISTING_TITLES := [?]cstring{"Add a Number", "Replace", "Skip"}
	@(static, rodata) STATUS := [?]cstring{"waiting", "done", "failed", "skipped"}
	@(static, rodata) FORMATS := [?]cstring{"png", "jpeg", "tiff"}
	@(static, rodata) EDGES := [?]cstring{"full", "long", "short"}
	@(static, rodata) ENGINES := [?]cstring{"reference", "fast"}
	@(static, rodata) EXTENSIONS := [?]cstring{"png", "jpg", "tif"}
	tables := [?][]cstring{NAMING[:], NAMING_TITLES[:], EXISTING[:], EXISTING_TITLES[:], STATUS[:], FORMATS[:], EDGES[:], ENGINES[:], EXTENSIONS[:]}
	if int(table) >= len(tables) || int(index) >= len(tables[table]) {
		return nil
	}
	return tables[table][index]
}

@(export, link_name = "negs_batch_from_key")
batch_from_key :: proc "c" (table: u32, key: [^]u8, n: uint) -> u32 {
	context = core_context()
	k := host_string(key, n)
	switch table {
	case 0:
		return u32(batch.naming_from_key(k))
	case 2:
		return u32(batch.existing_from_key(k))
	case 5:
		return u32(batch.format_from_key(k))
	case 6:
		return u32(batch.edge_from_key(k))
	case 7:
		return u32(batch.engine_from_key(k))
	}
	return 0
}

@(export, link_name = "negs_batch_options_default")
batch_options_default :: proc "c" () -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return text_buf(batch.options_to_string(batch.DEFAULT_OPTIONS, context.temp_allocator))
}

@(export, link_name = "negs_batch_options_save")
batch_options_save :: proc "c" (format: [^]u8, n: uint, bit_depth: u32, color_space: u64, jpeg_quality: u32, out: ^Batch_Save_C) {
	context = core_context()
	if out == nil {
		return
	}
	o := batch.DEFAULT_OPTIONS
	o.format = host_string(format, n)
	o.bit_depth = bit_depth
	o.color_space = color_space
	o.jpeg_quality = jpeg_quality
	out^ = batch.options_save(o)
}

// MARK: Names

@(export, link_name = "negs_batch_file_name")
batch_file_name :: proc "c" (
	naming: u32,
	custom: [^]u8,
	custom_len: uint,
	name: [^]u8,
	name_len: uint,
	film: [^]u8,
	film_len: uint,
	format: [^]u8,
	format_len: uint,
) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	n := batch.Naming(min(naming, 2))
	f := batch.format_from_key(host_string(format, format_len))
	text := batch.file_name(n, host_string(custom, custom_len), host_string(name, name_len), host_string(film, film_len), f, context.temp_allocator)
	return text_buf(text)
}

@(export, link_name = "negs_batch_name")
batch_name :: proc "c" (photo: [^]u8, photo_len: uint, title: [^]u8, title_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return text_buf(batch.batch_name(host_string(photo, photo_len), host_string(title, title_len), context.temp_allocator))
}

@(export, link_name = "negs_batch_numbered_name")
batch_numbered_name :: proc "c" (file_name: [^]u8, n: uint, number: u64) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return text_buf(batch.numbered_name(host_string(file_name, n), number, context.temp_allocator))
}

@(export, link_name = "negs_batch_unique")
batch_unique :: proc "c" (names: [^]Str_C, count: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	unique := batch.unique(host_strings(names, count), context.temp_allocator)
	b := strings.builder_make(0, 64, context.temp_allocator)
	strings.write_byte(&b, '[')
	for u, i in unique {
		if i > 0 {
			strings.write_byte(&b, ',')
		}
		value.write_string(&b, u)
	}
	strings.write_byte(&b, ']')
	store_buf(out, buf_from(b.buf[:]))
	return 1
}

// MARK: Matching

@(export, link_name = "negs_batch_matched_exposure")
batch_matched_exposure :: proc "c" (exposure: [^]u8, n: uint, has_old: u8, old: f64, ev: f64, total: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if total != nil {
		total^ = {}
	}
	v, _ := host_value(exposure, n)
	o: Maybe(f64)
	if has_old != 0 {
		o = old
	}
	t, set := batch.matched_exposure(v, o, ev)
	if !set {
		return 0
	}
	store_buf(total, value_buf(t))
	return 1
}

@(export, link_name = "negs_batch_matched")
batch_matched :: proc "c" (state_json: [^]u8, n: uint, ev: f64, limited: u8, inputs: [^]u8, inputs_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	s, err := state.parse(host_bytes(state_json, n), context.temp_allocator)
	if err != .None {
		return 0
	}
	in_v, ok := host_value(inputs, inputs_len)
	if !ok {
		return 0
	}
	next, on := batch.matched(s, ev, limited != 0, in_v, context.temp_allocator)
	if !on {
		return 0
	}
	store_buf(out, text_buf(state.to_string(next, allocator = context.temp_allocator)))
	return 1
}

// MARK: Run words

@(private = "file")
run_of :: proc(r: ^Batch_Run_C) -> batch.Run {
	if r == nil {
		return {}
	}
	statuses := make([]batch.Status, r.count, context.temp_allocator)
	for i in 0 ..< int(r.count) {
		statuses[i] = batch.Status(min(r.statuses[i], 3))
	}
	return batch.run_of(statuses, r.running != 0, r.cancelled != 0)
}

@(export, link_name = "negs_batch_run_counts")
batch_run_counts :: proc "c" (run: ^Batch_Run_C, out: ^Batch_Counts_C) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out == nil {
		return
	}
	r := run_of(run)
	e, o := batch.rule_parts(r)
	out^ = Batch_Counts_C {
		total           = r.total,
		remaining       = r.waiting,
		finished        = batch.finished(r),
		exported        = r.done,
		failed          = r.failed,
		skipped         = r.skipped,
		fraction        = batch.fraction(r),
		exported_part   = e,
		other_part      = o,
		needs_attention = u8(batch.needs_attention(r)),
	}
}

@(export, link_name = "negs_batch_run_title")
batch_run_title :: proc "c" (run: ^Batch_Run_C) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return text_buf(batch.title(run_of(run), context.temp_allocator))
}

@(export, link_name = "negs_batch_run_outcome")
batch_run_outcome :: proc "c" (run: ^Batch_Run_C, words: ^Buf, failed: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	w, f := batch.outcome_words(run_of(run), context.temp_allocator)
	store_buf(words, text_buf(w))
	if failed != nil {
		failed^ = {}
	}
	text, has := f.?
	if has {
		store_buf(failed, text_buf(text))
	}
	return u8(has)
}

@(export, link_name = "negs_batch_run_finished")
batch_run_finished :: proc "c" (run: ^Batch_Run_C, destination: [^]u8, n: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return text_buf(batch.finished_words(run_of(run), host_string(destination, n), context.temp_allocator))
}

@(export, link_name = "negs_batch_run_progress")
batch_run_progress :: proc "c" (run: ^Batch_Run_C, lightroom: [^]u8, n: uint, has_lightroom: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	key: Maybe(string)
	if has_lightroom != 0 {
		key = host_string(lightroom, n)
	}
	return text_buf(batch.progress_words(run_of(run), key, context.temp_allocator))
}

@(export, link_name = "negs_batch_lightroom_title")
batch_lightroom_title :: proc "c" (key: [^]u8, n: uint) -> cstring {
	return "Lightroom Classic" if host_string(key, n) == "classic" else "Lightroom"
}

@(export, link_name = "negs_batch_exporting")
batch_exporting :: proc "c" (finished, total: u64, lightroom: [^]u8, n: uint, has_lightroom: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	title: Maybe(string)
	if has_lightroom != 0 {
		title = host_string(lightroom, n)
	}
	return text_buf(batch.exporting(finished, total, title, context.temp_allocator))
}

@(export, link_name = "negs_batch_message")
batch_message :: proc "c" (count: u64, engine: u32, edge: u32, pixels: u32, save: ^Batch_Save_C, preview_space: [^]u8, n: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	s := save^ if save != nil else Batch_Save_C{}
	text := batch.message(
		count,
		batch.Export_Engine(min(engine, 1)),
		batch.Edge(min(edge, 2)),
		pixels,
		s,
		host_string(preview_space, n),
		context.temp_allocator,
	)
	return text_buf(text)
}

// MARK: The journal

@(export, link_name = "negs_batch_journal_rewrite")
batch_journal_rewrite :: proc "c" (text: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	j, err := batch.journal_parse(host_bytes(text, n), context.temp_allocator)
	if err != .None {
		return 0
	}
	store_buf(out, text_buf(batch.journal_to_string(j, context.temp_allocator)))
	return 1
}

@(private = "file")
error_buf :: proc(out: ^Buf, err: files.Error) {
	store_buf(out, text_buf(files.error_text(err, context.temp_allocator)))
}

@(export, link_name = "negs_batch_journal_write")
batch_journal_write :: proc "c" (path: [^]u8, path_len: uint, json: [^]u8, n: uint, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if error != nil {
		error^ = {}
	}
	j, perr := batch.journal_parse(host_bytes(json, n), context.temp_allocator)
	if perr != .None {
		return 2
	}
	if err := batch.journal_write(j, host_string(path, path_len)); err != 0 {
		error_buf(error, err)
		return 1
	}
	return 0
}

@(export, link_name = "negs_batch_journal_write_bytes")
batch_journal_write_bytes :: proc "c" (path: [^]u8, path_len: uint, bytes: [^]u8, n: uint, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if error != nil {
		error^ = {}
	}
	if err := batch.write_staged(host_string(path, path_len), "json.tmp", host_bytes(bytes, n)); err != 0 {
		error_buf(error, err)
		return 1
	}
	return 0
}

@(export, link_name = "negs_batch_journal_read")
batch_journal_read :: proc "c" (path: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	j, ok := batch.journal_read(host_string(path, n), context.temp_allocator)
	if !ok {
		return 0
	}
	store_buf(out, text_buf(batch.journal_to_string(j, context.temp_allocator)))
	return 1
}

@(export, link_name = "negs_batch_journal_remove")
batch_journal_remove :: proc "c" (path: [^]u8, n: uint) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	batch.journal_remove(host_string(path, n))
}
