// core/src/rules/help: lookups, grain lanes and the backlight.
package help_tests

import "core:testing"
import "../../../src/rules/help"

SCAN :: "film_render.convert.scan_illuminant"
FIELDS := []string{"rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"}

@(private)
text :: proc(path: string) -> (string, bool) {
	return help.text(path, SCAN, FIELDS)
}

@(test)
looks_up_paths_lanes_and_groups :: proc(t: ^testing.T) {
	s, ok := text("enlarger.print_exposure")
	testing.expect(t, ok && len(s) > 0)
	s, ok = text(SCAN)
	testing.expect(t, ok && s == help.SCAN_ILLUMINANT)
	lane, lane_ok := text("film_render.grain.uniformity.2")
	own, own_ok := text("film_render.grain.uniformity")
	testing.expect(t, lane_ok && own_ok && lane == own)
	_, ok = text("film_render.grain.uniformity.-1")
	testing.expect(t, !ok)
	_, ok = text("film_render.grain.uniformity.+1")
	testing.expect(t, ok)
	_, ok = text("film_render.grain.uniformity.18446744073709551616")
	testing.expect(t, !ok)
	_, ok = text("film_render.grain.uniformity.18446744073709551615")
	testing.expect(t, ok)
	// density_min is a lane field with no tooltip of its own.
	_, ok = text("film_render.grain.density_min.0")
	testing.expect(t, !ok)
	_, ok = text("no.such.control")
	testing.expect(t, !ok)
	g, group_ok := help.group("Grain")
	testing.expect(t, group_ok && len(g) > 0)
	_, group_ok = help.group("grain")
	testing.expect(t, !group_ok)
}
