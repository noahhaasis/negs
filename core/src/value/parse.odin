// Reading as `serde_json::from_slice` does (serde_json 1.0.149, feature
// `float_roundtrip`): the same texts accepted and rejected, the same tree.
//
// - Whitespace is space, \n, \t and \r. After the value only whitespace.
// - Nesting: 128 levels of arrays and objects fail (serde_json's
//   `remaining_depth`); 127 are fine.
// - Strings: valid UTF-8, no control characters (below 0x20) unescaped,
//   escapes \" \\ \/ \b \f \n \r \t \uXXXX, a \u surrogate only as a
//   leading + trailing pair.
// - Numbers: JSON's grammar (no leading zeros, digits after '.' and 'e').
//   An integer that fits a u64 is a u64; a negative one that fits an i64 is
//   an i64 ("-0" is the double -0.0); anything else is the double nearest
//   its exact value (ties to even), and an error if that is infinite.
// - Objects: the last of a repeated key wins.
//
// The Parser and its pieces are exported for the struct decoders
// (core/src/state), which read fields as serde's derives do; skipped
// values go through `ignore_value`, which checks less (see there), as
// serde_json's `IgnoredAny` does.
package value

import "base:runtime"
import "core:slice"
import "core:strings"

// Why a text was refused. Only accepting or refusing has to match
// serde_json; the reasons are for people.
Error :: enum u8 {
	None,
	Eof,
	Expected_Value,
	Expected_Ident,
	Expected_Comma_Or_End,
	Expected_Colon,
	Key_Must_Be_String,
	Trailing_Comma,
	Trailing_Characters,
	Invalid_Number,
	Number_Out_Of_Range,
	Recursion_Limit,
	Control_Character,
	Invalid_Escape,
	Lone_Surrogate,
	Invalid_Utf8,
	// The struct decoders' (serde's derive errors).
	Invalid_Type,
	Invalid_Value,
	Invalid_Length,
	Missing_Field,
	Duplicate_Field,
	Unknown_Variant,
}

// serde_json's starting `remaining_depth`.
RECURSION_LIMIT :: 128

Parser :: struct {
	s:         []u8,
	i:         int,
	depth:     int,
	err:       Error,
	allocator: runtime.Allocator,
}

parser_make :: proc(s: []u8, allocator := context.allocator) -> Parser {
	return Parser{s = s, depth = RECURSION_LIMIT, allocator = allocator}
}

// Records the first error; always false, for `return fail(p, ...)`.
fail :: proc "contextless" (p: ^Parser, e: Error) -> bool {
	if p.err == .None {
		p.err = e
	}
	return false
}

// The next byte after whitespace, not taken.
peek_ws :: proc "contextless" (p: ^Parser) -> (u8, bool) {
	for p.i < len(p.s) {
		switch p.s[p.i] {
		case ' ', '\n', '\t', '\r':
			p.i += 1
		case:
			return p.s[p.i], true
		}
	}
	return 0, false
}

@(private)
peek :: proc "contextless" (p: ^Parser) -> u8 {
	return p.s[p.i] if p.i < len(p.s) else 0
}

// Into an array or object: serde_json's `check_recursion`.
enter :: proc "contextless" (p: ^Parser) -> bool {
	p.depth -= 1
	if p.depth == 0 {
		return fail(p, .Recursion_Limit)
	}
	return true
}

leave :: proc "contextless" (p: ^Parser) {
	p.depth += 1
}

// The rest of `null`, `true` or `false` after its first letter.
parse_ident :: proc "contextless" (p: ^Parser, rest: string) -> bool {
	for c in transmute([]u8)rest {
		if p.i >= len(p.s) {
			return fail(p, .Eof)
		}
		if p.s[p.i] != c {
			return fail(p, .Expected_Ident)
		}
		p.i += 1
	}
	return true
}

// Nothing but whitespace left: `Deserializer::end`.
finish :: proc "contextless" (p: ^Parser) -> bool {
	if _, more := peek_ws(p); more {
		return fail(p, .Trailing_Characters)
	}
	return true
}

// MARK: Sequences and maps, as serde_json's SeqAccess and MapAccess

// `has_next_element`: whether another element follows (after '[' or the
// last element). `first` starts true.
seq_next :: proc "contextless" (p: ^Parser, first: ^bool) -> (more: bool, ok: bool) {
	c, any := peek_ws(p)
	if !any {
		return false, fail(p, .Eof)
	}
	if c == ']' {
		return false, true
	}
	if c == ',' && !first^ {
		p.i += 1
		c, any = peek_ws(p)
	} else if first^ {
		first^ = false
	} else {
		return false, fail(p, .Expected_Comma_Or_End)
	}
	if !any {
		return false, fail(p, .Eof)
	}
	if c == ']' {
		return false, fail(p, .Trailing_Comma)
	}
	return true, true
}

