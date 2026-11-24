// What the library does to sidecars: writing a patch, ratings and flags,
// Copy/Paste/Reset Edits and Duplicate, each one undo step (library.rs).
package library

import "base:runtime"
import "core:fmt"
import "core:strings"
import fs "../platform/fs"

// Keeps each sidecar's bytes before an action first touches it
// (temporary).
@(private)
Recorder :: struct {
	before: [dynamic]Recorded,
}

@(private)
Recorded :: struct {
	photo:      string,
	has_before: bool,
	before:     []u8,
}

// A sidecar's bytes, none when it can't be read or isn't there.
@(private)
current_bytes :: proc(photo: string) -> ([]u8, bool) {
	bytes, found, problem := sidecar_read_bytes(photo)
	if problem != nil || !found {
		return nil, false
	}
	return bytes, true
}

@(private)
bytes_equal :: proc(has_a: bool, a: []u8, has_b: bool, b: []u8) -> bool {
	return has_a == has_b && (!has_a || string(a) == string(b))
}

@(private)
recorder_make :: proc() -> Recorder {
	return Recorder{make([dynamic]Recorded, 0, 8, context.temp_allocator)}
}

@(private)
recorder_touch :: proc(r: ^Recorder, photo: string) {
	for e in r.before {
		if e.photo == photo {
			return
		}
	}
	bytes, has := current_bytes(photo)
	append(&r.before, Recorded{strings.clone(photo, context.temp_allocator), has, bytes})
}

@(private)
recorder_photos :: proc(r: ^Recorder) -> []string {
	out := make([]string, len(r.before), context.temp_allocator)
	for e, i in r.before {
		out[i] = e.photo
	}
	return out
}

@(private)
recorder_has :: proc(r: ^Recorder, photo: string) -> bool {
	for e in r.before {
		if e.photo == photo {
			return true
		}
	}
	return false
}

@(private)
recorder_finish :: proc(r: ^Recorder) -> []File_Change {
	out := make([dynamic]File_Change, 0, len(r.before), context.temp_allocator)
	for e in r.before {
		after, has_after := current_bytes(e.photo)
		if !bytes_equal(e.has_before, e.before, has_after, after) {
			append(&out, File_Change{e.photo, e.has_before, e.before, has_after, after})
		}
	}
	return out[:]
}

@(private)
read_only_photo :: proc(photo: string) -> bool {
	dir, ok := parent(photo)
	return !can_write(dir if ok else ".")
}

// Applies `patch` to `photo`'s sidecar as it is on disk now and writes it,
// or removes it once nothing is left; why not (temporary).
@(private)
write :: proc(lib: ^Library, photo: string, patch: ^Patch) -> (string, bool) {
	// Its own scratch memory: an action on many photographs doesn't hold
	// every file's parse until it returns.
	outer := context.temp_allocator
	scratch: runtime.Arena
	_ = runtime.arena_init(&scratch, 16 * 1024, lib.allocator)
	defer runtime.arena_destroy(&scratch)
	context.temp_allocator = runtime.arena_allocator(&scratch)
	reason, ok := write_in(lib, photo, patch)
	return strings.clone(reason, outer), ok
}

@(private = "file")
write_in :: proc(lib: ^Library, photo: string, patch: ^Patch) -> (string, bool) {
	ta := context.temp_allocator
	raw, problem := sidecar_read_raw(photo)
	if p, bad := problem.?; bad {
		return problem_message(p, file_name(photo), ta), false
	}
	next, applied_problem := patch_applied(patch, raw)
	if p, bad := applied_problem.?; bad {
		return problem_message(p, file_name(photo), ta), false
	}
	if json_equal(next, raw) && (raw != nil) == fs.exists(sidecar_path(photo, ta)) {
		return "", true
	}
	if err := sidecar_write_raw(photo, next); err != 0 {
		return fs.error_text(err, ta), false
	}
	return "", true
}

