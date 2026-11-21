// The rating an export carries, as Lightroom writes it: `xmp:Rating`, the
// stars or −1 for a reject (`ExportRating` in `BatchExport.swift`;
// xmp.rs). Added to the finished file without touching its image data.
package library

import "core:fmt"
import "core:os"
import "core:slice"
import "core:strings"

// The rating an export embeds: the stars, or −1 for a reject; false (no
// packet) for an unrated, unrejected photograph.
export_rating :: proc(s: Summary) -> (i32, bool) {
	if s.flag == .Reject {
		return -1, true
	}
	if s.rating > 0 {
		return i32(s.rating), true
	}
	return 0, false
}

// The XMP packet with only the rating (`ExportRating.packet`).
xmp_packet :: proc(rating: i32, allocator := context.allocator) -> string {
	return fmt.aprintf(
		"<?xpacket begin=\"\xef\xbb\xbf\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"%d\"/></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>",
		rating,
		allocator = allocator,
	)
}

@(private = "file")
PNG_SIGNATURE := [8]u8{0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A}
@(private = "file")
XMP_KEYWORD :: "XML:com.adobe.xmp"
@(private = "file")
JPEG_XMP :: "http://ns.adobe.com/xap/1.0/\x00"

@(private = "file")
starts_with :: proc(data: []u8, prefix: string) -> bool {
	return len(data) >= len(prefix) && string(data[:len(prefix)]) == prefix
}

// `bytes` (a PNG, JPEG or TIFF) with the rating, replacing any XMP packet
// it had; false for anything else.
xmp_embed :: proc(bytes: []u8, rating: i32, allocator := context.allocator) -> ([]u8, bool) {
	if len(bytes) >= 8 && slice.equal(bytes[:8], PNG_SIGNATURE[:]) {
		return embed_png(bytes, rating, allocator)
	}
	if len(bytes) >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
		return embed_jpeg(bytes, rating, allocator)
	}
	if starts_with(bytes, "II*\x00") || starts_with(bytes, "MM\x00*") {
		return embed_tiff(bytes, rating, allocator)
	}
	return nil, false
}

// Adds the rating to the file at `path` through a hidden file beside it
// and a rename. False when it isn't a PNG, JPEG or TIFF.
xmp_embed_in_file :: proc(path: string, rating: i32) -> (bool, Io_Error) {
	ta := context.temp_allocator
	bytes, err := fs_read(path, max(int), ta)
	if err != 0 {
		return false, err
	}
	out, ok := xmp_embed(bytes, rating, ta)
	if !ok {
		return false, 0
	}
	dir := path[:len(path) - len(file_name(path))]
	staging := fmt.aprintf("%s.negs-rating-%d-%s", dir, os.get_pid(), file_name(path), allocator = ta)
	err = fs_write(staging, out)
	if err == 0 {
		err = fs_rename(staging, path)
	}
	if err != 0 {
		fs_remove(staging)
		return false, err
	}
	return true, 0
}

// The rating a file carries: the last packet's.
xmp_read_rating :: proc(bytes: []u8) -> (i32, bool) {
	text := string(bytes)
	at := strings.last_index(text, "xmp:Rating")
	if at < 0 {
		return 0, false
	}
	rest := strings.trim_left(text[at + len("xmp:Rating"):], "=\"> ")
	end := 0
	for end < len(rest) && (rest[end] == '-' || (rest[end] >= '0' && rest[end] <= '9')) {
		end += 1
	}
	value := rest[:end]
	negative := false
	digits := value
	if len(digits) > 0 && (digits[0] == '-' || digits[0] == '+') {
		negative = digits[0] == '-'
		digits = digits[1:]
	}
	if len(digits) == 0 {
		return 0, false
	}
	n: i64 = 0
	for c in transmute([]u8)digits {
		if c < '0' || c > '9' {
			return 0, false
		}
		n = n * 10 + i64(c - '0')
		if n > 1 << 31 {
			return 0, false
		}
	}
	if negative {
		n = -n
	}
	if n > i64(max(i32)) || n < i64(min(i32)) {
		return 0, false
	}
	return i32(n), true
}

@(private = "file")
crc32 :: proc(data: []u8) -> u32 {
	c: u32 = 0xFFFF_FFFF
	for b in data {
		c ~= u32(b)
		for _ in 0 ..< 8 {
			c = 0xEDB8_8320 ~ (c >> 1) if c & 1 != 0 else c >> 1
		}
	}
	return c ~ 0xFFFF_FFFF
}

