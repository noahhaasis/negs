// The Library section's pure helpers: sidecar JSON as the Mac writes it,
// thumbnail names, the export rating, the filter's words, Finder's order and
// the selection, without a handle.
package negs_core

import "base:runtime"
import lib "../library"
import fs "../platform/fs"

// A sidecar's bytes read (Foundation's JSONDecoder) and written again as
// the Mac writes them. 0 and the reason in *out when they aren't JSON.
@(export, link_name = "negs_sidecar_rewrite")
sidecar_rewrite :: proc "c" (data: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	bytes := data[:n] if data != nil else nil
	v, err := lib.json_parse(bytes, context.temp_allocator)
	if e, bad := err.?; bad {
		if out != nil {
			out^ = buf_text(e.reason)
		}
		return 0
	}
	if out != nil {
		out^ = buf_from(lib.to_sidecar_bytes(v, context.temp_allocator))
	}
	return 1
}

// JSON as the thumbnail looks hash it: compact, sorted keys, slashes
// escaped. 0 when it isn't JSON.
@(export, link_name = "negs_json_compact_sorted")
json_compact_sorted :: proc "c" (data: [^]u8, n: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(data, n)
	if !ok {
		return 0
	}
	if out != nil {
		out^ = buf_text(lib.to_compact_sorted(v, context.temp_allocator))
	}
	return 1
}

// The look of stored edits (an object) against defaults table `version`.
@(export, link_name = "negs_thumbnail_look")
thumbnail_look :: proc "c" (edits: [^]u8, edits_len: uint, version: i64) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(edits, edits_len)
	if !ok {
		return {}
	}
	return buf_text(lib.look(v, version, context.temp_allocator))
}

// The look of a photograph without stored edits, developed as `state`.
@(export, link_name = "negs_thumbnail_unedited_look")
thumbnail_unedited_look :: proc "c" (state: [^]u8, state_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(state, state_len)
	if !ok {
		return {}
	}
	s, decoded := dec_state(v)
	if !decoded {
		return {}
	}
	return buf_text(lib.unedited_look(s, context.temp_allocator))
}

// EditedThumbnailStore.key; `variant` NULL for a photograph's own.
@(export, link_name = "negs_thumbnail_key")
thumbnail_key :: proc "c" (
	path: [^]u8,
	path_len: uint,
	has_size: u8,
	size: u64,
	modified: f64,
	variant: [^]u8,
	variant_len: uint,
	look: [^]u8,
	look_len: uint,
	engine: [^]u8,
	engine_len: uint,
) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	key := lib.thumbnail_key(
		host_string(path, path_len),
		has_size != 0,
		size,
		modified,
		host_string(variant, variant_len),
		host_string(look, look_len),
		host_string(engine, engine_len),
		context.temp_allocator,
	)
	return buf_text(key)
}

@(export, link_name = "negs_thumbnail_loose_key")
thumbnail_loose_key :: proc "c" (path: [^]u8, path_len: uint, has_size: u8, size: u64, modified: f64, variant: [^]u8, variant_len: uint, look: [^]u8, look_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	key := lib.thumbnail_loose_key(host_string(path, path_len), has_size != 0, size, modified, host_string(variant, variant_len), host_string(look, look_len), context.temp_allocator)
	return buf_text(key)
}

// A PNG, JPEG or TIFF with the rating embedded; 0 for anything else.
@(export, link_name = "negs_xmp_embed")
xmp_embed :: proc "c" (data: [^]u8, n: uint, rating: i32, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if data == nil {
		return 0
	}
	bytes, ok := lib.xmp_embed(data[:n], rating, context.temp_allocator)
	if !ok {
		return 0
	}
	if out != nil {
		out^ = buf_from(bytes)
	}
	return 1
}

// The rating embedded in the file at `path`: 1 done, 0 not a PNG, JPEG or
// TIFF, 2 an error (its words in *error).
@(export, link_name = "negs_xmp_embed_in_file")
xmp_embed_in_file :: proc "c" (path: [^]u8, path_len: uint, rating: i32, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	done, err := lib.xmp_embed_in_file(host_string(path, path_len), rating)
	if err != 0 {
		if error != nil {
			error^ = buf_text(fs.error_text(err, context.temp_allocator))
		}
		return 2
	}
	return u8(done)
}

// The filter sheet's sentence: [{"lead","word"}].
@(export, link_name = "negs_filter_words")
filter_words :: proc "c" (filter: [^]u8, filter_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(filter, filter_len)
	if !ok {
		return {}
	}
	f, decoded := dec_filter(v)
	if !decoded {
		return {}
	}
	out := arr()
	for w in lib.filter_words(f, context.temp_allocator) {
		o := obj()
		put(&o, "lead", str(w.lead))
		put(&o, "word", str(w.word))
		append(&out, o)
	}
	return buf_json(out)
}

// The grid foot's count ("3 of 12 photographs").
@(export, link_name = "negs_count_words")
count_words :: proc "c" (shown, total, selected: u64, filtered: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_text(lib.count_words(int(shown), int(total), int(selected), filtered != 0, context.temp_allocator))
}

// Finder's order of two file names: -1, 0 or 1.
@(export, link_name = "negs_standard_compare")
standard_compare :: proc "c" (a: [^]u8, a_len: uint, b: [^]u8, b_len: uint) -> i32 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return i32(lib.standard_compare(host_string(a, a_len), host_string(b, b_len)))
}

