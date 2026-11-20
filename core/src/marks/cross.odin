// A reject's cross (round 14, r14-reject.html; RejectCross.swift;
// windows/marks/src/cross.rs): one of two hand-drawn crosses, chosen from
// the file name, with its own bow, wobble, weight and retrace.
//
// - Loose: drawn fast. Each stroke starts short of one corner, runs past the
//   other and ends in a flick; the second is a little lighter.
// - Twice (gone over twice): a thinner line traced again just beside itself.
package marks

Cross_Kind :: enum u32 {
	Loose = 1,
	Twice = 2,
}

// One way of drawing the cross. Lengths are in points.
@(private = "file")
Style :: struct {
	inset, jitter:      f64,
	// How far short of its first corner a stroke starts (+), as a range.
	start:              Maybe([2]f64),
	// How far short of its last corner a stroke ends: − runs past it.
	end:                Maybe([2]f64),
	// The bow at the middle, as a share of the stroke's length.
	bow:                f64,
	// The chance a stroke bows the other way.
	bow_flip:           f64,
	wobble:             f64,
	passes:             int,
	retrace:            f64,
	pass_opacity:       f64,
	profile:            Profile,
	scale, opacity:     f64,
	// The second stroke's width scale and opacity, if they differ.
	second:             Maybe([2]f64),
}

@(private = "file")
LOOSE_PROFILE := [?][2]f64{{0.0, 2.2}, {0.1, 3.0}, {0.7, 2.4}, {1.0, 0.2}}
@(private = "file")
TWICE_PROFILE := [?][2]f64{{0.0, 0.7}, {0.12, 1.7}, {0.8, 1.5}, {1.0, 0.4}}

// The style a name draws, with its own spread (the page's `mixed`).
@(private = "file")
style_of :: proc "contextless" (random: ^Random) -> (Cross_Kind, Style) {
	if next(random) < 0.5 {
		style := Style {
			inset        = 12.0,
			jitter       = 3.0,
			start        = [2]f64{10.0, 24.0},
			end          = [2]f64{-14.0, -5.0},
			bow          = 0.0,
			bow_flip     = 0.5,
			wobble       = 0.0,
			passes       = 1,
			retrace      = 0.0,
			pass_opacity = 1.0,
			profile      = LOOSE_PROFILE[:],
			scale        = 1.0,
			opacity      = 0.9,
		}
		style.scale = lerp(0.85, 1.15, next(random))
		style.bow = lerp(0.025, 0.05, next(random))
		style.wobble = lerp(0.5, 1.0, next(random))
		style.second = [2]f64{lerp(0.8, 1.0, next(random)), 0.8}
		return .Loose, style
	}
	style := Style {
		inset        = 8.0,
		jitter       = 2.0,
		bow          = 0.02,
		bow_flip     = 0.3,
		wobble       = 0.45,
		passes       = 2,
		retrace      = 0.0,
		pass_opacity = 1.0,
		profile      = TWICE_PROFILE[:],
		scale        = 1.0,
		opacity      = 0.95,
	}
	style.scale = lerp(0.85, 1.2, next(random))
	style.retrace = lerp(1.2, 2.3, next(random))
	style.pass_opacity = lerp(0.55, 0.8, next(random))
	return .Twice, style
}

cross_kind :: proc "contextless" (name: string) -> Cross_Kind {
	random := Random{hash(name)}
	kind, _ := style_of(&random)
	return kind
}

