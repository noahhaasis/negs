// Paths as Rust's `std::path` takes them apart, for the paths the library
// makes (a folder joined with a listed name): file names, parents,
// extensions and stems, and the sidecar's own names.
package library

import "core:strings"
import "core:unicode"
import "core:unicode/utf8"

SIDECAR_EXTENSION :: "negs"

is_separator :: proc(c: u8) -> bool {
	when ODIN_OS == .Windows {
		return c == '/' || c == '\\'
	} else {
		return c == '/'
	}
}

SEPARATOR :: "\\" when ODIN_OS == .Windows else "/"

// The path without trailing separators and `.` components (a root stays).
@(private = "file")
trim_end :: proc(path: string) -> string {
	s := path
	for {
		for len(s) > 1 && is_separator(s[len(s) - 1]) {
			s = s[:len(s) - 1]
		}
		if len(s) >= 2 && s[len(s) - 1] == '.' && is_separator(s[len(s) - 2]) {
			s = s[:len(s) - 1]
			continue
		}
		return s
	}
}

@(private = "file")
last_separator :: proc(s: string) -> int {
	for i := len(s) - 1; i >= 0; i -= 1 {
		if is_separator(s[i]) {
			return i
		}
	}
	return -1
}

// `Path::file_name`: the last component, "" when it is none (`..`, a root).
file_name :: proc(path: string) -> string {
	s := trim_end(path)
	i := last_separator(s)
	name := s[i + 1:]
	if name == ".." || name == "." {
		return ""
	}
	return name
}

// `Path::parent`: the path without its last component; false for a root
// or an empty path.
parent :: proc(path: string) -> (string, bool) {
	s := trim_end(path)
	if s == "" || (len(s) == 1 && is_separator(s[0])) {
		return "", false
	}
	i := last_separator(s)
	if i < 0 {
		return "", true
	}
	head := s[:i]
	for len(head) > 1 && is_separator(head[len(head) - 1]) {
		head = head[:len(head) - 1]
	}
	if head == "" {
		return s[:1], true
	}
	return head, true
}

// `Path::join`.
join :: proc(base, name: string, allocator := context.allocator) -> string {
	if base == "" || (len(name) > 0 && is_separator(name[0])) {
		return strings.clone(name, allocator)
	}
	if is_separator(base[len(base) - 1]) {
		return strings.concatenate({base, name}, allocator)
	}
	return strings.concatenate({base, SEPARATOR, name}, allocator)
}

// The extension of a file name as `Path::extension` gives it, and its stem.
split_extension :: proc(name: string) -> (stem: string, extension: string, has: bool) {
	if name == ".." {
		return name, "", false
	}
	i := strings.last_index_byte(name, '.')
	if i <= 0 {
		return name, "", false
	}
	return name[:i], name[i + 1:], true
}

// The lowercased extension of `path`'s file name ("" without one).
extension_lower :: proc(path: string, allocator := context.temp_allocator) -> string {
	_, ext, _ := split_extension(file_name(path))
	return lower_full(ext, allocator)
}

// The lowercased stem of `path`'s file name.
stem_lower :: proc(path: string, allocator := context.temp_allocator) -> string {
	stem, _, _ := split_extension(file_name(path))
	return lower_full(stem, allocator)
}

// Rust's `str::to_lowercase`: each character's lower case, İ as i̇, and a
// capital sigma at the end of a word as ς.
lower_full :: proc(s: string, allocator := context.temp_allocator) -> string {
	ascii := true
	for i in 0 ..< len(s) {
		if s[i] >= 0x80 || (s[i] >= 'A' && s[i] <= 'Z') {
			ascii = false
			break
		}
	}
	if ascii {
		return s
	}
	b := strings.builder_make(0, len(s), allocator)
	for r, i in s {
		switch {
		case r == 0x130:
			strings.write_string(&b, "i̇")
		case r == 0x3A3:
			strings.write_rune(&b, 0x3C2 if final_sigma(s, i) else 0x3C3)
		case:
			strings.write_rune(&b, unicode.to_lower(r))
		}
	}
	return strings.to_string(b)
}

@(private = "file")
case_ignorable :: proc(r: rune) -> bool {
	switch r {
	case '\'', '.', ':', '^', '`', 0xAD, 0xB7, 0x2019:
		return true
	}
	return unicode.is_nonspacing_mark(r) || unicode.is_enclosing_mark(r)
}

@(private = "file")
cased :: proc(r: rune) -> bool {
	return unicode.is_upper(r) || unicode.is_lower(r) || unicode.is_title(r)
}

// Σ at `at` ends a word: a cased letter before it and none after it,
// case-ignorable characters skipped.
@(private = "file")
final_sigma :: proc(s: string, at: int) -> bool {
	before := false
	i := at
	for i > 0 {
		r, width := utf8.decode_last_rune_in_string(s[:i])
		i -= width
		if case_ignorable(r) {
			continue
		}
		before = cased(r)
		break
	}
	if !before {
		return false
	}
	_, width := utf8.decode_rune_in_string(s[at:])
	for r in s[at + width:] {
		if case_ignorable(r) {
			continue
		}
		return !cased(r)
	}
	return true
}

// `IMG_0412.CR3` → `IMG_0412.CR3.negs`.
sidecar_path :: proc(photo: string, allocator := context.allocator) -> string {
	name := file_name(photo)
	dir := photo[:len(photo) - len(trim_tail(photo, name))]
	return strings.concatenate({dir, name, ".", SIDECAR_EXTENSION}, allocator)
}

// The hidden file a write goes through (`.IMG_0412.CR3.negs.tmp`), which
// folder listings skip.
temporary_path :: proc(photo: string, allocator := context.allocator) -> string {
	name := file_name(photo)
	dir := photo[:len(photo) - len(trim_tail(photo, name))]
	return strings.concatenate({dir, ".", name, ".", SIDECAR_EXTENSION, ".tmp"}, allocator)
}

// What of `path` follows the folder its file name `name` is in (the name
// and any trailing separators), so the rest is the folder with its
// separator.
@(private = "file")
trim_tail :: proc(path, name: string) -> string {
	if name == "" {
		return ""
	}
	s := trim_end(path)
	return path[len(s) - len(name):]
}
