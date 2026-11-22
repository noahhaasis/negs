// Compare (ticket 46, windows/src/compare.rs; `Compare.swift`): two
// photographs side by side or in one frame with a line between them, or a
// photograph against its original; and what Original ⌄ makes the Original
// word do. The rules and words here; the geometry in layout.odin.
//
// A photograph is its path and, for a duplicate, its variant; two are the
// same when both are byte for byte (as the library compares paths).
package compare

import "core:fmt"
import "core:strings"

// A string that may be missing (negs_compare_text_t).
Text :: struct {
	text: string,
	has:  bool,
}

// A photograph, or one of its duplicates (negs_compare_item_t).
Item :: struct {
	photo:   string,
	variant: Text,
}

text_equal :: proc "contextless" (a, b: Text) -> bool {
	return a.has == b.has && (!a.has || a.text == b.text)
}

item_equal :: proc "contextless" (a, b: Item) -> bool {
	return a.photo == b.photo && text_equal(a.variant, b.variant)
}

// Which side the panel edits: the active one, or the developed one beside
// its original. True for the first.
edited_is_first :: proc "contextless" (first_original, second_original, active_first: bool) -> bool {
	active_original := first_original if active_first else second_original
	return !active_first if active_original else active_first
}

against_original :: proc "contextless" (first_original, second_original: bool) -> bool {
	return first_original || second_original
}

// What the context menu's item for a right-click offers.
Offer_Kind :: enum u32 {
	// Three or more: "Compare Two Photographs", off.
	Too_Many,
	// The two targets with each other.
	Pair,
	// The one clicked with the current photograph (first).
	With_Current,
	// The one clicked with its original.
	With_Original,
}

Offer :: struct {
	kind:          Offer_Kind,
	enabled:       bool,
	with_original: bool,
}

// The item for a right-click acting on `target_count` photographs (the
// selection it's in, or itself): two compare with each other, three or
// more can't, one compares with the current photograph (`current_other`:
// there is one and it isn't the one clicked), or with its original.
offer :: proc "contextless" (target_count: uint, current_other, can_compare, can_with_original: bool) -> Offer {
	switch {
	case target_count > 2:
		return {.Too_Many, false, false}
	case target_count == 2:
		return {.Pair, can_compare, false}
	case current_other:
		return {.With_Current, can_compare, false}
	}
	return {.With_Original, can_compare && can_with_original, true}
}

// The offer's title: `a` and `b` are the pair's numbers, `a` the current
// photograph's, or the original's word ("Original", "Camera JPEG").
offer_title :: proc(kind: Offer_Kind, a, b: string, allocator := context.allocator) -> string {
	switch kind {
	case .Too_Many:
		return strings.clone("Compare Two Photographs", allocator)
	case .Pair:
		return strings.concatenate({"Compare ", a, " and ", b}, allocator)
	case .With_Current, .With_Original:
		return strings.concatenate({"Compare with ", a}, allocator)
	}
	return ""
}

Word_Kind :: enum u32 {
	// Two selected: it acts on them.
	Pair,
	Too_Many,
	// Fewer than two, or comparing isn't possible now.
	Select_Second,
}

// The title bar's Compare word, the C key and View › Compare: whether they
// act on the selection, and the word's tooltip.
word :: proc "contextless" (selected: uint, can_compare: bool) -> Word_Kind {
	switch {
	case selected == 2 && can_compare:
		return .Pair
	case selected > 2:
		return .Too_Many
	}
	return .Select_Second
}

word_title :: proc(kind: Word_Kind, a, b: string, allocator := context.allocator) -> string {
	switch kind {
	case .Pair:
		return strings.concatenate({"Compare ", a, " and ", b, " (C)"}, allocator)
	case .Too_Many:
		return strings.clone("Compare takes two photographs. Three or more can't be compared.", allocator)
	case .Select_Second:
		return strings.clone("Select a second photograph to compare (Ctrl-click)", allocator)
	}
	return ""
}

