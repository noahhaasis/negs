// The sheet rules' own tests (core/src/rules/sheet), some of
// windows/src/sheet_rules.rs's tests ported. The differential tests
// (core/diff/tests/sheet_rules.rs) hold the rest to the Rust. Run by
// `python3 core/build.py test`.
package sheet_tests

import "core:testing"
import lib "../../../src/library"
import sheet "../../../src/rules/sheet"

@(test)
strip_frames_take_their_proportions :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.PICTURE, 140.0)
	testing.expect_value(t, sheet.BAND, 186.0)
	testing.expect_value(t, sheet.frame_width(1.5, true), 210.0)
	testing.expect_value(t, sheet.frame_width(2.0 / 3.0, true), 93.0)
	testing.expect_value(t, sheet.frame_width(0, false), 210.0)
}

@(test)
a_thumbnail_keeps_the_known_shape_unless_it_really_differs :: proc(t: ^testing.T) {
	header := 6016.0 / 4016.0
	testing.expect_value(t, sheet.kept_ratio(header, true, 512.0 / 342.0), header)
	testing.expect_value(t, sheet.kept_ratio(header, true, 512.0 / 340.0), header)
	testing.expect_value(t, sheet.kept_ratio(header, true, 1.0), 1.0)
	testing.expect_value(t, sheet.kept_ratio(0, false, 512.0 / 342.0), 512.0 / 342.0)

	widths := [3]f64{210, 93, 210}
	xs: [3]f64
	testing.expect_value(t, sheet.strip_layout(widths[:], xs[:]), 523.0)
	testing.expect_value(t, xs, [3]f64{0, 215, 313})
	testing.expect_value(t, sheet.strip_middle(xs[1], widths[1]), 261.5)
	start, end := sheet.strip_indices(xs[:], widths[:], 0, 100)
	testing.expect_value(t, [2]int{start, end}, [2]int{0, 1})
	start, end = sheet.strip_indices(xs[:], widths[:], 212, 214)
	testing.expect_value(t, [2]int{start, end}, [2]int{1, 1})
	start, end = sheet.strip_indices(xs[:], widths[:], 200, 320)
	testing.expect_value(t, [2]int{start, end}, [2]int{0, 3})
	testing.expect_value(t, sheet.strip_hit(xs[:], widths[:], 212), -1)
	testing.expect_value(t, sheet.strip_hit(xs[:], widths[:], 216), 1)
	testing.expect_value(t, sheet.strip_hit(xs[:], widths[:], 600), -1)
	testing.expect_value(t, sheet.strip_index_at(xs[:], widths[:], 600), 2)
	testing.expect_value(t, sheet.strip_index_at(nil, nil, 3), -1)
}

@(test)
centring_stops_at_the_rows_ends :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.centred(100, 800, 2000), 0.0)
	testing.expect_value(t, sheet.centred(1000, 800, 2000), 600.0)
	testing.expect_value(t, sheet.centred(1900, 800, 2000), 1200.0)
	testing.expect_value(t, sheet.centred(300, 800, 500), 0.0)
}

@(test)
dimming_follows_the_mac :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.strip_opacity(true, false, false, false, false), 1.0)
	testing.expect_value(t, sheet.strip_opacity(false, false, false, false, false), sheet.STRIP_DIM)
	testing.expect_value(t, sheet.strip_opacity(false, false, false, true, true), 0.9)
	testing.expect_value(t, sheet.strip_opacity(false, false, true, true, false), 0.22)
	testing.expect_value(t, sheet.strip_opacity(true, false, true, false, false), 0.5)
	testing.expect_value(t, sheet.reject_opacity(false, true), 0.4)
}

@(test)
sheet_rows_match_the_swift :: proc(t: ^testing.T) {
	ratios := [8]f64{1.5, 0.667, 1.5, 1.5, 1.5, 1.5, 0.667, 1.5}
	frames: [8]sheet.Rect
	rows: [8]sheet.Row
	count, height := sheet.layout(ratios[:], 1388, 176, frames[:], rows[:])
	testing.expect_value(t, count, 2)
	testing.expect_value(t, [2]uint{rows[0].start, rows[0].end}, [2]uint{0, 6})
	testing.expect_value(t, rows[0].height, 162.0) // ((1356 - 30) / 8.167).round()
	testing.expect_value(t, rows[1].height, 176.0)
	testing.expect_value(t, sheet.rect_max_x(frames[5]), 1388.0 - sheet.GRID_SIDE)
	testing.expect_value(t, height, rows[1].y + rows[1].height + sheet.GRID_BOTTOM)

	testing.expect_value(t, sheet.row_of(rows[:count], 8, 6), 1)
	testing.expect_value(t, sheet.row_of(rows[:count], 8, 99), -1)
	testing.expect_value(t, sheet.vertical(frames[:], rows[:count], 0, -1), -1)
	testing.expect_value(t, sheet.hit(frames[:], rows[:count], frames[0].x + 1, frames[0].y + 1), 0)
	testing.expect_value(t, sheet.nearest(frames[:], rows[:count], -50, -50), 0)
	start, end := sheet.indices(rows[:count], 1e6, 2e6)
	testing.expect_value(t, [2]uint{start, end}, [2]uint{0, 0})

	_, empty := sheet.layout(nil, 1000, 176, nil, nil)
	testing.expect_value(t, empty, sheet.GRID_TOP + sheet.GRID_BOTTOM)
}

