// std::path's rules: a path's components, file name, stem and extension,
// parent, a path joined on, the extension changed, whether it is absolute.
// Both platforms' rules are here, chosen by `Style` (`HOST` for the one the
// core runs on), so the Windows rules are tested on the Mac too: Windows has
// prefixes (`C:`, `\\server\share`, `\\?\...`, `\\.\...`) and takes `/` as
// well as `\` as a separator (only `\` after a verbatim `\\?\` prefix); Unix
// has neither. A line-for-line port of std's Components
// (library/std/src/path.rs, sys/path/windows_prefix.rs, Rust 1.93).
package platform_fs

import "core:strings"

Style :: enum u8 {
	Unix,
	Windows,
}

HOST :: Style.Windows when ODIN_OS == .Windows else Style.Unix

// The separator `push` adds.
main_separator :: proc "contextless" (style: Style) -> u8 {
	return '\\' if style == .Windows else '/'
}

Prefix_Kind :: enum u8 {
	None,
	Verbatim, // \\?\prefix
	Verbatim_UNC, // \\?\UNC\server\share
	Verbatim_Disk, // \\?\C:
	Device_NS, // \\.\COM42
	UNC, // \\server\share
	Disk, // C:
}

Prefix :: struct {
	kind: Prefix_Kind,
	// Its length in bytes (Prefix::len).
	len:  int,
}

@(private)
is_sep :: proc "contextless" (style: Style, b: u8) -> bool {
	return b == '/' || (style == .Windows && b == '\\')
}

@(private)
is_verbatim_sep :: proc "contextless" (b: u8) -> bool {
	return b == '\\'
}

is_verbatim :: proc "contextless" (p: Prefix) -> bool {
	return p.kind == .Verbatim || p.kind == .Verbatim_UNC || p.kind == .Verbatim_Disk
}

is_drive :: proc "contextless" (p: Prefix) -> bool {
	return p.kind == .Disk
}

@(private)
has_implicit_root :: proc "contextless" (p: Prefix) -> bool {
	return p.kind != .None && !is_drive(p)
}

// parse_next_component: the component up to the next separator, and the
// rest after it.
@(private)
next_component :: proc "contextless" (path: string, verbatim: bool) -> (component: string, rest: string) {
	for i in 0 ..< len(path) {
		c := path[i]
		if c == '\\' || (!verbatim && c == '/') {
			return path[:i], path[i + 1:]
		}
	}
	return path, ""
}

@(private)
drive_letter :: proc "contextless" (path: string) -> bool {
	if len(path) < 2 || path[1] != ':' {
		return false
	}
	c := path[0]
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}

