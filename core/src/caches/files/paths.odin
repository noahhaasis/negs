// std::path's rules for the parts of a path the caches and the batch
// name things by: `file_name`, `file_stem`, `extension`, `parent`,
// `with_extension`, `join` (PathBuf::push of a plain name) and `display`.
// Both platforms' rules are here, so each can be tested anywhere; STYLE is
// the one this build's std uses. Windows' follow std's prefixes (`C:`,
// `\\server\share`, `\\?\…`, `\\.\…`) and separators (`\` and `/`, only
// `\` after a verbatim prefix). One shortcut: `join` onto a verbatim path
// doesn't tidy its `.`, `..` and doubled separators as std does.
package cache_files

import "core:strings"
import "core:unicode/utf8"

Style :: enum u8 {
	Unix,
	Windows,
}

STYLE :: Style.Windows when ODIN_OS == .Windows else Style.Unix

Prefix_Kind :: enum u8 {
	None,
	Verbatim,
	Verbatim_UNC,
	Verbatim_Disk,
	Device_NS,
	UNC,
	Disk,
}

@(private = "file")
Back :: enum u8 {
	Body,
	Start_Dir,
	Prefix,
	Done,
}

@(private = "file")
Component_Kind :: enum u8 {
	None,
	Prefix,
	Root_Dir,
	Cur_Dir,
	Parent_Dir,
	Normal,
}

// std's Components, walked from the back only (`next_back`, `as_path`).
@(private = "file")
Components :: struct {
	path:          string,
	style:         Style,
	prefix:        Prefix_Kind,
	prefix_len:    int,
	physical_root: bool,
	back:          Back,
}

@(private = "file")
any_sep :: proc "contextless" (c: u8, style: Style) -> bool {
	return c == '/' || (style == .Windows && c == '\\')
}

@(private = "file")
next_part :: proc "contextless" (path: string, verbatim: bool) -> (part: string, rest: string) {
	for i in 0 ..< len(path) {
		c := path[i]
		if c == '\\' || (!verbatim && c == '/') {
			return path[:i], path[i + 1:]
		}
	}
	return path, ""
}

@(private = "file")
is_drive :: proc "contextless" (path: string) -> bool {
	if len(path) < 2 || path[1] != ':' {
		return false
	}
	c := path[0]
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}

// std::sys::path::windows::parse_prefix, and the prefix's length.
parse_prefix :: proc "contextless" (path: string) -> (Prefix_Kind, int) {
	// PrefixParser compares the first 8 bytes with `/` read as `\`.
	norm :: proc "contextless" (path: string, i: int) -> u8 {
		if i >= len(path) {
			return 0
		}
		return '\\' if path[i] == '/' else path[i]
	}
	if norm(path, 0) == '\\' && norm(path, 1) == '\\' {
		if norm(path, 2) == '?' && norm(path, 3) == '\\' && strings.index_byte(path[:4], '/') < 0 {
			if norm(path, 4) == 'U' && norm(path, 5) == 'N' && norm(path, 6) == 'C' && norm(path, 7) == '\\' {
				server, rest := next_part(path[8:], true)
				share, _ := next_part(rest, true)
				return .Verbatim_UNC, 8 + len(server) + (1 + len(share) if len(share) > 0 else 0)
			}
			rest := path[4:]
			if (len(rest) < 3 || rest[2] == '\\') && is_drive(rest) {
				return .Verbatim_Disk, 6
			}
			part, _ := next_part(rest, true)
			return .Verbatim, 4 + len(part)
		}
		if norm(path, 2) == '.' && norm(path, 3) == '\\' {
			part, _ := next_part(path[4:], false)
			return .Device_NS, 4 + len(part)
		}
		server, rest := next_part(path[2:], false)
		share, _ := next_part(rest, false)
		if len(server) > 0 && len(share) > 0 {
			return .UNC, 2 + len(server) + 1 + len(share)
		}
		return .None, 0
	}
	if is_drive(path) {
		return .Disk, 2
	}
	return .None, 0
}

@(private = "file")
components :: proc "contextless" (path: string, style: Style) -> Components {
	c := Components{path = path, style = style}
	if style == .Windows {
		c.prefix, c.prefix_len = parse_prefix(path)
	}
	c.physical_root = len(path) > c.prefix_len && any_sep(path[c.prefix_len], style)
	return c
}

