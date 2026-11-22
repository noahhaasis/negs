// Reads a heap block after freeing it, on purpose. `build.py test --asan`
// runs it and fails unless AddressSanitizer stops it, so an ASan run that
// checks nothing can't pass silently.
//
// What ASan adds here: Odin's heap is libc's malloc, so use after free and
// stray raw pointers are caught. Small overruns of a slice's end often are
// not (the heap allocator pads blocks for alignment); Odin's bounds checks
// cover those. The block is the heap allocator's own: the test runner's
// allocator holds on to freed blocks.
package asancheck

import "base:runtime"
import "core:testing"

seen: u8

@(test)
reads_after_free :: proc(t: ^testing.T) {
	block := make([]u8, 64, runtime.heap_allocator())
	p := ([^]u8)(raw_data(block))
	delete(block, runtime.heap_allocator())
	(^u8)(&seen)^ = p[8]
	testing.expect(t, seen == 0 || seen != 0)
}