// `end_seq`: the ']' after the elements a visitor took.
seq_end :: proc "contextless" (p: ^Parser) -> bool {
	c, any := peek_ws(p)
	switch {
	case !any:
		return fail(p, .Eof)
	case c == ']':
		p.i += 1
		return true
	case c == ',':
		p.i += 1
		if d, _ := peek_ws(p); d == ']' {
			return fail(p, .Trailing_Comma)
		}
		return fail(p, .Trailing_Characters)
	}
	return fail(p, .Trailing_Characters)
}

// `has_next_key`: whether another key follows; the next byte is then its
// opening quote, not taken.
map_next :: proc "contextless" (p: ^Parser, first: ^bool) -> (more: bool, ok: bool) {
	c, any := peek_ws(p)
	if !any {
		return false, fail(p, .Eof)
	}
	if c == '}' {
		return false, true
	}
	if c == ',' && !first^ {
		p.i += 1
		c, any = peek_ws(p)
	} else if first^ {
		first^ = false
	} else {
		return false, fail(p, .Expected_Comma_Or_End)
	}
	switch {
	case !any:
		return false, fail(p, .Eof)
	case c == '"':
		return true, true
	case c == '}':
		return false, fail(p, .Trailing_Comma)
	}
	return false, fail(p, .Key_Must_Be_String)
}

// A key (validated, as MapKey reads it) and the colon after it.
map_key :: proc(p: ^Parser) -> (key: string, ok: bool) {
	p.i += 1
	key = parse_str(p) or_return
	if c, any := peek_ws(p); !any {
		return key, fail(p, .Eof)
	} else if c != ':' {
		return key, fail(p, .Expected_Colon)
	}
	p.i += 1
	return key, true
}

// `end_map`: the '}' after the entries a visitor took.
map_end :: proc "contextless" (p: ^Parser) -> bool {
	c, any := peek_ws(p)
	switch {
	case !any:
		return fail(p, .Eof)
	case c == '}':
		p.i += 1
		return true
	case c == ',':
		return fail(p, .Trailing_Comma)
	}
	return fail(p, .Trailing_Characters)
}

// MARK: Values

// A value, as `Value::deserialize` reads it (`deserialize_any`).
parse_value :: proc(p: ^Parser) -> (v: Value, ok: bool) {
	c, any := peek_ws(p)
	if !any {
		return nil, fail(p, .Eof)
	}
	switch c {
	case 'n':
		p.i += 1
		return nil, parse_ident(p, "ull")
	case 't':
		p.i += 1
		return true, parse_ident(p, "rue")
	case 'f':
		p.i += 1
		return false, parse_ident(p, "alse")
	case '-', '0' ..= '9':
		return parse_number(p)
	case '"':
		p.i += 1
		s := parse_str(p) or_return
		return s, true
	case '[':
		enter(p) or_return
		p.i += 1
		items := make(Array, 0, 4, p.allocator)
		first := true
		for {
			more, good := seq_next(p, &first)
			if !good {
				destroy(items, p.allocator)
				return nil, false
			}
			if !more {
				break
			}
			item, parsed := parse_value(p)
			if !parsed {
				destroy(items, p.allocator)
				return nil, false
			}
			append(&items, item)
		}
		leave(p)
		if !seq_end(p) {
			destroy(items, p.allocator)
			return nil, false
		}
		return items, true
	case '{':
		enter(p) or_return
		p.i += 1
		fields := make([dynamic]Field, 0, 4, p.allocator)
		first := true
		good := true
		for good {
			more: bool
			more, good = map_next(p, &first)
			if !good || !more {
				break
			}
			key: string
			key, good = map_key(p)
			if !good {
				delete(key, p.allocator)
				break
			}
			item: Value
			item, good = parse_value(p)
			append(&fields, Field{key, item})
		}
		if !good {
			destroy(Object(fields), p.allocator)
			return nil, false
		}
		leave(p)
		o := finish_object(fields, p.allocator)
		if !map_end(p) {
			destroy(o, p.allocator)
			return nil, false
		}
		return o, true
	}
	return nil, fail(p, .Expected_Value)
}

