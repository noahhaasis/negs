// The selected photographs and the current one (`LibrarySelection` in
// `Selection.swift`; selection.rs). A plain click selects one and makes it
// current; modifier clicks change only the selection. The current one is
// always selected. A value: everything in it is in the temporary allocator.
package library

// Items by `item_key`.
Item_Set :: map[string]Item

Selection :: struct {
	has_current: bool,
	current:     Item,
	selected:    Item_Set,
	// Where a range starts (the last plain or Ctrl-click) and where it ends
	// now (the last Shift-click or Shift-arrow).
	has_anchor:  bool,
	anchor:      Item,
	has_lead:    bool,
	lead:        Item,
	// The selection a range is added to: as it was when the anchor was set.
	base:        Item_Set,
}

selection_make :: proc() -> Selection {
	ta := context.temp_allocator
	return Selection{selected = make(Item_Set, ta), base = make(Item_Set, ta)}
}

item_set_of :: proc(items: []Item) -> Item_Set {
	out := make(Item_Set, context.temp_allocator)
	for i in items {
		out[item_key(i)] = i
	}
	return out
}

@(private = "file")
set_clone :: proc(s: Item_Set) -> Item_Set {
	out := make(Item_Set, context.temp_allocator)
	for k, v in s {
		out[k] = v
	}
	return out
}

@(private = "file")
set_has :: proc(s: Item_Set, i: Item) -> bool {
	return item_key(i) in s
}

@(private = "file")
set_add :: proc(s: ^Item_Set, i: Item) {
	s[item_key(i)] = i
}

@(private = "file")
set_remove :: proc(s: ^Item_Set, i: Item) -> bool {
	key := item_key(i)
	if key in s {
		delete_key(s, key)
		return true
	}
	return false
}

sets_equal :: proc(a, b: Item_Set) -> bool {
	if len(a) != len(b) {
		return false
	}
	for k in a {
		if k not_in b {
			return false
		}
	}
	return true
}

selection_contains :: proc(s: ^Selection, i: Item) -> bool {
	return set_has(s.selected, i)
}

// Plain click: only `item`, and it's current.
selection_select :: proc(s: ^Selection, item: Item) {
	s.has_current, s.current = true, item
	s.selected = item_set_of({item})
	s.has_anchor, s.anchor = true, item
	s.has_lead, s.lead = true, item
	s.base = set_clone(s.selected)
}

// `items` alone, the first current: new duplicates.
selection_select_all_of :: proc(s: ^Selection, items: []Item) {
	if len(items) == 0 {
		return
	}
	selection_select(s, items[0])
	s.selected = item_set_of(items)
	s.base = set_clone(s.selected)
}

@(private = "file")
position :: proc(order: []Item, item: Item) -> (int, bool) {
	for o, i in order {
		if item_equal(o, item) {
			return i, true
		}
	}
	return 0, false
}

// Ctrl-click: adds or removes `item`. The current one stays, unless it's
// the one removed and `order` is given (the grid): then the next selected
// one to its right becomes current, or the nearest to its left.
selection_toggle :: proc(s: ^Selection, item: Item, order: []Item, has_order: bool) {
	if !s.has_current {
		selection_select(s, item)
		return
	}
	current := s.current
	if item_equal(item, current) {
		if !has_order {
			return
		}
		index, found := position(order, current)
		if !found {
			return
		}
		next: Item
		has_next := false
		for o in order[index + 1:] {
			if set_has(s.selected, o) {
				next, has_next = o, true
				break
			}
		}
		if !has_next {
			for k := index - 1; k >= 0; k -= 1 {
				if set_has(s.selected, order[k]) {
					next, has_next = order[k], true
					break
				}
			}
		}
		if !has_next {
			return
		}
		set_remove(&s.selected, current)
		s.current = next
		s.has_anchor, s.anchor = true, next
		s.has_lead, s.lead = true, next
		s.base = set_clone(s.selected)
		return
	}
	if !set_remove(&s.selected, item) {
		set_add(&s.selected, item)
	}
	s.has_anchor, s.anchor = true, item
	s.has_lead, s.lead = true, item
	s.base = set_clone(s.selected)
}

