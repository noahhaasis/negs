// The scopes' own tests (core/src/rules/histogram), with
// windows/src/histogram.rs's values. core/diff/tests/histogram.rs holds
// every bin to the Rust. Run by `python3 core/build.py test`.
package histogram_tests

import "core:testing"
import crop "../../../src/rules/crop"
import histogram "../../../src/rules/histogram"

// Black left, one red column in the middle, white right.
halves :: proc(width, height: int) -> []u8 {
	rgba := make([]u8, width * height * 4)
	for y in 0 ..< height {
		for x in 0 ..< width {
			p := (y * width + x) * 4
			c := [3]u8{200, 0, 0} if x == width / 2 else ([3]u8{0, 0, 0} if x < width / 2 else [3]u8{255, 255, 255})
			rgba[p], rgba[p + 1], rgba[p + 2], rgba[p + 3] = c[0], c[1], c[2], 255
		}
	}
	return rgba
}

@(test)
waveform_keeps_position_and_levels :: proc(t: ^testing.T) {
	image := halves(100, 60)
	defer delete(image)
	o := crop.orientation(0, false)
	s, ok := histogram.shape(100, 60, crop.DEFAULT_SETTINGS, o, 1 << 20).?
	testing.expect(t, ok)
	testing.expect_value(t, s.wave_width, 100)
	n := s.wave_width * 256
	red, green, blue := make([]u32, n), make([]u32, n), make([]u32, n)
	defer delete(red)
	defer delete(green)
	defer delete(blue)
	h: histogram.Histogram
	samples := histogram.count(image, 100, 60, crop.DEFAULT_SETTINGS, o, s, &h, red, green, blue)
	testing.expect_value(t, samples, 6000)
	testing.expect_value(t, red[0], 60)
	testing.expect_value(t, red[200 * 100 + 50], 60)
	testing.expect_value(t, blue[255 * 100 + 99], 60)
	testing.expect_value(t, h.red[0], 50 * 60)
	testing.expect_value(t, h.red[200], 60)
	testing.expect_value(t, h.red[255], 49 * 60)
	raster := make([][4]u8, n)
	defer delete(raster)
	histogram.raster(100, red, green, blue, raster)
	testing.expect(t, raster[255 * 100][0] > 0 && raster[99][0] > 0)
}

@(test)
peak_ignores_clipping :: proc(t: ^testing.T) {
	h: histogram.Histogram
	h.red[0] = 1000
	h.green[255] = 900
	h.blue[17] = 30
	testing.expect_value(t, histogram.peak(&h), 30)
	clipped: histogram.Histogram
	clipped.red[255] = 5
	testing.expect_value(t, histogram.peak(&clipped), 5)
}

@(test)
nothing_to_count :: proc(t: ^testing.T) {
	o := crop.orientation(0, false)
	_, none := histogram.shape(100, 60, crop.DEFAULT_SETTINGS, o, 0).?
	testing.expect(t, !none)
	_, empty := histogram.shape(0, 60, crop.DEFAULT_SETTINGS, o, 100).?
	testing.expect(t, !empty)
	// Any budget, up to all ones, as the Rust's usize.
	s, any := histogram.shape(100, 60, crop.DEFAULT_SETTINGS, o, max(uint)).?
	testing.expect(t, any && s.columns == 100 && s.rows == 60)
}