// The camera's facts by part (`PhotoFacts.parts`), as the status line names them.
PARTS := [5]cstring{"camera", "ISO", "shutter", "aperture", "focal length"}

// What two photographs differ in, as the status line says it after
// "Comparing 3 and 7 · differ in": "shutter", "film and paper", "ISO,
// shutter and film" (`CompareDifferences`). The facts count where either
// has them; nothing while either's are unknown (nil).
differences :: proc(a, b: ^[5]Text, film: [2]string, paper: [2]Text, allocator := context.allocator) -> string {
	names := make([dynamic]string, 0, 7, context.temp_allocator)
	if a != nil && b != nil {
		for name, i in PARTS {
			if (a[i].has || b[i].has) && !text_equal(a[i], b[i]) {
				append(&names, string(name))
			}
		}
	}
	if film[0] != film[1] {
		append(&names, "film")
	}
	if !text_equal(paper[0], paper[1]) {
		append(&names, "paper")
	}
	switch len(names) {
	case 0:
		return strings.clone("", allocator)
	case 1:
		return strings.clone(names[0], allocator)
	}
	rest := strings.join(names[:len(names) - 1], ", ", context.temp_allocator)
	return strings.concatenate({rest, " and ", names[len(names) - 1]}, allocator)
}

// The View menu's Compare: "Stop Comparing" while comparing.
menu_title :: proc "contextless" (comparing: bool) -> cstring {
	return "Stop C&omparing\tC" if comparing else "C&ompare\tC"
}

// ← →: the photograph `offset` places from `from` in `order`, skipping
// `first`; −1 past the ends. One no longer shown steps from the first. The
// index into `order`.
step :: proc "contextless" (from, first: Item, order: []Item, offset: int) -> int {
	// The candidates are `order` without `first`; positions count among them.
	start := -1
	n := 0
	for item in order {
		if item_equal(item, first) {
			continue
		}
		if item_equal(item, from) {
			start = n
			break
		}
		n += 1
	}
	if start < 0 {
		index := -1
		for item, i in order {
			if item_equal(item, first) {
				index = i
				break
			}
		}
		if index < 0 {
			return -1
		}
		// From where the first is: the one after it is a step on.
		start = index - 1 if offset > 0 else index
	}
	to := start + offset
	candidates := 0
	for item in order {
		if !item_equal(item, first) {
			candidates += 1
		}
	}
	if to < 0 || to >= candidates {
		return -1
	}
	k := 0
	for item, i in order {
		if item_equal(item, first) {
			continue
		}
		if k == to {
			return i
		}
		k += 1
	}
	return -1
}

// The number a photograph is known by on the roll: its place in the
// folder, counted from 1, and a duplicate's letter after it (7a, 7b).
// `items` is the folder's, every duplicate in order. "" when it isn't there.
frame_number :: proc(items: []Item, item: Item, allocator := context.allocator) -> string {
	// Its photograph's place among the folder's photographs, each counted
	// once where it first comes.
	index := -1
	photos := 0
	for it, i in items {
		seen := false
		for earlier in items[:i] {
			if earlier.photo == it.photo {
				seen = true
				break
			}
		}
		if seen {
			continue
		}
		if it.photo == item.photo {
			index = photos
			break
		}
		photos += 1
	}
	if index < 0 {
		return strings.clone("", allocator)
	}
	siblings := 0
	duplicate := -1
	for it in items {
		if it.photo == item.photo {
			if duplicate < 0 && item_equal(it, item) {
				duplicate = siblings
			}
			siblings += 1
		}
	}
	if !item.variant.has || siblings < 2 {
		return fmt.aprintf("%d", index + 1, allocator = allocator)
	}
	duplicate = max(duplicate, 0)
	if duplicate < 26 {
		letters := "abcdefghijklmnopqrstuvwxyz"
		letter := letters[duplicate:][:1]
		return fmt.aprintf("%d%s", index + 1, letter, allocator = allocator)
	}
	return fmt.aprintf("%d·%d", index + 1, duplicate + 2, allocator = allocator)
}
