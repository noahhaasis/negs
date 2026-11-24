#+build openbsd
// `Metadata::created` on OpenBSD: none, as std has no birth time there.
package files

import "core:sys/posix"

@(private)
birth_time :: proc(path: string, st: posix.stat_t) -> Maybe(Time) {
	return nil
}
