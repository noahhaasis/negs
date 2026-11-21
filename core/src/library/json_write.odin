// Writing JSON as Foundation's `JSONEncoder` does, byte for byte: pretty
// with sorted keys, `" : "`, two-space indents and unescaped slashes for a
// sidecar; compact with escaped slashes for the hashes of thumbnail looks.
// Numbers are `Double.description` without a trailing `.0`.
package library

import "core:strconv"
import "core:strconv/decimal"
import "core:strings"

// The bytes of a sidecar: `JSONEncoder` with pretty printing, sorted keys
// and unescaped slashes, then a newline (`PhotoSidecar.encode`).
to_sidecar_bytes :: proc(v: Json, allocator := context.allocator) -> []u8 {
	b := strings.builder_make(0, 1024, allocator)
	write_pretty(&b, v, 0)
	strings.write_byte(&b, '\n')
	return b.buf[:]
}

// The pretty form without its final newline (`Json`'s Display).
to_pretty_string :: proc(v: Json, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 1024, allocator)
	write_pretty(&b, v, 0)
	return strings.to_string(b)
}

// `JSONEncoder` with sorted keys only, as the thumbnail looks hash it: no
// spaces, slashes escaped.
to_compact_sorted :: proc(v: Json, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 256, allocator)
	write_compact(&b, v)
	return strings.to_string(b)
}

@(private = "file")
indent :: proc(b: ^strings.Builder, depth: int) {
	for _ in 0 ..< depth {
		strings.write_string(b, "  ")
	}
}

@(private = "file")
write_pretty :: proc(b: ^strings.Builder, v: Json, depth: int) {
	#partial switch c in v {
	case Json_Array:
		strings.write_string(b, "[\n")
		if len(c) == 0 {
			strings.write_byte(b, '\n')
		}
		for item, index in c {
			indent(b, depth + 1)
			write_pretty(b, item, depth + 1)
			if index + 1 < len(c) {
				strings.write_byte(b, ',')
			}
			strings.write_byte(b, '\n')
		}
		indent(b, depth)
		strings.write_byte(b, ']')
	case Json_Object:
		strings.write_string(b, "{\n")
		if len(c) == 0 {
			strings.write_byte(b, '\n')
		}
		for field, index in c {
			indent(b, depth + 1)
			write_json_string(b, field.key, false)
			strings.write_string(b, " : ")
			write_pretty(b, field.value, depth + 1)
			if index + 1 < len(c) {
				strings.write_byte(b, ',')
			}
			strings.write_byte(b, '\n')
		}
		indent(b, depth)
		strings.write_byte(b, '}')
	case:
		write_scalar(b, v, false)
	}
}

// Compact, slashes escaped.
write_compact :: proc(b: ^strings.Builder, v: Json) {
	#partial switch c in v {
	case Json_Array:
		strings.write_byte(b, '[')
		for item, index in c {
			if index > 0 {
				strings.write_byte(b, ',')
			}
			write_compact(b, item)
		}
		strings.write_byte(b, ']')
	case Json_Object:
		strings.write_byte(b, '{')
		for field, index in c {
			if index > 0 {
				strings.write_byte(b, ',')
			}
			write_json_string(b, field.key, true)
			strings.write_byte(b, ':')
			write_compact(b, field.value)
		}
		strings.write_byte(b, '}')
	case:
		write_scalar(b, v, true)
	}
}

@(private = "file")
write_scalar :: proc(b: ^strings.Builder, v: Json, escape_slashes: bool) {
	#partial switch c in v {
	case nil:
		strings.write_string(b, "null")
	case bool:
		strings.write_string(b, "true" if c else "false")
	case f64:
		write_json_number(b, c)
	case string:
		write_json_string(b, c, escape_slashes)
	}
}

@(private = "file")
HEX := "0123456789abcdef"

