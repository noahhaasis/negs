// What Phase 3a's exports share (negs_core.h, "Phase 3a"): words written
// into the caller's buffer, and optional indices.
package negs_core

import "base:runtime"

// Writes as much of `s` as fits into the caller's `out` (`cap` bytes, not
// NUL-terminated) and returns its whole length: a caller whose buffer was
// too small asks again with one that long.
@(private)
text_out_3a :: proc "contextless" (s: string, out: [^]u8, cap: uint) -> uint {
	n := min(uint(len(s)), cap)
	if out != nil && n > 0 {
		runtime.mem_copy_non_overlapping(out, raw_data(s), int(n))
	}
	return uint(len(s))
}

// An index or none (−1) as the header's int64_t.
@(private)
index_3a :: #force_inline proc "contextless" (i: int) -> i64 {
	return i64(i) if i >= 0 else -1
}

// A host slice: `count` items at `data` (NULL with 0 is empty).
@(private)
host_slice_3a :: #force_inline proc "contextless" (data: [^]$T, count: uint) -> []T {
	if data == nil || count == 0 {
		return nil
	}
	return data[:count]
}
