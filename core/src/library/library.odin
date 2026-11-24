// The open folder as a library: its photographs, their ratings, flags and
// edits, the open photograph, and undo. What `PhotoLibrary` and the
// library half of `EditorModel` do on macOS, without a UI (library.rs).
//
// Every call works on the files at once: a sidecar is read, patched and
// written (atomically) before the call returns. The open photograph's edits
// are the exception: `library_edit` keeps them in memory and
// `library_save` writes them.
//
// Memory: everything the library keeps is in `lib.allocator`; what a call
// returns is in the temporary allocator, which the caller frees after it.
package library

import "base:runtime"
import "core:os"
import "core:strings"
import "core:time"
import fs "../platform/fs"

// Where deleted photographs go: the platform's bin, given by the host.
// Strings returned are temporary.
Bin :: struct {
	user:      rawptr,
	// What the platform calls it ("Trash", "Recycle Bin").
	name:      proc(user: rawptr) -> string,
	// Whether `folder`'s volume has a bin. Without one, a delete is
	// permanent.
	available: proc(user: rawptr, folder: string) -> bool,
	// Moves `file` to the bin: where it went, or why it couldn't.
	trash:     proc(user: rawptr, file: string) -> (landed: string, error: string, ok: bool),
	// Puts a file back from where `trash` put it.
	put_back:  proc(user: rawptr, trashed, original: string) -> (error: string, ok: bool),
	// Deletes `file` at once; nil unlinks it.
	remove:    proc(user: rawptr, file: string) -> (error: string, ok: bool),
	// Called once when the library is freed.
	release:   proc(user: rawptr),
}

bin_name :: proc(b: Bin) -> string {
	if b.name == nil {
		return "Trash"
	}
	return b.name(b.user)
}

bin_remove :: proc(b: Bin, file: string) -> (string, bool) {
	if b.remove != nil {
		return b.remove(b.user, file)
	}
	if err := fs.remove_file(file); err != 0 {
		return fs.error_text(err, context.temp_allocator), false
	}
	return "", true
}

// How a library is set up.
Config :: struct {
	// The engine's data folder, which holds `sidecar-defaults/`.
	data:        string,
	// The app's version, written into each sidecar as `app`.
	app:         string,
	bin:         Bin,
	// Settings › Show camera JPEGs separately, off.
	pairing:     bool,
	// Settings › Save ratings and edits next to photos.
	sidecars:    bool,
	// Seeds the ids of new variants; random without.
	has_id_seed: bool,
	id_seed:     u64,
}

// What the app says briefly after an action, and why it wasn't saved.
Note :: struct {
	text:       string,
	has_detail: bool,
	detail:     string,
	// How many photographs it acted on.
	count:      int,
}

// What an action did, beyond the files and the library's summaries.
Outcome :: struct {
	has_note:     bool,
	note:         Note,
	has_error:    bool,
	error:        string,
	open_changed: bool,
	open_gone:    bool,
	has_open:     bool,
	open:         Item,
	select:       []Item,
}

// Sets the outcome's error unless it has one.
outcome_error :: proc(o: ^Outcome, error: string) {
	if !o.has_error {
		o.has_error = true
		o.error = error
	}
}

// What opening a photograph found.
Opened :: struct {
	state:       Edit_State,
	has_problem: bool,
	problem:     string,
}

// What Copy Edits took.
Copied_Edits :: struct {
	state:  Edit_State,
	groups: Edit_Groups,
	edited: bool,
}

// A photograph's stored edits as a batch exports them.
Export_Edits :: struct {
	state:        Edit_State,
	match_solved: bool,
	missing:      []string,
	look:         string,
}

// A stored name and the one shown instead.
Missing :: struct {
	name:  string,
	shown: string,
}

// The open photograph's sidecar as it stands (`PhotoSidecarContext`).
Open :: struct {
	item:             Item,
	variant:          string,
	variant_is_new:   bool,
	defaults_version: i64,
	opened:           Edit_State,
	edited_at_open:   bool,
	// The stored edits last read or sent; nil for none.
	written:          Json,
	has_blocked:      bool,
	blocked:          string,
	has_missing_film: bool,
	missing_film:     Missing,
	has_missing_paper: bool,
	missing_paper:    Missing,
	state:            Edit_State,
}

