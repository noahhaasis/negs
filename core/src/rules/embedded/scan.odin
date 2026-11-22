package embedded

MAX_IFDS :: 32
MAX_ENTRIES :: 1000

@(private)
Walk :: struct {
	src:             ^Source,
	little:          bool,
	// (offset, length) of every JPEG an IFD names, in the order named.
	named:           [dynamic][2]u64,
	orientation:     u16,
	has_orientation: bool,
	visited:         [MAX_IFDS]u64,
	visited_len:     int,
}

@(private)
u16_of :: proc "contextless" (little: bool, b: []u8) -> u16 {
	if little {
		return u16(b[0]) | u16(b[1]) << 8
	}
	return u16(b[1]) | u16(b[0]) << 8
}

@(private)
u32_of :: proc "contextless" (little: bool, b: []u8) -> u32 {
	if little {
		return u32(b[0]) | u32(b[1]) << 8 | u32(b[2]) << 16 | u32(b[3]) << 24
	}
	return u32(b[3]) | u32(b[2]) << 8 | u32(b[1]) << 16 | u32(b[0]) << 24
}

@(private)
visited :: proc "contextless" (w: ^Walk, offset: u64) -> bool {
	for v in w.visited[:w.visited_len] {
		if v == offset {
			return true
		}
	}
	return false
}

// The previews `src` names; false when it isn't a TIFF or RAF header (or
// the header can't be read). Found but empty is true with no JPEGs.
find_in :: proc(src: ^Source, allocator := context.allocator) -> (out: Previews, ok: bool) {
	head_buf: [16]u8
	head := read_at(src, 0, int(min(src.size, 16)), head_buf[:]) or_return
	if len(head) >= 16 && string(head[:15]) == "FUJIFILMCCD-RAW" {
		return raf(src, allocator)
	}
	if len(head) < 8 {
		return {}, false
	}
	little: bool
	switch string(head[:2]) {
	case "II":
		little = true
	case "MM":
		little = false
	case:
		return {}, false
	}
	// TIFF's 42, Olympus' "RO"/"RS" and Panasonic's 0x55.
	switch u16_of(little, head[2:4]) {
	case 42, 0x4f52, 0x5352, 0x55:
	case:
		return {}, false
	}
	w := Walk {
		src    = src,
		little = little,
		named  = make([dynamic][2]u64, allocator),
	}
	defer delete(w.named)
	chain(&w, u64(u32_of(little, head[4:8])), 0)
	out.jpegs = make([dynamic]Jpeg, allocator)
	outer: for pair in w.named {
		jpeg := jpeg_at(src, pair[0], pair[1]) or_continue
		for j in out.jpegs {
			if j.offset == jpeg.offset {
				continue outer
			}
		}
		append(&out.jpegs, jpeg)
	}
	out.orientation, out.has_orientation = w.orientation, w.has_orientation
	return out, true
}

// RAF: the header's JPEG offset and length at 84 and 88, big-endian.
@(private)
raf :: proc(src: ^Source, allocator := context.allocator) -> (out: Previews, ok: bool) {
	field_buf: [8]u8
	field := read_at(src, 84, 8, field_buf[:]) or_return
	out.jpegs = make([dynamic]Jpeg, allocator)
	if jpeg, found := jpeg_at(src, u64(u32_of(false, field[:4])), u64(u32_of(false, field[4:]))); found {
		append(&out.jpegs, jpeg)
	}
	return out, true
}

// An IFD and those chained after it.
@(private)
chain :: proc(w: ^Walk, first: u64, depth: u32) {
	offset := first
	ifd0 := depth == 0
	for offset != 0 && w.visited_len < MAX_IFDS && !visited(w, offset) {
		w.visited[w.visited_len] = offset
		w.visited_len += 1
		next, ok := ifd(w, offset, depth, ifd0)
		if !ok {
			return
		}
		ifd0 = false
		offset = next
	}
}

