// Reading JSON as `JSONDecoder` does (and as json.rs's `Json::parse`):
// strict JSON, plus a leading byte order mark and trailing commas, which it
// accepts. The first of two equal keys wins; a number out of a double's
// range is refused.
package library

import "core:math"
import "core:slice"
import "core:strconv"
import "core:strings"
import "core:unicode/utf8"

Parse_Error :: struct {
	offset: int,
	reason: string,
}

@(private = "file")
Parser :: struct {
	s:         []u8,
	i:         int,
	allocator: Allocator,
}

// Parses `bytes` into a tree in `allocator`.
json_parse :: proc(bytes: []u8, allocator := context.allocator) -> (Json, Maybe(Parse_Error)) {
	if bad, ok := utf8_invalid_at(bytes); ok {
		return nil, Parse_Error{bad, "invalid UTF-8"}
	}
	p := Parser{s = bytes, allocator = allocator}
	if len(bytes) >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
		p.i = 3
	}
	ws(&p)
	value, err := parse_value(&p, 0)
	if err != nil {
		return nil, err
	}
	ws(&p)
	if p.i != len(p.s) {
		json_destroy(value, allocator)
		return nil, parse_error(&p, "unexpected text after the value")
	}
	return value, nil
}

// Where `bytes` stop being UTF-8 as Rust's `str::from_utf8` sees it
// (no surrogates, no overlong forms, nothing above U+10FFFF).
utf8_invalid_at :: proc(bytes: []u8) -> (int, bool) {
	i := 0
	n := len(bytes)
	for i < n {
		b := bytes[i]
		if b < 0x80 {
			i += 1
			continue
		}
		width := 0
		lo, hi: u8 = 0x80, 0xBF
		switch b {
		case 0xC2 ..= 0xDF:
			width = 2
		case 0xE0:
			width, lo = 3, 0xA0
		case 0xE1 ..= 0xEC, 0xEE ..= 0xEF:
			width = 3
		case 0xED:
			width, hi = 3, 0x9F
		case 0xF0:
			width, lo = 4, 0x90
		case 0xF1 ..= 0xF3:
			width = 4
		case 0xF4:
			width, hi = 4, 0x8F
		case:
			return i, true
		}
		if i + 1 >= n || bytes[i + 1] < lo || bytes[i + 1] > hi {
			return i, true
		}
		for k in 2 ..< width {
			if i + k >= n || bytes[i + k] < 0x80 || bytes[i + k] > 0xBF {
				return i, true
			}
		}
		i += width
	}
	return 0, false
}

@(private = "file")
parse_error :: proc(p: ^Parser, reason: string) -> Maybe(Parse_Error) {
	return Parse_Error{p.i, reason}
}

@(private = "file")
ws :: proc(p: ^Parser) {
	for p.i < len(p.s) {
		switch p.s[p.i] {
		case ' ', '\t', '\n', '\r':
			p.i += 1
		case:
			return
		}
	}
}

@(private = "file")
peek :: proc(p: ^Parser) -> (u8, bool) {
	if p.i < len(p.s) {
		return p.s[p.i], true
	}
	return 0, false
}

@(private = "file")
literal :: proc(p: ^Parser, word: string, value: Json) -> (Json, Maybe(Parse_Error)) {
	if strings.has_prefix(string(p.s[p.i:]), word) {
		p.i += len(word)
		return value, nil
	}
	return nil, parse_error(p, "unexpected character")
}

@(private = "file")
parse_value :: proc(p: ^Parser, depth: int) -> (Json, Maybe(Parse_Error)) {
	if depth > 512 {
		return nil, parse_error(p, "too deeply nested")
	}
	c, ok := peek(p)
	if !ok {
		return nil, parse_error(p, "unexpected end")
	}
	switch c {
	case '{':
		return parse_object(p, depth)
	case '[':
		return parse_array(p, depth)
	case '"':
		s, err := parse_string(p)
		if err != nil {
			return nil, err
		}
		return s, nil
	case 't':
		return literal(p, "true", true)
	case 'f':
		return literal(p, "false", false)
	case 'n':
		return literal(p, "null", nil)
	case '-', '0' ..= '9':
		return parse_number(p)
	}
	return nil, parse_error(p, "unexpected character")
}