// A sidecar's size and date as listed; `has` false when it had none.
Stamp :: struct {
	has:      bool,
	modified: i64,
	size:     u64,
}

// A sidecar as it was before and after an undoable action.
File_Change :: struct {
	photo:      string,
	has_before: bool,
	before:     []u8,
	has_after:  bool,
	after:      []u8,
}

Step_Kind :: enum u8 {
	// Ratings and flags.
	Summaries,
	// Paste, Reset, Duplicate: the files as they were.
	Sidecars,
	// Files moved to the bin, and duplicates removed from sidecars.
	Delete,
	// Redo of a delete: the same plan again.
	Redelete,
}

Summary_Change :: struct {
	item:   Item,
	was:    Summary,
	became: Summary,
}

Move :: struct {
	original: string,
	trashed:  string,
}

Step :: struct {
	action:     string,
	kind:       Step_Kind,
	summaries:  []Summary_Change,
	plan:       Delete_Plan,
	moves:      []Move,
	has_reopen: bool,
	reopen:     Item,
	files:      []File_Change,
}

Library :: struct {
	allocator:     Allocator,
	folder:        string,
	tables:        Tables,
	defaults:      Edit_Defaults,
	app:           string,
	bin:           Bin,
	pairing:       bool,
	sidecars:      bool,
	filter:        Filter,
	files:         [dynamic]Photo_File,
	companions:    map[string]string,
	// Each photograph's sidecar as last listed.
	sidecar_files: map[string]Stamp,
	duplicates:    map[string][]string,
	// By `item_key`.
	summaries:     map[string]Summary,
	looks:         map[string]string,
	read_only:     bool,
	has_failed:    bool,
	failed:        string,
	has_open:      bool,
	open:          Open,
	has_copied:    bool,
	copied:        Copied_Edits,
	has_previous:  bool,
	previous:      Edit_State,
	undo:          [dynamic]Step,
	redo:          [dynamic]Step,
	// Folders whose sidecars couldn't be written, said once each.
	unwritable:    map[string]bool,
	ids:           u64,
}

// Small, unpredictable variant ids: 8 lowercase hex digits (SplitMix64).
next_id :: proc(lib: ^Library, allocator := context.temp_allocator) -> string {
	lib.ids += 0x9E37_79B9_7F4A_7C15
	z := lib.ids
	z = (z ~ (z >> 30)) * 0xBF58_476D_1CE4_E5B9
	z = (z ~ (z >> 27)) * 0x94D0_49BB_1331_11EB
	v := u32(z ~ (z >> 31))
	hex := "0123456789abcdef"
	out := make([]u8, 8, allocator)
	for i in 0 ..< 8 {
		out[7 - i] = hex[(v >> (u32(i) * 4)) & 0xF]
	}
	return string(out)
}

@(private = "file")
random_seed :: proc() -> u64 {
	local: u8
	z := u64(time.time_to_unix_nano(time.now())) ~ u64(uintptr(&local)) ~ u64(os.get_pid()) << 32
	z = (z ~ (z >> 30)) * 0xBF58_476D_1CE4_E5B9
	z = (z ~ (z >> 27)) * 0x94D0_49BB_1331_11EB
	return z ~ (z >> 31)
}

// Whether sidecars can be written in `folder`.
can_write :: proc(folder: string) -> bool {
	// `!metadata.permissions().readonly()`, false when it can't be read.
	st, err := fs.stat(folder)
	return err == 0 && !st.readonly
}

