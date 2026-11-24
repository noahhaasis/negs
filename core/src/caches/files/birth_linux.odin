#+build linux
// `Metadata::created` on Linux: statx's btime when the file system gives
// one (STATX_BTIME in the mask), as Rust's std asks for it; none
// otherwise, where std says "creation time is not available".
package cache_files

import "core:strings"
import "core:sys/linux"
import "core:sys/posix"

@(private)
birth_time :: proc(path: string, st: posix.stat_t) -> Maybe(Time) {
	c := strings.clone_to_cstring(path, context.temp_allocator)
	sx: linux.Statx
	if linux.statx(linux.AT_FDCWD, c, {}, {.BTIME}, &sx) != .NONE || .BTIME not_in sx.mask {
		return nil
	}
	return from_timespec(i64(sx.btime.sec), i64(sx.btime.nsec))
}
