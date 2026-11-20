// The marks drawn in pencil, as geometry: the stroke under a word, the rings,
// the hand-drawn stars, the reject's cross and the pick's box. A port of
// windows/marks (negs-marks), itself a port of the macOS Swift (Theme.swift
// PencilStroke and PencilRing, HandStar.swift, RejectCross.swift,
// PickBox.swift), with the same hash, the same random numbers in the same
// order and the same floating-point operations in the same order, so every
// platform draws a photograph's star, cross and box alike, bit for bit.
//
// The maths that isn't exact in IEEE (sin, cos, atan2, hypot) goes to the
// platform's libm, as Rust's and Swift's do; min and max follow Rust's
// f64::min/max (the other argument when one is NaN), as libc fmin/fmax do.
package marks

import "core:c/libc"

Pt :: struct {
	x, y: f64,
}

Rect :: struct {
	x, y, w, h: f64,
}

max_x :: proc "contextless" (r: Rect) -> f64 {return r.x + r.w}
max_y :: proc "contextless" (r: Rect) -> f64 {return r.y + r.h}
mid_x :: proc "contextless" (r: Rect) -> f64 {return r.x + r.w / 2.0}
mid_y :: proc "contextless" (r: Rect) -> f64 {return r.y + r.h / 2.0}

// One step of a path, as Core Graphics and Direct2D both take them. Move
// and Line use p[0]; Cubic is two control points, then the end.
Seg_Kind :: enum u32 {
	Move  = 0,
	Line  = 1,
	Cubic = 2,
	Close = 3,
}

Seg :: struct {
	kind:     Seg_Kind,
	reserved: u32,
	p:        [3]Pt,
}

move :: proc "contextless" (p: Pt) -> Seg {return Seg{kind = .Move, p = {p, {}, {}}}}
line :: proc "contextless" (p: Pt) -> Seg {return Seg{kind = .Line, p = {p, {}, {}}}}
cubic :: proc "contextless" (c1, c2, p: Pt) -> Seg {return Seg{kind = .Cubic, p = {c1, c2, p}}}
close :: proc "contextless" () -> Seg {return Seg{kind = .Close}}

// The libm calls Rust's f64 methods make.
sin :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.sin(x)}
cos :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.cos(x)}
atan2 :: #force_inline proc "contextless" (y, x: f64) -> f64 {return libc.atan2(y, x)}
hypot :: #force_inline proc "contextless" (x, y: f64) -> f64 {return libc.hypot(x, y)}
fmin :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmin(a, b)}
fmax :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmax(a, b)}
ceil :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.ceil(x)}
sqrt :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.sqrt(x)}

PI :: 3.14159265358979323846264338327950288

// The sine and cosine of one angle. Where Rust (and Swift) take both of the
// same value, LLVM calls the platform's fused routine instead of sin and
// cos, and it rounds differently now and then (~0.4% of angles on macOS),
// so the core calls the same one: __sincos_stret on Darwin, glibc's sincos
// on Linux; the MSVC runtime has none, so sin and cos there.
when ODIN_OS == .Darwin {
	@(private = "file")
	Sin_Cos :: struct {
		s, c: f64,
	}
	foreign import system "system:System"
	@(private = "file")
	@(default_calling_convention = "c")
	foreign system {
		__sincos_stret :: proc(x: f64) -> Sin_Cos ---
	}
	sin_cos :: #force_inline proc "contextless" (x: f64) -> (s, c: f64) {
		r := __sincos_stret(x)
		return r.s, r.c
	}
} else when ODIN_OS == .Linux {
	foreign import libm "system:m"
	@(private = "file")
	@(default_calling_convention = "c")
	foreign libm {
		sincos :: proc(x: f64, s, c: ^f64) ---
	}
	sin_cos :: #force_inline proc "contextless" (x: f64) -> (s, c: f64) {
		sincos(x, &s, &c)
		return
	}
} else {
	sin_cos :: #force_inline proc "contextless" (x: f64) -> (s, c: f64) {
		return sin(x), cos(x)
	}
}

// MARK: The page's numbers (RejectCross.swift)

// FNV-1a over the name's UTF-16 units, as the mockup pages hash it.
hash :: proc "contextless" (name: string) -> u32 {
	h: u32 = 2_166_136_261
	for r in name {
		c := u32(r)
		if c < 0x10000 {
			h = (h ~ c) * 16_777_619
		} else {
			c -= 0x10000
			h = (h ~ (0xD800 + (c >> 10))) * 16_777_619
			h = (h ~ (0xDC00 + (c & 0x3FF))) * 16_777_619
		}
	}
	return h
}

