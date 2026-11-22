// The tone graph's curve and graph maths (windows/src/tone_graph.rs, frozen
// in core/diff/oracle; a port of ToneGraph.swift): the paper's response to
// a ramp of greys, predicted for new print controls, its handles, hits and
// drags. Parsing the helper's JSON and comparing requests is the host's
// state plumbing; the series come in as slices.
package tone

// The controls the graph moves, as they act (negs_tone_controls_t).
Controls :: struct {
	print_exposure: f64,
	gamma:          f64,
	fast:           f64,
	slow:           f64,
}

// The graph's frame in pixels (negs_tone_geometry_t).
Geometry :: struct {
	width:  f64,
	height: f64,
	inset:  f64,
}

// A curve's (or a display's) series, borrowed: mean film exposure per grey
// in stops, the scan per channel, and the paper's log exposure per channel
// unless the film is scanned directly. The host checks the channels are at
// least as long as the original indexes them.
Curve :: struct {
	stops:     []f64,
	rgb:       [3][]f64,
	paper:     [3][]f64,
	has_paper: bool,
}

Handle_Kind :: enum u32 {
	Middle     = 0,
	Shadows    = 1,
	Highlights = 2,
	// Anywhere else on the curve, at a tone in stops.
	Curve      = 3,
}

Handle :: struct {
	kind: Handle_Kind,
	tone: f64,
}

// The sliders' ranges and step, which the drags keep to.
EXPOSURE_LOW: f64 = 0.1
EXPOSURE_HIGH: f64 = 5.0
GAMMA_LOW: f64 = 0.5
GAMMA_HIGH: f64 = 2.0
STEP: f64 = 0.01
// The film exposures the graph shows, in stops over mid-grey.
SHOWN_LOW: f64 = -6.0
SHOWN_HIGH: f64 = 6.0

// What a drag sets (the parameter paths are the host's).
Target :: enum u32 {
	None           = 0,
	Print_Exposure = 1,
	Gamma          = 2,
	Fast           = 3,
	Slow           = 4,
}

// A series per grey: one channel's, or (channel < 0) the luma of three or
// their mean, computed as the Rust's Vec would hold it.
Series :: struct {
	c:       [3][]f64,
	channel: int,
	luma:    bool,
}

plain :: proc "contextless" (s: []f64) -> Series {
	return Series{c = {s, nil, nil}, channel = 0}
}

series_len :: proc "contextless" (s: Series) -> int {
	return len(s.c[s.channel]) if s.channel >= 0 else len(s.c[0])
}

