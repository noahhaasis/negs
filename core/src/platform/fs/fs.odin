// The core's one file layer: what the library, the settings (presets,
// session) and the caches (batch journal, match cache, rendered preview)
// ask of the file system and of paths, as Rust's std answers it, so each
// does what its Rust original did.
//
// - fs.odin: the types and what is the same on every platform (errors'
//   text, create_dir_all, write_atomic, times).
// - fs_posix.odin: macOS and Linux, std's Unix calls; the `windows` crate's
//   calls (MoveFileExW without replacing, the hidden attribute) as the
//   frozen oracles stand in for them on the Mac.
// - fs_windows.odin: Win32 as std calls it, every path through std's
//   `maybe_verbatim` (verbatim.odin), so a path over MAX_PATH works.
// - path.odin: std::path's rules for both platforms; display.odin: a
//   path's text (Display's lossy text) and UTF-16 (WTF-8 as OsStr has it).
//
// Paths are the host's bytes (WTF-8 on Windows). A path with a NUL in it
// reaches no system call, as in Rust. Results are in the allocator given.
package platform_fs

import "core:strings"

// An OS error: errno on Unix, GetLastError's code on Windows; 0 is none.
// std's own errors that carry no OS code are negative.
Error :: distinct i32

// A path with a NUL in it, refused before asking the OS (Unix: "file name
// contained an unexpected NUL byte"; Windows: "strings passed to WinAPI
// cannot contain NULs").
NUL_IN_PATH :: Error(-1)

// create_dir_all's "failed to create whole tree": a folder whose parent
// is missing has no parent to make.
WHOLE_TREE :: Error(-2)

// Rust's io::Error Display: std's own error's words, or the system's text
// for the code and " (os error N)" (sys::os::error_string: strerror_r on
// Unix, FormatMessageW on Windows).
error_text :: proc(e: Error, allocator := context.allocator) -> string {
	switch e {
	case NUL_IN_PATH:
		return strings.clone(NUL_TEXT, allocator)
	case WHOLE_TREE:
		return strings.clone("failed to create whole tree", allocator)
	}
	detail := error_string(e, context.temp_allocator)
	b := strings.builder_make(0, len(detail) + 24, allocator)
	strings.write_string(&b, detail)
	strings.write_string(&b, " (os error ")
	strings.write_int(&b, int(e))
	strings.write_byte(&b, ')')
	return strings.to_string(b)
}

// What std's FileType says: a symbolic link (a name-surrogate reparse
// point on Windows) is neither a file nor a folder.
Kind :: enum u8 {
	Other,
	File,
	Dir,
}

// A SystemTime: seconds since 1970 (negative before) and nanoseconds within
// the second.
System_Time :: struct {
	secs:  i64,
	nanos: u32,
}

// A time since 1970 (`duration_since(UNIX_EPOCH)`).
Duration :: struct {
	secs:  u64,
	nanos: u32,
}

// `std::fs::Metadata`.
Stat :: struct {
	kind:     Kind,
	size:     u64,
	// `permissions().readonly()`: no write bit at all on Unix, the
	// read-only attribute on Windows.
	readonly: bool,
	// The hidden attribute: FILE_ATTRIBUTE_HIDDEN on Windows, UF_HIDDEN in
	// st_flags on macOS, never on Linux.
	hidden:   bool,
	// `modified()` and `created()`; none where std's are an error (no
	// birth time on this file system, a timestamp out of range).
	modified: Maybe(System_Time),
	created:  Maybe(System_Time),
}

// One `read_dir` entry.
Dir_Entry :: struct {
	// `file_name()`, as OsString holds it (WTF-8 on Windows); `unicode` is
	// whether `to_str()` takes it.
	name:     string,
	unicode:  bool,
	// `file_type()`, unless that is an error (Unix: lstat failing for a
	// file system that gives no type).
	kind:     Kind,
	kind_ok:  bool,
	// `metadata()` (the entry itself, links not followed), when asked for
	// and it can be read.
	stat:     Stat,
	stat_ok:  bool,
}

delete_entries :: proc(entries: []Dir_Entry, allocator := context.allocator) {
	for e in entries {
		delete(e.name, allocator)
	}
	delete(entries, allocator)
}

// `duration_since(UNIX_EPOCH)`: false before 1970.
since_epoch :: proc "contextless" (t: System_Time) -> (Duration, bool) {
	if t.secs < 0 {
		return {}, false
	}
	return Duration{u64(t.secs), t.nanos}, true
}

// `Duration::as_secs_f64`: the seconds plus the nanoseconds over 1e9.
secs_f64 :: proc "contextless" (d: Duration) -> f64 {
	return f64(d.secs) + f64(d.nanos) / f64(1_000_000_000)
}

// `Duration::as_nanos`.
as_nanos :: proc "contextless" (d: Duration) -> u128 {
	return u128(d.secs) * 1_000_000_000 + u128(d.nanos)
}

// Nanoseconds since 1970, negative before.
unix_nanos :: proc "contextless" (t: System_Time) -> i64 {
	return t.secs * 1_000_000_000 + i64(t.nanos)
}

// A time from seconds and nanoseconds since 1970 as std makes one from a
// timespec: nanoseconds outside 0..<1e9 are an error.
@(private)
from_timespec :: proc "contextless" (secs: i64, nsec: i64) -> Maybe(System_Time) {
	if nsec < 0 || nsec >= 1_000_000_000 {
		return nil
	}
	return System_Time{secs, u32(nsec)}
}

// `Path::exists`: `metadata` succeeds.
exists :: proc(path: string) -> bool {
	_, err := stat(path)
	return err == 0
}

// `Path::is_dir`.
is_dir :: proc(path: string) -> bool {
	st, err := stat(path)
	return err == 0 && st.kind == .Dir
}

// `Path::is_file`.
is_file :: proc(path: string) -> bool {
	st, err := stat(path)
	return err == 0 && st.kind == .File
}

// `std::fs::create_dir_all`: every missing folder on the way, in std's
// order (the parent first only when the folder itself can't be made for a
// missing parent); a folder already there, or made meanwhile, is fine.
create_dir_all :: proc(path: string) -> Error {
	if path == "" {
		return 0
	}
	err := create_dir(path)
	if err == 0 {
		return 0
	}
	if is_not_found(err) {
		// Below: make the parent first.
	} else if is_dir(path) {
		return 0
	} else {
		return err
	}
	up, has_parent := parent(path)
	if !has_parent {
		return WHOLE_TREE
	}
	if e := create_dir_all(up); e != 0 {
		return e
	}
	err = create_dir(path)
	if err == 0 || is_dir(path) {
		return 0
	}
	return err
}

// windows/src/session.rs's `write_atomic`: `data` to a sibling with the
// extension `tmp`, renamed over `path` (platform.rs's replace_file), so a
// crash never leaves half a file. The error as anyhow's `{:#}` shows it.
write_atomic :: proc(path: string, data: []u8, allocator := context.allocator) -> (err: string, ok: bool) {
	if dir, has := parent(path); has {
		if e := create_dir_all(dir); e != 0 {
			return error_text(e, allocator), false
		}
	}
	staging := with_extension(path, "tmp", allocator = context.temp_allocator)
	if e := write(staging, data); e != 0 {
		return error_text(e, allocator), false
	}
	if e := replace_file(staging, path); e != 0 {
		message := win32_error_message(e, context.temp_allocator)
		return strings.concatenate({"Could not move the file to ", display(path, allocator = context.temp_allocator), ": ", message}, allocator), false
	}
	return "", true
}
