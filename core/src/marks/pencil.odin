// The pencil stroke under a word and the rings (IMPLEMENTATION.md §2.1;
// PencilStroke and PencilRing in Theme.swift; windows/marks/src/pencil.rs).
package marks

// The stroke's width: 2.1 pt, not scaled with the stroke.
PENCIL_WIDTH :: 2.1
// The box the stroke is stretched to is this tall, under the baseline.
PENCIL_HEIGHT :: 6.0
// The highlighter's width (Light table): an 11 pt band, less its round ends.
HIGHLIGHTER_WIDTH :: 8.5
// A ring's line.
RING_LINE_WIDTH :: 2.0
// How far a ring sits outside the picture it rings (width, height).
RING_OUTSET_W :: 7.0
RING_OUTSET_H :: 6.0

// The one hand-drawn stroke: the SVG path
// `M1 3.6 C 22 1.2, 47 5.2, 70 2.6 S 93 2.2, 99 3.4` in a 100 × 6 box,
// stretched to `rect`. Plain draws a straight line through the middle.
pencil :: proc "contextless" (rect: Rect, plain: bool) -> (segs: [3]Seg, count: int) {
	if plain {
		segs[0] = move({rect.x + 1.0, mid_y(rect)})
		segs[1] = line({max_x(rect) - 1.0, mid_y(rect)})
		return segs, 2
	}
	sx, sy := rect.w / 100.0, rect.h / 6.0
	p :: proc "contextless" (rect: Rect, sx, sy, x, y: f64) -> Pt {return {rect.x + x * sx, rect.y + y * sy}}
	segs[0] = move(p(rect, sx, sy, 1.0, 3.6))
	segs[1] = cubic(p(rect, sx, sy, 22.0, 1.2), p(rect, sx, sy, 47.0, 5.2), p(rect, sx, sy, 70.0, 2.6))
	// S: the first control point mirrors (47, 5.2) about (70, 2.6).
	segs[2] = cubic(p(rect, sx, sy, 93.0, 0.0), p(rect, sx, sy, 93.0, 2.2), p(rect, sx, sy, 99.0, 3.4))
	return segs, 3
}

// A ring's corner radii, as CSS border-radius gives them: horizontal and
// vertical per corner, clockwise from the top left; `relative`: fractions
// of the width and height (CSS %) rather than points.
Radii :: struct {
	horizontal: [4]f64,
	vertical:   [4]f64,
	relative:   bool,
}

// Round a picked frame, tile or thumbnail: `30 40 28 44 / 34 26 40 28`.
RADII_BOX :: Radii{{30.0, 40.0, 28.0, 44.0}, {34.0, 26.0, 40.0, 28.0}, false}
// Round a number: `47% 53% 50% 50% / 56% 44% 58% 42%`.
RADII_NUMBER :: Radii{{0.47, 0.53, 0.50, 0.50}, {0.56, 0.44, 0.58, 0.42}, true}

// The rotation a ring is drawn at, in degrees; none in Plain.
ring_tilt :: proc "contextless" (degrees: f64, plain: bool) -> f64 {
	return 0.0 if plain else degrees
}

// A loose ellipse drawn in pencil: a rounded rectangle whose corners each
// have their own elliptical radii, scaled down together when they don't
// fit, as CSS does. Plain: a rounded rectangle, or an ellipse round a number.
ring :: proc "contextless" (rect: Rect, radii: Radii, plain: bool) -> [10]Seg {
	if plain {
		r := fmin(rect.w, rect.h) / 2.0 if radii.relative else fmin(fmin(6.0, rect.w / 2.0), rect.h / 2.0)
		return corners(rect, {r, r, r, r}, {r, r, r, r})
	}
	w, h := rect.w, rect.h
	rx, ry := radii.horizontal, radii.vertical
	if radii.relative {
		for i in 0 ..< 4 {
			rx[i] *= w
			ry[i] *= h
		}
	}
	// CSS: scale every radius by the smallest side / (sum of its radii).
	f := fmin(fmin(fmin(fmin(1.0, w / (rx[0] + rx[1])), w / (rx[3] + rx[2])), h / (ry[0] + ry[3])), h / (ry[1] + ry[2]))
	for i in 0 ..< 4 {
		rx[i] *= f
		ry[i] *= f
	}
	return corners(rect, rx, ry)
}

