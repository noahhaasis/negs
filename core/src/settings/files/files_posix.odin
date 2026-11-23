#+build darwin, linux
// The file operations on macOS and Linux, as Rust's std::fs makes them
// there and as core/diff/state-oracle stands in for the Win32 calls:
// MoveFileExW is rename(2) (replacing) or renamex_np(RENAME_EXCL)
// (refusing; renameat2(RENAME_NOREPLACE) on Linux), SetFileAttributesW's
// hidden bit is chflags(UF_HIDDEN) (nothing on Linux), and a Win32 error
// is the stand-in's mapping of errno (see `move_code`).
package files

import "core:c"
import "core:strings"
import "core:sys/posix"
import "../paths"

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
} else {
	foreign import libc "system:c"

	@(default_calling_convention = "c")
	@(private)
	foreign libc {
		renameat2 :: proc(olddirfd: c.int, from: cstring, newdirfd: c.int, to: cstring, flags: c.uint) -> c.int ---
	}
}

@(private)
EINTR :: 4
@(private)
ENOENT :: 2
@(private)
EEXIST :: 17

@(private)
last :: proc "contextless" () -> Code {
	return Code(posix.errno())
}

@(private)
cpath :: proc(path: string) -> cstring {
	return strings.clone_to_cstring(path, context.temp_allocator)
}

// std's `io::ErrorKind::NotFound`.
is_not_found :: proc "contextless" (code: Code) -> bool {
	return code == ENOENT
}

// What Rust's io::Error says of an errno (sys::os::error_string).
@(private)
os_error_text :: proc(code: Code, allocator := context.temp_allocator) -> string {
	if code == CODE_WHOLE_TREE {
		return strings.clone("failed to create whole tree", allocator)
	}
	buf: [128]u8
	text := "Unknown error"
	if posix.strerror_r(posix.Errno(code), raw_data(buf[:]), len(buf)) == .NONE {
		text = string(cstring(raw_data(buf[:])))
	}
	b := strings.builder_make(0, len(text) + 20, allocator)
	strings.write_string(&b, text)
	strings.write_string(&b, " (os error ")
	strings.write_int(&b, int(code))
	strings.write_byte(&b, ')')
	return strings.to_string(b)
}

UF_HIDDEN :: 0x8000

// `std::fs::metadata`.
stat :: proc(path: string) -> (out: Stat, err: Code) {
	st: posix.stat_t
	if posix.stat(cpath(path), &st) != .OK {
		return {}, last()
	}
	out.is_file = posix.S_ISREG(st.st_mode)
	out.is_dir = posix.S_ISDIR(st.st_mode)
	out.size = u64(st.st_size)
	out.seconds = i64(st.st_mtim.tv_sec)
	out.nanos = u32(st.st_mtim.tv_nsec)
	when ODIN_OS == .Darwin {
		out.hidden = st.st_flags & UF_HIDDEN != 0
	}
	return out, 0
}

// `std::fs::read`.
read_file :: proc(path: string, allocator := context.temp_allocator) -> ([]u8, Code) {
	fd := posix.open(cpath(path), {.CLOEXEC})
	if fd < 0 {
		return nil, last()
	}
	defer posix.close(fd)
	size := 4096
	st: posix.stat_t
	if posix.fstat(fd, &st) == .OK && st.st_size >= 0 {
		size = int(st.st_size) + 1
	}
	out := make([dynamic]u8, 0, max(size, 1), allocator)
	for {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		resize(&out, cap(out))
		room: int = len(out) - start
		n := posix.read(fd, raw_data(out[start:]), uint(room))
		if n < 0 {
			e := last()
			resize(&out, start)
			if e == EINTR {
				continue
			}
			delete(out)
			return nil, e
		}
		resize(&out, start + int(n))
		if n == 0 {
			break
		}
	}
	return out[:], 0
}