// A write failed: the library shows what the file keeps, the open
// photograph's sidecar isn't written again, and the folder's problem is
// said once (`sidecarFailed`).
@(private)
failed_write :: proc(lib: ^Library, photo_in, reason: string, outcome: ^Outcome) {
	ta := context.temp_allocator
	photo := strings.clone(photo_in, ta)
	if lib.has_open && lib.open.item.photo == photo {
		set_blocked(lib, reason)
	}
	reload(lib, photo)
	folder, _ := parent(photo)
	if folder not_in lib.unwritable {
		lib.unwritable[strings.clone(folder, lib.allocator)] = true
		if !outcome.has_error {
			outcome_error(outcome, fmt.aprintf("Edits to photographs in “%s” can’t be saved beside them: %s", file_name(folder), reason, allocator = ta))
		}
	}
}

// MARK: Ratings and flags

@(private)
Summary_Op_Kind :: enum u8 {
	Set_Rating,
	Change_Rating,
	Set_Flag,
}

@(private)
Summary_Op :: struct {
	kind:  Summary_Op_Kind,
	value: i64,
	flag:  Flag,
}

@(private)
summary_op_apply :: proc(op: Summary_Op, s: ^Summary) {
	switch op.kind {
	case .Set_Rating:
		s.rating = u8(op.value)
	case .Change_Rating:
		s.rating = u8(clamp(i64(s.rating) + op.value, 0, 5))
	case .Set_Flag:
		s.flag = op.flag
	}
}

@(private)
summary_op_note :: proc(op: Summary_Op, s: Summary) -> string {
	if op.kind == .Set_Flag {
		return flag_text(s.flag)
	}
	return rating_text(s.rating, context.temp_allocator)
}

// Rates `items`, 0 (none) to 5 stars.
library_set_rating :: proc(lib: ^Library, items: []Item, stars: i64) -> Outcome {
	return change_summaries(lib, "Rating", "", items, Summary_Op{kind = .Set_Rating, value = clamp(stars, 0, 5)})
}

// Raises or lowers the rating of `items` (] and [).
library_change_rating :: proc(lib: ^Library, items: []Item, by: i64) -> Outcome {
	return change_summaries(lib, "Rating", "Rating Raised" if by > 0 else "Rating Lowered", items, Summary_Op{kind = .Change_Rating, value = by})
}

library_set_flag :: proc(lib: ^Library, items: []Item, flag: Flag) -> Outcome {
	return change_summaries(lib, "Flag", "", items, Summary_Op{kind = .Set_Flag, flag = flag})
}

// `many` is the note for several photographs, when the note would differ
// between them (`changeLibrary`); "" for none.
@(private)
change_summaries :: proc(lib: ^Library, action, many: string, items_in: []Item, op: Summary_Op) -> Outcome {
	ta := context.temp_allocator
	outcome := Outcome{}
	if len(items_in) == 0 {
		return outcome
	}
	items := make([]Item, len(items_in), ta)
	for i, k in items_in {
		items[k] = item_clone(i, ta)
	}
	recorder := recorder_make()
	befores := make([dynamic]Summary_Change, 0, len(items), ta)
	has_unsaved := false
	unsaved: string
	last := Summary{}
	for item in items {
		before := summary_clone(library_summary(lib, item), ta)
		after := before
		summary_op_apply(op, &after)
		last = after
		if summary_equal(after, before) {
			continue
		}
		append(&befores, Summary_Change{item, before, after})
		set_summary(lib, item, after)
		recorder_touch(&recorder, item.photo)
		if reason, ok := save_summary(lib, before, after, item, &outcome); !ok && !has_unsaved {
			has_unsaved, unsaved = true, reason
		}
	}
	for photo in recorder_photos(&recorder) {
		if lib.sidecars {
			reload_keeping_unsaved(lib, photo)
		}
	}
	text := summary_op_note(op, last)
	if len(items) != 1 && many != "" {
		text = many
	}
	outcome.has_note = true
	outcome.note = Note{text, has_unsaved, unsaved, len(items)}
	if len(befores) > 0 {
		files := recorder_finish(&recorder)
		push_step(lib, Step{action = action, kind = .Summaries, summaries = befores[:], files = files})
	}
	return outcome
}

