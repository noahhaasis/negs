#+build darwin, linux, freebsd, openbsd, netbsd
// The file operations the library needs, as Rust's std::fs does them on
// Unix, with its error words ("No such file or directory (os error 2)").
package library

import "core:strings"
import "core:sys/posix"

// An OS error number; 0 is none.
Io_Error :: distinct i32

io_not_found :: proc(e: Io_Error) -> bool {
	return posix.Errno(e) == .ENOENT
}

// What Rust's io::Error says for it.
io_error_text :: proc(e: Io_Error, allocator := context.allocator) -> string {
	buf: [256]u8
	text := "Unknown error"
	if posix.strerror_r(posix.Errno(e), raw_data(buf[:]), len(buf)) == .NONE {
		text = string(cstring(raw_data(buf[:])))
	}
	b := strings.builder_make(0, len(text) + 16, allocator)
	strings.write_string(&b, text)
	strings.write_string(&b, " (os error ")
	strings.write_int(&b, int(e))
	strings.write_byte(&b, ')')
	return strings.to_string(b)
}

@(private = "file")
last_error :: proc() -> Io_Error {
	return Io_Error(posix.errno())
}

@(private = "file")
c_path :: proc(path: string) -> cstring {
	return strings.clone_to_cstring(path, context.temp_allocator)
}

// A file's size and modification time (nanoseconds since 1970).
File_Stat :: struct {
	regular:  bool,
	size:     i64,
	modified: i64,
}

@(private = "file")
from_stat :: proc(st: posix.stat_t) -> File_Stat {
	return File_Stat {
		regular = posix.S_ISREG(st.st_mode),
		size = i64(st.st_size),
		modified = i64(st.st_mtim.tv_sec) * 1_000_000_000 + i64(st.st_mtim.tv_nsec),
	}
}

// `std::fs::metadata` (symbolic links followed).
fs_stat :: proc(path: string) -> (File_Stat, Io_Error) {
	st: posix.stat_t
	if posix.stat(c_path(path), &st) != .OK {
		return {}, last_error()
	}
	return from_stat(st), 0
}

fs_exists :: proc(path: string) -> bool {
	_, err := fs_stat(path)
	return err == 0
}

// Whether `folder` isn't read-only: `!metadata.permissions().readonly()`,
// no write bit at all.
fs_writable_dir :: proc(folder: string) -> bool {
	st: posix.stat_t
	if posix.stat(c_path(folder), &st) != .OK {
		return false
	}
	return st.st_mode & {.IWUSR, .IWGRP, .IWOTH} != {}
}

Dir_Entry :: struct {
	name:   string,
	// From lstat: false when it couldn't be read.
	ok:     bool,
	hidden: bool,
	stat:   File_Stat,
}

// The entries of `folder` in the order the OS gives them, with lstat's view
// of each (`fs::read_dir` and `DirEntry::metadata`).
fs_read_dir :: proc(folder: string, allocator := context.temp_allocator) -> ([]Dir_Entry, Io_Error) {
	dir := posix.opendir(c_path(folder))
	if dir == nil {
		return nil, last_error()
	}
	defer posix.closedir(dir)
	fd := posix.dirfd(dir)
	out := make([dynamic]Dir_Entry, 0, 64, allocator)
	for {
		posix.set_errno(.NONE)
		entry := posix.readdir(dir)
		if entry == nil {
			break
		}
		name := string(cstring(raw_data(entry.d_name[:])))
		if name == "." || name == ".." {
			continue
		}
		e := Dir_Entry{name = strings.clone(name, allocator), hidden = len(name) > 0 && name[0] == '.'}
		st: posix.stat_t
		if posix.fstatat(fd, cstring(raw_data(entry.d_name[:])), &st, {.SYMLINK_NOFOLLOW}) == .OK {
			e.ok = true
			e.stat = from_stat(st)
		}
		append(&out, e)
	}
	return out[:], 0
}

// A file's bytes, at most `limit` (`File::open` and `take(limit)`).
fs_read :: proc(path: string, limit: int, allocator := context.temp_allocator) -> ([]u8, Io_Error) {
	fd := posix.open(c_path(path), {.CLOEXEC})
	if fd < 0 {
		return nil, last_error()
	}
	defer posix.close(fd)
	out := make([dynamic]u8, 0, 4096, allocator)
	for len(out) < limit {
		if cap(out) - len(out) < 4096 {
			reserve(&out, cap(out) * 2)
		}
		room := min(cap(out) - len(out), limit - len(out))
		start := len(out)
		resize(&out, start + room)
		n := posix.read(fd, raw_data(out[start:]), uint(room))
		if n < 0 {
			if posix.errno() == .EINTR {
				resize(&out, start)
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

// `fs::write`: created or truncated, 0o666 before the umask.
fs_write :: proc(path: string, data: []u8) -> Io_Error {
	fd := posix.open(c_path(path), {.WRONLY, .CREAT, .TRUNC, .CLOEXEC}, transmute(posix.mode_t)posix._mode_t(0o666))
	if fd < 0 {
		return last_error()
	}
	rest := data
	for len(rest) > 0 {
		n := posix.write(fd, raw_data(rest), uint(len(rest)))
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
	// Rust ignores close's errors.
	posix.close(fd)
	return 0
}

fs_rename :: proc(from, to: string) -> Io_Error {
	if posix.rename(c_path(from), c_path(to)) != 0 {
		return last_error()
	}
	return 0
}

fs_remove :: proc(path: string) -> Io_Error {
	if posix.unlink(c_path(path)) != .OK {
		return last_error()
	}
	return 0
}