write_json_string :: proc(b: ^strings.Builder, s: string, escape_slashes: bool) {
	strings.write_byte(b, '"')
	start := 0
	for i in 0 ..< len(s) {
		c := s[i]
		escape := ""
		if c == '"' {
			escape = "\\\""
		} else if c == '\\' {
			escape = "\\\\"
		} else if c == '/' && escape_slashes {
			escape = "\\/"
		} else if c == '\n' {
			escape = "\\n"
		} else if c == '\t' {
			escape = "\\t"
		} else if c == '\r' {
			escape = "\\r"
		} else if c == 0x08 {
			escape = "\\b"
		} else if c == 0x0C {
			escape = "\\f"
		} else if c >= 0x20 {
			continue
		}
		strings.write_string(b, s[start:i])
		if escape != "" {
			strings.write_string(b, escape)
		} else {
			strings.write_string(b, "\\u00")
			strings.write_byte(b, HEX[c >> 4])
			strings.write_byte(b, HEX[c & 0xF])
		}
		start = i + 1
	}
	strings.write_string(b, s[start:])
	strings.write_byte(b, '"')
}

@(private = "file")
F64_INFO := strconv.Float_Info{52, 11, -1023}

// The shortest digits that read back as |v| (closest, ties to even, as
// Swift writes them), and the power of ten of the first: 1234.5 is
// ("12345", 3). `v` is finite and not zero.
@(private = "file")
shortest_digits :: proc(v: f64, d: ^decimal.Decimal) -> (digits: string, exponent: int) {
	bits := transmute(u64)abs(v)
	exp := int(bits >> 52) & 0x7FF
	mant := bits & (1 << 52 - 1)
	if exp == 0 {
		exp += 1
	} else {
		mant |= 1 << 52
	}
	exp += F64_INFO.bias
	decimal.assign(d, mant)
	decimal.shift(d, exp - 52)
	strconv.round_shortest(d, mant, exp, &F64_INFO)
	return string(d.digits[:d.count]), d.decimal_point - 1
}

// `Double.description` in Swift: shortest round-trip digits, in decimal
// from 10^-4 up to 2^53 and in exponent form (`1e-05`, `1.5e+16`) outside
// that; whole numbers end in `.0`.
write_swift_double_description :: proc(b: ^strings.Builder, v: f64) {
	if v != v {
		strings.write_string(b, "nan")
		return
	}
	if v == INF {
		strings.write_string(b, "inf")
		return
	}
	if v == -INF {
		strings.write_string(b, "-inf")
		return
	}
	negative := (transmute(u64)v) >> 63 != 0
	if negative {
		strings.write_byte(b, '-')
	}
	if v == 0 {
		strings.write_string(b, "0.0")
		return
	}
	d: decimal.Decimal
	digits, exponent := shortest_digits(v, &d)
	if exponent < -4 || abs(v) > 9007199254740992.0 {
		strings.write_byte(b, digits[0])
		if len(digits) > 1 {
			strings.write_byte(b, '.')
			strings.write_string(b, digits[1:])
		}
		strings.write_byte(b, 'e')
		strings.write_byte(b, '-' if exponent < 0 else '+')
		e := abs(exponent)
		if e < 10 {
			strings.write_byte(b, '0')
		}
		strings.write_int(b, e)
		return
	}
	if exponent < 0 {
		strings.write_string(b, "0.")
		for _ in 0 ..< -exponent - 1 {
			strings.write_byte(b, '0')
		}
		strings.write_string(b, digits)
		return
	}
	point := exponent + 1
	if len(digits) <= point {
		strings.write_string(b, digits)
		for _ in 0 ..< point - len(digits) {
			strings.write_byte(b, '0')
		}
		strings.write_string(b, ".0")
	} else {
		strings.write_string(b, digits[:point])
		strings.write_byte(b, '.')
		strings.write_string(b, digits[point:])
	}
}

@(private = "file")
INF :: f64(0h7FF0_0000_0000_0000)

swift_double_description :: proc(v: f64, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 32, allocator)
	write_swift_double_description(&b, v)
	return strings.to_string(b)
}

// A number as `JSONEncoder` writes it: `Double.description` without a
// trailing `.0`.
write_json_number :: proc(b: ^strings.Builder, v: f64) {
	start := len(b.buf)
	write_swift_double_description(b, v)
	if len(b.buf) - start >= 2 && strings.has_suffix(string(b.buf[start:]), ".0") {
		resize(&b.buf, len(b.buf) - 2)
	}
}

json_number_text :: proc(v: f64, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 32, allocator)
	write_json_number(&b, v)
	return strings.to_string(b)
}
