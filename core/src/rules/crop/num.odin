// Rust's f64 and integer semantics, so the picture rules round, clamp and
// cast exactly as the Rust they replace (windows/src/crop.rs and the rest):
// min and max ignore a NaN (llvm.minnum, which libm's fmin is), clamp lets a
// NaN through, and `as` casts from floats saturate (NaN is 0).
package crop

import "core:c/libc"
import "../../marks"

PI :: 3.14159265358979323846264338327950288

fmin :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmin(a, b)}
fmax :: #force_inline proc "contextless" (a, b: f64) -> f64 {return libc.fmax(a, b)}
fabs :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.fabs(x)}
round :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.round(x)}
floor :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.floor(x)}
ceil :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.ceil(x)}
sqrt :: #force_inline proc "contextless" (x: f64) -> f64 {return libc.sqrt(x)}

// The sine and cosine of one angle, as Rust's `sin_cos` computes them (the
// platform's fused routine where LLVM uses one: see marks.sin_cos).
sin_cos :: #force_inline proc "contextless" (x: f64) -> (s, c: f64) {return marks.sin_cos(x)}

// Rust's `f64::clamp`: a NaN stays NaN.
clamp_f64 :: #force_inline proc "contextless" (x, low, high: f64) -> f64 {
	v := x
	if v < low {
		v = low
	}
	if v > high {
		v = high
	}
	return v
}

// `x.to_radians()`: one multiply by π/180 rounded to f64 first, as Rust's
// constant is (a variable, so Odin doesn't fold it exactly).
to_radians :: #force_inline proc "contextless" (x: f64) -> f64 {
	pi: f64 = PI
	return x * (pi / 180.0)
}

// `x as u32`.
sat_u32 :: #force_inline proc "contextless" (x: f64) -> u32 {
	if !(x > 0) {
		return 0
	}
	if x >= 4294967295.0 {
		return max(u32)
	}
	return u32(x)
}

// `x as u8`.
sat_u8 :: #force_inline proc "contextless" (x: f64) -> u8 {
	if !(x > 0) {
		return 0
	}
	if x >= 255.0 {
		return 255
	}
	return u8(x)
}

// `x as i64`.
sat_i64 :: #force_inline proc "contextless" (x: f64) -> i64 {
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

// `x as usize` (64-bit).
sat_uint :: #force_inline proc "contextless" (x: f64) -> uint {
	if !(x > 0) {
		return 0
	}
	if x >= 18446744073709551615.0 {
		return max(uint)
	}
	return uint(x)
}

// `n.rem_euclid(4)`.
rem4 :: #force_inline proc "contextless" (n: i32) -> i32 {
	return n %% 4
}
