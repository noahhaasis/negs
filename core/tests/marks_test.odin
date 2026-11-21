// The marks against the mockup pages' own numbers (HandStarTests.swift,
// RejectCrossTests.swift and PickBoxTests.swift, by way of windows/marks's
// tests): the hash, and per stroke its point count, first point, the point a
// third of the way round and its last, to two places.
package tests

import "core:fmt"
import "core:testing"
import "../src/marks"

@(private = "file")
Outline :: struct {
	count: int,
	want:  [3][2]f64,
}

@(private = "file")
check_outline :: proc(t: ^testing.T, label: string, points: []marks.Pt, o: Outline) {
	testing.expectf(t, len(points) == o.count, "%s: %d points, want %d", label, len(points), o.count)
	if len(points) != o.count {
		return
	}
	n := len(points)
	got := [3]marks.Pt{points[0], points[n / 3], points[n - 1]}
	for p, i in got {
		w := o.want[i]
		testing.expectf(t, abs(p.x - w[0]) < 0.006 && abs(p.y - w[1]) < 0.006, "%s: %v against %v", label, p, w)
	}
}

@(private = "file")
Star_Row :: struct {
	name:    string,
	index:   int,
	seed:    u32,
	strokes: [3]Outline,
}

@(private = "file")
STARS := [?]Star_Row {
	{"DSC_6435.NEF", 0, 3315392284, {{28, {{-1.99, -4.77}, {-0.09, 3.89}, {-1.98, -4.98}}}, {28, {{-5.62, -3.29}, {4.14, 3.07}, {-5.77, -3.44}}}, {28, {{4.47, -3.68}, {-5.1, 3.21}, {4.67, -3.76}}}}},
	{"DSC_6435.NEF", 1, 3332169903, {{28, {{0.02, -5.01}, {-1.06, 4.73}, {0.09, -5.22}}}, {28, {{-3.96, -2.24}, {3.76, 3.89}, {-4.08, -2.42}}}, {28, {{5.12, -2.12}, {-5.19, 2.22}, {5.34, -2.16}}}}},
	{"DSC_6435.NEF", 4, 3248281808, {{28, {{0.18, -5.59}, {-0.78, 4.84}, {0.27, -5.78}}}, {28, {{-4.36, -2.06}, {4.13, 2.18}, {-4.53, -2.19}}}, {28, {{4.23, -2.86}, {-3.94, 1.73}, {4.44, -2.91}}}}},
	{"IMG_0001.JPG", 0, 948015264, {{28, {{-0.13, -4.8}, {-0.51, 5.99}, {-0.04, -5.03}}}, {28, {{-4.51, -2.73}, {3.9, 4.59}, {-4.63, -2.95}}}, {28, {{4.02, -2.84}, {-4.55, 2.54}, {4.26, -2.92}}}}},
	{"IMG_0001.JPG", 2, 981570502, {{28, {{0.07, -4.46}, {0.67, 5.6}, {0.13, -4.69}}}, {28, {{-3.9, -1.28}, {4.56, 2.95}, {-4.06, -1.45}}}, {28, {{4.36, -2.93}, {-3.96, 3.07}, {4.58, -3.01}}}}},
	{"DSC_7041.NEF", 3, 1708573339, {{28, {{-1.51, -5.29}, {1.08, 6.06}, {-1.48, -5.53}}}, {28, {{-5.01, -1.3}, {4.69, 1.77}, {-5.21, -1.44}}}, {28, {{4.05, -3.18}, {-4.47, 2.1}, {4.28, -3.25}}}}},
	{"f03.jpg", 0, 2243906042, {{28, {{0.02, -5.24}, {-0.79, 5.83}, {0.11, -5.46}}}, {28, {{-5.49, -2.1}, {4.59, 3.45}, {-5.65, -2.26}}}, {28, {{4.93, -2.24}, {-5.38, 2.27}, {5.17, -2.27}}}}},
	{"Ålesund 2024.dng", 1, 124155722, {{28, {{0.2, -5.65}, {-0.5, 5.5}, {0.28, -5.89}}}, {28, {{-4.49, -1.42}, {5.21, 2.24}, {-4.69, -1.57}}}, {28, {{3.82, -3.61}, {-4.21, 2.26}, {4.07, -3.68}}}}},
}

