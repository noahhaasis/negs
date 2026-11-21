// Finder's name order (`localizedStandardCompare`), as folder.rs has it:
// numbers by value, case and accents only when nothing else differs (lower
// case first), punctuation before digits before letters. Exact for Latin
// file names; other scripts sort by character.
package library

import "core:strings"
import "core:unicode"
import "core:unicode/utf8"

// Where a character sorts before letters: spaces, then punctuation and
// symbols in this order, then digits and letters.
@(private = "file")
PUNCTUATION :: "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$"

@(private = "file")
Primary_Kind :: enum u8 {
	Space,
	Punctuation,
	Other,
	Number,
	Letter,
}

@(private = "file")
Element :: struct {
	kind:   Primary_Kind,
	// The punctuation's rank, the other character, the letter, or the
	// number's length without leading zeros.
	value:  u32,
	// A number's digits without leading zeros.
	digits: string,
	accent: u32,
	// Upper case sorts after lower.
	upper:  bool,
	// Leading zeros of a number.
	zeros:  int,
}

@(private = "file")
ACCENTS := [19]struct {
	base:     rune,
	accented: string,
} {
	{'a', "àáâãäåāăą"},
	{'c', "çćĉċč"},
	{'d', "ďđ"},
	{'e', "èéêëēĕėęě"},
	{'g', "ĝğġģ"},
	{'h', "ĥħ"},
	{'i', "ìíîïĩīĭįı"},
	{'j', "ĵ"},
	{'k', "ķ"},
	{'l', "ĺļľŀł"},
	{'n', "ñńņňŉ"},
	{'o', "òóôõöøōŏő"},
	{'r', "ŕŗř"},
	{'s', "śŝşš"},
	{'t', "ţťŧ"},
	{'u', "ùúûüũūŭůűų"},
	{'w', "ŵ"},
	{'y', "ýÿŷ"},
	{'z', "źżž"},
}

// An accented Latin letter's base letter and accent, or the letter itself.
@(private = "file")
base_letter :: proc(c: rune) -> (rune, u32) {
	lower := unicode.to_lower(c)
	for entry in ACCENTS {
		position: u32 = 0
		for a in entry.accented {
			position += 1
			if a == lower {
				return entry.base, position
			}
		}
	}
	return lower, 0
}

// Rust's `char::is_whitespace` (Unicode White_Space).
is_rust_whitespace :: proc(r: rune) -> bool {
	switch r {
	case 0x09 ..= 0x0D, 0x20, 0x85, 0xA0, 0x1680, 0x2000 ..= 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return false
}

@(private = "file")
elements :: proc(name: string) -> []Element {
	out := make([dynamic]Element, 0, len(name), context.temp_allocator)
	i := 0
	for i < len(name) {
		if name[i] >= '0' && name[i] <= '9' {
			start := i
			for i < len(name) && name[i] >= '0' && name[i] <= '9' {
				i += 1
			}
			run := name[start:i]
			trimmed := strings.trim_left(run, "0")
			append(&out, Element{kind = .Number, value = u32(len(trimmed)), digits = trimmed, zeros = len(run) - len(trimmed)})
			continue
		}
		c, width := utf8.decode_rune_in_string(name[i:])
		i += width
		if is_rust_whitespace(c) {
			append(&out, Element{kind = .Space})
		} else if rank := strings.index_rune(PUNCTUATION, c); rank >= 0 {
			append(&out, Element{kind = .Punctuation, value = u32(rank)})
		} else if unicode.is_letter(c) {
			base, accent := base_letter(c)
			append(&out, Element{kind = .Letter, value = u32(base), accent = accent, upper = unicode.is_upper(c)})
		} else {
			append(&out, Element{kind = .Other, value = u32(c)})
		}
	}
	return out[:]
}

@(private = "file")
cmp :: proc(a, b: $T) -> int {
	if a < b {
		return -1
	}
	if a > b {
		return 1
	}
	return 0
}

@(private = "file")
primary_cmp :: proc(a, b: Element) -> int {
	if a.kind != b.kind {
		return cmp(u8(a.kind), u8(b.kind))
	}
	if c := cmp(a.value, b.value); c != 0 {
		return c
	}
	if a.kind == .Number {
		return strings.compare(a.digits, b.digits)
	}
	return 0
}

// Lexicographic over the sequences, as Rust's `Iterator::cmp`.
@(private = "file")
sequence_cmp :: proc(x, y: []Element, field: proc(a, b: Element) -> int) -> int {
	for i in 0 ..< min(len(x), len(y)) {
		if c := field(x[i], y[i]); c != 0 {
			return c
		}
	}
	return cmp(len(x), len(y))
}

// -1, 0 or 1 as `a` sorts before, with or after `b`.
standard_compare :: proc(a, b: string) -> int {
	x, y := elements(a), elements(b)
	if c := sequence_cmp(x, y, primary_cmp); c != 0 {
		return c
	}
	if c := sequence_cmp(x, y, proc(p, q: Element) -> int {return cmp(p.accent, q.accent)}); c != 0 {
		return c
	}
	if c := sequence_cmp(x, y, proc(p, q: Element) -> int {return cmp(u8(p.upper), u8(q.upper))}); c != 0 {
		return c
	}
	if c := sequence_cmp(x, y, proc(p, q: Element) -> int {return cmp(p.zeros, q.zeros)}); c != 0 {
		return c
	}
	return strings.compare(a, b)
}
