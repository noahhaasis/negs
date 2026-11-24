// One lossless startup frame (windows/src/rendered_preview.rs): a single
// file holding its identity, geometry and exact RGBA pixels, written
// through a staging file. The host's background writer orders saves and
// invalidations; this package is what each of them does.
//
// The file: the header as serde_json writes `Header { key, frame,
// checksum }` (Frame's fields by their Rust names, the pixels left out),
// a newline, then the pixels.
package preview

import "core:slice"
import "core:strings"
import "../../state"
import "../../value"
import files "../files"
import "../serde"

MAX_PIXELS :: 8192 * 8192 * 4
MAX_HEADER :: 1_048_576
// The key's own version (`"schema": 3`).
KEY_SCHEMA :: 3

// `Frame` without its pixels. Laid out as negs_preview_frame_t.
Frame :: struct {
	width:                     u32,
	height:                    u32,
	photo_width:               f64,
	photo_height:              f64,
	// For Image Size readouts: the decoded source and the uncapped working
	// frame.
	source_width:              f64,
	source_height:             f64,
	working_width:             f64,
	working_height:            f64,
	raw:                       u8,
	// Shows the Lens corrections switch before the first render.
	lens_correction_supported: u8,
}

// Why a read failed (`read`'s errors).
Read_Error :: enum u8 {
	None,
	// The file can't be read (`?` on an io::Error).
	Io,
	Size,
	Header,
	// The header isn't serde's Header.
	Parse,
	Mismatch,
	Photo_Size,
	Pixels,
}

// MARK: The key

// `{path}|{len}|{modified ns}|{created ns}`; false when any of them can't
// be had (on a file system without birth times, std's `created()` fails,
// so there is no key and no cached frame).
@(private = "file")
stamp :: proc(path: string, allocator := context.allocator) -> (text: string, ok: bool) {
	st := files.stat(path) or_return
	modified := st.modified.? or_return
	created := st.created.? or_return
	b := strings.builder_make(0, len(path) + 64, allocator)
	strings.write_string(&b, files.display(path, context.temp_allocator))
	strings.write_byte(&b, '|')
	value.write_u64(&b, st.size)
	strings.write_byte(&b, '|')
	write_u128(&b, files.nanos(modified))
	strings.write_byte(&b, '|')
	write_u128(&b, files.nanos(created))
	return strings.to_string(b), true
}

@(private = "file")
write_u128 :: proc(b: ^strings.Builder, v: u128) {
	buf: [40]u8
	i := len(buf)
	n := v
	for {
		i -= 1
		buf[i] = u8('0' + n % 10)
		n /= 10
		if n == 0 {
			break
		}
	}
	strings.write_bytes(b, buf[i:])
}

// Every file under `dir`, folders followed (links neither), stamped.
@(private = "file")
data_stamps :: proc(dir: string, out: ^[dynamic]string) -> bool {
	entries := files.read_dir(dir, context.temp_allocator) or_return
	for e in entries {
		switch e.kind {
		case .Dir:
			data_stamps(files.join(dir, e.name, context.temp_allocator), out) or_return
		case .File:
			s := stamp(files.join(dir, e.name, context.temp_allocator), context.temp_allocator) or_return
			append(out, s)
		case .Other:
		}
	}
	return true
}

// `key` with `rendered` already the JSON value of `state.rendered()`
// (`serde_json::to_value`): `{"files":[…],"schema":3,"state":…}`, the
// stamps of the source, the helper, the data folder and every file in it,
// sorted. False when any can't be stamped.
key_of_rendered :: proc(source, helper, data: string, rendered: value.Value, allocator := context.allocator) -> (text: string, ok: bool) {
	stamps := make([dynamic]string, 0, 64, context.temp_allocator)
	for path in ([]string{source, helper, data}) {
		s := stamp(path, context.temp_allocator) or_return
		append(&stamps, s)
	}
	data_stamps(data, &stamps) or_return
	slice.sort(stamps[:])
	b := strings.builder_make(0, 4096, allocator)
	strings.write_string(&b, `{"files":[`)
	for s, i in stamps {
		if i > 0 {
			strings.write_byte(&b, ',')
		}
		value.write_string(&b, s)
	}
	strings.write_string(&b, `],"schema":3,"state":`)
	value.write_compact(&b, rendered)
	strings.write_byte(&b, '}')
	return strings.to_string(b), true
}

// `key`: compares the entire canonical identity, so settings cannot
// hash-collide. Crop, rotation and border are applied separately by the
// restored canvas; with an Image Size target the crop's extent is part of
// `rendered()`.
key :: proc(source, helper, data: string, s: state.Edit_State, allocator := context.allocator) -> (string, bool) {
	rendered := state.rendered(s, context.temp_allocator)
	v := state.to_value(rendered, context.temp_allocator)
	return key_of_rendered(source, helper, data, v, allocator)
}

// MARK: The header

FRAME_FIELDS := [?]string {
	"width",
	"height",
	"photo_width",
	"photo_height",
	"source_width",
	"source_height",
	"working_width",
	"working_height",
	"raw",
	"display_space",
	"lens_correction_supported",
}

HEADER_FIELDS := [?]string{"key", "frame", "checksum"}

@(private = "file")
Read_Frame :: struct {
	frame:         Frame,
	display_space: string,
}

