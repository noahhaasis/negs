// The rendered preview's checksum: Rust's `DefaultHasher::new()` (SipHash
// 1-3 with both keys zero) over `&[u8]`, whose Hash writes the length as
// a usize (8 bytes, little-endian on the 64-bit platforms Negs ships for)
// before the bytes.
package preview

import "base:intrinsics"

@(private = "file")
Sip :: struct {
	v0, v1, v2, v3: u64,
}

@(private = "file")
rotl :: #force_inline proc "contextless" (x: u64, b: u64) -> u64 {
	return (x << b) | (x >> (64 - b))
}

@(private = "file")
round :: #force_inline proc "contextless" (s: ^Sip) {
	s.v0 += s.v1
	s.v1 = rotl(s.v1, 13)
	s.v1 ~= s.v0
	s.v0 = rotl(s.v0, 32)
	s.v2 += s.v3
	s.v3 = rotl(s.v3, 16)
	s.v3 ~= s.v2
	s.v0 += s.v3
	s.v3 = rotl(s.v3, 21)
	s.v3 ~= s.v0
	s.v2 += s.v1
	s.v1 = rotl(s.v1, 17)
	s.v1 ~= s.v2
	s.v2 = rotl(s.v2, 32)
}

@(private = "file")
compress :: #force_inline proc "contextless" (s: ^Sip, m: u64) {
	s.v3 ~= m
	round(s)
	s.v0 ~= m
}

// `checksum`: SipHash-1-3(0, 0) of len(bytes) as a u64 then the bytes.
checksum :: proc "contextless" (bytes: []u8) -> u64 {
	s := Sip{0x736f6d6570736575, 0x646f72616e646f6d, 0x6c7967656e657261, 0x7465646279746573}
	compress(&s, u64(len(bytes)))
	n := len(bytes)
	p := raw_data(bytes)
	whole := n &~ 7
	#no_bounds_check for i := 0; i < whole; i += 8 {
		m := intrinsics.unaligned_load((^u64le)(&p[i]))
		compress(&s, u64(m))
	}
	tail: u64 = 0
	for i in whole ..< n {
		tail |= u64(p[i]) << (8 * u64(i - whole))
	}
	// The length byte counts the usize prefix too.
	b := (u64(n + 8) & 0xff) << 56 | tail
	compress(&s, b)
	s.v2 ~= 0xff
	round(&s)
	round(&s)
	round(&s)
	return s.v0 ~ s.v1 ~ s.v2 ~ s.v3
}
