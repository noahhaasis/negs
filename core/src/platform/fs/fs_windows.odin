#+build windows
// The file system on Windows, as Rust's std::fs makes its Win32 calls
// (sys/fs/windows.rs, Rust 1.93), and the `windows` crate's calls the app
// makes itself (MoveFileExW with and without MOVEFILE_REPLACE_EXISTING,
// SetFileAttributesW(FILE_ATTRIBUTE_HIDDEN)). Every path goes through
// std's `maybe_verbatim` (verbatim.odin), so one over MAX_PATH gets its
// `\\?\` prefix as in std. A sharing or lock violation is reported as it
// is: the Rust doesn't retry, and neither does this.
//
// Type-checked on the Mac (`odin check core/src/abi -target:windows_amd64`);
// it runs at the Windows gate.
package platform_fs

import "base:runtime"
import "core:strings"
import win "core:sys/windows"

foreign import kernel32 "system:Kernel32.lib"

@(default_calling_convention = "system")
@(private)
foreign kernel32 {
	FindFirstFileExW :: proc(lpFileName: win.LPCWSTR, fInfoLevelId: i32, lpFindFileData: rawptr, fSearchOp: i32, lpSearchFilter: rawptr, dwAdditionalFlags: win.DWORD) -> win.HANDLE ---
}

@(private)
NUL_TEXT :: "strings passed to WinAPI cannot contain NULs"

@(private)
FIND_EX_INFO_BASIC :: 1
@(private)
FIND_EX_SEARCH_NAME_MATCH :: 0
@(private)
IO_REPARSE_TAG_NAME_SURROGATE :: 0x2000_0000
@(private)
ERROR_CANT_ACCESS_FILE :: 1920
@(private)
ERROR_DIR_NOT_EMPTY :: 145
@(private)
FACILITY_NT_BIT :: 0x1000_0000
@(private)
FILE_DISPOSITION_INFO_EX_CLASS :: win.FILE_INFO_BY_HANDLE_CLASS(21)
@(private)
FILE_RENAME_INFO_EX_CLASS :: win.FILE_INFO_BY_HANDLE_CLASS(22)
@(private)
SHARE_ALL :: win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE

@(private)
last :: proc "contextless" () -> Error {
	return Error(win.GetLastError())
}

// GetFullPathNameW through std's fill_utf16_buf: a 512-unit buffer first,
// then the size the call asks for.
@(private)
full_path_name :: proc(path: []u16, allocator: runtime.Allocator) -> ([]u16, Error) {
	n := 512
	for {
		buf := make([]u16, n, allocator)
		win.SetLastError(0)
		k := int(win.GetFullPathNameW(win.wstring(raw_data(path)), win.DWORD(n), win.wstring(raw_data(buf)), nil))
		if k == 0 {
			if e := last(); e != 0 {
				return nil, e
			}
			return buf[:0], 0
		}
		if k == n && win.GetLastError() == win.ERROR_INSUFFICIENT_BUFFER {
			n *= 2
		} else if k > n {
			n = k
		} else {
			return buf[:k], 0
		}
	}
}

// A path as std hands it to Win32 (`maybe_verbatim`), in the temporary
// allocator.
@(private)
native :: proc(path: string) -> (win.wstring, Error) {
	units, err := maybe_verbatim(path, full_path_name, context.temp_allocator)
	if err != 0 {
		return nil, err
	}
	return win.wstring(raw_data(units)), 0
}

// The units of a NUL-terminated path before its NUL.
@(private)
wide_len :: proc(path: win.wstring) -> int {
	n := 0
	for ([^]u16)(path)[n] != 0 {
		n += 1
	}
	return n
}

// std's `io::ErrorKind::NotFound` (decode_error_kind).
is_not_found :: proc "contextless" (e: Error) -> bool {
	switch e {
	case Error(win.ERROR_FILE_NOT_FOUND), Error(win.ERROR_PATH_NOT_FOUND), Error(win.ERROR_INVALID_DRIVE), Error(53), Error(67) /* ERROR_BAD_NETPATH, ERROR_BAD_NET_NAME */:
		return true
	}
	return false
}