// The fields as a BTreeMap holds them: sorted by key, the last of each key.
@(private)
finish_object :: proc(fields: [dynamic]Field, allocator: runtime.Allocator) -> Object {
	sorted := true
	for i in 1 ..< len(fields) {
		if !(fields[i - 1].key < fields[i].key) {
			sorted = false
			break
		}
	}
	if sorted {
		return Object(fields)
	}
	f := fields
	slice.stable_sort_by(f[:], proc(a, b: Field) -> bool {return a.key < b.key})
	n := 0
	for i in 0 ..< len(f) {
		if i + 1 < len(f) && f[i + 1].key == f[i].key {
			delete(f[i].key, allocator)
			destroy(f[i].value, allocator)
			continue
		}
		f[n] = f[i]
		n += 1
	}
	resize(&f, n)
	return Object(f)
}

// `serde_json::from_slice::<Value>`: the tree in `allocator`, or why not.
parse :: proc(text: []u8, allocator := context.allocator) -> (Value, Error) {
	p := parser_make(text, allocator)
	v, ok := parse_value(&p)
	if ok && finish(&p) {
		return v, .None
	}
	destroy(v, allocator)
	return nil, p.err if p.err != .None else .Expected_Value
}

parse_string :: proc(text: string, allocator := context.allocator) -> (Value, Error) {
	return parse(transmute([]u8)text, allocator)
}

// MARK: Strings

// A string's contents after its opening quote, through the closing one:
// valid UTF-8 (raw bytes and escapes), copied into the parser's allocator.
parse_str :: proc(p: ^Parser) -> (string, bool) {
	start := p.i
	// Most strings have no escapes: one scan, then one copy.
	for p.i < len(p.s) {
		c := p.s[p.i]
		if c == '"' {
			raw := p.s[start:p.i]
			p.i += 1
			if !utf8_valid(raw) {
				return "", fail(p, .Invalid_Utf8)
			}
			return strings.clone(string(raw), p.allocator), true
		}
		if c == '\\' {
			break
		}
		if c < 0x20 {
			return "", fail(p, .Control_Character)
		}
		p.i += 1
	}
	if p.i >= len(p.s) {
		return "", fail(p, .Eof)
	}
	b := strings.builder_make(0, p.i - start + 16, p.allocator)
	strings.write_bytes(&b, p.s[start:p.i])
	for p.i < len(p.s) {
		c := p.s[p.i]
		switch {
		case c == '"':
			p.i += 1
			if !utf8_valid(b.buf[:]) {
				strings.builder_destroy(&b)
				return "", fail(p, .Invalid_Utf8)
			}
			return strings.to_string(b), true
		case c == '\\':
			p.i += 1
			if !parse_escape(p, &b) {
				strings.builder_destroy(&b)
				return "", false
			}
		case c < 0x20:
			strings.builder_destroy(&b)
			return "", fail(p, .Control_Character)
		case:
			strings.write_byte(&b, c)
			p.i += 1
		}
	}
	strings.builder_destroy(&b)
	return "", fail(p, .Eof)
}

@(private)
hex_digit :: proc "contextless" (c: u8) -> (u32, bool) {
	switch c {
	case '0' ..= '9':
		return u32(c - '0'), true
	case 'a' ..= 'f':
		return u32(c - 'a' + 10), true
	case 'A' ..= 'F':
		return u32(c - 'A' + 10), true
	}
	return 0, false
}

// Four hex digits of a \u escape.
@(private)
hex4 :: proc "contextless" (p: ^Parser) -> (u32, bool) {
	if p.i + 4 > len(p.s) {
		p.i = len(p.s)
		return 0, fail(p, .Eof)
	}
	n: u32
	for k in 0 ..< 4 {
		d, ok := hex_digit(p.s[p.i + k])
		if !ok {
			p.i += 4
			return 0, fail(p, .Invalid_Escape)
		}
		n = n << 4 | d
	}
	p.i += 4
	return n, true
}