// Reads `photo`'s sidecar again, keeping summaries the app holds but
// couldn't save.
@(private)
reload_keeping_unsaved :: proc(lib: ^Library, photo: string) {
	if read_only_photo(photo) {
		return
	}
	reload(lib, photo)
}

// Writes a rating or flag change to `item`'s sidecar; why it can't, if it
// can't (`saveLibraryChange`).
@(private)
save_summary :: proc(lib: ^Library, before, after: Summary, item: Item, outcome: ^Outcome) -> (string, bool) {
	ta := context.temp_allocator
	if !lib.sidecars {
		return "Not saved: Settings › Save ratings and edits is off", false
	}
	photo := strings.clone(item.photo, ta)
	patch: Patch
	if lib.has_open && item_equal(lib.open.item, item) {
		open := &lib.open
		if open.has_blocked {
			return "Not saved: its .negs file can’t be changed", false
		}
		if read_only_photo(photo) {
			return "Not saved: the folder is read-only", false
		}
		patch = patch_new(strings.clone(open.variant, ta), open.defaults_version, lib.app)
		patch.primary = open.variant_is_new || !open.item.has_variant
	} else {
		if before.has_problem {
			return "Not saved: its .negs file can’t be changed", false
		}
		if read_only_photo(photo) {
			return "Not saved: the folder is read-only", false
		}
		variant := item.variant if item.has_variant else next_id(lib)
		patch = patch_new(variant, LATEST_DEFAULTS, lib.app)
		patch.primary = !item.has_variant
	}
	if after.rating != before.rating {
		patch.set_rating, patch.rating = true, i64(after.rating)
	}
	if after.flag != before.flag {
		patch.set_flag, patch.flag = true, after.flag
	}
	if reason, ok := write(lib, photo, &patch); !ok {
		failed_write(lib, photo, reason, outcome)
	}
	return "", true
}

// MARK: Edits per photograph

// How the edits of each item change: `merge` lays `groups` of `source`
// over them; `identity` leaves them; `callback` asks the host.
Transform :: struct {
	kind:     enum u8 {
		Identity,
		Merge,
		Callback,
	},
	groups:   Edit_Groups,
	source:   Edit_State,
	// Returns the new state (temporary).
	callback: proc(user: rawptr, item: Item, state: Edit_State) -> Edit_State,
	user:     rawptr,
}

@(private)
transform_apply :: proc(t: Transform, item: Item, state: Edit_State) -> Edit_State {
	ta := context.temp_allocator
	switch t.kind {
	case .Identity:
		return edit_state_clone(state, ta)
	case .Merge:
		return edit_group_merge(t.groups, t.source, state, ta)
	case .Callback:
		return t.callback(t.user, item, state)
	}
	return state
}

// Copy Edits: `item`'s edits in `groups`.
library_copy_edits :: proc(lib: ^Library, item: Item, groups: Edit_Groups) -> Outcome {
	outcome := Outcome{has_note = true}
	state, edited, ok := user_edits_of(lib, item)
	if !ok {
		outcome.note = Note{"Edits Not Copied", true, "Its .negs file can’t be read", 1}
		return outcome
	}
	if lib.has_copied {
		edit_state_destroy(lib.copied.state, lib.allocator)
	}
	lib.has_copied = true
	lib.copied = Copied_Edits{edit_state_clone(state, lib.allocator), groups, edited}
	outcome.note = Note{text = "Edits Copied", count = 1}
	return outcome
}