// char::is_whitespace, for str::trim_end.
@(private)
white_space :: proc "contextless" (u: u16) -> bool {
	switch u {
	case 9 ..= 13, ' ', 0x85, 0xA0, 0x1680, 0x2000 ..= 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return false
}

// sys::os::error_string: FormatMessageW's text for the code (from ntdll
// for an NTSTATUS one), trailing white space trimmed; std's own words when
// FormatMessageW fails or gives invalid UTF-16.
@(private)
error_string :: proc(e: Error, allocator := context.allocator) -> string {
	buf: [2048]u16
	code := u32(e)
	module: win.HMODULE
	flags: u32
	if code & FACILITY_NT_BIT != 0 {
		ntdll := win.utf8_to_wstring("NTDLL.DLL", context.temp_allocator)
		module = win.GetModuleHandleW(ntdll)
		if module != nil {
			code ~= FACILITY_NT_BIT
			flags = win.FORMAT_MESSAGE_FROM_HMODULE
		}
	}
	n := int(win.FormatMessageW(flags | win.FORMAT_MESSAGE_FROM_SYSTEM | win.FORMAT_MESSAGE_IGNORE_INSERTS, module, code, 0, raw_data(buf[:]), len(buf), nil))
	b := strings.builder_make(0, 64, allocator)
	if n == 0 {
		fm := win.GetLastError()
		strings.write_string(&b, "OS Error ")
		strings.write_int(&b, int(i32(code)))
		strings.write_string(&b, " (FormatMessageW() returned error ")
		strings.write_int(&b, int(i32(fm)))
		strings.write_byte(&b, ')')
		return strings.to_string(b)
	}
	text, unicode := from_utf16(buf[:n], context.temp_allocator)
	if !unicode {
		strings.write_string(&b, "OS Error ")
		strings.write_int(&b, int(i32(code)))
		strings.write_string(&b, " (FormatMessageW() returned invalid UTF-16)")
		return strings.to_string(b)
	}
	// Trimmed as UTF-16 (the same code points as str::trim_end).
	for n > 0 && white_space(buf[n - 1]) {
		n -= 1
	}
	text, _ = from_utf16(buf[:n], context.temp_allocator)
	strings.write_string(&b, text)
	return strings.to_string(b)
}

// FormatMessageW's text for `code`, trailing ASCII white space trimmed (the
// windows crate's message()).
@(private)
system_text :: proc(code: u32, allocator := context.allocator) -> string {
	buf: [2048]u16
	n := win.FormatMessageW(win.FORMAT_MESSAGE_FROM_SYSTEM | win.FORMAT_MESSAGE_IGNORE_INSERTS, nil, code, 0, raw_data(buf[:]), len(buf), nil)
	if n == 0 {
		return ""
	}
	for n > 0 && (buf[n - 1] == ' ' || (buf[n - 1] >= 9 && buf[n - 1] <= 13)) {
		n -= 1
	}
	s, _ := from_utf16(buf[:n], allocator)
	return display(s, .Windows, allocator)
}

// The windows crate's `Error::message()` for a Win32 error: FormatMessageW
// of its HRESULT, trailing white space trimmed (windows-result 0.2).
win32_error_message :: proc(e: Error, allocator := context.allocator) -> string {
	hresult := u32(e) & 0xFFFF | 0x8007_0000 if e > 0 else 0
	return system_text(hresult, allocator)
}

// MARK: Metadata

// FILETIME (100 ns since 1601) as Rust's SystemTime since 1970.
@(private)
system_time :: proc "contextless" (t: win.FILETIME) -> System_Time {
	ticks := i64(u64(t.dwHighDateTime) << 32 | u64(t.dwLowDateTime))
	since := ticks - 116_444_736_000_000_000
	secs := since / 10_000_000
	rest := since % 10_000_000
	if rest < 0 {
		secs -= 1
		rest += 10_000_000
	}
	return System_Time{secs, u32(rest * 100)}
}

// FileType::new: a name-surrogate reparse point is a link, neither file
// nor folder.
@(private)
kind_of :: proc "contextless" (attributes, reparse_tag: u32) -> Kind {
	if attributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 && reparse_tag & IO_REPARSE_TAG_NAME_SURROGATE != 0 {
		return .Other
	}
	return .Dir if attributes & win.FILE_ATTRIBUTE_DIRECTORY != 0 else .File
}

@(private)
from_attributes :: proc "contextless" (attributes, size_high, size_low: u32, written, created: win.FILETIME, reparse_tag: u32) -> Stat {
	return Stat {
		kind = kind_of(attributes, reparse_tag),
		size = u64(size_high) << 32 | u64(size_low),
		readonly = attributes & win.FILE_ATTRIBUTE_READONLY != 0,
		hidden = attributes & win.FILE_ATTRIBUTE_HIDDEN != 0,
		modified = system_time(written),
		created = system_time(created),
	}
}

// FileAttr from a WIN32_FIND_DATAW: the reparse tag only for a reparse
// point.
@(private)
from_find :: proc "contextless" (found: ^win.WIN32_FIND_DATAW) -> Stat {
	tag := found.dwReserved0 if found.dwFileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 else 0
	return from_attributes(found.dwFileAttributes, found.nFileSizeHigh, found.nFileSizeLow, found.ftLastWriteTime, found.ftCreationTime, tag)
}

// File::file_attr: GetFileInformationByHandle, and the reparse tag of a
// reparse point.
@(private)
file_attr :: proc(handle: win.HANDLE) -> (Stat, Error) {
	info: win.BY_HANDLE_FILE_INFORMATION
	if !win.GetFileInformationByHandle(handle, &info) {
		return {}, last()
	}
	tag: u32
	if info.dwFileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		tag_info: win.FILE_ATTRIBUTE_TAG_INFO
		if !win.GetFileInformationByHandleEx(handle, .FileAttributeTagInfo, &tag_info, size_of(tag_info)) {
			return {}, last()
		}
		if tag_info.FileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			tag = tag_info.ReparseTag
		}
	}
	return from_attributes(info.dwFileAttributes, info.nFileSizeHigh, info.nFileSizeLow, info.ftLastWriteTime, info.ftCreationTime, tag), 0
}