@(test)
stars_match_the_drawing :: proc(t: ^testing.T) {
	for row in STARS {
		testing.expect_value(t, marks.star_seed(row.name, row.index), row.seed)
		points: [dynamic]marks.Pt
		counts: [dynamic]int
		defer delete(points)
		defer delete(counts)
		marks.star_strokes(row.name, row.index, &points, &counts)
		testing.expect_value(t, len(counts), 3)
		at := 0
		for c, i in counts {
			check_outline(t, fmt.tprintf("%s #%d", row.name, row.index), points[at:at + c], row.strokes[i])
			at += c
		}
	}
}

@(test)
stars_stay_in_their_box :: proc(t: ^testing.T) {
	for n in 0 ..< 200 {
		name := fmt.tprintf("DSC_%d.NEF", 7000 + n)
		for index in 0 ..< 5 {
			points: [dynamic]marks.Pt
			counts: [dynamic]int
			defer delete(points)
			defer delete(counts)
			marks.star_strokes(name, index, &points, &counts)
			b, _ := marks.polygon_bounds(points[:])
			testing.expectf(t, b.x >= -8 && b.y >= -8 && b.x + b.w <= 8 && b.y + b.h <= 8, "%s #%d %v", name, index, b)
			testing.expectf(t, b.w > 7 && b.h > 7 && b.x <= 0 && b.x + b.w >= 0 && b.y <= 0 && b.y + b.h >= 0, "%s #%d %v", name, index, b)
		}
	}
}

@(private = "file")
Cross_Row :: struct {
	name:      string,
	seed:      u32,
	first:     [2]f64,
	starts:    [][2]f64,
	opacities: []f64,
}

@(private = "file")
CROSSES := [?]Cross_Row {
	{"DSC_6435.NEF", 2972515683, {0.212829355, 0.331041164}, {{24.84, 19.71}, {173.45, 18.11}}, {0.9, 0.8}},
	{"DSC_5784.NEF", 1338732189, {0.090616154, 0.541751701}, {{22.24, 24.10}, {172.57, 17.38}}, {0.9, 0.8}},
	{"IMG_0001.JPG", 2515027847, {0.705942326, 0.111862622}, {{5.90, 10.17}, {5.79, 11.72}, {191.77, 5.74}, {191.37, 4.19}}, {0.95, 0.69, 0.95, 0.69}},
	{"DSC_8200.NEF", 2362156491, {0.952154420, 0.436344665}, {{6.83, 7.86}, {5.85, 9.28}, {192.66, 8.28}, {192.06, 6.88}}, {0.95, 0.66, 0.95, 0.66}},
}

@(test)
crosses_match_the_drawing :: proc(t: ^testing.T) {
	for row in CROSSES {
		testing.expect_value(t, marks.hash(row.name), row.seed)
		random := marks.Random{row.seed}
		for v in row.first {
			testing.expect(t, abs(marks.next(&random) - v) < 1e-9, row.name)
		}
		points: [dynamic]marks.Pt
		counts: [dynamic]int
		opacities: [dynamic]f64
		defer delete(points)
		defer delete(counts)
		defer delete(opacities)
		marks.cross_strokes(200, 140, row.name, &points, &counts, &opacities)
		testing.expect_value(t, len(counts), len(row.starts))
		at := 0
		for c, i in counts {
			if i >= len(row.starts) {
				break
			}
			p := points[at]
			testing.expectf(t, abs(p.x - row.starts[i][0]) < 0.006 && abs(p.y - row.starts[i][1]) < 0.006, "%s: %v", row.name, p)
			testing.expect(t, abs(opacities[i] - row.opacities[i]) < 0.006, row.name)
			at += c
		}
	}
	testing.expect_value(t, marks.cross_kind("DSC_6435.NEF"), marks.Cross_Kind.Loose)
	testing.expect_value(t, marks.cross_kind("IMG_0001.JPG"), marks.Cross_Kind.Twice)
	loose := 0
	for n in 0 ..< 40 {
		if marks.cross_kind(fmt.tprintf("DSC_%d.NEF", 7000 + n)) == .Loose {
			loose += 1
		}
	}
	testing.expectf(t, loose >= 8 && loose <= 32, "%d of 40 loose", loose)
}

