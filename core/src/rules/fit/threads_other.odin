#+build !windows
package fit

import "core:os"

// The Rust's `available_parallelism` (the logical processors), at most 16.
@(private = "package")
threads :: proc() -> int {
	return clamp(os.get_processor_core_count(), 1, 16)
}
