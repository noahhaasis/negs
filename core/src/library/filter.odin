// Which photographs the strip and grid show, and the words for it
// (`LibraryFilter`, `SheetWords`, `FilterMenu`; filter.rs).
package library

import "core:fmt"
import "core:strings"

Rating_Filter_Kind :: enum u8 {
	Any,
	// At least `stars`, 1 to 5.
	At_Least,
	Unrated,
}

Rating_Filter :: struct {
	kind:  Rating_Filter_Kind,
	stars: u8,
}

// In the menu's order.
Flag_Filter :: enum u8 {
	Pick,
	Unflagged,
	Reject,
}

Flag_Filters :: bit_set[Flag_Filter]

Edits_Filter :: enum u8 {
	Any,
	Edited,
	Unedited,
}

// Remembered for the app, not per folder.
Filter :: struct {
	rating: Rating_Filter,
	// The flags shown; empty shows every one.
	flags:  Flag_Filters,
	edits:  Edits_Filter,
}

flag_filter_title :: proc(f: Flag_Filter) -> string {
	switch f {
	case .Pick:
		return "Picks"
	case .Unflagged:
		return "Unflagged"
	case .Reject:
		return "Rejected"
	}
	return ""
}

flag_filter_key :: proc(f: Flag_Filter) -> string {
	switch f {
	case .Pick:
		return "pick"
	case .Unflagged:
		return "unflagged"
	case .Reject:
		return "reject"
	}
	return ""
}

filter_equal :: proc(a, b: Filter) -> bool {
	return a.rating.kind == b.rating.kind && (a.rating.kind != .At_Least || a.rating.stars == b.rating.stars) && a.flags == b.flags && a.edits == b.edits
}

filter_active :: proc(f: Filter) -> bool {
	return !filter_equal(f, Filter{})
}

filter_includes :: proc(f: Filter, s: Summary) -> bool {
	switch f.rating.kind {
	case .Any:
	case .At_Least:
		if s.rating < f.rating.stars {
			return false
		}
	case .Unrated:
		if s.rating != 0 {
			return false
		}
	}
	if f.flags != {} {
		flag: Flag_Filter
		switch s.flag {
		case .Pick:
			flag = .Pick
		case .Reject:
			flag = .Reject
		case .None:
			flag = .Unflagged
		}
		if flag not_in f.flags {
			return false
		}
	}
	switch f.edits {
	case .Any:
		return true
	case .Edited:
		return s.edited
	case .Unedited:
		return !s.edited
	}
	return true
}

// The menu's flag items turn on and off.
filter_toggle_flag :: proc(f: ^Filter, flag: Flag_Filter) {
	f.flags ~= {flag}
}

// Edited and Unedited exclude each other; choosing the one that's on turns
// it off.
filter_toggle_edits :: proc(f: ^Filter, choice: Edits_Filter) {
	f.edits = .Any if f.edits == choice else choice
}

// A part of the sheet's sentence: words, then an underlined one that opens
// the filter menu.
Filter_Word :: struct {
	lead: string,
	word: string,
}

// What the filter lets through, as the sheet's sentence says it.
filter_words :: proc(f: Filter, allocator := context.allocator) -> []Filter_Word {
	parts := make([dynamic]Filter_Word, 0, 3, allocator)
	switch f.rating.kind {
	case .Any:
	case .At_Least:
		if f.rating.stars == 5 {
			append(&parts, Filter_Word{strings.clone("rated ", allocator), strings.clone("★★★★★", allocator)})
		} else {
			stars := strings.repeat("★", int(f.rating.stars), context.temp_allocator)
			append(&parts, Filter_Word{strings.clone("rated ", allocator), strings.concatenate({stars, " or more"}, allocator)})
		}
	case .Unrated:
		append(&parts, Filter_Word{strings.clone("", allocator), strings.clone("unrated", allocator)})
	}
	flags := make([dynamic]string, 0, 3, context.temp_allocator)
	for flag in Flag_Filter {
		if flag in f.flags {
			append(&flags, strings.to_lower(flag_filter_title(flag), context.temp_allocator))
		}
	}
	if len(flags) > 0 {
		append(&parts, Filter_Word{strings.clone("", allocator), strings.join(flags[:], " or ", allocator)})
	}
	switch f.edits {
	case .Any:
	case .Edited:
		append(&parts, Filter_Word{strings.clone("", allocator), strings.clone("edited", allocator)})
	case .Unedited:
		append(&parts, Filter_Word{strings.clone("", allocator), strings.clone("unedited", allocator)})
	}
	return parts[:]
}

