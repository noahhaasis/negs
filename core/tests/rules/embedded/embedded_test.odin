// core/src/rules/embedded: windows/src/embedded_preview.rs's own tests.
package embedded_tests

import "core:fmt"
import "core:os"
import "core:strings"
import "core:testing"
import "../../../src/rules/embedded"

@(private)
le16 :: proc(out: ^[dynamic]u8, v: u16) {
	append(out, u8(v), u8(v >> 8))
}

@(private)
le32 :: proc(out: ^[dynamic]u8, v: u32) {
	append(out, u8(v), u8(v >> 8), u8(v >> 16), u8(v >> 24))
}

@(private)
entry :: proc(out: ^[dynamic]u8, tag, kind: u16, n, value: u32) {
	le16(out, tag)
	le16(out, kind)
	le32(out, n)
	le32(out, value)
}

@(private)
jpeg :: proc(out: ^[dynamic]u8, sof: u8, w, h: u16) {
	start := len(out)
	append(out, 0xff, 0xd8, 0xff, 0xe1, 0x00, 0x04, 0, 0, 0xff, sof, 0x00, 0x11, 8)
	append(out, u8(h >> 8), u8(h), u8(w >> 8), u8(w))
	for len(out) < start + 200 {
		append(out, 0)
	}
}

// A little-endian TIFF: IFD0 with an orientation and a JPEG by
// JPEGInterchangeFormat, one SubIFD with a bigger JPEG strip, and a
// lossless-JPEG raw strip in another.
@(private)
tiff :: proc() -> [dynamic]u8 {
	f: [dynamic]u8
	append(&f, 'I', 'I', '*', 0)
	le32(&f, 8)
	ifd0 := u32(8)
	sub_list := ifd0 + 2 + 4 * 12 + 4
	sub_a := sub_list + 8
	sub_b := sub_a + 2 + 3 * 12 + 4
	data := sub_b + 2 + 3 * 12 + 4
	small_at, big_at, raw_at := data, data + 200, data + 600
	le16(&f, 4)
	entry(&f, 0x0112, 3, 1, 6)
	entry(&f, 0x014a, 4, 2, sub_list)
	entry(&f, 0x0201, 4, 1, small_at)
	entry(&f, 0x0202, 4, 1, 200)
	le32(&f, 0)
	le32(&f, sub_a)
	le32(&f, sub_b)
	le16(&f, 3)
	entry(&f, 0x0103, 3, 1, 6)
	entry(&f, 0x0111, 4, 1, big_at)
	entry(&f, 0x0117, 4, 1, 200)
	le32(&f, 0)
	le16(&f, 3)
	entry(&f, 0x0103, 3, 1, 7)
	entry(&f, 0x0111, 4, 1, raw_at)
	entry(&f, 0x0117, 4, 1, 200)
	le32(&f, 0)
	assert(u32(len(f)) == data)
	jpeg(&f, 0xc0, 640, 480)
	jpeg(&f, 0xc0, 6000, 4000)
	jpeg(&f, 0xc2, 1620, 1080)
	jpeg(&f, 0xc3, 6000, 4000)
	return f
}

@(test)
finds_the_baseline_and_progressive_previews_and_not_the_lossless_raw :: proc(t: ^testing.T) {
	file := tiff()
	defer delete(file)
	src := embedded.source_bytes(file[:])
	p, ok := embedded.find_in(&src)
	defer embedded.previews_destroy(&p)
	testing.expect(t, ok)
	testing.expect_value(t, len(p.jpegs), 2)
	testing.expect_value(t, p.jpegs[0].width, 640)
	testing.expect_value(t, p.jpegs[1].width, 6000)
	testing.expect(t, p.has_orientation && p.orientation == 6)
	big, _ := embedded.largest(p.jpegs[:])
	testing.expect_value(t, big.width, 6000)
	r, _ := embedded.ratio(p.jpegs[:], p.orientation, p.has_orientation)
	testing.expect(t, abs(r - 4000.0 / 6000.0) < 1e-9)
}

