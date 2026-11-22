// Thumbnails: the sizes they're read at, the order they're read and
// developed in (`EditorModel.thumbnailCentre`, `outward`, `prefetchTargets`,
// `pumpThumbnails`), and how many at once.
package sheet

import "core:slice"

// The sizes thumbnails are read at (`PhotoThumbnails.Size`): the strip's
// (frames 140 DIP tall, up to about 210 wide) and the grid's two.
Thumb_Size :: enum u32 {
	Strip,
	Medium,
	Large,
}

// The long side in pixels.
thumb_pixels :: proc "contextless" (size: Thumb_Size) -> u32 {
	switch size {
	case .Strip:
		return 432
	case .Medium:
		return 384
	case .Large:
		return 768
	}
	return 0
}

// The grid's, for pictures `points` on their long side at `scale`.
thumb_grid :: proc "contextless" (points, scale: f64) -> Thumb_Size {
	return .Medium if points * scale <= 384.0 else .Large
}

// The grid's for a row height: landscapes are about 1.5 × it across.
thumb_for_rows :: proc "contextless" (size, scale: f64) -> Thumb_Size {
	return thumb_grid(size * 1.5, scale)
}

// Each size's memory cache: bytes and count (`PhotoThumbnails.caches`).
thumb_cache_limits :: proc "contextless" (size: Thumb_Size) -> (bytes, count: uint) {
	switch size {
	case .Strip:
		return 48 << 20, 120
	case .Medium:
		return 48 << 20, 400
	case .Large:
		return 96 << 20, 200
	}
	return 0, 0
}

// Where reading and developing go out from once those on screen are done:
// the middle of the view (indices ascending), else the open photograph.
thumbnail_centre :: proc "contextless" (on_screen: []uint, open: uint, has_open: bool) -> uint {
	if len(on_screen) == 0 {
		return open if has_open else 0
	}
	return on_screen[len(on_screen) / 2]
}

// Each of `indices` (in any order) nearest `centre` first; of two as near,
// the one before; of the same index twice, the earlier: the positions in
// `indices`, into `order` (as long). Rust's stable sort by (distance, index).
outward :: proc(indices: []uint, centre: uint, order: []uint) {
	Key :: struct {
		distance, index, position: uint,
	}
	keys := make([]Key, len(indices))
	defer delete(keys)
	for index, position in indices {
		keys[position] = {abs_diff(index, centre), index, uint(position)}
	}
	slice.sort_by(keys, proc(a, b: Key) -> bool {
		if a.distance != b.distance {
			return a.distance < b.distance
		}
		if a.index != b.index {
			return a.index < b.index
		}
		return a.position < b.position
	})
	for k, i in keys {
		order[i] = k.position
	}
}

// `ascending` (distinct indices, ascending) nearest `centre` first, as
// `outward` orders them, into `out`: the two sides merged, no sort needed.
outward_ascending :: proc "contextless" (ascending: []uint, centre: uint, out: []uint) {
	// The first index past the centre: left of it, nearest last.
	split := len(ascending)
	for v, i in ascending {
		if v > centre {
			split = i
			break
		}
	}
	left, right, n := split - 1, split, 0
	for left >= 0 || right < len(ascending) {
		take_left: bool
		if left < 0 {
			take_left = false
		} else if right >= len(ascending) {
			take_left = true
		} else {
			// A tie goes left: the one before.
			take_left = centre - ascending[left] <= ascending[right] - centre
		}
		if take_left {
			out[n] = ascending[left]
			left -= 1
		} else {
			out[n] = ascending[right]
			right += 1
		}
		n += 1
	}
}

// What to read ahead (`prefetchTargets`): the shown indices within
// PREFETCH_REACH of the middle of the view (else the current one), nearest
// first, but those on screen, which are read anyway. `out` holds at least
// 2 × PREFETCH_REACH + 1; returns how many.
prefetch_order :: proc "contextless" (count: uint, on_screen: []uint, current: uint, has_current: bool, out: []uint) -> int {
	if count == 0 {
		return 0
	}
	centre := min(thumbnail_centre(on_screen, current, has_current), count - 1)
	low := centre - PREFETCH_REACH if centre >= PREFETCH_REACH else 0
	high := min(centre + PREFETCH_REACH, count - 1)
	candidates: [2 * PREFETCH_REACH + 1]uint
	n := 0
	for i := low; i <= high; i += 1 {
		if !slice.contains(on_screen, i) {
			candidates[n] = i
			n += 1
		}
	}
	outward_ascending(candidates[:n], centre, out[:n])
	return n
}

// One photograph as the developing pump sees it (negs_sheet_pump_item_t).
Pump_Item :: struct {
	// Its look is known (its sidecar was read, and could be).
	look:      bool,
	on_screen: bool,
	// Open in the edit view: its thumbnail comes from its preview.
	open:      bool,
	// Being developed, already has a thumbnail, or failed with this look.
	done:      bool,
}

// The order thumbnails are developed in (`pumpThumbnails`): those on screen
// first (in folder order), then the rest outward from the middle of the view
// (or the open photograph); in Low Power those on screen only. `out` is as
// long as `items`; returns how many.
pump_order :: proc "contextless" (items: []Pump_Item, low_power: bool, out: []uint) -> int {
	// The middle of those on screen (not open), else the open photograph.
	shown := 0
	open, has_open := uint(0), false
	for item, i in items {
		if item.on_screen && !item.open {
			shown += 1
		}
		if item.open && !has_open {
			open, has_open = uint(i), true
		}
	}
	centre := open if has_open else 0
	if shown > 0 {
		k := 0
		for item, i in items {
			if item.on_screen && !item.open {
				if k == shown / 2 {
					centre = uint(i)
					break
				}
				k += 1
			}
		}
	}
	wanted :: #force_inline proc "contextless" (i: Pump_Item) -> bool {return i.look && !i.open && !i.done}
	n := 0
	for item, i in items {
		if item.on_screen && wanted(item) {
			out[n] = uint(i)
			n += 1
		}
	}
	if low_power {
		return n
	}
	// The rest outward from the centre: two cursors over the items, the
	// nearest wanted one at or before it and the nearest after it.
	pred :: #force_inline proc "contextless" (items: []Pump_Item, i: int) -> bool {
		return !items[i].on_screen && wanted(items[i])
	}
	c := int(centre)
	left := min(c, len(items) - 1)
	for left >= 0 && !pred(items, left) {
		left -= 1
	}
	right := c + 1
	for right < len(items) && !pred(items, right) {
		right += 1
	}
	for left >= 0 || right < len(items) {
		// A tie goes left: the one before.
		if left >= 0 && (right >= len(items) || c - left <= right - c) {
			out[n] = uint(left)
			left -= 1
			for left >= 0 && !pred(items, left) {
				left -= 1
			}
		} else {
			out[n] = uint(right)
			right += 1
			for right < len(items) && !pred(items, right) {
				right += 1
			}
		}
		n += 1
	}
	return n
}

// How many thumbnails develop at once, each in a helper of its own (about
// 550 MB each): two with 8 cores and 16 GB, else one; one in Low Power.
helpers :: proc "contextless" (cores: uint, memory: u64, low_power: bool) -> uint {
	return 2 if !low_power && cores >= 8 && memory >= 16 << 30 else 1
}

// How many embedded previews are read at once: half the cores, 1…4.
readers :: proc "contextless" (cores: uint) -> uint {
	return clamp(cores / 2, 1, 4)
}
