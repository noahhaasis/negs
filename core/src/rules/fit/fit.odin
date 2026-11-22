// The fitted view's exact-size picture (windows/src/exact_fit.rs, ticket
// 38): the developed image resampled once to the very pixels the fit shows,
// with Lanczos-3 in linear light as Pillow's `Image.LANCZOS` does it. The
// same floating-point steps as the Rust, so the pixels are identical
// (core/diff/tests/exact_fit.rs).
package fit

import "core:c/libc"
import "core:os"
import "core:thread"
import "../crop"

// Lanczos-3's reach, in input pixels at scale 1.
SUPPORT: f64 : 3.0

// The exact picture's size for a `w` × `h` photograph shown at `z`
// physical pixels per photo pixel.
target :: proc "contextless" (w, h, z: f64) -> [2]u32 {
	return {max(crop.sat_u32(crop.round(w * z)), 1), max(crop.sat_u32(crop.round(h * z)), 1)}
}

// Whether the fitted view should draw an exact picture: shrinking, and the
// texture larger than the target.
worth_it :: proc "contextless" (z: f64, texture, target: [2]u32) -> bool {
	return z < 0.999 && (texture[0] > target[0] || texture[1] > target[1])
}

@(private = "file")
sinc :: proc "contextless" (x: f64) -> f64 {
	if x == 0 {
		return 1
	}
	pi: f64 = crop.PI
	y := x * pi
	return libc.sin(y) / y
}

// Pillow's `lanczos_filter`.
@(private = "file")
lanczos :: proc "contextless" (x: f64) -> f64 {
	support: f64 = SUPPORT
	if -support <= x && x < support {
		return sinc(x) * sinc(x / support)
	}
	return 0
}

// Per output pixel along an axis: the first input pixel it reads, and its
// weights' run in `weights`.
Taps :: struct {
	start, first, count: u32,
}

Coefficients :: struct {
	taps:    []Taps,
	weights: []f32,
}

@(private = "file")
span :: proc "contextless" (input, output, x: u32) -> (lo, hi: u32, center, ss: f64) {
	scale := f64(input) / f64(output)
	filterscale := crop.fmax(scale, 1.0)
	support := SUPPORT * filterscale
	ss = 1.0 / filterscale
	center = (f64(x) + 0.5) * scale
	// C's `(int)` truncates toward zero, then Pillow clamps.
	lo = u32(max(crop.sat_i64(center - support + 0.5), 0))
	hi = u32(min(crop.sat_i64(center + support + 0.5), i64(input)))
	return
}

// How many weights `coefficients` makes.
weight_count :: proc "contextless" (input, output: u32) -> int {
	n := 0
	for x in 0 ..< output {
		lo, hi, _, _ := span(input, output, x)
		if hi > lo {
			n += int(hi - lo)
		}
	}
	return n
}

// Pillow's `precompute_coeffs`: the kernel widened by the reduction, cut at
// the edges and renormalised. `taps` has `output` entries, `weights`
// weight_count's.
coefficients_into :: proc "contextless" (input, output: u32, taps: []Taps, weights: []f32) {
	at := 0
	for x in 0 ..< output {
		lo, hi, center, ss := span(input, output, x)
		count := hi - lo if hi > lo else 0
		sum: f64 = 0
		first := true
		for i in lo ..< lo + count {
			w := lanczos((f64(i) - center + 0.5) * ss)
			sum = w if first else sum + w
			first = false
		}
		for i in lo ..< lo + count {
			w := lanczos((f64(i) - center + 0.5) * ss)
			if sum != 0 {
				w /= sum
			}
			weights[at + int(i - lo)] = f32(w)
		}
		taps[x] = Taps{lo, u32(at), count}
		at += int(count)
	}
}

coefficients :: proc(input, output: u32, allocator := context.allocator) -> Coefficients {
	c := Coefficients {
		taps    = make([]Taps, output, allocator),
		weights = make([]f32, weight_count(input, output), allocator),
	}
	coefficients_into(input, output, c.taps, c.weights)
	return c
}

coefficients_free :: proc(c: Coefficients, allocator := context.allocator) {
	delete(c.taps, allocator)
	delete(c.weights, allocator)
}

@(private = "file")
decode :: proc "contextless" (x: f32) -> f32 {
	if x <= 0.04045 {
		return x / 12.92
	}
	gamma: f32 = 2.4
	return libc.powf((x + 0.055) / 1.055, gamma)
}

