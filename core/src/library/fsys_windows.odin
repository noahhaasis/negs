#+build windows
// The file operations the library needs, as Rust's std::fs does them on
// Windows, with its error words ("The system cannot find the file
// specified. (os error 2)"). Type-checked on the Mac
// (`odin check core/src/abi -target:windows_amd64`), not yet run on
// Windows: its differential run belongs to the Windows phase.
package library

import "core:strings"
import win "core:sys/windows"

// A Windows error code; 0 is none.
Io_Error :: distinct i32

io_not_found :: proc(e: Io_Error) -> bool {
	return e == Io_Error(win.ERROR_FILE_NOT_FOUND) || e == Io_Error(win.ERROR_PATH_NOT_FOUND)
}

// What Rust's io::Error says for it: FormatMessageW's text, trimmed.
io_error_text :: proc(e: Io_Error, allocator := context.allocator) -> string {
	buf: [512]win.WCHAR
	n := win.FormatMessageW(
		win.FORMAT_MESSAGE_FROM_SYSTEM | win.FORMAT_MESSAGE_IGNORE_INSERTS,
		nil,
		win.DWORD(e),
		0,
		raw_data(buf[:]),
		len(buf),
		nil,
	)
	text := "Unknown error"
	if n > 0 {
		if s, err := win.wstring_to_utf8(win.wstring(raw_data(buf[:])), int(n), context.temp_allocator); err == nil {
			text = strings.trim_right(s, " \r\n")
		}
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
	return Io_Error(win.GetLastError())
}

@(private = "file")
w_path :: proc(path: string) -> win.wstring {
	return win.utf8_to_wstring(path, context.temp_allocator)
}

// A file's size and modification time (nanoseconds since 1970).
File_Stat :: struct {
	regular:  bool,
	size:     i64,
	modified: i64,
}

// FILETIME counts 100 ns since 1601.
@(private = "file")
unix_nanoseconds :: proc(t: win.FILETIME) -> i64 {
	ticks := i64(t.dwHighDateTime) << 32 | i64(t.dwLowDateTime)
	return (ticks - 116_444_736_000_000_000) * 100
}

@(private = "file")
from_attributes :: proc(attributes, size_high, size_low: win.DWORD, written: win.FILETIME) -> File_Stat {
	return File_Stat {
		regular = attributes & (win.FILE_ATTRIBUTE_DIRECTORY | win.FILE_ATTRIBUTE_REPARSE_POINT) == 0,
		size = i64(size_high) << 32 | i64(size_low),
		modified = unix_nanoseconds(written),
	}
}

// `std::fs::metadata`. (Rust follows a symbolic link here; the attributes
// are the link's own.)
fs_stat :: proc(path: string) -> (File_Stat, Io_Error) {
	data: win.WIN32_FILE_ATTRIBUTE_DATA
	if !win.GetFileAttributesExW(w_path(path), win.GetFileExInfoStandard, &data) {
		return {}, last_error()
	}
	return from_attributes(data.dwFileAttributes, data.nFileSizeHigh, data.nFileSizeLow, data.ftLastWriteTime), 0
}

fs_exists :: proc(path: string) -> bool {
	_, err := fs_stat(path)
	return err == 0
}

// Whether `folder` isn't read-only: `!metadata.permissions().readonly()`,
// which on Windows is the read-only attribute.
fs_writable_dir :: proc(folder: string) -> bool {
	data: win.WIN32_FILE_ATTRIBUTE_DATA
	if !win.GetFileAttributesExW(w_path(folder), win.GetFileExInfoStandard, &data) {
		return false
	}
	return data.dwFileAttributes & win.FILE_ATTRIBUTE_READONLY == 0
}

Dir_Entry :: struct {
	name:   string,
	// False when it couldn't be read.
	ok:     bool,
	hidden: bool,
	stat:   File_Stat,
}

// The entries of `folder` in the order the OS gives them (`fs::read_dir`,
// FindFirstFileW), with the attributes the search returns.
fs_read_dir :: proc(folder: string, allocator := context.temp_allocator) -> ([]Dir_Entry, Io_Error) {
	pattern := strings.concatenate({strings.trim_right(folder, "\\/"), "\\*"}, context.temp_allocator)
	found: win.WIN32_FIND_DATAW
	handle := win.FindFirstFileW(w_path(pattern), &found)
	if handle == win.INVALID_HANDLE_VALUE {
		err := last_error()
		if err == Io_Error(win.ERROR_FILE_NOT_FOUND) {
			return nil, 0
		}
		return nil, err
	}
	defer win.FindClose(handle)
	out := make([dynamic]Dir_Entry, 0, 64, allocator)
	for {
		name, err := win.wstring_to_utf8(win.wstring(raw_data(found.cFileName[:])), -1, allocator)
		if err == nil && name != "." && name != ".." {
			append(
				&out,
				Dir_Entry {
					name = name,
					ok = true,
					hidden = len(name) > 0 && name[0] == '.',
					stat = from_attributes(found.dwFileAttributes, found.nFileSizeHigh, found.nFileSizeLow, found.ftLastWriteTime),
				},
			)
		}
		if !win.FindNextFileW(handle, &found) {
			break
		}
	}
	return out[:], 0
}

// A file's bytes, at most `limit` (`File::open` and `take(limit)`).
fs_read :: proc(path: string, limit: int, allocator := context.temp_allocator) -> ([]u8, Io_Error) {
	handle := win.CreateFileW(
		w_path(path),
		win.GENERIC_READ,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		win.FILE_ATTRIBUTE_NORMAL,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		return nil, last_error()
	}
	defer win.CloseHandle(handle)
	size := 4096
	file_size: win.LARGE_INTEGER
	if win.GetFileSizeEx(handle, &file_size) {
		size = int(file_size) + 1
	}
	out := make([dynamic]u8, 0, max(min(size, limit), 1), allocator)
	for len(out) < limit {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		room := min(cap(out) - len(out), limit - len(out), int(max(i32)))
		start := len(out)
		resize(&out, start + room)
		read: win.DWORD
		if !win.ReadFile(handle, raw_data(out[start:]), win.DWORD(room), &read, nil) {
			err := last_error()
			delete(out)
			return nil, err
		}
		resize(&out, start + int(read))
		if read == 0 {
			break
		}
	}
	return out[:], 0
}

// `fs::write`: created or truncated.
fs_write :: proc(path: string, data: []u8) -> Io_Error {
	handle := win.CreateFileW(
		w_path(path),
		win.GENERIC_WRITE,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.CREATE_ALWAYS,
		win.FILE_ATTRIBUTE_NORMAL,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		return last_error()
	}
	rest := data
	for len(rest) > 0 {
		written: win.DWORD
		chunk := min(len(rest), int(max(i32)))
		if !win.WriteFile(handle, raw_data(rest), win.DWORD(chunk), &written, nil) {
			err := last_error()
			win.CloseHandle(handle)
			return err
		}
		rest = rest[written:]
	}
	win.CloseHandle(handle)
	return 0
}

// `fs::rename`: replaces `to`, as Rust's MoveFileExW does.
fs_rename :: proc(from, to: string) -> Io_Error {
	if !win.MoveFileExW(w_path(from), w_path(to), win.MOVEFILE_REPLACE_EXISTING) {
		return last_error()
	}
	return 0
}

fs_remove :: proc(path: string) -> Io_Error {
	if !win.DeleteFileW(w_path(path)) {
		return last_error()
	}
	return 0
}
