// Rust's string casing and trimming (str::trim, split_whitespace,
// to_lowercase with its final sigma, to_uppercase), from the tables
// case_tables.odin reads off Rust's own char methods.
package facts

import "core:strings"
import "core:unicode/utf8"

is_white :: proc "contextless" (r: rune) -> bool {
	for w in WHITE_SPACE {
		if u32(r) == w {
			return true
		}
	}
	return false
}

// str::trim.
trim :: proc(s: string) -> string {
	start, end := 0, len(s)
	for start < end {
		r, n := utf8.decode_rune_in_string(s[start:])
		if !is_white(r) {
			break
		}
		start += n
	}
	for end > start {
		r, n := utf8.decode_last_rune_in_string(s[start:end])
		if !is_white(r) {
			break
		}
		end -= n
	}
	return s[start:end]
}

// The first of split_whitespace's words.
first_word :: proc(s: string) -> (string, bool) {
	start := 0
	for start < len(s) {
		r, n := utf8.decode_rune_in_string(s[start:])
		if !is_white(r) {
			break
		}
		start += n
	}
	if start == len(s) {
		return "", false
	}
	end := start
	for end < len(s) {
		r, n := utf8.decode_rune_in_string(s[end:])
		if is_white(r) {
			break
		}
		end += n
	}
	return s[start:end], true
}

@(private)
in_ranges :: proc "contextless" (table: [][2]u32, c: u32) -> bool {
	lo, hi := 0, len(table)
	for lo < hi {
		mid := (lo + hi) / 2
		if c < table[mid][0] {
			hi = mid
		} else if c > table[mid][1] {
			lo = mid + 1
		} else {
			return true
		}
	}
	return false
}

// Whether char::to_uppercase leaves `r` as it is.
upper_is_self :: proc "contextless" (r: rune) -> bool {
	return !in_ranges(UPPER_CHANGES[:], u32(r))
}

// str::to_uppercase(s) == s: every char its own uppercase (one that grows
// can't be matched by the others, which never shrink).
is_shouted :: proc "contextless" (s: string) -> bool {
	for r in s {
		if !upper_is_self(r) {
			return false
		}
	}
	return true
}

// 1 cased, 2 case-ignorable, 0 neither (SIGMA_CLASS).
@(private)
sigma_class :: proc "contextless" (r: rune) -> u32 {
	c := u32(r)
	lo, hi := 0, len(SIGMA_CLASS)
	for lo < hi {
		mid := (lo + hi) / 2
		if c < SIGMA_CLASS[mid][0] {
			hi = mid
		} else if c > SIGMA_CLASS[mid][1] {
			lo = mid + 1
		} else {
			return SIGMA_CLASS[mid][2]
		}
	}
	return 0
}

@(private)
write_lower_char :: proc(b: ^strings.Builder, r: rune) {
	c := u32(r)
	lo, hi := 0, len(LOWER)
	for lo < hi {
		mid := (lo + hi) / 2
		if c < LOWER[mid][0] {
			hi = mid
		} else if c > LOWER[mid][0] {
			lo = mid + 1
		} else {
			strings.write_rune(b, rune(LOWER[mid][1]))
			return
		}
	}
	for row in LOWER_LONG {
		if row[0] == c {
			for k in 1 ..< 4 {
				if x := row[k]; x != 0 {
					strings.write_rune(b, rune(x))
				}
			}
			return
		}
	}
	strings.write_rune(b, r)
}

// str::to_lowercase's case_ignorable_then_cased, looking back from `end`
// (backwards) or on from `start`.
@(private)
cased_before :: proc(s: string, end: int) -> bool {
	i := end
	for i > 0 {
		r, n := utf8.decode_last_rune_in_string(s[:i])
		k := sigma_class(r)
		if k != 2 {
			return k == 1
		}
		i -= n
	}
	return false
}

@(private)
cased_after :: proc(s: string, start: int) -> bool {
	i := start
	for i < len(s) {
		r, n := utf8.decode_rune_in_string(s[i:])
		k := sigma_class(r)
		if k != 2 {
			return k == 1
		}
		i += n
	}
	return false
}

// str::to_lowercase(s) written to `b`: Σ is ς at the end of a word.
write_lower :: proc(b: ^strings.Builder, s: string) {
	for r, i in s {
		if r == 'Σ' {
			strings.write_rune(b, 'ς' if cased_before(s, i) && !cased_after(s, i + 2) else 'σ')
		} else {
			write_lower_char(b, r)
		}
	}
}

// str::to_lowercase, in the context's allocator.
lower :: proc(s: string, allocator := context.allocator) -> string {
	b := strings.builder_make(0, len(s), allocator)
	write_lower(&b, s)
	return strings.to_string(b)
}
