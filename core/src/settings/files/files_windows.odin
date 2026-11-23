#+build windows
// The file operations on Windows, as presets.rs and session.rs make them:
// Rust's std::fs (CreateFileW, GetFileInformationByHandle, FindFirstFileW,
// DeleteFileW, CreateDirectoryW) and the `windows` crate's calls
// (MoveFileExW with and without MOVEFILE_REPLACE_EXISTING,
// SetFileAttributesW(FILE_ATTRIBUTE_HIDDEN)). A sharing or lock violation
// is reported as it is (ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION):
// the Rust doesn't retry, and neither does this. Type-checked on the Mac;
// it runs at the Windows gate.
package files

import "core:strings"
import win "core:sys/windows"
import "../paths"

@(private)
w :: proc(path: string) -> win.wstring {
	return win.utf8_to_wstring(path, context.temp_allocator)
}

@(private)
last :: proc "contextless" () -> Code {
	return Code(win.GetLastError())
}

// std's `io::ErrorKind::NotFound` (decode_error_kind).
is_not_found :: proc "contextless" (code: Code) -> bool {
	switch code {
	case Code(win.ERROR_FILE_NOT_FOUND), Code(win.ERROR_PATH_NOT_FOUND), Code(win.ERROR_INVALID_DRIVE), Code(53), Code(67) /* ERROR_BAD_NETPATH, ERROR_BAD_NET_NAME */:
		return true
	}
	return false
}

// FormatMessageW's text for `code` with trailing white space trimmed.
@(private)
system_text :: proc(code: u32, allocator := context.temp_allocator) -> string {
	buf: [2048]win.WCHAR
	n := win.FormatMessageW(
		win.FORMAT_MESSAGE_FROM_SYSTEM | win.FORMAT_MESSAGE_IGNORE_INSERTS,
		nil,
		code,
		0,
		raw_data(buf[:]),
		len(buf),
		nil,
	)
	if n == 0 {
		return ""
	}
	for n > 0 && (buf[n - 1] == ' ' || (buf[n - 1] >= 9 && buf[n - 1] <= 13)) {
		n -= 1
	}
	s, err := win.wstring_to_utf8(win.wstring(raw_data(buf[:])), int(n), allocator)
	return s if err == nil else ""
}

// What Rust's io::Error says of a Win32 error.
@(private)
os_error_text :: proc(code: Code, allocator := context.temp_allocator) -> string {
	if code == CODE_WHOLE_TREE {
		return strings.clone("failed to create whole tree", allocator)
	}
	text := system_text(u32(code), context.temp_allocator)
	b := strings.builder_make(0, len(text) + 40, allocator)
	if text == "" {
		strings.write_string(&b, "OS Error ")
		strings.write_int(&b, int(code))
		strings.write_string(&b, " (FormatMessageW() returned error)")
	} else {
		strings.write_string(&b, text)
	}
	strings.write_string(&b, " (os error ")
	strings.write_int(&b, int(code))
	strings.write_byte(&b, ')')
	return strings.to_string(b)
}

// The windows crate's `Error::message()` for a Win32 error: FormatMessageW
// of its HRESULT, trailing white space trimmed (windows-result 0.2).
@(private)
win32_message :: proc(code: Code, allocator := context.temp_allocator) -> string {
	hresult := u32(code) & 0xFFFF | 0x8007_0000 if code != 0 else 0
	return system_text(hresult, allocator)
}

// FILETIME (100 ns since 1601) as Rust's SystemTime since 1970.
@(private)
unix_time :: proc "contextless" (t: win.FILETIME) -> (seconds: i64, nanos: u32) {
	ticks := i64(t.dwHighDateTime) << 32 | i64(t.dwLowDateTime)
	since := ticks - 116_444_736_000_000_000
	seconds = since / 10_000_000
	rest := since % 10_000_000
	if rest < 0 {
		seconds -= 1
		rest += 10_000_000
	}
	return seconds, u32(rest * 100)
}

@(private)
IO_REPARSE_TAG_NAME_SURROGATE :: 0x2000_0000

@(private)
from_info :: proc "contextless" (attributes: u32, size_high, size_low: u32, written: win.FILETIME, tag: u32) -> Stat {
	reparse := attributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0
	symlink := reparse && tag & IO_REPARSE_TAG_NAME_SURROGATE != 0
	dir := attributes & win.FILE_ATTRIBUTE_DIRECTORY != 0
	s, n := unix_time(written)
	return Stat {
		is_file = !dir && !symlink,
		is_dir = dir && !symlink,
		hidden = attributes & win.FILE_ATTRIBUTE_HIDDEN != 0,
		size = u64(size_high) << 32 | u64(size_low),
		seconds = s,
		nanos = n,
	}
}

