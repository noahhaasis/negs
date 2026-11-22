// Numbers as text: serde_json's (itoa for integers, ryu for doubles, null
// for a double that isn't finite) and Rust's `{}` for f64. Both use the
// shortest digits that read back as the same double, the closest of them
// when several are as short (Żmij, core:strconv); they differ only in how
// the digits are laid out.
package value

import "core:strconv"
import "core:strings"

// The shortest round-trip digits of |v| (finite, not zero): `digits` has no
// leading or trailing zeros, and 10^(point-1) <= |v| < 10^point.
@(private)
shortest :: proc "contextless" (v: f64, buf: ^[48]u8) -> (digits: []u8, point: int) {
	bits := transmute(u64)v
	exp := int(bits >> 52) & 0x7FF
	mant := bits & (1 << 52 - 1)
	if exp == 0 {
		exp = 1
	} else {
		mant |= 1 << 52
	}
	d := strconv._shortest_decimal(buf[:], mant, exp, 52)
	return d.digits, d.decimal_point
}

// A double as serde_json writes it (ryu's `format64`, `pretty` layout):
// "1.0", "0.1", "1e16", "1.5e-7", "-0.0", "123456789.0"; null when it isn't
// finite. Decimal from 10^-5 to below 10^16, exponent form outside.
write_f64 :: proc(b: ^strings.Builder, v: f64) {
	bits := transmute(u64)v
	if (bits >> 52) & 0x7FF == 0x7FF {
		strings.write_string(b, "null")
		return
	}
	if bits >> 63 != 0 {
		strings.write_byte(b, '-')
	}
	if bits << 1 == 0 {
		strings.write_string(b, "0.0")
		return
	}
	buf: [48]u8
	digits, kk := shortest(v, &buf)
	length := len(digits)
	k := kk - length
	switch {
	case 0 <= k && kk <= 16:
		// 1234e7 -> 12340000000.0
		strings.write_bytes(b, digits)
		for _ in 0 ..< k {
			strings.write_byte(b, '0')
		}
		strings.write_string(b, ".0")
	case 0 < kk && kk <= 16:
		// 1234e-2 -> 12.34
		strings.write_bytes(b, digits[:kk])
		strings.write_byte(b, '.')
		strings.write_bytes(b, digits[kk:])
	case -5 < kk && kk <= 0:
		// 1234e-6 -> 0.001234
		strings.write_string(b, "0.")
		for _ in 0 ..< -kk {
			strings.write_byte(b, '0')
		}
		strings.write_bytes(b, digits)
	case length == 1:
		// 1e30
		strings.write_byte(b, digits[0])
		strings.write_byte(b, 'e')
		strings.write_int(b, kk - 1)
	case:
		// 1234e30 -> 1.234e33
		strings.write_byte(b, digits[0])
		strings.write_byte(b, '.')
		strings.write_bytes(b, digits[1:])
		strings.write_byte(b, 'e')
		strings.write_int(b, kk - 1)
	}
}

// A double as Rust's `{}` (Display) writes it: shortest round-trip digits,
// never in exponent form ("0.0000001", "1000000000000000000000"), whole
// numbers without a fraction ("3"), "-0", "NaN", "inf" and "-inf".
write_display :: proc(b: ^strings.Builder, v: f64) {
	bits := transmute(u64)v
	if (bits >> 52) & 0x7FF == 0x7FF {
		if bits & (1 << 52 - 1) != 0 {
			strings.write_string(b, "NaN")
		} else {
			strings.write_string(b, "-inf" if bits >> 63 != 0 else "inf")
		}
		return
	}
	if bits >> 63 != 0 {
		strings.write_byte(b, '-')
	}
	if bits << 1 == 0 {
		strings.write_byte(b, '0')
		return
	}
	buf: [48]u8
	digits, point := shortest(v, &buf)
	if point <= 0 {
		strings.write_string(b, "0.")
		for _ in 0 ..< -point {
			strings.write_byte(b, '0')
		}
		strings.write_bytes(b, digits)
	} else if point >= len(digits) {
		strings.write_bytes(b, digits)
		for _ in 0 ..< point - len(digits) {
			strings.write_byte(b, '0')
		}
	} else {
		strings.write_bytes(b, digits[:point])
		strings.write_byte(b, '.')
		strings.write_bytes(b, digits[point:])
	}
}

// write_display into a new string.
display :: proc(v: f64, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 32, allocator)
	write_display(&b, v)
	return strings.to_string(b)
}

write_u64 :: proc(b: ^strings.Builder, v: u64) {
	strings.write_u64(b, v)
}

write_i64 :: proc(b: ^strings.Builder, v: i64) {
	strings.write_i64(b, v)
}