// Paste Edits onto `items`, as one undo step.
library_paste_edits :: proc(lib: ^Library, items: []Item) -> Outcome {
	if !lib.has_copied {
		return {}
	}
	ta := context.temp_allocator
	copied := lib.copied
	t := Transform{kind = .Merge, groups = copied.groups, source = edit_state_clone(copied.state, ta)}
	return change_edits(lib, items, "Paste Edits", "Edits Pasted", false, !copied.edited, copied.groups, t)
}

// Applies the last photograph left with edits, no copy step.
library_paste_from_previous :: proc(lib: ^Library, items: []Item) -> Outcome {
	if !lib.has_previous {
		return {}
	}
	t := Transform{kind = .Merge, groups = EDIT_GROUPS_STANDARD, source = edit_state_clone(lib.previous, context.temp_allocator)}
	return change_edits(lib, items, "Paste from Previous", "Edits Pasted", false, false, {}, t)
}

// Removes the edits of `items`, crop and rotation included. Ratings and
// flags stay.
library_reset_edits :: proc(lib: ^Library, items: []Item) -> Outcome {
	return change_edits(lib, items, "Reset Edits", "Edits Reset", true, false, {}, Transform{kind = .Identity})
}

// Changes each of `items`' edits by `t` as one undo step.
library_change_each_edit :: proc(lib: ^Library, items: []Item, action, note: string, t: Transform) -> Outcome {
	return change_edits(lib, items, action, note, false, false, {}, t)
}

@(private)
items_contain :: proc(items: []Item, item: Item) -> bool {
	for i in items {
		if item_equal(i, item) {
			return true
		}
	}
	return false
}