// An escape after its backslash, validated as for a string (`validate`).
@(private)
parse_escape :: proc(p: ^Parser, b: ^strings.Builder) -> bool {
	if p.i >= len(p.s) {
		return fail(p, .Eof)
	}
	c := p.s[p.i]
	p.i += 1
	switch c {
	case '"':
		strings.write_byte(b, '"')
	case '\\':
		strings.write_byte(b, '\\')
	case '/':
		strings.write_byte(b, '/')
	case 'b':
		strings.write_byte(b, 0x08)
	case 'f':
		strings.write_byte(b, 0x0C)
	case 'n':
		strings.write_byte(b, '\n')
	case 'r':
		strings.write_byte(b, '\r')
	case 't':
		strings.write_byte(b, '\t')
	case 'u':
		n := hex4(p) or_return
		if n >= 0xDC00 && n <= 0xDFFF {
			return fail(p, .Lone_Surrogate)
		}
		if n < 0xD800 || n > 0xDBFF {
			write_code_point(b, n)
			return true
		}
		// A leading surrogate: a trailing one must follow.
		if p.i >= len(p.s) {
			return fail(p, .Eof)
		}
		if p.s[p.i] != '\\' {
			p.i += 1
			return fail(p, .Lone_Surrogate)
		}
		p.i += 1
		if p.i >= len(p.s) {
			return fail(p, .Eof)
		}
		if p.s[p.i] != 'u' {
			p.i += 1
			return fail(p, .Lone_Surrogate)
		}
		p.i += 1
		n2 := hex4(p) or_return
		if n2 < 0xDC00 || n2 > 0xDFFF {
			return fail(p, .Lone_Surrogate)
		}
		write_code_point(b, ((n - 0xD800) << 10 | (n2 - 0xDC00)) + 0x10000)
	case:
		return fail(p, .Invalid_Escape)
	}
	return true
}

@(private)
write_code_point :: proc(b: ^strings.Builder, n: u32) {
	switch {
	case n < 0x80:
		strings.write_byte(b, u8(n))
	case n < 0x800:
		strings.write_byte(b, u8(0xC0 | n >> 6))
		strings.write_byte(b, u8(0x80 | n & 0x3F))
	case n < 0x10000:
		strings.write_byte(b, u8(0xE0 | n >> 12))
		strings.write_byte(b, u8(0x80 | (n >> 6) & 0x3F))
		strings.write_byte(b, u8(0x80 | n & 0x3F))
	case:
		strings.write_byte(b, u8(0xF0 | n >> 18))
		strings.write_byte(b, u8(0x80 | (n >> 12) & 0x3F))
		strings.write_byte(b, u8(0x80 | (n >> 6) & 0x3F))
		strings.write_byte(b, u8(0x80 | n & 0x3F))
	}
}

// Rust's `str::from_utf8`: no overlong forms, no surrogates, nothing past
// U+10FFFF.
utf8_valid :: proc "contextless" (s: []u8) -> bool {
	i := 0
	n := len(s)
	for i < n {
		c := s[i]
		if c < 0x80 {
			i += 1
			continue
		}
		lo, hi: u8 = 0x80, 0xBF
		width := 0
		switch c {
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
			return false
		}
		if i + width > n {
			return false
		}
		if s[i + 1] < lo || s[i + 1] > hi {
			return false
		}
		for k in 2 ..< width {
			if s[i + k] < 0x80 || s[i + k] > 0xBF {
				return false
			}
		}
		i += width
	}
	return true
}

// MARK: Numbers

// A number at '-' or a digit.
parse_number :: proc(p: ^Parser) -> (Value, bool) {
	negative := false
	if p.s[p.i] == '-' {
		negative = true
		p.i += 1
	}
	if p.i >= len(p.s) {
		return nil, fail(p, .Eof)
	}
	int_start := p.i
	c := p.s[p.i]
	switch c {
	case '0':
		p.i += 1
		if d := peek(p); d >= '0' && d <= '9' {
			return nil, fail(p, .Invalid_Number)
		}
	case '1' ..= '9':
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			p.i += 1
		}
	case:
		p.i += 1
		return nil, fail(p, .Invalid_Number)
	}
	int_end := p.i
	frac_start, frac_end := p.i, p.i
	if peek(p) == '.' {
		p.i += 1
		frac_start = p.i
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			p.i += 1
		}
		frac_end = p.i
		if frac_end == frac_start {
			return nil, fail(p, .Eof if p.i >= len(p.s) else .Invalid_Number)
		}
	}
	has_exp := false
	exp: i64 = 0
	if e := peek(p); e == 'e' || e == 'E' {
		has_exp = true
		p.i += 1
		exp_negative := false
		if s := peek(p); s == '+' || s == '-' {
			exp_negative = s == '-'
			p.i += 1
		}
		if p.i >= len(p.s) {
			return nil, fail(p, .Eof)
		}
		if d := p.s[p.i]; d < '0' || d > '9' {
			p.i += 1
			return nil, fail(p, .Invalid_Number)
		}
		// Past ~10^10 the value is 0 or infinite either way.
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			if exp < 10_000_000_000 {
				exp = exp * 10 + i64(p.s[p.i] - '0')
			}
			p.i += 1
		}
		if exp_negative {
			exp = -exp
		}
	}
	if !has_exp && frac_end == frac_start {
		// An integer: a u64 while it fits.
		sig: u64 = 0
		fits := true
		for k in int_start ..< int_end {
			d := u64(p.s[k] - '0')
			if sig > (max(u64) - d) / 10 {
				fits = false
				break
			}
			sig = sig * 10 + d
		}
		if fits {
			if !negative {
				return sig, true
			}
			neg := -i64(sig)
			if neg >= 0 {
				// -0, or below i64::MIN.
				return -f64(sig), true
			}
			return neg, true
		}
	}
	f, finite := decimal_to_f64(p.s[int_start:int_end], p.s[frac_start:frac_end], exp)
	if !finite {
		return nil, fail(p, .Number_Out_Of_Range)
	}
	return -f if negative else f, true
}

