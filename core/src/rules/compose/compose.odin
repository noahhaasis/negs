// An edited thumbnail from the open photograph's settled preview
// (windows/src/thumb_compose.rs, ThumbnailComposer): the app's rules around
// the engine's crop and border, which stay with the host. How far the
// preview is reduced before cropping, and a Rec. 2020 preview brought to
// sRGB's primaries.
package compose

import "core:c/libc"
import "../crop"

// The size a `width` × `height` preview is reduced to before a crop whose
// rectangle is `crop_w` × `crop_h` of it is cut for a `long_side` thumbnail:
// about twice what the crop needs. None when it is small enough already.
reduced_size :: proc "contextless" (width, height: u32, crop_w, crop_h: f64, long_side: u32) -> Maybe([2]u32) {
	share := crop.clamp_f64(crop.fmax(crop_w, crop_h), 0.05, 1.0)
	wanted := crop.sat_u32(crop.ceil(2.0 * f64(long_side) / share))
	longest := max(width, height)
	if longest <= wanted {
		return nil
	}
	scale := f64(wanted) / f64(longest)
	return [2]u32{max(crop.sat_u32(crop.round(f64(width) * scale)), 1), max(crop.sat_u32(crop.round(f64(height) * scale)), 1)}
}

@(private = "file")
srgb_decode :: proc "contextless" (v: f32) -> f32 {
	if v <= 0.04045 {
		return v / 12.92
	}
	gamma: f32 = 2.4
	return libc.powf((v + 0.055) / 1.055, gamma)
}

@(private = "file")
srgb_encode :: proc "contextless" (v: f32) -> f32 {
	if v <= 0.0031308 {
		return v * 12.92
	}
	one, gamma: f32 = 1.0, 2.4
	return 1.055 * libc.powf(v, one / gamma) - 0.055
}

// RGBA pixels with Rec. 2020 primaries to sRGB's, both on sRGB's curve,
// clipped: `m` is the row-major matrix (the GPU's REC2020_TO_SRGB).
to_srgb :: proc "contextless" (rgba: []u8, m: [3][3]f32) {
	decode: [256]f32
	for i in 0 ..< 256 {
		decode[i] = srgb_decode(f32(i) / 255.0)
	}
	for p := 0; p + 4 <= len(rgba); p += 4 {
		c := [3]f32{decode[rgba[p]], decode[rgba[p + 1]], decode[rgba[p + 2]]}
		for i in 0 ..< 3 {
			linear := m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]
			if linear < 0 {
				linear = 0
			}
			if linear > 1 {
				linear = 1
			}
			rgba[p + i] = crop.sat_u8(f64(libc.roundf(srgb_encode(linear) * 255.0)))
		}
	}
}