// Whether sidecars can be written in `folder`.
@(export, link_name = "negs_can_write")
can_write :: proc "c" (folder: [^]u8, folder_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return u8(lib.can_write(host_string(folder, folder_len)))
}

// MARK: Selection

@(private = "file")
enc_set :: proc(s: lib.Item_Set) -> Json {
	a := arr(len(s))
	for _, i in s {
		append(&a, enc_item(i))
	}
	return a
}

@(private = "file")
enc_selection :: proc(s: lib.Selection) -> Json {
	o := obj()
	put(&o, "current", enc_item(s.current) if s.has_current else nil)
	put(&o, "selected", enc_set(s.selected))
	put(&o, "anchor", enc_item(s.anchor) if s.has_anchor else nil)
	put(&o, "lead", enc_item(s.lead) if s.has_lead else nil)
	put(&o, "base", enc_set(s.base))
	return o
}

@(private = "file")
dec_opt_item :: proc(v: Json) -> (item: lib.Item, has: bool, ok: bool) {
	if v == nil {
		return {}, false, true
	}
	item = dec_item(v) or_return
	return item, true, true
}

@(private = "file")
dec_set :: proc(v: Json) -> (lib.Item_Set, bool) {
	items, ok := dec_items(v)
	if !ok {
		return nil, false
	}
	return lib.item_set_of(items), true
}

@(private = "file")
dec_selection :: proc(v: Json) -> (s: lib.Selection, ok: bool) {
	o := v.(Json_Object) or_return
	s.current, s.has_current = dec_opt_item(lib.json_get(o, "current")) or_return
	s.selected = dec_set(lib.json_get(o, "selected")) or_return
	s.anchor, s.has_anchor = dec_opt_item(lib.json_get(o, "anchor")) or_return
	s.lead, s.has_lead = dec_opt_item(lib.json_get(o, "lead")) or_return
	s.base = dec_set(lib.json_get(o, "base")) or_return
	return s, true
}

// One change to a selection (LibrarySelection): `state` is the selection
// as JSON, `op` the change; returns {"selection", "items"}, `items` the
// answer of "ordered" and "nextAfter" (null otherwise). Empty on a bad
// argument. Ops: {"op":"select","item"}, {"op":"selectAllOf","items"},
// {"op":"toggle","item","order":[..]|null}, {"op":"extendTo","item","order"},
// {"op":"extendBy","offset","order"}, {"op":"selectAll","shown"},
// {"op":"deselectAll"}, {"op":"band","items","base"},
// {"op":"prune","shown"}, {"op":"rename","from","to"},
// {"op":"ordered","order"}, {"op":"nextAfter","item","order","gone"}.
@(export, link_name = "negs_selection_apply")
selection_apply :: proc "c" (state: [^]u8, state_len: uint, op: [^]u8, op_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	sv, s_ok := parse(state, state_len)
	ov, o_ok := parse(op, op_len)
	if !s_ok || !o_ok {
		return {}
	}
	s, decoded := dec_selection(sv)
	o, is_object := ov.(Json_Object)
	name, named := lib.json_get(o, "op").(string)
	if !decoded || !is_object || !named {
		return {}
	}
	answer: Json
	ok := selection_op(&s, name, o, &answer)
	if !ok {
		return {}
	}
	out := obj()
	put(&out, "selection", enc_selection(s))
	put(&out, "items", answer)
	return buf_json(out)
}

@(private = "file")
selection_op :: proc(s: ^lib.Selection, name: string, o: Json_Object, answer: ^Json) -> bool {
	get :: proc(o: Json_Object, key: string) -> Json {
		return lib.json_get(o, key)
	}
	switch name {
	case "select":
		item := dec_item(get(o, "item")) or_return
		lib.selection_select(s, item)
	case "selectAllOf":
		items := dec_items(get(o, "items")) or_return
		lib.selection_select_all_of(s, items)
	case "toggle":
		item := dec_item(get(o, "item")) or_return
		order: []lib.Item
		has_order := get(o, "order") != nil
		if has_order {
			order = dec_items(get(o, "order")) or_return
		}
		lib.selection_toggle(s, item, order, has_order)
	case "extendTo":
		item := dec_item(get(o, "item")) or_return
		order := dec_items(get(o, "order")) or_return
		lib.selection_extend_to(s, item, order)
	case "extendBy":
		offset := dec_number(get(o, "offset")) or_return
		order := dec_items(get(o, "order")) or_return
		lib.selection_extend_by(s, int(offset), order)
	case "selectAll":
		shown := dec_items(get(o, "shown")) or_return
		lib.selection_select_all(s, shown)
	case "deselectAll":
		lib.selection_deselect_all(s)
	case "band":
		items := dec_items(get(o, "items")) or_return
		base := dec_set(get(o, "base")) or_return
		lib.selection_band(s, items, base)
	case "prune":
		shown := dec_set(get(o, "shown")) or_return
		lib.selection_prune(s, shown)
	case "rename":
		from := dec_item(get(o, "from")) or_return
		to := dec_item(get(o, "to")) or_return
		lib.selection_rename(s, from, to)
	case "ordered":
		order := dec_items(get(o, "order")) or_return
		answer^ = enc_items(lib.selection_ordered(s, order))
	case "nextAfter":
		item := dec_item(get(o, "item")) or_return
		order := dec_items(get(o, "order")) or_return
		gone := dec_set(get(o, "gone")) or_return
		next, found := lib.next_after(item, order, gone)
		a := arr(1)
		if found {
			append(&a, enc_item(next))
		}
		answer^ = a
	case:
		return false
	}
	return true
}
