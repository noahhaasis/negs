// The floating-point steps Rust takes for the tone graph's maths, so the
// core's results are its bits: libm's log10 and hypot, and for 10^x what
// LLVM turns Rust's `10f64.powf(x)` into: exp10 where the platform has one
// (__exp10 on Darwin, glibc's exp10), pow on Windows.
package tone

import "core:c/libc"

log10 :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.log10(x)}
hypot :: #force_inline proc "contextless" (x, y: f64) -> f64 {return libc.hypot(x, y)}
round :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.round(x)}
floor :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.floor(x)}
// f64::max / min: the other one when one is NaN.
fmax :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmax(a, b)}
fmin :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmin(a, b)}

when ODIN_OS == .Darwin {
	foreign import system "system:System"
	@(private = "file")
	@(default_calling_convention = "c")
	foreign system {
		__exp10 :: proc(x: f64) -> f64 ---
	}
	exp10 :: #force_inline proc "contextless" (x: f64) -> f64 {return __exp10(x)}
} else when ODIN_OS == .Linux {
	foreign import libm "system:m"
	@(private = "file")
	@(default_calling_convention = "c")
	foreign libm {
		@(link_name = "exp10")
		c_exp10 :: proc(x: f64) -> f64 ---
	}
	exp10 :: #force_inline proc "contextless" (x: f64) -> f64 {return c_exp10(x)}
} else {
	exp10 :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.pow(10, x)}
}

// f64::clamp: NaN stays NaN.
clamp :: #force_inline proc "contextless" (x, lo, hi: f64) -> f64 {
	v := x
	if v < lo {
		v = lo
	}
	if v > hi {
		v = hi
	}
	return v
}

// f64::powi as LLVM lowers it: compiler-rt's __powidf2 (by squaring), but
// on Windows (MSVC has no __powidf2) the CRT's pow(a, f64(b)).
powi :: proc "contextless" (a: f64, b: i32) -> f64 {
	when ODIN_OS == .Windows {
		return libc.pow(a, f64(b))
	}
	recip := b < 0
	x, n := a, b
	r := 1.0
	for {
		if n & 1 != 0 {
			r *= x
		}
		n /= 2
		if n == 0 {
			break
		}
		x *= x
	}
	return 1 / r if recip else r
}

// `x as i32`: truncated, saturating, NaN 0.
as_i32 :: proc "contextless" (x: f64) -> i32 {
	if x != x {
		return 0
	}
	if x >= 2147483647.0 {
		return max(i32)
	}
	if x <= -2147483648.0 {
		return min(i32)
	}
	return i32(x)
}

// `x as usize`, for x ≥ 0 (64-bit): NaN 0.
as_index :: proc "contextless" (x: f64) -> int {
	if !(x > 0) {
		return 0
	}
	if x >= 9223372036854775807.0 {
		return max(int)
	}
	return int(x)
}

// state::snap: clamped to [low, high] (low ≤ high), to the step, rounded
// to the step's decimals.
snap :: proc "contextless" (value, low, high, step: f64) -> f64 {
	clamped := clamp(value, low, high)
	steps := round((clamped - low) / step)
	// `as i32 + 1` wraps in a release build.
	decimals := max(i32(u32(as_i32(floor(-log10(step)))) + 1), 0)
	scale := powi(10, decimals)
	return fmin(round((low + steps * step) * scale) / scale, high)
}
