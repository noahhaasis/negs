// The Library section of negs_core.h: a folder's photographs, their
// sidecars and undo, behind an opaque handle (core/src/library). Compound
// values go both ways as UTF-8 JSON (shapes in the header); each call's
// temporary memory is one arena, gone when it returns.
package negs_core

import "base:runtime"
import lib "../library"
import "core:strings"

// negs_bin_t
Bin_C :: struct {
	user:      rawptr,
	name:      proc "c" (user: rawptr) -> Buf,
	available: proc "c" (user: rawptr, folder: [^]u8, n: uint) -> u8,
	trash:     proc "c" (user: rawptr, file: [^]u8, n: uint, out: ^Buf) -> u8,
	put_back:  proc "c" (user: rawptr, trashed: [^]u8, trashed_len: uint, original: [^]u8, original_len: uint, error: ^Buf) -> u8,
	remove:    proc "c" (user: rawptr, file: [^]u8, n: uint, error: ^Buf) -> u8,
	release:   proc "c" (user: rawptr),
}

// negs_library_config_t
Library_Config :: struct {
	folder:      [^]u8,
	folder_len:  uint,
	data:        [^]u8,
	data_len:    uint,
	app:         [^]u8,
	app_len:     uint,
	bin:         Bin_C,
	pairing:     u8,
	sidecars:    u8,
	has_id_seed: u8,
	id_seed:     u64,
}

// negs_transform_fn
Transform_Fn :: proc "c" (user: rawptr, item: [^]u8, item_len: uint, state: [^]u8, state_len: uint) -> Buf

// MARK: The call's arena

// 16 KB to start, growing in blocks as a call needs: most calls (a name, a
// summary) need little, and a large first block cost more than the call.
@(private, deferred_out = call_end)
call_begin :: proc(arena: ^runtime.Arena) -> ^runtime.Arena {
	_ = runtime.arena_init(arena, 16 * 1024, context.allocator)
	return arena
}

@(private)
call_end :: proc(arena: ^runtime.Arena) {
	runtime.arena_destroy(arena)
}

// A host block's bytes, copied into the temporary allocator; the block is
// freed.
@(private)
take :: proc(b: Buf) -> string {
	if b.data == nil {
		return ""
	}
	s := strings.clone(string(b.data[:b.len]), context.temp_allocator)
	buf_free_in_context(b)
	return s
}

// MARK: The host's bin

@(private = "file")
bin_name :: proc(user: rawptr) -> string {
	c := (^Bin_C)(user)
	if c.name == nil {
		return "Trash"
	}
	return take(c.name(c.user))
}

@(private = "file")
bin_available :: proc(user: rawptr, folder: string) -> bool {
	c := (^Bin_C)(user)
	return c.available != nil && c.available(c.user, raw_data(folder), len(folder)) != 0
}

@(private = "file")
bin_trash :: proc(user: rawptr, file: string) -> (string, string, bool) {
	c := (^Bin_C)(user)
	if c.trash == nil {
		return "", "the bin can’t take files", false
	}
	out: Buf
	ok := c.trash(c.user, raw_data(file), len(file), &out) != 0
	text := take(out)
	if ok {
		return text, "", true
	}
	return "", text, false
}

@(private = "file")
bin_put_back :: proc(user: rawptr, trashed, original: string) -> (string, bool) {
	c := (^Bin_C)(user)
	if c.put_back == nil {
		return "the bin can’t put files back", false
	}
	error: Buf
	ok := c.put_back(c.user, raw_data(trashed), len(trashed), raw_data(original), len(original), &error) != 0
	text := take(error)
	return text, ok
}

@(private = "file")
bin_remove :: proc(user: rawptr, file: string) -> (string, bool) {
	c := (^Bin_C)(user)
	error: Buf
	ok := c.remove(c.user, raw_data(file), len(file), &error) != 0
	text := take(error)
	return text, ok
}

@(private = "file")
bin_release :: proc(user: rawptr) {
	c := (^Bin_C)(user)
	if c.release != nil {
		c.release(c.user)
	}
	free(c)
}

