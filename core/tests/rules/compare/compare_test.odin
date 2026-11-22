// The compare rules' own tests (core/src/rules/compare), some of
// windows/src/compare.rs's tests ported, and model/compare.rs's halving.
// The differential tests (core/diff/tests/compare.rs, model_compare.rs)
// hold the rest to the Rust. Run by `python3 core/build.py test`.
package compare_tests

import "core:testing"
import compare "../../../src/rules/compare"

some :: proc(s: string) -> compare.Text {
	return {s, true}
}

NONE :: compare.Text{}

@(test)
the_status_line_says_what_differs :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	a := [5]compare.Text{some("Z 6"), some("ISO 400"), some("1/250 s"), some("f/2.8"), some("35 mm")}
	b := a
	b[2] = some("1/500 s")
	b[3] = some("f/4")
	testing.expect_value(t, compare.differences(&a, &b, {"Portra", "Portra"}, {some("Endura"), some("Endura")}, ta), "shutter and aperture")
	testing.expect_value(t, compare.differences(&a, &a, {"Portra", "Gold"}, {some("Endura"), NONE}, ta), "film and paper")
	c := b
	c[1] = some("ISO 800")
	testing.expect_value(t, compare.differences(&a, &c, {"Portra", "Gold"}, {NONE, NONE}, ta), "ISO, shutter, aperture and film")
	testing.expect_value(t, compare.differences(&a, nil, {"P", "P"}, {NONE, NONE}, ta), "")
}

@(test)
compare_is_offered_for_two_never_three :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	three := compare.offer(3, false, true, true)
	testing.expect_value(t, three.kind, compare.Offer_Kind.Too_Many)
	testing.expect(t, !three.enabled)
	testing.expect_value(t, compare.offer_title(.Too_Many, "", "", ta), "Compare Two Photographs")
	two := compare.offer(2, false, true, true)
	testing.expect(t, two.kind == .Pair && two.enabled)
	testing.expect_value(t, compare.offer_title(.Pair, "3", "7", ta), "Compare 3 and 7")
	testing.expect(t, !compare.offer(2, false, false, true).enabled, "while cropping, say")
	testing.expect_value(t, compare.offer(1, true, true, true).kind, compare.Offer_Kind.With_Current)
	original := compare.offer(1, false, true, true)
	testing.expect(t, original.kind == .With_Original && original.enabled && original.with_original)
	testing.expect(t, !compare.offer(1, false, true, false).enabled, "in the grid, say")
	testing.expect_value(t, compare.word(2, true), compare.Word_Kind.Pair)
	testing.expect_value(t, compare.word_title(.Pair, "3", "7", ta), "Compare 3 and 7 (C)")
	testing.expect_value(t, compare.word(2, false), compare.Word_Kind.Select_Second)
	testing.expect_value(t, compare.word(3, true), compare.Word_Kind.Too_Many)
	testing.expect_value(t, compare.menu_title(true), "Stop C&omparing\tC")
}

@(test)
steps_skip_the_first_and_stop_at_the_ends :: proc(t: ^testing.T) {
	order := [4]compare.Item{{"p0", NONE}, {"p1", NONE}, {"p2", NONE}, {"p3", NONE}}
	testing.expect_value(t, compare.step(order[1], order[2], order[:], 1), 3)
	testing.expect_value(t, compare.step(order[3], order[0], order[:], 1), -1)
	testing.expect_value(t, compare.step(order[1], order[0], order[:], -1), -1)
	stray := compare.Item{"p9", NONE}
	testing.expect_value(t, compare.step(stray, order[1], order[:], 1), 2)
	testing.expect_value(t, compare.step(stray, order[1], order[:], -1), 0)
}

@(test)
frame_numbers_count_photographs_and_letter_duplicates :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	items := [4]compare.Item{{"a", NONE}, {"b", some("x")}, {"b", some("y")}, {"c", NONE}}
	testing.expect_value(t, compare.frame_number(items[:], items[0], ta), "1")
	testing.expect_value(t, compare.frame_number(items[:], items[1], ta), "2a")
	testing.expect_value(t, compare.frame_number(items[:], items[2], ta), "2b")
	testing.expect_value(t, compare.frame_number(items[:], items[3], ta), "3")
	testing.expect_value(t, compare.frame_number(items[:], {"z", NONE}, ta), "")
}

@(test)
two_landscapes_stack_in_a_tall_window :: proc(t: ^testing.T) {
	arrangement, frames := compare.layout({800, 1400}, {1.5, 1.5})
	testing.expect_value(t, arrangement, compare.Arrangement.Column)
	testing.expect(t, frames[0][1] < frames[1][1])
	arrangement, frames = compare.layout({1800, 900}, {0.667, 0.667})
	testing.expect_value(t, arrangement, compare.Arrangement.Row)
	testing.expect_value(t, frames[0][2], frames[1][2])
}

@(test)
original_modes_keep_their_keys :: proc(t: ^testing.T) {
	for mode in compare.Original_Mode {
		testing.expect_value(t, compare.original_mode_from_key(string(compare.original_mode_key(mode))), mode)
	}
	for reference in compare.Original_Reference {
		testing.expect_value(t, compare.original_reference_from_key(string(compare.original_reference_key(reference))), reference)
	}
	testing.expect_value(t, compare.original_mode_from_key("nonsense"), compare.Original_Mode.Hold)
}

@(test)
halving_averages_four_and_repeats_odd_edges :: proc(t: ^testing.T) {
	// 3 × 1: (0, 4) average to 2, the last column repeats (8, 8) to 8.
	src := [12]u8{0, 0, 0, 255, 4, 4, 4, 255, 8, 8, 8, 255}
	out: [4]u8
	compare.halved(src[:], 3, 1, out[:])
	testing.expect_value(t, out, [4]u8{2, 2, 2, 255})
	one := [4]u8{9, 9, 9, 9}
	single: [4]u8
	compare.halved(one[:], 1, 1, single[:])
	testing.expect_value(t, single, one)

	plan := compare.over_frame(300, 200, {6000, 4000})
	testing.expect(t, plan.same)
	testing.expect_value(t, plan.out, [2]u32{300, 200})
	plan = compare.over_frame(400, 400, {6000, 4000})
	testing.expect(t, !plan.same)
	testing.expect_value(t, plan.out, [2]u32{400, 267})
	testing.expect_value(t, plan.size, [2]u32{267, 267})
	testing.expect_value(t, plan.at, [2]i64{66, 0})
}
