// Your negative (windows/src/negative.rs, Negative.swift, ticket 33): a scan
// of a real colour negative, read back to the film's dyes with upstream's
// convert-film route, then printed. The words and the pure rules the label,
// the tray and Prep share. What changes an edit state (joining a roll,
// leaving a negative, keeping it under a preset) is Phase 4's and stays with
// the host's state; these take the state's fields as plain values.
package negative

import "core:c/libc"
import "core:fmt"
import "core:strings"
import "../crop"

// Upstream's `workflow.route` strings.
ROUTE :: "input > convert-film > print > scan"
SCENE_ROUTE :: "input > film > print > scan"
ROUTE_PATH :: "workflow.route"
// Read as this unless the photograph's film is a colour negative already.
DEFAULT_FILM :: "kodak_portra_400"
// The tray's "Your negative" tile; never a stock the helper knows.
STOCK_ID :: "negs.your-negative"
NAME :: "Your negative"
TILE_TEXT :: "Read the photograph as a scan of a colour negative"
SCAN_ILLUMINANT :: "film_render.convert.scan_illuminant"
// The light frame each scan is divided by: a path, or empty for off.
FLAT_FIELD :: "film_render.convert.flat_field"
FLAT_FIELD_HELP :: "A frame of the bare light, shot with the roll's camera, lens and aperture. Each scan is divided by it before the base is read, so the light's falloff doesn't print."