// MARK: The handle

@(export, link_name = "negs_library_new")
library_new :: proc "c" (config: ^Library_Config, error: ^Buf) -> ^lib.Library {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if error != nil {
		error^ = {}
	}
	if config == nil {
		if error != nil {
			error^ = buf_text("no config")
		}
		return nil
	}
	box := new(Bin_C)
	box^ = config.bin
	bin := lib.Bin {
		user      = box,
		name      = bin_name,
		available = bin_available,
		trash     = bin_trash,
		put_back  = bin_put_back,
		remove    = bin_remove if config.bin.remove != nil else nil,
		release   = bin_release,
	}
	c := lib.Config {
		data        = host_string(config.data, config.data_len),
		app         = host_string(config.app, config.app_len),
		bin         = bin,
		pairing     = config.pairing != 0,
		sidecars    = config.sidecars != 0,
		has_id_seed = config.has_id_seed != 0,
		id_seed     = config.id_seed,
	}
	handle, err := lib.library_new(host_string(config.folder, config.folder_len), c)
	if handle == nil && error != nil {
		error^ = buf_text(err)
	}
	return handle
}

@(export, link_name = "negs_library_free")
library_free :: proc "c" (l: ^lib.Library) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if l != nil {
		lib.library_free(l)
	}
}

// MARK: What it shows

@(export, link_name = "negs_library_snapshot")
library_snapshot :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	o := obj()
	put(&o, "folder", str(l.folder))
	files := arr(len(l.files))
	for f in l.files {
		append(&files, enc_file(f))
	}
	put(&o, "files", files)
	companions := arr(len(l.companions))
	for raw, picture in l.companions {
		pair := arr(2)
		append(&pair, str(raw), str(picture))
		append(&companions, pair)
	}
	put(&o, "companions", companions)
	duplicates := arr(len(l.duplicates))
	for photo, ids in l.duplicates {
		pair := arr(2)
		append(&pair, str(photo), enc_strings(ids))
		append(&duplicates, pair)
	}
	put(&o, "duplicates", duplicates)
	summaries := arr(len(l.summaries))
	for key, s in l.summaries {
		pair := arr(2)
		append(&pair, enc_item(lib.item_from_key(key)), enc_summary(s))
		append(&summaries, pair)
	}
	put(&o, "summaries", summaries)
	looks := arr(len(l.looks))
	for key, look in l.looks {
		pair := arr(2)
		append(&pair, enc_item(lib.item_from_key(key)), str(look))
		append(&looks, pair)
	}
	put(&o, "looks", looks)
	put(&o, "readOnly", l.read_only)
	put(&o, "failed", opt_str(l.has_failed, l.failed))
	put(&o, "filter", enc_filter(l.filter))
	put(&o, "pairing", l.pairing)
	put(&o, "sidecars", l.sidecars)
	put(&o, "openItem", enc_item(l.open.item) if l.has_open else nil)
	undo, has_undo := lib.library_undo_name(l)
	put(&o, "undoName", opt_str(has_undo, undo))
	redo, has_redo := lib.library_redo_name(l)
	put(&o, "redoName", opt_str(has_redo, redo))
	depth := arr(2)
	append(&depth, num(len(l.undo)), num(len(l.redo)))
	put(&o, "undoDepth", depth)
	return buf_json(o)
}

@(export, link_name = "negs_library_items")
library_items :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_items(lib.library_items(l)))
}

// `current` is an Item or empty for none.
@(private)
current_item :: proc(data: [^]u8, n: uint) -> (lib.Item, bool) {
	v, ok := parse(data, n)
	if !ok || v == nil {
		return {}, false
	}
	return dec_item(v)
}

@(export, link_name = "negs_library_shown")
library_shown :: proc "c" (l: ^lib.Library, current: [^]u8, current_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	item, has := current_item(current, current_len)
	return buf_json(enc_items(lib.library_shown(l, item, has)))
}

