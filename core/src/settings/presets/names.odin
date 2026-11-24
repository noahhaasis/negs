// presets.rs's `name` module: one naming policy on every platform, names
// Windows, macOS and Linux all accept as file names, compared without
// regard to case or Unicode form.
//
// Case and Unicode form come from the host (`set_text_functions`): NFC is
// Windows' NormalizeString there and ICU in the differential tests, lower
// case Rust's `str::to_lowercase`; the core keeps no Unicode tables. Until
// the host registers them, NFC leaves text as it is and lower case is
// core:unicode's per character (Rust's but for the final sigma).
package presets

import "core:strings"
import "core:unicode"
import "core:unicode/utf8"

// size_t (*negs_text_fn)(void *user, const uint8_t *text, size_t len,
// uint8_t *out, size_t cap): writes up to `cap` bytes of the result and
// returns its whole length.
Text_Fn :: #type proc "c" (user: rawptr, text: [^]u8, len: uint, out: [^]u8, cap: uint) -> uint

@(private)
text_nfc: Text_Fn
@(private)
text_lower: Text_Fn
@(private)
text_user: rawptr

// Registers the host's NFC and lower-case functions (nil for the core's
// stand-ins). Process-wide; register before the first call that compares
// names.
set_text_functions :: proc "contextless" (nfc, lower: Text_Fn, user: rawptr) {
	text_nfc = nfc
	text_lower = lower
	text_user = user
}

@(private)
call_text :: proc(f: Text_Fn, text: string, allocator := context.temp_allocator) -> string {
	buf := make([]u8, len(text) * 3 + 16, allocator)
	n := f(text_user, raw_data(text), uint(len(text)), raw_data(buf), uint(len(buf)))
	if int(n) > len(buf) {
		buf = make([]u8, int(n), allocator)
		n = f(text_user, raw_data(text), uint(len(text)), raw_data(buf), uint(len(buf)))
		n = min(n, uint(len(buf)))
	}
	return string(buf[:n])
}

nfc :: proc(text: string, allocator := context.temp_allocator) -> string {
	if text_nfc == nil || text == "" {
		return strings.clone(text, allocator)
	}
	return call_text(text_nfc, text, allocator)
}

to_lower :: proc(text: string, allocator := context.temp_allocator) -> string {
	if text_lower != nil {
		return call_text(text_lower, text, allocator)
	}
	b := strings.builder_make(0, len(text), allocator)
	for r in text {
		if r == 0x130 {
			// İ: "i" and a combining dot above.
			strings.write_string(&b, "i̇")
		} else {
			strings.write_rune(&b, unicode.to_lower(r))
		}
	}
	return strings.to_string(b)
}

MAX_LENGTH :: 120

@(private)
FORBIDDEN :: "/\\:*?\"<>|"

@(private)
forbidden :: proc "contextless" (r: rune) -> bool {
	switch r {
	case '/', '\\', ':', '*', '?', '"', '<', '>', '|':
		return true
	}
	return false
}

// `char::is_whitespace` (White_Space), as `str::trim` takes it.
@(private)
is_white_space :: proc "contextless" (r: rune) -> bool {
	switch r {
	case '\t', '\n', '\v', '\f', '\r', ' ', 0x85, 0xA0, 0x1680, 0x2000 ..= 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return false
}

@(private)
trim :: proc(s: string) -> string {
	return strings.trim_right_proc(strings.trim_left_proc(s, is_white_space_c), is_white_space_c)
}

@(private)
is_white_space_c :: proc(r: rune) -> bool {
	return is_white_space(r)
}

// `char::is_control`: general category Cc.
@(private)
is_control :: proc "contextless" (r: rune) -> bool {
	return r < 0x20 || (r >= 0x7F && r <= 0x9F)
}

// Why `name` can't name a preset, or false.
problem :: proc(name: string, allocator := context.temp_allocator) -> (string, bool) {
	if trim(name) == "" {
		return "Enter a name.", true
	}
	if utf8.rune_count_in_string(name) > MAX_LENGTH {
		return "Use at most 120 characters.", true
	}
	for r in name {
		if is_control(r) || forbidden(r) {
			return "A name can’t contain / \\ : * ? \" < > or |.", true
		}
	}
	if strings.has_prefix(name, ".") {
		return "A name can’t start with a period.", true
	}
	if strings.has_suffix(name, ".") || strings.has_suffix(name, " ") || strings.has_prefix(name, " ") {
		return "A name can’t start with a space or end with a space or period.", true
	}
	// Device names, also with an extension: "NUL.json" is NUL. Only ASCII
	// letters upper-case to the letters these use, so ASCII upper case is
	// `to_uppercase` here.
	first := name
	if dot := strings.index_byte(name, '.'); dot >= 0 {
		first = name[:dot]
	}
	stem := strings.to_upper(trim(first), context.temp_allocator)
	numbered :: proc(stem, prefix: string) -> bool {
		if !strings.has_prefix(stem, prefix) {
			return false
		}
		n := stem[len(prefix):]
		return len(n) == 1 && n[0] >= '1' && n[0] <= '9'
	}
	if stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL" || numbered(stem, "COM") || numbered(stem, "LPT") {
		return strings.concatenate({"“", name, "” is reserved on Windows."}, allocator), true
	}
	return "", false
}

// Names that are the same preset: NFC-normalized and lower-cased. NTFS
// compares names without case but doesn't normalize them.
key :: proc(name: string, allocator := context.temp_allocator) -> string {
	return to_lower(nfc(name, context.temp_allocator), allocator)
}

// `name`, or "name 2", "name 3"… the first whose key isn't `taken`.
unique :: proc(name: string, taken: map[string]bool, allocator := context.temp_allocator) -> string {
	if key(name) not_in taken {
		return strings.clone(name, allocator)
	}
	n := 2
	for {
		candidate := numbered_name(name, n)
		if key(candidate) not_in taken {
			return strings.clone(candidate, allocator)
		}
		n += 1
	}
}

@(private)
numbered_name :: proc(name: string, n: int) -> string {
	b := strings.builder_make(0, len(name) + 8, context.temp_allocator)
	strings.write_string(&b, name)
	strings.write_byte(&b, ' ')
	strings.write_int(&b, n)
	return strings.to_string(b)
}

// A file's name as a preset name: invalid characters become spaces.
from_file_name :: proc(file_name: string, allocator := context.temp_allocator) -> string {
	trim_dots :: proc(s: string) -> string {
		is_space_or_dot :: proc(r: rune) -> bool {
			return r == ' ' || r == '.'
		}
		return strings.trim_right_proc(strings.trim_left_proc(s, is_space_or_dot), is_space_or_dot)
	}
	b := strings.builder_make(0, len(file_name), context.temp_allocator)
	for r in file_name {
		if forbidden(r) || r == '\n' || r == '\r' {
			strings.write_byte(&b, ' ')
		} else {
			strings.write_rune(&b, r)
		}
	}
	name, _ := strings.replace_all(trim_dots(strings.to_string(b)), "  ", " ", context.temp_allocator)
	if utf8.rune_count_in_string(name) > MAX_LENGTH {
		end := 0
		count := 0
		for _, i in name {
			if count == MAX_LENGTH {
				end = i
				break
			}
			count += 1
		}
		name = trim_dots(name[:end])
	}
	if _, bad := problem(name); bad {
		return strings.clone("Preset", allocator)
	}
	return strings.clone(name, allocator)
}
