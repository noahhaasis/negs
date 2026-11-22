// The grid's fade (Windows only: the Mac's grid flies out of the strip),
// the cubics the compositor animates it with, the wheel's glide and the
// overlay scroll bar.
package sheet

// The sheet `along` ms into its fade (closing plays it backwards): its
// opacity, and how far below its place it is (DIPs), settling on
// `cubic-bezier(.22, .8, .24, 1)`.
sheet_look :: proc "contextless" (along: f64) -> (opacity, rise: f64) {
	p := clampf(along / FADE_MS, 0.0, 1.0)
	return p, FADE_RISE * (1.0 - cubic_bezier(0.22, 0.8, 0.24, 1.0, p))
}

// A CSS cubic Bézier timing function at `t` (0…1).
cubic_bezier :: proc "contextless" (x1, y1, x2, y2, time: f64) -> f64 {
	t := clampf(time, 0.0, 1.0)
	if t == 0.0 || t == 1.0 {
		return t
	}
	cx, bx := 3.0 * x1, 3.0 * (x2 - x1) - 3.0 * x1
	ax := 1.0 - cx - bx
	cy, by := 3.0 * y1, 3.0 * (y2 - y1) - 3.0 * y1
	ay := 1.0 - cy - by
	x :: #force_inline proc "contextless" (ax, bx, cx, s: f64) -> f64 {return ((ax * s + bx) * s + cx) * s}
	dx :: #force_inline proc "contextless" (ax, bx, cx, s: f64) -> f64 {return (3.0 * ax * s + 2.0 * bx) * s + cx}
	// Newton, then bisection where the slope is too flat.
	s := t
	for _ in 0 ..< 8 {
		d := dx(ax, bx, cx, s)
		if abs(d) < 1e-6 {
			break
		}
		next := s - (x(ax, bx, cx, s) - t) / d
		if abs(next - s) < 1e-7 {
			s = next
			break
		}
		s = next
	}
	if !(s >= 0.0 && s <= 1.0) || abs(x(ax, bx, cx, s) - t) > 1e-5 {
		low, high := 0.0, 1.0
		s = t
		for _ in 0 ..< 40 {
			if x(ax, bx, cx, s) < t {
				low = s
			} else {
				high = s
			}
			s = (low + high) / 2.0
		}
	}
	return ((ay * s + by) * s + cy) * s
}

// A curve sampled by `cubic_pieces`: the host's function of time.
Curve :: proc "c" (user: rawptr, t: f64) -> f64

// `f` over `0…length` as `pieces` cubics (at least one), for the
// compositor's animations: each piece's start and its coefficients in time
// since it (`c0 + c1·u + c2·u² + c3·u³`), meeting `f` and its slope at both
// ends, into `out` (five per piece).
cubic_pieces :: proc "contextless" (f: Curve, user: rawptr, length: f64, count: uint, out: [][5]f64) {
	pieces := max(count, 1)
	h := length / f64(pieces)
	eps := h * 1e-3
	for i in 0 ..< pieces {
		t0, t1 := f64(i) * h, f64(i + 1) * h
		v0 := f(user, t0)
		v1 := f(user, t1)
		// Slopes from inside the piece, so a kink at its ends stays sharp.
		d0 := (f(user, t0 + eps) - v0) / eps
		d1 := (v1 - f(user, t1 - eps)) / eps
		c2 := (3.0 * (v1 - v0) / h - 2.0 * d0 - d1) / h
		c3 := (2.0 * (v0 - v1) / h + d0 + d1) / (h * h)
		out[i] = {t0, v0, d0, c2, c3}
	}
}

// Where a wheel's glide from `at` towards `to` is `dt_ms` later; `to` once
// within half a DIP.
wheel_glide :: proc "contextless" (at, to, dt_ms: f64) -> f64 {
	next := at + (to - at) * (1.0 - expf(-max(dt_ms, 0.0) / WHEEL_TAU_MS))
	return to if abs(to - next) < 0.5 else next
}

// The knob's top and length in a track `viewport` long, for a sheet
// `content` high scrolled to `scroll`; false when it all fits.
bar_knob :: proc "contextless" (scroll, content, viewport: f64) -> (top, length: f64, ok: bool) {
	track := viewport - 2.0 * BAR_INSET
	if content <= viewport + 0.5 || track <= BAR_MIN {
		return 0, 0, false
	}
	length = min(max(track * viewport / content, BAR_MIN), track)
	room := max(content - viewport, 1.0)
	top = BAR_INSET + (track - length) * clampf(scroll / room, 0.0, 1.0)
	return top, length, true
}

// The scroll that puts the knob's top at `top` (dragging it).
bar_scroll :: proc "contextless" (top, content, viewport: f64) -> f64 {
	_, length, ok := bar_knob(0.0, content, viewport)
	if !ok {
		return 0.0
	}
	track := viewport - 2.0 * BAR_INSET - length
	room := max(content - viewport, 0.0)
	return 0.0 if track <= 0.0 else clampf((top - BAR_INSET) / track, 0.0, 1.0) * room
}

// The bar's opacity `since_ms` after the grid last scrolled.
bar_opacity :: proc "contextless" (since_ms: f64) -> f64 {
	if since_ms <= BAR_SHOWN_MS {
		return 1.0
	}
	return clampf(1.0 - (since_ms - BAR_SHOWN_MS) / BAR_FADE_MS, 0.0, 1.0)
}
