// Prep › Match camera brightness and the Exposure above it (facts.rs's
// `Brightness`). The match's inputs (`match_inputs`, a copy of the
// params) are worked out once per update, and only when a failure or a
// pending match needs them.
package state_facts

import "core:strings"
import "../../value"
import ".."

matches :: proc(f: ^Facts) -> bool {
	if f.host != nil {
		return f.host.flags(f.host.user) & HOST_MATCHES != 0
	}
	if f.state == nil {
		return false
	}
	_, ok := f.state.brightness_match.?
	return ok
}

@(private)
current_inputs :: proc(f: ^Facts) -> value.Value {
	if m, ok := f.match_inputs.?; ok {
		return m
	}
	m := state.match_inputs(f.state^, update_allocator(f))
	f.match_inputs = m
	return m
}

@(private)
failed_inputs :: proc(f: ^Facts, failure: Failure) -> value.Value {
	if v, ok := f.failure_inputs.?; ok {
		return v
	}
	v, _ := value.parse_string(failure.inputs_text, update_allocator(f))
	f.failure_inputs = v
	return v
}

// Why matching can't run for this photograph; ok false when it can.
unavailable :: proc(f: ^Facts) -> (string, bool) {
	if fl, ok := f.input.failure.?; ok && !fl.has_inputs && fl.generation == f.input.generation {
		return fl.message, true
	}
	if !f.input.is_raw && value.string_of(value_at(f, INPUT_MODE)) != "auto" {
		return UNAVAILABLE, true
	}
	return "", false
}

failed_here :: proc(f: ^Facts) -> bool {
	if f.failed >= 0 {
		return f.failed == 1
	}
	fl, ok := f.input.failure.?
	here := ok && fl.has_inputs && fl.generation == f.input.generation && f.state != nil && value.same(failed_inputs(f, fl), current_inputs(f))
	f.failed = int(here)
	return here
}

// On, possible, and not failed for the current settings.
active :: proc(f: ^Facts) -> bool {
	if !matches(f) {
		return false
	}
	if _, no := unavailable(f); no {
		return false
	}
	return !failed_here(f)
}

// On and not yet solved for the current settings. Save and Export wait.
pending :: proc(f: ^Facts) -> bool {
	if f.state == nil {
		return false
	}
	m, ok := f.state.brightness_match.?
	if !ok || !active(f) {
		return false
	}
	if _, has_ev := m.ev.?; !has_ev {
		return true
	}
	return !(m.has_inputs && value.same(m.inputs, current_inputs(f)))
}

// The note under the Exposure slider, in `allocator`; ok false for none.
note :: proc(f: ^Facts, allocator := context.allocator) -> (string, bool) {
	if !matches(f) {
		return "", false
	}
	if text, no := unavailable(f); no {
		return strings.clone(text, allocator), true
	}
	if failed_here(f) {
		fl := f.input.failure.?
		return strings.clone(fl.message, allocator), true
	}
	if pending(f) || f.input.matching {
		return strings.clone("Matching…", allocator), true
	}
	m := f.state.brightness_match.?
	ev, has := m.ev.?
	if !has {
		return "", false
	}
	b := strings.builder_make(0, 80, allocator)
	strings.write_string(&b, "Starts at ")
	strings.write_string(&b, "+" if ev >= 0.0 else "−")
	state.write_fixed(&b, abs(ev), 2)
	strings.write_string(&b, " EV to match ")
	strings.write_string(&b, "the camera" if f.input.is_raw else "the original")
	strings.write_string(&b, ", held back to protect highlights." if m.highlight_limited else ".")
	return strings.to_string(b), true
}

matched_ev :: proc(f: ^Facts) -> f64 {
	if !active(f) {
		return 0
	}
	m := f.state.brightness_match.?
	ev, _ := m.ev.?
	return ev
}

// The Exposure slider's range: the engine's total stays within ±3 EV.
exposure_offset_range :: proc(f: ^Facts) -> (f64, f64) {
	matched := matched_ev(f)
	return state.PREP_EXPOSURE_LOW - matched, state.PREP_EXPOSURE_HIGH - matched
}

// The Exposure slider: the user's exposure above the match.
exposure_offset :: proc(f: ^Facts) -> f64 {
	if f.host != nil {
		e: Exposure
		f.host.exposure(f.host.user, &e)
		return e.offset
	}
	return value.number(value_at(f, state.PREP_EXPOSURE)) - matched_ev(f)
}

exposure_offset_modified :: proc(f: ^Facts) -> bool {
	if f.host != nil {
		e: Exposure
		f.host.exposure(f.host.user, &e)
		return e.modified != 0
	}
	return abs(value.number(value_at(f, state.PREP_EXPOSURE)) - matched_ev(f)) > 1e-6
}

// Whether Prep shows Auto exposure and Metering.
shows_meter :: proc(f: ^Facts) -> bool {
	return !active(f) || is_modified(f, "camera.auto_exposure") || is_modified(f, "camera.auto_exposure_method")
}
