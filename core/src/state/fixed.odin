// Rust's `format!("{x:.N}")` for a double: the exact decimal value rounded
// to N places, ties to even; the sign of a negative zero kept; "NaN",
// "inf", "-inf". For the inspector's values (format_value), the colour
// filter's stops, the brightness note and ratio names.
package state

import "core:strings"

// Up to 17 places, which is all the rules ask for (at most 6).
FIXED_MAX_PLACES :: 17

write_fixed :: proc(b: ^strings.Builder, x: f64, wanted: int) {
	places := clamp(wanted, 0, FIXED_MAX_PLACES)
	if x != x {
		strings.write_string(b, "NaN")
		return
	}
	bits := transmute(u64)x
	if bits >> 63 != 0 {
		strings.write_byte(b, '-')
	}
	exp := int((bits >> 52) & 0x7ff)
	if exp == 0x7ff {
		strings.write_string(b, "inf")
		return
	}
	// |x| = m · 2^e exactly.
	m := bits & (1 << 52 - 1)
	if exp == 0 {
		exp = 1
	} else {
		m |= 1 << 52
	}
	e := exp - 1075
	if e >= 0 {
		// A whole number: its digits, then zeros.
		write_big(b, m, uint(e))
		if places > 0 {
			strings.write_byte(b, '.')
			for _ in 0 ..< places {
				strings.write_byte(b, '0')
			}
		}
		return
	}
	// m · 10^places / 2^k, rounded half to even (m · 10^17 < 2^110).
	k := uint(-e)
	scaled := u128(m) * pow10_u128(places)
	q: u128
	if k < 128 {
		q = scaled >> k
		r := scaled & (u128(1) << k - 1)
		half := u128(1) << (k - 1)
		if r > half || (r == half && q & 1 == 1) {
			q += 1
		}
	}
	unit := pow10_u128(places)
	whole := q / unit
	frac := q % unit
	write_u128(b, whole)
	if places > 0 {
		strings.write_byte(b, '.')
		digits: [FIXED_MAX_PLACES]u8
		for i := places - 1; i >= 0; i -= 1 {
			digits[i] = '0' + u8(frac % 10)
			frac /= 10
		}
		strings.write_bytes(b, digits[:places])
	}
}

fixed :: proc(x: f64, places: int, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 32, allocator)
	write_fixed(&b, x, places)
	return strings.to_string(b)
}

@(private)
pow10_u128 :: proc "contextless" (n: int) -> u128 {
	p: u128 = 1
	for _ in 0 ..< n {
		p *= 10
	}
	return p
}

@(private)
write_u128 :: proc(b: ^strings.Builder, v: u128) {
	digits: [40]u8
	n := 0
	v := v
	for {
		digits[n] = '0' + u8(v % 10)
		n += 1
		v /= 10
		if v == 0 {
			break
		}
	}
	for i := n - 1; i >= 0; i -= 1 {
		strings.write_byte(b, digits[i])
	}
}

// m · 2^shift in decimal (m < 2^53, shift ≤ 971).
@(private)
write_big :: proc(b: ^strings.Builder, m: u64, shift: uint) {
	limbs: [40]u32 // little-endian, 1,280 bits
	word := shift / 32
	bit := shift % 32
	wide := u128(m) << bit
	for i in 0 ..< 4 {
		limbs[int(word) + i] = u32(wide >> (32 * uint(i)))
	}
	n := int(word) + 4
	for n > 0 && limbs[n - 1] == 0 {
		n -= 1
	}
	// Repeated division by 10^9, least significant chunk first.
	chunks: [48]u32
	count := 0
	for n > 0 {
		rem: u64 = 0
		for i := n - 1; i >= 0; i -= 1 {
			cur := rem << 32 | u64(limbs[i])
			limbs[i] = u32(cur / 1_000_000_000)
			rem = cur % 1_000_000_000
		}
		chunks[count] = u32(rem)
		count += 1
		for n > 0 && limbs[n - 1] == 0 {
			n -= 1
		}
	}
	if count == 0 {
		strings.write_byte(b, '0')
		return
	}
	write_u128(b, u128(chunks[count - 1]))
	for i := count - 2; i >= 0; i -= 1 {
		c := chunks[i]
		digits: [9]u8
		for j := 8; j >= 0; j -= 1 {
			digits[j] = '0' + u8(c % 10)
			c /= 10
		}
		strings.write_bytes(b, digits[:])
	}
}