// sys::fs::metadata: the file opened with no access (links followed
// unless `open_reparse`); on a sharing violation or a denied access,
// FindFirstFileExW's view of it, unless that is a link being followed.
@(private)
metadata :: proc(path: win.wstring, open_reparse: bool) -> (Stat, Error) {
	flags := win.FILE_FLAG_BACKUP_SEMANTICS | (win.FILE_FLAG_OPEN_REPARSE_POINT if open_reparse else 0)
	handle := win.CreateFileW(path, 0, SHARE_ALL, nil, win.OPEN_EXISTING, flags, nil)
	if handle != win.INVALID_HANDLE_VALUE {
		defer win.CloseHandle(handle)
		return file_attr(handle)
	}
	err := last()
	if err != Error(win.ERROR_SHARING_VIOLATION) && err != Error(win.ERROR_ACCESS_DENIED) {
		return {}, err
	}
	found: win.WIN32_FIND_DATAW
	find := FindFirstFileExW(path, FIND_EX_INFO_BASIC, &found, FIND_EX_SEARCH_NAME_MATCH, nil, 0)
	if find == win.INVALID_HANDLE_VALUE {
		return {}, err
	}
	win.FindClose(find)
	st := from_find(&found)
	if !open_reparse && st.kind == .Other {
		return {}, err
	}
	return st, 0
}

// `std::fs::metadata` (sys::fs::stat): links followed; a reparse point
// CreateFileW can't handle (ERROR_CANT_ACCESS_FILE) is described by
// lstat, unless it is a link.
stat :: proc(path: string) -> (Stat, Error) {
	w, e := native(path)
	if e != 0 {
		return {}, e
	}
	st, err := metadata(w, false)
	if err == ERROR_CANT_ACCESS_FILE {
		if own, lerr := metadata(w, true); lerr == 0 && own.kind != .Other {
			return own, 0
		}
	}
	return st, err
}

