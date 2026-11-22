// The crop rules' own tests (core/src/rules/crop), with windows/src/crop.rs's
// values. core/diff/tests/crop.rs holds every function to the Rust bit for
// bit; these say what they mean. Run by `python3 core/build.py test`.
package crop_tests

import "core:testing"
import crop "../../../src/rules/crop"

near :: proc(a, b: crop.Rect, accuracy := 1e-9) -> bool {
	return abs(a.x - b.x) <= accuracy && abs(a.y - b.y) <= accuracy && abs(a.width - b.width) <= accuracy && abs(a.height - b.height) <= accuracy
}

@(test)
pixels_round_like_the_exporter :: proc(t: ^testing.T) {
	s := crop.DEFAULT_SETTINGS
	s.rect = {0.1004, 0, 0.5, 1}
	testing.expect(t, crop.rect_eq(crop.pixels(s, {1000, 800}), {100, 0, 500, 800}))
	s.rect = {0.999, 0, 0.0001, 1}
	testing.expect(t, crop.rect_eq(crop.pixels(s, {10, 10}), {9, 0, 1, 10}))
	testing.expect_value(t, crop.size_of_crop(crop.DEFAULT_SETTINGS, {96, 64}), crop.Size{96, 64})
	s = crop.DEFAULT_SETTINGS
	s.rect = {0.2, 0.1, 0.5, 0.25}
	s.angle = -3
	testing.expect_value(t, crop.size_of_crop(s, {1000, 800}), crop.Size{500, 200})
}

@(test)
rectangle_turns_with_the_photograph :: proc(t: ^testing.T) {
	top_left := crop.Rect{0, 0, 0.5, 0.25}
	testing.expect(t, near(crop.shown(top_left, 1, false), {0.75, 0, 0.25, 0.5}))
	testing.expect(t, near(crop.shown(top_left, 2, false), {0.5, 0.75, 0.5, 0.25}))
	testing.expect(t, near(crop.shown(top_left, 0, true), {0.5, 0, 0.5, 0.25}))
	testing.expect(t, near(crop.shown(top_left, 1, true), {0.75, 0.5, 0.25, 0.5}))
	any := crop.Rect{0.1, 0.2, 0.3, 0.4}
	for rotation in i32(0) ..< 4 {
		for flip in ([2]bool{false, true}) {
			testing.expect(t, near(crop.unrotated(crop.shown(any, rotation, flip), rotation, flip), any))
		}
	}
}

@(test)
resize_keeps_the_ratio_and_stays_inside :: proc(t: ^testing.T) {
	bounds := crop.of_size({600, 400})
	start := crop.Rect{100, 100, 300, 200}
	testing.expect(t, near(crop.resize(start, {1, 1}, {900, 900}, 1.5, bounds, 10), {100, 100, 450, 300}))
	testing.expect(t, near(crop.resize(start, {1, 0}, {460, 0}, 1.5, bounds, 10), {100, 80, 360, 240}))
	testing.expect(t, near(crop.resize(start, {-1, -1}, {395, -50}, nil, bounds, 20), {380, 0, 20, 300}))
	testing.expect(t, near(crop.move_rect(start, 1000, -1000, bounds), {300, 0, 300, 200}))
	testing.expect(t, near(crop.fit(1, {580, 200}, bounds), {200, 0, 400, 400}))
}

@(test)
a_flip_mirrors_the_straightening :: proc(t: ^testing.T) {
	start := crop.DEFAULT_SETTINGS
	start.angle = 3
	start.rect = {0.1, 0.1, 0.5, 0.5}
	s := crop.session_new(start, {600, 400}, 0, true)
	testing.expect_value(t, s.angle, -3)
	testing.expect_value(t, crop.session_settings(s).angle, 3)
	testing.expect(t, near(crop.session_shown(s), {240, 40, 300, 200}))
	crop.session_update(&s, {600, 400}, 0, false)
	testing.expect_value(t, s.angle, 3)
	testing.expect_value(t, crop.session_settings(s).angle, 3)
}

@(test)
straightening_stays_on_the_photograph :: proc(t: ^testing.T) {
	s := crop.session_new(crop.DEFAULT_SETTINGS, {600, 400}, 0, false)
	crop.session_begin_straightening(&s)
	crop.session_straighten(&s, 10.04)
	testing.expect_value(t, s.angle, 10)
	testing.expect(t, crop.fits(crop.session_shown(s), s.angle, s.photo))
	crop.session_straighten(&s, 0)
	testing.expect(t, near(crop.session_shown(s), {0, 0, 600, 400}, 1e-6))
	crop.session_end_straightening(&s)
	testing.expect(t, !s.has_intended && !s.straightening)
	crop.session_straighten(&s, 99)
	testing.expect_value(t, s.angle, crop.MAX_ANGLE)
}

@(test)
aspects :: proc(t: ^testing.T) {
	testing.expect_value(t, crop.aspect_format(.Five_Four), cstring("4×5"))
	testing.expect(t, crop.aspect_format(.Freeform) == nil)
	ratio, ok := crop.aspect_ratio(.Three_Two, {400, 600}).?
	testing.expect(t, ok && ratio == 1.5)
	_, free := crop.aspect_ratio(.Freeform, {400, 600}).?
	testing.expect(t, !free)
}
