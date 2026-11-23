// A decimal number as the nearest double, ties to even, as serde_json's
// `float_roundtrip` parser (lexical) reads it, for any number of digits and
// any exponent. Eisel-Lemire (core:strconv) settles nearly every number;
// when more than 19 digits leave it unsure, the digits are compared exactly
// with the halfway point between the two candidates.
package value

import "core:strconv"

// Digits beyond this many can't move a double's rounding: a halfway point
// between two doubles has at most 767 significant digits, so the rest only
// matter as "something nonzero" (one sticky digit).
@(private = "file")
MAX_DIGITS :: 800

// int_digits.frac_digits × 10^exp (the digits ASCII, either part may be
// empty), and whether it is finite (false: it rounds to infinity).
decimal_to_f64 :: proc(int_digits, frac_digits: []u8, exp: i64) -> (f64, bool) {
	// The significant digits: no leading or trailing zeros.
	digit :: #force_inline proc "contextless" (a, b: []u8, k: int) -> u8 {
		return a[k] if k < len(a) else b[k - len(a)]
	}
	total := len(int_digits) + len(frac_digits)
	first := 0
	for first < total && digit(int_digits, frac_digits, first) == '0' {
		first += 1
	}
	if first == total {
		return 0, true
	}
	last := total
	for digit(int_digits, frac_digits, last - 1) == '0' {
		last -= 1
	}
	n := last - first
	// value = d[first:last] × 10^e10
	e10 := exp - i64(len(frac_digits)) + i64(total - last)
	point := i64(n) + e10
	if point > 310 {
		return 0, false
	}
	if point < -330 {
		return 0, true
	}
	if n <= 19 {
		w: u64
		for k in first ..< last {
			w = w * 10 + u64(digit(int_digits, frac_digits, k) - '0')
		}
		bits := strconv.fast_float_compute_float(f64, int(e10), w)
		return transmute(f64)bits, bits != 0x7FF << 52
	}
	w: u64
	for k in first ..< first + 19 {
		w = w * 10 + u64(digit(int_digits, frac_digits, k) - '0')
	}
	q := int(e10) + (n - 19)
	low := strconv.fast_float_compute_float(f64, q, w)
	high := strconv.fast_float_compute_float(f64, q, w + 1)
	if low == high {
		return transmute(f64)low, low != 0x7FF << 52
	}
	// The value lies between `low` and the next double up: compare it with
	// their halfway point.
	digits := make([]u8, min(n, MAX_DIGITS + 1))
	defer delete(digits)
	kept := min(n, MAX_DIGITS)
	for k in 0 ..< kept {
		digits[k] = digit(int_digits, frac_digits, first + k) - '0'
	}
	e := int(e10)
	if n > MAX_DIGITS {
		// The last digit is nonzero, so something nonzero was cut.
		digits[MAX_DIGITS] = 1
		e += n - (MAX_DIGITS + 1)
	}
	order := compare_halfway(digits, e, low)
	bits := low
	if order > 0 || (order == 0 && low & 1 == 1) {
		bits += 1
	}
	return transmute(f64)bits, bits != 0x7FF << 52
}

// MARK: Exact comparison

// Little-endian base 2^32.
@(private = "file")
Big :: [dynamic]u32

@(private = "file")
big_mul_small :: proc(b: ^Big, m: u32, add: u32 = 0) {
	carry := u64(add)
	for &limb in b {
		x := u64(limb) * u64(m) + carry
		limb = u32(x)
		carry = x >> 32
	}
	if carry != 0 {
		append(b, u32(carry))
	}
}

@(private = "file")
big_mul_pow10 :: proc(b: ^Big, e: int) {
	e := e
	for e >= 9 {
		big_mul_small(b, 1_000_000_000)
		e -= 9
	}
	p: u32 = 1
	for _ in 0 ..< e {
		p *= 10
	}
	if p != 1 {
		big_mul_small(b, p)
	}
}