// `std::fs::metadata`: the file opened without access (links followed),
// GetFileInformationByHandle; on a sharing violation or access denied,
// FindFirstFileW's view of it unless that is a link.
stat :: proc(path: string) -> (Stat, Code) {
	wpath := w(path)
	handle := win.CreateFileW(
		wpath,
		0,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		win.FILE_FLAG_BACKUP_SEMANTICS,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		err := last()
		if err != Code(win.ERROR_SHARING_VIOLATION) && err != Code(win.ERROR_ACCESS_DENIED) {
			return {}, err
		}
		found: win.WIN32_FIND_DATAW
		find := win.FindFirstFileW(wpath, &found)
		if find == win.INVALID_HANDLE_VALUE {
			return {}, err
		}
		win.FindClose(find)
		st := from_info(found.dwFileAttributes, found.nFileSizeHigh, found.nFileSizeLow, found.ftLastWriteTime, found.dwReserved0)
		if found.dwFileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 && found.dwReserved0 & IO_REPARSE_TAG_NAME_SURROGATE != 0 {
			return {}, err
		}
		return st, 0
	}
	defer win.CloseHandle(handle)
	info: win.BY_HANDLE_FILE_INFORMATION
	if !win.GetFileInformationByHandle(handle, &info) {
		return {}, last()
	}
	tag: u32
	if info.dwFileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		tag_info: win.FILE_ATTRIBUTE_TAG_INFO
		if win.GetFileInformationByHandleEx(handle, .FileAttributeTagInfo, &tag_info, size_of(tag_info)) {
			tag = tag_info.ReparseTag
		}
	}
	return from_info(info.dwFileAttributes, info.nFileSizeHigh, info.nFileSizeLow, info.ftLastWriteTime, tag), 0
}

// `std::fs::read`.
read_file :: proc(path: string, allocator := context.temp_allocator) -> ([]u8, Code) {
	handle := win.CreateFileW(
		w(path),
		win.GENERIC_READ,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		0,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		return nil, last()
	}
	defer win.CloseHandle(handle)
	size := 4096
	file_size: win.LARGE_INTEGER
	if win.GetFileSizeEx(handle, &file_size) {
		size = int(file_size) + 1
	}
	out := make([dynamic]u8, 0, max(size, 1), allocator)
	for {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		room: int = min(cap(out) - start, int(max(i32)))
		resize(&out, start + room)
		got: win.DWORD
		if !win.ReadFile(handle, raw_data(out[start:]), win.DWORD(room), &got, nil) {
			err := last()
			if err == Code(win.ERROR_BROKEN_PIPE) {
				resize(&out, start)
				break
			}
			delete(out)
			return nil, err
		}
		resize(&out, start + int(got))
		if got == 0 {
			break
		}
	}
	return out[:], 0
}

@(private)
write_all :: proc(handle: win.HANDLE, data: []u8) -> Code {
	rest := data
	for len(rest) > 0 {
		written: win.DWORD
		chunk := min(len(rest), int(max(u32)))
		if !win.WriteFile(handle, raw_data(rest), win.DWORD(chunk), &written, nil) {
			return last()
		}
		rest = rest[written:]
	}
	return 0
}

@(private)
create :: proc(path: string, disposition: win.DWORD) -> (win.HANDLE, Code) {
	handle := win.CreateFileW(
		w(path),
		win.GENERIC_WRITE,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		disposition,
		0,
		nil,
	)
	if handle == win.INVALID_HANDLE_VALUE {
		return nil, last()
	}
	return handle, 0
}

// `std::fs::write` (File::create: CREATE_ALWAYS... Rust opens with
// OPEN_ALWAYS and truncates; the result is the same file).
write_file :: proc(path: string, data: []u8) -> Code {
	handle, err := create(path, win.CREATE_ALWAYS)
	if err != 0 {
		return err
	}
	defer win.CloseHandle(handle)
	return write_all(handle, data)
}

// OpenOptions::new().write(true).create_new(true) (CREATE_NEW), write_all,
// sync_all (FlushFileBuffers).
write_new_synced :: proc(path: string, data: []u8) -> Code {
	handle, err := create(path, win.CREATE_NEW)
	if err != 0 {
		return err
	}
	defer win.CloseHandle(handle)
	if e := write_all(handle, data); e != 0 {
		return e
	}
	if !win.FlushFileBuffers(handle) {
		return last()
	}
	return 0
}

// `std::fs::remove_file`: DeleteFileW.
remove_file :: proc(path: string) -> Code {
	if !win.DeleteFileW(w(path)) {
		return last()
	}
	return 0
}

@(private)
create_dir :: proc(path: string) -> Code {
	if !win.CreateDirectoryW(w(path), nil) {
		return last()
	}
	return 0
}

@(private)
parent_of :: proc(path: string) -> (string, bool) {
	return paths.parent(path, .Windows)
}