// The library of `folder`, listed and its sidecars read; or why not
// (temporary). The bin is the library's from here on, either way.
library_new :: proc(folder: string, config: Config, allocator := context.allocator) -> (^Library, string) {
	lib := new(Library, allocator)
	lib.allocator = allocator
	tables_init(&lib.tables, config.data, allocator)
	table, err := tables_latest(&lib.tables)
	if table == nil {
		tables_destroy(&lib.tables)
		free(lib, allocator)
		if config.bin.release != nil {
			config.bin.release(config.bin.user)
		}
		return nil, err
	}
	lib.defaults = edit_defaults_for_table(table, allocator)
	lib.folder = strings.clone(folder, allocator)
	lib.app = strings.clone(config.app, allocator)
	lib.bin = config.bin
	lib.pairing = config.pairing
	lib.sidecars = config.sidecars
	lib.files = make([dynamic]Photo_File, allocator)
	lib.companions = make(map[string]string, allocator)
	lib.sidecar_files = make(map[string]Stamp, allocator)
	lib.duplicates = make(map[string][]string, allocator)
	lib.summaries = make(map[string]Summary, allocator)
	lib.looks = make(map[string]string, allocator)
	lib.undo = make([dynamic]Step, allocator)
	lib.redo = make([dynamic]Step, allocator)
	lib.unwritable = make(map[string]bool, allocator)
	lib.ids = config.id_seed if config.has_id_seed else random_seed()
	library_refresh(lib, false)
	return lib, ""
}

library_free :: proc(lib: ^Library) {
	a := lib.allocator
	context.allocator = a
	if lib.bin.release != nil {
		lib.bin.release(lib.bin.user)
	}
	tables_destroy(&lib.tables)
	edit_defaults_destroy(lib.defaults, a)
	delete(lib.folder, a)
	delete(lib.app, a)
	set_files(lib, nil)
	delete(lib.files)
	clear_string_map(&lib.companions, true)
	delete(lib.companions)
	for k in lib.sidecar_files {
		delete(k, a)
	}
	delete(lib.sidecar_files)
	for k, v in lib.duplicates {
		delete(k, a)
		strings_destroy(v, a)
	}
	delete(lib.duplicates)
	for k, v in lib.summaries {
		delete(k, a)
		summary_destroy(v, a)
	}
	delete(lib.summaries)
	clear_string_map(&lib.looks, true)
	delete(lib.looks)
	delete(lib.failed, a)
	close_open(lib)
	if lib.has_copied {
		edit_state_destroy(lib.copied.state, a)
	}
	if lib.has_previous {
		edit_state_destroy(lib.previous, a)
	}
	for s in lib.undo {
		step_destroy(s, a)
	}
	delete(lib.undo)
	for s in lib.redo {
		step_destroy(s, a)
	}
	delete(lib.redo)
	for k in lib.unwritable {
		delete(k, a)
	}
	delete(lib.unwritable)
	free(lib, a)
}

// MARK: Owning

strings_destroy :: proc(list: []string, allocator: Allocator) {
	for s in list {
		delete(s, allocator)
	}
	delete(list, allocator)
}

strings_clone :: proc(list: []string, allocator: Allocator) -> []string {
	out := make([]string, len(list), allocator)
	for s, i in list {
		out[i] = strings.clone(s, allocator)
	}
	return out
}

@(private = "file")
clear_string_map :: proc(m: ^map[string]string, values: bool) {
	a := m.allocator
	for k, v in m {
		delete(k, a)
		if values {
			delete(v, a)
		}
	}
	clear(m)
}

@(private)
set_files :: proc(lib: ^Library, files: []Photo_File) {
	for f in lib.files {
		photo_file_destroy(f, lib.allocator)
	}
	clear(&lib.files)
	for f in files {
		append(&lib.files, photo_file_clone(f, lib.allocator))
	}
}

@(private)
set_string :: proc(lib: ^Library, field: ^string, value: string) {
	old := field^
	field^ = strings.clone(value, lib.allocator)
	delete(old, lib.allocator)
}

@(private)
set_failed :: proc(lib: ^Library, has: bool, text: string) {
	lib.has_failed = has
	set_string(lib, &lib.failed, text if has else "")
}

@(private)
set_summary :: proc(lib: ^Library, item: Item, s: Summary) {
	key := item_key(item, context.temp_allocator)
	if old, found := lib.summaries[key]; found {
		summary_destroy(old, lib.allocator)
		lib.summaries[key] = summary_clone(s, lib.allocator)
	} else {
		lib.summaries[strings.clone(key, lib.allocator)] = summary_clone(s, lib.allocator)
	}
}

