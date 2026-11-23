// The edit state's own tests (core/src/state), with windows/src/state.rs's
// cases; core/diff/tests/edit_state.rs holds it to the Rust on random
// states. Run by `build.py test`.
package state_tests

import "core:testing"
import "../../src/state"
import "../../src/value"

parse_value :: proc(text: string) -> value.Value {
	v, _ := value.parse_string(text)
	return v
}

with_params :: proc(text: string) -> state.Edit_State {
	s := state.default_state()
	value.destroy(s.params)
	s.params = parse_value(text)
	return s
}

@(test)
reads_and_writes_as_the_derives :: proc(t: ^testing.T) {
	text := `{"params":{"io":{"rotate_cw":1}},"film":"f","paper":"p","limitPreview":false,"border":{"enabled":false,"style":"white","export":true,"instagram":false,"width":0.03,"filmWidth":0.0,"line":0.004,"color":"#ffffff","lineColor":"#000000"},"crop":{"rect":[[0.0,0.0],[1.0,1.0]],"aspect":"original","custom":[5.0,4.0],"angle":0.0}}`
	s, err := state.parse(transmute([]u8)string(text))
	defer state.destroy(s)
	testing.expect_value(t, err, value.Error.None)
	out := state.to_string(s)
	defer delete(out)
	testing.expect_value(t, out, text)
	sorted := state.to_string(s, sorted = true)
	defer delete(sorted)
	testing.expect_value(t, sorted[:11], `{"border":{`)

	// Defaults for what's missing, unknown fields ignored (even invalid UTF-8
	// and out-of-range numbers), the array form, {"name": null} enums.
	short := `{"params":[],"film":"f","paper":"p","x":["` + "\xff" + `",1e999],"border":{"style":{"film":null}}}`
	s2, err2 := state.parse(transmute([]u8)string(short))
	defer state.destroy(s2)
	testing.expect_value(t, err2, value.Error.None)
	testing.expect(t, s2.border.style == .Film && s2.border.export)
	seq := `[{}, "f", "p"]`
	s3, err3 := state.parse(transmute([]u8)string(seq))
	defer state.destroy(s3)
	testing.expect_value(t, err3, value.Error.None)
	for bad in ([]string{`{"film":"f","paper":"p"}`, `{"params":{},"film":"f","film":"g","paper":"p"}`, `{"params":{},"film":"f","paper":"p","limitPreview":null}`, `{"params":{},"film":"f","paper":"p","crop":{"rect":[[0,0],[1,1,1]]}}`, `{"params":{},"film":"f","paper":"p","roll":{"id":"r","magenta":1}}`, `{"params":{},"film":"f","paper":"p","border":{"color":"#ff"}}`, `{"params":{},"film":"f","paper":"p"`}) {
		b, e := state.parse(transmute([]u8)string(bad))
		state.destroy(b)
		testing.expectf(t, e != .None, "%s was accepted", bad)
	}
}

@(test)
rendered_state_ignores_presentation :: proc(t: ^testing.T) {
	s := with_params(`{"io": {"rotate_cw": 1}, "a": 2}`)
	defer state.destroy(s)
	r := state.rendered(s)
	defer state.destroy(r)
	s.border.enabled = true
	s.crop.angle = 3
	value.set(&s.params, "io.rotate_cw", u64(2))
	r2 := state.rendered(s)
	testing.expect(t, state.equal(r, r2))
	state.destroy(r2)
	value.set(&s.params, "a", f64(2.5))
	r3 := state.rendered(s)
	testing.expect(t, !state.equal(r, r3))
	state.destroy(r3)
}

@(test)
rendered_state_keeps_the_crop_extent_only_for_longest_side :: proc(t: ^testing.T) {
	s := with_params(`{"io": {"rotate_cw": 0, "photo_long_side_px": null}}`)
	defer state.destroy(s)
	original := state.rendered(s)
	defer state.destroy(original)
	s.crop.rect = {0.1, 0.2, 0.5, 0.5}
	r := state.rendered(s)
	testing.expect(t, state.equal(r, original))
	state.destroy(r)
	value.set(&s.params, state.SIZE_TARGET, u64(4000))
	testing.expect_value(t, state.size_mode(s.params), state.Size_Mode.Longest_Side)
	sized := state.rendered(s)
	defer state.destroy(sized)
	s.crop.rect.x = 0.3
	s.crop.angle = -2
	r = state.rendered(s)
	testing.expect(t, state.equal(r, sized))
	state.destroy(r)
	s.crop.rect.width = 0.6
	r = state.rendered(s)
	testing.expect(t, !state.equal(r, sized))
	state.destroy(r)
}

@(test)
match_inputs_leave_out_the_users_own_brightness :: proc(t: ^testing.T) {
	s := with_params(`{"prep": {"exposure_ev": 0.5}, "enlarger": {"print_exposure": 1.0, "m_filter_shift": 0.0}, "io": {"rotate_cw": 0, "output_color_space": "sRGB"}, "camera": {"exposure_compensation_ev": 0.0}}`)
	defer state.destroy(s)
	inputs := state.match_inputs(s)
	defer value.destroy(inputs)
	same := [?]string{"prep.exposure_ev", "prep.shadows", "enlarger.print_exposure", "io.rotate_cw", "io.output_color_space"}
	for path in same {
		e := state.clone(s)
		value.set(&e.params, path, u64(7))
		m := state.match_inputs(e)
		testing.expectf(t, value.same(m, inputs), "%s", path)
		value.destroy(m)
		state.destroy(e)
	}
	for path in ([]string{"camera.exposure_compensation_ev", "enlarger.m_filter_shift"}) {
		e := state.clone(s)
		value.set(&e.params, path, f64(5))
		m := state.match_inputs(e)
		testing.expectf(t, !value.same(m, inputs), "%s", path)
		value.destroy(m)
		state.destroy(e)
	}
	text := value.to_string(inputs)
	defer delete(text)
	testing.expect_value(t, text[:22], `{"film":"kodak_gold_20`)
}

@(test)
sessions_before_lens_corrections_keep_them_off :: proc(t: ^testing.T) {
	defaults := parse_value(`{"io": {"lens_correction": {"enabled": true}, "rotate_cw": 0}}`)
	old := parse_value(`{"io": {"rotate_cw": 1}}`)
	defer value.destroy(defaults)
	defer value.destroy(old)
	p := state.restored_params(defaults, old, nil)
	defer value.destroy(p)
	text := value.to_string(p)
	defer delete(text)
	testing.expect_value(t, text, `{"io":{"lens_correction":{"enabled":false},"rotate_cw":1}}`)
}

@(test)
rolls_and_rotation :: proc(t: ^testing.T) {
	s := with_params(`{"enlarger": {"m_filter_shift": 3, "y_filter_shift": -1.5}, "io": {"rotate_cw": 7.9, "flip": true}}`)
	defer state.destroy(s)
	testing.expect(t, state.roll_filtered(state.Roll{magenta = 3, yellow = -1.5}, s))
	testing.expect(t, !state.roll_filtered(state.Roll{magenta = 3, yellow = 1.5}, s))
	testing.expect_value(t, state.roll_own_filtration(state.Roll{paper = "other"}, s.paper), state.ROLL_OTHER_PAPER)
	testing.expect_value(t, state.roll_own_filtration(state.Roll{}, s.paper), state.ROLL_OWN)
	testing.expect_value(t, state.rotation(s), 7)
	testing.expect(t, state.flip(s))
	testing.expect_value(t, state.orientation(s).turns, 3)
}
