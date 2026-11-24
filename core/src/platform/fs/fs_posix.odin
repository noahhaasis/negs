#+build darwin, linux
// The file system on macOS and Linux, as Rust's std::fs makes its calls
// there, and the `windows` crate's calls as core/diff/state-oracle stands
// in for them: MoveFileExW is rename(2) (replacing) or renamex_np
// (RENAME_EXCL) / renameat2(RENAME_NOREPLACE) (refusing), and
// SetFileAttributesW's hidden bit is chflags(UF_HIDDEN) (nothing on Linux).
package platform_fs

import "core:c"
import "core:strings"
import "core:sys/posix"
import "core:unicode/utf8"

when ODIN_OS == .Darwin {
	foreign import libc "system:System"

	@(default_calling_convention = "c")
	@(private)
	foreign libc {
		renamex_np :: proc(from, to: cstring, flags: c.uint) -> c.int ---
		chflags :: proc(path: cstring, flags: c.uint) -> c.int ---
		confstr :: proc(name: c.int, buf: [^]u8, len: c.size_t) -> c.size_t ---
	}

	@(private)
	F_FULLFSYNC :: 51
	@(private)
	_CS_DARWIN_USER_TEMP_DIR :: 65537
	@(private)
	RENAME_EXCL :: 0x4
} else {
	foreign import libc "system:c"

	@(default_calling_convention = "c")
	@(private)
	foreign libc {
		renameat2 :: proc(olddirfd: c.int, from: cstring, newdirfd: c.int, to: cstring, flags: c.uint) -> c.int ---
	}
}

UF_HIDDEN :: 0x8000

@(private)
NUL_TEXT :: "file name contained an unexpected NUL byte"

@(private)
last :: proc "contextless" () -> Error {
	return Error(posix.errno())
}

// The path for a call (std's run_path_with_cstr): false when it holds a
// NUL.
@(private)
cpath :: proc(path: string) -> (cstring, bool) {
	if strings.index_byte(path, 0) >= 0 {
		return nil, false
	}
	return strings.clone_to_cstring(path, context.temp_allocator), true
}

// std's `io::ErrorKind::NotFound`.
is_not_found :: proc "contextless" (e: Error) -> bool {
	return posix.Errno(e) == .ENOENT
}

// sys::os::error_string: strerror_r's text in 128 bytes, lossy UTF-8.
// (macOS fills the buffer with "Unknown error: N" for a code it doesn't
// know, and std shows that.)
@(private)
error_string :: proc(e: Error, allocator := context.allocator) -> string {
	buf: [128]u8
	_ = posix.strerror_r(posix.Errno(e), raw_data(buf[:]), len(buf))
	buf[len(buf) - 1] = 0
	return display(string(cstring(raw_data(buf[:]))), .Unix, allocator)
}

// The windows crate's `Error::message()` for a MoveFileExW error, as the
// stand-in gives it: io::Error's text.
win32_error_message :: proc(e: Error, allocator := context.allocator) -> string {
	return error_text(e, allocator)
}

@(private)
kind_of_mode :: proc "contextless" (mode: posix.mode_t) -> Kind {
	if posix.S_ISDIR(mode) {
		return .Dir
	}
	if posix.S_ISREG(mode) {
		return .File
	}
	return .Other
}

@(private)
from_stat :: proc(st: posix.stat_t) -> Stat {
	out := Stat {
		kind     = kind_of_mode(st.st_mode),
		size     = u64(st.st_size),
		readonly = st.st_mode & {.IWUSR, .IWGRP, .IWOTH} == {},
		modified = from_timespec(i64(st.st_mtim.tv_sec), i64(st.st_mtim.tv_nsec)),
	}
	when ODIN_OS == .Darwin {
		out.hidden = st.st_flags & UF_HIDDEN != 0
		out.created = from_timespec(i64(st.st_birthtimespec.tv_sec), i64(st.st_birthtimespec.tv_nsec))
	}
	return out
}

// `std::fs::metadata` (links followed). `created` is st_birthtimespec on
// macOS, statx's btime on Linux when the file system gives one.
stat :: proc(path: string) -> (Stat, Error) {
	p, ok := cpath(path)
	if !ok {
		return {}, NUL_IN_PATH
	}
	st: posix.stat_t
	if posix.stat(p, &st) != .OK {
		return {}, last()
	}
	out := from_stat(st)
	when ODIN_OS == .Linux {
		out.created = linux_birth_time(p)
	}
	return out, 0
}