// Keys sorted, the first of equal ones kept (a stable sort keeps them in
// the order read).
@(private = "file")
finish_object :: proc(fields: ^Json_Object, allocator: Allocator) {
	slice.stable_sort_by(fields[:], proc(a, b: Json_Field) -> bool {return a.key < b.key})
	out := 0
	for i in 0 ..< len(fields) {
		if out > 0 && fields[out - 1].key == fields[i].key {
			delete(fields[i].key, allocator)
			json_destroy(fields[i].value, allocator)
			continue
		}
		fields[out] = fields[i]
		out += 1
	}
	resize(fields, out)
}

@(private = "file")
parse_object :: proc(p: ^Parser, depth: int) -> (result: Json, err: Maybe(Parse_Error)) {
	p.i += 1
	fields := make(Json_Object, 0, 4, p.allocator)
	defer if err != nil {
		json_destroy(fields, p.allocator)
	}
	ws(p)
	if c, _ := peek(p); c == '}' {
		p.i += 1
		return fields, nil
	}
	for {
		ws(p)
		if c, _ := peek(p); c != '"' || p.i >= len(p.s) {
			return nil, parse_error(p, "expected a key")
		}
		key := parse_string(p) or_return
		ws(p)
		if c, _ := peek(p); c != ':' || p.i >= len(p.s) {
			delete(key, p.allocator)
			return nil, parse_error(p, "expected ':'")
		}
		p.i += 1
		ws(p)
		value, value_err := parse_value(p, depth + 1)
		if value_err != nil {
			delete(key, p.allocator)
			return nil, value_err
		}
		append(&fields, Json_Field{key, value})
		ws(p)
		c, ok := peek(p)
		if ok && c == ',' {
			p.i += 1
			ws(p)
			if next, _ := peek(p); next == '}' && p.i < len(p.s) {
				p.i += 1
				finish_object(&fields, p.allocator)
				return fields, nil
			}
		} else if ok && c == '}' {
			p.i += 1
			finish_object(&fields, p.allocator)
			return fields, nil
		} else {
			return nil, parse_error(p, "expected ',' or '}'")
		}
	}
}

@(private = "file")
parse_array :: proc(p: ^Parser, depth: int) -> (result: Json, err: Maybe(Parse_Error)) {
	p.i += 1
	items := make(Json_Array, 0, 4, p.allocator)
	defer if err != nil {
		json_destroy(items, p.allocator)
	}
	ws(p)
	if c, ok := peek(p); ok && c == ']' {
		p.i += 1
		return items, nil
	}
	for {
		ws(p)
		value := parse_value(p, depth + 1) or_return
		append(&items, value)
		ws(p)
		c, ok := peek(p)
		if ok && c == ',' {
			p.i += 1
			ws(p)
			if next, more := peek(p); more && next == ']' {
				p.i += 1
				return items, nil
			}
		} else if ok && c == ']' {
			p.i += 1
			return items, nil
		} else {
			return nil, parse_error(p, "expected ',' or ']'")
		}
	}
}

// Four hex digits as Rust's `u32::from_str_radix(text, 16)` reads them,
// which also takes a leading '+'.
@(private = "file")
hex4 :: proc(p: ^Parser) -> (u32, Maybe(Parse_Error)) {
	if p.i + 4 > len(p.s) {
		return 0, parse_error(p, "short \\u escape")
	}
	digits := p.s[p.i:p.i + 4]
	if _, bad := utf8_invalid_at(digits); bad {
		return 0, parse_error(p, "bad \\u escape")
	}
	if digits[0] == '+' {
		digits = digits[1:]
	}
	value: u32 = 0
	for c in digits {
		d: u32
		switch c {
		case '0' ..= '9':
			d = u32(c - '0')
		case 'a' ..= 'f':
			d = u32(c - 'a' + 10)
		case 'A' ..= 'F':
			d = u32(c - 'A' + 10)
		case:
			return 0, parse_error(p, "bad \\u escape")
		}
		value = value * 16 + d
	}
	p.i += 4
	return value, nil
}

