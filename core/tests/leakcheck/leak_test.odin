// A test that leaks on purpose. build.py runs it and fails unless the
// tracking allocator catches it, so a broken leak check can't pass silently.
package leakcheck

import "core:testing"

@(test)
leaks_one_allocation :: proc(t: ^testing.T) {
	leaked := new(int)
	leaked^ = 42
	testing.expect_value(t, leaked^, 42)
}