// The strokes for a picture of this size: top left to bottom right, then top
// right to bottom left, as a hand draws an X. Outlines are appended to
// `points`, their lengths to `counts` and opacities to `opacities`.
cross_strokes :: proc(w, h: f64, name: string, points: ^[dynamic]Pt, counts: ^[dynamic]int, opacities: ^[dynamic]f64) {
	if w <= 0.0 || h <= 0.0 {
		return
	}
	random := Random{hash(name)}
	_, style := style_of(&random)
	m := style.inset
	corners := [2][2]Pt{{{m, m}, {w - m, h - m}}, {{w - m, m}, {m, h - m}}}
	for pair, k in corners {
		a, b := pair[0], pair[1]
		dx, dy := b.x - a.x, b.y - a.y
		length := hypot(dx, dy)
		if length <= 0.0 {
			continue
		}
		ux, uy := dx / length, dy / length
		s := 0.0
		if r, ok := style.start.?; ok {
			s = lerp(r[0], r[1], next(&random))
		}
		e := 0.0
		if r, ok := style.end.?; ok {
			e = lerp(r[0], r[1], next(&random))
		}
		ax := a.x + ux * s + jitter(&random, style.jitter)
		ay := a.y + uy * s + jitter(&random, style.jitter)
		bx := b.x - ux * e + jitter(&random, style.jitter)
		by := b.y - uy * e + jitter(&random, style.jitter)
		scale, opacity := style.scale, style.opacity
		if k == 1 {
			if second, ok := style.second.?; ok {
				scale, opacity = second[0], second[1]
			}
		}
		for pass in 0 ..< style.passes {
			off := 0.0
			if pass > 0 {
				off = style.retrace * (-1.0 if next(&random) < 0.5 else 1.0)
			}
			nx, ny := -uy * off, ux * off
			from := Pt{ax + nx + jitter(&random, style.jitter) * 0.4, ay + ny}
			to := Pt{bx + nx, by + ny + jitter(&random, style.jitter) * 0.4}
			before := len(points)
			cross_outline(from, to, &style, scale, &random, points)
			append(counts, len(points) - before)
			append(opacities, opacity * (style.pass_opacity if pass > 0 else 1.0))
		}
	}
}

// One stroke as a filled outline: a bowed, slightly wobbling line from `a`
// to `b`, its width following the profile, with round ends.
@(private = "file")
cross_outline :: proc(a, b: Pt, style: ^Style, scale: f64, random: ^Random, out: ^[dynamic]Pt) {
	dx, dy := b.x - a.x, b.y - a.y
	length := hypot(dx, dy)
	nx, ny := -dy / length, dx / length
	bow_size := style.bow * length * (0.6 + 0.8 * next(random))
	bow := bow_size * (-1.0 if next(random) < style.bow_flip else 1.0)
	cx, cy := (a.x + b.x) / 2.0 + nx * bow * 2.0, (a.y + b.y) / 2.0 + ny * bow * 2.0
	amp := style.wobble * (0.5 + next(random))
	frequency := 1.2 + 1.6 * next(random)
	phase := next(random) * 6.28
	N :: 56
	centre: [N + 1]Pt
	widths: [N + 1]f64
	for i in 0 ..= N {
		t := f64(i) / f64(N)
		u := 1.0 - t
		wave := sin(PI * t) * amp * sin(6.283 * frequency * t + phase)
		x := u * u * a.x + 2.0 * u * t * cx + t * t * b.x + nx * wave
		y := u * u * a.y + 2.0 * u * t * cy + t * t * b.y + ny * wave
		centre[i] = {x, y}
		widths[i] = width(style.profile, t) * scale
	}
	left, right: [N + 1]Pt
	for i in 0 ..= N {
		p, w := centre[i], widths[i]
		q, s := centre[max(i, 1) - 1], centre[min(i + 1, N)]
		tx, ty := s.x - q.x, s.y - q.y
		tl := 1.0 if hypot(tx, ty) == 0.0 else hypot(tx, ty)
		half := w / 2.0
		left[i] = {p.x - ty / tl * half, p.y + tx / tl * half}
		right[i] = {p.x + ty / tl * half, p.y - tx / tl * half}
	}
	reserve(out, len(out) + 2 * N + 12)
	append(out, ..left[:])
	cross_cap(centre[N], widths[N], left[N], right[N], out)
	#reverse for r in right {
		append(out, r)
	}
	cross_cap(centre[0], widths[0], right[0], left[0], out)
}

// Half a circle round `p`, from one side of the stroke to the other.
@(private = "file")
cross_cap :: proc(p: Pt, w: f64, from, to: Pt, out: ^[dynamic]Pt) {
	a0 := atan2(from.y - p.y, from.x - p.x)
	half := w / 2.0
	a1 := atan2(to.y - p.y, to.x - p.x)
	for a1 < a0 {
		a1 += 6.283
	}
	if a1 - a0 > 3.3 {
		a1 -= 6.283
	}
	for k in 1 ..< 6 {
		angle := lerp(a0, a1, f64(k) / 6.0)
		sa, ca := sin_cos(angle)
		append(out, Pt{p.x + ca * half, p.y + sa * half})
	}
}
