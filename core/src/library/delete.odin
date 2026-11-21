// What a delete does, and the question before it (`DeletePlan` in
// `Delete.swift`; delete.rs). A photograph goes to the bin with its
// sidecar, and a RAW with its camera JPEG, only with all its duplicates.
// Otherwise what is chosen leaves the sidecar and the file stays.
package library

import "core:fmt"
import "core:strings"

Companion :: struct {
	file:    string,
	picture: string,
}

Rename :: struct {
	from: Item,
	to:   Item,
}

Delete_Plan :: struct {
	// Everything chosen, in order: what Undo selects again.
	items:      []Item,
	// Photographs whose files go.
	files:      []string,
	// What leaves the sidecar of a photograph that stays.
	duplicates: []Item,
	// The duplicates that go with `files`.
	following:  int,
	// The camera JPEGs that go with `files`.
	companions: []Companion,
	// Each duplicate that becomes its photograph, its photograph removed.
	renamed:    []Rename,
}

// The plan for `chosen` (temporary). `duplicates` gives a photograph's
// duplicates by variant id, `companions` its camera JPEG.
delete_plan_new :: proc(chosen_in: []Item, duplicates: map[string][]string, companions: map[string]string) -> Delete_Plan {
	ta := context.temp_allocator
	seen := make(map[string]bool, ta)
	chosen := make([dynamic]Item, 0, len(chosen_in), ta)
	for i in chosen_in {
		key := item_key(i)
		if key not_in seen {
			seen[key] = true
			append(&chosen, i)
		}
	}
	dups_of :: proc(duplicates: map[string][]string, photo: string) -> []string {
		if d, ok := duplicates[photo]; ok {
			return d
		}
		return nil
	}
	files := make([dynamic]string, 0, len(chosen), ta)
	for i in chosen {
		if i.has_variant {
			continue
		}
		all := true
		for id in dups_of(duplicates, i.photo) {
			if item_key(item_duplicate(i.photo, id)) not_in seen {
				all = false
				break
			}
		}
		if all {
			append(&files, i.photo)
		}
	}
	trashed := make(map[string]bool, ta)
	for f in files {
		trashed[f] = true
	}
	removed := make([dynamic]Item, 0, len(chosen), ta)
	items := make([dynamic]Item, 0, len(chosen), ta)
	for i in chosen {
		if i.photo not_in trashed {
			append(&removed, i)
		}
		if !i.has_variant || i.photo not_in trashed {
			append(&items, i)
		}
	}
	following := 0
	for f in files {
		following += len(dups_of(duplicates, f))
	}
	comps := make([dynamic]Companion, 0, len(files), ta)
	for f in files {
		if p, ok := companions[f]; ok {
			append(&comps, Companion{f, p})
		}
	}
	renamed := make([dynamic]Rename, 0, 2, ta)
	for photo in removed {
		if photo.has_variant {
			continue
		}
		for id in dups_of(duplicates, photo.photo) {
			if item_key(item_duplicate(photo.photo, id)) not_in seen {
				append(&renamed, Rename{item_duplicate(photo.photo, id), photo})
				break
			}
		}
	}
	return Delete_Plan{items[:], files[:], removed[:], following, comps[:], renamed[:]}
}

// What `item` is called once the plan is done.
delete_plan_after :: proc(p: Delete_Plan, item: Item) -> Item {
	for r in p.renamed {
		if item_equal(r.from, item) {
			return r.to
		}
	}
	return item
}

@(private = "file")
kept_photographs :: proc(p: Delete_Plan) -> int {
	n := 0
	for i in p.duplicates {
		if !i.has_variant {
			n += 1
		}
	}
	return n
}

delete_plan_is_empty :: proc(p: Delete_Plan) -> bool {
	return len(p.items) == 0
}

delete_plan_companion :: proc(p: Delete_Plan, file: string) -> (string, bool) {
	for c in p.companions {
		if c.file == file {
			return c.picture, true
		}
	}
	return "", false
}

// The same plan without its files: after a permanent delete, only the
// duplicates can come back and go again.
delete_plan_without_files :: proc(p: Delete_Plan) -> Delete_Plan {
	return Delete_Plan{items = p.duplicates, duplicates = p.duplicates, renamed = p.renamed}
}

