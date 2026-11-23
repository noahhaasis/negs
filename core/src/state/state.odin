// The edit (windows/src/state.rs's `EditState`, `BrightnessMatch` and
// `Roll`; `Parameters.swift` on the Mac): the engine's parameters as JSON,
// the film and paper, and the presentation settings that don't change the
// render. serde.odin reads and writes it exactly as the Rust derives do.
// core/diff/tests/edit_state.rs holds it to the frozen Rust.
//
// Phase 4a extends this package with the rest of state.rs (stocks, colour
// filters, the filter link, groups and matching, the adjustment groups).
//
// An Edit_State's strings and values live in one allocator, like a
// value.Value tree: clone, destroy.
package state

import "core:strings"
import border "../rules/border"
import crop "../rules/crop"
import "../value"

Edit_State :: struct {
	params:           value.Value,
	film:             string,
	paper:            string,
	limit_preview:    bool,
	// Drawn around the developed photograph; not part of the render.
	border:           border.Settings,
	// Trims the developed photograph; not part of the render.
	crop:             crop.Settings,
	// Prep › Match camera brightness: present while it's on.
	brightness_match: Maybe(Brightness_Match),
	// A scanned negative printed as a frame of a darkroom roll (§9.2).
	roll:             Maybe(Roll),
}

// The exposure that matches the camera's brightness, solved live for the
// settings it depends on. `prep.exposure_ev` holds the total the engine
// sees; the Exposure slider shows and edits the part above `ev`.
Brightness_Match :: struct {
	// The solved Prep exposure; none until the first solve.
	ev:                Maybe(f64),
	// `match_inputs` the value was solved for (`Option<Value>`: none when
	// `has_inputs` is false; JSON can't say Some(null), so a read never
	// gives one).
	has_inputs:        bool,
	inputs:            value.Value,
	// The solve held exposure back to keep highlights off paper white.
	highlight_limited: bool,
}

// A darkroom roll (§9.2, `NegativeRoll`): frames printed alike, with one
// filtration, each with its own exposure.
Roll :: struct {
	id:      string,
	// The roll's enlarger filtration: magenta and yellow shifts.
	magenta: f64,
	yellow:  f64,
	// The paper the roll's filtration is balanced for; none for a roll kept
	// before it was recorded.
	paper:   Maybe(string),
}

PREP_EXPOSURE :: "prep.exposure_ev"
// Flip (`IoParams::flip`): the engine mirrors the saved photograph left to
// right after the crop and before `io.rotate_cw`.
FLIP :: "io.flip"
ROTATE :: "io.rotate_cw"
// Image Size: the resolution the film develops at (a Longest side target),
// and the engine's older working-resolution factor, kept at 1.
SIZE_TARGET :: "io.photo_long_side_px"
SIZE_FACTOR :: "io.upscale_factor"
// The camera's own lens corrections for a RAW, per photograph.
LENS_CORRECTION :: "io.lens_correction.enabled"
ENLARGER_MAGENTA :: "enlarger.m_filter_shift"
ENLARGER_YELLOW :: "enlarger.y_filter_shift"

DEFAULT_FILM :: "kodak_gold_200"
DEFAULT_PAPER :: "kodak_portra_endura"

// What a brightness match leaves out of `match_inputs`: the user's own
// brightness edits and settings that can't change brightness.
MATCH_IGNORED := [?]string {
	"prep",
	"enlarger.print_exposure",
	"post",
	"film_render.grain",
	"io.rotate_cw",
	"io.flip",
	"io.crop",
	"io.crop_center",
	"io.crop_size",
	"io.output_color_space",
	"io.output_cctf_encoding",
	"settings.serial_grain",
}

Size_Mode :: enum u32 {
	Original,
	Longest_Side,
}

ROLL_OTHER_PAPER :: "On another paper than its roll, so it has its own filtration."
ROLL_OWN :: "This frame has its own filtration."

// `EditState::default()`.
default_state :: proc(allocator := context.allocator) -> Edit_State {
	return Edit_State {
		params = value.make_object(0, allocator),
		film = strings.clone(DEFAULT_FILM, allocator),
		paper = strings.clone(DEFAULT_PAPER, allocator),
		border = border.DEFAULT_SETTINGS,
		crop = crop.DEFAULT_SETTINGS,
	}
}