luma :: proc "contextless" (r, g, b: f64) -> f64 {
	return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

series_at :: proc "contextless" (s: Series, i: int) -> f64 {
	if s.channel >= 0 {
		return s.c[s.channel][i]
	}
	if s.luma {
		return luma(s.c[0][i], s.c[1][i], s.c[2][i])
	}
	return (s.c[0][i] + s.c[1][i] + s.c[2][i]) / 3.0
}

// The curve's brightness per grey for `channel` (< 0: the luma).
brightness :: proc "contextless" (rgb: [3][]f64, channel: int) -> Series {
	return Series{c = rgb, channel = channel, luma = true}
}

// A per-channel series for `channel` (< 0: the channels' mean).
mean_or :: proc "contextless" (series: [3][]f64, channel: int) -> Series {
	return Series{c = series, channel = channel}
}

// The fractional index where a decreasing series reaches `value`, clamped
// to the ends. `negate` reads the series and value negated (an increasing
// series), with the same steps as the Rust's negated copy.
index_of :: proc "contextless" (s: Series, value: f64, negate: bool) -> f64 {
	n := series_len(s)
	if n == 0 {
		return 0
	}
	at := proc "contextless" (s: Series, i: int, negate: bool) -> f64 {
		v := series_at(s, i)
		return -v if negate else v
	}
	v := -value if negate else value
	if v >= at(s, 0, negate) {
		return 0
	}
	if v <= at(s, n - 1, negate) {
		return f64(n - 1)
	}
	lo, hi := 0, n - 1
	for hi - lo > 1 {
		mid := (lo + hi) / 2
		if at(s, mid, negate) > v {
			lo = mid
		} else {
			hi = mid
		}
	}
	span := at(s, lo, negate) - at(s, hi, negate)
	return f64(lo) + ((at(s, lo, negate) - v) / span if span > 0 else 0)
}

index_of_decreasing :: proc "contextless" (s: Series, value: f64) -> f64 {
	return index_of(s, value, false)
}

index_of_increasing :: proc "contextless" (s: Series, value: f64) -> f64 {
	return index_of(s, value, true)
}

interpolate :: proc "contextless" (s: Series, index: f64) -> f64 {
	n := series_len(s)
	if n == 0 {
		return 0
	}
	i := clamp(index, 0, f64(n - 1))
	lo := as_index(floor(i))
	hi := min(lo + 1, n - 1)
	a := series_at(s, lo)
	return a + (series_at(s, hi) - a) * (i - f64(lo))
}

controls_equal :: proc "contextless" (a, b: Controls) -> bool {
	return a.print_exposure == b.print_exposure && a.gamma == b.gamma && a.fast == b.fast && a.slow == b.slow
}

// Curve::predicted: the curve `next` would give, predicted from this one,
// into `rgb_out` and `paper_out` (copies of the curve's rgb and paper, at
// least as long as its stops). False when there's nothing to predict (no
// paper, or the same controls): the display is the curve as it is.
predicted :: proc "contextless" (curve: Curve, controls, next: Controls, rgb_out, paper_out: [3][]f64) -> bool {
	if !curve.has_paper || controls_equal(next, controls) {
		return false
	}
	shift := log10(next.print_exposure / controls.print_exposure)
	gamma := next.gamma / controls.gamma
	slow, fast := next.slow / controls.slow, next.fast / controls.fast
	for c in 0 ..< 3 {
		paper := plain(curve.paper[c])
		rgb := plain(curve.rgb[c])
		for i in 0 ..< len(curve.stops) {
			x := curve.paper[c][i] + shift
			k := gamma * (slow if x >= 0 else fast)
			paper_out[c][i] = x
			rgb_out[c][i] = interpolate(rgb, index_of_decreasing(paper, k * x))
		}
	}
	return true
}

// Display::handles: where the paper's exposure is 0, and 2 stops either
// side; false if the paper exposure never crosses 0.
handles :: proc "contextless" (display: Curve, channel: int) -> (shadows, middle, highlights: f64, ok: bool) {
	if !display.has_paper {
		return
	}
	series := mean_or(display.paper, channel)
	n := series_len(series)
	if n == 0 {
		return
	}
	first, last := series_at(series, 0), series_at(series, n - 1)
	if !(first > 0 && last < 0) {
		return
	}
	middle = interpolate(plain(display.stops), index_of_decreasing(series, 0))
	return middle - 2, middle, middle + 2, true
}

// Display::value: the series at `stops` film exposure.
value_at :: proc "contextless" (display: Curve, series: Series, stops: f64) -> f64 {
	return interpolate(series, index_of_increasing(plain(display.stops), stops))
}

x_of :: proc "contextless" (g: Geometry, stops: f64) -> f64 {
	return (stops - SHOWN_LOW) / (SHOWN_HIGH - SHOWN_LOW) * g.width
}

stops_of :: proc "contextless" (g: Geometry, x: f64) -> f64 {
	return SHOWN_LOW + x / fmax(g.width, 1.0) * (SHOWN_HIGH - SHOWN_LOW)
}

y_of :: proc "contextless" (g: Geometry, b: f64) -> f64 {
	return g.inset + (1.0 - clamp(b, 0.0, 1.0)) * (g.height - 2.0 * g.inset)
}

brightness_of :: proc "contextless" (g: Geometry, y: f64) -> f64 {
	return clamp(1.0 - (y - g.inset) / fmax(g.height - 2.0 * g.inset, 1.0), 0.001, 0.999)
}

Handle_Point :: struct {
	kind: Handle_Kind,
	x, y: f64,
}

// Geometry::handle_points: shadows, middle, highlights; none without
// handles.
handle_points :: proc "contextless" (g: Geometry, display: Curve, channel: int) -> (points: [3]Handle_Point, count: int) {
	shadows, middle, highlights, ok := handles(display, channel)
	if !ok {
		return
	}
	curve := brightness(display.rgb, channel)
	tones := [3]f64{shadows, middle, highlights}
	kinds := [3]Handle_Kind{.Shadows, .Middle, .Highlights}
	for t, i in tones {
		points[i] = {kinds[i], x_of(g, t), y_of(g, value_at(display, curve, t))}
	}
	return points, 3
}

// Geometry::hit: the handle within `radius` of the point, or the curve
// within `tolerance` of it.
hit :: proc "contextless" (g: Geometry, display: Curve, channel: int, px, py, radius, tolerance: f64) -> (Handle, bool) {
	points, count := handle_points(g, display, channel)
	best := -1
	best_d: f64
	for p, i in points[:count] {
		d := hypot(p.x - px, p.y - py)
		if d < radius && (best < 0 || d < best_d) {
			best, best_d = i, d
		}
	}
	if best >= 0 {
		return {points[best].kind, 0}, true
	}
	if !display.has_paper {
		return {}, false
	}
	t := stops_of(g, px)
	y := y_of(g, value_at(display, brightness(display.rgb, channel), t))
	if abs(y - py) < tolerance {
		return {.Curve, t}, true
	}
	return {}, false
}

// Drag::start's tone: the curve's own, or the handle's; false without
// handles.
drag_tone :: proc "contextless" (handle: Handle, display: Curve, channel: int) -> (f64, bool) {
	if handle.kind == .Curve {
		return handle.tone, true
	}
	shadows, middle, highlights, ok := handles(display, channel)
	if !ok {
		return 0, false
	}
	#partial switch handle.kind {
	case .Shadows:
		return shadows, true
	case .Highlights:
		return highlights, true
	}
	return middle, true
}