@(private)
remove_summary :: proc(lib: ^Library, item: Item) {
	key := item_key(item, context.temp_allocator)
	if key in lib.summaries {
		k, v := delete_key(&lib.summaries, key)
		delete(k, lib.allocator)
		summary_destroy(v, lib.allocator)
	}
}

@(private)
set_look :: proc(lib: ^Library, item: Item, look: string) {
	key := item_key(item, context.temp_allocator)
	if old, found := lib.looks[key]; found {
		delete(old, lib.allocator)
		lib.looks[key] = strings.clone(look, lib.allocator)
	} else {
		lib.looks[strings.clone(key, lib.allocator)] = strings.clone(look, lib.allocator)
	}
}

@(private)
remove_look :: proc(lib: ^Library, item: Item) {
	key := item_key(item, context.temp_allocator)
	if key in lib.looks {
		k, v := delete_key(&lib.looks, key)
		delete(k, lib.allocator)
		delete(v, lib.allocator)
	}
}

@(private)
open_destroy :: proc(o: Open, a: Allocator) {
	item_destroy(o.item, a)
	delete(o.variant, a)
	edit_state_destroy(o.opened, a)
	json_destroy(o.written, a)
	delete(o.blocked, a)
	delete(o.missing_film.name, a)
	delete(o.missing_film.shown, a)
	delete(o.missing_paper.name, a)
	delete(o.missing_paper.shown, a)
	edit_state_destroy(o.state, a)
}

// A temporary copy of the open photograph's sidecar context.
@(private)
open_clone :: proc(o: Open, a: Allocator) -> Open {
	out := o
	out.item = item_clone(o.item, a)
	out.variant = strings.clone(o.variant, a)
	out.opened = edit_state_clone(o.opened, a)
	out.written = json_clone(o.written, a)
	out.blocked = strings.clone(o.blocked, a)
	out.missing_film = Missing{strings.clone(o.missing_film.name, a), strings.clone(o.missing_film.shown, a)}
	out.missing_paper = Missing{strings.clone(o.missing_paper.name, a), strings.clone(o.missing_paper.shown, a)}
	out.state = edit_state_clone(o.state, a)
	return out
}

@(private)
close_open :: proc(lib: ^Library) {
	if lib.has_open {
		open_destroy(lib.open, lib.allocator)
		lib.open = {}
		lib.has_open = false
	}
}

@(private)
set_blocked :: proc(lib: ^Library, reason: string) {
	lib.open.has_blocked = true
	set_string(lib, &lib.open.blocked, reason)
}

@(private)
set_open_state :: proc(lib: ^Library, state: Edit_State) {
	old := lib.open.state
	lib.open.state = edit_state_clone(state, lib.allocator)
	edit_state_destroy(old, lib.allocator)
}

@(private)
set_open_opened :: proc(lib: ^Library, state: Edit_State) {
	old := lib.open.opened
	lib.open.opened = edit_state_clone(state, lib.allocator)
	edit_state_destroy(old, lib.allocator)
}

@(private)
set_open_written :: proc(lib: ^Library, written: Json) {
	old := lib.open.written
	lib.open.written = json_clone(written, lib.allocator)
	json_destroy(old, lib.allocator)
}

@(private)
set_previous :: proc(lib: ^Library, state: Edit_State) {
	if lib.has_previous {
		edit_state_destroy(lib.previous, lib.allocator)
	}
	lib.has_previous = true
	lib.previous = edit_state_clone(state, lib.allocator)
}

@(private)
file_change_clone :: proc(c: File_Change, a: Allocator) -> File_Change {
	out := c
	out.photo = strings.clone(c.photo, a)
	out.before = runtime.make_slice([]u8, len(c.before), a) or_else nil
	copy(out.before, c.before)
	out.after = runtime.make_slice([]u8, len(c.after), a) or_else nil
	copy(out.after, c.after)
	return out
}

