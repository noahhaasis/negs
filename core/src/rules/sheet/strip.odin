// The film strip: frames edge to edge but for the gaps, each at its
// photograph's proportions. Worked out, never measured.
package sheet

// A photograph's shape as its frames keep it: the one already known (its
// header's, or its embedded preview's) unless a thumbnail's differs by more
// than a thumbnail's rounding and a lens correction's trim, so frames don't
// move as previews and developed thumbnails arrive.
kept_ratio :: proc "contextless" (known: f64, has_known: bool, thumbnail: f64) -> f64 {
	if has_known && abs(known - thumbnail) / known < 0.01 {
		return known
	}
	return thumbnail
}

// A frame's width: the picture's height at its photograph's proportions.
frame_width :: proc "contextless" (ratio: f64, has_ratio: bool) -> f64 {
	r := ratio if has_ratio else FALLBACK_RATIO
	return roundf(PICTURE * clampf(r, 0.1, 10.0))
}

// Each frame's left edge into `xs` (as long as `widths`); the whole row's width.
strip_layout :: proc "contextless" (widths: []f64, xs: []f64) -> (width: f64) {
	x := 0.0
	sum := sum_zero()
	for w, i in widths {
		xs[i] = x
		x += w + STRIP_GAP
		sum += w
	}
	gaps := len(widths) - 1 if len(widths) > 0 else 0
	return sum + STRIP_GAP * f64(gaps)
}

// A frame's middle.
strip_middle :: proc "contextless" (x, width: f64) -> f64 {
	return x + width / 2.0
}

Strip_Test :: enum u8 {
	// The frame reaches `v` (its right edge at or past it).
	Reaches,
	// The frame's right edge is past `v`.
	Ends_Past,
	// The frame starts past `v`.
	Starts_Past,
}

strip_test :: #force_inline proc "contextless" (xs, widths: []f64, i: int, test: Strip_Test, v: f64) -> bool {
	switch test {
	case .Reaches:
		return xs[i] + widths[i] >= v
	case .Ends_Past:
		return xs[i] + widths[i] > v
	case .Starts_Past:
		return xs[i] > v
	}
	return false
}

// The first index for which the test holds, the count if none (the strip's
// own bisection, `StripLayout::partition`).
strip_partition :: proc "contextless" (xs, widths: []f64, test: Strip_Test, v: f64) -> int {
	low, high := 0, len(xs)
	for low < high {
		middle := (low + high) / 2
		if strip_test(xs, widths, middle, test, v) {
			high = middle
		} else {
			low = middle + 1
		}
	}
	return low
}

// The frames reaching into `min_x…max_x`.
strip_indices :: proc "contextless" (xs, widths: []f64, min_x, max_x: f64) -> (start, end: int) {
	start = strip_partition(xs, widths, .Reaches, min_x)
	end = strip_partition(xs, widths, .Starts_Past, max_x)
	return start, max(end, start)
}

// The frame under `x` (in the row), not the gaps; −1 for none.
strip_hit :: proc "contextless" (xs, widths: []f64, x: f64) -> int {
	index := strip_partition(xs, widths, .Ends_Past, x)
	if index < len(xs) && x >= xs[index] {
		return index
	}
	return -1
}

// The frame at `x`, or the nearest one; −1 without frames.
strip_index_at :: proc "contextless" (xs, widths: []f64, x: f64) -> int {
	if len(xs) == 0 {
		return -1
	}
	return min(strip_partition(xs, widths, .Reaches, x), len(xs) - 1)
}

// The scroll offset that puts `middle` in the middle of a view `visible`
// wide, as far as a row `content` wide goes (`StripSettling.target`).
centred :: proc "contextless" (middle, visible, content: f64) -> f64 {
	return min(max(middle - visible / 2.0, 0.0), max(content - visible, 0.0))
}

// The row was laid out anew: the scroll offset that keeps a frame's middle
// (`old_middle`, now `new_middle`) where it was on screen (`PhotoBrowser.hold`).
hold :: proc "contextless" (old_middle, new_middle, new_width, scroll, visible: f64) -> f64 {
	x := scroll + new_middle - old_middle
	return min(max(x, 0.0), max(new_width - visible, 0.0))
}

// The smoothstep glide back to the middle after a resize.
smoothstep :: proc "contextless" (along: f64) -> f64 {
	t := clampf(along, 0.0, 1.0)
	return t * t * (3.0 - 2.0 * t)
}