// MARK: Ignored values

// A value skipped as serde_json's `ignore_value` skips it (a struct's
// unknown field): its grammar is checked, but not its strings' UTF-8 or
// surrogates, not its numbers' range, and not its depth.
ignore_value :: proc(p: ^Parser) -> bool {
	stack := make([dynamic]u8, 0, 16)
	defer delete(stack)
	for {
		c, any := peek_ws(p)
		if !any {
			return fail(p, .Eof)
		}
		frame: u8 = 0
		switch c {
		case 'n':
			p.i += 1
			parse_ident(p, "ull") or_return
		case 't':
			p.i += 1
			parse_ident(p, "rue") or_return
		case 'f':
			p.i += 1
			parse_ident(p, "alse") or_return
		case '-', '0' ..= '9':
			ignore_number(p) or_return
		case '"':
			p.i += 1
			ignore_str(p) or_return
		case '[', '{':
			p.i += 1
			frame = c
		case:
			return fail(p, .Expected_Value)
		}
		accept_comma := frame == 0
		if frame != 0 {
			append(&stack, frame)
		} else if len(stack) == 0 {
			return true
		}
		// Close what ends here, then find the next value's place.
		for {
			top := stack[len(stack) - 1]
			d, more := peek_ws(p)
			if !more {
				return fail(p, .Eof)
			}
			if d == ',' && accept_comma {
				p.i += 1
				break
			}
			if (d == ']' && top == '[') || (d == '}' && top == '{') {
				p.i += 1
				pop(&stack)
				if len(stack) == 0 {
					return true
				}
				accept_comma = true
				continue
			}
			if accept_comma {
				return fail(p, .Expected_Comma_Or_End)
			}
			break
		}
		if stack[len(stack) - 1] == '{' {
			d, more := peek_ws(p)
			if !more {
				return fail(p, .Eof)
			}
			if d != '"' {
				return fail(p, .Key_Must_Be_String)
			}
			p.i += 1
			ignore_str(p) or_return
			d, more = peek_ws(p)
			if !more {
				return fail(p, .Eof)
			}
			if d != ':' {
				return fail(p, .Expected_Colon)
			}
			p.i += 1
		}
	}
}

// A string skipped: control characters refused, escapes' letters and hex
// digits checked, nothing else.
ignore_str :: proc "contextless" (p: ^Parser) -> bool {
	for p.i < len(p.s) {
		c := p.s[p.i]
		p.i += 1
		switch {
		case c == '"':
			return true
		case c == '\\':
			if p.i >= len(p.s) {
				return fail(p, .Eof)
			}
			e := p.s[p.i]
			p.i += 1
			switch e {
			case '"', '\\', '/', 'b', 'f', 'n', 'r', 't':
			case 'u':
				hex4(p) or_return
			case:
				return fail(p, .Invalid_Escape)
			}
		case c < 0x20:
			return fail(p, .Control_Character)
		}
	}
	return fail(p, .Eof)
}

// A number skipped: its grammar only.
@(private)
ignore_number :: proc "contextless" (p: ^Parser) -> bool {
	if p.s[p.i] == '-' {
		p.i += 1
	}
	switch peek(p) {
	case '0':
		p.i += 1
		if d := peek(p); d >= '0' && d <= '9' {
			return fail(p, .Invalid_Number)
		}
	case '1' ..= '9':
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			p.i += 1
		}
	case:
		return fail(p, .Invalid_Number)
	}
	if peek(p) == '.' {
		p.i += 1
		from := p.i
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			p.i += 1
		}
		if p.i == from {
			return fail(p, .Invalid_Number)
		}
	}
	if e := peek(p); e == 'e' || e == 'E' {
		p.i += 1
		if s := peek(p); s == '+' || s == '-' {
			p.i += 1
		}
		from := p.i
		for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
			p.i += 1
		}
		if p.i == from {
			return fail(p, .Invalid_Number)
		}
	}
	return true
}
