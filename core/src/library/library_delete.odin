// Deleting photographs and duplicates (library.rs `delete`, `delete_now`).
package library

import "core:fmt"
import fs "../platform/fs"

// What deleting `items` does (temporary).
library_delete_plan :: proc(lib: ^Library, items: []Item) -> Delete_Plan {
	return delete_plan_clone(delete_plan_new(items, lib.duplicates, lib.companions), context.temp_allocator)
}

@(private = "file")
name_of :: proc(user: rawptr, item: Item) -> string {
	return library_name((^Library)(user), item)
}

// The alert before deleting `plan` (temporary).
library_delete_question :: proc(lib: ^Library, plan: Delete_Plan) -> Question {
	return delete_plan_question(plan, Namer{name_of, lib}, library_delete_is_permanent(lib, plan), bin_name(lib.bin))
}

// Whether a delete of `plan` would be for good: its folder's volume has no
// bin.
library_delete_is_permanent :: proc(lib: ^Library, plan: Delete_Plan) -> bool {
	return len(plan.files) > 0 && !lib.bin.available(lib.bin.user, lib.folder)
}

// Moves `plan`'s photographs to the bin with their sidecars (and camera
// JPEGs), and removes the rest from theirs, as one undo step.
library_delete :: proc(lib: ^Library, plan: Delete_Plan) -> Outcome {
	ta := context.temp_allocator
	outcome := Outcome{}
	if delete_plan_is_empty(plan) {
		return outcome
	}
	if read_only_photo(join(lib.folder, "x", ta)) {
		outcome_error(&outcome, fmt.aprintf("Nothing was deleted: the folder “%s” is read-only.", file_name(lib.folder), allocator = ta))
		return outcome
	}
	permanent := library_delete_is_permanent(lib, plan)
	step, has_step, done := delete_now(lib, delete_plan_clone(plan, ta), permanent)
	if has_step {
		push_step(lib, step)
	}
	return done
}

@(private)
delete_now :: proc(lib: ^Library, plan: Delete_Plan, permanent: bool) -> (Step, bool, Outcome) {
	ta := context.temp_allocator
	outcome := Outcome{}
	// The open photograph's last edits go to its sidecar first.
	library_save(lib)
	gone := make(map[string]bool, ta)
	for i in plan.items {
		gone[item_key(i, ta)] = true
	}
	for file in plan.files {
		for id in strings_clone(library_duplicates(lib, file), ta) {
			gone[item_key(item_duplicate(file, id), ta)] = true
		}
	}
	// The open one goes, or is renamed as its photograph's place is taken.
	has_open := lib.has_open
	open := item_clone(lib.open.item, ta) if has_open else Item{}
	has_renamed := false
	renamed: Item
	if has_open {
		after := delete_plan_after(plan, open)
		if !item_equal(after, open) {
			has_renamed, renamed = true, after
		}
	}
	reopen := has_open && (item_key(open, ta) in gone || has_renamed)
	if reopen {
		close_open(lib)
		outcome.open_gone = true
		outcome.has_open = has_renamed
		outcome.open = renamed
	}
	// What is chosen of a photograph that stays leaves its sidecar.
	recorder := recorder_make()
	if lib.sidecars {
		for item in plan.duplicates {
			raw, problem := sidecar_read_raw(item.photo)
			if problem != nil {
				outcome_error(&outcome, fmt.aprintf("“%s” couldn’t be removed: its .negs file can’t be changed.", library_name(lib, item), allocator = ta))
				continue
			}
			if raw == nil {
				continue
			}
			id := item.variant
			if !item.has_variant {
				first, ok := json_at(raw, "variants.0.id").(string)
				if _, is_array := json_get(raw, "variants").(Json_Array); !is_array || !ok {
					continue
				}
				id = first
			}
			removed, bad := removing_variant(id, raw)
			if bad != nil {
				outcome_error(&outcome, fmt.aprintf("“%s” couldn’t be removed: its .negs file can’t be changed.", library_name(lib, item), allocator = ta))
				continue
			}
			recorder_touch(&recorder, item.photo)
			if err := sidecar_write_raw(item.photo, removed); err != 0 {
				outcome_error(&outcome, fmt.aprintf("“%s” couldn’t be removed: %s", library_name(lib, item), fs.error_text(err, ta), allocator = ta))
			}
		}
		for photo in recorder_photos(&recorder) {
			reload(lib, photo)
		}
	}
	moves := make([dynamic]Move, 0, len(plan.files), ta)
	for file in plan.files {
		for path in delete_plan_files_of(plan, file) {
			error: string
			ok: bool
			if permanent {
				error, ok = bin_remove(lib.bin, path)
			} else {
				landed: string
				landed, error, ok = lib.bin.trash(lib.bin.user, path)
				if ok {
					append(&moves, Move{path, landed})
				}
			}
			if !ok {
				outcome_error(&outcome, fmt.aprintf("“%s” couldn’t be deleted. %s", file_name(file), error, allocator = ta))
				break
			}
		}
	}
	files := recorder_finish(&recorder)
	library_refresh(lib, false)
	kept := delete_plan_without_files(plan) if permanent else plan
	if permanent && len(files) == 0 {
		return {}, false, outcome
	}
	step := Step {
		action     = delete_plan_action_name(plan, bin_name(lib.bin), ta),
		kind       = .Delete,
		plan       = kept,
		moves      = moves[:],
		has_reopen = reopen,
		reopen     = open if reopen else Item{},
		files      = files,
	}
	return step, true, outcome
}
