// A path's text and its UTF-16 form, as Rust's OsStr has them. A path is
// the host's bytes: whatever the file system holds on Unix, WTF-8 on
// Windows (UTF-8 that may also hold unpaired surrogates, which is what
// `as_encoded_bytes` gives the core there).
package platform_fs

import "core:strings"
import "core:unicode/utf8"

// `Path::display` (and `to_string_lossy`). Unix: the bytes as UTF-8, each
// maximal invalid run replaced by U+FFFD (String::from_utf8_lossy). Windows:
// each unpaired surrogate is one U+FFFD (Wtf8::to_string_lossy); bytes that
// aren't WTF-8 at all, which no Rust path holds, as on Unix.
display :: proc(path: string, style := HOST, allocator := context.allocator) -> string {
	if utf8.valid_string(path) {
		return strings.clone(path, allocator)
	}
	b := strings.builder_make(0, len(path) + 8, allocator)
	write_lossy(&b, path, style)
	return strings.to_string(b)
}

// The text of `s` as `display` makes it.
write_lossy :: proc(b: ^strings.Builder, s: string, style := HOST) {
	i := 0
	for i < len(s) {
		c := s[i]
		if c < 0x80 {
			strings.write_byte(b, c)
			i += 1
			continue
		}
		// A WTF-8 surrogate: ED A0..BF 80..BF.
		if style == .Windows && c == 0xED && i + 2 < len(s) && s[i + 1] >= 0xA0 && s[i + 1] <= 0xBF && s[i + 2] >= 0x80 && s[i + 2] <= 0xBF {
			strings.write_string(b, "�")
			i += 3
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

// MARK: UTF-16

// One code point of WTF-8 at the start of `s` (surrogates included) and
// its length; U+FFFD and 1 for a byte that doesn't start one.
@(private)
decode_wtf8 :: proc "contextless" (s: string) -> (rune, int) {
	c := s[0]
	if c < 0x80 {
		return rune(c), 1
	}
	cont :: proc "contextless" (s: string, i: int, lo, hi: u8) -> bool {
		return i < len(s) && s[i] >= lo && s[i] <= hi
	}
	switch c {
	case 0xC2 ..= 0xDF:
		if cont(s, 1, 0x80, 0xBF) {
			return rune(c & 0x1F) << 6 | rune(s[1] & 0x3F), 2
		}
	case 0xE0 ..= 0xEF:
		lo: u8 = 0xA0 if c == 0xE0 else 0x80
		if cont(s, 1, lo, 0xBF) && cont(s, 2, 0x80, 0xBF) {
			return rune(c & 0x0F) << 12 | rune(s[1] & 0x3F) << 6 | rune(s[2] & 0x3F), 3
		}
	case 0xF0 ..= 0xF4:
		lo: u8 = 0x90 if c == 0xF0 else 0x80
		hi: u8 = 0x8F if c == 0xF4 else 0xBF
		if cont(s, 1, lo, hi) && cont(s, 2, 0x80, 0xBF) && cont(s, 3, 0x80, 0xBF) {
			return rune(c & 0x07) << 18 | rune(s[1] & 0x3F) << 12 | rune(s[2] & 0x3F) << 6 | rune(s[3] & 0x3F), 4
		}
	}
	return 0xFFFD, 1
}

// `OsStr::encode_wide`: WTF-8 as UTF-16 (an unpaired surrogate as itself),
// with a NUL after it when `nul` (std's `to_u16s`). False when `s` holds a
// NUL and `nul` is asked for: "strings passed to WinAPI cannot contain
// NULs".
to_utf16 :: proc(s: string, nul: bool, allocator := context.allocator) -> ([]u16, bool) {
	out := make([dynamic]u16, 0, len(s) + 1, allocator)
	for i := 0; i < len(s); {
		r, n := decode_wtf8(s[i:])
		i += n
		if r == 0 && nul {
			delete(out)
			return nil, false
		}
		if r >= 0x1_0000 {
			r -= 0x1_0000
			append(&out, u16(0xD800 + (r >> 10)), u16(0xDC00 + (r & 0x3FF)))
		} else {
			append(&out, u16(r))
		}
	}
	if nul {
		append(&out, 0)
	}
	return out[:], true
}

// `OsString::from_wide`: UTF-16 as WTF-8 (an unpaired surrogate as its
// three bytes). `unicode` is false when it holds one, where `to_str()`
// gives None.
from_utf16 :: proc(units: []u16, allocator := context.allocator) -> (s: string, unicode: bool) {
	b := strings.builder_make(0, len(units) + len(units) / 2, allocator)
	unicode = true
	for i := 0; i < len(units); i += 1 {
		u := rune(units[i])
		if u >= 0xD800 && u < 0xDC00 && i + 1 < len(units) && units[i + 1] >= 0xDC00 && units[i + 1] < 0xE000 {
			u = 0x1_0000 + (u - 0xD800) << 10 + (rune(units[i + 1]) - 0xDC00)
			i += 1
		} else if u >= 0xD800 && u < 0xE000 {
			// utf8.encode_rune refuses surrogates: WTF-8 writes them as
			// any other three-byte code point.
			unicode = false
			strings.write_byte(&b, 0xE0 | u8(u >> 12))
			strings.write_byte(&b, 0x80 | u8(u >> 6) & 0x3F)
			strings.write_byte(&b, 0x80 | u8(u) & 0x3F)
			continue
		}
		strings.write_rune(&b, u)
	}
	return strings.to_string(b), unicode
}
