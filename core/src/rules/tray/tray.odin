// The film and paper tray's rules (windows/src/tray_rules.rs; IMPLEMENTATION.md
// §4.5, §5.1, §5.2; `Tray.swift`'s `StockTrayRules` and `TrayVisit`, and the
// keys of `EditorWindowController.trayKeys`): the order of the tiles,
// stepping through them, search, the row's geometry and hit areas, the
// foot's words, what keeping or going back does to Undo, and Esc's order.
//
// Lengths are DIPs from the row's left edge. Every constant is exact in
// binary, so Odin's exact folding of constant expressions moves no bit.
package tray

// A tile: 212 wide, the picture at 9:5 (118 tall) and the name under it.
TILE :: 212.0
PICTURE :: 118.0
// Between tiles, and between groups.
TILE_GAP :: 8.0
GROUP_GAP :: 20.0
// The group's name, written up its left side, and the gap after it.
TITLE :: 18.0
TITLE_GAP :: 9.0
// Room round the tiles for the pencil ring, left and right.
INSET :: 8.0
// The tiles' top, under the row's top edge.
TOP :: 9.0
// The picture to the name's line, and the name's line.
NAME_GAP :: 4.0
NAME_LINE :: 20.0
// The row of tiles takes the strip's 158, the foot the status line's 28.
ROW :: 158.0
FOOT :: 28.0
HEIGHT :: ROW + FOOT
// The favourite's star: 20 square, 5 in from the tile's top right corner.
STAR :: 20.0
STAR_INSET :: 5.0
// One notch of a mouse wheel scrolls the row this far.
WHEEL_NOTCH :: 80.0
// A group clicked in the foot glides there over 0.25 s (`TrayScroll`).
GLIDE_MS :: 250.0

FAVOURITES :: "Favourites"
// Your negative's group (ticket 33), and its tile's id: never a stock.
SCAN :: "Scan"
YOUR_NEGATIVE :: "negs.your-negative"
// The paper lock's switch, as a screen reader says it.
KEEP_PAPER_LABEL :: "Keep paper when changing film"

// A stock as the tray lists it (negs_tray_entry_t).
Entry :: struct {
	id, name: string,
}

// A group of tiles under its title; the favourites are one too
// (negs_tray_section_t).
Section :: struct {
	title:      string,
	stocks:     []Entry,
	favourites: bool,
}

// A tile in the row: its stock, and whether it's the favourites' copy
// (negs_tray_place_t).
Place :: struct {
	stock:     string,
	favourite: bool,
}

// MARK: The row

// Which of the matched groups show, and whether the favourites lead: the
// favourites first under their own title (each also stays in its group),
// hidden while searching; empty groups gone. `counts`: each matched group's
// stocks; `kept` (as long) gets the groups shown, in order. Returns how many.
sections :: proc "contextless" (counts: []uint, favourite_count: uint, query: string, kept: []uint) -> (n: int, favourites_lead: bool) {
	for c, i in counts {
		if c > 0 {
			kept[n] = uint(i)
			n += 1
		}
	}
	return n, !(searching(query) || favourite_count == 0)
}

// The film tray's row leads with Your negative, in its own Scan group, so
// the order never changes from one photograph to the next; not while
// searching, nor for papers or the films a negative is read as.
with_scan :: proc "contextless" (paper, read_as: bool, query: string) -> bool {
	return !(paper || read_as || searching(query))
}

// The order ← → step through: the tiles as shown, the favourites first.
// `out` holds every stock of every section; returns how many.
places :: proc "contextless" (sections: []Section, out: []Place) -> int {
	n := 0
	for s in sections {
		for e in s.stocks {
			out[n] = Place{e.id, s.favourites}
			n += 1
		}
	}
	return n
}

// The tile `offset` along from `current` (its favourites' copy if
// `in_favourites` and it has one), held at either end; from a stock not in
// the row, the first. A copy of the same stock next to it, the last
// favourite's own in the first group, is passed over, so every step tries
// another stock. The place's index; −1 without places.
step :: proc "contextless" (current: string, in_favourites: bool, offset: i32, places: []Place) -> int {
	if len(places) == 0 {
		return -1
	}
	index := -1
	for p, i in places {
		if p.stock == current && p.favourite == in_favourites {
			index = i
			break
		}
	}
	if index < 0 {
		for p, i in places {
			if p.stock == current {
				index = i
				break
			}
		}
	}
	if index < 0 {
		return 0
	}
	direction := -1 if offset < 0 else 1
	count := u32(offset) if offset >= 0 else u32(-i64(offset))
	for _ in 0 ..< count {
		next := index + direction
		for next >= 0 && next < len(places) && places[next].stock == places[index].stock {
			next += direction
		}
		if next < 0 || next >= len(places) {
			break
		}
		index = next
	}
	return index
}

// MARK: The row's geometry

// A section's width: its title and its tiles.
width :: proc "contextless" (stock_count: uint) -> f64 {
	count := f64(stock_count)
	return TITLE + TITLE_GAP + count * TILE + max(count - 1.0, 0.0) * TILE_GAP
}

// Where each section starts in the row, into `out` (as long).
starts :: proc "contextless" (sections: []Section, out: []f64) {
	x := f64(INSET)
	for s, i in sections {
		out[i] = x
		x += width(uint(len(s.stocks))) + GROUP_GAP
	}
}

// The row's whole width.
content_width :: proc "contextless" (sections: []Section) -> f64 {
	sum := transmute(f64)u64(0x8000_0000_0000_0000) // Rust's sum starts from −0.
	for s in sections {
		sum += width(uint(len(s.stocks)))
	}
	return sum + 2.0 * INSET + f64(max(len(sections), 1) - 1) * GROUP_GAP
}