@(private = "file")
Box_Row :: struct {
	name:         string,
	w, h, outset: f64,
	tight:        bool,
	weight:       f64,
	seed:         u32,
	kind:         marks.Box_Kind,
	tilt:         f64,
	strokes:      []Outline,
}

@(private = "file")
BOXES := [?]Box_Row {
	{"DSC_6435.NEF", 264.0, 176.0, 3.0, false, 1.0, 2056138581, .Four, -0.59, {{206, {{-8.41, -0.79}, {193.73, -2.11}, {-8.74, -0.88}}}, {138, {{268.14, -1.58}, {264.9, 136.16}, {268.24, -1.9}}}, {202, {{269.28, 181.45}, {69.25, 177.21}, {269.6, 181.55}}}, {136, {{-4.62, 177.63}, {-4.02, 43.08}, {-4.71, 177.95}}}}},
	{"DSC_5784.NEF", 266.0, 176.0, 3.0, false, 1.0, 1189942715, .Loop, 0.0, {{668, {{70.5, -2.14}, {-1.59, 174.03}, {70.19, -2.23}}}}},
	{"DSC_6448.NEF", 117.0, 176.0, 3.0, false, 1.0, 1833535613, .Loop, 0.56, {{452, {{38.51, -3.2}, {0.39, 124.35}, {38.13, -3.32}}}}},
	{"DSC_7006.NEF", 264.0, 176.0, 3.0, false, 1.0, 1275140550, .Two, -0.13, {{318, {{-4.46, 182.24}, {130.35, -0.59}, {-4.55, 182.61}}}, {316, {{267.83, 1.74}, {127.29, 176.95}, {267.92, 1.36}}}}},
	{"IMG_0001.JPG", 210.0, 140.0, 2.0, true, 1.0, 1738235033, .Four, -0.15, {{158, {{-1.63, -0.65}, {152.78, 1.22}, {-2.01, -0.75}}}, {108, {{213.05, 1.02}, {210.9, 108.06}, {213.16, 0.64}}}, {156, {{210.36, 140.7}, {56.28, 139.43}, {210.75, 140.8}}}, {108, {{-3.16, 140.76}, {-0.51, 34.3}, {-3.27, 141.14}}}}},
	{"DSC_7007.NEF", 100.0, 140.0, 2.0, true, 1.0, 969449513, .Two, 0.27, {{182, {{-0.93, 144.85}, {29.92, 0.0}, {-1.03, 145.27}}}, {180, {{100.66, -0.05}, {65.64, 141.42}, {100.79, -0.46}}}}},
	{"DSC_6435.NEF·second", 264.0, 176.0, 8.0, false, 4.0 / 2.6, 4080007402, .Loop, 0.08, {{688, {{96.6, -7.38}, {-4.56, 138.58}, {95.97, -7.58}}}}},
	{"DSC_5784.NEF·second", 266.0, 176.0, 8.0, false, 4.0 / 2.6, 2258131768, .Four, -0.43, {{206, {{-13.07, -6.6}, {189.88, -7.2}, {-13.69, -6.75}}}, {144, {{271.51, -10.8}, {271.41, 131.71}, {271.65, -11.43}}}, {208, {{278.52, 183.19}, {71.65, 183.21}, {279.15, 183.33}}}, {144, {{-8.15, 182.6}, {-7.56, 40.08}, {-8.31, 183.22}}}}},
}