// `std::fs::read_dir`, "." and ".." left out, with each entry's
// `file_type()` (d_type, or lstat when the file system gives none) and,
// when `metadata` is asked for, `metadata()` (lstat). `err` is opening's
// error; `stopped` the error that ended the listing early (std's iterator
// gives it, then nothing more).
read_dir :: proc(path: string, metadata := false, allocator := context.allocator) -> (entries: []Dir_Entry, err: Error, stopped: Error) {
	p, ok := cpath(path)
	if !ok {
		return nil, NUL_IN_PATH, 0
	}
	dir := posix.opendir(p)
	if dir == nil {
		return nil, last(), 0
	}
	defer posix.closedir(dir)
	fd := posix.dirfd(dir)
	out := make([dynamic]Dir_Entry, 0, 16, allocator)
	for {
		posix.set_errno(.NONE)
		entry := posix.readdir(dir)
		if entry == nil {
			stopped = last()
			break
		}
		cname := cstring(raw_data(entry.d_name[:]))
		name := string(cname)
		if name == "." || name == ".." {
			continue
		}
		e := Dir_Entry {
			name    = strings.clone(name, allocator),
			unicode = utf8.valid_string(name),
			kind_ok = true,
		}
		looked := false
		if metadata {
			st: posix.stat_t
			if posix.fstatat(fd, cname, &st, {.SYMLINK_NOFOLLOW}) == .OK {
				e.stat = from_stat(st)
				e.stat_ok = true
			}
			looked = true
		}
		#partial switch entry.d_type {
		case .DIR:
			e.kind = .Dir
		case .REG:
			e.kind = .File
		case .UNKNOWN:
			if looked {
				e.kind, e.kind_ok = e.stat.kind, e.stat_ok
			} else {
				st: posix.stat_t
				if posix.fstatat(fd, cname, &st, {.SYMLINK_NOFOLLOW}) == .OK {
					e.kind = kind_of_mode(st.st_mode)
				} else {
					e.kind_ok = false
				}
			}
		case:
			e.kind = .Other
		}
		append(&out, e)
	}
	return out[:], 0, stopped
}

// `File::open` and `read_to_end`, at most `limit` bytes (`take(limit)`):
// `std::fs::read` with no limit.
read :: proc(path: string, limit := max(int), allocator := context.allocator) -> ([]u8, Error) {
	p, ok := cpath(path)
	if !ok {
		return nil, NUL_IN_PATH
	}
	fd := posix.open(p, {.CLOEXEC})
	if fd < 0 {
		return nil, last()
	}
	defer posix.close(fd)
	// Room for the whole file and the read that finds its end.
	size := 0
	st: posix.stat_t
	if posix.fstat(fd, &st) == .OK && st.st_size > 0 {
		size = int(st.st_size)
	}
	out := make([dynamic]u8, 0, max(min(size, limit - 1) + 1, 1), allocator)
	for len(out) < limit {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		room := min(cap(out) - start, limit - start, 0x7fff_fffe)
		resize(&out, start + room)
		n := posix.read(fd, raw_data(out[start:]), uint(room))
		if n < 0 {
			resize(&out, start)
			if posix.errno() == .EINTR {
				continue
			}
			err := last()
			delete(out)
			return nil, err
		}
		resize(&out, start + int(n))
		if n == 0 {
			break
		}
	}
	return out[:], 0
}

@(private)
write_all :: proc(fd: posix.FD, data: []u8) -> Error {
	rest := data
	for len(rest) > 0 {
		n := posix.write(fd, raw_data(rest), uint(min(len(rest), 0x7fff_fffe)))
		if n < 0 {
			e := last()
			if posix.Errno(e) == .EINTR {
				continue
			}
			return e
		}
		rest = rest[n:]
	}
	return 0
}

@(private)
MODE_666 :: transmute(posix.mode_t)posix._mode_t(0o666)

// `File::create` and `write_all` of each part in turn (`std::fs::write`
// for one): created or truncated, 0o666 before the umask. Rust ignores
// close's errors.
write :: proc(path: string, parts: ..[]u8) -> Error {
	p, ok := cpath(path)
	if !ok {
		return NUL_IN_PATH
	}
	fd := posix.open(p, {.WRONLY, .CREAT, .TRUNC, .CLOEXEC}, MODE_666)
	if fd < 0 {
		return last()
	}
	defer posix.close(fd)
	for part in parts {
		if e := write_all(fd, part); e != 0 {
			return e
		}
	}
	return 0
}