// parse_prefix: Windows' prefix of `path`, if any.
parse_prefix :: proc "contextless" (path: string, style := HOST) -> Prefix {
	if style != .Windows {
		return {}
	}
	// The first 8 bytes with `/` read as `\`.
	head: [8]u8
	n := min(len(path), 8)
	for i in 0 ..< n {
		head[i] = '\\' if path[i] == '/' else path[i]
	}
	starts :: proc "contextless" (head: []u8, at: int, s: string) -> bool {
		if at + len(s) > len(head) {
			return false
		}
		for i in 0 ..< len(s) {
			if head[at + i] != s[i] {
				return false
			}
		}
		return true
	}
	h := head[:n]
	if starts(h, 0, `\\`) {
		// A verbatim prefix only with real backslashes.
		if starts(h, 2, `?\`) && path[2] != '/' && path[3] != '/' && path[0] != '/' && path[1] != '/' {
			if starts(h, 4, `UNC\`) {
				rest := path[8:]
				server, after := next_component(rest, true)
				share, _ := next_component(after, true)
				return {.Verbatim_UNC, 8 + len(server) + (1 + len(share) if len(share) > 0 else 0)}
			}
			rest := path[4:]
			// parse_drive_exact: "C:" followed by nothing or a separator.
			if drive_letter(rest) && (len(rest) == 2 || rest[2] == '/' || rest[2] == '\\') {
				return {.Verbatim_Disk, 6}
			}
			prefix, _ := next_component(rest, true)
			return {.Verbatim, 4 + len(prefix)}
		}
		if starts(h, 2, `.\`) {
			rest := path[4:]
			prefix, _ := next_component(rest, false)
			return {.Device_NS, 4 + len(prefix)}
		}
		rest := path[2:]
		server, after := next_component(rest, false)
		share, _ := next_component(after, false)
		if len(server) > 0 && len(share) > 0 {
			return {.UNC, 2 + len(server) + 1 + len(share)}
		}
		return {}
	}
	if drive_letter(path) {
		return {.Disk, 2}
	}
	return {}
}

// MARK: Components

// Component::RootDir's text: the main separator, whichever byte was there.
@(private)
root_text :: proc "contextless" (style: Style) -> string {
	return "\\" if style == .Windows else "/"
}

Component_Kind :: enum u8 {
	Prefix,
	Root_Dir,
	Cur_Dir,
	Parent_Dir,
	Normal,
}

Component :: struct {
	kind: Component_Kind,
	text: string,
}

@(private)
State :: enum u8 {
	Prefix,
	Start_Dir,
	Body,
	Done,
}

Components :: struct {
	path:              string,
	style:             Style,
	prefix:            Prefix,
	has_physical_root: bool,
	front, back:       State,
}

components :: proc "contextless" (path: string, style := HOST) -> Components {
	p := parse_prefix(path, style)
	rest := path[p.len:]
	return Components {
		path = path,
		style = style,
		prefix = p,
		has_physical_root = len(rest) > 0 && is_sep(style, rest[0]),
		front = .Prefix if style == .Windows else .Start_Dir,
		back = .Body,
	}
}

@(private)
c_is_sep :: proc "contextless" (c: ^Components, b: u8) -> bool {
	return is_verbatim_sep(b) if is_verbatim(c.prefix) else is_sep(c.style, b)
}

@(private)
prefix_remaining :: proc "contextless" (c: ^Components) -> int {
	return c.prefix.len if c.front == .Prefix else 0
}

// Is the original path rooted?
@(private)
c_has_root :: proc "contextless" (c: ^Components) -> bool {
	return c.has_physical_root || has_implicit_root(c.prefix)
}

@(private)
include_cur_dir :: proc "contextless" (c: ^Components) -> bool {
	if c_has_root(c) {
		return false
	}
	s := c.path[prefix_remaining(c):]
	if len(s) == 1 && s[0] == '.' {
		return true
	}
	return len(s) >= 2 && s[0] == '.' && c_is_sep(c, s[1])
}

@(private)
len_before_body :: proc "contextless" (c: ^Components) -> int {
	root := 1 if c.front <= .Start_Dir && c.has_physical_root else 0
	cur := 1 if c.front <= .Start_Dir && include_cur_dir(c) else 0
	return prefix_remaining(c) + root + cur
}

@(private)
finished :: proc "contextless" (c: ^Components) -> bool {
	return c.front == .Done || c.back == .Done || c.front > c.back
}

@(private)
single :: proc "contextless" (c: ^Components, comp: string) -> (Component, bool) {
	switch comp {
	case ".":
		if is_verbatim(c.prefix) {
			return {.Cur_Dir, comp}, true
		}
		return {}, false
	case "..":
		return {.Parent_Dir, comp}, true
	case "":
		return {}, false
	}
	return {.Normal, comp}, true
}

@(private)
parse_front :: proc "contextless" (c: ^Components) -> (size: int, comp: Component, ok: bool) {
	for i in 0 ..< len(c.path) {
		if c_is_sep(c, c.path[i]) {
			comp, ok = single(c, c.path[:i])
			return i + 1, comp, ok
		}
	}
	comp, ok = single(c, c.path)
	return len(c.path), comp, ok
}

@(private)
parse_back :: proc "contextless" (c: ^Components) -> (size: int, comp: Component, ok: bool) {
	start := len_before_body(c)
	body := c.path[start:]
	for i := len(body) - 1; i >= 0; i -= 1 {
		if c_is_sep(c, body[i]) {
			comp, ok = single(c, body[i + 1:])
			return len(body) - i, comp, ok
		}
	}
	comp, ok = single(c, body)
	return len(body), comp, ok
}

@(private)
trim_left :: proc "contextless" (c: ^Components) {
	for len(c.path) > 0 {
		size, _, ok := parse_front(c)
		if ok {
			return
		}
		c.path = c.path[size:]
	}
}

@(private)
trim_right :: proc "contextless" (c: ^Components) {
	for len(c.path) > len_before_body(c) {
		size, _, ok := parse_back(c)
		if ok {
			return
		}
		c.path = c.path[:len(c.path) - size]
	}
}

// Components::as_path: what is left to iterate.
as_path :: proc "contextless" (c: Components) -> string {
	c := c
	if c.front == .Body {
		trim_left(&c)
	}
	if c.back == .Body {
		trim_right(&c)
	}
	return c.path
}

next :: proc "contextless" (c: ^Components) -> (Component, bool) {
	for !finished(c) {
		switch c.front {
		case .Body:
			if len(c.path) > 0 {
				size, comp, ok := parse_front(c)
				c.path = c.path[size:]
				if ok {
					return comp, true
				}
			} else {
				c.front = .Done
			}
		case .Start_Dir:
			c.front = .Body
			if c.has_physical_root {
				c.path = c.path[1:]
				return {.Root_Dir, root_text(c.style)}, true
			} else if c.prefix.kind != .None {
				if has_implicit_root(c.prefix) && !is_verbatim(c.prefix) {
					return {.Root_Dir, root_text(c.style)}, true
				}
			} else if include_cur_dir(c) {
				c.path = c.path[1:]
				return {.Cur_Dir, "."}, true
			}
		case .Prefix:
			c.front = .Start_Dir
			if c.prefix.len > 0 {
				raw := c.path[:c.prefix.len]
				c.path = c.path[c.prefix.len:]
				return {.Prefix, raw}, true
			}
		case .Done:
			return {}, false
		}
	}
	return {}, false
}

next_back :: proc "contextless" (c: ^Components) -> (Component, bool) {
	for !finished(c) {
		switch c.back {
		case .Body:
			if len(c.path) > len_before_body(c) {
				size, comp, ok := parse_back(c)
				c.path = c.path[:len(c.path) - size]
				if ok {
					return comp, true
				}
			} else {
				c.back = .Start_Dir
			}
		case .Start_Dir:
			c.back = .Prefix if c.style == .Windows else .Done
			if c.has_physical_root {
				c.path = c.path[:len(c.path) - 1]
				return {.Root_Dir, root_text(c.style)}, true
			} else if c.prefix.kind != .None {
				if has_implicit_root(c.prefix) && !is_verbatim(c.prefix) {
					return {.Root_Dir, root_text(c.style)}, true
				}
			} else if include_cur_dir(c) {
				c.path = c.path[:len(c.path) - 1]
				return {.Cur_Dir, "."}, true
			}
		case .Prefix:
			c.back = .Done
			if c.prefix.len > 0 {
				return {.Prefix, c.path}, true
			}
			return {}, false
		case .Done:
			return {}, false
		}
	}
	return {}, false
}

// MARK: Path's methods

has_root :: proc "contextless" (path: string, style := HOST) -> bool {
	c := components(path, style)
	return c_has_root(&c)
}

is_absolute :: proc "contextless" (path: string, style := HOST) -> bool {
	if style == .Windows {
		return has_root(path, style) && parse_prefix(path, style).kind != .None
	}
	return has_root(path, style)
}

// `Path::file_name`: a slice of `path`.
file_name :: proc "contextless" (path: string, style := HOST) -> (string, bool) {
	c := components(path, style)
	comp, ok := next_back(&c)
	if ok && comp.kind == .Normal {
		return comp.text, true
	}
	return "", false
}

// `Path::parent`: a slice of `path`.
parent :: proc "contextless" (path: string, style := HOST) -> (string, bool) {
	c := components(path, style)
	comp, ok := next_back(&c)
	if !ok {
		return "", false
	}
	#partial switch comp.kind {
	case .Normal, .Cur_Dir, .Parent_Dir:
		return as_path(c), true
	}
	return "", false
}

// rsplit_file_at_dot: (stem, extension).
@(private)
split_at_dot :: proc "contextless" (file: string) -> (before: string, has_before: bool, after: string, has_after: bool) {
	if file == ".." {
		return file, true, "", false
	}
	for i := len(file) - 1; i >= 0; i -= 1 {
		if file[i] == '.' {
			if i == 0 {
				return file, true, "", false
			}
			return file[:i], true, file[i + 1:], true
		}
	}
	return "", false, file, true
}

// `Path::file_stem`: a slice of `path`.
file_stem :: proc "contextless" (path: string, style := HOST) -> (string, bool) {
	name, ok := file_name(path, style)
	if !ok {
		return "", false
	}
	before, has_before, after, _ := split_at_dot(name)
	return before if has_before else after, true
}

// `Path::extension`: a slice of `path`.
extension :: proc "contextless" (path: string, style := HOST) -> (string, bool) {
	name, ok := file_name(path, style)
	if !ok {
		return "", false
	}
	_, has_before, after, has_after := split_at_dot(name)
	if has_before && has_after {
		return after, true
	}
	return "", false
}

// `PathBuf::set_extension` on a copy: cut right after the file stem, then
// `.ext` unless `ext` is empty; unchanged without a file name.
@(private)
set_extension :: proc(path: string, ext: string, style: Style, allocator := context.allocator) -> string {
	stem, ok := file_stem(path, style)
	if !ok {
		return strings.clone(path, allocator)
	}
	// The stem is a slice of `path`: keep everything up to its end.
	end := int(uintptr(raw_data(stem)) - uintptr(raw_data(path))) + len(stem)
	b := strings.builder_make(0, end + 1 + len(ext), allocator)
	strings.write_string(&b, path[:end])
	if len(ext) > 0 {
		strings.write_byte(&b, '.')
		strings.write_string(&b, ext)
	}
	return strings.to_string(b)
}

// `Path::with_extension`: the path up to its old extension's length from
// the end (all of it without one), then set_extension.
with_extension :: proc(path: string, ext: string, style := HOST, allocator := context.allocator) -> string {
	base := path
	if old, has := extension(path, style); has {
		base = path[:len(path) - len(old)]
	}
	return set_extension(base, ext, style, allocator)
}

// `PathBuf::from(base).join(name)` (PathBuf::push).
join :: proc(base: string, name: string, style := HOST, allocator := context.allocator) -> string {
	need_sep := len(base) > 0 && !is_sep(style, base[len(base) - 1])
	c := components(base, style)
	// `C:` alone takes no separator.
	if c.prefix.len > 0 && c.prefix.len == len(base) && is_drive(c.prefix) {
		need_sep = false
	}
	sep := main_separator(style)
	b := strings.builder_make(0, len(base) + 1 + len(name), allocator)
	if is_absolute(name, style) || parse_prefix(name, style).kind != .None {
		// An absolute `name` replaces `base`.
	} else if is_verbatim(c.prefix) && len(name) > 0 {
		// Verbatim paths are rebuilt from their components, `.` and `..`
		// of `name` resolved.
		parts := make([dynamic]Component, 0, 16, context.temp_allocator)
		for comp in next(&c) {
			append(&parts, comp)
		}
		n := components(name, style)
		for comp in next(&n) {
			#partial switch comp.kind {
			case .Root_Dir:
				resize(&parts, min(len(parts), 1))
				append(&parts, comp)
			case .Cur_Dir:
			case .Parent_Dir:
				if len(parts) > 0 && parts[len(parts) - 1].kind == .Normal {
					pop(&parts)
				}
			case:
				append(&parts, comp)
			}
		}
		sep_next := false
		for comp in parts {
			if sep_next && comp.kind != .Root_Dir {
				strings.write_byte(&b, sep)
			}
			strings.write_string(&b, comp.text)
			#partial switch comp.kind {
			case .Root_Dir:
				sep_next = false
			case .Prefix:
				sep_next = !is_drive(c.prefix) && c.prefix.len > 0
			case:
				sep_next = true
			}
		}
		return strings.to_string(b)
	} else if has_root(name, style) {
		// `\windows`: keeps only `base`'s prefix.
		strings.write_string(&b, base[:prefix_remaining(&c)])
	} else {
		strings.write_string(&b, base)
		if need_sep {
			strings.write_byte(&b, sep)
		}
	}
	strings.write_string(&b, name)
	return strings.to_string(b)
}