@(private = "file")
read_frame :: proc(p: ^value.Parser) -> (out: Read_Frame, ok: bool) {
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		r := (^Read_Frame)(data)
		f := &r.frame
		switch index {
		case 0:
			f.width = serde.read_u32(p) or_return
		case 1:
			f.height = serde.read_u32(p) or_return
		case 2:
			f.photo_width = serde.read_f64(p) or_return
		case 3:
			f.photo_height = serde.read_f64(p) or_return
		case 4:
			f.source_width = serde.read_f64(p) or_return
		case 5:
			f.source_height = serde.read_f64(p) or_return
		case 6:
			f.working_width = serde.read_f64(p) or_return
		case 7:
			f.working_height = serde.read_f64(p) or_return
		case 8:
			f.raw = u8(serde.read_bool(p) or_return)
		case 9:
			r.display_space = serde.read_string(p) or_return
		case 10:
			f.lens_correction_supported = u8(serde.read_bool(p) or_return)
		}
		return true
	}
	fs: serde.Fields
	fs, ok = serde.read_struct(p, FRAME_FIELDS[:], &out, field)
	if ok {
		ok = serde.has_fields(p, fs, {0, 1, 2, 3, 4, 5, 6, 7, 8, 10}, {})
	}
	return
}

@(private = "file")
Header :: struct {
	key:      string,
	frame:    Read_Frame,
	checksum: u64,
}

// `serde_json::from_slice::<Header>`, strings in the parser's allocator.
@(private = "file")
parse_header :: proc(text: []u8, allocator: value.Allocator) -> (h: Header, ok: bool) {
	p := value.parser_make(text, allocator)
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		h := (^Header)(data)
		switch index {
		case 0:
			h.key = serde.read_string(p) or_return
		case 1:
			h.frame = read_frame(p) or_return
		case 2:
			h.checksum = serde.read_u64(p) or_return
		}
		return true
	}
	fs: serde.Fields
	fs, ok = serde.read_struct(&p, HEADER_FIELDS[:], &h, field)
	if ok {
		ok = serde.has_fields(&p, fs, {0, 1, 2}, {})
	}
	if ok {
		ok = value.finish(&p)
	}
	return
}

// The header line as serde_json writes it, newline included.
write_header :: proc(b: ^strings.Builder, key: string, f: Frame, display_space: string, sum: u64) {
	strings.write_byte(b, '{')
	serde.key(b, "key", true)
	serde.write_str(b, key)
	serde.key(b, "frame")
	strings.write_byte(b, '{')
	serde.key(b, "width", true)
	serde.write_u64(b, u64(f.width))
	serde.key(b, "height")
	serde.write_u64(b, u64(f.height))
	serde.key(b, "photo_width")
	serde.write_f64(b, f.photo_width)
	serde.key(b, "photo_height")
	serde.write_f64(b, f.photo_height)
	serde.key(b, "source_width")
	serde.write_f64(b, f.source_width)
	serde.key(b, "source_height")
	serde.write_f64(b, f.source_height)
	serde.key(b, "working_width")
	serde.write_f64(b, f.working_width)
	serde.key(b, "working_height")
	serde.write_f64(b, f.working_height)
	serde.key(b, "raw")
	serde.write_bool(b, f.raw != 0)
	serde.key(b, "display_space")
	serde.write_str(b, display_space)
	serde.key(b, "lens_correction_supported")
	serde.write_bool(b, f.lens_correction_supported != 0)
	strings.write_byte(b, '}')
	serde.key(b, "checksum")
	serde.write_u64(b, sum)
	strings.write_string(b, "}\n")
}

// MARK: Reading and writing

Loaded :: struct {
	frame:         Frame,
	// In the allocator given.
	display_space: string,
	// The whole file, in the allocator given; the pixels are its tail.
	bytes:         []u8,
	pixels_offset: int,
}

// `read`: the frame saved for `key`, checked as the Rust checks it.
read :: proc(file: string, key: string, allocator := context.allocator) -> (out: Loaded, err: Read_Error) {
	st, ok := files.stat(file)
	if !ok {
		return {}, .Io
	}
	if st.size > MAX_PIXELS + MAX_HEADER {
		return {}, .Size
	}
	bytes, ioerr := files.read(file, allocator)
	if ioerr != 0 {
		return {}, .Io
	}
	defer if err != .None {
		delete(bytes, allocator)
	}
	end := -1
	for i in 0 ..< min(len(bytes), MAX_HEADER) {
		if bytes[i] == '\n' {
			end = i
			break
		}
	}
	if end < 0 {
		return {}, .Header
	}
	h, parsed := parse_header(bytes[:end], context.temp_allocator)
	if !parsed {
		return {}, .Parse
	}
	f := h.frame.frame
	if h.key != key || f.width < 1 || f.width > 8192 || f.height < 1 || f.height > 8192 {
		return {}, .Mismatch
	}
	for v in ([]f64{f.photo_width, f.photo_height, f.source_width, f.source_height, f.working_width, f.working_height}) {
		if !(v - v == 0 && v >= 1.0) {
			return {}, .Photo_Size
		}
	}
	pixels := bytes[end + 1:]
	if len(pixels) != int(f.width) * int(f.height) * 4 || checksum(pixels) != h.checksum {
		return {}, .Pixels
	}
	return Loaded{f, strings.clone(h.frame.display_space, allocator), bytes, end + 1}, .None
}

// `write`: the header and the pixels into `{stem}.tmp`, moved over the
// file; the staging file removed whatever happened after the folder.
write :: proc(file: string, key: string, f: Frame, display_space: string, pixels: []u8) -> files.Error {
	b := strings.builder_make(0, 512, context.temp_allocator)
	write_header(&b, key, f, display_space, checksum(pixels))
	if folder, ok := files.parent(file); ok {
		if err := files.create_dir_all(folder); err != 0 {
			return err
		}
	}
	staging := files.with_extension(file, "tmp", context.temp_allocator)
	err := files.write(staging, b.buf[:], pixels)
	if err == 0 {
		err = files.replace(staging, file)
	}
	files.remove(staging)
	return err
}
