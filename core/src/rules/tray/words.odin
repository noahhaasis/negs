// The tray's words: tooltips, what a screen reader says, the foot's hint
// and count. Static ones are cstrings (they live as long as the process);
// the rest are made in the allocator given.
package tray

import "core:fmt"
import "core:strings"

// Rust's char::is_whitespace: Unicode's White_Space.
is_white_space :: proc "contextless" (r: rune) -> bool {
	switch r {
	case '\t', '\n', '\v', '\f', '\r', ' ', 0x85, 0xA0, 0x1680, 0x2000 ..= 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return false
}

// Rust's str::trim.
trim :: proc(s: string) -> string {
	return strings.trim_right_proc(strings.trim_left_proc(s, is_white_space_c), is_white_space_c)
}

@(private = "file")
is_white_space_c :: proc(r: rune) -> bool {
	return is_white_space(r)
}

// Something is typed in Search (spaces alone don't count).
searching :: proc "contextless" (query: string) -> bool {
	for r in query {
		if !is_white_space(r) {
			return true
		}
	}
	return false
}

// The star's tooltip.
star_help :: proc "contextless" (favourite: bool) -> cstring {
	return "Remove from favourites" if favourite else "Add to favourites"
}

// What a screen reader says for a tile's star (`starLabel`).
star_label :: proc(name: string, favourite: bool, allocator := context.allocator) -> string {
	if favourite {
		return strings.concatenate({"Remove ", name, " from favourites"}, allocator)
	}
	return strings.concatenate({"Add ", name, " to favourites"}, allocator)
}

// A tile's accessible value: the one tried, or the one the tray opened on.
tile_value :: proc "contextless" (selected, before: bool) -> cstring {
	if selected && !before {
		return "Trying"
	}
	if !selected && before {
		return "Before"
	}
	return ""
}

// The foot's group names, as a screen reader says them.
index_label :: proc(title: string, allocator := context.allocator) -> string {
	return strings.concatenate({"Show ", title}, allocator)
}

// The foot's hint: how to try before anything is tried, how to keep or go
// back after (`before`: the name of the stock the tray opened on).
hint :: proc(before: string, has_before: bool, allocator := context.allocator) -> string {
	if !has_before {
		return strings.clone("click to try · ← → to step", allocator)
	}
	return strings.concatenate({"return keeps · esc goes back to ", before}, allocator)
}

// While searching the index gives way to the count.
matches :: proc(count: uint, allocator := context.allocator) -> string {
	switch count {
	case 0:
		return strings.clone("", allocator)
	case 1:
		return strings.clone("1 match · return tries the first", allocator)
	}
	return fmt.aprintf("%d matches · return tries the first", count, allocator = allocator)
}

no_match :: proc(query: string, paper: bool, allocator := context.allocator) -> string {
	return strings.concatenate({"No ", "papers" if paper else "films", " match “", trim(query), "”"}, allocator)
}

// Done's tooltip.
done_help :: proc "contextless" (paper: bool) -> cstring {
	return "Keep this paper (Return)" if paper else "Keep this film (Return)"
}

// The paper tray's switch for the paper lock.
keep_paper_words :: proc "contextless" (kept: bool) -> cstring {
	return "Kept for every film" if kept else "Keep for every film"
}
