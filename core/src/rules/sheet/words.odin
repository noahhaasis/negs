// The sentence over the grid and the foot under it (`SheetWords`), and the
// foot's Size slider.
package sheet

import "core:fmt"
import "core:strings"
import "../../library"

Part_Kind :: enum u32 {
	// Words in the sentence's italic.
	Words,
	// An underlined word that opens the filter menu; `on` while filtered
	// (the pencil under it).
	Filter,
	// The × that clears the filter.
	Clear,
}

// A piece of the sentence over the grid.
Part :: struct {
	kind: Part_Kind,
	text: string,
	on:   bool,
}

// The most parts a sentence has: "Showing … · ", a lead and a word for each
// of the filter's three parts, and the ×.
MAX_PARTS :: 8

// "Showing *all* 12 photographs", or when filtered "Showing 4 of 12 · rated
// *★★★ or more*, *picks*, *edited* ×". The texts are in `allocator`.
sentence :: proc(filter: library.Filter, shown, total: uint, allocator := context.allocator) -> [dynamic]Part {
	parts := make([dynamic]Part, 0, MAX_PARTS, allocator)
	if !library.filter_active(filter) {
		append(&parts, Part{.Words, strings.clone("Showing ", allocator), false})
		append(&parts, Part{.Filter, strings.clone("all", allocator), false})
		words := library.photographs(int(total), context.temp_allocator)
		append(&parts, Part{.Words, strings.concatenate({" ", words}, allocator), false})
		return parts
	}
	append(&parts, Part{.Words, fmt.aprintf("Showing %d of %d · ", shown, total, allocator = allocator), false})
	for word, index in library.filter_words(filter, context.temp_allocator) {
		lead := strings.concatenate({", ", word.lead}, allocator) if index > 0 else strings.clone(word.lead, allocator)
		if len(lead) > 0 {
			append(&parts, Part{.Words, lead, false})
		} else {
			delete(lead, allocator)
		}
		append(&parts, Part{.Filter, strings.clone(word.word, allocator), true})
	}
	append(&parts, Part{.Clear, "", false})
	return parts
}

// The foot's count: "12 photographs · 1 selected", or "4 of 12 photographs".
foot :: proc(shown, total, selected: uint, filtered: bool, allocator := context.allocator) -> string {
	return library.count_words(int(shown), int(total), int(selected), filtered, allocator)
}

// The foot's Size slider: its value as a share of its track, and back.
size_fraction :: proc "contextless" (size: f64) -> f64 {
	return clampf((size - GRID_SIZE_MIN) / (GRID_SIZE_MAX - GRID_SIZE_MIN), 0.0, 1.0)
}

size_at :: proc "contextless" (fraction: f64) -> f64 {
	return clamp_size(GRID_SIZE_MIN + clampf(fraction, 0.0, 1.0) * (GRID_SIZE_MAX - GRID_SIZE_MIN))
}