@(private)
step_clone :: proc(s: Step, a: Allocator) -> Step {
	out := s
	out.action = strings.clone(s.action, a)
	out.summaries = make([]Summary_Change, len(s.summaries), a)
	for c, i in s.summaries {
		out.summaries[i] = Summary_Change{item_clone(c.item, a), summary_clone(c.was, a), summary_clone(c.became, a)}
	}
	out.plan = delete_plan_clone(s.plan, a)
	out.moves = make([]Move, len(s.moves), a)
	for m, i in s.moves {
		out.moves[i] = Move{strings.clone(m.original, a), strings.clone(m.trashed, a)}
	}
	out.reopen = item_clone(s.reopen, a)
	out.files = make([]File_Change, len(s.files), a)
	for f, i in s.files {
		out.files[i] = file_change_clone(f, a)
	}
	return out
}

@(private)
step_destroy :: proc(s: Step, a: Allocator) {
	delete(s.action, a)
	for c in s.summaries {
		item_destroy(c.item, a)
		summary_destroy(c.was, a)
		summary_destroy(c.became, a)
	}
	delete(s.summaries, a)
	delete_plan_destroy(s.plan, a)
	for m in s.moves {
		delete(m.original, a)
		delete(m.trashed, a)
	}
	delete(s.moves, a)
	item_destroy(s.reopen, a)
	for f in s.files {
		delete(f.photo, a)
		delete(f.before, a)
		delete(f.after, a)
	}
	delete(s.files, a)
}

// Keeps `step` (temporary) as the newest undo step; Redo is cleared.
@(private)
push_step :: proc(lib: ^Library, step: Step) {
	append(&lib.undo, step_clone(step, lib.allocator))
	for s in lib.redo {
		step_destroy(s, lib.allocator)
	}
	clear(&lib.redo)
}

// MARK: What it shows

// The photographs, each followed by its duplicates (temporary).
library_items :: proc(lib: ^Library) -> []Item {
	ta := context.temp_allocator
	out := make([dynamic]Item, 0, len(lib.files), ta)
	for f in lib.files {
		photo := strings.clone(f.path, ta)
		append(&out, item_photo(photo))
		if ids, ok := lib.duplicates[f.path]; ok {
			for id in ids {
				append(&out, item_duplicate(photo, strings.clone(id, ta)))
			}
		}
	}
	return out[:]
}

// The listing of `item`'s photograph.
library_file :: proc(lib: ^Library, item: Item) -> (Photo_File, bool) {
	for f in lib.files {
		if f.path == item.photo {
			return f, true
		}
	}
	return {}, false
}

// A photograph's duplicates, by variant id (the library's own).
library_duplicates :: proc(lib: ^Library, photo: string) -> []string {
	if ids, ok := lib.duplicates[photo]; ok {
		return ids
	}
	return nil
}

// `IMG_0412.CR3`, or `IMG_0412.CR3 (2)` for its first duplicate (temporary).
library_name :: proc(lib: ^Library, item: Item) -> string {
	name := file_name(item.photo)
	if item.has_variant {
		for id, index in library_duplicates(lib, item.photo) {
			if id == item.variant {
				return strings.concatenate({name, " (", int_text(index + 2), ")"}, context.temp_allocator)
			}
		}
	}
	return strings.clone(name, context.temp_allocator)
}

@(private)
int_text :: proc(n: int) -> string {
	b := strings.builder_make(0, 20, context.temp_allocator)
	strings.write_int(&b, n)
	return strings.to_string(b)
}

// The library's summary of `item` (the library's own strings).
library_summary :: proc(lib: ^Library, item: Item) -> Summary {
	if s, ok := lib.summaries[item_key(item, context.temp_allocator)]; ok {
		return s
	}
	return {}
}

// The look of an item's stored edits; false without edits.
library_look :: proc(lib: ^Library, item: Item) -> (string, bool) {
	look, ok := lib.looks[item_key(item, context.temp_allocator)]
	return look, ok
}

library_companion :: proc(lib: ^Library, photo: string) -> (string, bool) {
	p, ok := lib.companions[photo]
	return p, ok
}

