// The core's unit tests: `python3 core/build.py test`.
//
// `odin test` runs each @(test) procedure under its own mem.Tracking_Allocator;
// build.py passes -define:ODIN_TEST_FAIL_ON_BAD_MEMORY=true, so a test that
// leaks or frees badly fails. tests/leakcheck holds a test that leaks on
// purpose: build.py runs it and fails unless it fails.
package tests

import "core:testing"
import core "../src/abi"

@(test)
abi_version_matches_the_header :: proc(t: ^testing.T) {
	testing.expect_value(t, core.abi_version(), 4)
	testing.expect_value(t, string(core.version()), "0.1.0 (odin dev-2026-11a)")
}

@(test)
buffers_round_trip_without_leaks :: proc(t: ^testing.T) {
	data := []u8{1, 2, 3, 250}
	b := core.buf_from(data)
	testing.expect_value(t, int(b.len), 4)
	testing.expect_value(t, b.data[3], 250)
	core.buf_free_in_context(b)
	empty := core.buf_from(nil)
	testing.expect(t, empty.data == nil && empty.len == 0)
}

@(test)
layout_knows_its_structs :: proc(t: ^testing.T) {
	name := "negs_buf_t.len"
	testing.expect_value(t, core.layout(raw_data(name), uint(len(name))), 8)
	missing := "negs_nothing_t"
	testing.expect_value(t, core.layout(raw_data(missing), uint(len(missing))), -1)
}
