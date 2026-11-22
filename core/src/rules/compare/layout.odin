// Compare's geometry (`CompareArrangement`), in DIPs: the two frames in a
// row or one above the other, whichever shows them larger, and the slider's
// one frame. Rust's f64 semantics: Odin's min and max are its minnum and
// maxnum; sums start from −0. Every constant is exact in binary.
package compare

import "core:math"

// Above each photograph in a row: its caption; below: its label.
CAPTION :: 54.0
LABEL :: 68.0
// Stacked, only a caption line.
STACKED_CAPTION :: 40.0
// Between the two, and round the canvas's edges.
GAP :: 28.0
STACKED_GAP :: 14.0
SIDE :: 36.0
BOTTOM :: 12.0

// x, y, width, height (negs_rect_t).
Frame :: [4]f64

Arrangement :: enum u32 {
	Row,
	Column,
}

// The largest size of `aspect` (width ÷ height; 1.5 if it isn't a positive
// number) that fits `room`.
fit :: proc "contextless" (aspect: f64, room: [2]f64) -> [2]f64 {
	aspect := aspect if !math.is_nan(aspect) && !math.is_inf(aspect) && aspect > 0.0 else 1.5
	if room[0] / room[1] > aspect {
		return {room[1] * aspect, room[1]}
	}
	return {room[0], room[0] / aspect}
}

row_layout :: proc "contextless" (canvas, aspects: [2]f64) -> [2]Frame {
	width := max((canvas[0] - 2.0 * SIDE - GAP) / 2.0, 1.0)
	height := max(canvas[1] - CAPTION - LABEL, 1.0)
	frames: [2]Frame
	for i in 0 ..< 2 {
		size := fit(aspects[i], {width, height})
		x := SIDE + f64(i) * (width + GAP) + (width - size[0]) / 2.0
		frames[i] = {math.round(x), math.round(CAPTION + (height - size[1]) / 2.0), math.round(size[0]), math.round(size[1])}
	}
	return frames
}

column_layout :: proc "contextless" (canvas, aspects: [2]f64) -> [2]Frame {
	width := max(canvas[0] - 2.0 * SIDE, 1.0)
	height := max((canvas[1] - 2.0 * STACKED_CAPTION - STACKED_GAP - BOTTOM) / 2.0, 1.0)
	sizes := [2][2]f64{fit(aspects[0], {width, height}), fit(aspects[1], {width, height})}
	// The pair sits in the middle of the canvas, as one block.
	block := 2.0 * STACKED_CAPTION + STACKED_GAP + sizes[0][1] + sizes[1][1]
	y := max((canvas[1] - BOTTOM - block) / 2.0, 0.0)
	frames: [2]Frame
	for size, i in sizes {
		y += STACKED_CAPTION
		frames[i] = {math.round((canvas[0] - size[0]) / 2.0), math.round(y), math.round(size[0]), math.round(size[1])}
		y += size[1] + STACKED_GAP
	}
	return frames
}

@(private = "file")
area :: proc "contextless" (frames: [2]Frame) -> f64 {
	sum := transmute(f64)u64(0x8000_0000_0000_0000) // Rust's sum starts from −0.
	for f in frames {
		sum += f[2] * f[3]
	}
	return sum
}

// The arrangement that shows the two larger: in a row, or one above the other.
layout :: proc "contextless" (canvas, aspects: [2]f64) -> (Arrangement, [2]Frame) {
	row, column := row_layout(canvas, aspects), column_layout(canvas, aspects)
	if area(column) > area(row) {
		return .Column, column
	}
	return .Row, row
}

// The slider's one frame, at the first photograph's proportions.
slider_frame :: proc "contextless" (canvas: [2]f64, aspect: f64) -> Frame {
	room := [2]f64{max(canvas[0] - 2.0 * SIDE, 1.0), max(canvas[1] - CAPTION - LABEL, 1.0)}
	size := fit(aspect, room)
	return {math.round((canvas[0] - size[0]) / 2.0), math.round(CAPTION + (room[1] - size[1]) / 2.0), math.round(size[0]), math.round(size[1])}
}
