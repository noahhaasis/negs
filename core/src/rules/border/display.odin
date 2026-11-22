// An f64 as Rust's `{}` (Display) writes it: the shortest digits that read
// back as the same number, never in exponent form ("0.0000001",
// "1000000000000000000000"), whole numbers without a fraction ("3"), "-0",
// "NaN", "inf" and "-inf".
package border

import "core:strconv"
import "core:strconv/decimal"
import "core:strings"

@(private = "file")
F64_INFO := strconv.Float_Info{52, 11, -1023}

write_rust_display :: proc(b: ^strings.Builder, v: f64) {
	bits := transmute(u64)v
	exp := int(bits >> 52) & 0x7FF
	mant := bits & (1 << 52 - 1)
	if exp == 0x7FF {
		if mant != 0 {
			strings.write_string(b, "NaN")
		} else {
			strings.write_string(b, "-inf" if bits >> 63 != 0 else "inf")
		}
		return
	}
	if bits >> 63 != 0 {
		strings.write_byte(b, '-')
	}
	if exp == 0 && mant == 0 {
		strings.write_byte(b, '0')
		return
	}
	if exp == 0 {
		exp += 1
	} else {
		mant |= 1 << 52
	}
	exp += F64_INFO.bias
	d: decimal.Decimal
	decimal.assign(&d, mant)
	decimal.shift(&d, exp - 52)
	strconv.round_shortest(&d, mant, exp, &F64_INFO)
	digits := string(d.digits[:d.count])
	point := d.decimal_point
	if point <= 0 {
		strings.write_string(b, "0.")
		for _ in 0 ..< -point {
			strings.write_byte(b, '0')
		}
		strings.write_string(b, digits)
	} else if point >= len(digits) {
		strings.write_string(b, digits)
		for _ in 0 ..< point - len(digits) {
			strings.write_byte(b, '0')
		}
	} else {
		strings.write_string(b, digits[:point])
		strings.write_byte(b, '.')
		strings.write_string(b, digits[point:])
	}
}

rust_display :: proc(v: f64, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 32, allocator)
	write_rust_display(&b, v)
	return strings.to_string(b)
}