// What the strip and grid show: what the filter lets through, and always
// `current` (temporary).
library_shown :: proc(lib: ^Library, current: Item, has_current: bool) -> []Item {
	items := library_items(lib)
	if !filter_active(lib.filter) {
		return items
	}
	out := make([dynamic]Item, 0, len(items), context.temp_allocator)
	for i in items {
		if (has_current && item_equal(i, current)) || filter_includes(lib.filter, library_summary(lib, i)) {
			append(&out, i)
		}
	}
	return out[:]
}

// The folder's rejected photographs and duplicates (temporary).
library_rejected :: proc(lib: ^Library) -> []Item {
	items := library_items(lib)
	out := make([dynamic]Item, 0, len(items), context.temp_allocator)
	for i in items {
		if library_summary(lib, i).flag == .Reject {
			append(&out, i)
		}
	}
	return out[:]
}

library_undo_name :: proc(lib: ^Library) -> (string, bool) {
	if len(lib.undo) == 0 {
		return "", false
	}
	return lib.undo[len(lib.undo) - 1].action, true
}

library_redo_name :: proc(lib: ^Library) -> (string, bool) {
	if len(lib.redo) == 0 {
		return "", false
	}
	return lib.redo[len(lib.redo) - 1].action, true
}

library_set_filter :: proc(lib: ^Library, f: Filter) {
	lib.filter = f
}

// A new id for a variant or a roll (temporary).
library_new_id :: proc(lib: ^Library) -> string {
	return next_id(lib)
}

// MARK: Settings

// The engine's defaults and Settings › New photographs (`defaults` is
// copied). A photograph open without edits of its own follows a new film
// or paper, without an undo step.
library_set_defaults :: proc(lib: ^Library, defaults: Edit_Defaults) -> Outcome {
	a := lib.allocator
	ta := context.temp_allocator
	stocks_changed := defaults.new_photo_film != lib.defaults.new_photo_film || defaults.new_photo_paper != lib.defaults.new_photo_paper
	old := lib.defaults
	lib.defaults = edit_defaults_clone(defaults, a)
	edit_defaults_destroy(old, a)
	outcome := Outcome{}
	if !stocks_changed || !lib.has_open {
		return outcome
	}
	with_new_stocks :: proc(d: Edit_Defaults, edits: Edit_State) -> Edit_State {
		ta := context.temp_allocator
		next := edit_state_clone(edits, ta)
		next.film = d.new_photo_film
		next.paper = d.new_photo_paper
		positive, _ := film_lookup(d, next.film)
		json_set_at(&next.params, "io.scan_film", positive, ta)
		return next
	}
	open := &lib.open
	if open.edited_at_open || open.has_blocked {
		return outcome
	}
	if !edit_state_equal(user_edits(open.state, ta), open.opened) {
		set_open_opened(lib, with_new_stocks(lib.defaults, open.opened))
		return outcome
	}
	next := with_new_stocks(lib.defaults, open.state)
	if !edit_state_equal(next, open.state) {
		set_open_state(lib, next)
		set_open_opened(lib, user_edits(lib.open.state, ta))
		outcome.open_changed = true
	}
	return outcome
}

// Settings › Show camera JPEGs separately (off pairs them).
library_set_pairing :: proc(lib: ^Library, pairing: bool) {
	if pairing != lib.pairing {
		lib.pairing = pairing
		library_refresh(lib, false)
	}
}

// Settings › Save ratings and edits next to photos.
library_set_sidecars :: proc(lib: ^Library, on: bool) {
	if on == lib.sidecars {
		return
	}
	a := lib.allocator
	lib.sidecars = on
	for k, v in lib.summaries {
		delete(k, a)
		summary_destroy(v, a)
	}
	clear(&lib.summaries)
	clear_string_map(&lib.looks, true)
	for k, v in lib.duplicates {
		delete(k, a)
		strings_destroy(v, a)
	}
	clear(&lib.duplicates)
	for k in lib.sidecar_files {
		delete(k, a)
	}
	clear(&lib.sidecar_files)
	library_refresh(lib, false)
}