// Changes the edits of `items` as one undo step: the open one in place, the
// others in their sidecars (`changeEdits`). `from_unedited`: the groups
// were pasted from a photograph without edits.
@(private)
change_edits :: proc(lib: ^Library, items_in: []Item, action, note: string, reset: bool, has_from_unedited: bool, from_unedited: Edit_Groups, t: Transform) -> Outcome {
	ta := context.temp_allocator
	outcome := Outcome{}
	if len(items_in) == 0 {
		return outcome
	}
	items := make([]Item, len(items_in), ta)
	for i, k in items_in {
		items[k] = item_clone(i, ta)
	}
	has_open_item := lib.has_open
	open_item := item_clone(lib.open.item, ta) if lib.has_open else Item{}
	if !lib.sidecars {
		// Nothing is saved per photograph: only the open one changes.
		if lib.has_open && items_contain(items, lib.open.item) {
			next: Edit_State
			if reset {
				next = new_photo_state(lib.defaults, lib.open.state, ta)
			} else {
				next = transform_apply(t, open_item, edit_state_clone(lib.open.state, ta))
			}
			set_open_state(lib, next)
			outcome.open_changed = true
		}
		has_unsaved := false
		for i in items {
			if !has_open_item || !item_equal(i, open_item) {
				has_unsaved = true
			}
		}
		outcome.has_note = true
		outcome.note = Note{note, has_unsaved, "Not saved: Settings › Save ratings and edits is off" if has_unsaved else "", len(items)}
		return outcome
	}
	// The open photograph's own edits are in its sidecar before it's kept.
	library_save(lib)
	recorder := recorder_make()
	has_unsaved := false
	unsaved: string
	CANT :: "Not saved: its .negs file can’t be changed"
	note_unsaved :: proc(has: ^bool, text: ^string, reason: string) {
		if !has^ {
			has^, text^ = true, reason
		}
	}
	for item in items {
		photo := item.photo
		if read_only_photo(photo) {
			note_unsaved(&has_unsaved, &unsaved, "Not saved: the folder is read-only")
			continue
		}
		raw, problem := sidecar_read_raw(photo)
		if problem != nil {
			note_unsaved(&has_unsaved, &unsaved, CANT)
			continue
		}
		if has_open_item && item_equal(item, open_item) {
			open := &lib.open
			if open.has_blocked {
				note_unsaved(&has_unsaved, &unsaved, CANT)
				continue
			}
			// Nothing pasted onto nothing.
			if has_from_unedited && !library_open_edited(lib) {
				continue
			}
			pasted := transform_apply(t, item, edit_state_clone(open.state, ta))
			cleared := has_from_unedited && holds_no_edits(lib.defaults, pasted, from_unedited)
			recorder_touch(&recorder, photo)
			outcome.open_changed = true
			if reset || cleared {
				// Without edits it shows what a new photograph does now.
				next := new_photo_state(lib.defaults, open.state, ta)
				patch := patch_new(strings.clone(open.variant, ta), open.defaults_version, lib.app)
				patch.primary = open.variant_is_new || !open.item.has_variant
				patch.set_edits = true
				set_open_state(lib, next)
				set_open_written(lib, nil)
				lib.open.edited_at_open = false
				set_open_opened(lib, user_edits(lib.open.state, ta))
				if reason, ok := write(lib, photo, &patch); !ok {
					failed_write(lib, photo, reason, &outcome)
				}
			} else {
				set_open_state(lib, pasted)
				if error, ok := library_save(lib); !ok {
					outcome_error(&outcome, error)
				}
			}
			continue
		}
		s: Sidecar
		has_s := raw != nil
		if has_s {
			found, p := sidecar_from_raw(raw)
			if p != nil {
				note_unsaved(&has_unsaved, &unsaved, CANT)
				continue
			}
			s = found
		}
		variant: Variant
		has_variant := false
		if has_s {
			if item.has_variant {
				variant, has_variant = sidecar_variant(s, item.variant)
			} else if len(s.variants) > 0 {
				variant, has_variant = s.variants[0], true
			}
		}
		// A duplicate removed meanwhile.
		if item.has_variant && !has_variant {
			continue
		}
		// Nothing pasted onto nothing.
		if has_from_unedited && (!has_variant || variant.edits == nil) {
			continue
		}
		version := s.defaults_version if has_s else LATEST_DEFAULTS
		table, _ := tables_get(&lib.tables, version)
		if table == nil {
			note_unsaved(&has_unsaved, &unsaved, CANT)
			continue
		}
		edits: Json
		if !reset {
			base: Edit_State
			if has_variant && variant.edits != nil {
				saved, bad := decode_edits(variant.edits, table)
				if bad != nil {
					note_unsaved(&has_unsaved, &unsaved, CANT)
					continue
				}
				base = restored_state(lib.defaults, saved.state, ta)
			} else {
				base = plain_state(lib.defaults, ta)
			}
			pasted := transform_apply(t, item, base)
			// Its match, if on, is solved again when it opens.
			if !(has_from_unedited && holds_no_edits(lib.defaults, pasted, from_unedited)) {
				edits = encode_edits(pasted, false, table, ta)
			}
		}
		recorder_touch(&recorder, photo)
		id := strings.clone(variant.id, ta) if has_variant else next_id(lib)
		patch := patch_new(id, version, lib.app)
		patch.primary = !item.has_variant
		patch.set_edits = true
		patch.edits = edits
		patch.known = defaults_table_known(table, lib.defaults.params, true)
		if reason, ok := write(lib, photo, &patch); !ok {
			failed_write(lib, photo, reason, &outcome)
		}
	}
	finish(lib, &recorder, action, .Sidecars)
	outcome.has_note = true
	outcome.note = Note{note, has_unsaved, unsaved, len(items)}
	return outcome
}

// After changing several sidecars: the library shows them, and one undo
// step puts them back (`finishSidecarChange`).
@(private)
finish :: proc(lib: ^Library, recorder: ^Recorder, action: string, kind: Step_Kind) {
	for photo in recorder_photos(recorder) {
		reload(lib, photo)
	}
	files := recorder_finish(recorder)
	if len(files) > 0 {
		push_step(lib, Step{action = action, kind = kind, files = files})
	}
}

