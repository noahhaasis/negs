// Writing as serde_json does: `to_string` / `to_vec` (compact) and
// `to_string_pretty` / `to_vec_pretty` (two spaces, "key": value, empty
// containers as [] and {}).
package value

import "core:strings"

@(private = "file")
HEX := "0123456789abcdef"

// A string as serde_json escapes it: `"` and `\`, and the control
// characters (\b \t \n \f \r by name, the others as \u00xx); nothing else.
write_string :: proc(b: ^strings.Builder, s: string) {
	strings.write_byte(b, '"')
	start := 0
	for i in 0 ..< len(s) {
		c := s[i]
		if c >= 0x20 && c != '"' && c != '\\' {
			continue
		}
		strings.write_string(b, s[start:i])
		switch c {
		case '"':
			strings.write_string(b, `\"`)
		case '\\':
			strings.write_string(b, `\\`)
		case 0x08:
			strings.write_string(b, `\b`)
		case 0x09:
			strings.write_string(b, `\t`)
		case 0x0A:
			strings.write_string(b, `\n`)
		case 0x0C:
			strings.write_string(b, `\f`)
		case 0x0D:
			strings.write_string(b, `\r`)
		case:
			strings.write_string(b, `\u00`)
			strings.write_byte(b, HEX[c >> 4])
			strings.write_byte(b, HEX[c & 15])
		}
		start = i + 1
	}
	strings.write_string(b, s[start:])
	strings.write_byte(b, '"')
}

write_scalar :: proc(b: ^strings.Builder, v: Value) {
	#partial switch c in v {
	case nil:
		strings.write_string(b, "null")
	case bool:
		strings.write_string(b, "true" if c else "false")
	case u64:
		write_u64(b, c)
	case i64:
		write_i64(b, c)
	case f64:
		write_f64(b, c)
	case string:
		write_string(b, c)
	}
}

// `serde_json::to_string`.
write_compact :: proc(b: ^strings.Builder, v: Value) {
	#partial switch c in v {
	case Array:
		strings.write_byte(b, '[')
		for item, i in c {
			if i > 0 {
				strings.write_byte(b, ',')
			}
			write_compact(b, item)
		}
		strings.write_byte(b, ']')
	case Object:
		strings.write_byte(b, '{')
		for field, i in c {
			if i > 0 {
				strings.write_byte(b, ',')
			}
			write_string(b, field.key)
			strings.write_byte(b, ':')
			write_compact(b, field.value)
		}
		strings.write_byte(b, '}')
	case:
		write_scalar(b, v)
	}
}

// serde_json's PrettyFormatter: a new line and two spaces per level before
// each element, the closing bracket on its own line.
write_indent :: proc(b: ^strings.Builder, depth: int) {
	strings.write_byte(b, '\n')
	for _ in 0 ..< depth {
		strings.write_string(b, "  ")
	}
}

// `serde_json::to_string_pretty`, at `depth` levels in.
write_pretty :: proc(b: ^strings.Builder, v: Value, depth := 0) {
	#partial switch c in v {
	case Array:
		strings.write_byte(b, '[')
		for item, i in c {
			if i > 0 {
				strings.write_byte(b, ',')
			}
			write_indent(b, depth + 1)
			write_pretty(b, item, depth + 1)
		}
		if len(c) > 0 {
			write_indent(b, depth)
		}
		strings.write_byte(b, ']')
	case Object:
		strings.write_byte(b, '{')
		for field, i in c {
			if i > 0 {
				strings.write_byte(b, ',')
			}
			write_indent(b, depth + 1)
			write_string(b, field.key)
			strings.write_string(b, ": ")
			write_pretty(b, field.value, depth + 1)
		}
		if len(c) > 0 {
			write_indent(b, depth)
		}
		strings.write_byte(b, '}')
	case:
		write_scalar(b, v)
	}
}

// `serde_json::to_string` into a new string.
to_string :: proc(v: Value, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 256, allocator)
	write_compact(&b, v)
	return strings.to_string(b)
}

// `serde_json::to_string_pretty` into a new string.
to_string_pretty :: proc(v: Value, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 256, allocator)
	write_pretty(&b, v)
	return strings.to_string(b)
}
