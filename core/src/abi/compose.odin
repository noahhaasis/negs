// The thumbnail composer's and the scopes' C ABI (negs_core.h, "Phase 3b:
// Picture rules", Thumbnail compose and Histogram): pixels and bins through
// the caller's buffers.
package negs_core

import "../rules/compose"
import "../rules/crop"
import "../rules/histogram"

@(export, link_name = "negs_compose_reduced_size")
compose_reduced_size :: proc "c" (width, height: u32, crop_w, crop_h: f64, long_side: u32, out: ^[2]u32) -> u8 {
	size, ok := compose.reduced_size(width, height, crop_w, crop_h, long_side).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = size
	}
	return 1
}

@(export, link_name = "negs_compose_to_srgb")
compose_to_srgb :: proc "c" (rgba: [^]u8, len: uint, m: ^[3][3]f32) {
	if rgba == nil || m == nil {
		return
	}
	compose.to_srgb(rgba[:len], m^)
}

@(export, link_name = "negs_histogram_peak")
scopes_histogram_peak :: proc "c" (h: ^histogram.Histogram) -> u32 {
	if h == nil {
		return 0
	}
	return histogram.peak(h)
}

// The waveform's width for these scopes; 0 when there's nothing to count.
@(export, link_name = "negs_scopes_wave_width")
scopes_wave_width :: proc "c" (width, height: uint, s: ^crop.Settings, o: crop.Orientation, max_samples: uint) -> uint {
	shape, ok := histogram.shape(int(width), int(height), s^, o, int(max_samples)).?
	return uint(shape.wave_width) if ok else 0
}

// Counts into *h and the three waveform bins (wave_len = wave width × 256
// each); returns the samples, 0 (and nothing counted) when the preview is
// too short or wave_len doesn't match.
@(export, link_name = "negs_scopes_count")
scopes_count :: proc "c" (rgba: [^]u8, rgba_len: uint, width, height: uint, s: ^crop.Settings, o: crop.Orientation, max_samples: uint, h: ^histogram.Histogram, red, green, blue: [^]u32, wave_len: uint) -> uint {
	if h == nil || s == nil || red == nil || green == nil || blue == nil || rgba == nil {
		return 0
	}
	h^ = {}
	shape, ok := histogram.shape(int(width), int(height), s^, o, int(max_samples)).?
	if !ok || rgba_len < width * height * 4 || wave_len != uint(shape.wave_width) * 256 {
		return 0
	}
	return uint(histogram.count(rgba[:rgba_len], int(width), int(height), s^, o, shape, h, red[:wave_len], green[:wave_len], blue[:wave_len]))
}

@(export, link_name = "negs_waveform_raster")
scopes_waveform_raster :: proc "c" (width: uint, red, green, blue: [^]u32, len: uint, out: [^][4]u8, out_len: uint) -> u8 {
	if width == 0 || red == nil || green == nil || blue == nil || out == nil || len < width * 256 || out_len < width * 256 {
		return 0
	}
	histogram.raster(int(width), red[:len], green[:len], blue[:len], out[:width * 256])
	return 1
}
