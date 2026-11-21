// The library's listing: the folder listed again, and the sidecars that
// are new, changed or gone since read (library.rs `refresh`, `reload`).
package library

import "core:strings"

// Lists the folder again and reads the sidecars that are new, changed or
// gone since. Returns the photographs whose sidecars another app changed
// (temporary). If the open photograph's is one of them, its edits are
// taken from the file when nothing of the user's is unsaved and they're not
// `interacting`: `open_changed` then.
library_refresh :: proc(lib: ^Library, interacting: bool) -> ([]string, Outcome) {
	ta := context.temp_allocator
	a := lib.allocator
	outcome := Outcome{}
	found, err := listing(lib.folder, lib.pairing)
	if err != 0 {
		set_failed(lib, true, io_error_text(err, ta))
		return nil, outcome
	}
	set_failed(lib, false, "")
	set_files(lib, found.photos[:])
	for k, v in lib.companions {
		delete(k, a)
		delete(v, a)
	}
	clear(&lib.companions)
	for raw, picture in found.companions {
		lib.companions[strings.clone(raw, a)] = strings.clone(picture, a)
	}
	lib.read_only = !can_write(lib.folder)
	present := make(map[string]bool, ta)
	for f in lib.files {
		present[f.path] = true
	}
	{
		gone := make([dynamic]string, 0, 4, ta)
		for k in lib.sidecar_files {
			if k not_in present {
				append(&gone, k)
			}
		}
		for k in gone {
			key, _ := delete_key(&lib.sidecar_files, k)
			delete(key, a)
		}
	}
	{
		gone := make([dynamic]string, 0, 4, ta)
		for k in lib.duplicates {
			if k not_in present {
				append(&gone, k)
			}
		}
		for k in gone {
			key, ids := delete_key(&lib.duplicates, k)
			delete(key, a)
			strings_destroy(ids, a)
		}
	}
	{
		gone := make([dynamic]Item, 0, 4, ta)
		for k in lib.summaries {
			item := item_from_key(k)
			if item.photo not_in present {
				append(&gone, item_clone(item, ta))
			}
		}
		for item in gone {
			remove_summary(lib, item)
		}
	}
	{
		gone := make([dynamic]Item, 0, 4, ta)
		for k in lib.looks {
			item := item_from_key(k)
			if item.photo not_in present {
				append(&gone, item_clone(item, ta))
			}
		}
		for item in gone {
			remove_look(lib, item)
		}
	}
	if !lib.sidecars {
		return nil, outcome
	}
	changed := make([dynamic]string, 0, 4, ta)
	files := make([]Photo_File, len(lib.files), ta)
	for f, i in lib.files {
		files[i] = photo_file_clone(f, ta)
	}
	for file in files {
		now := Stamp{}
		if s, ok := found.sidecars[file.name]; ok {
			now = Stamp{true, s.modified, s.size}
		}
		known, has_known := lib.sidecar_files[file.path]
		if !has_known || known != now {
			if has_known {
				append(&changed, file.path)
			}
			reload(lib, file.path)
		}
	}
	if lib.has_open {
		for c in changed {
			if c == lib.open.item.photo {
				outcome = sidecar_changed_on_disk(lib, interacting)
				break
			}
		}
	}
	return changed[:], outcome
}

// Reads `photo`'s sidecar again: its items' summaries, looks and
// duplicates.
@(private)
reload :: proc(lib: ^Library, photo_in: string) {
	if !lib.sidecars {
		return
	}
	ta := context.temp_allocator
	a := lib.allocator
	photo := strings.clone(photo_in, ta)
	// What this read saw, so the next listing tells another app's changes
	// from the library's own.
	stamp := Stamp{}
	if st, err := fs_stat(sidecar_path(photo, ta)); err == 0 {
		stamp = Stamp{true, st.modified, u64(st.size)}
	}
	if photo in lib.sidecar_files {
		lib.sidecar_files[photo] = stamp
	} else {
		lib.sidecar_files[strings.clone(photo, a)] = stamp
	}
	read: []Item_Read
	s, found, problem := sidecar_read(photo)
	if p, bad := problem.?; bad {
		read = make([]Item_Read, 1, ta)
		read[0] = Item_Read {
			item = item_photo(photo),
			summary = Summary{has_problem = true, problem = problem_message(p, file_name(photo), ta)},
		}
	} else {
		read = items_of(photo, s, found)
	}
	ids := make([dynamic]string, 0, len(read), ta)
	for r in read {
		if r.item.has_variant {
			append(&ids, r.item.variant)
		}
	}
	if old, ok := lib.duplicates[photo]; ok {
		previous := strings_clone(old, ta)
		for gone in previous {
			kept := false
			for id in ids {
				if id == gone {
					kept = true
					break
				}
			}
			if !kept {
				item := item_duplicate(photo, gone)
				remove_summary(lib, item)
				remove_look(lib, item)
			}
		}
	}
	if old, ok := lib.duplicates[photo]; ok {
		strings_destroy(old, a)
		if len(ids) == 0 {
			key, _ := delete_key(&lib.duplicates, photo)
			delete(key, a)
		} else {
			lib.duplicates[photo] = strings_clone(ids[:], a)
		}
	} else if len(ids) > 0 {
		lib.duplicates[strings.clone(photo, a)] = strings_clone(ids[:], a)
	}
	for r in read {
		set_summary(lib, r.item, r.summary)
		if r.has_look {
			set_look(lib, r.item, r.look)
		} else {
			remove_look(lib, r.item)
		}
	}
}
