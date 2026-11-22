// A presentation crop's geometry (windows/src/crop.rs, Crop.swift): points,
// sizes and rectangles in a photograph's pixels or fractions, y down, and
// the rules for dragging, fitting and straightening a crop rectangle.
package crop

Point :: struct {
	x, y: f64,
}

Size :: struct {
	width, height: f64,
}

// Laid out as negs_rect_t.
Rect :: struct {
	x, y, width, height: f64,
}

UNIT :: Rect{0, 0, 1, 1}

// Field by field, as Rust's PartialEq: -0 equals 0, a NaN nothing.
rect_eq :: proc "contextless" (a, b: Rect) -> bool {
	return a.x == b.x && a.y == b.y && a.width == b.width && a.height == b.height
}

of_size :: proc "contextless" (s: Size) -> Rect {return Rect{0, 0, s.width, s.height}}
max_x :: proc "contextless" (r: Rect) -> f64 {return r.x + r.width}
max_y :: proc "contextless" (r: Rect) -> f64 {return r.y + r.height}
mid_x :: proc "contextless" (r: Rect) -> f64 {return r.x + r.width / 2.0}
mid_y :: proc "contextless" (r: Rect) -> f64 {return r.y + r.height / 2.0}
size :: proc "contextless" (r: Rect) -> Size {return Size{r.width, r.height}}
swapped :: proc "contextless" (s: Size) -> Size {return Size{s.height, s.width}}
is_zero :: proc "contextless" (s: Size) -> bool {return s.width == 0 && s.height == 0}

inset :: proc "contextless" (r: Rect, dx, dy: f64) -> Rect {
	return Rect{r.x + dx, r.y + dy, r.width - 2.0 * dx, r.height - 2.0 * dy}
}

contains :: proc "contextless" (r: Rect, p: Point) -> bool {
	return p.x >= r.x && p.x < max_x(r) && p.y >= r.y && p.y < max_y(r)
}

intersection :: proc "contextless" (a, b: Rect) -> Rect {
	x0 := fmax(a.x, b.x)
	y0 := fmax(a.y, b.y)
	x1 := fmin(max_x(a), max_x(b))
	y1 := fmin(max_y(a), max_y(b))
	return Rect{x0, y0, fmax(x1 - x0, 0), fmax(y1 - y0, 0)}
}

offset :: proc "contextless" (r: Rect, dx, dy: f64) -> Rect {
	return Rect{r.x + dx, r.y + dy, r.width, r.height}
}

// An edge or corner: -1 the left or top, 1 the right or bottom, 0 neither.
// Both zero moves the rectangle.
Handle :: struct {
	x, y: i32,
}

MOVE :: Handle{0, 0}

clamp :: proc "contextless" (value, low, high: f64) -> f64 {
	return fmin(fmax(value, low), fmax(low, high))
}

// The largest `ratio` (width over height) rectangle in `bounds`, centred on
// `center` as far as it fits.
fit :: proc "contextless" (ratio: f64, center: Point, bounds: Rect) -> Rect {
	width := fmin(bounds.width, bounds.height * ratio)
	height := width / ratio
	return Rect {
		clamp(center.x - width / 2.0, bounds.x, max_x(bounds) - width),
		clamp(center.y - height / 2.0, bounds.y, max_y(bounds) - height),
		width,
		height,
	}
}

move_rect :: proc "contextless" (rect: Rect, dx, dy: f64, bounds: Rect) -> Rect {
	return Rect {
		clamp(rect.x + dx, bounds.x, max_x(bounds) - rect.width),
		clamp(rect.y + dy, bounds.y, max_y(bounds) - rect.height),
		rect.width,
		rect.height,
	}
}

@(private = "file")
origin :: proc "contextless" (side: i32, anchor, length, mid, low, high: f64) -> f64 {
	if side > 0 {
		return anchor
	} else if side < 0 {
		return anchor - length
	}
	return clamp(mid - length / 2.0, low, high - length)
}