Rating_Choice :: struct {
	rating: Rating_Filter,
	title:  string,
}

// The rating items of the filter menu, in order, with what each sets.
rating_choices :: proc(allocator := context.allocator) -> []Rating_Choice {
	out := make([]Rating_Choice, 7, allocator)
	out[0] = {{.Any, 0}, strings.clone("Any Rating", allocator)}
	for stars in 1 ..= 5 {
		title: string
		if stars == 5 {
			title = strings.clone("★★★★★", allocator)
		} else {
			title = strings.concatenate({"≥ ", strings.repeat("★", stars, context.temp_allocator)}, allocator)
		}
		out[stars] = {{.At_Least, u8(stars)}, title}
	}
	out[6] = {{.Unrated, 0}, strings.clone("Unrated Only", allocator)}
	return out
}

// As macOS keeps it in its settings (`LibraryFilter`'s Codable form).
filter_to_json :: proc(f: Filter, allocator := context.allocator) -> Json {
	rating := make(Json_Object, 0, 1, allocator)
	switch f.rating.kind {
	case .Any:
		object_set(&rating, "any", json_object(allocator))
	case .At_Least:
		inner := make(Json_Object, 0, 1, allocator)
		object_set(&inner, "_0", f64(f.rating.stars))
		object_set(&rating, "atLeast", inner)
	case .Unrated:
		object_set(&rating, "unrated", json_object(allocator))
	}
	flags := make(Json_Array, 0, 3, allocator)
	for flag in Flag_Filter {
		if flag in f.flags {
			append(&flags, json_string(flag_filter_key(flag), allocator))
		}
	}
	edits := "any"
	switch f.edits {
	case .Any:
	case .Edited:
		edits = "edited"
	case .Unedited:
		edits = "unedited"
	}
	out := make(Json_Object, 0, 3, allocator)
	object_set(&out, "rating", rating)
	object_set(&out, "flags", flags)
	object_set(&out, "edits", json_string(edits, allocator))
	return out
}

// Reads `filter_to_json`'s form, and filters saved before Unedited, which
// had only `editedOnly`. Anything unreadable is no filter.
filter_from_json :: proc(json: Json) -> Filter {
	f := Filter{}
	rating := json_get(json, "rating")
	if o, ok := rating.(Json_Object); ok && object_has(o, "unrated") {
		f.rating = {.Unrated, 0}
	} else if n, has := as_integer(json_at(rating, "atLeast._0")); has && n >= 1 && n <= 5 {
		f.rating = {.At_Least, u8(n)}
	}
	if flags, ok := json_get(json, "flags").(Json_Array); ok {
		for flag in flags {
			for choice in Flag_Filter {
				if s, is_string := flag.(string); is_string && s == flag_filter_key(choice) {
					f.flags += {choice}
				}
			}
		}
	}
	if e, ok := json_get(json, "edits").(string); ok {
		switch e {
		case "edited":
			f.edits = .Edited
		case "unedited":
			f.edits = .Unedited
		}
	} else if b, is_bool := json_get(json, "editedOnly").(bool); is_bool && b {
		f.edits = .Edited
	}
	return f
}

// "1 photograph", "12 photographs".
photographs :: proc(count: int, allocator := context.allocator) -> string {
	if count == 1 {
		return strings.clone("1 photograph", allocator)
	}
	return fmt.aprintf("%d photographs", count, allocator = allocator)
}

// The grid foot's count.
count_words :: proc(shown, total, selected: int, filtered: bool, allocator := context.allocator) -> string {
	ta := context.temp_allocator
	if filtered {
		return fmt.aprintf("%d of %s", shown, photographs(total, ta), allocator = allocator)
	}
	if selected > 0 {
		return fmt.aprintf("%s · %d selected", photographs(total, ta), selected, allocator = allocator)
	}
	return photographs(total, allocator)
}
