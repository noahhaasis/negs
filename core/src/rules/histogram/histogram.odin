// The developed photograph's tones (windows/src/histogram.rs,
// Histogram.swift): 256 bins per channel of the cropped photograph, and the
// RGB waveform, signal level against horizontal position in the shown crop,
// both from one bounded grid of samples.
package histogram

import "core:c/libc"
import "../crop"

// Laid out as negs_histogram_t.
Histogram :: struct {
	red, green, blue: [256]u32,
}

// The tallest bin, leaving out pure black and white so clipping doesn't
// flatten the rest; their spikes run off the top instead.
peak :: proc "contextless" (h: ^Histogram) -> u32 {
	channels := [3]^[256]u32{&h.red, &h.green, &h.blue}
	inner: u32 = 0
	for c in channels {
		for v in c^[1:255] {
			inner = max(inner, v)
		}
	}
	if inner > 0 {
		return inner
	}
	all: u32 = 0
	for c in channels {
		for v in c^ {
			all = max(all, v)
		}
	}
	return all
}

// The sampling grid over the crop's pixels, and the waveform's width.
Shape :: struct {
	area:          crop.Rect,
	columns, rows: int,
	shown_columns: int,
	wave_width:    int,
	turn:          i32,
}

// The grid for a `width` × `height` preview: an even grid of at most
// `max_samples` pixels over the crop. None when there's nothing to count.
shape :: proc "contextless" (width, height: int, settings: crop.Settings, o: crop.Orientation, max_samples: uint) -> Maybe(Shape) {
	area := crop.pixels(settings, crop.Size{f64(width), f64(height)})
	if area.width < 1 || area.height < 1 || max_samples == 0 {
		return nil
	}
	// In unsigned arithmetic, as the Rust's usize: any budget, up to all ones.
	aw, ah := crop.sat_uint(area.width), crop.sat_uint(area.height)
	root := crop.sat_uint(crop.sqrt(f64(max_samples) * f64(aw) / f64(ah)))
	columns := min(min(aw, max_samples), max(root, 1))
	rows := min(ah, max(max_samples / columns, 1))
	turn := crop.rem4(o.turns)
	shown_columns := columns if turn % 2 == 0 else rows
	return Shape{area, int(columns), int(rows), int(shown_columns), int(min(shown_columns, 256)), turn}
}

// Counts the samples of an RGBA preview into `h` (cleared first) and the
// waveform's bins (`wave_width` × 256 each, row-major, level 0 in row 0,
// cleared first). The waveform runs across the photograph as shown:
// mirrored if `o.flip`, then after its quarter turns. Returns the samples.
count :: proc "contextless" (rgba: []u8, width, height: int, settings: crop.Settings, o: crop.Orientation, s: Shape, h: ^Histogram, red, green, blue: []u32) -> int {
	h^ = {}
	for i in 0 ..< len(red) {
		red[i] = 0
		green[i] = 0
		blue[i] = 0
	}
	area := s.area
	ww := s.wave_width
	samples := 0
	// A straightened crop is in the turned frame: turn each sample back to
	// the photograph.
	pi: f64 = crop.PI
	sin, cos := crop.sin_cos(-settings.angle * pi / 180.0)
	cx, cy := f64(width) / 2.0, f64(height) / 2.0
	for row in 0 ..< s.rows {
		for column in 0 ..< s.columns {
			dx := area.x + (f64(column) + 0.5) * area.width / f64(s.columns) - cx
			dy := area.y + (f64(row) + 0.5) * area.height / f64(s.rows) - cy
			x, y := libc.floor(cx + dx * cos - dy * sin), libc.floor(cy + dx * sin + dy * cos)
			if x < 0 || y < 0 || crop.sat_uint(x) >= uint(width) || crop.sat_uint(y) >= uint(height) {
				continue
			}
			p := (int(crop.sat_uint(y)) * width + int(crop.sat_uint(x))) * 4
			r, g, b := rgba[p], rgba[p + 1], rgba[p + 2]
			h.red[r] += 1
			h.green[g] += 1
			h.blue[b] += 1
			col := s.columns - 1 - column if o.flip else column
			shown: int
			switch s.turn {
			case 1:
				shown = s.rows - 1 - row
			case 2:
				shown = s.columns - 1 - col
			case 3:
				shown = row
			case:
				shown = col
			}
			wx := shown * ww / s.shown_columns
			red[int(r) * ww + wx] += 1
			green[int(g) * ww + wx] += 1
			blue[int(b) * ww + wx] += 1
			samples += 1
		}
	}
	return samples
}

// A small raster, `width` × 256 with level 255 in the top row, as
// premultiplied red, green, blue and alpha: a log scale of the counts so
// faint levels show. `out` holds width × 256 pixels.
raster :: proc "contextless" (width: int, red, green, blue: []u32, out: [][4]u8) {
	for i in 0 ..< len(out) {
		out[i] = {}
	}
	peak: u32 = 0
	for v in red {
		peak = max(peak, v)
	}
	for v in green {
		peak = max(peak, v)
	}
	for v in blue {
		peak = max(peak, v)
	}
	if peak == 0 {
		return
	}
	normalizer := libc.log1p(f64(peak))
	intensity :: proc "contextless" (count: u32, normalizer: f64) -> u8 {
		if count == 0 {
			return 0
		}
		return crop.sat_u8(crop.round(255.0 * (0.18 + 0.82 * libc.log1p(f64(count)) / normalizer)))
	}
	for level in 0 ..< 256 {
		for x in 0 ..< width {
			bin := level * width + x
			r, g, b := intensity(red[bin], normalizer), intensity(green[bin], normalizer), intensity(blue[bin], normalizer)
			out[(255 - level) * width + x] = {r, g, b, max(r, g, b)}
		}
	}
}