// `std::fs::read_dir` (FindFirstFileExW on `path\*`, made verbatim; an
// empty path is ERROR_PATH_NOT_FOUND; no match at all is an empty folder)
// with each entry's `file_type()` and `metadata()` from the search, "."
// and ".." left out. `stopped` is FindNextFileW's error other than
// ERROR_NO_MORE_FILES, which ends the listing. (`metadata` is always
// there: the search gives it.)
read_dir :: proc(path: string, metadata := false, allocator := context.allocator) -> (entries: []Dir_Entry, err: Error, stopped: Error) {
	if path == "" {
		return nil, Error(win.ERROR_PATH_NOT_FOUND), 0
	}
	pattern := join(path, "*", .Windows, context.temp_allocator)
	w, e := native(pattern)
	if e != 0 {
		return nil, e, 0
	}
	found: win.WIN32_FIND_DATAW
	handle := FindFirstFileExW(w, FIND_EX_INFO_BASIC, &found, FIND_EX_SEARCH_NAME_MATCH, nil, 0)
	if handle == win.INVALID_HANDLE_VALUE {
		e = last()
		if e == Error(win.ERROR_FILE_NOT_FOUND) {
			return make([]Dir_Entry, 0, allocator), 0, 0
		}
		return nil, e, 0
	}
	defer win.FindClose(handle)
	out := make([dynamic]Dir_Entry, 0, 16, allocator)
	for {
		n := 0
		for n < len(found.cFileName) && found.cFileName[n] != 0 {
			n += 1
		}
		raw := found.cFileName[:n]
		dot := n == 1 && raw[0] == '.'
		dotdot := n == 2 && raw[0] == '.' && raw[1] == '.'
		if !dot && !dotdot {
			name, unicode := from_utf16(raw, allocator)
			st := from_find(&found)
			append(&out, Dir_Entry{name = name, unicode = unicode, kind = st.kind, kind_ok = true, stat = st, stat_ok = true})
		}
		if !win.FindNextFileW(handle, &found) {
			if e = last(); e != Error(win.ERROR_NO_MORE_FILES) {
				stopped = e
			}
			break
		}
	}
	return out[:], 0, stopped
}

// MARK: Files

// Handle::read: ERROR_BROKEN_PIPE is the end.
@(private)
read_some :: proc(handle: win.HANDLE, buf: []u8) -> (int, Error) {
	got: win.DWORD
	if !win.ReadFile(handle, raw_data(buf), win.DWORD(min(len(buf), int(max(u32)))), &got, nil) {
		e := last()
		if e == Error(win.ERROR_BROKEN_PIPE) {
			return 0, 0
		}
		return 0, e
	}
	return int(got), 0
}

// `File::open` and `read_to_end`, at most `limit` bytes (`take(limit)`):
// `std::fs::read` with no limit.
read :: proc(path: string, limit := max(int), allocator := context.allocator) -> ([]u8, Error) {
	w, e := native(path)
	if e != 0 {
		return nil, e
	}
	handle := win.CreateFileW(w, win.GENERIC_READ, SHARE_ALL, nil, win.OPEN_EXISTING, 0, nil)
	if handle == win.INVALID_HANDLE_VALUE {
		return nil, last()
	}
	defer win.CloseHandle(handle)
	size := 0
	file_size: win.LARGE_INTEGER
	if win.GetFileSizeEx(handle, &file_size) && file_size > 0 {
		size = int(file_size)
	}
	out := make([dynamic]u8, 0, max(min(size, limit - 1) + 1, 1), allocator)
	for len(out) < limit {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		room := min(cap(out) - start, limit - start, int(max(i32)))
		resize(&out, start + room)
		got, err := read_some(handle, out[start:])
		if err != 0 {
			delete(out)
			return nil, err
		}
		resize(&out, start + got)
		if got == 0 {
			break
		}
	}
	return out[:], 0
}

@(private)
write_all :: proc(handle: win.HANDLE, data: []u8) -> Error {
	rest := data
	for len(rest) > 0 {
		written: win.DWORD
		if !win.WriteFile(handle, raw_data(rest), win.DWORD(min(len(rest), int(max(u32)))), &written, nil) {
			return last()
		}
		rest = rest[written:]
	}
	return 0
}

