#+build windows
// The file system as Rust's std::fs sees it on Windows. Unlike std, paths
// longer than MAX_PATH don't get a `\\?\` prefix here.
package cache_files

import win "core:sys/windows"

@(private = "file")
w_path :: proc(path: string) -> win.wstring {
	return win.utf8_to_wstring(path, context.temp_allocator)
}

@(private = "file")
last_error :: proc() -> Error {
	return Error(win.GetLastError())
}

// ErrorKind::NotFound: no such file, or no such folder on the way.
@(private)
is_not_found :: proc(e: Error) -> bool {
	return e == Error(win.ERROR_FILE_NOT_FOUND) || e == Error(win.ERROR_PATH_NOT_FOUND)
}

@(private)
os_message :: proc(e: Error) -> string {
	return "Windows error"
}

// FILETIME counts 100 ns since 1601, read as an i64 as std reads it;
// none before 1970.
@(private = "file")
from_filetime :: proc "contextless" (t: win.FILETIME) -> Maybe(Time) {
	ticks := i64(u64(t.dwHighDateTime) << 32 | u64(t.dwLowDateTime))
	EPOCH :: i64(116_444_736_000_000_000)
	if ticks < EPOCH {
		return nil
	}
	d := u64(ticks - EPOCH)
	return Time{d / 10_000_000, u32(d % 10_000_000) * 100}
}

// FileType: a name-surrogate reparse point (a symbolic link or junction)
// is neither a file nor a folder.
@(private = "file")
kind_of :: proc "contextless" (attributes, reparse_tag: win.DWORD) -> Kind {
	if attributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 && reparse_tag & 0x2000_0000 != 0 {
		return .Other
	}
	return .Dir if attributes & win.FILE_ATTRIBUTE_DIRECTORY != 0 else .File
}

// `std::fs::metadata`: the file opened with no access (links followed)
// and GetFileInformationByHandle; FindFirstFileW when it can't be opened
// for a sharing violation or a denied access.
stat :: proc(path: string) -> (Stat, bool) {
	if has_nul(path) {
		return {}, false
	}
	w := w_path(path)
	h := win.CreateFileW(
		w,
		0,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		win.FILE_FLAG_BACKUP_SEMANTICS,
		nil,
	)
	if h != win.INVALID_HANDLE_VALUE {
		defer win.CloseHandle(h)
		info: win.BY_HANDLE_FILE_INFORMATION
		if !win.GetFileInformationByHandle(h, &info) {
			return {}, false
		}
		return Stat {
				kind = kind_of(info.dwFileAttributes, 0),
				size = u64(info.nFileSizeHigh) << 32 | u64(info.nFileSizeLow),
				modified = from_filetime(info.ftLastWriteTime),
				created = from_filetime(info.ftCreationTime),
			},
			true
	}
	err := win.GetLastError()
	if err != win.ERROR_SHARING_VIOLATION && err != win.ERROR_ACCESS_DENIED {
		return {}, false
	}
	found: win.WIN32_FIND_DATAW
	fh := win.FindFirstFileW(w, &found)
	if fh == win.INVALID_HANDLE_VALUE {
		return {}, false
	}
	win.FindClose(fh)
	return Stat {
			kind = kind_of(found.dwFileAttributes, found.dwReserved0),
			size = u64(found.nFileSizeHigh) << 32 | u64(found.nFileSizeLow),
			modified = from_filetime(found.ftLastWriteTime),
			created = from_filetime(found.ftCreationTime),
		},
		true
}

// `std::fs::read_dir` (FindFirstFileW on `path\*`) and each entry's
// `file_type()`, "." and ".." left out. No match at all is an empty
// folder, as in std.
read_dir :: proc(path: string, allocator := context.allocator) -> ([]Entry, bool) {
	if has_nul(path) {
		return nil, false
	}
	pattern := join(path, "*", context.temp_allocator)
	found: win.WIN32_FIND_DATAW
	h := win.FindFirstFileW(w_path(pattern), &found)
	if h == win.INVALID_HANDLE_VALUE {
		return make([]Entry, 0, allocator), win.GetLastError() == win.ERROR_FILE_NOT_FOUND
	}
	defer win.FindClose(h)
	out := make([dynamic]Entry, 0, 16, allocator)
	for {
		name, err := win.wstring_to_utf8(win.wstring(raw_data(found.cFileName[:])), -1, allocator)
		if err == nil {
			if name == "." || name == ".." {
				delete(name, allocator)
			} else {
				tag := found.dwReserved0 if found.dwFileAttributes & win.FILE_ATTRIBUTE_REPARSE_POINT != 0 else 0
				append(&out, Entry{name, kind_of(found.dwFileAttributes, tag)})
			}
		}
		if !win.FindNextFileW(h, &found) {
			break
		}
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
	h := win.CreateFileW(
		w_path(path),
		win.GENERIC_READ,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.OPEN_EXISTING,
		win.FILE_ATTRIBUTE_NORMAL,
		nil,
	)
	if h == win.INVALID_HANDLE_VALUE {
		return nil, last_error()
	}
	defer win.CloseHandle(h)
	size := 0
	file_size: win.LARGE_INTEGER
	if win.GetFileSizeEx(h, &file_size) && file_size > 0 {
		size = int(file_size)
	}
	out := make([dynamic]u8, 0, size + 1, allocator)
	for {
		if cap(out) == len(out) {
			reserve(&out, max(cap(out) * 2, 4096))
		}
		start := len(out)
		room := min(cap(out) - start, int(max(i32)))
		resize(&out, start + room)
		got: win.DWORD
		if !win.ReadFile(h, raw_data(out[start:]), win.DWORD(room), &got, nil) {
			err := last_error()
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

// `File::create` and `write_all` of each part in turn.
write :: proc(path: string, parts: ..[]u8) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	h := win.CreateFileW(
		w_path(path),
		win.GENERIC_WRITE,
		win.FILE_SHARE_READ | win.FILE_SHARE_WRITE | win.FILE_SHARE_DELETE,
		nil,
		win.CREATE_ALWAYS,
		win.FILE_ATTRIBUTE_NORMAL,
		nil,
	)
	if h == win.INVALID_HANDLE_VALUE {
		return last_error()
	}
	for part in parts {
		rest := part
		for len(rest) > 0 {
			written: win.DWORD
			if !win.WriteFile(h, raw_data(rest), win.DWORD(min(len(rest), 1 << 30)), &written, nil) {
				err := last_error()
				win.CloseHandle(h)
				return err
			}
			rest = rest[written:]
		}
	}
	win.CloseHandle(h)
	return 0
}

// windows/src/platform.rs's replace_file: MoveFileExW with
// MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
replace :: proc(from, to: string) -> Error {
	if has_nul(from) || has_nul(to) {
		return NUL_IN_PATH
	}
	if !win.MoveFileExW(w_path(from), w_path(to), win.MOVEFILE_REPLACE_EXISTING | win.MOVEFILE_WRITE_THROUGH) {
		return last_error()
	}
	return 0
}

// `std::fs::remove_file`.
remove :: proc(path: string) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	if !win.DeleteFileW(w_path(path)) {
		return last_error()
	}
	return 0
}

@(private)
make_dir :: proc(path: string) -> Error {
	if has_nul(path) {
		return NUL_IN_PATH
	}
	if !win.CreateDirectoryW(w_path(path), nil) {
		return last_error()
	}
	return 0
}
