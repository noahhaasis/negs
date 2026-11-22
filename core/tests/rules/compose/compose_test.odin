// The thumbnail's own rules (core/src/rules/compose), with
// windows/src/thumb_compose.rs's values. core/diff/tests/thumb_compose.rs
// holds the thumbnails to the Rust byte for byte. Run by
// `python3 core/build.py test`.
package compose_tests

import "core:testing"
import compose "../../../src/rules/compose"

REC2020_TO_SRGB :: [3][3]f32{{1.6603176, -0.5875727, -0.0729166}, {-0.1244067, 1.1328007, -0.0083489}, {-0.0181114, -0.1005965, 1.1187665}}

@(test)
previews_are_reduced_to_twice_the_crop :: proc(t: ^testing.T) {
	// A whole 6000 × 4000 preview for a 512 thumbnail: 1024 on the long side.
	testing.expect_value(t, compose.reduced_size(6000, 4000, 1, 1, 512), [2]u32{1024, 683})
	// Half the frame each way: twice as much.
	testing.expect_value(t, compose.reduced_size(6000, 4000, 0.5, 0.5, 512), [2]u32{2048, 1365})
	// Small enough already.
	_, reduced := compose.reduced_size(400, 200, 0.5, 1, 512).?
	testing.expect(t, !reduced)
}

@(test)
white_stays_white_and_alpha_is_left :: proc(t: ^testing.T) {
	pixels := [8]u8{255, 255, 255, 7, 0, 255, 0, 9}
	compose.to_srgb(pixels[:], REC2020_TO_SRGB)
	testing.expect_value(t, pixels[0], 255)
	testing.expect_value(t, pixels[1], 255)
	testing.expect_value(t, pixels[2], 255)
	testing.expect_value(t, pixels[3], 7)
	// A saturated Rec. 2020 green clips in sRGB: no red or blue to give.
	testing.expect_value(t, pixels[4], 0)
	testing.expect_value(t, pixels[5], 255)
	testing.expect_value(t, pixels[7], 9)
}
