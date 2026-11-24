#+build darwin, linux, freebsd, openbsd, netbsd
// The file system as Rust's std::fs sees it on Unix.
package files

import "core:strings"
import "core:sys/posix"

@(private = "file")
c_path :: proc(path: string) -> cstring {
	return strings.clone_to_cstring(path, context.temp_allocator)
}

@(private = "file")
last_error :: proc() -> Error {
	return Error(posix.errno())
}

@(private)
is_not_found :: proc(e: Error) -> bool {
	return posix.Errno(e) == .ENOENT
}

@(private)
os_message :: proc(e: Error) -> string {
	buf: [256]u8
	if posix.strerror_r(posix.Errno(e), raw_data(buf[:]), len(buf)) == .NONE {
		return strings.clone(string(cstring(raw_data(buf[:]))), context.temp_allocator)
	}
	return "Unknown error"
}

@(private = "file")
kind_of_mode :: proc "contextless" (mode: posix.mode_t) -> Kind {
	if posix.S_ISDIR(mode) {
		return .Dir
	}
	if posix.S_ISREG(mode) {
		return .File
	}
	return .Other
}

// `std::fs::metadata`.
stat :: proc(path: string) -> (Stat, bool) {
	if has_nul(path) {
		return {}, false
	}
	st: posix.stat_t
	if posix.stat(c_path(path), &st) != .OK {
		return {}, false
	}
	out := Stat {
		kind     = kind_of_mode(st.st_mode),
		size     = u64(st.st_size),
		modified = from_timespec(i64(st.st_mtim.tv_sec), i64(st.st_mtim.tv_nsec)),
	}
	out.created = birth_time(path, st)
	return out, true
}

// `std::fs::read_dir` and each entry's `file_type()` (d_type, or lstat
// when the file system doesn't say), "." and ".." left out; false at the
// first error, as `?` on any of them stops there.
read_dir :: proc(path: string, allocator := context.allocator) -> ([]Entry, bool) {
	if has_nul(path) {
		return nil, false
	}
	dir := posix.opendir(c_path(path))
	if dir == nil {
		return nil, false
	}
	defer posix.closedir(dir)
	fd := posix.dirfd(dir)
	out := make([dynamic]Entry, 0, 16, allocator)
	for {
		posix.set_errno(.NONE)
		entry := posix.readdir(dir)
		if entry == nil {
			if posix.errno() != .NONE {
				delete_entries(out[:], allocator)
				return nil, false
			}
			break
		}
		name := string(cstring(raw_data(entry.d_name[:])))
		if name == "." || name == ".." {
			continue
		}
		kind: Kind
		#partial switch entry.d_type {
		case .DIR:
			kind = .Dir
		case .REG:
			kind = .File
		case .UNKNOWN:
			st: posix.stat_t
			if posix.fstatat(fd, cstring(raw_data(entry.d_name[:])), &st, {.SYMLINK_NOFOLLOW}) != .OK {
				delete_entries(out[:], allocator)
				return nil, false
			}
			kind = kind_of_mode(st.st_mode)
		case:
			kind = .Other
		}
		append(&out, Entry{strings.clone(name, allocator), kind})
	}
	return out[:], true
}

delete_entries :: proc(entries: []Entry, allocator := context.allocator) {
	for e in entries {
		delete(e.name, allocator)
	}
	delete(entries, allocator)
}

// `std::fs::read`.
read :: proc(path: string, allocator := context.allocator) -> ([]u8, Error) {
	if has_nul(path) {
		return nil, NUL_IN_PATH
	}
	fd := posix.open(c_path(path), {.CLOEXEC})
	if fd < 0 {
		return nil, last_error()
	}
	defer posix.close(fd)
	size := 0
	st: posix.stat_t
	if posix.fstat(fd, &st) == .OK && st.st_size > 0 {
		size = int(st.st_size)
	}
	out := make([dynamic]u8, 0, size + 1, allocator)
	for {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		room := cap(out) - start
		resize(&out, cap(out))
		n := posix.read(fd, raw_data(out[start:]), uint(room))
		if n < 0 {
			resize(&out, start)
			if posix.errno() == .EINTR {
				continue
			}
			err := last_error()
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

// `File::create` and `write_all` of each part in turn (`std::fs::write`
// for one): created or truncated, 0o666 before the umask.
write :: proc(path: string, parts: ..[]u8) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	fd := posix.open(c_path(path), {.WRONLY, .CREAT, .TRUNC, .CLOEXEC}, transmute(posix.mode_t)posix._mode_t(0o666))
	if fd < 0 {
		return last_error()
	}
	for part in parts {
		rest := part
		for len(rest) > 0 {
			n := posix.write(fd, raw_data(rest), uint(min(len(rest), 1 << 30)))
			if n < 0 {
				if posix.errno() == .EINTR {
					continue
				}
				err := last_error()
				posix.close(fd)
				return err
			}
			rest = rest[n:]
		}
	}
	// Rust ignores close's errors.
	posix.close(fd)
	return 0
}

// What windows/src/platform.rs's replace_file is on Unix (the frozen
// oracles' stand-in): rename(2), which replaces `to` atomically.
replace :: proc(from, to: string) -> Error {
	if has_nul(from) || has_nul(to) {
		return NUL_IN_PATH
	}
	if posix.rename(c_path(from), c_path(to)) != 0 {
		return last_error()
	}
	return 0
}

// `std::fs::remove_file`.
remove :: proc(path: string) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	if posix.unlink(c_path(path)) != .OK {
		return last_error()
	}
	return 0
}

@(private)
make_dir :: proc(path: string) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	if posix.mkdir(c_path(path), transmute(posix.mode_t)posix._mode_t(0o777)) != .OK {
		return last_error()
	}
	return 0
}
