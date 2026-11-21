// SHA-256 (FIPS 180-4), for the names the macOS app gives thumbnails
// (CryptoKit there). Small inputs only, so kept simple (sha256.rs).
package library

@(private = "file")
K := [64]u32 {
	0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
	0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
	0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
	0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
	0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
	0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
	0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
	0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
}

@(private = "file")
rotr :: #force_inline proc "contextless" (x: u32, n: u32) -> u32 {
	return (x >> n) | (x << (32 - n))
}

@(private = "file")
compress :: proc "contextless" (h: ^[8]u32, block: []u8) {
	w: [64]u32
	for i in 0 ..< 16 {
		w[i] = u32(block[i * 4]) << 24 | u32(block[i * 4 + 1]) << 16 | u32(block[i * 4 + 2]) << 8 | u32(block[i * 4 + 3])
	}
	for i in 16 ..< 64 {
		s0 := rotr(w[i - 15], 7) ~ rotr(w[i - 15], 18) ~ (w[i - 15] >> 3)
		s1 := rotr(w[i - 2], 17) ~ rotr(w[i - 2], 19) ~ (w[i - 2] >> 10)
		w[i] = w[i - 16] + s0 + w[i - 7] + s1
	}
	a, b, c, d, e, f, g, hh := h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]
	for i in 0 ..< 64 {
		s1 := rotr(e, 6) ~ rotr(e, 11) ~ rotr(e, 25)
		ch := (e & f) ~ (~e & g)
		t1 := hh + s1 + ch + K[i] + w[i]
		s0 := rotr(a, 2) ~ rotr(a, 13) ~ rotr(a, 22)
		maj := (a & b) ~ (a & c) ~ (b & c)
		t2 := s0 + maj
		hh = g
		g = f
		f = e
		e = d + t1
		d = c
		c = b
		b = a
		a = t1 + t2
	}
	h[0] += a
	h[1] += b
	h[2] += c
	h[3] += d
	h[4] += e
	h[5] += f
	h[6] += g
	h[7] += hh
}

sha256 :: proc "contextless" (data: []u8) -> (out: [32]u8) {
	h := [8]u32{0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19}
	full := len(data) / 64
	for i in 0 ..< full {
		compress(&h, data[i * 64:i * 64 + 64])
	}
	tail: [128]u8
	rest := len(data) - full * 64
	copy(tail[:], data[full * 64:])
	tail[rest] = 0x80
	blocks := 1 if rest < 56 else 2
	bits := u64(len(data)) * 8
	for i in 0 ..< 8 {
		tail[blocks * 64 - 1 - i] = u8(bits >> (u64(i) * 8))
	}
	for i in 0 ..< blocks {
		compress(&h, tail[i * 64:i * 64 + 64])
	}
	for word, i in h {
		out[i * 4] = u8(word >> 24)
		out[i * 4 + 1] = u8(word >> 16)
		out[i * 4 + 2] = u8(word >> 8)
		out[i * 4 + 3] = u8(word)
	}
	return
}

@(private = "file")
HEX_DIGITS := "0123456789abcdef"

// The first 16 bytes of the hash in lowercase hex, as the macOS app names
// thumbnails and looks.
digest16 :: proc(data: []u8, allocator := context.allocator) -> string {
	hash := sha256(data)
	out := make([]u8, 32, allocator)
	for i in 0 ..< 16 {
		out[i * 2] = HEX_DIGITS[hash[i] >> 4]
		out[i * 2 + 1] = HEX_DIGITS[hash[i] & 0xF]
	}
	return string(out)
}