// Drag::value: the control the pointer at (px, py) asks for, snapped to
// its slider's range and step; None where it can't be solved. Solved on
// the drag's own `base` curve.
drag_value :: proc "contextless" (handle: Handle_Kind, tone: f64, base: Curve, start, current: Controls, channel: int, g: Geometry, px, py: f64) -> (Target, f64) {
	if !base.has_paper {
		return .None, 0
	}
	paper := mean_or(base.paper, channel)
	stops := plain(base.stops)
	target: Target
	value, low, high: f64
	if handle == .Middle {
		// Paper exposure 0 under the pointer: new = base · 10^(−paper(t)).
		t := stops_of(g, px)
		p := interpolate(paper, index_of_increasing(stops, t))
		target, value, low, high = .Print_Exposure, start.print_exposure * exp10(-p), EXPOSURE_LOW, EXPOSURE_HIGH
	} else {
		// The tone on the base curve that already has the pointer's
		// brightness; the scale is its paper exposure over the handle's.
		reached := interpolate(paper, index_of_increasing(brightness(base.rgb, channel), brightness_of(g, py)))
		shift := log10(current.print_exposure / start.print_exposure)
		at := interpolate(paper, index_of_increasing(stops, tone)) + shift
		if abs(at) <= 0.03 {
			return .None, 0
		}
		k := fmax(reached / at, 0.01)
		side := current.slow / start.slow if at >= 0 else current.fast / start.fast
		gamma := current.gamma / start.gamma
		low, high = GAMMA_LOW, GAMMA_HIGH
		#partial switch handle {
		case .Shadows:
			target, value = .Slow, start.slow * k / gamma
		case .Highlights:
			target, value = .Fast, start.fast * k / gamma
		case:
			target, value = .Gamma, start.gamma * k / side
		}
	}
	if value - value != 0 {
		return .None, 0
	}
	return target, snap(value, low, high, STEP)
}

handle_title :: proc "contextless" (kind: Handle_Kind) -> string {
	switch kind {
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

handle_hint :: proc "contextless" (kind: Handle_Kind) -> string {
	return "drag sideways" if kind == .Middle else "drag up or down"
}
