// Enlarger › Shifts move (state.rs's `FilterLink`, `Parameters.swift`):
// how the magenta and yellow shift sliders move each other. A preference
// of the app, not of the edit.
package state

Filter_Link :: enum u32 {
	Separate,
	Together,
	Opposite,
}

FILTER_LINK_MAGENTA :: "enlarger.m_filter_shift"
FILTER_LINK_YELLOW :: "enlarger.y_filter_shift"
FILTER_LINK_LOW :: -50.0
FILTER_LINK_HIGH :: 50.0

// As the Mac stores it (`filterShiftLink`); anything else is Separately.
filter_link_from_key :: proc "contextless" (key: string) -> Filter_Link {
	switch key {
	case "together":
		return .Together
	case "opposite":
		return .Opposite
	}
	return .Separate
}

FILTER_LINK_KEYS := [Filter_Link]cstring {
	.Separate = "separate",
	.Together = "together",
	.Opposite = "opposite",
}

FILTER_LINK_TITLES := [Filter_Link]cstring {
	.Separate = "Separately",
	.Together = "Together",
	.Opposite = "Opposite ways",
}

filter_link_links :: proc "contextless" (path: string) -> bool {
	return path == FILTER_LINK_MAGENTA || path == FILTER_LINK_YELLOW
}

// The two shifts after moving `path` to `value` (already snapped). The
// other moves by the same change, so their gap (Together) or sum
// (Opposite) holds; at either end of the range both stop.
filter_link_shifts :: proc "contextless" (link: Filter_Link, path: string, v, magenta, yellow: f64) -> (f64, f64) {
	moving_magenta := path == FILTER_LINK_MAGENTA
	current, other := yellow, magenta
	if moving_magenta {
		current, other = magenta, yellow
	}
	if link == .Separate {
		if moving_magenta {
			return v, yellow
		}
		return magenta, v
	}
	sign := 1.0 if link == .Together else -1.0
	// The other's room to move, in the moving slider's direction.
	low, high: f64
	if sign > 0 {
		low, high = FILTER_LINK_LOW - other, FILTER_LINK_HIGH - other
	} else {
		low, high = other - FILTER_LINK_HIGH, other - FILTER_LINK_LOW
	}
	delta := fmin(fmax(v - current, fmin(low, 0.0)), fmax(high, 0.0))
	moved, followed := current + delta, other + sign * delta
	if moving_magenta {
		return moved, followed
	}
	return followed, moved
}
