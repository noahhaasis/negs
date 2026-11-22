// The exact fit's C ABI (negs_core.h, "Phase 3b: Picture rules", Exact
// fit): pixels in and out through the caller's buffers.
package negs_core

import "../rules/fit"

@(export, link_name = "negs_fit_target")
fit_target :: proc "c" (w, h, z: f64, out: ^[2]u32) {
	if out != nil {
		out^ = fit.target(w, h, z)
	}
}

@(export, link_name = "negs_fit_worth_it")
fit_worth_it :: proc "c" (z: f64, texture_w, texture_h, target_w, target_h: u32) -> u8 {
	return u8(fit.worth_it(z, {texture_w, texture_h}, {target_w, target_h}))
}

// Writes `output` starts and counts and the weights (when `weights_cap`
// holds them all); returns how many weights there are.
@(export, link_name = "negs_fit_coefficients")
fit_coefficients :: proc "c" (input, output: u32, starts, counts: [^]u32, weights: [^]f32, weights_cap: uint) -> uint {
	context = core_context()
	n := fit.weight_count(input, output)
	if starts == nil || counts == nil || weights == nil || weights_cap < uint(n) {
		return uint(n)
	}
	taps := make([]fit.Taps, output)
	defer delete(taps)
	fit.coefficients_into(input, output, taps, weights[:n])
	for t, i in taps {
		starts[i] = t.start
		counts[i] = t.count
	}
	return uint(n)
}

@(export, link_name = "negs_fit_resample")
fit_resample :: proc "c" (rgba: [^]u8, rgba_len: uint, w, h, tw, th: u32, out: [^]u8, out_len: uint) -> u8 {
	context = core_context()
	if (rgba == nil && rgba_len > 0) || out == nil {
		return 0
	}
	source := rgba[:rgba_len] if rgba != nil else nil
	return u8(fit.resample(source, w, h, tw, th, out[:out_len]))
}
