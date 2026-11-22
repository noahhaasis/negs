// Floating point as Rust's f64 does it, so the layouts match the Rust they
// replace bit for bit. Odin's `min` and `max` on floats are LLVM's minnum
// and maxnum, as Rust's f64::min and max; its `clamp` is not Rust's (NaN
// and −0 differ), hence `clampf`. Logarithms and exponentials come from the
// platform's libm, as Rust's do.
package sheet

import "core:c/libc"
import "core:math"

// Rust's f64::clamp: NaN stays NaN; an end equal to the value keeps the value.
clampf :: #force_inline proc "contextless" (x, lo, hi: f64) -> f64 {
	x := x
	if x < lo {
		x = lo
	}
	if x > hi {
		x = hi
	}
	return x
}

// Rust's f64::round: half away from zero.
roundf :: #force_inline proc "contextless" (x: f64) -> f64 {
	return math.round(x)
}

lnf :: #force_inline proc "contextless" (x: f64) -> f64 {
	return f64(libc.log(libc.double(x)))
}

expf :: #force_inline proc "contextless" (x: f64) -> f64 {
	return f64(libc.exp(libc.double(x)))
}

// Where Rust's Iterator::sum of f64s starts: −0 (an untyped constant
// would lose the sign).
sum_zero :: #force_inline proc "contextless" () -> f64 {
	return transmute(f64)u64(0x8000_0000_0000_0000)
}

// f64::total_cmp's key: orders −0 before +0 and NaNs by their bits.
total_key :: #force_inline proc "contextless" (x: f64) -> i64 {
	b := transmute(i64)x
	return b ~ i64(u64(b >> 63) >> 1)
}

// abs_diff of two indices.
abs_diff :: #force_inline proc "contextless" (a, b: uint) -> uint {
	return a - b if a > b else b - a
}
