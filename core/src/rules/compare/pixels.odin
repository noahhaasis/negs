// Compare's pure pixel and geometry helpers from the model's side
// (windows/src/model/compare.rs): halving a large develop, and where the
// camera's JPEG goes over the RAW's frame. The filtered resize itself is the
// image crate's and stays with the host.
package compare

import "core:math"

// `src` (width × height RGBA, rows packed) at half its size, each pixel the
// mean of four (rounded), into `out`: max(width / 2, 1) × max(height / 2, 1).
// Odd edges repeat their last row and column. Width and height at least 1.
halved :: proc "contextless" (src: []u8, width, height: u32, out: []u8) {
	w, h := max(width / 2, 1), max(height / 2, 1)
	stride := int(width) * 4
	for y in 0 ..< int(h) {
		y0 := min(2 * y, int(height) - 1)
		y1 := min(2 * y + 1, int(height) - 1)
		row := out[y * int(w) * 4:]
		for x in 0 ..< int(w) {
			x0 := min(2 * x, int(width) - 1) * 4
			x1 := min(2 * x + 1, int(width) - 1) * 4
			for c in 0 ..< 4 {
				sum := u32(src[y0 * stride + x0 + c]) + u32(src[y0 * stride + x1 + c]) + u32(src[y1 * stride + x0 + c]) + u32(src[y1 * stride + x1 + c])
				row[x * 4 + c] = u8((sum + 2) / 4)
			}
		}
	}
}

// Rust's `as u32` from f64: NaN and below zero 0, saturating at the top.
as_u32 :: proc "contextless" (x: f64) -> u32 {
	if !(x > 0.0) {
		return 0
	}
	if x >= 4294967296.0 {
		return max(u32)
	}
	return u32(x)
}

// How the camera's JPEG (width × height) is laid over the RAW's frame, so
// it lines up with the developed image (negs_compare_over_frame_t): the
// picture made is `out` (its long side the JPEG's, the frame's shape); one
// of the same shape (within 1%) is stretched to it (`same`), another is
// resized to `size` and sits at `at` on black.
Over_Frame :: struct {
	out:  [2]u32,
	size: [2]u32,
	at:   [2]i64,
	same: bool,
}

over_frame :: proc "contextless" (width, height: u32, frame: [2]u32) -> (plan: Over_Frame) {
	aspect := f64(frame[0]) / f64(max(frame[1], 1))
	long := f64(max(width, height))
	if aspect >= 1.0 {
		plan.out = {as_u32(long), as_u32(math.round(long / aspect))}
	} else {
		plan.out = {as_u32(math.round(long * aspect)), as_u32(long)}
	}
	plan.same = abs((f64(width) / f64(height)) / aspect - 1.0) < 0.01
	if plan.same {
		return
	}
	scale := min(f64(plan.out[0]) / f64(width), f64(plan.out[1]) / f64(height))
	plan.size = {max(as_u32(math.round(f64(width) * scale)), 1), max(as_u32(math.round(f64(height) * scale)), 1)}
	plan.at = {i64((plan.out[0] - min(plan.size[0], plan.out[0])) / 2), i64((plan.out[1] - min(plan.size[1], plan.out[1])) / 2)}
	return
}
