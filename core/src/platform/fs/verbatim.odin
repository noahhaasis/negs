// std's long paths on Windows (sys/path/windows.rs, `maybe_verbatim` and
// `get_long_path`, Rust 1.93): every path std hands a Win32 call goes
// through it, so a path longer than MAX_PATH still works. Plain logic on
// UTF-16, so it is tested on any platform; GetFullPathNameW comes in as a
// procedure (fs_windows.odin gives the real one).
package platform_fs

import "base:runtime"

// MAX_PATH is 260 units with the NUL, but CreateDirectoryW's limit is 248.
LEGACY_MAX_PATH :: 248

// GetFullPathNameW as std calls it (fill_utf16_buf): `path` ends in a NUL;
// the absolute path without one, or the call's error.
Full_Path_Proc :: #type proc(path: []u16, allocator: runtime.Allocator) -> ([]u16, Error)

@(private = "file")
SEP :: u16('\\')
@(private = "file")
ALT_SEP :: u16('/')

@(private = "file")
starts :: proc "contextless" (path: []u16, prefix: string) -> bool {
	if len(path) < len(prefix) {
		return false
	}
	for i in 0 ..< len(prefix) {
		if path[i] != u16(prefix[i]) {
			return false
		}
	}
	return true
}

// get_long_path's early return: `path` (with its NUL) is used as it is when
// it is verbatim already (`\\?\`, `\??\`), empty, or shorter than
// LEGACY_MAX_PATH with its NUL and either a drive and a separator or
// nothing (`C:\…`, `C:/…`, `C:`, where the "drive" is any unit but a
// separator) or two separators at the start (`\\server\…`, `\\.\…`,
// `//?/…`). Anything else (relative, `C:file`, `\file`, or long) goes
// through GetFullPathNameW.
keeps_path :: proc "contextless" (path: []u16) -> bool {
	if starts(path, `\\?\`) || starts(path, `\??\`) || (len(path) == 1 && path[0] == 0) {
		return true
	}
	if len(path) >= LEGACY_MAX_PATH {
		return false
	}
	is_sep :: proc "contextless" (u: u16) -> bool {
		return u == SEP || u == ALT_SEP
	}
	if len(path) >= 3 && path[1] == ':' && !is_sep(path[0]) {
		if len(path) == 3 && path[2] == 0 {
			return true
		}
		if is_sep(path[2]) {
			return true
		}
	}
	return len(path) >= 2 && is_sep(path[0]) && is_sep(path[1])
}

// get_long_path's second step, on GetFullPathNameW's `absolute` (no NUL,
// separators already `\`): the prefix to put before it and how many of its
// first units to drop. Only when std prefers verbatim (maybe_verbatim
// always does) or the path is LEGACY_MAX_PATH long with its NUL:
// `C:\…` → `\\?\C:\…`, `\\.\…` → `\\?\…`, `\\server\…` → `\\?\UNC\server\…`;
// `\\?\…`, `\??\…` and anything else are left alone.
verbatim_prefix :: proc "contextless" (absolute: []u16, prefer_verbatim: bool) -> (prefix: string, drop: int) {
	if !prefer_verbatim && len(absolute) + 1 < LEGACY_MAX_PATH {
		return "", 0
	}
	switch {
	case len(absolute) >= 3 && absolute[1] == ':' && absolute[2] == SEP:
		return `\\?\`, 0
	case starts(absolute, `\\.\`):
		return `\\?\`, 4
	case starts(absolute, `\\?\`), starts(absolute, `\??\`):
		return "", 0
	case starts(absolute, `\\`):
		return `\\?\UNC\`, 2
	}
	return "", 0
}

// `get_long_path(path, prefer_verbatim)`: `path` ends in a NUL (as
// `to_utf16(_, true)` makes it), and so does the result.
long_path :: proc(path: []u16, prefer_verbatim: bool, full: Full_Path_Proc, allocator := context.allocator) -> ([]u16, Error) {
	if keeps_path(path) {
		return path, 0
	}
	absolute, err := full(path, context.temp_allocator)
	if err != 0 {
		return nil, err
	}
	prefix, drop := verbatim_prefix(absolute, prefer_verbatim)
	rest := absolute[drop:]
	out := make([]u16, len(prefix) + len(rest) + 1, allocator)
	for i in 0 ..< len(prefix) {
		out[i] = u16(prefix[i])
	}
	copy(out[len(prefix):], rest)
	out[len(out) - 1] = 0
	return out, 0
}

// `maybe_verbatim`: a path as std gives it to Win32 (to_u16s, then
// get_long_path preferring verbatim). NUL_IN_PATH when it holds a NUL.
maybe_verbatim :: proc(path: string, full: Full_Path_Proc, allocator := context.allocator) -> ([]u16, Error) {
	units, ok := to_utf16(path, true, allocator)
	if !ok {
		return nil, NUL_IN_PATH
	}
	return long_path(units, true, full, allocator)
}