@(private = "file")
verbatim :: proc "contextless" (c: ^Components) -> bool {
	return c.prefix == .Verbatim || c.prefix == .Verbatim_UNC || c.prefix == .Verbatim_Disk
}

@(private = "file")
is_sep :: proc "contextless" (c: ^Components, b: u8) -> bool {
	if c.style == .Windows && verbatim(c) {
		return b == '\\'
	}
	return any_sep(b, c.style)
}

@(private = "file")
has_root :: proc "contextless" (c: ^Components) -> bool {
	return c.physical_root || (c.prefix != .None && c.prefix != .Disk)
}

@(private = "file")
include_cur_dir :: proc "contextless" (c: ^Components) -> bool {
	if has_root(c) {
		return false
	}
	s := c.path[min(c.prefix_len, len(c.path)):]
	return (len(s) == 1 && s[0] == '.') || (len(s) >= 2 && s[0] == '.' && is_sep(c, s[1]))
}

@(private = "file")
len_before_body :: proc "contextless" (c: ^Components) -> int {
	return c.prefix_len + (1 if c.physical_root else 0) + (1 if include_cur_dir(c) else 0)
}

// The last component of the body: its kind, where it starts, and how many
// bytes it and its separator take.
@(private = "file")
next_component_back :: proc "contextless" (c: ^Components) -> (kind: Component_Kind, start: int, size: int) {
	lbb := len_before_body(c)
	body := c.path[lbb:]
	extra := 0
	start = lbb
	for i := len(body) - 1; i >= 0; i -= 1 {
		if is_sep(c, body[i]) {
			extra = 1
			start = lbb + i + 1
			break
		}
	}
	comp := c.path[start:]
	size = len(comp) + extra
	switch comp {
	case ".":
		kind = .Cur_Dir if verbatim(c) else .None
	case "..":
		kind = .Parent_Dir
	case "":
		kind = .None
	case:
		kind = .Normal
	}
	return
}

// `Components::next_back`: the kind of the last component and where it is.
@(private = "file")
next_back :: proc "contextless" (c: ^Components) -> (kind: Component_Kind, start: int, end: int) {
	for c.back != .Done {
		switch c.back {
		case .Body:
			if len(c.path) > len_before_body(c) {
				k, s, size := next_component_back(c)
				end = len(c.path)
				c.path = c.path[:len(c.path) - size]
				if k != .None {
					return k, s, end
				}
			} else {
				c.back = .Start_Dir
			}
		case .Start_Dir:
			c.back = .Prefix
			if c.physical_root {
				c.path = c.path[:len(c.path) - 1]
				return .Root_Dir, len(c.path), len(c.path) + 1
			} else if c.prefix != .None {
				if c.prefix != .Disk && !verbatim(c) {
					return .Root_Dir, len(c.path), len(c.path)
				}
			} else if include_cur_dir(c) {
				c.path = c.path[:len(c.path) - 1]
				return .Cur_Dir, len(c.path), len(c.path) + 1
			}
		case .Prefix:
			c.back = .Done
			if c.prefix_len > 0 {
				return .Prefix, 0, c.prefix_len
			}
			return .None, 0, 0
		case .Done:
		}
	}
	return .None, 0, 0
}

// `Components::as_path` after next_back: the rest, trailing separators and
// `.` components trimmed while the walk is in the body.
@(private = "file")
as_path :: proc "contextless" (c: ^Components) -> string {
	if c.back == .Body {
		for len(c.path) > len_before_body(c) {
			k, _, size := next_component_back(c)
			if k != .None {
				break
			}
			c.path = c.path[:len(c.path) - size]
		}
	}
	return c.path
}

// `Path::file_name`, as where it lies in `path`.
@(private = "file")
file_name_span :: proc "contextless" (path: string, style: Style) -> (start: int, end: int, ok: bool) {
	c := components(path, style)
	kind, s, e := next_back(&c)
	if kind != .Normal {
		return 0, 0, false
	}
	return s, e, true
}

file_name :: proc "contextless" (path: string, style := STYLE) -> (string, bool) {
	s, e, ok := file_name_span(path, style)
	return path[s:e], ok
}

// rsplit_file_at_dot: the stem and the extension of a file name.
@(private = "file")
split_at_dot :: proc "contextless" (name: string) -> (stem: string, ext: string, has_ext: bool) {
	if name == ".." {
		return name, "", false
	}
	for i := len(name) - 1; i >= 0; i -= 1 {
		if name[i] == '.' {
			if i == 0 {
				return name, "", false
			}
			return name[:i], name[i + 1:], true
		}
	}
	return name, "", false
}