@(private = "file")
big_shl :: proc(b: ^Big, bits: int) {
	words, rest := bits / 32, uint(bits % 32)
	if rest != 0 {
		carry: u32 = 0
		for &limb in b {
			next := limb >> (32 - rest)
			limb = limb << rest | carry
			carry = next
		}
		if carry != 0 {
			append(b, carry)
		}
	}
	if words > 0 {
		old := len(b)
		resize(b, old + words)
		copy(b[words:], b[:old])
		for w in 0 ..< words {
			b[w] = 0
		}
	}
}

@(private = "file")
big_trim :: proc(b: ^Big) {
	for len(b) > 0 && b[len(b) - 1] == 0 {
		pop(b)
	}
}

@(private = "file")
big_cmp :: proc(a, b: ^Big) -> int {
	big_trim(a)
	big_trim(b)
	if len(a) != len(b) {
		return 1 if len(a) > len(b) else -1
	}
	for i := len(a) - 1; i >= 0; i -= 1 {
		if a[i] != b[i] {
			return 1 if a[i] > b[i] else -1
		}
	}
	return 0
}

// The sign of digits × 10^e − (the halfway point above `bits`): the double
// `bits` (finite, positive) is m × 2^k, its upper neighbour's halfway point
// (2m + 1) × 2^(k−1).
@(private = "file")
compare_halfway :: proc(digits: []u8, e: int, bits: u64) -> int {
	biased := int(bits >> 52)
	m := bits & (1 << 52 - 1)
	k: int
	if biased == 0 {
		k = -1074
	} else {
		m |= 1 << 52
		k = biased - 1075
	}
	lhs := make(Big, 0, 64)
	rhs := make(Big, 0, 64)
	defer delete(lhs)
	defer delete(rhs)
	// The digits, nine at a time.
	append(&lhs, 0)
	i := 0
	for i < len(digits) {
		chunk: u32 = 0
		scale: u32 = 1
		for j := 0; j < 9 && i < len(digits); j += 1 {
			chunk = chunk * 10 + u32(digits[i])
			scale *= 10
			i += 1
		}
		big_mul_small(&lhs, scale, chunk)
	}
	half := 2 * m + 1
	append(&rhs, u32(half), u32(half >> 32))
	if e >= 0 {
		big_mul_pow10(&lhs, e)
	} else {
		big_mul_pow10(&rhs, -e)
	}
	if k - 1 >= 0 {
		big_shl(&rhs, k - 1)
	} else {
		big_shl(&lhs, 1 - k)
	}
	return big_cmp(&lhs, &rhs)
}

// Whether the double `bits` (finite, not zero) lies exactly halfway
// between `digits` (ASCII, its shortest digits, 10^(point-1) <= |v| <
// 10^point) and the next number of as many digits above them. Rust's
// shortest formatting (core::fmt's flt2dec) rounds such a tie up; Żmij
// (core:strconv, and zmij in serde_json) rounds it to even.
halfway_above :: proc(digits: []u8, point: int, bits: u64) -> bool {
	biased := int(bits >> 52) & 0x7FF
	m := bits & (1 << 52 - 1)
	k: int
	if biased == 0 {
		k = -1074
	} else {
		m |= 1 << 52
		k = biased - 1075
	}
	// m × 2^k == (2D + 1) × 10^e / 2, e the last digit's exponent:
	// m × 2^(k+1) == (2D + 1) × 10^e.
	e := point - len(digits)
	lhs := make(Big, 0, 48)
	rhs := make(Big, 0, 48)
	defer delete(lhs)
	defer delete(rhs)
	append(&lhs, u32(m), u32(m >> 32))
	for d in digits {
		big_mul_small(&rhs, 10, u32(d - '0'))
	}
	big_mul_small(&rhs, 2, 1)
	if k + 1 >= 0 {
		big_shl(&lhs, k + 1)
	} else {
		big_shl(&rhs, -(k + 1))
	}
	if e >= 0 {
		big_mul_pow10(&rhs, e)
	} else {
		big_mul_pow10(&lhs, -e)
	}
	return big_cmp(&lhs, &rhs) == 0
}