clone :: proc(s: Edit_State, allocator := context.allocator) -> Edit_State {
	out := s
	out.params = value.clone(s.params, allocator)
	out.film = strings.clone(s.film, allocator)
	out.paper = strings.clone(s.paper, allocator)
	if m, ok := s.brightness_match.?; ok {
		m.inputs = value.clone(m.inputs, allocator)
		out.brightness_match = m
	}
	if r, ok := s.roll.?; ok {
		r.id = strings.clone(r.id, allocator)
		if paper, has := r.paper.?; has {
			r.paper = strings.clone(paper, allocator)
		}
		out.roll = r
	}
	return out
}

destroy :: proc(s: Edit_State, allocator := context.allocator) {
	value.destroy(s.params, allocator)
	delete(s.film, allocator)
	delete(s.paper, allocator)
	if m, ok := s.brightness_match.?; ok {
		value.destroy(m.inputs, allocator)
	}
	if r, ok := s.roll.?; ok {
		delete(r.id, allocator)
		if paper, has := r.paper.?; has {
			delete(paper, allocator)
		}
	}
}

// MARK: Equality, as the Rust PartialEqs

border_equal :: proc "contextless" (a, b: border.Settings) -> bool {
	return(
		a.enabled == b.enabled &&
		a.style == b.style &&
		a.export == b.export &&
		a.instagram == b.instagram &&
		a.width == b.width &&
		a.film_width == b.film_width &&
		a.line == b.line &&
		a.color == b.color &&
		a.line_color == b.line_color \
	)
}

crop_equal :: proc "contextless" (a, b: crop.Settings) -> bool {
	return(
		crop.rect_eq(a.rect, b.rect) &&
		a.aspect == b.aspect &&
		a.custom.width == b.custom.width &&
		a.custom.height == b.custom.height &&
		a.angle == b.angle \
	)
}

maybe_f64_equal :: proc "contextless" (a, b: Maybe(f64)) -> bool {
	x, has_x := a.?
	y, has_y := b.?
	return has_x == has_y && (!has_x || x == y)
}

// BrightnessMatch's PartialEq: the inputs compared with `json::same`.
match_equal :: proc(a, b: Brightness_Match) -> bool {
	return(
		maybe_f64_equal(a.ev, b.ev) &&
		a.highlight_limited == b.highlight_limited &&
		a.has_inputs == b.has_inputs &&
		(!a.has_inputs || value.same(a.inputs, b.inputs)) \
	)
}

// Roll's derived PartialEq.
roll_equal :: proc(a, b: Roll) -> bool {
	pa, has_a := a.paper.?
	pb, has_b := b.paper.?
	return a.id == b.id && a.magenta == b.magenta && a.yellow == b.yellow && has_a == has_b && (!has_a || pa == pb)
}

// EditState's PartialEq: params compared with `json::same`.
equal :: proc(a, b: Edit_State) -> bool {
	if !value.same(a.params, b.params) || a.film != b.film || a.paper != b.paper || a.limit_preview != b.limit_preview {
		return false
	}
	if !border_equal(a.border, b.border) || !crop_equal(a.crop, b.crop) {
		return false
	}
	ma, has_ma := a.brightness_match.?
	mb, has_mb := b.brightness_match.?
	if has_ma != has_mb || (has_ma && !match_equal(ma, mb)) {
		return false
	}
	ra, has_ra := a.roll.?
	rb, has_rb := b.roll.?
	return has_ra == has_rb && (!has_ra || roll_equal(ra, rb))
}

// MARK: Rules

// Rust's `f64 as i32`: saturating, NaN 0.
saturating_i32 :: proc "contextless" (v: f64) -> i32 {
	switch {
	case v != v:
		return 0
	case v >= 2147483647:
		return max(i32)
	case v <= -2147483648:
		return min(i32)
	}
	return i32(v)
}

// Rust's `f64 as u32`.
saturating_u32 :: proc "contextless" (v: f64) -> u32 {
	switch {
	case v != v || v <= 0:
		return 0
	case v >= 4294967295:
		return max(u32)
	}
	return u32(v)
}

// `EditState::value`: the parameter at a dotted path (borrowed).
value_at :: proc(s: Edit_State, path: string) -> value.Value {
	return value.get(s.params, path)
}