// A rounded rectangle with elliptical corners, each quarter a cubic with
// the usual 0.5523 control length, from the top edge clockwise.
@(private)
corners :: proc "contextless" (rect: Rect, rx, ry: [4]f64) -> [10]Seg {
	// A variable, not a constant: Odin folds constant expressions exactly,
	// where Rust and Swift round `1.0 - k` in f64.
	k: f64 = 0.5522847
	x0, y0, x1, y1 := rect.x, rect.y, max_x(rect), max_y(rect)
	return {
		move({x0 + rx[0], y0}),
		line({x1 - rx[1], y0}),
		cubic({x1 - rx[1] * (1.0 - k), y0}, {x1, y0 + ry[1] * (1.0 - k)}, {x1, y0 + ry[1]}),
		line({x1, y1 - ry[2]}),
		cubic({x1, y1 - ry[2] * (1.0 - k)}, {x1 - rx[2] * (1.0 - k), y1}, {x1 - rx[2], y1}),
		line({x0 + rx[3], y1}),
		cubic({x0 + rx[3] * (1.0 - k), y1}, {x0, y1 - ry[3] * (1.0 - k)}, {x0, y1 - ry[3]}),
		line({x0, y0 + ry[0]}),
		cubic({x0, y0 + ry[0] * (1.0 - k)}, {x0 + rx[0] * (1.0 - k), y0}, {x0 + rx[0], y0}),
		close(),
	}
}

// The smallest box round the path itself (Bézier extremes, not control
// points), as boundingBoxOfPath; ok is false when the path is empty.
bounds :: proc "contextless" (segs: []Seg) -> (r: Rect, ok: bool) {
	x0, y0, x1, y1: f64
	add :: proc "contextless" (ok: ^bool, x0, y0, x1, y1: ^f64, p: Pt) {
		if !ok^ {
			x0^, y0^, x1^, y1^ = p.x, p.y, p.x, p.y
			ok^ = true
		} else {
			x0^, y0^ = fmin(x0^, p.x), fmin(y0^, p.y)
			x1^, y1^ = fmax(x1^, p.x), fmax(y1^, p.y)
		}
	}
	at := Pt{0, 0}
	for seg in segs {
		switch seg.kind {
		case .Move, .Line:
			add(&ok, &x0, &y0, &x1, &y1, seg.p[0])
			at = seg.p[0]
		case .Cubic:
			c1, c2, p := seg.p[0], seg.p[1], seg.p[2]
			add(&ok, &x0, &y0, &x1, &y1, p)
			ts := cubic_extremes(at, c1, c2, p)
			for t in ts {
				if t > 0.0 && t < 1.0 {
					add(&ok, &x0, &y0, &x1, &y1, cubic_at(at, c1, c2, p, t))
				}
			}
			at = p
		case .Close:
		}
	}
	if !ok {
		return {}, false
	}
	return {x0, y0, x1 - x0, y1 - y0}, true
}

// The bounds of a closed polygon through `points` (Path::polygon(..).bounds()).
polygon_bounds :: proc "contextless" (points: []Pt) -> (r: Rect, ok: bool) {
	if len(points) == 0 {
		return {}, false
	}
	x0, y0, x1, y1 := points[0].x, points[0].y, points[0].x, points[0].y
	for p in points[1:] {
		x0, y0 = fmin(x0, p.x), fmin(y0, p.y)
		x1, y1 = fmax(x1, p.x), fmax(y1, p.y)
	}
	return {x0, y0, x1 - x0, y1 - y0}, true
}

@(private)
cubic_at :: proc "contextless" (a, b, c, d: Pt, t: f64) -> Pt {
	u := 1.0 - t
	f :: proc "contextless" (u, t, a, b, c, d: f64) -> f64 {
		return u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
	}
	return {f(u, t, a.x, b.x, c.x, d.x), f(u, t, a.y, b.y, c.y, d.y)}
}

// The parameters where a cubic turns in x or y (NaN where there's none);
// the caller keeps those in (0, 1).
@(private)
cubic_extremes :: proc "contextless" (a, b, c, d: Pt) -> [4]f64 {
	roots :: proc "contextless" (a, b, c, d: f64) -> [2]f64 {
		// The derivative's coefficients, over 3.
		qa, qb, qc := -a + 3.0 * b - 3.0 * c + d, 2.0 * (a - 2.0 * b + c), b - a
		NAN :: 0h7FF8_0000_0000_0000
		out := [2]f64{NAN, NAN}
		if abs(qa) < 1e-12 {
			if abs(qb) > 1e-12 {
				out[0] = -qc / qb
			}
		} else {
			disc := qb * qb - 4.0 * qa * qc
			if disc >= 0.0 {
				s := sqrt(disc)
				out = {(-qb + s) / (2.0 * qa), (-qb - s) / (2.0 * qa)}
			}
		}
		return out
	}
	x := roots(a.x, b.x, c.x, d.x)
	y := roots(a.y, b.y, c.y, d.y)
	return {x[0], x[1], y[0], y[1]}
}