// `File::create` and `write_all` of each part in turn (`std::fs::write`
// for one). std opens with OPEN_ALWAYS and truncates a file that was there
// (CREATE_ALWAYS fails on a hidden or system file), so this does too.
write :: proc(path: string, parts: ..[]u8) -> Error {
	w, e := native(path)
	if e != 0 {
		return e
	}
	handle := win.CreateFileW(w, win.GENERIC_WRITE, SHARE_ALL, nil, win.OPEN_ALWAYS, 0, nil)
	if handle == win.INVALID_HANDLE_VALUE {
		return last()
	}
	defer win.CloseHandle(handle)
	if win.GetLastError() == win.ERROR_ALREADY_EXISTS {
		// FileAllocationInfo first, FileEndOfFileInfo for WINE.
		size: win.LARGE_INTEGER
		if !win.SetFileInformationByHandle(handle, .FileAllocationInfo, &size, size_of(size)) {
			if !win.SetFileInformationByHandle(handle, .FileEndOfFileInfo, &size, size_of(size)) {
				return last()
			}
		}
	}
	for part in parts {
		if err := write_all(handle, part); err != 0 {
			return err
		}
	}
	return 0
}

// OpenOptions::new().write(true).create_new(true) (CREATE_NEW, which std
// opens with FILE_FLAG_OPEN_REPARSE_POINT), write_all, sync_all
// (FlushFileBuffers).
write_new_synced :: proc(path: string, data: []u8) -> Error {
	w, e := native(path)
	if e != 0 {
		return e
	}
	handle := win.CreateFileW(w, win.GENERIC_WRITE, SHARE_ALL, nil, win.CREATE_NEW, win.FILE_FLAG_OPEN_REPARSE_POINT, nil)
	if handle == win.INVALID_HANDLE_VALUE {
		return last()
	}
	defer win.CloseHandle(handle)
	if err := write_all(handle, data); err != 0 {
		return err
	}
	if !win.FlushFileBuffers(handle) {
		return last()
	}
	return 0
}

// `std::fs::remove_file`: DeleteFileW; when that is denied, the file
// opened for DELETE and removed with POSIX semantics ignoring its
// read-only attribute; the first error if that fails too.
remove_file :: proc(path: string) -> Error {
	w, e := native(path)
	if e != 0 {
		return e
	}
	if win.DeleteFileW(w) {
		return 0
	}
	err := last()
	if err == Error(win.ERROR_ACCESS_DENIED) {
		handle := win.CreateFileW(w, win.DELETE, SHARE_ALL, nil, win.OPEN_EXISTING, win.FILE_FLAG_OPEN_REPARSE_POINT, nil)
		if handle != win.INVALID_HANDLE_VALUE {
			defer win.CloseHandle(handle)
			// FILE_DISPOSITION_FLAG_DELETE | POSIX_SEMANTICS |
			// IGNORE_READONLY_ATTRIBUTE.
			flags: u32 = 0x1 | 0x2 | 0x10
			if win.SetFileInformationByHandle(handle, FILE_DISPOSITION_INFO_EX_CLASS, &flags, size_of(flags)) {
				return 0
			}
		}
	}
	return err
}

// `std::fs::create_dir`: CreateDirectoryW.
create_dir :: proc(path: string) -> Error {
	w, e := native(path)
	if e != 0 {
		return e
	}
	if !win.CreateDirectoryW(w, nil) {
		return last()
	}
	return 0
}

// FILE_RENAME_INFO with Flags for FileRenameInfoEx, the name after it.
@(private)
Rename_Info :: struct {
	flags:     u32,
	root:      win.HANDLE,
	name_size: u32,
}