@(test)
sizes_clamp_and_slide :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.clamp_size(40), 100.0)
	testing.expect_value(t, sheet.clamp_size(999), 360.0)
	testing.expect_value(t, sheet.size_at(sheet.size_fraction(176)), 176.0)
	testing.expect_value(t, sheet.thumb_for_rows(176, 1), sheet.Thumb_Size.Medium)
	testing.expect_value(t, sheet.thumb_for_rows(176, 1.5), sheet.Thumb_Size.Large)
}

@(test)
reading_goes_outward_from_the_view :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.thumbnail_centre(nil, 7, true), 7)
	on := [4]uint{3, 4, 5, 6}
	testing.expect_value(t, sheet.thumbnail_centre(on[:], 0, false), 5)
	indices := [5]uint{1, 3, 5, 2, 4}
	order: [5]uint
	sheet.outward(indices[:], 3, order[:])
	// 'b', 'd', 'e', 'a', 'c': positions 1, 3, 4, 0, 2.
	testing.expect_value(t, order, [5]uint{1, 3, 4, 0, 2})
	out: [2 * sheet.PREFETCH_REACH + 1]uint
	n := sheet.prefetch_order(5, nil, 0, true, out[:])
	testing.expect_value(t, n, 5)
	testing.expect_value(t, [5]uint{out[0], out[1], out[2], out[3], out[4]}, [5]uint{0, 1, 2, 3, 4})
	testing.expect_value(t, sheet.prefetch_order(0, nil, 0, false, out[:]), 0)
}

@(test)
pools_follow_the_machine :: proc(t: ^testing.T) {
	testing.expect_value(t, sheet.helpers(12, 16 << 30, false), 2)
	testing.expect_value(t, sheet.helpers(12, 16 << 30, true), 1)
	testing.expect_value(t, sheet.helpers(16, 8 << 30, false), 1)
	testing.expect_value(t, sheet.readers(12), 4)
	testing.expect_value(t, sheet.readers(1), 1)
}

@(test)
the_scroll_bar_knob_follows_the_scroll :: proc(t: ^testing.T) {
	_, _, shown := sheet.bar_knob(0, 500, 600)
	testing.expect(t, !shown, "nothing to scroll")
	top, length, _ := sheet.bar_knob(0, 2400, 600)
	testing.expect_value(t, top, sheet.BAR_INSET)
	testing.expect_value(t, length, (600 - 2 * sheet.BAR_INSET) / 4)
	_, tiny, _ := sheet.bar_knob(0, 600_000, 600)
	testing.expect_value(t, tiny, sheet.BAR_MIN)
	testing.expect_value(t, sheet.bar_scroll(5000, 2400, 600), 1800.0)
	testing.expect_value(t, sheet.bar_opacity(0), 1.0)
	testing.expect_value(t, sheet.bar_opacity(1e9), 0.0)
}

@(test)
the_sentence_says_what_is_shown :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	all := sheet.sentence({}, 12, 12, context.temp_allocator)
	testing.expect_value(t, len(all), 3)
	testing.expect_value(t, all[0].text, "Showing ")
	testing.expect_value(t, all[1].kind, sheet.Part_Kind.Filter)
	testing.expect_value(t, all[1].text, "all")
	testing.expect(t, !all[1].on)
	testing.expect_value(t, all[2].text, " 12 photographs")

	filter := lib.Filter{rating = lib.Rating_Filter{kind = .At_Least, stars = 3}, flags = {.Pick}, edits = .Edited}
	parts := sheet.sentence(filter, 4, 12, context.temp_allocator)
	testing.expect_value(t, parts[0].text, "Showing 4 of 12 · ")
	testing.expect_value(t, parts[len(parts) - 1].kind, sheet.Part_Kind.Clear)
	testing.expect(t, len(parts) <= sheet.MAX_PARTS)
	testing.expect_value(t, sheet.foot(4, 12, 0, true, context.temp_allocator), "4 of 12 photographs")
	testing.expect_value(t, sheet.foot(12, 12, 1, false, context.temp_allocator), "12 photographs · 1 selected")
}

@(test)
the_fade_settles :: proc(t: ^testing.T) {
	opacity, rise := sheet.sheet_look(0)
	testing.expect_value(t, opacity, 0.0)
	testing.expect_value(t, rise, sheet.FADE_RISE)
	opacity, rise = sheet.sheet_look(sheet.FADE_MS)
	testing.expect_value(t, opacity, 1.0)
	testing.expect_value(t, rise, 0.0)
	testing.expect_value(t, sheet.cubic_bezier(0.22, 0.8, 0.24, 1, 1), 1.0)
}