// OpenOptions::new().write(true).create_new(true), write_all, sync_all
// (F_FULLFSYNC on macOS, as Rust's).
write_new_synced :: proc(path: string, data: []u8) -> Error {
	p, ok := cpath(path)
	if !ok {
		return NUL_IN_PATH
	}
	fd := posix.open(p, {.WRONLY, .CREAT, .EXCL, .CLOEXEC}, MODE_666)
	if fd < 0 {
		return last()
	}
	defer posix.close(fd)
	if e := write_all(fd, data); e != 0 {
		return e
	}
	for {
		when ODIN_OS == .Darwin {
			synced := posix.fcntl(fd, posix.FCNTL_Cmd(F_FULLFSYNC)) != -1
		} else {
			synced := posix.fsync(fd) == .OK
		}
		if synced {
			return 0
		}
		if e := last(); posix.Errno(e) != .EINTR {
			return e
		}
	}
}

// `std::fs::remove_file`.
remove_file :: proc(path: string) -> Error {
	p, ok := cpath(path)
	if !ok {
		return NUL_IN_PATH
	}
	if posix.unlink(p) != .OK {
		return last()
	}
	return 0
}

// `std::fs::create_dir` (DirBuilder, 0o777 before the umask).
create_dir :: proc(path: string) -> Error {
	p, ok := cpath(path)
	if !ok {
		return NUL_IN_PATH
	}
	if posix.mkdir(p, transmute(posix.mode_t)posix._mode_t(0o777)) != .OK {
		return last()
	}
	return 0
}

@(private)
rename_with :: proc(from, to: string, exclusive: bool) -> Error {
	f, fok := cpath(from)
	t, tok := cpath(to)
	if !fok || !tok {
		return NUL_IN_PATH
	}
	if !exclusive {
		if posix.rename(f, t) != 0 {
			return last()
		}
		return 0
	}
	when ODIN_OS == .Darwin {
		done := renamex_np(f, t, RENAME_EXCL) == 0
	} else {
		AT_FDCWD :: -100
		RENAME_NOREPLACE :: 1
		done := renameat2(AT_FDCWD, f, AT_FDCWD, t, RENAME_NOREPLACE) == 0
	}
	return 0 if done else last()
}

// `std::fs::rename`: rename(2), which replaces `to`.
rename :: proc(from, to: string) -> Error {
	return rename_with(from, to, false)
}

// windows/src/platform.rs's replace_file (MoveFileExW with
// MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH), as the stand-ins
// make it: rename(2).
replace_file :: proc(from, to: string) -> Error {
	return rename_with(from, to, false)
}

// MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH), which fails if `to`
// exists (presets.rs), as the stand-in makes it: renamex_np(RENAME_EXCL)
// on macOS, renameat2(RENAME_NOREPLACE) on Linux. The error is the errno.
move_no_replace :: proc(from, to: string) -> Error {
	return rename_with(from, to, true)
}

// SetFileAttributesW(path, FILE_ATTRIBUTE_HIDDEN), errors ignored:
// chflags(UF_HIDDEN) on macOS (replacing the other flags, as the hidden
// attribute alone replaces the others there); nothing on Linux.
hide :: proc(path: string) {
	when ODIN_OS == .Darwin {
		if p, ok := cpath(path); ok {
			_ = chflags(p, UF_HIDDEN)
		}
	}
}

// MARK: The process

// `std::env::var_os`: unset is (_, false); set and empty is ("", true).
env :: proc(name: string, allocator := context.allocator) -> (string, bool) {
	n, ok := cpath(name)
	if !ok {
		return "", false
	}
	v := posix.getenv(n)
	if v == nil {
		return "", false
	}
	return strings.clone(string(v), allocator), true
}

// `std::env::temp_dir`: TMPDIR, else the user's temporary folder (macOS),
// else /tmp.
temp_dir :: proc(allocator := context.allocator) -> string {
	if dir, ok := env("TMPDIR", allocator); ok {
		return dir
	}
	when ODIN_OS == .Darwin {
		buf: [1024]u8
		n := confstr(_CS_DARWIN_USER_TEMP_DIR, raw_data(buf[:]), len(buf))
		if n > 0 && n <= len(buf) {
			return strings.clone(string(buf[:n - 1]), allocator)
		}
	}
	return strings.clone("/tmp", allocator)
}

process_id :: proc "contextless" () -> u32 {
	return u32(posix.getpid())
}

// `SystemTime::now()`.
now :: proc "contextless" () -> System_Time {
	ts: posix.timespec
	posix.clock_gettime(.REALTIME, &ts)
	return System_Time{i64(ts.tv_sec), u32(ts.tv_nsec)}
}
