// The grid (`SheetLayout`): thumbnails packed edge to edge in rows with gaps
// of 6, each at its own proportions, every full row stretched or shrunk a
// little to the sheet's width, the last row left short at the chosen height.
package sheet

// x, y, width, height (negs_rect_t).
Rect :: struct {
	x, y, w, h: f64,
}

// A row of the grid: its photographs start..<end, its top and height
// (negs_sheet_row_t).
Row :: struct {
	start, end: uint,
	y, height:  f64,
}

rect_max_x :: #force_inline proc "contextless" (r: Rect) -> f64 {return r.x + r.w}
rect_max_y :: #force_inline proc "contextless" (r: Rect) -> f64 {return r.y + r.h}
rect_mid_x :: #force_inline proc "contextless" (r: Rect) -> f64 {return r.x + r.w / 2.0}

rect_contains :: #force_inline proc "contextless" (r: Rect, x, y: f64) -> bool {
	return x >= r.x && x < rect_max_x(r) && y >= r.y && y < rect_max_y(r)
}

// They overlap, edges touching not counted (`CGRect.intersects`).
rect_intersects :: #force_inline proc "contextless" (r, o: Rect) -> bool {
	return r.x < rect_max_x(o) && o.x < rect_max_x(r) && r.y < rect_max_y(o) && o.y < rect_max_y(r)
}

clamp_size :: proc "contextless" (size: f64) -> f64 {
	return roundf(clampf(size, GRID_SIZE_MIN, GRID_SIZE_MAX))
}

// Lays out `ratios` (each photograph's width ÷ height, in order) across a
// sheet `width` wide at rows about `target` high: a frame per ratio into
// `frames`, the rows into `rows` (both as long as `ratios`). Returns the
// rows used and the whole grid's height, padding included.
//
// Unchecked indexing (bounds checks made it slower than the Rust): every
// index is below `n`, which no slice is shorter than, and a row holds at
// least one frame, so there are never more rows than frames.
layout :: proc "contextless" (ratios: []f64, width, chosen: f64, frames: []Rect, rows: []Row) -> (row_count: int, height: f64) #no_bounds_check {
	usable := max(width - 2.0 * GRID_SIDE, 1.0)
	target := max(chosen, 1.0)
	n := min(len(ratios), len(frames), len(rows))
	y := f64(GRID_TOP)
	start := 0
	for start < n {
		end, sum, full := start, 0.0, false
		for end < n {
			next := sum + clampf(ratios[end], 0.1, 10.0)
			gaps := GRID_GAP * f64(end - start)
			if next * target + gaps < usable {
				sum = next
				end += 1
				continue
			}
			full = true
			// Take this one too if that leaves the row nearer the chosen
			// height than stopping short of it does.
			if end == start || abs(lnf((usable - gaps) / next / target)) <= abs(lnf((usable - gaps + GRID_GAP) / sum / target)) {
				sum = next
				end += 1
			}
			break
		}
		count := end - start
		h := roundf((usable - GRID_GAP * f64(count - 1)) / sum) if full else roundf(target)
		x := f64(GRID_SIDE)
		for index in start ..< end {
			// Whole DIPs; a full row's last cell ends flush at the right.
			w: f64
			if full && index == end - 1 {
				w = GRID_SIDE + usable - x
			} else {
				w = max(roundf(clampf(ratios[index], 0.1, 10.0) * h), 1.0)
			}
			frames[index] = Rect{x, y, w, h}
			x += w + GRID_GAP
		}
		rows[row_count] = Row{uint(start), uint(end), y, h}
		row_count += 1
		y += h + GRID_GAP
		start = end
	}
	height = GRID_TOP + GRID_BOTTOM if row_count == 0 else y - GRID_GAP + GRID_BOTTOM
	return
}

Row_Test :: enum u8 {
	// The row starts at or before an index.
	Starts_By,
	// The row ends above `v`.
	Ends_Above,
	// The row starts at or above `v`.
	Starts_At_Or_Above,
}

row_test :: #force_inline proc "contextless" (r: Row, test: Row_Test, index: uint, v: f64) -> bool {
	switch test {
	case .Starts_By:
		return r.start <= index
	case .Ends_Above:
		return r.y + r.height < v
	case .Starts_At_Or_Above:
		return r.y <= v
	}
	return false
}

// Rust's slice::partition_point, probe for probe (binary_search_by), so a
// predicate that isn't partitioned gives the same answer too.
row_partition :: proc "contextless" (rows: []Row, test: Row_Test, index: uint, v: f64) -> int {
	size := len(rows)
	if size == 0 {
		return 0
	}
	base := 0
	for size > 1 {
		half := size / 2
		mid := base + half
		// pred true is Less: go right.
		if row_test(rows[mid], test, index, v) {
			base = mid
		}
		size -= half
	}
	return base + 1 if row_test(rows[base], test, index, v) else base
}