@(export, link_name = "negs_library_grid")
library_grid :: proc "c" (l: ^lib.Library, current: [^]u8, current_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	item, has := current_item(current, current_len)
	shown := lib.library_shown(l, item, has)
	out := arr(len(shown))
	for i in shown {
		o := obj()
		put(&o, "item", enc_item(i))
		put(&o, "name", str(lib.library_name(l, i)))
		put(&o, "summary", enc_summary(lib.library_summary(l, i)))
		look, has_look := lib.library_look(l, i)
		put(&o, "look", opt_str(has_look, look))
		append(&out, o)
	}
	return buf_json(out)
}

@(export, link_name = "negs_library_rejected")
library_rejected :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_items(lib.library_rejected(l)))
}

@(export, link_name = "negs_library_name")
library_name :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	if !ok {
		return {}
	}
	return buf_text(lib.library_name(l, i))
}

@(export, link_name = "negs_library_summary")
library_summary :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, _ := current_item(item, item_len)
	return buf_json(enc_summary(lib.library_summary(l, i)))
}

@(export, link_name = "negs_library_thumbnail_state")
library_thumbnail_state :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	if !ok {
		return buf_json(nil)
	}
	state, solved, found := lib.library_thumbnail_state(l, i)
	if !found {
		return buf_json(nil)
	}
	o := obj()
	put(&o, "state", enc_state(state))
	put(&o, "matchSolved", solved)
	return buf_json(o)
}

@(export, link_name = "negs_library_export_edits")
library_export_edits :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	if !ok {
		return buf_json(nil)
	}
	e, found := lib.library_export_edits(l, i)
	if !found {
		return buf_json(nil)
	}
	o := obj()
	put(&o, "state", enc_state(e.state))
	put(&o, "matchSolved", e.match_solved)
	put(&o, "missing", enc_strings(e.missing))
	put(&o, "look", str(e.look))
	return buf_json(o)
}

@(export, link_name = "negs_library_store_solved_match")
library_store_solved_match :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint, look: [^]u8, look_len: uint, state: [^]u8, state_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	v, parsed := parse(state, state_len)
	if !ok || !parsed {
		return 0
	}
	s, decoded := dec_state(v)
	if !decoded {
		return 0
	}
	return u8(lib.library_store_solved_match(l, i, host_string(look, look_len), s))
}

@(export, link_name = "negs_library_unedited_state")
library_unedited_state :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_state(lib.library_unedited_state(l)))
}

@(export, link_name = "negs_library_defaults")
library_defaults :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_defaults(l.defaults))
}

@(export, link_name = "negs_library_copied")
library_copied :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if !l.has_copied {
		return buf_json(nil)
	}
	o := obj()
	put(&o, "state", enc_state(l.copied.state))
	put(&o, "groups", enc_groups(l.copied.groups))
	put(&o, "edited", l.copied.edited)
	return buf_json(o)
}

@(export, link_name = "negs_library_previous_edits")
library_previous_edits :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_state(l.previous) if l.has_previous else nil)
}

@(export, link_name = "negs_library_state")
library_state :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_json(enc_state(l.open.state) if l.has_open else nil)
}

@(export, link_name = "negs_library_open_edited")
library_open_edited :: proc "c" (l: ^lib.Library) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return u8(lib.library_open_edited(l))
}

@(export, link_name = "negs_library_unsaved")
library_unsaved :: proc "c" (l: ^lib.Library) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return u8(lib.library_unsaved(l))
}

// MARK: Settings

@(export, link_name = "negs_library_set_defaults")
library_set_defaults :: proc "c" (l: ^lib.Library, defaults: [^]u8, defaults_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(defaults, defaults_len)
	if !ok {
		return {}
	}
	d, decoded := dec_defaults(v)
	if !decoded {
		return {}
	}
	return buf_json(enc_outcome(lib.library_set_defaults(l, d)))
}

@(export, link_name = "negs_library_set_filter")
library_set_filter :: proc "c" (l: ^lib.Library, filter: [^]u8, filter_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(filter, filter_len)
	if !ok {
		return 0
	}
	f, decoded := dec_filter(v)
	if !decoded {
		return 0
	}
	lib.library_set_filter(l, f)
	return 1
}

