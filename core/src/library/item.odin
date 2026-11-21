// What the film strip and grid show: photographs and their duplicates
// (`Library.swift`: the `#variant=` URLs, `LibrarySummary`; item.rs).
package library

import "core:strings"

// One thing the strip shows: a photograph (no variant: its sidecar's first
// variant) or one of its duplicates (a later variant, by id).
Item :: struct {
	photo:       string,
	has_variant: bool,
	variant:     string,
}

item_photo :: proc(photo: string) -> Item {
	return Item{photo = photo}
}

item_duplicate :: proc(photo, id: string) -> Item {
	return Item{photo, true, id}
}

item_equal :: proc(a, b: Item) -> bool {
	return a.photo == b.photo && a.has_variant == b.has_variant && (!a.has_variant || a.variant == b.variant)
}

// A string that names the item, for maps: the photograph's path, and the
// variant after a NUL.
item_key :: proc(i: Item, allocator := context.temp_allocator) -> string {
	if !i.has_variant {
		return strings.clone(i.photo, allocator)
	}
	return strings.concatenate({i.photo, "\x00", i.variant}, allocator)
}

item_from_key :: proc(key: string) -> Item {
	if at := strings.index_byte(key, 0); at >= 0 {
		return item_duplicate(key[:at], key[at + 1:])
	}
	return item_photo(key)
}

item_clone :: proc(i: Item, allocator := context.allocator) -> Item {
	return Item{strings.clone(i.photo, allocator), i.has_variant, strings.clone(i.variant, allocator)}
}

item_destroy :: proc(i: Item, allocator := context.allocator) {
	delete(i.photo, allocator)
	delete(i.variant, allocator)
}

// What the strip shows of an item (`LibrarySummary`).
Summary :: struct {
	rating:      u8,
	flag:        Flag,
	edited:      bool,
	// Why its sidecar isn't used: the message to show.
	has_problem: bool,
	problem:     string,
}

summary_equal :: proc(a, b: Summary) -> bool {
	return(
		a.rating == b.rating &&
		a.flag == b.flag &&
		a.edited == b.edited &&
		a.has_problem == b.has_problem &&
		(!a.has_problem || a.problem == b.problem) \
	)
}

summary_clone :: proc(s: Summary, allocator := context.allocator) -> Summary {
	out := s
	out.problem = strings.clone(s.problem, allocator)
	return out
}

summary_destroy :: proc(s: Summary, allocator := context.allocator) {
	delete(s.problem, allocator)
}

summary_of :: proc(v: Variant) -> Summary {
	return Summary{rating = v.rating, flag = v.flag, edited = v.edits != nil}
}

Item_Read :: struct {
	item:     Item,
	summary:  Summary,
	has_look: bool,
	look:     string,
}

// Each library item of `photo` as its sidecar holds them, with their
// summaries and looks (temporary).
items_of :: proc(photo: string, sidecar: Sidecar, has_sidecar: bool) -> []Item_Read {
	ta := context.temp_allocator
	if !has_sidecar || len(sidecar.variants) == 0 {
		out := make([]Item_Read, 1, ta)
		out[0] = Item_Read{item = item_photo(photo)}
		return out
	}
	out := make([]Item_Read, len(sidecar.variants), ta)
	for v, index in sidecar.variants {
		item := item_photo(photo) if index == 0 else item_duplicate(photo, v.id)
		r := Item_Read{item = item, summary = summary_of(v)}
		if v.edits != nil {
			r.has_look = true
			r.look = look(v.edits, sidecar.defaults_version, ta)
		}
		out[index] = r
	}
	return out
}

// "No Rating", or the stars (`EditorModel.ratingText`).
rating_text :: proc(stars: u8, allocator := context.allocator) -> string {
	if stars == 0 {
		return strings.clone("No Rating", allocator)
	}
	return strings.repeat("★", int(stars), allocator)
}

// "Pick", "Rejected" or "Unflagged" (`EditorModel.flagText`).
flag_text :: proc(flag: Flag) -> string {
	switch flag {
	case .Pick:
		return "Pick"
	case .Reject:
		return "Rejected"
	case .None:
	}
	return "Unflagged"
}