// The row holding the `index`th photograph; −1 past the last.
row_of :: proc "contextless" (rows: []Row, frame_count: int, index: uint) -> int {
	if index >= uint(frame_count) || len(rows) == 0 {
		return -1
	}
	return row_partition(rows, .Starts_By, index, 0) - 1
}

// The photograph `by` rows up (−) or down (+) from `index`: the one nearest
// it across. −1 past the first or last row.
vertical :: proc "contextless" (frames: []Rect, rows: []Row, index: uint, by: int) -> int {
	current := row_of(rows, len(frames), index)
	if current < 0 {
		return -1
	}
	target := current + by
	if target < 0 || target >= len(rows) {
		return -1
	}
	row := rows[target]
	x := rect_mid_x(frames[index])
	best := -1
	best_key: i64
	for i in row.start ..< row.end {
		key := total_key(abs(rect_mid_x(frames[i]) - x))
		if best < 0 || key < best_key {
			best, best_key = int(i), key
		}
	}
	return best
}

// The cells `rect` touches, into `out` (as long as the frames); how many.
indices_in :: proc "contextless" (frames: []Rect, rows: []Row, rect: Rect, out: []uint) -> int {
	n := 0
	for r in rows {
		if !(r.y <= rect_max_y(rect) && r.y + r.height >= rect.y) {
			continue
		}
		for i in r.start ..< r.end {
			if rect_intersects(frames[i], rect) {
				out[n] = i
				n += 1
			}
		}
	}
	return n
}

// The photographs in the rows reaching into `min_y…max_y`.
indices :: proc "contextless" (rows: []Row, min_y, max_y: f64) -> (start, end: uint) {
	first := row_partition(rows, .Ends_Above, 0, min_y)
	last := row_partition(rows, .Starts_At_Or_Above, 0, max_y)
	if first >= last {
		return 0, 0
	}
	return rows[first].start, rows[last - 1].end
}

// The cell under (`x`, `y`), not the gaps; −1 for none.
hit :: proc "contextless" (frames: []Rect, rows: []Row, x, y: f64) -> int {
	for r in rows {
		if y >= r.y && y < r.y + r.height {
			for i in r.start ..< r.end {
				if rect_contains(frames[i], x, y) {
					return int(i)
				}
			}
			return -1
		}
	}
	return -1
}

@(private = "file")
distance :: #force_inline proc "contextless" (low, high, v: f64) -> f64 {
	return max(max(low - v, v - high), 0.0)
}

// The photograph at a point, or else the nearest one; −1 without rows.
nearest :: proc "contextless" (frames: []Rect, rows: []Row, x, y: f64) -> int {
	row := -1
	row_key: i64
	for r, i in rows {
		key := total_key(distance(r.y, r.y + r.height, y))
		if row < 0 || key < row_key {
			row, row_key = i, key
		}
	}
	if row < 0 {
		return -1
	}
	best := -1
	best_key: i64
	for i in rows[row].start ..< rows[row].end {
		f := frames[i]
		key := total_key(distance(f.x, rect_max_x(f), x))
		if best < 0 || key < best_key {
			best, best_key = int(i), key
		}
	}
	return best
}

// The scroll offset that puts `row` in the middle of a view `viewport`
// high, as far as the grid's ends (`height`) let it.
centre_row :: proc "contextless" (row: Row, height, viewport: f64) -> f64 {
	return min(max(row.y + row.height / 2.0 - viewport / 2.0, 0.0), max(height - viewport, 0.0))
}

// The least scroll from `scroll` that shows a frame's yellow box whole,
// with BOX_MARGIN round it; up and down only.
reveal :: proc "contextless" (frame: Rect, height, scroll, viewport: f64) -> f64 {
	top := frame.y - SELECTION_OUTSET - BOX_MARGIN
	bottom := rect_max_y(frame) + SELECTION_OUTSET + BOX_MARGIN
	most := max(height - viewport, 0.0)
	s: f64
	if top < scroll {
		s = top
	} else if bottom > scroll + viewport {
		s = bottom - viewport
	} else {
		s = scroll
	}
	return min(max(s, 0.0), most)
}

// After the grid is laid out anew with the same photographs, the scroll
// offset that keeps a photograph (its `frame`) where it was on screen
// (`SheetScroller.layoutChanged`): the point `down` of the way down its
// picture stays `from_top` below the top.
keep_anchor :: proc "contextless" (frame: Rect, height, down, from_top, viewport: f64) -> f64 {
	return min(max(frame.y + down * frame.h - from_top, 0.0), max(height - viewport, 0.0))
}
