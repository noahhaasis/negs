// The marks' C ABI (negs_core.h, "Marks"): paths into the caller's buffer,
// strokes as one owned block (negs_strokes_t, freed by negs_strokes_free).
package negs_core

import "base:runtime"
import "core:mem"
import "../marks"

Strokes :: struct {
	points:       [^]marks.Pt,
	counts:       [^]u32,
	opacities:    [^]f64,
	stroke_count: uint,
	point_count:  uint,
	kind:         u32,
	reserved:     u32,
	tilt:         f64,
}

Marks_Constants :: struct {
	pencil_width, pencil_height, highlighter_width, ring_line_width: f64,
	ring_outset_w, ring_outset_h, star_size, star_gap, reach:        f64,
}

@(export, link_name = "negs_marks_constants")
marks_constants :: proc "c" (out: ^Marks_Constants) {
	if out == nil {
		return
	}
	out^ = {
		pencil_width      = marks.PENCIL_WIDTH,
		pencil_height     = marks.PENCIL_HEIGHT,
		highlighter_width = marks.HIGHLIGHTER_WIDTH,
		ring_line_width   = marks.RING_LINE_WIDTH,
		ring_outset_w     = marks.RING_OUTSET_W,
		ring_outset_h     = marks.RING_OUTSET_H,
		star_size         = marks.STAR_SIZE,
		star_gap          = marks.STAR_GAP,
		reach             = marks.REACH,
	}
}

@(private = "file")
copy_segs :: proc "contextless" (segs: []marks.Seg, out: [^]marks.Seg, cap: uint) -> uint {
	if out != nil {
		for seg, i in segs[:min(uint(len(segs)), cap)] {
			out[i] = seg
		}
	}
	return uint(len(segs))
}

@(export, link_name = "negs_marks_pencil")
marks_pencil :: proc "c" (rect: marks.Rect, plain: u8, out: [^]marks.Seg, cap: uint) -> uint {
	segs, count := marks.pencil(rect, plain != 0)
	return copy_segs(segs[:count], out, cap)
}

@(export, link_name = "negs_marks_ring")
marks_ring :: proc "c" (rect: marks.Rect, radii: ^marks.Radii, plain: u8, out: [^]marks.Seg, cap: uint) -> uint {
	r := radii^ if radii != nil else marks.RADII_BOX
	segs := marks.ring(rect, r, plain != 0)
	return copy_segs(segs[:], out, cap)
}

@(export, link_name = "negs_marks_ring_tilt")
marks_ring_tilt :: proc "c" (degrees: f64, plain: u8) -> f64 {
	return marks.ring_tilt(degrees, plain != 0)
}

@(export, link_name = "negs_marks_radii")
marks_radii :: proc "c" (which: u32, out: ^marks.Radii) {
	if out != nil {
		out^ = marks.RADII_NUMBER if which == 1 else marks.RADII_BOX
	}
}

@(export, link_name = "negs_marks_path_bounds")
marks_path_bounds :: proc "c" (segs: [^]marks.Seg, count: uint, out: ^marks.Rect) -> u8 {
	if segs == nil || count == 0 {
		return 0
	}
	r, ok := marks.bounds(segs[:count])
	if out != nil {
		out^ = r
	}
	return 1 if ok else 0
}

@(export, link_name = "negs_marks_hash")
marks_hash :: proc "c" (name: [^]u8, n: uint) -> u32 {
	return marks.hash(host_string(name, n))
}

@(export, link_name = "negs_marks_star_seed")
marks_star_seed :: proc "c" (name: [^]u8, n: uint, index: u32) -> u32 {
	return marks.star_seed(host_string(name, n), int(index))
}

@(export, link_name = "negs_marks_cross_kind")
marks_cross_kind :: proc "c" (name: [^]u8, n: uint) -> u32 {
	return u32(marks.cross_kind(host_string(name, n)))
}

@(export, link_name = "negs_marks_box_kind")
marks_box_kind :: proc "c" (name: [^]u8, n: uint) -> u32 {
	return u32(marks.box_kind(host_string(name, n)))
}