@(private = "file")
be32 :: proc(v: u32) -> [4]u8 {
	return {u8(v >> 24), u8(v >> 16), u8(v >> 8), u8(v)}
}

@(private = "file")
Range :: struct {
	start, end: int,
}

// A PNG gets the packet as an `iTXt` chunk before its image data,
// replacing one already there; every other byte stays.
@(private = "file")
embed_png :: proc(data: []u8, rating: i32, allocator: Allocator) -> ([]u8, bool) {
	ta := context.temp_allocator
	Chunk :: struct {
		kind:  string,
		range: Range,
	}
	chunks := make([dynamic]Chunk, 0, 8, ta)
	at := 8
	for at + 12 <= len(data) {
		length := int(u32(data[at]) << 24 | u32(data[at + 1]) << 16 | u32(data[at + 2]) << 8 | u32(data[at + 3]))
		if at + 12 + length > len(data) {
			return nil, false
		}
		append(&chunks, Chunk{string(data[at + 4:at + 8]), {at, at + 12 + length}})
		at += 12 + length
	}
	image := -1
	for c, i in chunks {
		if c.kind == "IDAT" {
			image = i
			break
		}
	}
	if image < 0 {
		return nil, false
	}
	body := make([dynamic]u8, 0, 512, ta)
	append(&body, "iTXt")
	append(&body, XMP_KEYWORD)
	append(&body, 0, 0, 0, 0, 0)
	append(&body, xmp_packet(rating, ta))
	out := make([dynamic]u8, 0, len(data) + len(body) + 8, allocator)
	append(&out, ..PNG_SIGNATURE[:])
	for c, index in chunks {
		if index == image {
			length := be32(u32(len(body) - 4))
			append(&out, ..length[:])
			append(&out, ..body[:])
			crc := be32(crc32(body[:]))
			append(&out, ..crc[:])
		}
		start := c.range.start + 8
		stop := min(c.range.end, start + len(XMP_KEYWORD))
		is_xmp := c.kind == "iTXt" && string(data[start:stop]) == XMP_KEYWORD
		if !is_xmp {
			append(&out, ..data[c.range.start:c.range.end])
		}
	}
	return out[:], true
}

// A JPEG gets the packet as an APP1 segment after its JFIF and Exif
// segments, replacing one already there.
@(private = "file")
embed_jpeg :: proc(data: []u8, rating: i32, allocator: Allocator) -> ([]u8, bool) {
	ta := context.temp_allocator
	at := 2
	insert := 2
	segments := make([dynamic]Range, 0, 8, ta)
	for at + 4 <= len(data) && data[at] == 0xFF {
		marker := data[at + 1]
		if marker == 0xDA || marker == 0xD9 {
			break
		}
		length := int(u16(data[at + 2]) << 8 | u16(data[at + 3]))
		if length < 2 || at + 2 + length > len(data) {
			return nil, false
		}
		r := Range{at, at + 2 + length}
		if marker == 0xE0 || (marker == 0xE1 && !starts_with(data[at + 4:r.end], JPEG_XMP)) {
			if insert == r.start {
				insert = r.end
			}
		}
		append(&segments, r)
		at += 2 + length
	}
	payload := make([dynamic]u8, 0, 512, ta)
	append(&payload, JPEG_XMP)
	append(&payload, xmp_packet(rating, ta))
	if len(payload) + 2 > 0xFFFF {
		return nil, false
	}
	segment := make([dynamic]u8, 0, len(payload) + 4, ta)
	append(&segment, 0xFF, 0xE1, u8((len(payload) + 2) >> 8), u8(len(payload) + 2))
	append(&segment, ..payload[:])
	old_xmp := make([dynamic]Range, 0, 2, ta)
	for r in segments {
		if data[r.start + 1] == 0xE1 && starts_with(data[r.start + 4:r.end], JPEG_XMP) {
			append(&old_xmp, r)
		}
	}
	out := make([dynamic]u8, 0, len(data) + len(segment), allocator)
	position := 0
	for r in old_xmp {
		if r.start >= insert {
			break
		}
		append(&out, ..data[position:r.start])
		position = r.end
	}
	append(&out, ..data[position:insert])
	append(&out, ..segment[:])
	position = insert
	for r in old_xmp {
		if r.start < insert {
			continue
		}
		append(&out, ..data[position:r.start])
		position = r.end
	}
	append(&out, ..data[position:])
	return out[:], true
}