// What reading a negative sets, put back to the defaults when the
// photograph is printed as a scene again.
OWN_PATHS := [?]cstring{ROUTE_PATH, "film_render.base", "film_render.convert", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"}

// What finding the base sets: this frame's measurements.
FOUND_PATHS := [?]cstring{"film_render.base", "film_render.convert.exposure_compensation_ev", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"}

// What a preset never changes on a negative: how it's read and its print
// balance. The preset brings the look and the paper.
PRESET_KEEPS := [?]cstring {
	"workflow",
	"film_render.base",
	"film_render.convert",
	"enlarger.m_filter_shift",
	"enlarger.y_filter_shift",
	"enlarger.print_exposure",
	"io.raw_white_balance",
	"io.lens_correction",
	"io.input_interpretation",
}

// How the scan is read from the file; a roll's frames share it.
SCAN_READING := [?]cstring{"io.raw_white_balance", "io.lens_correction", "io.input_interpretation"}

// What a camera recorded of a scan's light: shutter (seconds), aperture
// and ISO, each if recorded.
Scan_Light :: [3]Maybe(f64)

// How much more light the camera gave a frame's scan than the reference's,
// in stops, from what both recorded; none when they share none.
scan_stops :: proc "contextless" (frame, reference: Scan_Light) -> Maybe(f64) {
	stops: f64
	any := false
	for i in 0 ..< 3 {
		v, ok1 := frame[i].?
		b, ok2 := reference[i].?
		if !ok1 || !ok2 {
			continue
		}
		term: f64
		switch i {
		case 0, 2:
			term = libc.log2(v / b)
		case 1:
			term = 2.0 * libc.log2(b / v)
		}
		if !any {
			stops = 0
			any = true
		}
		stops += term
	}
	if !any {
		return nil
	}
	return stops
}

// The base tuning's exposure for a frame without clear film of its own:
// the roll's, less the extra light its scan got.
exposure_ev :: proc "contextless" (roll: f64, scan_stops: Maybe(f64)) -> f64 {
	return roll - (scan_stops.? or_else 0.0)
}

// The whole roll's word for the photographs it left alone.
skipped :: proc(count: uint, allocator := context.allocator) -> string {
	if count == 1 {
		return strings.clone("1 photograph skipped: it doesn't look like a negative.", allocator)
	}
	return fmt.aprintf("%d photographs skipped: they don't look like negatives.", count, allocator = allocator)
}

is_route :: proc "contextless" (route: string) -> bool {
	return route == ROUTE
}

// A film a negative can be read as: a colour negative.
readable :: proc "contextless" (paper, positive, bw: bool) -> bool {
	return !paper && !positive && !bw
}

// `path` is, holds or lies in one of FOUND_PATHS.
is_found :: proc "contextless" (path: string) -> bool {
	for f in FOUND_PATHS {
		found := string(f)
		if found == path {
			return true
		}
		if len(path) > len(found) && path[:len(found)] == found && path[len(found)] == '.' {
			return true
		}
		if len(found) > len(path) && found[:len(path)] == path && found[len(path)] == '.' {
			return true
		}
	}
	return false
}

// The crop to the picture the rebate finder found (`negative_base`'s
// `picture`: x0, y0, x1, y1, NaN for a missing one, and its angle). None
// unless it lies in the photograph.
picture_crop :: proc "contextless" (x0, y0, x1, y1, angle: f64) -> Maybe(crop.Settings) {
	finite :: proc "contextless" (v: f64) -> bool {return v - v == 0}
	ok := finite(x0) && finite(y0) && finite(x1) && finite(y1) && 0 <= x0 && x0 < x1 && x1 <= 1 && 0 <= y0 && y0 < y1 && y1 <= 1
	if !ok {
		return nil
	}
	s := crop.DEFAULT_SETTINGS
	s.rect = crop.Rect{x0, y0, x1 - x0, y1 - y0}
	s.aspect = .Freeform
	s.angle = crop.clamp_f64(angle, -crop.MAX_ANGLE, crop.MAX_ANGLE)
	return s
}

// Prep's Flat field row: the light frame's name, or Off.
flat_field_value :: proc "contextless" (path: string) -> string {
	if len(path) == 0 {
		return "Off"
	}
	for i := len(path) - 1; i >= 0; i -= 1 {
		if path[i] == '/' || path[i] == '\\' {
			return path[i + 1:]
		}
	}
	return path
}

// Under the row: why this frame doesn't use the light frame. `used` is the
// helper's answer (none until it has said), `reason` its words.
flat_field_hint :: proc "contextless" (path: string, used: Maybe(bool), reason: Maybe(string)) -> Maybe(string) {
	u, said := used.?
	if len(path) == 0 || !said || u {
		return nil
	}
	return reason.? or_else "The light frame isn't used on this frame."
}

// Prep's word when the scan doesn't look like a colour negative.
not_negative :: proc "contextless" (raw: bool) -> cstring {
	if raw {
		return "This doesn't look like a colour negative. If it is, set White balance to Daylight."
	}
	return "This doesn't look like a colour negative."
}

// The photograph's edge ("left", "top"…) as `o` shows it; none for a word
// that isn't an edge (the host keeps it as it is).
shown_edge :: proc "contextless" (o: crop.Orientation, edge: string) -> Maybe(cstring) {
	point: crop.Point
	switch edge {
	case "left":
		point = {0, 0.5}
	case "right":
		point = {1, 0.5}
	case "top":
		point = {0.5, 0}
	case "bottom":
		point = {0.5, 1}
	case:
		return nil
	}
	p := crop.shown_point(o, point)
	if p.x < 0.25 {
		return "left"
	} else if p.x > 0.75 {
		return "right"
	} else if p.y < 0.25 {
		return "top"
	}
	return "bottom"
}

// Where the film base shown came from (`NegativeScan.BaseStatus`).
Base_Status :: struct {
	found, picked: bool,
	// A base no detection made while the photograph is open.
	by_hand:       bool,
	// The roll's base, on a frame without clear film of its own.
	from_roll:     bool,
	edge:          Maybe(string),
}

// The base is a guess from the picture: no clear film was found.
guessed :: proc "contextless" (s: Base_Status) -> bool {
	return !s.found && !s.picked && !s.by_hand && !s.from_roll
}

sentence :: proc(s: Base_Status, o: crop.Orientation, allocator := context.allocator) -> string {
	if s.from_roll {
		return strings.clone("Film base from the roll: this frame shows no clear film of its own.", allocator)
	}
	if s.by_hand {
		return strings.clone("Film base as picked or set in Lab.", allocator)
	}
	if s.picked {
		return strings.clone("Film base read where you clicked.", allocator)
	}
	if s.found {
		edge, ok := s.edge.?
		if !ok {
			return strings.clone("Film base found all round the picture.", allocator)
		}
		word := shown_edge(o, edge).? or_else ""
		shown := string(word) if word != "" else edge
		return fmt.aprintf("Film base found along the %s edge.", shown, allocator = allocator)
	}
	return strings.clone("No clear film found: the base is a guess from the thinnest part of the picture.", allocator)
}

// What a click on the photograph reads while picking.
Pick :: enum u32 {
	// A click on clear film (the rebate) reads the base there.
	Base,
	// A click on something grey balances the print on it.
	Neutral,
}

PICK_PROMPTS := [Pick]cstring {
	.Base    = "Click the clear film between frames or along the edge. Esc cancels.",
	.Neutral = "Click something that should be grey. Esc cancels.",
}

// Two numbers of a film base are the same, allowing for the last bit.
same_number :: proc "contextless" (p, q: f64) -> bool {
	return crop.fabs(p - q) <= 1e-9 * crop.fmax(crop.fabs(q), 1.0)
}