@(export, link_name = "negs_library_set_pairing")
library_set_pairing :: proc "c" (l: ^lib.Library, pairing: u8) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	lib.library_set_pairing(l, pairing != 0)
}

@(export, link_name = "negs_library_set_sidecars")
library_set_sidecars :: proc "c" (l: ^lib.Library, on: u8) {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	lib.library_set_sidecars(l, on != 0)
}

@(export, link_name = "negs_library_refresh")
library_refresh :: proc "c" (l: ^lib.Library, interacting: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	changed, outcome := lib.library_refresh(l, interacting != 0)
	o := obj()
	put(&o, "changed", enc_strings(changed))
	put(&o, "outcome", enc_outcome(outcome))
	return buf_json(o)
}

// MARK: The open photograph

@(export, link_name = "negs_library_open")
library_open :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	if !ok {
		return {}
	}
	opened := lib.library_open(l, i)
	o := obj()
	put(&o, "state", enc_state(opened.state))
	put(&o, "problem", opt_str(opened.has_problem, opened.problem))
	return buf_json(o)
}

@(private)
saved_result :: proc(text: string, ok: bool, error: ^Buf) -> u8 {
	if error != nil {
		error^ = {} if ok else buf_text(text)
	}
	return u8(ok)
}

@(export, link_name = "negs_library_close")
library_close :: proc "c" (l: ^lib.Library, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	text, ok := lib.library_close(l)
	return saved_result(text, ok, error)
}

@(export, link_name = "negs_library_save")
library_save :: proc "c" (l: ^lib.Library, error: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	text, ok := lib.library_save(l)
	return saved_result(text, ok, error)
}

@(export, link_name = "negs_library_edit")
library_edit :: proc "c" (l: ^lib.Library, state: [^]u8, state_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	v, ok := parse(state, state_len)
	if !ok {
		return 0
	}
	s, decoded := dec_state(v)
	if !decoded {
		return 0
	}
	lib.library_edit(l, s)
	return 1
}

@(export, link_name = "negs_library_opening_state")
library_opening_state :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	if !ok {
		return buf_json(nil)
	}
	state, found := lib.library_opening_state(l, i)
	return buf_json(enc_state(state) if found else nil)
}

@(export, link_name = "negs_library_new_id")
library_new_id :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	return buf_text(lib.library_new_id(l))
}

// MARK: Actions on items

@(private)
items_arg :: proc(data: [^]u8, n: uint) -> ([]lib.Item, bool) {
	v, ok := parse(data, n)
	if !ok {
		return nil, false
	}
	return dec_items(v)
}

@(export, link_name = "negs_library_set_rating")
library_set_rating :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint, stars: i64) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_set_rating(l, list, stars)))
}

@(export, link_name = "negs_library_change_rating")
library_change_rating :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint, by: i64) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_change_rating(l, list, by)))
}

// `flag`: 0 unflagged, 1 pick, 2 reject.
@(export, link_name = "negs_library_set_flag")
library_set_flag :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint, flag: u8) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok || flag > 2 {
		return {}
	}
	return buf_json(enc_outcome(lib.library_set_flag(l, list, lib.Flag(flag))))
}

@(export, link_name = "negs_library_copy_edits")
library_copy_edits :: proc "c" (l: ^lib.Library, item: [^]u8, item_len: uint, groups: [^]u8, groups_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	i, ok := current_item(item, item_len)
	v, parsed := parse(groups, groups_len)
	if !ok || !parsed {
		return {}
	}
	g, decoded := dec_groups(v)
	if !decoded {
		return {}
	}
	return buf_json(enc_outcome(lib.library_copy_edits(l, i, g)))
}

@(export, link_name = "negs_library_paste_edits")
library_paste_edits :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_paste_edits(l, list)))
}

@(export, link_name = "negs_library_paste_from_previous")
library_paste_from_previous :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_paste_from_previous(l, list)))
}