// The hash of `parts` joined, without joining them.
hash_parts :: proc "contextless" (parts: ..string) -> u32 {
	h: u32 = 2_166_136_261
	for part in parts {
		for r in part {
			c := u32(r)
			if c < 0x10000 {
				h = (h ~ c) * 16_777_619
			} else {
				c -= 0x10000
				h = (h ~ (0xD800 + (c >> 10))) * 16_777_619
				h = (h ~ (0xDC00 + (c & 0x3FF))) * 16_777_619
			}
		}
	}
	return h
}

// mulberry32, as the pages' `rng`.
Random :: struct {
	state: u32,
}

next :: proc "contextless" (r: ^Random) -> f64 {
	r.state += 0x6D2B_79F5
	s := r.state
	t := (s ~ (s >> 15)) * (1 | s)
	t = (t + (t ~ (t >> 7)) * (61 | t)) ~ t
	return f64(t ~ (t >> 14)) / 4_294_967_296.0
}

// ±`m`, evenly.
jitter :: proc "contextless" (r: ^Random, m: f64) -> f64 {
	return (next(r) - 0.5) * 2.0 * m
}

// Width along a stroke, from keyframes (t, width) eased between.
Profile :: []([2]f64)

width :: proc "contextless" (keys: Profile, t: f64) -> f64 {
	for i in 1 ..< len(keys) {
		if t <= keys[i][0] {
			t0, w0 := keys[i - 1][0], keys[i - 1][1]
			t1, w1 := keys[i][0], keys[i][1]
			return lerp(w0, w1, smooth((t - t0) / (t1 - t0)))
		}
	}
	return keys[len(keys) - 1][1]
}

lerp :: #force_inline proc "contextless" (a, b, t: f64) -> f64 {
	return a + (b - a) * t
}

smooth :: #force_inline proc "contextless" (t: f64) -> f64 {
	return t * t * (3.0 - 2.0 * t)
}

// The page's `x || 1`.
non_zero :: #force_inline proc "contextless" (x: f64) -> f64 {
	return 1.0 if x == 0.0 else x
}

// A stroke of varying width along any centreline, as a filled outline with
// round ends: the profile's width × `scale` at the share t of the way along
// (PickBox.ribbon). Appends to `out`.
ribbon :: proc(p: []Pt, profile: Profile, scale: f64, out: ^[dynamic]Pt) {
	n := len(p)
	if n < 2 {
		return
	}
	acc := make([]f64, n, context.temp_allocator)
	for i in 1 ..< n {
		acc[i] = acc[i - 1] + hypot(p[i].x - p[i - 1].x, p[i].y - p[i - 1].y)
	}
	total := non_zero(acc[n - 1])
	left := make([]Pt, n, context.temp_allocator)
	right := make([]Pt, n, context.temp_allocator)
	tangent := make([]Pt, n, context.temp_allocator)
	half := make([]f64, n, context.temp_allocator)
	for i in 0 ..< n {
		q, s := p[max(i, 1) - 1], p[min(i + 1, n - 1)]
		tx, ty := s.x - q.x, s.y - q.y
		tl := non_zero(hypot(tx, ty))
		h := width(profile, acc[i] / total) * scale / 2.0
		tangent[i] = {tx / tl, ty / tl}
		half[i] = h
		left[i] = {p[i].x - ty / tl * h, p[i].y + tx / tl * h}
		right[i] = {p[i].x + ty / tl * h, p[i].y - tx / tl * h}
	}
	reserve(out, len(out) + 2 * n + 10)
	append(out, ..left)
	ribbon_cap(p[n - 1], tangent[n - 1], half[n - 1], 1.0, out)
	#reverse for r in right {
		append(out, r)
	}
	ribbon_cap(p[0], tangent[0], half[0], -1.0, out)
}

// Half a circle round `p`, from its left side to its right, bulging along `t` × `d`.
@(private)
ribbon_cap :: proc(p, t: Pt, h, d: f64, out: ^[dynamic]Pt) {
	for k in 1 ..< 6 {
		a := f64(k) / 6.0 * PI
		s, c := sin_cos(a)
		append(out, Pt{p.x + (-t.y * c * d) * h + t.x * s * h * d, p.y + (t.x * c * d) * h + t.y * s * h * d})
	}
}