@(test)
boxes_match_the_drawing :: proc(t: ^testing.T) {
	for row in BOXES {
		testing.expect_value(t, marks.box_seed(row.name), row.seed)
		testing.expect_value(t, marks.box_kind(row.name), row.kind)
		points: [dynamic]marks.Pt
		counts: [dynamic]int
		defer delete(points)
		defer delete(counts)
		kind, tilt := marks.pick_box(row.w, row.h, row.outset, row.name, false, row.tight, row.weight, &points, &counts)
		testing.expect_value(t, kind, row.kind)
		testing.expectf(t, abs(tilt - row.tilt) < 0.006, "%s: tilt %v", row.name, tilt)
		testing.expect_value(t, len(counts), len(row.strokes))
		at := 0
		for c, i in counts {
			if i < len(row.strokes) {
				check_outline(t, row.name, points[at:at + c], row.strokes[i])
			}
			at += c
		}
	}
	// The page draws 16 four, 10 loop and 14 two of these.
	tally: [marks.Box_Kind]int
	for n in 0 ..< 40 {
		tally[marks.box_kind(fmt.tprintf("DSC_%d.NEF", 7000 + n))] += 1
	}
	testing.expect_value(t, tally[.Four], 16)
	testing.expect_value(t, tally[.Loop], 10)
	testing.expect_value(t, tally[.Two], 14)
}

@(test)
pencil_spans_its_word :: proc(t: ^testing.T) {
	rect := marks.Rect{10, 20, 80, 6}
	segs, count := marks.pencil(rect, false)
	b, ok := marks.bounds(segs[:count])
	testing.expect(t, ok)
	testing.expectf(t, abs(b.x - 10.8) < 0.01 && abs(b.x + b.w - 89.2) < 0.01, "%v", b)
	testing.expect(t, b.y >= rect.y && b.y + b.h <= rect.y + rect.h)
	testing.expect_value(t, segs[0].p[0], marks.Pt{10.8, 20 + 3.6})
	plain, n := marks.pencil({0, 0, 50, 6}, true)
	testing.expect_value(t, n, 2)
	testing.expect_value(t, plain[0].p[0], marks.Pt{1, 3})
	testing.expect_value(t, plain[1].p[0], marks.Pt{49, 3})
}

@(test)
ring_scales_its_corners_like_css :: proc(t: ^testing.T) {
	large_segs := marks.ring({0, 0, 200, 120}, marks.RADII_BOX, false)
	large, _ := marks.bounds(large_segs[:])
	testing.expectf(t, abs(large.x) < 1e-9 && abs(large.y) < 1e-9 && abs(large.w - 200) < 1e-9 && abs(large.h - 120) < 1e-9, "%v", large)
	small := marks.ring({0, 0, 60, 40}, marks.RADII_BOX, false)
	f := 40.0 / 66.0
	testing.expect_value(t, small[0].p[0], marks.Pt{30 * f, 0})
	testing.expect(t, abs(small[7].p[0].y - 34 * f) < 1e-9)
	testing.expect_value(t, small[9].kind, marks.Seg_Kind.Close)
	number := marks.ring({0, 0, 20, 20}, marks.RADII_NUMBER, false)
	testing.expect(t, abs(number[0].p[0].x - 0.47 * 20 / 1.02) < 1e-9)
	testing.expect_value(t, marks.ring({0, 0, 100, 60}, marks.RADII_BOX, true)[0].p[0], marks.Pt{6, 0})
	testing.expect_value(t, marks.ring({0, 0, 20, 16}, marks.RADII_NUMBER, true)[0].p[0], marks.Pt{8, 0})
	testing.expect_value(t, marks.ring_tilt(-0.8, false), -0.8)
	testing.expect_value(t, marks.ring_tilt(-0.8, true), 0.0)
}

@(test)
names_past_ascii_hash_their_utf16 :: proc(t: ^testing.T) {
	// U+1F4F7 (a camera) is a surrogate pair in UTF-16.
	testing.expect_value(t, marks.hash("a\U0001F4F7"), marks.hash_parts("a", "\U0001F4F7"))
	testing.expect_value(t, marks.star_seed("Ålesund 2024.dng", 1), 124_155_722)
}