// The strokes drawn into the temp allocator, as one block in the core's.
@(private = "file")
pack :: proc(points: []marks.Pt, counts: []int, opacities: []f64, kind: u32, tilt: f64) -> Strokes {
	k := len(counts)
	if k == 0 {
		return Strokes{kind = kind, tilt = tilt}
	}
	n := len(points)
	size := n * size_of(marks.Pt) + k * size_of(f64) + k * size_of(u32)
	block, err := mem.alloc_bytes_non_zeroed(size, align_of(marks.Pt))
	if err != nil {
		return {}
	}
	base := raw_data(block)
	s := Strokes {
		points       = ([^]marks.Pt)(base),
		opacities    = ([^]f64)(mem.ptr_offset(base, n * size_of(marks.Pt))),
		counts       = ([^]u32)(mem.ptr_offset(base, n * size_of(marks.Pt) + k * size_of(f64))),
		stroke_count = uint(k),
		point_count  = uint(n),
		kind         = kind,
		tilt         = tilt,
	}
	runtime.mem_copy_non_overlapping(s.points, raw_data(points), n * size_of(marks.Pt))
	for i in 0 ..< k {
		s.counts[i] = u32(counts[i])
		s.opacities[i] = opacities[i] if i < len(opacities) else 1.0
	}
	return s
}

@(export, link_name = "negs_marks_star")
marks_star :: proc "c" (name: [^]u8, n: uint, index: u32) -> Strokes {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	points := make([dynamic]marks.Pt, 0, 96, context.temp_allocator)
	counts := make([dynamic]int, 0, 3, context.temp_allocator)
	marks.star_strokes(host_string(name, n), int(index), &points, &counts)
	return pack(points[:], counts[:], nil, 0, 0)
}

@(export, link_name = "negs_marks_cross")
marks_cross :: proc "c" (w, h: f64, name: [^]u8, n: uint) -> Strokes {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	key := host_string(name, n)
	points := make([dynamic]marks.Pt, 0, 512, context.temp_allocator)
	counts := make([dynamic]int, 0, 4, context.temp_allocator)
	opacities := make([dynamic]f64, 0, 4, context.temp_allocator)
	marks.cross_strokes(w, h, key, &points, &counts, &opacities)
	return pack(points[:], counts[:], opacities[:], u32(marks.cross_kind(key)), 0)
}

// pick_box(w, h, outset, name, tight, weight), the name used as it is.
@(export, link_name = "negs_marks_pick_box")
marks_pick_box :: proc "c" (w, h, outset: f64, name: [^]u8, n: uint, tight: u8, weight: f64) -> Strokes {
	return box(w, h, outset, host_string(name, n), false, tight != 0, weight)
}

@(export, link_name = "negs_marks_pick_box_grid")
marks_pick_box_grid :: proc "c" (w, h: f64, name: [^]u8, n: uint) -> Strokes {
	return box(w, h, 3.0, host_string(name, n), false, false, 1.0)
}

@(export, link_name = "negs_marks_pick_box_strip")
marks_pick_box_strip :: proc "c" (w, h: f64, name: [^]u8, n: uint) -> Strokes {
	return box(w, h, 2.0, host_string(name, n), false, true, 1.0)
}

@(export, link_name = "negs_marks_pick_box_selection")
marks_pick_box_selection :: proc "c" (w, h: f64, name: [^]u8, n: uint) -> Strokes {
	// 4 / 2.6 in f64, as Rust and Swift divide it (an Odin constant would be
	// divided exactly, then rounded: a different last bit).
	four, line: f64 = 4.0, 2.6
	return box(w, h, 8.0, host_string(name, n), true, false, four / line)
}

@(private = "file")
box :: proc "contextless" (w, h, o: f64, name: string, second, tight: bool, weight: f64) -> Strokes {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	points := make([dynamic]marks.Pt, 0, 1024, context.temp_allocator)
	counts := make([dynamic]int, 0, 4, context.temp_allocator)
	kind, tilt := marks.pick_box(w, h, o, name, second, tight, weight, &points, &counts)
	return pack(points[:], counts[:], nil, u32(kind), tilt)
}

@(export, link_name = "negs_strokes_free")
strokes_free :: proc "c" (s: ^Strokes) {
	if s == nil {
		return
	}
	context = core_context()
	if s.points != nil {
		mem.free(s.points)
	}
	s^ = {}
}

@(export, link_name = "negs_marks_random_next")
marks_random_next :: proc "c" (state: ^u32) -> f64 {
	if state == nil {
		return 0
	}
	r := marks.Random{state^}
	v := marks.next(&r)
	state^ = r.state
	return v
}