// A deep copy, for an undo step.
delete_plan_clone :: proc(p: Delete_Plan, allocator := context.allocator) -> Delete_Plan {
	items_clone :: proc(items: []Item, allocator: Allocator) -> []Item {
		out := make([]Item, len(items), allocator)
		for i, k in items {
			out[k] = item_clone(i, allocator)
		}
		return out
	}
	out := Delete_Plan {
		items      = items_clone(p.items, allocator),
		duplicates = items_clone(p.duplicates, allocator),
		following  = p.following,
	}
	out.files = make([]string, len(p.files), allocator)
	for f, k in p.files {
		out.files[k] = strings.clone(f, allocator)
	}
	out.companions = make([]Companion, len(p.companions), allocator)
	for c, k in p.companions {
		out.companions[k] = Companion{strings.clone(c.file, allocator), strings.clone(c.picture, allocator)}
	}
	out.renamed = make([]Rename, len(p.renamed), allocator)
	for r, k in p.renamed {
		out.renamed[k] = Rename{item_clone(r.from, allocator), item_clone(r.to, allocator)}
	}
	return out
}

delete_plan_destroy :: proc(p: Delete_Plan, allocator := context.allocator) {
	for i in p.items {
		item_destroy(i, allocator)
	}
	delete(p.items, allocator)
	for i in p.duplicates {
		item_destroy(i, allocator)
	}
	delete(p.duplicates, allocator)
	for f in p.files {
		delete(f, allocator)
	}
	delete(p.files, allocator)
	for c in p.companions {
		delete(c.file, allocator)
		delete(c.picture, allocator)
	}
	delete(p.companions, allocator)
	for r in p.renamed {
		item_destroy(r.from, allocator)
		item_destroy(r.to, allocator)
	}
	delete(p.renamed, allocator)
}

// The Undo menu's name for it. `bin` is what the platform calls it.
delete_plan_action_name :: proc(p: Delete_Plan, bin: string, allocator := context.allocator) -> string {
	if len(p.files) > 0 {
		return strings.concatenate({"Move to ", bin}, allocator)
	}
	if kept_photographs(p) > 0 {
		return strings.clone("Remove Photographs" if len(p.duplicates) > 1 else "Remove Photograph", allocator)
	}
	return strings.clone("Remove Duplicates" if len(p.duplicates) > 1 else "Remove Duplicate", allocator)
}

// The files that go for a photograph: itself, its sidecar, and its camera
// JPEG with that one's sidecar, those that exist (temporary).
delete_plan_files_of :: proc(p: Delete_Plan, photo: string) -> []string {
	ta := context.temp_allocator
	out := make([dynamic]string, 0, 4, ta)
	append(&out, photo)
	sidecar := sidecar_path(photo, ta)
	if fs_exists(sidecar) {
		append(&out, sidecar)
	}
	if picture, ok := delete_plan_companion(p, photo); ok {
		for file in ([]string{picture, sidecar_path(picture, ta)}) {
			if fs_exists(file) {
				append(&out, file)
			}
		}
	}
	return out[:]
}

// The alert before a delete.
Question :: struct {
	message:     string,
	information: string,
	button:      string,
}

Namer :: struct {
	name: proc(user: rawptr, item: Item) -> string,
	user: rawptr,
}

