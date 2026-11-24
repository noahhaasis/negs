// What the caches and the batch journal ask of the file system, as Rust's
// std::fs answers it (files_posix.odin, files_windows.odin), and the
// std::path rules they name files by (paths.odin).
//
// Paths are the host's bytes: UTF-8 on Windows (Rust's WTF-8 of a path
// that is valid Unicode), whatever the file system holds on Unix. A path
// with a NUL in it opens nothing, as in Rust.
package cache_files

import "base:runtime"
import "core:strings"
import "core:time"

// A time since 1970 (std's SystemTime::duration_since(UNIX_EPOCH)); an
// earlier one is none, as `duration_since` fails for it.
Time :: struct {
	secs:  u64,
	nanos: u32,
}

// What std's FileType says: a symbolic link (read_dir doesn't follow
// one) is neither a file nor a folder.
Kind :: enum u8 {
	Other,
	File,
	Dir,
}

// `std::fs::metadata`: links followed. A time that std can't give (before
// 1970, out of range, or no birth time on this file system) is none.
Stat :: struct {
	kind:     Kind,
	size:     u64,
	modified: Maybe(Time),
	created:  Maybe(Time),
}

// One `read_dir` entry: its name and `DirEntry::file_type`.
Entry :: struct {
	name: string,
	kind: Kind,
}

// An OS error (errno, or GetLastError on Windows); 0 is none.
Error :: distinct i32

// For paths with a NUL in them, which Rust refuses before asking the OS.
NUL_IN_PATH :: Error(-1)

// `Duration::as_secs_f64`: the seconds plus the nanoseconds over 1e9.
secs_f64 :: proc "contextless" (t: Time) -> f64 {
	return f64(t.secs) + f64(t.nanos) / f64(1_000_000_000)
}

// `Duration::as_nanos`, which the rendered preview's stamps write.
nanos :: proc "contextless" (t: Time) -> u128 {
	return u128(t.secs) * 1_000_000_000 + u128(t.nanos)
}

// `SystemTime::now().duration_since(UNIX_EPOCH)`, none before 1970.
now :: proc "contextless" () -> Maybe(Time) {
	ns := time.to_unix_nanoseconds(time.now())
	if ns < 0 {
		return nil
	}
	return Time{u64(ns / 1_000_000_000), u32(ns % 1_000_000_000)}
}

// A time from seconds and nanoseconds since 1970 as std makes one from
// a timespec: nanoseconds outside 0..<1e9 are an error, and a time before
// 1970 has no duration since it.
from_timespec :: proc "contextless" (secs: i64, nsec: i64) -> Maybe(Time) {
	if nsec < 0 || nsec >= 1_000_000_000 || secs < 0 {
		return nil
	}
	return Time{u64(secs), u32(nsec)}
}

@(private)
has_nul :: proc "contextless" (path: string) -> bool {
	return strings.index_byte(path, 0) >= 0
}

// `std::fs::create_dir_all`: nothing to do for ""; mkdir, and when the
// parent is missing, the parents first; a folder already there (or made
// meanwhile) is fine.
create_dir_all :: proc(path: string) -> Error {
	if path == "" {
		return 0
	}
	err := make_dir(path)
	if err == 0 {
		return 0
	}
	if !is_not_found(err) {
		if st, ok := stat(path); ok && st.kind == .Dir {
			return 0
		}
		return err
	}
	parent_path, has := parent(path)
	if !has {
		return err
	}
	if e := create_dir_all(parent_path); e != 0 {
		return e
	}
	err = make_dir(path)
	if err == 0 {
		return 0
	}
	if st, ok := stat(path); ok && st.kind == .Dir {
		return 0
	}
	return err
}

// What Rust's io::Error says for an OS error: "{message} (os error {n})".
error_text :: proc(e: Error, allocator := context.allocator) -> string {
	if e == NUL_IN_PATH {
		return strings.clone("file name contained an unexpected NUL byte", allocator)
	}
	b := strings.builder_make(0, 64, allocator)
	strings.write_string(&b, os_message(e))
	strings.write_string(&b, " (os error ")
	strings.write_int(&b, int(e))
	strings.write_byte(&b, ')')
	return strings.to_string(b)
}

@(private)
Allocator :: runtime.Allocator
