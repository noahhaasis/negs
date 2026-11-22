// An f64 as Rust's `{}` (Display) writes it: the shortest digits that read
// back as the same number, never in exponent form ("0.0000001",
// "1000000000000000000000"), whole numbers without a fraction ("3"), "-0",
// "NaN", "inf" and "-inf". The one implementation is value's
// (core/src/value/number.odin); core/diff/tests/value.rs holds it to Rust.
package border

import "core:strings"
import "../../value"

write_rust_display :: proc(b: ^strings.Builder, v: f64) {
	value.write_display(b, v)
}

rust_display :: proc(v: f64, allocator := context.allocator) -> string {
	return value.display(v, allocator)
}