// A TIFF gets the packet as tag 700 of its first image: the image's
// directory is written again at the end of the file with the tag, and the
// header points to it. The image data stays where it is.
@(private = "file")
embed_tiff :: proc(data: []u8, rating: i32, allocator: Allocator) -> ([]u8, bool) {
	ta := context.temp_allocator
	little := starts_with(data, "II")
	u16_at :: proc(data: []u8, little: bool, at: int) -> (u16, bool) {
		if at < 0 || at + 2 > len(data) {
			return 0, false
		}
		if little {
			return u16(data[at]) | u16(data[at + 1]) << 8, true
		}
		return u16(data[at]) << 8 | u16(data[at + 1]), true
	}
	u32_at :: proc(data: []u8, little: bool, at: int) -> (u32, bool) {
		if at < 0 || at + 4 > len(data) {
			return 0, false
		}
		if little {
			return u32(data[at]) | u32(data[at + 1]) << 8 | u32(data[at + 2]) << 16 | u32(data[at + 3]) << 24, true
		}
		return u32(data[at]) << 24 | u32(data[at + 1]) << 16 | u32(data[at + 2]) << 8 | u32(data[at + 3]), true
	}
	put16 :: proc(little: bool, v: u16) -> [2]u8 {
		return {u8(v), u8(v >> 8)} if little else {u8(v >> 8), u8(v)}
	}
	put32 :: proc(little: bool, v: u32) -> [4]u8 {
		return {u8(v), u8(v >> 8), u8(v >> 16), u8(v >> 24)} if little else {u8(v >> 24), u8(v >> 16), u8(v >> 8), u8(v)}
	}
	ifd32, ok := u32_at(data, little, 4)
	if !ok {
		return nil, false
	}
	ifd := int(ifd32)
	count16, has_count := u16_at(data, little, ifd)
	if !has_count {
		return nil, false
	}
	count := int(count16)
	next, has_next := u32_at(data, little, ifd + 2 + count * 12)
	if !has_next {
		return nil, false
	}
	entries := make([dynamic][12]u8, 0, count + 1, ta)
	for i in 0 ..< count {
		start := ifd + 2 + i * 12
		if start + 12 > len(data) {
			return nil, false
		}
		entry: [12]u8
		copy(entry[:], data[start:start + 12])
		tag, _ := u16_at(data, little, start)
		if tag != 700 {
			append(&entries, entry)
		}
	}
	xmp := transmute([]u8)xmp_packet(rating, ta)
	out := make([dynamic]u8, 0, len(data) + len(xmp) + (count + 1) * 12 + 8, allocator)
	append(&out, ..data)
	if len(out) % 2 == 1 {
		append(&out, 0)
	}
	xmp_at := u32(len(out))
	append(&out, ..xmp)
	if len(out) % 2 == 1 {
		append(&out, 0)
	}
	tag: [12]u8
	t16 := put16(little, 700)
	one := put16(little, 1) // BYTE
	n32 := put32(little, u32(len(xmp)))
	at32 := put32(little, xmp_at)
	copy(tag[0:2], t16[:])
	copy(tag[2:4], one[:])
	copy(tag[4:8], n32[:])
	copy(tag[8:12], at32[:])
	append(&entries, tag)
	Sort :: struct {
		little: bool,
	}
	if little {
		slice.stable_sort_by(entries[:], proc(a, b: [12]u8) -> bool {return u16(a[0]) | u16(a[1]) << 8 < u16(b[0]) | u16(b[1]) << 8})
	} else {
		slice.stable_sort_by(entries[:], proc(a, b: [12]u8) -> bool {return u16(a[0]) << 8 | u16(a[1]) < u16(b[0]) << 8 | u16(b[1])})
	}
	if u64(len(out)) > u64(max(u32)) {
		delete(out)
		return nil, false
	}
	new_ifd := u32(len(out))
	n16 := put16(little, u16(len(entries)))
	append(&out, ..n16[:])
	for e in entries {
		e := e
		append(&out, ..e[:])
	}
	nx := put32(little, next)
	append(&out, ..nx[:])
	header := put32(little, new_ifd)
	copy(out[4:8], header[:])
	return out[:], true
}