// A UTF-16 name as UTF-8; false when it holds an unpaired surrogate (Rust's
// `to_str()` refuses it, so the Rust skips the entry).
@(private)
utf8_name :: proc(name: []u16, allocator := context.temp_allocator) -> (string, bool) {
	for i := 0; i < len(name); i += 1 {
		u := name[i]
		if u >= 0xD800 && u < 0xDC00 {
			if i + 1 >= len(name) || name[i + 1] < 0xDC00 || name[i + 1] >= 0xE000 {
				return "", false
			}
			i += 1
		} else if u >= 0xDC00 && u < 0xE000 {
			return "", false
		}
	}
	s, err := win.utf16_to_utf8(name, allocator)
	return s, err == nil
}

// The names in `folder` but `.` and `..` (`std::fs::read_dir`:
// FindFirstFileW on `folder\*`; a folder with nothing matching is empty).
// Names that aren't valid UTF-16 are left out.
read_dir :: proc(folder: string, allocator := context.temp_allocator) -> ([]string, Code) {
	if folder == "" {
		return nil, Code(win.ERROR_PATH_NOT_FOUND)
	}
	pattern := paths.join(folder, "*", .Windows, context.temp_allocator)
	found: win.WIN32_FIND_DATAW
	handle := win.FindFirstFileW(w(pattern), &found)
	if handle == win.INVALID_HANDLE_VALUE {
		err := last()
		if err == Code(win.ERROR_FILE_NOT_FOUND) {
			return nil, 0
		}
		return nil, err
	}
	defer win.FindClose(handle)
	out := make([dynamic]string, 0, 64, allocator)
	for {
		n := 0
		for n < len(found.cFileName) && found.cFileName[n] != 0 {
			n += 1
		}
		raw := found.cFileName[:n]
		dot := n == 1 && raw[0] == '.'
		dotdot := n == 2 && raw[0] == '.' && raw[1] == '.'
		if !dot && !dotdot {
			if name, ok := utf8_name(raw, allocator); ok {
				append(&out, name)
			}
		}
		if !win.FindNextFileW(handle, &found) {
			break
		}
	}
	return out[:], 0
}

// MARK: The windows crate's calls

// MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
// (`platform::replace_file`). The error and `Error::message()`.
replace_file :: proc(from, to: string, allocator := context.temp_allocator) -> (Code, string) {
	if win.MoveFileExW(w(from), w(to), win.MOVEFILE_REPLACE_EXISTING | win.MOVEFILE_WRITE_THROUGH) {
		return 0, ""
	}
	e := last()
	return e, win32_message(e, allocator)
}

// MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH): fails if `to` exists.
move_no_replace :: proc(from, to: string, allocator := context.temp_allocator) -> (Code, string) {
	if win.MoveFileExW(w(from), w(to), win.MOVEFILE_WRITE_THROUGH) {
		return 0, ""
	}
	e := last()
	return e, win32_message(e, allocator)
}

// SetFileAttributesW(path, FILE_ATTRIBUTE_HIDDEN), errors ignored.
hide :: proc(path: string) {
	_ = win.SetFileAttributesW(w(path), win.FILE_ATTRIBUTE_HIDDEN)
}

// MARK: The process

// `std::env::var_os`: GetEnvironmentVariableW; unset is (_, false), set
// and empty ("", true).
env :: proc(name: string, allocator := context.temp_allocator) -> (string, bool) {
	wname := w(name)
	buf := make([]u16, 512, context.temp_allocator)
	for {
		win.SetLastError(0)
		n := win.GetEnvironmentVariableW(wname, raw_data(buf), win.DWORD(len(buf)))
		if n == 0 {
			if win.GetLastError() != 0 {
				return "", false
			}
			return strings.clone("", allocator), true
		}
		if int(n) < len(buf) {
			s, err := win.utf16_to_utf8(buf[:n], allocator)
			return s, err == nil
		}
		buf = make([]u16, int(n) + 1, context.temp_allocator)
	}
}

// `std::env::temp_dir` (GetTempPathW: it ends in a backslash).
temp_dir :: proc(allocator := context.temp_allocator) -> string {
	buf: [win.MAX_PATH + 1]u16
	n := win.GetTempPathW(len(buf), win.LPCWSTR(raw_data(buf[:])))
	if n == 0 || int(n) > len(buf) {
		return strings.clone("", allocator)
	}
	s, _ := win.utf16_to_utf8(buf[:n], allocator)
	return s
}

process_id :: proc "contextless" () -> u32 {
	return u32(win.GetCurrentProcessId())
}

// SystemTime::now() since 1970, in nanoseconds.
now_nanos :: proc "contextless" () -> i128 {
	t: win.FILETIME
	win.GetSystemTimePreciseAsFileTime(&t)
	ticks := i64(t.dwHighDateTime) << 32 | i64(t.dwLowDateTime)
	return i128(ticks - 116_444_736_000_000_000) * 100
}
