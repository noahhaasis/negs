// core/src/rules/tone: windows/src/tone_graph.rs's own tests, on the same
// synthetic curve.
package tone_tests

import "core:math"
import "core:testing"
import "../../../src/rules/tone"

GEOMETRY :: tone.Geometry{300, 170, 8}

@(private)
close :: proc(a, b: f64) -> bool {
	return abs(a - b) < 1e-9
}

// Linear in film exposure: the paper gets 0.3 log less per stop, and the
// print brightens by 0.05 per stop.
Synthetic :: struct {
	stops: [161]f64,
	paper: [161]f64,
	rgb:   [161]f64,
}

@(private)
synthetic :: proc(s: ^Synthetic) -> tone.Curve {
	for i in 0 ..= 160 {
		v := -8.0 + f64(i) * 0.1
		s.stops[i], s.paper[i], s.rgb[i] = v, -0.3 * v, 0.5 + 0.05 * v
	}
	return tone.Curve{stops = s.stops[:], rgb = {s.rgb[:], s.rgb[:], s.rgb[:]}, paper = {s.paper[:], s.paper[:], s.paper[:]}, has_paper = true}
}

@(test)
indexes_interpolate_and_clamp_to_the_ends :: proc(t: ^testing.T) {
	falling := []f64{3, 2, 1, 0}
	testing.expect(t, close(tone.index_of_decreasing(tone.plain(falling), 1.5), 1.5))
	testing.expect_value(t, tone.index_of_decreasing(tone.plain(falling), 9), 0)
	testing.expect_value(t, tone.index_of_decreasing(tone.plain(falling), -9), 3)
	testing.expect(t, close(tone.index_of_increasing(tone.plain([]f64{0, 1, 2}), 0.25), 0.25))
	testing.expect(t, close(tone.interpolate(tone.plain([]f64{0, 10, 20}), 1.5), 15))
	testing.expect_value(t, tone.interpolate(tone.plain([]f64{0, 10}), 7), 10)
	testing.expect_value(t, tone.interpolate(tone.plain(nil), 1), 0)
}

@(test)
prediction_is_exact_for_print_exposure :: proc(t: ^testing.T) {
	s: Synthetic
	base := synthetic(&s)
	one := tone.Controls{1, 1, 1, 1}
	rgb, paper: [3][161]f64
	out_rgb := [3][]f64{rgb[0][:], rgb[1][:], rgb[2][:]}
	out_paper := [3][]f64{paper[0][:], paper[1][:], paper[2][:]}
	testing.expect(t, !tone.predicted(base, one, one, out_rgb, out_paper))
	testing.expect(t, tone.predicted(base, one, {2, 1, 1, 1}, out_rgb, out_paper))
	shift := math.log10(f64(2)) / 0.3
	for i in ([]int{40, 80, 120}) {
		testing.expect(t, abs(rgb[1][i] - (0.5 + 0.05 * (s.stops[i] - shift))) < 1e-9)
	}
	display := tone.Curve{stops = s.stops[:], rgb = out_rgb, paper = out_paper, has_paper = true}
	_, middle, _, ok := tone.handles(display, -1)
	testing.expect(t, ok && abs(middle - shift) < 1e-9)
}

@(test)
handles_and_hits :: proc(t: ^testing.T) {
	s: Synthetic
	display := synthetic(&s)
	shadows, middle, highlights, ok := tone.handles(display, -1)
	testing.expect(t, ok && close(shadows, -2) && close(middle, 0) && close(highlights, 2))
	points, count := tone.handle_points(GEOMETRY, display, -1)
	testing.expect_value(t, count, 3)
	testing.expect(t, close(points[1].x, 150) && close(points[1].y, tone.y_of(GEOMETRY, 0.5)))
	h, hit := tone.hit(GEOMETRY, display, -1, points[1].x + 3, points[1].y, 10, 12)
	testing.expect(t, hit && h.kind == .Middle)
	tt := 4.0
	h, hit = tone.hit(GEOMETRY, display, -1, tone.x_of(GEOMETRY, tt), tone.y_of(GEOMETRY, 0.5 + 0.05 * tt) + 5, 10, 12)
	testing.expect(t, hit && h.kind == .Curve && abs(h.tone - tt) < 1e-9)
	_, hit = tone.hit(GEOMETRY, display, -1, tone.x_of(GEOMETRY, tt), tone.y_of(GEOMETRY, 0.5 + 0.05 * tt) + 35, 10, 12)
	testing.expect(t, !hit)
}

@(test)
drags :: proc(t: ^testing.T) {
	s: Synthetic
	base := synthetic(&s)
	one := tone.Controls{1, 1, 1, 1}
	// One stop right: paper −0.3 there, so 10^0.3 = 1.995 → 2.00.
	target, value := tone.drag_value(.Middle, 0, base, one, one, -1, GEOMETRY, tone.x_of(GEOMETRY, 1), 0)
	testing.expect(t, target == .Print_Exposure && value == 2.0)
	_, value = tone.drag_value(.Middle, 0, base, one, one, -1, GEOMETRY, 0, 0)
	testing.expect_value(t, value, 0.1)
	target, value = tone.drag_value(.Curve, 2, base, one, one, -1, GEOMETRY, tone.x_of(GEOMETRY, 2), tone.y_of(GEOMETRY, 0.5 + 0.05 * 2.4))
	testing.expect(t, target == .Gamma && value == 1.2)
	tone_at, _ := tone.drag_tone({.Shadows, 0}, base, -1)
	target, value = tone.drag_value(.Shadows, tone_at, base, one, one, -1, GEOMETRY, tone.x_of(GEOMETRY, -2), tone.y_of(GEOMETRY, 0.5 - 0.05 * 2.5))
	testing.expect(t, target == .Slow && value == 1.25)
	target, _ = tone.drag_value(.Curve, 0, base, one, one, -1, GEOMETRY, 150, 20)
	testing.expect_value(t, target, tone.Target.None)
	target, value = tone.drag_value(.Highlights, 2, base, one, {1, 1.2, 1, 1}, -1, GEOMETRY, tone.x_of(GEOMETRY, 2), tone.y_of(GEOMETRY, 0.5 + 0.05 * 2.4))
	testing.expect(t, target == .Fast && value == 1.0)
}
