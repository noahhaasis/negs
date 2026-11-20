// A rating star drawn by hand (round 22, r22-edge.html, A; HandStar.swift;
// windows/marks/src/star.rs): three strokes through the middle, the upright
// one first, each seeded from the file name and the star's place in the row.
package marks

// The box each star is drawn in (round (0, 0)), and the gap between two.
STAR_SIZE :: 12.0
STAR_GAP :: 2.5

@(private = "file")
STAR_PROFILE := [?][2]f64{{0.0, 0.9}, {0.15, 1.75}, {0.85, 1.55}, {1.0, 0.7}}

// The page's seed: the name, "·star" and the star's place, from 0.
star_seed :: proc "contextless" (name: string, index: int) -> u32 {
	digits: [24]u8
	return hash_parts(name, "·star", format_uint(digits[:], u64(index)))
}

// `n` in decimal, in `buf`.
format_uint :: proc "contextless" (buf: []u8, n: u64) -> string {
	i := len(buf)
	n := n
	for {
		i -= 1
		buf[i] = u8('0' + n % 10)
		n /= 10
		if n == 0 {
			break
		}
	}
	return string(buf[i:])
}

// The three strokes of the `index`th star in `name`'s row, round (0, 0),
// each a filled outline: appended to `points`, their lengths to `counts`.
star_strokes :: proc(name: string, index: int, points: ^[dynamic]Pt, counts: ^[dynamic]int) {
	random := Random{star_seed(name, index)}
	scale := lerp(0.9, 1.15, next(&random))
	tilt := jitter(&random, 9.0)
	cx := jitter(&random, 0.5)
	cy := jitter(&random, 0.5)
	for a0 in ([3]f64{90.0, 30.0, 150.0}) {
		a := (a0 + tilt + jitter(&random, 8.0)) * PI / 180.0
		length := lerp(4.6, 5.8, next(&random))
		off := jitter(&random, 0.7)
		sa, ca := sin_cos(a)
		from := Pt{cx + off - ca * length, cy - sa * length}
		to_x := cx + ca * length * lerp(0.85, 1.05, next(&random))
		to_y := cy + sa * length * lerp(0.85, 1.05, next(&random)) + off
		before := len(points)
		star_stroke(from, {to_x, to_y}, 0.25, &random, scale, points)
		append(counts, len(points) - before)
	}
}

// A line from `a` to `b` in 8 steps, bowed a little to one side.
@(private = "file")
star_stroke :: proc(a, b: Pt, bow: f64, random: ^Random, scale: f64, out: ^[dynamic]Pt) {
	length := hypot(b.x - a.x, b.y - a.y)
	l := 1.0 if length == 0.0 else length
	nx, ny := -(b.y - a.y) / l, (b.x - a.x) / l
	q := (next(random) - 0.5) * 2.0 * bow
	points: [9]Pt
	for s in 0 ..= 8 {
		t := f64(s) / 8.0
		d := sin(PI * t) * q
		points[s] = {lerp(a.x, b.x, t) + nx * d, lerp(a.y, b.y, t) + ny * d}
	}
	ribbon(points[:], STAR_PROFILE[:], scale, out)
}
