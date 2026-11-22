// Numbers as Rust's format! writes them: `x as i64` (saturating, NaN 0),
// and `{:.1}` (exact, ties to even).
package facts

import "core:c/libc"
import "core:strings"

// f64::round: half away from zero.
round :: #force_inline proc "contextless" (x: f64) -> f64 {
	return libc.round(x)
}

// `x as i64`: truncated, saturating, NaN 0.
as_i64 :: proc "contextless" (x: f64) -> i64 {
	if x != x {
		return 0
	}
	if x >= 9223372036854775807.0 {
		return max(i64)
	}
	if x <= -9223372036854775808.0 {
		return min(i64)
	}
	return i64(x)
}

// At most one decimal, none when whole ("4", "2,8").
write_one_decimal :: proc(b: ^strings.Builder, value: f64, decimal: rune) {
	rounded := round(value * 10.0) / 10.0
	if rounded - libc.trunc(rounded) == 0.0 {
		strings.write_i64(b, as_i64(rounded))
		return
	}
	write_fixed_1(b, rounded, decimal)
}

// `format!("{:.1}", x)` with `decimal` for the point; x not a whole number.
@(private)
write_fixed_1 :: proc(b: ^strings.Builder, x: f64, decimal: rune) {
	if x != x {
		strings.write_string(b, "NaN")
		return
	}
	bits := transmute(u64)x
	negative := bits >> 63 != 0
	if x == libc.INFINITY || x == -libc.INFINITY {
		strings.write_string(b, "-inf" if negative else "inf")
		return
	}
	// |x| = m · 2^e exactly.
	exp := int((bits >> 52) & 0x7ff)
	m := bits & (1 << 52 - 1)
	if exp == 0 {
		exp = 1
	} else {
		m |= 1 << 52
	}
	e := exp - 1075
	// Tenths, rounded half to even: m·10 / 2^-e.
	tenths: u64
	if e >= 0 {
		// Whole numbers don't come here, but be exact anyway for small e.
		tenths = (m * 10) << uint(e) if e < 7 else 0
	} else {
		k := uint(-e)
		scaled := m * 10
		if k >= 64 {
			tenths = 0
		} else {
			q := scaled >> k
			r := scaled & (u64(1) << k - 1)
			half := u64(1) << (k - 1)
			if r > half || (r == half && q & 1 == 1) {
				q += 1
			}
			tenths = q
		}
	}
	if negative {
		strings.write_byte(b, '-')
	}
	strings.write_u64(b, tenths / 10)
	strings.write_rune(b, decimal)
	strings.write_byte(b, '0' + u8(tenths % 10))
}
