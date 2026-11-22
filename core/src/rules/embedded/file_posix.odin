#+build darwin, linux, freebsd, openbsd, netbsd
// Reading a file a piece at a time, as Rust's File::open, metadata().len()
// and seek + read_exact do on Unix.
package embedded

import "core:strings"
import "core:sys/posix"

File :: posix.FD

// The file open for reading and its size; false when it can't be opened
// (a path with a NUL in it can't be, as in Rust).
file_open :: proc(path: string) -> (File, u64, bool) {
	if strings.index_byte(path, 0) >= 0 {
		return -1, 0, false
	}
	c := strings.clone_to_cstring(path, context.allocator)
	defer delete(c)
	fd := posix.open(c, {.CLOEXEC})
	if fd < 0 {
		return -1, 0, false
	}
	st: posix.stat_t
	if posix.fstat(fd, &st) != .OK || st.st_size < 0 {
		posix.close(fd)
		return -1, 0, false
	}
	return fd, u64(st.st_size), true
}

file_close :: proc(f: File) {
	posix.close(f)
}

// All of `out` from `offset`: false at an error or the file's end.
file_read_exact :: proc(f: File, offset: u64, out: []u8) -> bool {
	done := 0
	for done < len(out) {
		remaining := len(out) - done
		n := posix.pread(f, raw_data(out[done:]), uint(remaining), posix.off_t(offset + u64(done)))
		if n < 0 {
			if posix.errno() == .EINTR {
				continue
			}
			return false
		}
		if n == 0 {
			return false
		}
		done += int(n)
	}
	return true
}
