// Your negative's own rules (core/src/rules/negative), with
// windows/src/negative.rs's words. core/diff/tests/negative.rs holds them
// to the Rust. Run by `python3 core/build.py test`.
package negative_tests

import "core:testing"
import crop "../../../src/rules/crop"
import negative "../../../src/rules/negative"

@(test)
scan_stops_from_what_both_recorded :: proc(t: ^testing.T) {
	// Twice the shutter and a stop wider: two stops more light.
	stops, ok := negative.scan_stops({1.0 / 60.0, 4.0, nil}, {1.0 / 125.0, 5.6, 100}).?
	testing.expect(t, ok)
	testing.expect(t, abs(stops - (1.0588936890535685 + 0.9708536543404751)) < 1e-12)
	_, none := negative.scan_stops({nil, nil, 100}, {1, 2, nil}).?
	testing.expect(t, !none)
	testing.expect_value(t, negative.exposure_ev(1.5, 0.5), 1.0)
	testing.expect_value(t, negative.exposure_ev(1.5, nil), 1.5)
}

@(test)
words :: proc(t: ^testing.T) {
	one := negative.skipped(1)
	defer delete(one)
	testing.expect_value(t, one, "1 photograph skipped: it doesn't look like a negative.")
	many := negative.skipped(3)
	defer delete(many)
	testing.expect_value(t, many, "3 photographs skipped: they don't look like negatives.")
	testing.expect_value(t, negative.flat_field_value(""), "Off")
	testing.expect_value(t, negative.flat_field_value(`C:\Scans\Roll 12\flat.tif`), "flat.tif")
	testing.expect_value(t, negative.flat_field_value("/scans/light.dng"), "light.dng")
	hint, shown := negative.flat_field_hint("/l.dng", false, nil).?
	testing.expect(t, shown)
	testing.expect_value(t, hint, "The light frame isn't used on this frame.")
	_, used := negative.flat_field_hint("/l.dng", true, nil).?
	testing.expect(t, !used)
}

@(test)
routes_stocks_and_found_paths :: proc(t: ^testing.T) {
	testing.expect(t, negative.is_route(negative.ROUTE))
	testing.expect(t, !negative.is_route(negative.SCENE_ROUTE))
	testing.expect(t, negative.readable(false, false, false))
	testing.expect(t, !negative.readable(false, false, true))
	testing.expect(t, negative.is_found("film_render.base"))
	testing.expect(t, negative.is_found("film_render.base.r"))
	testing.expect(t, negative.is_found("enlarger"))
	testing.expect(t, !negative.is_found("film_render.based"))
	testing.expect(t, !negative.is_found("film_render.convert.scan_illuminant"))
}

@(test)
the_picture_and_its_edges :: proc(t: ^testing.T) {
	s, ok := negative.picture_crop(0.1, 0.2, 0.9, 0.8, 50).?
	testing.expect(t, ok)
	testing.expect_value(t, s.aspect, crop.Aspect.Freeform)
	testing.expect_value(t, s.angle, crop.MAX_ANGLE)
	_, bad := negative.picture_crop(0.5, 0.2, 0.4, 0.8, 0).?
	testing.expect(t, !bad)
	edge, known := negative.shown_edge(crop.orientation(1, false), "left").?
	testing.expect(t, known)
	testing.expect_value(t, edge, cstring("top"))
	_, unknown := negative.shown_edge(crop.orientation(1, false), "middle").?
	testing.expect(t, !unknown)
	sentence := negative.sentence({found = true, edge = "left"}, crop.orientation(1, false))
	defer delete(sentence)
	testing.expect_value(t, sentence, "Film base found along the top edge.")
	testing.expect(t, negative.guessed({}))
	testing.expect(t, negative.same_number(1, 1 + 0.5e-9))
	testing.expect(t, !negative.same_number(1, 1 + 3e-9))
}