// The left edge of the `index`th tile of the section starting at `start`.
tile_left :: proc "contextless" (start: f64, index: uint) -> f64 {
	return start + TITLE + TITLE_GAP + f64(index) * (TILE + TILE_GAP)
}

// The left edge of `stock`'s tile in section `section`.
tile_x :: proc "contextless" (stock: string, section: uint, sections: []Section) -> (x: f64, ok: bool) {
	if section >= uint(len(sections)) {
		return 0, false
	}
	for e, index in sections[section].stocks {
		if e.id == stock {
			start := f64(INSET)
			for s in sections[:section] {
				start += width(uint(len(s.stocks))) + GROUP_GAP
			}
			return tile_left(start, uint(index)), true
		}
	}
	return 0, false
}

@(private = "file")
has :: proc "contextless" (s: Section, stock: string) -> bool {
	for e in s.stocks {
		if e.id == stock {
			return true
		}
	}
	return false
}

// The section a stock's tile is shown in: with `favourites_first` (as the
// tray opens, or once stepped or clicked to among the favourites) the
// favourites' copy of a favourite, else its group. −1 for none.
section_of :: proc "contextless" (stock: string, sections: []Section, favourites_first: bool) -> int {
	if favourites_first {
		for s, i in sections {
			if s.favourites && has(s, stock) {
				return i
			}
		}
	}
	for s, i in sections {
		if !s.favourites && has(s, stock) {
			return i
		}
	}
	return -1
}

// The group in view, underlined in the foot: the last one starting within
// 40 of the row's left edge.
current :: proc "contextless" (scrolled_to: f64, starts: []f64) -> uint {
	for i := len(starts) - 1; i >= 0; i -= 1 {
		if starts[i] <= scrolled_to + 40.0 {
			return uint(i)
		}
	}
	return 0
}

// The scroll offset that centres a tile at `tile_x` in a row `visible` wide.
centring :: proc "contextless" (tile_x, visible, content: f64) -> f64 {
	return clamp_offset(tile_x - (visible - TILE) / 2.0, visible, content)
}

// The scroll offset that puts a group 30 in from the left, as the foot's
// index jumps.
jump :: proc "contextless" (start, visible, content: f64) -> f64 {
	return clamp_offset(start - 30.0, visible, content)
}

clamp_offset :: proc "contextless" (x, visible, content: f64) -> f64 {
	return min(max(x, 0.0), max(content - visible, 0.0))
}

// The offset after scrolling `offset` to show `stock`'s tile: centred, or
// (not `centre`) moved only if it's partly out of view.
reveal :: proc "contextless" (offset: f64, stock: string, in_favourites, centre: bool, sections: []Section, visible: f64) -> f64 {
	section := section_of(stock, sections, in_favourites)
	if section < 0 {
		return offset
	}
	x, ok := tile_x(stock, uint(section), sections)
	if !ok {
		return offset
	}
	if !centre && x >= offset && x + TILE <= offset + visible {
		return offset
	}
	return centring(x, visible, content_width(sections))
}

// The offset after a wheel turn: a vertical wheel scrolls the row sideways,
// as a sideways one does. `delta` in Windows' wheel units (120 a notch),
// positive away from the user or to the right.
wheel :: proc "contextless" (offset: f64, delta: i32, sideways: bool, visible, content: f64) -> f64 {
	step := f64(delta) / 120.0 * WHEEL_NOTCH
	// Rolling away moves back along the row, as a list scrolls up; tilting
	// right moves on.
	return clamp_offset(offset + step if sideways else offset - step, visible, content)
}

// The offset `elapsed_ms` into a glide from `from` to `to`, eased in and
// out; `to` once it's over.
glide :: proc "contextless" (from, to, elapsed_ms: f64) -> f64 {
	t := elapsed_ms / GLIDE_MS
	if t < 0.0 {
		t = 0.0
	}
	if t > 1.0 {
		t = 1.0
	}
	return from + (to - from) * t * t * (3.0 - 2.0 * t)
}

Hit_Kind :: enum u32 {
	None,
	// A tile: its section and index.
	Tile,
	// A tile's star.
	Star,
}

// What's under a point of the row, in DIPs from its left edge (with the
// scroll added) and from its top.
hit :: proc "contextless" (sections: []Section, x, y: f64) -> (kind: Hit_Kind, section, index: uint) {
	if y < TOP || y >= TOP + PICTURE + NAME_GAP + NAME_LINE {
		return .None, 0, 0
	}
	start := f64(INSET)
	for s, si in sections {
		for i in 0 ..< len(s.stocks) {
			left := tile_left(start, uint(i))
			if x >= left && x < left + TILE {
				star := star_rect(left)
				if x >= star[0] && x < star[0] + star[2] && y >= star[1] && y < star[1] + star[3] {
					return .Star, uint(si), uint(i)
				}
				return .Tile, uint(si), uint(i)
			}
		}
		start += width(uint(len(s.stocks))) + GROUP_GAP
	}
	return .None, 0, 0
}

// The star's place (x, y, w, h) on a tile whose left edge is `left`.
star_rect :: proc "contextless" (left: f64) -> [4]f64 {
	return {left + TILE - STAR_INSET - STAR, TOP + STAR_INSET, STAR, STAR}
}

// MARK: The stand-in

// The size a tried stock's tile is laid out at on the canvas until its
// develop is in: its own shape, with the long side of the photograph
// (`photo`, after cropping), else 2048 as a RAW's (`standInSize`).
stand_in_size :: proc "contextless" (image: [2]u32, photo: [2]f64) -> [2]f64 {
	long := max(photo[0], photo[1])
	long = long if long > 0.0 else 2048.0
	scale := long / f64(max(max(image[0], image[1]), 1))
	return {f64(image[0]) * scale, f64(image[1]) * scale}
}
