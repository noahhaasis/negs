// The exact fit's own tests (core/src/rules/fit), with
// windows/src/exact_fit.rs's values. core/diff/tests/exact_fit.rs holds
// the pixels to the Rust byte for byte. Run by `python3 core/build.py test`.
package fit_tests

import "core:testing"
import fit "../../../src/rules/fit"

@(test)
targets_round_the_fitted_size :: proc(t: ^testing.T) {
	testing.expect_value(t, fit.target(6000, 4000, 2200.0 / 6000.0), [2]u32{2200, 1467})
	testing.expect_value(t, fit.target(4896, 3264, 1035.0 / 3264.0), [2]u32{1553, 1035})
	testing.expect_value(t, fit.target(333, 777, 0.5), [2]u32{167, 389})
}

@(test)
only_shrinking_larger_textures :: proc(t: ^testing.T) {
	testing.expect(t, fit.worth_it(0.317, {4896, 3264}, {1553, 1035}))
	testing.expect(t, !fit.worth_it(1, {4896, 3264}, {4896, 3264}))
	testing.expect(t, !fit.worth_it(0.3, {1200, 800}, {1553, 1035}))
}

@(test)
coefficients_follow_pillow :: proc(t: ^testing.T) {
	c := fit.coefficients(100, 50)
	defer fit.coefficients_free(c)
	taps := c.taps[25]
	testing.expect_value(t, taps.start, 45)
	testing.expect_value(t, taps.count, 12)
	weights := c.weights[taps.first:][:taps.count]
	sum: f32
	for w in weights {
		sum += w
	}
	testing.expect(t, abs(sum - 1) < 1e-6)
	for k in 0 ..< 6 {
		testing.expect(t, abs(weights[k] - weights[11 - k]) < 1e-6)
	}
}

@(test)
flat_stays_flat_and_buffers_are_checked :: proc(t: ^testing.T) {
	w, h :: 97, 61
	grey := make([]u8, w * h * 4)
	defer delete(grey)
	for &v in grey {
		v = 128
	}
	out := make([]u8, 40 * 25 * 4)
	defer delete(out)
	testing.expect(t, fit.resample(grey, w, h, 40, 25, out))
	for i := 0; i < len(out); i += 4 {
		testing.expect(t, out[i] == 128 && out[i + 1] == 128 && out[i + 2] == 128 && out[i + 3] == 255)
	}
	testing.expect(t, !fit.resample(grey, w, h, 0, 25, out))
	testing.expect(t, !fit.resample(grey, w, h, 41, 25, out))
	testing.expect(t, !fit.resample(grey[:10], w, h, 40, 25, out))
}