// Drags `handle` of `rect` to `point`. The opposite edges stay put; with a
// ratio, an edge drag grows the other side about its centre, and a corner
// follows whichever side the pointer pulls further.
resize :: proc "contextless" (rect: Rect, handle: Handle, point: Point, ratio: Maybe(f64), bounds: Rect, minimum: f64) -> Rect {
	p := Point{clamp(point.x, bounds.x, max_x(bounds)), clamp(point.y, bounds.y, max_y(bounds))}
	anchor := Point{max_x(rect) if handle.x < 0 else rect.x, max_y(rect) if handle.y < 0 else rect.y}
	room: Size
	if handle.x > 0 {
		room.width = max_x(bounds) - anchor.x
	} else if handle.x < 0 {
		room.width = anchor.x - bounds.x
	} else {
		room.width = bounds.width
	}
	if handle.y > 0 {
		room.height = max_y(bounds) - anchor.y
	} else if handle.y < 0 {
		room.height = anchor.y - bounds.y
	} else {
		room.height = bounds.height
	}
	pulled := Size{fmax(f64(handle.x) * (p.x - anchor.x), 0), fmax(f64(handle.y) * (p.y - anchor.y), 0)}
	sz := size(rect)
	if r, ok := ratio.?; ok {
		width: f64 = pulled.width if handle.x != 0 else 0
		if handle.y != 0 {
			width = fmax(width, pulled.height * r)
		}
		smallest := fmax(minimum, minimum * r)
		width = fmin(fmax(width, smallest), fmin(room.width, room.height * r))
		sz = Size{width, width / r}
	} else {
		if handle.x != 0 {
			sz.width = fmin(fmax(pulled.width, fmin(minimum, room.width)), room.width)
		}
		if handle.y != 0 {
			sz.height = fmin(fmax(pulled.height, fmin(minimum, room.height)), room.height)
		}
	}
	return Rect {
		origin(handle.x, anchor.x, sz.width, mid_x(rect), bounds.x, max_x(bounds)),
		origin(handle.y, anchor.y, sz.height, mid_y(rect), bounds.y, max_y(bounds)),
		sz.width,
		sz.height,
	}
}

@(private = "file")
side :: proc "contextless" (value, low, high, tolerance: f64) -> Maybe(i32) {
	if fabs(value - low) <= tolerance {
		return -1
	} else if fabs(value - high) <= tolerance {
		return 1
	}
	return nil
}

// The handle under `point`, for a crop drawn at `frame` (both in view
// pixels); `scale` is the view's pixels per DIP. None outside its reach.
handle_at :: proc "contextless" (point: Point, frame: Rect, scale: f64) -> Maybe(Handle) {
	reach := fmin(fmin(18.0 * scale, frame.width / 3.0), frame.height / 3.0)
	edge := 8.0 * scale
	outer := inset(frame, -edge, -edge)
	if !(point.x >= outer.x && point.x <= max_x(outer) && point.y >= outer.y && point.y <= max_y(outer)) {
		return nil
	}
	cx, okx := side(point.x, frame.x, max_x(frame), reach).?
	cy, oky := side(point.y, frame.y, max_y(frame), reach).?
	if okx && oky {
		return Handle{cx, cy}
	}
	if x, ok := side(point.x, frame.x, max_x(frame), edge).?; ok {
		return Handle{x, 0}
	}
	if y, ok := side(point.y, frame.y, max_y(frame), edge).?; ok {
		return Handle{0, y}
	}
	return MOVE
}

// `point` turned `degrees` clockwise (y down) about `center`.
turn :: proc "contextless" (point: Point, degrees: f64, center: Point) -> Point {
	pi: f64 = PI
	sin, cos := sin_cos(degrees * pi / 180.0)
	dx, dy := point.x - center.x, point.y - center.y
	return Point{center.x + dx * cos - dy * sin, center.y + dx * sin + dy * cos}
}

// The corners of a `photo` turned by `angle`, clockwise from the top left.
corners :: proc "contextless" (photo: Size, angle: f64) -> [4]Point {
	center := Point{photo.width / 2.0, photo.height / 2.0}
	return [4]Point {
		turn(Point{0, 0}, angle, center),
		turn(Point{photo.width, 0}, angle, center),
		turn(Point{photo.width, photo.height}, angle, center),
		turn(Point{0, photo.height}, angle, center),
	}
}

