// Undo and Redo of library actions (library.rs `undo`, `apply_inverse`,
// `put_back`). Undone at once, an action leaves every file byte for byte
// as it was.
package library

import "core:fmt"

// Undoes the last library action; false when there's none.
library_undo :: proc(lib: ^Library) -> (Outcome, bool) {
	return undo_from(lib, &lib.undo, &lib.redo)
}

library_redo :: proc(lib: ^Library) -> (Outcome, bool) {
	return undo_from(lib, &lib.redo, &lib.undo)
}

@(private = "file")
undo_from :: proc(lib: ^Library, from, to: ^[dynamic]Step) -> (Outcome, bool) {
	if len(from) == 0 {
		return {}, false
	}
	kept := pop(from)
	step := step_clone(kept, context.temp_allocator)
	step_destroy(kept, lib.allocator)
	inverse, outcome := apply_inverse(lib, step)
	append(to, step_clone(inverse, lib.allocator))
	return outcome, true
}

// Puts `step` back and returns the step that does it again.
@(private = "file")
apply_inverse :: proc(lib: ^Library, step: Step) -> (Step, Outcome) {
	ta := context.temp_allocator
	outcome := Outcome{}
	library_save(lib)
	switch step.kind {
	case .Summaries:
		recorder := recorder_make()
		restored := make(map[string]bool, ta)
		// A file still as the step left it goes back byte for byte.
		for change in step.files {
			bytes, has := current_bytes(change.photo)
			if bytes_equal(has, bytes, change.has_after, change.after) {
				recorder_touch(&recorder, change.photo)
				if sidecar_write_bytes(change.photo, change.before, !change.has_before) == 0 {
					restored[change.photo] = true
				}
			}
		}
		items := make(map[string]bool, ta)
		for i in library_items(lib) {
			items[item_key(i, ta)] = true
		}
		befores := make([dynamic]Summary_Change, 0, len(step.summaries), ta)
		for target in step.summaries {
			item := target.item
			if item_key(item, ta) not_in items {
				continue
			}
			before := summary_clone(library_summary(lib, item), ta)
			after := before
			if target.was.rating != target.became.rating {
				after.rating = target.was.rating
			}
			if target.was.flag != target.became.flag {
				after.flag = target.was.flag
			}
			append(&befores, Summary_Change{item, before, after})
			if item.photo in restored || summary_equal(after, before) {
				continue
			}
			// Changed elsewhere meanwhile: merged with the file as it is.
			set_summary(lib, item, after)
			recorder_touch(&recorder, item.photo)
			save_summary(lib, before, after, item, &outcome)
		}
		for photo in recorder_photos(&recorder) {
			reload_keeping_unsaved(lib, photo)
		}
		files := recorder_finish(&recorder)
		return Step{action = step.action, kind = .Summaries, summaries = befores[:], files = files}, outcome
	case .Sidecars:
		files := put_back(lib, step.files, &outcome)
		return Step{action = step.action, kind = .Sidecars, files = files}, outcome
	case .Delete:
		for i := len(step.moves) - 1; i >= 0; i -= 1 {
			m := step.moves[i]
			if error, ok := lib.bin.put_back(lib.bin.user, m.trashed, m.original); !ok {
				outcome_error(&outcome, fmt.aprintf("“%s” couldn’t be put back. %s", file_name(m.original), error, allocator = ta))
			}
		}
		put_back(lib, step.files, &outcome)
		library_refresh(lib, false)
		restored := make([dynamic]Item, 0, len(step.plan.items), ta)
		for i in step.plan.items {
			if fs_exists(i.photo) {
				append(&restored, i)
			}
		}
		if step.has_reopen && fs_exists(step.reopen.photo) {
			outcome.has_open = true
			outcome.open = step.reopen
		}
		outcome.select = restored[:]
		return Step{action = step.action, kind = .Redelete, plan = step.plan}, outcome
	case .Redelete:
		permanent := library_delete_is_permanent(lib, step.plan)
		again, has_again, done := delete_now(lib, step.plan, permanent)
		if !has_again {
			again = Step{action = step.action, kind = .Sidecars}
		}
		return again, done
	}
	return step, outcome
}

// Writes each file back as it was before `changes` (`restoreSidecars`):
// the open photograph shows its edits as restored, or its photograph opens
// if it was a duplicate that's gone. Returns the changes that would do them
// again (temporary).
@(private = "file")
put_back :: proc(lib: ^Library, changes: []File_Change, outcome: ^Outcome) -> []File_Change {
	ta := context.temp_allocator
	recorder := recorder_make()
	for change in changes {
		bytes, has := current_bytes(change.photo)
		if has && !bytes_equal(has, bytes, change.has_after, change.after) {
			if _, bad := sidecar_from_bytes(bytes); bad != nil {
				outcome_error(outcome, fmt.aprintf("The .negs file of %s changed and is left as it is.", file_name(change.photo), allocator = ta))
				continue
			}
		}
		recorder_touch(&recorder, change.photo)
		if err := sidecar_write_bytes(change.photo, change.before, !change.has_before); err != 0 {
			outcome_error(outcome, fmt.aprintf("Edits to “%s” couldn’t be put back: %s", file_name(change.photo), io_error_text(err, ta), allocator = ta))
		}
		reload(lib, change.photo)
	}
	if lib.has_open {
		open := open_clone(lib.open, ta)
		if recorder_has(&recorder, open.item.photo) {
			duplicate_gone := false
			if open.item.has_variant {
				duplicate_gone = true
				for id in library_duplicates(lib, open.item.photo) {
					if id == open.item.variant {
						duplicate_gone = false
						break
					}
				}
			}
			if duplicate_gone {
				close_open(lib)
				outcome.open_gone = true
				outcome.has_open = true
				outcome.open = item_photo(open.item.photo)
			} else {
				r := restore(lib, open.item.photo, open.item)
				adopt(lib, open.item, r, open.state)
				outcome.open_changed = true
			}
		}
	}
	return recorder_finish(&recorder)
}