// `std::fs::rename`: MoveFileExW(MOVEFILE_REPLACE_EXISTING); when that is
// denied, the file opened for DELETE and renamed with FileRenameInfoEx
// (replace if it exists, POSIX semantics); the first error if that fails,
// or ERROR_DIR_NOT_EMPTY.
rename :: proc(from, to: string) -> Error {
	f, fe := native(from)
	if fe != 0 {
		return fe
	}
	t, te := native(to)
	if te != 0 {
		return te
	}
	if win.MoveFileExW(f, t, win.MOVEFILE_REPLACE_EXISTING) {
		return 0
	}
	err := last()
	if err != Error(win.ERROR_ACCESS_DENIED) {
		return err
	}
	handle := win.CreateFileW(f, win.DELETE, SHARE_ALL, nil, win.OPEN_EXISTING, win.FILE_FLAG_OPEN_REPARSE_POINT | win.FILE_FLAG_BACKUP_SEMANTICS, nil)
	if handle == win.INVALID_HANDLE_VALUE {
		return err
	}
	defer win.CloseHandle(handle)
	units := wide_len(t)
	NAME_OFFSET :: 20 // offset_of(FILE_RENAME_INFO, FileName)
	#assert(offset_of(Rename_Info, name_size) == 16)
	size := NAME_OFFSET + units * 2 + 2
	block := make([]u8, size, context.temp_allocator)
	info := (^Rename_Info)(raw_data(block))
	// FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS
	info.flags = 0x1 | 0x2
	info.name_size = u32(units * 2)
	copy(([^]u16)(&block[NAME_OFFSET])[:units + 1], ([^]u16)(t)[:units + 1])
	if !win.SetFileInformationByHandle(handle, FILE_RENAME_INFO_EX_CLASS, raw_data(block), win.DWORD(size)) {
		if win.GetLastError() == ERROR_DIR_NOT_EMPTY {
			return Error(ERROR_DIR_NOT_EMPTY)
		}
		return err
	}
	return 0
}

// The `windows` crate's MoveFileExW as the app calls it.
@(private)
move_file :: proc(from, to: string, flags: win.DWORD) -> Error {
	f, fe := native(from)
	if fe != 0 {
		return fe
	}
	t, te := native(to)
	if te != 0 {
		return te
	}
	if !win.MoveFileExW(f, t, flags) {
		return last()
	}
	return 0
}

// windows/src/platform.rs's replace_file: MoveFileExW with
// MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
replace_file :: proc(from, to: string) -> Error {
	return move_file(from, to, win.MOVEFILE_REPLACE_EXISTING | win.MOVEFILE_WRITE_THROUGH)
}

// MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH), which fails if `to`
// exists (presets.rs). The error is GetLastError's.
move_no_replace :: proc(from, to: string) -> Error {
	return move_file(from, to, win.MOVEFILE_WRITE_THROUGH)
}

// SetFileAttributesW(path, FILE_ATTRIBUTE_HIDDEN), errors ignored.
hide :: proc(path: string) {
	if w, e := native(path); e == 0 {
		_ = win.SetFileAttributesW(w, win.FILE_ATTRIBUTE_HIDDEN)
	}
}

// MARK: The process

// `std::env::var_os`: GetEnvironmentVariableW through fill_utf16_buf;
// unset is (_, false), set and empty ("", true).
env :: proc(name: string, allocator := context.allocator) -> (string, bool) {
	wname, ok := to_utf16(name, true, context.temp_allocator)
	if !ok {
		return "", false
	}
	n := 512
	for {
		buf := make([]u16, n, context.temp_allocator)
		win.SetLastError(0)
		k := int(win.GetEnvironmentVariableW(win.wstring(raw_data(wname)), raw_data(buf), win.DWORD(n)))
		if k == 0 && win.GetLastError() != 0 {
			return "", false
		}
		if k == n && win.GetLastError() == win.ERROR_INSUFFICIENT_BUFFER {
			n *= 2
		} else if k > n {
			n = k
		} else {
			s, _ := from_utf16(buf[:k], allocator)
			return s, true
		}
	}
}

// `std::env::temp_dir` (GetTempPathW: it ends in a backslash).
temp_dir :: proc(allocator := context.allocator) -> string {
	buf: [win.MAX_PATH + 1]u16
	n := win.GetTempPathW(len(buf), win.LPCWSTR(raw_data(buf[:])))
	if n == 0 || int(n) > len(buf) {
		return strings.clone("", allocator)
	}
	s, _ := from_utf16(buf[:n], allocator)
	return s
}

process_id :: proc "contextless" () -> u32 {
	return u32(win.GetCurrentProcessId())
}

// `SystemTime::now()`.
now :: proc "contextless" () -> System_Time {
	t: win.FILETIME
	win.GetSystemTimePreciseAsFileTime(&t)
	return system_time(t)
}
