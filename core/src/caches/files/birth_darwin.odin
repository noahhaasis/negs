#+build darwin, freebsd, netbsd
// `Metadata::created` on the BSDs: st_birthtimespec.
package cache_files

import "core:sys/posix"

@(private)
birth_time :: proc(path: string, st: posix.stat_t) -> Maybe(Time) {
	return from_timespec(i64(st.st_birthtimespec.tv_sec), i64(st.st_birthtimespec.tv_nsec))
}