// Duplicate: a virtual copy of each item, a new variant in its sidecar
// with the same edits, rating and flag, listed after the others.
library_duplicate :: proc(lib: ^Library, items_in: []Item) -> Outcome {
	ta := context.temp_allocator
	outcome := Outcome{}
	if len(items_in) == 0 {
		return outcome
	}
	if !lib.sidecars {
		outcome.has_note = true
		outcome.note = Note{"Not Duplicated", true, "Duplicates are kept in .negs files, which Settings › Save ratings and edits turns off", 1}
		return outcome
	}
	items := make([]Item, len(items_in), ta)
	for i, k in items_in {
		items[k] = item_clone(i, ta)
	}
	library_save(lib)
	recorder := recorder_make()
	made := make([dynamic]Item, 0, len(items), ta)
	has_unsaved := false
	unsaved: string
	for item in items {
		photo := item.photo
		if read_only_photo(photo) {
			if !has_unsaved {
				has_unsaved, unsaved = true, "Not saved: the folder is read-only"
			}
			continue
		}
		raw, problem := sidecar_read_raw(photo)
		if problem != nil {
			if !has_unsaved {
				has_unsaved, unsaved = true, "Not saved: its .negs file can’t be changed"
			}
			continue
		}
		recorder_touch(&recorder, photo)
		top: Json_Object
		if o, ok := raw.(Json_Object); ok {
			top = json_clone(o, ta).(Json_Object)
		} else {
			top = make(Json_Object, 0, 4, ta)
			object_set(&top, "negs", f64(SCHEMA))
			object_set(&top, "defaultsVersion", f64(LATEST_DEFAULTS))
		}
		variants: Json_Array
		if v, ok := json_get(top, "variants").(Json_Array); ok {
			variants = json_clone(v, ta).(Json_Array)
		} else {
			variants = make(Json_Array, 0, 2, ta)
		}
		if len(variants) == 0 {
			// The photograph's own variant first, with the id the open one
			// already uses.
			id: string
			if lib.has_open && item_equal(lib.open.item, item) {
				id = strings.clone(lib.open.variant, ta)
			} else {
				id = next_id(lib)
			}
			first := make(Json_Object, 0, 1, ta)
			object_set(&first, "id", json_string(id, ta))
			append(&variants, first)
		}
		original := 0
		if item.has_variant {
			for v, i in variants {
				if id, ok := json_get(v, "id").(string); ok && id == item.variant {
					original = i
					break
				}
			}
		}
		taken := make(map[string]bool, ta)
		for v in variants {
			if id, ok := json_get(v, "id").(string); ok {
				taken[id] = true
			}
		}
		id := next_id(lib)
		for taken[id] {
			id = next_id(lib)
		}
		copy_fields := make(Json_Object, 0, 4, ta)
		object_set(&copy_fields, "id", json_string(id, ta))
		// Edits, rating and flag, as Lightroom's virtual copies.
		if o, ok := variants[original].(Json_Object); ok {
			for key in ([]string{"edits", "rating", "flag"}) {
				if value, found := object_find(o, key); found {
					object_set(&copy_fields, key, json_clone(value, ta))
				}
			}
		}
		append(&variants, copy_fields)
		object_set(&top, "variants", variants)
		object_set(&top, "app", json_string(lib.app, ta))
		patch := patch_new(id, LATEST_DEFAULTS, lib.app)
		patch.replace = true
		patch.replacement = top
		if reason, ok := write(lib, photo, &patch); ok {
			append(&made, item_duplicate(photo, id))
		} else {
			failed_write(lib, photo, reason, &outcome)
		}
	}
	finish(lib, &recorder, "Duplicate Photographs" if len(made) > 1 else "Duplicate", .Sidecars)
	outcome.has_note = true
	outcome.note = Note{"Duplicated" if len(made) > 0 else "Not Duplicated", has_unsaved, unsaved, max(len(made), 1)}
	outcome.select = made[:]
	return outcome
}