// Reads one IFD's previews; the next IFD's offset.
@(private)
ifd :: proc(w: ^Walk, offset: u64, depth: u32, ifd0: bool) -> (next: u64, ok: bool) {
	little := w.little
	count_buf: [2]u8
	count := int(u16_of(little, read_at(w.src, offset, 2, count_buf[:]) or_return))
	if count == 0 || count > MAX_ENTRIES {
		return 0, false
	}
	table_buf: [MAX_ENTRIES * 12 + 4]u8 = ---
	table := read_at(w.src, offset + 2, count * 12 + 4, table_buf[:]) or_return

	Opt :: struct {
		value: u64,
		ok:    bool,
	}
	interchange, interchange_length, strip, strip_length, compression, exif: Opt
	subs := make([dynamic]u64, context.allocator)
	defer delete(subs)
	for e in 0 ..< count {
		entry := table[e * 12:][:12]
		tag, kind, n := u16_of(little, entry[0:2]), u16_of(little, entry[2:4]), u32_of(little, entry[4:8])
		value := entry[8:12]
		// The first value of a SHORT or LONG.
		first: Opt
		switch kind {
		case 3:
			first = {u64(u16_of(little, value)), true}
		case 4, 13:
			first = {u64(u32_of(little, value)), true}
		}
		switch tag {
		case 0x0103:
			compression = first
		case 0x0111:
			if n == 1 {
				strip = first
			}
		case 0x0117:
			if n == 1 {
				strip_length = first
			}
		case 0x0201:
			interchange = first
		case 0x0202:
			interchange_length = first
		case 0x002e:
			// RW2's JpgFromRaw: the JPEG's bytes as the value.
			if kind == 7 && n > 4 {
				append(&w.named, [2]u64{u64(u32_of(little, value)), u64(n)})
			}
		case 0x0112:
			if (ifd0 || !w.has_orientation) && first.ok && first.value >= 1 && first.value <= 8 {
				w.orientation, w.has_orientation = u16(first.value), true
			}
		case 0x014a:
			if depth < 3 {
				if n == 1 {
					if first.ok {
						append(&subs, first.value)
					}
				} else if kind == 4 || kind == 13 {
					list_buf: [MAX_IFDS * 4]u8
					m := int(min(n, MAX_IFDS))
					if list, found := read_at(w.src, u64(u32_of(little, value)), m * 4, list_buf[:]); found {
						for i in 0 ..< m {
							append(&subs, u64(u32_of(little, list[i * 4:][:4])))
						}
					}
				}
			}
		case 0x8769:
			if depth < 3 {
				exif = first
			}
		}
	}
	if interchange.ok && interchange_length.ok {
		append(&w.named, [2]u64{interchange.value, interchange_length.value})
	}
	// A single strip of old- or new-style JPEG (a lossless raw is weeded
	// out by its SOF3).
	if strip.ok && strip_length.ok && compression.ok && (compression.value == 6 || compression.value == 7) {
		append(&w.named, [2]u64{strip.value, strip_length.value})
	}
	next = u64(u32_of(little, table[count * 12:][:4]))
	for sub in subs {
		chain(w, sub, depth + 1)
	}
	if exif.ok {
		// Only its orientation, when IFD0 has none.
		if !visited(w, exif.value) && w.visited_len < MAX_IFDS {
			w.visited[w.visited_len] = exif.value
			w.visited_len += 1
			ifd(w, exif.value, depth + 1, false)
		}
	}
	return next, true
}

@(private)
be16 :: proc "contextless" (b: []u8) -> u16 {
	return u16(b[1]) | u16(b[0]) << 8
}

// A baseline or progressive JPEG at `offset`: its stored size from its
// frame header. A lossless or arithmetic one (a raw strip), or anything
// else, gives false.
@(private)
jpeg_at :: proc(src: ^Source, offset, length: u64) -> (jpeg: Jpeg, ok: bool) {
	end := offset + length
	if length < 64 || end < offset || end > src.size {
		return {}, false
	}
	buf: [5]u8
	soi := read_at(src, offset, 2, buf[:]) or_return
	if soi[0] != 0xff || soi[1] != 0xd8 {
		return {}, false
	}
	p := offset + 2
	for _ in 0 ..< 64 {
		if p + 4 > end {
			return {}, false
		}
		marker := read_at(src, p, 4, buf[:]) or_return
		if marker[0] != 0xff {
			return {}, false
		}
		kind := marker[1]
		if kind == 0xff {
			p += 1
			continue
		}
		size := u64(be16(marker[2:4]))
		switch kind {
		case 0xc0 ..= 0xc2:
			sof := read_at(src, p + 4, 5, buf[:]) or_return
			height, width := u32(be16(sof[1:3])), u32(be16(sof[3:5]))
			if width > 0 && height > 0 {
				return Jpeg{offset, length, width, height}, true
			}
			return {}, false
		// Lossless, hierarchical or arithmetic: not a picture WIC shows.
		case 0xc3, 0xc5 ..= 0xc7, 0xc9 ..= 0xcb, 0xcd ..= 0xcf, 0xd8, 0xd9, 0xda:
			return {}, false
		case:
			p += 2 + size
		}
	}
	return {}, false
}

// `path`'s previews: false when it has none this can find, or can't be read.
find :: proc(path: string, allocator := context.allocator) -> (Previews, bool) {
	f, size, opened := file_open(path)
	if !opened {
		return {}, false
	}
	defer file_close(f)
	src := Source{file = f, is_file = true, size = size}
	p, ok := find_in(&src, allocator)
	if !ok {
		return {}, false
	}
	if len(p.jpegs) == 0 {
		previews_destroy(&p)
		return {}, false
	}
	return p, true
}
