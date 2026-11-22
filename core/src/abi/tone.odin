// The tone graph's C ABI (negs_core.h, "Phase 3c"): the series are
// borrowed for the call; results go into the caller's memory.
package negs_core

import "../rules/tone"

Tone_Series_C :: struct {
	data: [^]f64,
	len:  uint,
}

Tone_Curve_C :: struct {
	stops:     Tone_Series_C,
	rgb:       [3]Tone_Series_C,
	paper:     [3]Tone_Series_C,
	has_paper: u8,
	reserved:  [7]u8,
}

Tone_Point_C :: struct {
	kind:     u32,
	reserved: u32,
	x, y:     f64,
}

@(private = "file")
slice_of :: proc "contextless" (s: Tone_Series_C) -> []f64 {
	return s.data[:s.len] if s.data != nil else nil
}

@(private = "file")
curve_of :: proc "contextless" (c: ^Tone_Curve_C) -> (out: tone.Curve) {
	if c == nil {
		return
	}
	out.stops = slice_of(c.stops)
	for i in 0 ..< 3 {
		out.rgb[i] = slice_of(c.rgb[i])
		out.paper[i] = slice_of(c.paper[i])
	}
	out.has_paper = c.has_paper != 0
	return
}

@(private = "file")
kind_of :: proc "contextless" (kind: u32) -> tone.Handle_Kind {
	return tone.Handle_Kind(kind) if kind <= 3 else .Curve
}

@(export, link_name = "negs_tone_index_of_decreasing")
tone_index_of_decreasing :: proc "c" (series: [^]f64, n: uint, value: f64) -> f64 {
	return tone.index_of_decreasing(tone.plain(slice_of({series, n})), value)
}

@(export, link_name = "negs_tone_index_of_increasing")
tone_index_of_increasing :: proc "c" (series: [^]f64, n: uint, value: f64) -> f64 {
	return tone.index_of_increasing(tone.plain(slice_of({series, n})), value)
}

@(export, link_name = "negs_tone_interpolate")
tone_interpolate :: proc "c" (series: [^]f64, n: uint, index: f64) -> f64 {
	return tone.interpolate(tone.plain(slice_of({series, n})), index)
}

@(export, link_name = "negs_tone_luma")
tone_luma :: proc "c" (r, g, b: f64) -> f64 {
	return tone.luma(r, g, b)
}

@(export, link_name = "negs_tone_combine")
tone_combine :: proc "c" (three: ^[3]Tone_Series_C, luma: u8, out: [^]f64) {
	if three == nil || out == nil {
		return
	}
	s := tone.Series{c = {slice_of(three[0]), slice_of(three[1]), slice_of(three[2])}, channel = -1, luma = luma != 0}
	for i in 0 ..< tone.series_len(s) {
		out[i] = tone.series_at(s, i)
	}
}

@(export, link_name = "negs_tone_predict")
tone_predict :: proc "c" (curve: ^Tone_Curve_C, controls, next: tone.Controls, rgb_out, paper_out: ^[3][^]f64) -> u8 {
	if curve == nil || rgb_out == nil || paper_out == nil {
		return 0
	}
	c := curve_of(curve)
	n := len(c.stops)
	rgb, paper: [3][]f64
	for i in 0 ..< 3 {
		rgb[i] = rgb_out[i][:n] if rgb_out[i] != nil else nil
		paper[i] = paper_out[i][:n] if paper_out[i] != nil else nil
	}
	return u8(tone.predicted(c, controls, next, rgb, paper))
}

@(export, link_name = "negs_tone_handles")
tone_handles :: proc "c" (display: ^Tone_Curve_C, channel: i32, out: ^[3]f64) -> u8 {
	s, m, h, ok := tone.handles(curve_of(display), int(channel))
	if ok && out != nil {
		out^ = {s, m, h}
	}
	return u8(ok)
}

@(export, link_name = "negs_tone_value")
tone_value :: proc "c" (display: ^Tone_Curve_C, series: [^]f64, n: uint, stops: f64) -> f64 {
	return tone.value_at(curve_of(display), tone.plain(slice_of({series, n})), stops)
}

@(export, link_name = "negs_tone_x")
tone_x :: proc "c" (g: tone.Geometry, stops: f64) -> f64 {
	return tone.x_of(g, stops)
}

@(export, link_name = "negs_tone_stops")
tone_stops :: proc "c" (g: tone.Geometry, x: f64) -> f64 {
	return tone.stops_of(g, x)
}

@(export, link_name = "negs_tone_y")
tone_y :: proc "c" (g: tone.Geometry, brightness: f64) -> f64 {
	return tone.y_of(g, brightness)
}

@(export, link_name = "negs_tone_brightness")
tone_brightness :: proc "c" (g: tone.Geometry, y: f64) -> f64 {
	return tone.brightness_of(g, y)
}

@(export, link_name = "negs_tone_handle_points")
tone_handle_points :: proc "c" (g: tone.Geometry, display: ^Tone_Curve_C, channel: i32, out: ^[3]Tone_Point_C) -> uint {
	points, count := tone.handle_points(g, curve_of(display), int(channel))
	if out != nil {
		for p, i in points[:count] {
			out[i] = {u32(p.kind), 0, p.x, p.y}
		}
	}
	return uint(count)
}

@(export, link_name = "negs_tone_hit")
tone_hit :: proc "c" (g: tone.Geometry, display: ^Tone_Curve_C, channel: i32, x, y, radius, tolerance: f64, kind: ^u32, tone_out: ^f64) -> u8 {
	h, ok := tone.hit(g, curve_of(display), int(channel), x, y, radius, tolerance)
	if ok {
		if kind != nil {
			kind^ = u32(h.kind)
		}
		if tone_out != nil {
			tone_out^ = h.tone
		}
	}
	return u8(ok)
}

@(export, link_name = "negs_tone_drag_tone")
tone_drag_tone :: proc "c" (kind: u32, at: f64, display: ^Tone_Curve_C, channel: i32, out: ^f64) -> u8 {
	t, ok := tone.drag_tone({kind_of(kind), at}, curve_of(display), int(channel))
	if ok && out != nil {
		out^ = t
	}
	return u8(ok)
}

@(export, link_name = "negs_tone_drag_value")
tone_drag_value :: proc "c" (kind: u32, at: f64, base: ^Tone_Curve_C, start, current: tone.Controls, channel: i32, g: tone.Geometry, x, y: f64, out: ^f64) -> u32 {
	target, value := tone.drag_value(kind_of(kind), at, curve_of(base), start, current, int(channel), g, x, y)
	if target != .None && out != nil {
		out^ = value
	}
	return u32(target)
}

@(export, link_name = "negs_tone_snap")
tone_snap :: proc "c" (value, low, high, step: f64) -> f64 {
	return tone.snap(value, low, high, step)
}

@(export, link_name = "negs_tone_handle_title")
tone_handle_title :: proc "c" (kind: u32) -> cstring {
	switch kind_of(kind) {
	case .Middle:
		return "Print exposure"
	case .Shadows:
		return "Shadow contrast"
	case .Highlights:
		return "Highlight contrast"
	case .Curve:
		return "Contrast"
	}
	return ""
}

@(export, link_name = "negs_tone_handle_hint")
tone_handle_hint :: proc "c" (kind: u32) -> cstring {
	return "drag sideways" if kind_of(kind) == .Middle else "drag up or down"
}
