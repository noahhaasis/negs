// A box drawn in grease pencil round a picture (rounds 20 and 21,
// r20-box.html, B; PickBox.swift; windows/marks/src/pick.rs): a pick's box,
// and in the grid selection's second box over it. Each name draws one of
// three:
//
// - Four: four separate sides, crossing at the corners.
// - Loop: one stroke round, its end running past its start.
// - Two: down and across, then across and up, meeting at the corners.
package marks

// How far outside the picture a box may draw: the yellow box's 8 pt, a
// corner's jitter, an overshoot of up to 12 pt, the stroke's width and
// wobble, and the tilt on a wide picture.
REACH :: 32.0

Box_Kind :: enum u32 {
	Four = 1,
	Loop = 2,
	Two  = 3,
}

@(private = "file")
BOX_PROFILE := [?][2]f64{{0.0, 1.4}, {0.05, 2.9}, {0.85, 2.6}, {1.0, 0.9}}

// The seed: the name and "·box".
box_seed :: proc "contextless" (name: string) -> u32 {
	return hash_parts(name, "·box")
}

@(private = "file")
kind_of :: proc "contextless" (k: f64) -> Box_Kind {
	if k < 0.36 {
		return .Four
	} else if k < 0.7 {
		return .Loop
	}
	return .Two
}

box_kind :: proc "contextless" (name: string) -> Box_Kind {
	random := Random{box_seed(name)}
	return kind_of(next(&random))
}

// The box round a `w` × `h` picture, `o` outside it. `tight` keeps the
// corners and overshoots small (the strip); `weight` scales the stroke.
// `second`: the selection's box, seeded from name + "·second". Each stroke's
// outline (before the tilt, in the picture's coordinates) is appended to
// `points`, its length to `counts`; returns the kind and the tilt round the
// picture's centre, in degrees, clockwise.
pick_box :: proc(
	w, h, o: f64,
	name: string,
	second: bool,
	tight: bool,
	weight: f64,
	points: ^[dynamic]Pt,
	counts: ^[dynamic]int,
) -> (
	kind: Box_Kind,
	tilt: f64,
) {
	random := Random{hash_parts(name, "·second", "·box") if second else box_seed(name)}
	kind = kind_of(next(&random))
	over, cj := 12.0, 2.6
	if tight {
		over, cj = 4.0, 1.5
	}
	c: [4]Pt
	starts := [4]Pt{{-o, -o}, {w + o, -o}, {w + o, h + o}, {-o, h + o}}
	for s, i in starts {
		jx := jitter(&random, cj)
		jy := jitter(&random, cj)
		c[i] = {s.x + jx, s.y + jy}
	}
	// The legs a stroke runs through, `ways[i]` of `way_len[i]` points.
	ways: [4][6]Pt
	way_len: [4]int
	way_count := 0
	switch kind {
	case .Four:
		for i in 0 ..< 4 {
			a, b := c[i], c[(i + 1) % 4]
			from := ext(a, b, lerp(-2.0, over * 0.6, next(&random)))
			to := ext(b, a, lerp(-1.0, over, next(&random)))
			ways[i][0], ways[i][1] = from, to
			way_len[i] = 2
		}
		way_count = 4
	case .Loop:
		t0 := lerp(0.1, 0.4, next(&random))
		end := at(c[0], c[1], fmin(0.95, t0 + lerp(0.1, 0.3, next(&random))))
		end.y += jitter(&random, 2.5)
		ways[0] = {at(c[0], c[1], t0), c[1], c[2], c[3], c[0], end}
		way_len[0] = 6
		way_count = 1
	case .Two:
		a0 := ext(c[3], c[0], lerp(-2.0, over * 0.5, next(&random)))
		a2 := ext(c[1], c[0], lerp(0.0, over, next(&random)))
		b0 := ext(c[1], c[2], lerp(-4.0, 2.0, next(&random)))
		b2 := ext(c[3], c[2], lerp(0.0, over, next(&random)))
		ways[0][0], ways[0][1], ways[0][2] = a0, c[0], a2
		ways[1][0], ways[1][1], ways[1][2] = b0, c[2], b2
		way_len[0], way_len[1] = 3, 3
		way_count = 2
	}
	scale := lerp(0.85, 1.2, next(&random)) * weight
	tilt = jitter(&random, 0.6)
	wobble := lerp(0.3, 0.9, next(&random))
	period := lerp(70.0, 150.0, next(&random))
	phase := next(&random) * 6.283
	for wi in 0 ..< way_count {
		way := ways[wi][:way_len[wi]]
		// Each leg in steps of about 3 pt, bowed a little.
		centre := make([dynamic]Pt, 0, 64, context.temp_allocator)
		for i in 0 ..< len(way) - 1 {
			a, b := way[i], way[i + 1]
			length := non_zero(hypot(b.x - a.x, b.y - a.y))
			n := max(2, saturating_uint(ceil(length / 3.0)))
			nx, ny := -(b.y - a.y) / length, (b.x - a.x) / length
			bow := jitter(&random, fmin(2.2, length * 0.012))
			for s in 0 ..< n {
				t := f64(s) / f64(n)
				q := sin(PI * t) * bow
				append(&centre, Pt{lerp(a.x, b.x, t) + nx * q, lerp(a.y, b.y, t) + ny * q})
			}
		}
		append(&centre, way[len(way) - 1])
		smoothed := smooth_points(centre[:], 2)
		// A slow wobble across the line, by the distance along it.
		along := 0.0
		wobbled := make([]Pt, len(smoothed), context.temp_allocator)
		for p, i in smoothed {
			if i > 0 {
				along += hypot(p.x - smoothed[i - 1].x, p.y - smoothed[i - 1].y)
			}
			q, u := smoothed[max(i, 1) - 1], smoothed[min(i + 1, len(smoothed) - 1)]
			tx, ty := u.x - q.x, u.y - q.y
			tl := non_zero(hypot(tx, ty))
			d := wobble * sin(6.283 * along / period + phase)
			wobbled[i] = {p.x - ty / tl * d, p.y + tx / tl * d}
		}
		before := len(points)
		ribbon(wobbled, BOX_PROFILE[:], scale, points)
		append(counts, len(points) - before)
	}
	return kind, tilt
}

// `p` moved `d` further from `q`.
@(private = "file")
ext :: proc "contextless" (p, q: Pt, d: f64) -> Pt {
	dx, dy := p.x - q.x, p.y - q.y
	l := non_zero(hypot(dx, dy))
	return {p.x + dx / l * d, p.y + dy / l * d}
}

@(private = "file")
at :: proc "contextless" (a, b: Pt, t: f64) -> Pt {
	return {lerp(a.x, b.x, t), lerp(a.y, b.y, t)}
}

// Rust's `x as usize`: saturating, NaN to 0.
@(private = "file")
saturating_uint :: proc "contextless" (x: f64) -> int {
	if !(x > 0.0) {
		return 0
	}
	if x >= f64(max(int)) {
		return max(int)
	}
	return int(x)
}

// Each point but the ends moved to the mean of the `k` either side of it.
@(private = "file")
smooth_points :: proc(p: []Pt, k: int) -> []Pt {
	out := make([]Pt, len(p), context.temp_allocator)
	for i in 0 ..< len(p) {
		if i == 0 || i == len(p) - 1 {
			out[i] = p[i]
			continue
		}
		x, y, count := 0.0, 0.0, 0.0
		for q in p[max(i, k) - k:min(i + k, len(p) - 1) + 1] {
			x += q.x
			y += q.y
			count += 1.0
		}
		out[i] = {x / count, y / count}
	}
	return out
}