// Shift-click or Shift-arrow: the items from the anchor to `item`, in
// `order`, added to the selection as it was when the anchor was set.
selection_extend_to :: proc(s: ^Selection, item: Item, order: []Item) {
	if !s.has_current {
		selection_select(s, item)
		return
	}
	start, has_start := 0, false
	if s.has_anchor {
		start, has_start = position(order, s.anchor)
	}
	if !has_start {
		start, has_start = position(order, s.current)
	}
	end, has_end := position(order, item)
	if !has_start || !has_end {
		return
	}
	selected := set_clone(s.base)
	for o in order[min(start, end):max(start, end) + 1] {
		set_add(&selected, o)
	}
	set_add(&selected, s.current)
	s.selected = selected
	s.has_lead, s.lead = true, item
}

// Shift-arrow: moves the range's end `offset` places on in `order`.
selection_extend_by :: proc(s: ^Selection, offset: int, order: []Item) {
	from: Item
	if s.has_lead {
		from = s.lead
	} else if s.has_current {
		from = s.current
	} else {
		return
	}
	index, found := position(order, from)
	if !found {
		return
	}
	to := clamp(index + offset, 0, len(order) - 1)
	selection_extend_to(s, order[to], order)
}

// Ctrl+A: everything shown.
selection_select_all :: proc(s: ^Selection, shown: []Item) {
	if !s.has_current {
		return
	}
	s.selected = item_set_of(shown)
	set_add(&s.selected, s.current)
	s.has_anchor, s.anchor = true, s.current
	s.has_lead, s.lead = true, s.current
	s.base = set_clone(s.selected)
}

// All but the current one.
selection_deselect_all :: proc(s: ^Selection) {
	if s.has_current {
		selection_select(s, s.current)
	}
}

// Rubber band: `base` and the items inside the band.
selection_band :: proc(s: ^Selection, items: []Item, base: Item_Set) {
	if !s.has_current {
		return
	}
	selected := set_clone(base)
	for i in items {
		set_add(&selected, i)
	}
	set_add(&selected, s.current)
	s.selected = selected
	s.has_lead, s.lead = true, s.current
	s.base = set_clone(s.selected)
}

// Keeps only the items in `shown`, such as after the filter changed.
selection_prune :: proc(s: ^Selection, shown: Item_Set) {
	pruned := make(Item_Set, context.temp_allocator)
	for k, v in s.selected {
		if k in shown || (s.has_current && item_equal(v, s.current)) {
			pruned[k] = v
		}
	}
	if !sets_equal(pruned, s.selected) {
		base := make(Item_Set, context.temp_allocator)
		for k, v in s.base {
			if k in pruned {
				base[k] = v
			}
		}
		s.base = base
		s.selected = pruned
	}
	if s.has_anchor && !set_has(s.selected, s.anchor) {
		s.has_anchor, s.anchor = s.has_current, s.current
	}
	if s.has_lead && !set_has(s.selected, s.lead) {
		s.has_lead, s.lead = s.has_current, s.current
	}
}

// `from` is now called `to`: a duplicate that took its photograph's place.
selection_rename :: proc(s: ^Selection, from, to: Item) {
	if s.has_current && item_equal(s.current, from) {
		s.current = to
	}
	if s.has_anchor && item_equal(s.anchor, from) {
		s.anchor = to
	}
	if s.has_lead && item_equal(s.lead, from) {
		s.lead = to
	}
	if set_remove(&s.selected, from) {
		set_add(&s.selected, to)
	}
	if set_remove(&s.base, from) {
		set_add(&s.base, to)
	}
}

// The selected items in `order`.
selection_ordered :: proc(s: ^Selection, order: []Item) -> []Item {
	out := make([dynamic]Item, 0, len(order), context.temp_allocator)
	for o in order {
		if set_has(s.selected, o) {
			append(&out, o)
		}
	}
	return out[:]
}

// The first item after `item` in `order` that isn't `gone`, or else the
// last one before it (`EditorModel.next`).
next_after :: proc(item: Item, order: []Item, gone: Item_Set) -> (Item, bool) {
	index, found := position(order, item)
	if !found {
		for o in order {
			if !set_has(gone, o) {
				return o, true
			}
		}
		return {}, false
	}
	for o in order[index:] {
		if !set_has(gone, o) {
			return o, true
		}
	}
	for k := index - 1; k >= 0; k -= 1 {
		if !set_has(gone, order[k]) {
			return order[k], true
		}
	}
	return {}, false
}