@(private)
write_all :: proc(fd: posix.FD, data: []u8) -> Code {
	rest := data
	for len(rest) > 0 {
		n := posix.write(fd, raw_data(rest), uint(min(len(rest), 0x7fff_ffff)))
		if n < 0 {
			e := last()
			if e == EINTR {
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

// `std::fs::write`: created or truncated, 0o666 before the umask.
write_file :: proc(path: string, data: []u8) -> Code {
	fd := posix.open(cpath(path), {.WRONLY, .CREAT, .TRUNC, .CLOEXEC}, MODE_666)
	if fd < 0 {
		return last()
	}
	err := write_all(fd, data)
	posix.close(fd)
	return err
}

// OpenOptions::new().write(true).create_new(true), write_all, sync_all
// (F_FULLFSYNC on macOS, as Rust's).
write_new_synced :: proc(path: string, data: []u8) -> Code {
	fd := posix.open(cpath(path), {.WRONLY, .CREAT, .EXCL, .CLOEXEC}, MODE_666)
	if fd < 0 {
		return last()
	}
	defer posix.close(fd)
	if err := write_all(fd, data); err != 0 {
		return err
	}
	for {
		when ODIN_OS == .Darwin {
			ok := posix.fcntl(fd, posix.FCNTL_Cmd(F_FULLFSYNC)) != -1
		} else {
			ok := posix.fsync(fd) == .OK
		}
		if ok {
			return 0
		}
		if e := last(); e != EINTR {
			return e
		}
	}
}

// `std::fs::remove_file`.
remove_file :: proc(path: string) -> Code {
	if posix.unlink(cpath(path)) != .OK {
		return last()
	}
	return 0
}

@(private)
create_dir :: proc(path: string) -> Code {
	if posix.mkdir(cpath(path), transmute(posix.mode_t)posix._mode_t(0o777)) != .OK {
		return last()
	}
	return 0
}

@(private)
parent_of :: proc(path: string) -> (string, bool) {
	return paths.parent(path, .Posix)
}

// The names in `folder` but `.` and `..`, in the order the OS gives them
// (`std::fs::read_dir`).
read_dir :: proc(folder: string, allocator := context.temp_allocator) -> ([]string, Code) {
	dir := posix.opendir(cpath(folder))
	if dir == nil {
		return nil, last()
	}
	defer posix.closedir(dir)
	out := make([dynamic]string, 0, 64, allocator)
	for {
		entry := posix.readdir(dir)
		if entry == nil {
			break
		}
		name := string(cstring(raw_data(entry.d_name[:])))
		if name == "." || name == ".." {
			continue
		}
		append(&out, strings.clone(name, allocator))
	}
	return out[:], 0
}

// MARK: The Win32 calls, as the stand-ins make them

// The Win32 error code the stand-in's MoveFileExW reports for an errno:
// EEXIST is ERROR_ALREADY_EXISTS (183), ENOENT ERROR_FILE_NOT_FOUND (2),
// EACCES and EPERM ERROR_ACCESS_DENIED (5), others 0x10000 + errno.
@(private)
move_code :: proc "contextless" (errno: Code) -> Code {
	switch errno {
	case EEXIST:
		return 183
	case ENOENT:
		return 2
	case 1, 13:
		return 5
	}
	return 0x1_0000 + errno
}

// MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
// (`platform::replace_file`): rename(2). The error's Win32 code and the
// windows crate's `message()` (here io::Error's text, as the stand-in's).
replace_file :: proc(from, to: string, allocator := context.temp_allocator) -> (Code, string) {
	when ODIN_OS == .Darwin {
		ok := renamex_np(cpath(from), cpath(to), 0) == 0
	} else {
		ok := posix.rename(cpath(from), cpath(to)) == 0
	}
	if ok {
		return 0, ""
	}
	e := last()
	return move_code(e), os_error_text(e, allocator)
}

@(private)
RENAME_EXCL :: 0x4

// MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH): fails if `to` exists.
move_no_replace :: proc(from, to: string, allocator := context.temp_allocator) -> (Code, string) {
	when ODIN_OS == .Darwin {
		ok := renamex_np(cpath(from), cpath(to), RENAME_EXCL) == 0
	} else {
		AT_FDCWD :: -100
		RENAME_NOREPLACE :: 1
		ok := renameat2(AT_FDCWD, cpath(from), AT_FDCWD, cpath(to), RENAME_NOREPLACE) == 0
	}
	if ok {
		return 0, ""
	}
	e := last()
	return move_code(e), os_error_text(e, allocator)
}

// SetFileAttributesW(path, FILE_ATTRIBUTE_HIDDEN), errors ignored:
// chflags(UF_HIDDEN) on macOS (replacing the other flags, as the hidden
// attribute alone replaces the others there); nothing on Linux.
hide :: proc(path: string) {
	when ODIN_OS == .Darwin {
		_ = chflags(cpath(path), UF_HIDDEN)
	}
}

// MARK: The process

// `std::env::var_os`: unset is (_, false); set and empty is ("", true).
env :: proc(name: string, allocator := context.temp_allocator) -> (string, bool) {
	v := posix.getenv(cpath(name))
	if v == nil {
		return "", false
	}
	return strings.clone(string(v), allocator), true
}

// `std::env::temp_dir`: TMPDIR, else the user's temporary folder (macOS),
// else /tmp.
temp_dir :: proc(allocator := context.temp_allocator) -> string {
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

// SystemTime::now() since 1970, in nanoseconds.
now_nanos :: proc "contextless" () -> i128 {
	ts: posix.timespec
	posix.clock_gettime(.REALTIME, &ts)
	return i128(ts.tv_sec) * 1_000_000_000 + i128(ts.tv_nsec)
}
