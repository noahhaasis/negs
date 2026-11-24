// A match solved for a photograph's edits, applied as opening applies it
// (`Model::apply_brightness_match`, batch.rs's `matched`).
package batch

import "../../state"
import "../../value"

// state.rs's PREP_EXPOSURE_RANGE: the Prep exposure slider's range.
EXPOSURE_LOW :: -3.0
EXPOSURE_HIGH :: 3.0

// The new `prep.exposure_ev` for a match of `ev` over a state whose
// exposure is `exposure` and whose match was `old` (none before its first
// solve): the total moves with the match, keeping the user's exposure
// above it. A change under 0.05 EV keeps the value (`set` false).
matched_exposure :: proc(exposure: value.Value, old: Maybe(f64), ev: f64) -> (total: value.Value, set: bool) {
	old_ev, solved := old.?
	offset := value.number(exposure) - (old_ev if solved else 0)
	if solved && !(abs(ev - old_ev) >= 0.05) {
		return nil, false
	}
	t := ev if abs(offset) < 1e-9 else ev + offset
	// f64::clamp: a NaN stays a NaN.
	if t < EXPOSURE_LOW {
		t = EXPOSURE_LOW
	}
	if t > EXPOSURE_HIGH {
		t = EXPOSURE_HIGH
	}
	return value.num(t), true
}

// `matched`: `s` with the match applied, in `allocator`; false without
// the match on. `inputs` is copied.
matched :: proc(s: state.Edit_State, ev: f64, limited: bool, inputs: value.Value, allocator := context.allocator) -> (state.Edit_State, bool) {
	m, on := s.brightness_match.?
	if !on {
		return {}, false
	}
	next := state.clone(s, allocator)
	if total, set := matched_exposure(state.value_at(s, state.PREP_EXPOSURE), m.ev, ev); set {
		m.ev = ev
		value.set(&next.params, state.PREP_EXPOSURE, total, allocator)
	}
	nm := next.brightness_match.?
	value.destroy(nm.inputs, allocator)
	nm.ev = m.ev
	nm.inputs = value.clone(inputs, allocator)
	nm.has_inputs = true
	nm.highlight_limited = limited
	next.brightness_match = nm
	return next, true
}
