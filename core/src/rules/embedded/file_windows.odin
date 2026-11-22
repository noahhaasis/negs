#+build windows
// Reading a file a piece at a time, as Rust's File::open, metadata().len()
// and seek + read_exact do on Windows.
package embedded

import "core:strings"
import win "core:sys/windows"

File :: win.HANDLE

// The file open for reading and its size; false when it can't be opened
// (a path with a NUL in it can't be, as in Rust).
file_open :: proc(path: string) -> (File, u64, bool) {
	if strings.index_byte(path, 0) >= 0 {
		return nil, 0, false
	}
	wide := win.utf8_to_wstring(path, context.allocator)
	defer free(rawptr(wide))
	handle := win.CreateFileW(
		wide,
		win.GENERIC_READ,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		win.FILE_ATTRIBUTE_NORMAL,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		return nil, 0, false
	}
	size: win.LARGE_INTEGER
	if !win.GetFileSizeEx(handle, &size) || size < 0 {
		win.CloseHandle(handle)
		return nil, 0, false
	}
	return handle, u64(size), true
}

file_close :: proc(f: File) {
	win.CloseHandle(f)
}

// All of `out` from `offset`: false at an error or the file's end.
file_read_exact :: proc(f: File, offset: u64, out: []u8) -> bool {
	if !win.SetFilePointerEx(f, win.LARGE_INTEGER(offset), nil, win.FILE_BEGIN) {
		return false
	}
	done := 0
	for done < len(out) {
		chunk := min(len(out) - done, int(max(i32)))
		read: win.DWORD
		if !win.ReadFile(f, raw_data(out[done:]), win.DWORD(chunk), &read, nil) {
			return false
		}
		if read == 0 {
			return false
		}
		done += int(read)
	}
	return true
}