file_stem :: proc "contextless" (path: string, style := STYLE) -> (stem: string, ok: bool) {
	name := file_name(path, style) or_return
	stem, _, _ = split_at_dot(name)
	return stem, true
}

extension :: proc "contextless" (path: string, style := STYLE) -> (ext: string, ok: bool) {
	name := file_name(path, style) or_return
	has: bool
	_, ext, has = split_at_dot(name)
	return ext, has
}

// `Path::parent`.
parent :: proc "contextless" (path: string, style := STYLE) -> (string, bool) {
	c := components(path, style)
	kind, _, _ := next_back(&c)
	#partial switch kind {
	case .Normal, .Cur_Dir, .Parent_Dir:
		return as_path(&c), true
	}
	return "", false
}

// `PathBuf::set_extension` on a copy: cut right after the file stem, then
// `.ext` unless `ext` is empty; unchanged without a file name.
@(private = "file")
set_extension :: proc(path: string, ext: string, allocator: Allocator, style: Style) -> string {
	s, e, ok := file_name_span(path, style)
	if !ok {
		return strings.clone(path, allocator)
	}
	stem, _, _ := split_at_dot(path[s:e])
	cut := s + len(stem)
	if ext == "" {
		return strings.clone(path[:cut], allocator)
	}
	return strings.concatenate({path[:cut], ".", ext}, allocator)
}

// `Path::with_extension`: the path up to its old extension's length from
// the end (all of it without one), then set_extension.
with_extension :: proc(path: string, ext: string, allocator := context.allocator, style := STYLE) -> string {
	base := path
	if old, has := extension(path, style); has {
		base = path[:len(path) - len(old)]
	}
	return set_extension(base, ext, allocator, style)
}

// `dir.join(name)` for a relative `name` without a root or prefix (a
// folder entry's name): a separator between them unless `dir` ends in one,
// is empty, or (on Windows) is a bare drive like `C:`.
join :: proc(dir: string, name: string, allocator := context.allocator, style := STYLE) -> string {
	need_sep := len(dir) > 0 && !any_sep(dir[len(dir) - 1], style)
	if style == .Windows {
		kind, n := parse_prefix(dir)
		if kind == .Disk && n == len(dir) {
			need_sep = false
		}
	}
	if need_sep {
		return strings.concatenate({dir, "\\" if style == .Windows else "/", name}, allocator)
	}
	return strings.concatenate({dir, name}, allocator)
}

// `Path::display` (and `to_string_lossy`): the bytes as UTF-8, each
// maximal invalid run replaced by U+FFFD, as std's Utf8Chunks does.
display :: proc(path: string, allocator := context.allocator) -> string {
	if utf8.valid_string(path) {
		return strings.clone(path, allocator)
	}
	b := strings.builder_make(0, len(path) + 8, allocator)
	write_lossy(&b, path)
	return strings.to_string(b)
}

// The text of `s` as String::from_utf8_lossy makes it.
write_lossy :: proc(b: ^strings.Builder, s: string) {
	i := 0
	for i < len(s) {
		c := s[i]
		if c < 0x80 {
			strings.write_byte(b, c)
			i += 1
			continue
		}
		// The length of a valid sequence starting here, or how many bytes
		// form the maximal invalid prefix (at least one).
		n := 0
		lo, hi: u8 = 0x80, 0xBF
		switch c {
		case 0xC2 ..= 0xDF:
			n = 2
		case 0xE0:
			n, lo = 3, 0xA0
		case 0xE1 ..= 0xEC, 0xEE ..= 0xEF:
			n = 3
		case 0xED:
			n, hi = 3, 0x9F
		case 0xF0:
			n, lo = 4, 0x90
		case 0xF1 ..= 0xF3:
			n = 4
		case 0xF4:
			n, hi = 4, 0x8F
		}
		if n == 0 {
			strings.write_string(b, "�")
			i += 1
			continue
		}
		j := i + 1
		ok := true
		for k in 1 ..< n {
			if j >= len(s) {
				ok = false
				break
			}
			d := s[j]
			l, h: u8 = 0x80, 0xBF
			if k == 1 {
				l, h = lo, hi
			}
			if d < l || d > h {
				ok = false
				break
			}
			j += 1
		}
		if ok {
			strings.write_string(b, s[i:j])
		} else {
			strings.write_string(b, "�")
		}
		i = j
	}
}