@(private = "file")
encode :: proc "contextless" (v: f32) -> f32 {
	x := v
	if x < 0 {
		x = 0
	}
	if x > 1 {
		x = 1
	}
	if x <= 0.0031308 {
		return x * 12.92
	}
	one, gamma: f32 = 1.0, 2.4
	return 1.055 * libc.powf(x, one / gamma) - 0.055
}

@(private = "file")
Job :: struct {
	rgba:    []u8,
	w, tw:   int,
	lut:     [256]f32,
	across:  ^Coefficients,
	down:    ^Coefficients,
	middle:  []f32,
	out:     []u8,
	first:   int,
	rows:    int,
}

// Across: `rows` rows of `tw` linear pixels.
@(private = "file")
run_across :: proc(job: ^Job) {
	tw := job.tw
	for y in job.first ..< job.first + job.rows {
		source := job.rgba[y * job.w * 4:(y + 1) * job.w * 4]
		row := job.middle[y * tw * 3:(y + 1) * tw * 3]
		for t, x in job.across.taps {
			weights := job.across.weights[t.first:t.first + t.count]
			sum: [3]f32
			for weight, k in weights {
				p := (int(t.start) + k) * 4
				for c in 0 ..< 3 {
					sum[c] += weight * job.lut[source[p + c]]
				}
			}
			row[x * 3] = sum[0]
			row[x * 3 + 1] = sum[1]
			row[x * 3 + 2] = sum[2]
		}
	}
}

// Down: `rows` rows of `tw` pixels, encoded.
@(private = "file")
run_down :: proc(job: ^Job) {
	tw := job.tw
	for y in job.first ..< job.first + job.rows {
		t := job.down.taps[y]
		weights := job.down.weights[t.first:t.first + t.count]
		row := job.out[y * tw * 4:(y + 1) * tw * 4]
		for x in 0 ..< tw {
			sum: [3]f32
			for weight, k in weights {
				p := ((int(t.start) + k) * tw + x) * 3
				for c in 0 ..< 3 {
					sum[c] += weight * job.middle[p + c]
				}
			}
			for c in 0 ..< 3 {
				row[x * 4 + c] = crop.sat_u8(f64(libc.roundf(encode(sum[c]) * 255.0)))
			}
			row[x * 4 + 3] = 255
		}
	}
}

@(private = "file")
threads :: proc() -> int {
	return clamp(os.get_processor_core_count(), 1, 16)
}

// Runs `work` over `total` rows in up to `threads()` bands, as the Rust
// splits them (each row is computed alone, so the bands don't change it).
@(private = "file")
parallel :: proc(job: Job, total: int, work: proc(job: ^Job)) {
	n := threads()
	each := max((total + n - 1) / n, 1)
	jobs: [16]Job
	workers: [16]^thread.Thread
	started := 0
	first := 0
	for i in 0 ..< n {
		if first >= total {
			break
		}
		jobs[i] = job
		jobs[i].first = first
		jobs[i].rows = min(each, total - first)
		first += jobs[i].rows
		if i == n - 1 || first >= total {
			work(&jobs[i])
		} else {
			workers[started] = thread.create_and_start_with_poly_data2(&jobs[i], work, proc(j: ^Job, f: proc(job: ^Job)) {f(j)}, context)
			started += 1
		}
	}
	for t in workers[:started] {
		thread.join(t)
		thread.destroy(t)
	}
}

// Resamples sRGB RGBA `w` × `h` to `tw` × `th` in linear light into `out`
// (tw × th × 4 bytes): across, then down, as Pillow does, in floats
// between. Opaque out. False when a buffer is too small.
resample :: proc(rgba: []u8, w, h, tw, th: u32, out: []u8) -> bool {
	if tw == 0 || th == 0 || len(rgba) < int(w) * int(h) * 4 || len(out) < int(tw) * int(th) * 4 {
		return false
	}
	lut: [256]f32
	for i in 0 ..< 256 {
		lut[i] = decode(f32(i) / 255.0)
	}
	across := coefficients(w, tw)
	defer coefficients_free(across)
	down := coefficients(h, th)
	defer coefficients_free(down)
	middle, err := make([]f32, int(tw) * int(h) * 3)
	if err != nil {
		return false
	}
	defer delete(middle)
	job := Job {
		rgba   = rgba,
		w      = int(w),
		tw     = int(tw),
		lut    = lut,
		across = &across,
		down   = &down,
		middle = middle,
		out    = out,
	}
	parallel(job, int(h), run_across)
	parallel(job, int(th), run_down)
	return true
}