// `EditState::rotation` from `io.rotate_cw`'s value.
rotation_of :: proc "contextless" (v: value.Value) -> i32 {
	return saturating_i32(value.number(v))
}

// `EditState::flip` from `io.flip`'s value.
flip_of :: proc "contextless" (v: value.Value) -> bool {
	return value.boolean(v)
}

rotation :: proc(s: Edit_State) -> i32 {
	return rotation_of(value_at(s, ROTATE))
}

flip :: proc(s: Edit_State) -> bool {
	return flip_of(value_at(s, FLIP))
}

orientation :: proc(s: Edit_State) -> crop.Orientation {
	return crop.orientation(rotation(s), flip(s))
}

// `size_target` from `io.photo_long_side_px`'s value.
size_target_of :: proc "contextless" (v: value.Value) -> Maybe(u32) {
	n, ok := value.as_f64(v)
	if !ok {
		return nil
	}
	return saturating_u32(n)
}

// The Longest side target, if set.
size_target :: proc(params: value.Value) -> Maybe(u32) {
	return size_target_of(value.get(params, SIZE_TARGET))
}

size_mode :: proc(params: value.Value) -> Size_Mode {
	_, ok := size_target(params).?
	return .Longest_Side if ok else .Original
}

// `EditState::rendered`: the state without presentation-only settings, to
// decide whether an edit needs a new render: no border, crop or match, no
// rotation or flip; with a Longest side target the crop's extent stays.
rendered :: proc(s: Edit_State, allocator := context.allocator) -> Edit_State {
	out := clone(s, allocator)
	out.border = border.DEFAULT_SETTINGS
	out.crop = crop.DEFAULT_SETTINGS
	if m, ok := out.brightness_match.?; ok {
		value.destroy(m.inputs, allocator)
	}
	out.brightness_match = nil
	if size_mode(s.params) == .Longest_Side && crop.cropped(s.crop) {
		out.crop.rect.width = s.crop.rect.width
		out.crop.rect.height = s.crop.rect.height
	}
	value.set(&out.params, ROTATE, nil, allocator)
	value.set(&out.params, FLIP, nil, allocator)
	return out
}

// `match_inputs`: what a brightness match depends on, as
// `{"film", "paper", "params"}` (a Value, so its keys are sorted).
match_inputs :: proc(s: Edit_State, allocator := context.allocator) -> value.Value {
	params := value.clone(s.params, allocator)
	for path in MATCH_IGNORED {
		value.set(&params, path, nil, allocator)
	}
	out := value.make_object(3, allocator)
	append(&out, value.Field{strings.clone("film", allocator), strings.clone(s.film, allocator)})
	append(&out, value.Field{strings.clone("paper", allocator), strings.clone(s.paper, allocator)})
	append(&out, value.Field{strings.clone("params", allocator), params})
	return out
}

// `restored_params`: a saved edit's params on today's defaults. A session
// from before lens corrections was developed without them, so they stay
// off.
restored_params :: proc(defaults, saved: value.Value, nullable: []string, allocator := context.allocator) -> value.Value {
	params := value.restoring(defaults, saved, nullable, allocator)
	if value.get(saved, LENS_CORRECTION) == nil {
		value.set(&params, LENS_CORRECTION, false, allocator)
	}
	return params
}

// `Roll::filtered` from the state's two shifts: it prints with the roll's
// filtration, not one of its own.
roll_filtered_of :: proc "contextless" (r_magenta, r_yellow: f64, magenta, yellow: value.Value) -> bool {
	m, has_m := value.as_f64(magenta)
	y, has_y := value.as_f64(yellow)
	return has_m && m == r_magenta && has_y && y == r_yellow
}

roll_filtered :: proc(r: Roll, s: Edit_State) -> bool {
	return roll_filtered_of(r.magenta, r.yellow, value_at(s, ENLARGER_MAGENTA), value_at(s, ENLARGER_YELLOW))
}

// `Roll::own_filtration`: Prep's word for a frame of this roll with its own
// filtration (static).
roll_own_filtration :: proc(r: Roll, paper: string) -> string {
	if roll_paper, ok := r.paper.?; ok && roll_paper != paper {
		return ROLL_OTHER_PAPER
	}
	return ROLL_OWN
}
