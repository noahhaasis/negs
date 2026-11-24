// Rust's string pieces the state rules use: char::to_uppercase (from
// upper_tables.odin, read off Rust's own tables), split_whitespace's
// White_Space, and str::contains.
package state

import "core:strings"
import "core:unicode/utf8"

@(private)
upper_search :: proc "contextless" (c: u32) -> (one: u32, long: [3]u32, kind: int) {
	lo, hi := 0, len(UPPER)
	for lo < hi {
		mid := (lo + hi) / 2
		if UPPER[mid][0] < c {
			lo = mid + 1
		} else {
			hi = mid
		}
	}
	if lo < len(UPPER) && UPPER[lo][0] == c {
		return UPPER[lo][1], {}, 1
	}
	lo, hi = 0, len(UPPER_LONG)
	for lo < hi {
		mid := (lo + hi) / 2
		if UPPER_LONG[mid][0] < c {
			lo = mid + 1
		} else {
			hi = mid
		}
	}
	if lo < len(UPPER_LONG) && UPPER_LONG[lo][0] == c {
		row := UPPER_LONG[lo]
		return 0, {row[1], row[2], row[3]}, 2
	}
	return c, {}, 0
}

// char::to_uppercase(r), written out.
write_upper :: proc(b: ^strings.Builder, r: rune) {
	one, long, kind := upper_search(u32(r))
	switch kind {
	case 1:
		strings.write_rune(b, rune(one))
	case 2:
		for c in long {
			if c != 0 {
				strings.write_rune(b, rune(c))
			}
		}
	case:
		strings.write_rune(b, r)
	}
}

// `first.to_uppercase().chain(rest)`: `s` with its first char upper-cased.
write_capitalised :: proc(b: ^strings.Builder, s: string) {
	if len(s) == 0 {
		return
	}
	r, n := utf8.decode_rune_in_string(s)
	write_upper(b, r)
	strings.write_string(b, s[n:])
}

// char::is_whitespace: Unicode's White_Space.
is_white_space :: proc "contextless" (r: rune) -> bool {
	switch r {
	case '\t', '\n', '\v', '\f', '\r', ' ', 0x85, 0xa0, 0x1680, 0x2000 ..= 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000:
		return true
	}
	return false
}

// str::split_whitespace's words, borrowed from `s`, in `allocator`.
split_whitespace :: proc(s: string, allocator := context.allocator) -> [dynamic]string {
	out := make([dynamic]string, 0, 4, allocator)
	start := -1
	for r, i in s {
		if is_white_space(r) {
			if start >= 0 {
				append(&out, s[start:i])
				start = -1
			}
		} else if start < 0 {
			start = i
		}
	}
	if start >= 0 {
		append(&out, s[start:])
	}
	return out
}
