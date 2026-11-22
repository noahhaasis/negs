// How the cropped photograph is shown (`io.flip`, `io.rotate_cw`): mirrored
// left to right if `flip`, then turned `turns` quarter turns clockwise. A
// mirror top to bottom is a flip and a half turn, so one Flip covers both
// (`Orientation` in Crop.swift).
package crop

// Laid out as negs_orientation_t.
Orientation :: struct {
	turns:    i32,
	flip:     bool,
	reserved: [3]u8,
}

orientation :: proc "contextless" (turns: i32, flip: bool) -> Orientation {
	return Orientation{turns = rem4(turns), flip = flip}
}

// Mirrored left to right as shown: a flip conjugates the turns, so a
// quarter turn becomes three.
flipped_as_shown :: proc "contextless" (o: Orientation) -> Orientation {
	return orientation(4 - o.turns, !o.flip)
}

// Where a point at `fraction` of the cropped photograph (unmirrored,
// unturned, from its top left) is shown, as a fraction of the shown photograph.
shown_point :: proc "contextless" (o: Orientation, fraction: Point) -> Point {
	p := Point{1.0 - fraction.x, fraction.y} if o.flip else fraction
	for _ in 0 ..< o.turns {
		p = Point{1.0 - p.y, p.x}
	}
	return p
}

// The inverse of `shown_point`.
unshown :: proc "contextless" (o: Orientation, fraction: Point) -> Point {
	p := fraction
	for _ in 0 ..< o.turns {
		p = Point{p.y, 1.0 - p.x}
	}
	return Point{1.0 - p.x, p.y} if o.flip else p
}

@(private = "file")
in_unit :: proc "contextless" (v: f64) -> bool {
	return 0 <= v && v <= 1
}

// A point at `at` (fractions of the photograph as the canvas shows it
// through `crop`) back to a fraction of the whole, uncropped photograph:
// through the crop, its straightening (turned back about the photograph's
// centre, as the canvas's shader samples it), the turns and the flip.
// `turned` is the whole photograph's size as shown, after the turns. None
// outside the photograph.
unshown_point :: proc "contextless" (o: Orientation, crop: Settings, turned: Size, at: [2]f64) -> Maybe(Point) {
	if !in_unit(at[0]) || !in_unit(at[1]) || turned.width <= 0 || turned.height <= 0 {
		return nil
	}
	r := crop.rect
	a, b := shown_point(o, Point{r.x, r.y}), shown_point(o, Point{r.x + r.width, r.y + r.height})
	x0, y0, x1, y1 := fmin(a.x, b.x), fmin(a.y, b.y), fmax(a.x, b.x), fmax(a.y, b.y)
	// In the turned photograph's pixels, about its centre.
	cx, cy := turned.width / 2.0, turned.height / 2.0
	dx, dy := (x0 + at[0] * (x1 - x0)) * turned.width - cx, (y0 + at[1] * (y1 - y0)) * turned.height - cy
	// Straightened in the unflipped photograph: the other way round as shown.
	angle := -crop.angle if o.flip else crop.angle
	s, k := sin_cos(to_radians(angle))
	g := Point{(cx + dx * k + dy * s) / turned.width, (cy - dx * s + dy * k) / turned.height}
	p := unshown(o, g)
	if in_unit(p.x) && in_unit(p.y) {
		return p
	}
	return nil
}