@(export, link_name = "negs_library_reset_edits")
library_reset_edits :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_reset_edits(l, list)))
}

@(private = "file")
Host_Transform :: struct {
	fn:   Transform_Fn,
	user: rawptr,
}

// Asks the host for an item's new edits; a reply that isn't an EditState
// leaves them as they are.
@(private = "file")
host_transform :: proc(user: rawptr, item: lib.Item, state: lib.Edit_State) -> lib.Edit_State {
	h := (^Host_Transform)(user)
	ta := context.temp_allocator
	item_text := strings.builder_make(0, 128, ta)
	lib.write_compact(&item_text, enc_item(item))
	state_text := strings.builder_make(0, 1024, ta)
	lib.write_compact(&state_text, enc_state(state))
	reply := take(h.fn(h.user, raw_data(item_text.buf), len(item_text.buf), raw_data(state_text.buf), len(state_text.buf)))
	if v, bad := lib.json_parse(transmute([]u8)reply, ta); bad == nil {
		if next, decoded := dec_state(v); decoded {
			return next
		}
	}
	return lib.edit_state_clone(state, ta)
}

@(export, link_name = "negs_library_change_each_edit")
library_change_each_edit :: proc "c" (
	l: ^lib.Library,
	items: [^]u8,
	items_len: uint,
	action: [^]u8,
	action_len: uint,
	note: [^]u8,
	note_len: uint,
	transform: Transform_Fn,
	user: rawptr,
) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok || transform == nil {
		return {}
	}
	h := Host_Transform{transform, user}
	t := lib.Transform {
		kind     = .Callback,
		callback = host_transform,
		user     = &h,
	}
	return buf_json(enc_outcome(lib.library_change_each_edit(l, list, host_string(action, action_len), host_string(note, note_len), t)))
}

@(export, link_name = "negs_library_duplicate")
library_duplicate :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_duplicate(l, list)))
}

// MARK: Deleting

@(export, link_name = "negs_library_delete_plan")
library_delete_plan :: proc "c" (l: ^lib.Library, items: [^]u8, items_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	list, ok := items_arg(items, items_len)
	if !ok {
		return {}
	}
	return buf_json(enc_plan(lib.library_delete_plan(l, list)))
}

@(private)
plan_arg :: proc(data: [^]u8, n: uint) -> (lib.Delete_Plan, bool) {
	v, ok := parse(data, n)
	if !ok {
		return {}, false
	}
	return dec_plan(v)
}

@(export, link_name = "negs_library_delete_question")
library_delete_question :: proc "c" (l: ^lib.Library, plan: [^]u8, plan_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	p, ok := plan_arg(plan, plan_len)
	if !ok {
		return {}
	}
	q := lib.library_delete_question(l, p)
	o := obj()
	put(&o, "message", str(q.message))
	put(&o, "information", str(q.information))
	put(&o, "button", str(q.button))
	return buf_json(o)
}

@(export, link_name = "negs_library_delete_is_permanent")
library_delete_is_permanent :: proc "c" (l: ^lib.Library, plan: [^]u8, plan_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	p, ok := plan_arg(plan, plan_len)
	if !ok {
		return 0
	}
	return u8(lib.library_delete_is_permanent(l, p))
}

@(export, link_name = "negs_library_delete")
library_delete :: proc "c" (l: ^lib.Library, plan: [^]u8, plan_len: uint) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	p, ok := plan_arg(plan, plan_len)
	if !ok {
		return {}
	}
	return buf_json(enc_outcome(lib.library_delete(l, p)))
}

// MARK: Undo

@(export, link_name = "negs_library_undo")
library_undo :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	outcome, done := lib.library_undo(l)
	return buf_json(enc_outcome(outcome) if done else nil)
}

@(export, link_name = "negs_library_redo")
library_redo :: proc "c" (l: ^lib.Library) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	outcome, done := lib.library_redo(l)
	return buf_json(enc_outcome(outcome) if done else nil)
}
