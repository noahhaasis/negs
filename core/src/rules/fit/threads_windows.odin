#+build windows
package fit

import win32 "core:sys/windows"

// The Rust's `available_parallelism`, at most 16. Not `os`'s count: on
// Windows it takes the temporary allocator.
@(private = "package")
threads :: proc() -> int {
	info: win32.SYSTEM_INFO
	win32.GetSystemInfo(&info)
	return clamp(int(info.dwNumberOfProcessors), 1, 16)
}