// The turned photograph's bounding box.
bounds :: proc "contextless" (photo: Size, angle: f64) -> Rect {
	points := corners(photo, angle)
	inf := transmute(f64)u64(0x7FF0_0000_0000_0000)
	x0, x1, y0, y1 := inf, -inf, inf, -inf
	for p in points {
		x0 = fmin(x0, p.x)
	}
	for p in points {
		x1 = fmax(x1, p.x)
	}
	for p in points {
		y0 = fmin(y0, p.y)
	}
	for p in points {
		y1 = fmax(y1, p.y)
	}
	return Rect{x0, y0, x1 - x0, y1 - y0}
}

// Whether `rect` lies on the photograph turned by `angle`.
fits :: proc "contextless" (rect: Rect, angle: f64, photo: Size) -> bool {
	center := Point{photo.width / 2.0, photo.height / 2.0}
	tolerance := 1e-6 * fmax(fmax(photo.width, photo.height), 1.0)
	cs := [4]Point{{rect.x, rect.y}, {max_x(rect), rect.y}, {max_x(rect), max_y(rect)}, {rect.x, max_y(rect)}}
	for corner in cs {
		p := turn(corner, -angle, center)
		if !(p.x >= -tolerance && p.y >= -tolerance && p.x <= photo.width + tolerance && p.y <= photo.height + tolerance) {
			return false
		}
	}
	return true
}

@(private = "file")
lerp :: proc "contextless" (start, target: Rect, t: f64) -> Rect {
	return Rect {
		start.x + (target.x - start.x) * t,
		start.y + (target.y - start.y) * t,
		start.width + (target.width - start.width) * t,
		start.height + (target.height - start.height) * t,
	}
}

// The furthest step from `start` towards `target` that stays on the turned photograph.
constrain :: proc "contextless" (start, target: Rect, angle: f64, photo: Size) -> Rect {
	if fits(target, angle, photo) {
		return target
	}
	if !fits(start, angle, photo) {
		return start
	}
	low, high: f64 = 0, 1
	for _ in 0 ..< 24 {
		mid := (low + high) / 2.0
		if fits(lerp(start, target, mid), angle, photo) {
			low = mid
		} else {
			high = mid
		}
	}
	return lerp(start, target, low)
}

@(private = "file")
inscribed_scale :: proc "contextless" (middle: Point, half: [4]Point, angle: f64, center: Point, photo: Size) -> Maybe(f64) {
	q := turn(middle, -angle, center)
	if !(q.x >= 0 && q.y >= 0 && q.x <= photo.width && q.y <= photo.height) {
		return nil
	}
	s: f64 = 1.0
	for v in half {
		if v.x > 0 {
			s = fmin(s, (photo.width - q.x) / v.x)
		} else if v.x < 0 {
			s = fmin(s, q.x / -v.x)
		}
		if v.y > 0 {
			s = fmin(s, (photo.height - q.y) / v.y)
		} else if v.y < 0 {
			s = fmin(s, q.y / -v.y)
		}
	}
	return fmax(s, 0)
}

// `rect` shrunk about its centre, never grown, until it lies on the
// photograph turned by `angle`. A centre off the turned photograph moves
// towards its centre, as little as keeps nearly the largest size.
inscribe :: proc "contextless" (rect: Rect, angle: f64, photo: Size) -> Rect {
	center := Point{photo.width / 2.0, photo.height / 2.0}
	o := Point{}
	half := [4]Point {
		turn(Point{-rect.width / 2.0, -rect.height / 2.0}, -angle, o),
		turn(Point{rect.width / 2.0, -rect.height / 2.0}, -angle, o),
		turn(Point{rect.width / 2.0, rect.height / 2.0}, -angle, o),
		turn(Point{-rect.width / 2.0, rect.height / 2.0}, -angle, o),
	}
	middles: [21]Point
	scales: [21]f64
	for i in 0 ..= 20 {
		t := f64(i) / 20.0
		middles[i] = Point{mid_x(rect) + (center.x - mid_x(rect)) * t, mid_y(rect) + (center.y - mid_y(rect)) * t}
		scales[i] = inscribed_scale(middles[i], half, angle, center, photo).? or_else 0
	}
	best: f64 = 0
	for s in scales {
		best = fmax(best, s)
	}
	if !(best > 0) {
		return rect
	}
	for s, i in scales {
		if s >= best * 0.98 {
			middle := middles[i]
			sz := Size{rect.width * s, rect.height * s}
			return Rect{middle.x - sz.width / 2.0, middle.y - sz.height / 2.0, sz.width, sz.height}
		}
	}
	return rect
}
