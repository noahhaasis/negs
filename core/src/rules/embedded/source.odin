package embedded

// Where the header is read from: bytes in memory, or an open file read a
// piece at a time (the Rust module's `At`: `&[u8]` and its file Reader).
Source :: struct {
	bytes:   []u8,
	file:    File,
	is_file: bool,
	size:    u64,
}

source_bytes :: proc "contextless" (data: []u8) -> Source {
	return Source{bytes = data, size = u64(len(data))}
}

// `length` bytes at `offset`, or false when they aren't all there (or the
// file can't be read). A file's are read into `buf` (at least `length`
// long); bytes in memory are viewed where they are.
read_at :: proc(s: ^Source, offset: u64, length: int, buf: []u8) -> ([]u8, bool) {
	end := offset + u64(length)
	if end < offset || end > s.size {
		return nil, false
	}
	if !s.is_file {
		return s.bytes[offset:end], true
	}
	out := buf[:length]
	if !file_read_exact(s.file, offset, out) {
		return nil, false
	}
	return out, true
}

// The bytes of `jpeg` in a file: a block in the context's allocator
// (empty for a zero length), or false.
read_file :: proc(path: string, offset, length: u64, allocator := context.allocator) -> ([]u8, bool) {
	f, size, ok := file_open(path)
	if !ok {
		return nil, false
	}
	defer file_close(f)
	end := offset + length
	if end < offset || end > size {
		return nil, false
	}
	out := make([]u8, int(length), allocator)
	if !file_read_exact(f, offset, out) {
		delete(out, allocator)
		return nil, false
	}
	return out, true
}