@(test)
the_smallest_big_enough_of_the_largest_ones_shape :: proc(t: ^testing.T) {
	j :: proc(w, h: u32) -> embedded.Jpeg {return {u64(w), 1000, w, h}}
	jpegs := []embedded.Jpeg{j(160, 120), j(1620, 1080), j(6000, 4000), j(570, 380)}
	pick :: proc(jpegs: []embedded.Jpeg, px: u32) -> u32 {
		got, _ := embedded.for_size(jpegs, px)
		return got.width
	}
	testing.expect_value(t, pick(jpegs, 432), 570)
	testing.expect_value(t, pick(jpegs, 768), 1620)
	testing.expect_value(t, pick(jpegs, 8000), 6000)
	// The 4:3 one is never chosen, however small the size.
	testing.expect_value(t, pick(jpegs, 100), 570)
	_, none := embedded.for_size(nil, 100)
	testing.expect(t, !none)
}

@(test)
nothing_from_what_isnt_tiff_or_raf :: proc(t: ^testing.T) {
	cr3 := "\x00\x00\x00\x18ftypcrx \x00\x00\x00\x01"
	src := embedded.source_bytes(transmute([]u8)cr3)
	_, ok := embedded.find_in(&src)
	testing.expect(t, !ok)
	short := "short"
	src = embedded.source_bytes(transmute([]u8)short)
	_, ok = embedded.find_in(&src)
	testing.expect(t, !ok)
}

@(test)
reads_files_a_piece_at_a_time :: proc(t: ^testing.T) {
	file := tiff()
	defer delete(file)
	defer free_all(context.temp_allocator)
	base := os.get_env("TMPDIR", context.temp_allocator)
	if base == "" {
		base = "/tmp"
	}
	path := fmt.aprintf("%s/negs-core-embedded-%d.tiff", strings.trim_right(base, "/"), os.get_pid(), allocator = context.temp_allocator)
	testing.expect(t, os.write_entire_file(path, file[:]) == nil)
	defer os.remove(path)
	p, ok := embedded.find(path)
	defer embedded.previews_destroy(&p)
	testing.expect(t, ok && len(p.jpegs) == 2)
	bytes, read := embedded.read_file(path, p.jpegs[1].offset, p.jpegs[1].length)
	defer delete(bytes)
	testing.expect(t, read && len(bytes) == 200 && bytes[0] == 0xff && bytes[9] == 0xc0)
	_, past := embedded.read_file(path, u64(len(file)) - 10, 11)
	testing.expect(t, !past)
	// Into the host's memory: asked, then filled.
	j := p.jpegs[1]
	testing.expect(t, embedded.read_file_into(path, j.offset, j.length, nil, false))
	testing.expect(t, !embedded.read_file_into(path, u64(len(file)) - 10, 11, nil, false))
	testing.expect(t, !embedded.read_file_into(path, max(u64), 2, nil, false))
	into: [200]u8
	testing.expect(t, embedded.read_file_into(path, j.offset, j.length, into[:], true))
	testing.expect(t, into[0] == 0xff && into[9] == 0xc0)
	testing.expect(t, !embedded.read_file_into(path, j.offset, j.length, into[:199], true))
	_, missing := embedded.find("no such file.nef")
	testing.expect(t, !missing)
}

// Cyclic and absurd IFDs end: IFD0's next is itself, and its SubIFDs
// point back at it.
@(test)
cycles_end :: proc(t: ^testing.T) {
	f: [dynamic]u8
	defer delete(f)
	append(&f, 'M', 'M', 0, 42, 0, 0, 0, 8)
	// IFD0 at 8 (big-endian): a SubIFD list of 40 pointing at 8.
	append(&f, 0, 1, 0x01, 0x4a, 0, 4, 0, 0, 0, 40, 0, 0, 0, 26, 0, 0, 0, 8)
	for _ in 0 ..< 40 {
		append(&f, 0, 0, 0, 8)
	}
	src := embedded.source_bytes(f[:])
	p, ok := embedded.find_in(&src)
	defer embedded.previews_destroy(&p)
	testing.expect(t, ok && len(p.jpegs) == 0)
}
