// f64's methods as Rust has them: round half away from zero, floor, trunc,
// log10 (libm), min/max that pass over a NaN (libm's fmin/fmax), clamp,
// powi (what LLVM makes of Rust's powi: compiler-rt's __powidf2, or pow on
// Windows), and `as` casts.
package state

import "core:c/libc"

round :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.round(x)}
floor :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.floor(x)}
trunc :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.trunc(x)}
log10 :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.log10(x)}
fmin :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmin(a, b)}
fmax :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmax(a, b)}

// f64::clamp (the caller keeps low <= high; a NaN stays NaN).
clamp_f64 :: proc "contextless" (x, low, high: f64) -> f64 {
	x := x
	if x < low {
		x = low
	}
	if x > high {
		x = high
	}
	return x
}

// f64::powi as LLVM lowers it: compiler-rt's __powidf2 (by squaring), but
// on Windows (MSVC has no __powidf2) the CRT's pow(a, f64(n)). They differ
// once the power isn't exact (10^301 by a few ULPs).
powi :: proc "contextless" (a: f64, n: i32) -> f64 {
	when ODIN_OS == .Windows {
		return libc.pow(a, f64(n))
	}
	a := a
	b := n
	recip := b < 0
	r := 1.0
	for {
		if b & 1 != 0 {
			r *= a
		}
		b /= 2
		if b == 0 {
			break
		}
		a *= a
	}
	return 1.0 / r if recip else r
}

// `f64 as usize` on 64 bits: truncated, saturating, NaN 0.
as_usize :: proc "contextless" (v: f64) -> u64 {
	switch {
	case v != v || v <= 0:
		return 0
	case v >= 18446744073709551615.0:
		return max(u64)
	}
	return u64(v)
}
