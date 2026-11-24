#+build linux
// `Metadata::created` on Linux: statx's btime when the file system gives
// one (STATX_BTIME in the mask), as Rust's std asks for it; none
// otherwise, where std says "creation time is not available".
package platform_fs

import "core:sys/linux"

@(private)
linux_birth_time :: proc(path: cstring) -> Maybe(System_Time) {
	sx: linux.Statx
	if linux.statx(linux.AT_FDCWD, path, {}, {.BTIME}, &sx) != .NONE || .BTIME not_in sx.mask {
		return nil
	}
	return from_timespec(i64(sx.btime.sec), i64(sx.btime.nsec))
}