@(private = "file")
parse_string :: proc(p: ^Parser) -> (result: string, err: Maybe(Parse_Error)) {
	p.i += 1
	out := strings.builder_make(0, 16, p.allocator)
	defer if err != nil {
		strings.builder_destroy(&out)
	}
	for {
		start := p.i
		for p.i < len(p.s) {
			b := p.s[p.i]
			if b == '"' || b == '\\' || b < 0x20 {
				break
			}
			p.i += 1
		}
		strings.write_bytes(&out, p.s[start:p.i])
		c, ok := peek(p)
		if !ok {
			return "", parse_error(p, "unterminated string")
		}
		switch c {
		case '"':
			p.i += 1
			return strings.to_string(out), nil
		case '\\':
			p.i += 1
			escape, more := peek(p)
			if !more {
				return "", parse_error(p, "unterminated string")
			}
			p.i += 1
			switch escape {
			case '"':
				strings.write_byte(&out, '"')
			case '\\':
				strings.write_byte(&out, '\\')
			case '/':
				strings.write_byte(&out, '/')
			case 'b':
				strings.write_byte(&out, 0x08)
			case 'f':
				strings.write_byte(&out, 0x0C)
			case 'n':
				strings.write_byte(&out, '\n')
			case 'r':
				strings.write_byte(&out, '\r')
			case 't':
				strings.write_byte(&out, '\t')
			case 'u':
				first := hex4(p) or_return
				code: u32
				if first >= 0xD800 && first < 0xDC00 {
					if p.i + 2 > len(p.s) || p.s[p.i] != '\\' || p.s[p.i + 1] != 'u' {
						return "", parse_error(p, "lone surrogate")
					}
					p.i += 2
					second := hex4(p) or_return
					if second < 0xDC00 || second >= 0xE000 {
						return "", parse_error(p, "lone surrogate")
					}
					code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
				} else if first >= 0xDC00 && first < 0xE000 {
					return "", parse_error(p, "lone surrogate")
				} else {
					code = first
				}
				bytes, n := utf8.encode_rune(rune(code))
				strings.write_bytes(&out, bytes[:n])
			case:
				return "", parse_error(p, "bad escape")
			}
		case:
			return "", parse_error(p, "control character in a string")
		}
	}
}

@(private = "file")
digits :: proc(p: ^Parser) -> int {
	from := p.i
	for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
		p.i += 1
	}
	return p.i - from
}

@(private = "file")
parse_number :: proc(p: ^Parser) -> (Json, Maybe(Parse_Error)) {
	start := p.i
	if c, _ := peek(p); c == '-' && p.i < len(p.s) {
		p.i += 1
	}
	int_start := p.i
	whole := digits(p)
	if whole == 0 {
		return nil, parse_error(p, "bad number")
	}
	if whole > 1 && p.s[int_start] == '0' {
		return nil, parse_error(p, "leading zero")
	}
	if c, ok := peek(p); ok && c == '.' {
		p.i += 1
		if digits(p) == 0 {
			return nil, parse_error(p, "bad number")
		}
	}
	mantissa_end := p.i
	if c, ok := peek(p); ok && (c == 'e' || c == 'E') {
		p.i += 1
		if s, more := peek(p); more && (s == '+' || s == '-') {
			p.i += 1
		}
		if digits(p) == 0 {
			return nil, parse_error(p, "bad number")
		}
	}
	text := string(p.s[start:p.i])
	value, _ := strconv.parse_f64(text)
	nonzero := false
	for b in p.s[start:mantissa_end] {
		if b >= '1' && b <= '9' {
			nonzero = true
			break
		}
	}
	if math.is_inf(value) || (value == 0 && nonzero) {
		return nil, Parse_Error{start, "number out of range"}
	}
	return value, nil
}
