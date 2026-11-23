// The file operations windows/src/presets.rs and session.rs make, as
// Rust's std::fs and the `windows` crate make them: files_posix.odin on
// macOS and Linux (as core/diff/state-oracle's stand-ins do them, so the
// differential tests compare like with like), files_windows.odin on
// Windows (the Win32 calls the Rust makes). Paths are UTF-8; results are
// in the allocator given (the temporary one by default).
package files

import "core:strings"
import "../paths"

// An OS error: errno, or a Win32 error code; 0 is none.
Code :: distinct i32

// What `std::fs::metadata` says of a path (symbolic links followed).
Stat :: struct {
	is_file:  bool,
	is_dir:   bool,
	// FILE_ATTRIBUTE_HIDDEN on Windows; UF_HIDDEN in st_flags on macOS.
	hidden:   bool,
	size:     u64,
	// The modification time as Rust's SystemTime holds it: seconds since
	// 1970 (negative before) and nanoseconds within the second.
	seconds:  i64,
	nanos:    u32,
}

// Rust's io::Error Display for an OS error: the system's text and
// " (os error N)".
error_text :: proc(code: Code, allocator := context.temp_allocator) -> string {
	return os_error_text(code, allocator)
}

// `std::fs::create_dir_all`: every missing folder on the way, in Rust's
// order (the parent first only when the folder itself can't be made).
create_dir_all :: proc(path: string) -> Code {
	if path == "" {
		return 0
	}
	err := create_dir(path)
	if err == 0 {
		return 0
	}
	if is_not_found(err) {
		// Below: make the parent first.
	} else if is_dir(path) {
		return 0
	} else {
		return err
	}
	parent, has_parent := parent_of(path)
	if !has_parent {
		// "failed to create whole tree": Rust's own error, no OS code.
		return CODE_WHOLE_TREE
	}
	if e := create_dir_all(parent); e != 0 {
		return e
	}
	err = create_dir(path)
	if err == 0 || is_dir(path) {
		return 0
	}
	return err
}

// Rust's io::const_error!(Uncategorized, "failed to create whole tree").
CODE_WHOLE_TREE :: Code(-1)

is_dir :: proc(path: string) -> bool {
	st, err := stat(path)
	return err == 0 && st.is_dir
}

// session.rs's `write_atomic`: `data` to a sibling with the extension
// `tmp`, renamed over `path` (MoveFileExW, replacing), so a crash never
// leaves half a file. The error as anyhow's `{:#}` shows it.
write_atomic :: proc(path: string, data: []u8, allocator := context.temp_allocator) -> (err: string, ok: bool) {
	style := paths.HOST
	if dir, has := paths.parent(path, style); has {
		if code := create_dir_all(dir); code != 0 {
			return error_text(code, allocator), false
		}
	}
	staging := paths.with_extension(path, "tmp", style, context.temp_allocator)
	if code := write_file(staging, data); code != 0 {
		return error_text(code, allocator), false
	}
	if code, message := replace_file(staging, path, context.temp_allocator); code != 0 {
		return strings.concatenate({"Could not move the file to ", path, ": ", message}, allocator), false
	}
	return "", true
}