// The alert before it, as macOS words it (temporary). A single photograph
// is named rather than counted. `bin` is what the platform calls it.
delete_plan_question :: proc(p: Delete_Plan, namer: Namer, permanent: bool, bin: string) -> Question {
	ta := context.temp_allocator
	name :: proc(namer: Namer, item: Item) -> string {
		return namer.name(namer.user, item)
	}
	count :: proc(n: int, noun: string) -> string {
		if n == 1 {
			return fmt.aprintf("1 %s", noun, allocator = context.temp_allocator)
		}
		return fmt.aprintf("%d %ss", n, noun, allocator = context.temp_allocator)
	}
	photos := len(p.files)
	copies := len(p.duplicates)
	following := p.following
	with := ""
	if following != 0 {
		if photos == 1 {
			with = ", with its duplicate" if following == 1 else fmt.aprintf(", with its %d duplicates", following, allocator = ta)
		} else {
			with = fmt.aprintf(", with %s", count(following, "duplicate"), allocator = ta)
		}
	}
	files_go: string
	no_companions := len(p.companions) == 0
	switch {
	case no_companions && photos == 1:
		files_go = "the photograph’s file and its Negs rating and edits"
	case no_companions:
		files_go = "the photograph files and their Negs ratings and edits"
	case photos == 1:
		files_go = "the photograph’s file, its camera JPEG and its Negs rating and edits"
	case:
		files_go = "the photograph files, their camera JPEGs and their Negs ratings and edits"
	}
	kept := kept_photographs(p)
	copies_go: string
	switch {
	case kept == 0 && copies == 1:
		copies_go = "The duplicate’s rating and edits are removed from its .negs file; the photograph stays."
	case kept == 0:
		copies_go = "The duplicates’ ratings and edits are removed from their .negs files; the photographs stay."
	case copies == 1:
		if len(p.renamed) > 0 {
			copies_go = fmt.aprintf("Its rating and edits are removed from its .negs file; the file stays, and its duplicate “%s” takes its place.", name(namer, p.renamed[0].from), allocator = ta)
		} else {
			copies_go = "Its rating and edits are removed from its .negs file; the file stays for its duplicates."
		}
	case:
		copies_go = "Their ratings and edits are removed from their .negs files; the files stay, and the first duplicate left takes each photograph’s place."
	}
	removals: string
	switch {
	case kept == 0:
		removals = count(copies, "duplicate")
	case copies - kept == 0:
		removals = count(kept, "photograph")
	case:
		removals = fmt.aprintf("%s and %s", count(kept, "photograph"), count(copies - kept, "duplicate"), allocator = ta)
	}
	if photos == 0 {
		message: string
		switch {
		case copies == 1 && kept == 0:
			message = fmt.aprintf("Remove the duplicate “%s”?", name(namer, p.duplicates[0]), allocator = ta)
		case copies == 1:
			message = fmt.aprintf("Remove “%s”?", name(namer, p.duplicates[0]), allocator = ta)
		case kept == 0:
			message = fmt.aprintf("Remove %d duplicates?", copies, allocator = ta)
		case:
			message = fmt.aprintf("Remove %s?", removals, allocator = ta)
		}
		return Question {
			message = message,
			information = fmt.aprintf("%s You can put %s back with Undo.", copies_go, "it" if copies == 1 else "them", allocator = ta),
			button = "Remove",
		}
	}
	what: string
	if photos == 1 && copies == 0 {
		what = fmt.aprintf("“%s”", name(namer, item_photo(p.files[0])), allocator = ta)
	} else {
		what = count(photos, "photograph")
	}
	and_copies := "" if copies == 0 else fmt.aprintf(" and remove %s", removals, allocator = ta)
	if permanent {
		back := "" if copies == 0 else " The duplicates can be put back with Undo."
		also := "" if copies == 0 else fmt.aprintf(" %s", copies_go, allocator = ta)
		return Question {
			message = fmt.aprintf("Delete %s permanently%s?", what, and_copies, allocator = ta),
			information = fmt.aprintf("This volume has no %s, so %s are deleted at once%s. This can’t be undone.%s%s", bin, files_go, with, also, back, allocator = ta),
			button = "Delete",
		}
	}
	back: string
	if photos == 1 && copies == 0 {
		back = fmt.aprintf("You can put it back with Undo or from the %s.", bin, allocator = ta)
	} else {
		back = fmt.aprintf("You can put them back with Undo or from the %s.", bin, allocator = ta)
	}
	// files_go starts with an ASCII letter.
	first := strings.concatenate({strings.to_upper(files_go[:1], ta), files_go[1:]}, ta)
	also := "" if copies == 0 else fmt.aprintf("%s ", copies_go, allocator = ta)
	return Question {
		message = fmt.aprintf("Move %s to the %s%s?", what, bin, and_copies, allocator = ta),
		information = fmt.aprintf("%s are moved to the %s%s. %s%s", first, bin, with, also, back, allocator = ta),
		button = strings.concatenate({"Move to ", bin}, ta) if copies == 0 else "Delete",
	}
}
