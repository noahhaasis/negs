// The journal as serde reads and writes it (`#[serde(rename_all =
// "camelCase")]` on every struct; Options `#[serde(default)]` as a whole;
// an Item's status, reason and written `#[serde(default)]`).
package batch

import "core:strings"
import "../../state"
import "../../value"
import "../serde"

OPTIONS_FIELDS := [?]string {
	"format",
	"engine",
	"edge",
	"pixels",
	"bitDepth",
	"colorSpace",
	"jpegQuality",
	"naming",
	"custom",
	"existing",
	"lowMemory",
	"lightroom",
}

ITEM_FIELDS := [?]string {
	"photo",
	"variant",
	"title",
	"state",
	"needsMatch",
	"storesMatch",
	"rating",
	"fileName",
	"size",
	"look",
	"status",
	"reason",
	"written",
}

JOURNAL_FIELDS := [?]string{"schema", "destination", "options", "items"}

// MARK: Reading

@(private = "file")
read_opt_string :: proc(p: ^value.Parser, out: ^Maybe(string)) -> bool {
	if is_null := serde.read_null(p) or_return; !is_null {
		out^ = serde.read_string(p) or_return
	}
	return true
}

read_options :: proc(p: ^value.Parser) -> (Options, bool) {
	out := options_clone(DEFAULT_OPTIONS, p.allocator)
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		o := (^Options)(data)
		text :: proc(p: ^value.Parser, s: ^string) -> bool {
			t := serde.read_string(p) or_return
			delete(s^, p.allocator)
			s^ = t
			return true
		}
		switch index {
		case 0:
			text(p, &o.format) or_return
		case 1:
			text(p, &o.engine) or_return
		case 2:
			text(p, &o.edge) or_return
		case 3:
			o.pixels = serde.read_u32(p) or_return
		case 4:
			o.bit_depth = serde.read_u32(p) or_return
		case 5:
			o.color_space = serde.read_u64(p) or_return
		case 6:
			o.jpeg_quality = serde.read_u32(p) or_return
		case 7:
			o.naming = Naming(serde.read_variant(p, NAMING_KEYS[:]) or_return)
		case 8:
			text(p, &o.custom) or_return
		case 9:
			o.existing = Existing(serde.read_variant(p, EXISTING_KEYS[:]) or_return)
		case 10:
			o.low_memory = serde.read_bool(p) or_return
		case 11:
			old := o.lightroom
			o.lightroom = nil
			read_opt_string(p, &o.lightroom) or_return
			if l, had := old.?; had {
				delete(l, p.allocator)
			}
		}
		return true
	}
	if _, ok := serde.read_struct(p, OPTIONS_FIELDS[:], &out, field); !ok {
		options_destroy(out, p.allocator)
		return {}, false
	}
	return out, true
}

read_item :: proc(p: ^value.Parser) -> (Item, bool) {
	out: Item
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		it := (^Item)(data)
		switch index {
		case 0:
			it.photo = serde.read_string(p) or_return
		case 1:
			read_opt_string(p, &it.variant) or_return
		case 2:
			it.title = serde.read_string(p) or_return
		case 3:
			it.state = state.read_state(p) or_return
		case 4:
			it.needs_match = serde.read_bool(p) or_return
		case 5:
			it.stores_match = serde.read_bool(p) or_return
		case 6:
			if is_null := serde.read_null(p) or_return; !is_null {
				it.rating = serde.read_i32(p) or_return
			}
		case 7:
			it.file_name = serde.read_string(p) or_return
		case 8:
			if is_null := serde.read_null(p) or_return; !is_null {
				it.size = serde.read_pair(p) or_return
			}
		case 9:
			read_opt_string(p, &it.look) or_return
		case 10:
			it.status = Status(serde.read_variant(p, STATUS_KEYS[:]) or_return)
		case 11:
			read_opt_string(p, &it.reason) or_return
		case 12:
			read_opt_string(p, &it.written) or_return
		}
		return true
	}
	f, ok := serde.read_struct(p, ITEM_FIELDS[:], &out, field)
	if ok {
		ok = serde.has_fields(p, f, {0, 2, 3, 4, 5, 7}, {1, 6, 8, 9})
	}
	if !ok {
		item_destroy(out, p.allocator)
		return {}, false
	}
	return out, true
}

// `Vec<Item>`: an array only.
@(private = "file")
read_items :: proc(p: ^value.Parser) -> ([]Item, bool) {
	c, any := value.peek_ws(p)
	if !any {
		return nil, value.fail(p, .Eof)
	}
	if c != '[' {
		return nil, value.fail(p, .Invalid_Type)
	}
	items := make([dynamic]Item, 0, 8, p.allocator)
	ok := false
	defer if !ok {
		for it in items {
			item_destroy(it, p.allocator)
		}
		delete(items)
	}
	value.enter(p) or_return
	p.i += 1
	first := true
	for {
		more := value.seq_next(p, &first) or_return
		if !more {
			break
		}
		it := read_item(p) or_return
		append(&items, it)
	}
	value.leave(p)
	value.seq_end(p) or_return
	ok = true
	return items[:], true
}

read_journal :: proc(p: ^value.Parser) -> (Journal, bool) {
	out: Journal
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		j := (^Journal)(data)
		switch index {
		case 0:
			j.schema = serde.read_u32(p) or_return
		case 1:
			j.destination = serde.read_string(p) or_return
		case 2:
			j.options = read_options(p) or_return
		case 3:
			j.items = read_items(p) or_return
		}
		return true
	}
	f, ok := serde.read_struct(p, JOURNAL_FIELDS[:], &out, field)
	if ok {
		ok = serde.has_fields(p, f, {0, 1, 2, 3}, {})
	}
	if !ok {
		// Fields never read are zero, which destroys to nothing.
		delete(out.destination, p.allocator)
		if 2 in f.seen {
			options_destroy(out.options, p.allocator)
		}
		for it in out.items {
			item_destroy(it, p.allocator)
		}
		delete(out.items, p.allocator)
		return {}, false
	}
	return out, true
}

// `serde_json::from_slice::<Journal>` (any schema), in `allocator`.
journal_parse :: proc(text: []u8, allocator := context.allocator) -> (Journal, value.Error) {
	p := value.parser_make(text, allocator)
	j, ok := read_journal(&p)
	if ok && value.finish(&p) {
		return j, .None
	}
	if ok {
		journal_destroy(j, allocator)
	}
	return {}, p.err if p.err != .None else .Invalid_Type
}

// MARK: Writing

write_options :: proc(b: ^strings.Builder, o: Options) {
	strings.write_byte(b, '{')
	serde.key(b, "format", true)
	serde.write_str(b, o.format)
	serde.key(b, "engine")
	serde.write_str(b, o.engine)
	serde.key(b, "edge")
	serde.write_str(b, o.edge)
	serde.key(b, "pixels")
	serde.write_u64(b, u64(o.pixels))
	serde.key(b, "bitDepth")
	serde.write_u64(b, u64(o.bit_depth))
	serde.key(b, "colorSpace")
	serde.write_u64(b, o.color_space)
	serde.key(b, "jpegQuality")
	serde.write_u64(b, u64(o.jpeg_quality))
	serde.key(b, "naming")
	serde.write_str(b, NAMING_KEYS[o.naming])
	serde.key(b, "custom")
	serde.write_str(b, o.custom)
	serde.key(b, "existing")
	serde.write_str(b, EXISTING_KEYS[o.existing])
	serde.key(b, "lowMemory")
	serde.write_bool(b, o.low_memory)
	serde.key(b, "lightroom")
	serde.write_opt_str(b, o.lightroom)
	strings.write_byte(b, '}')
}

write_item :: proc(b: ^strings.Builder, it: Item) {
	strings.write_byte(b, '{')
	serde.key(b, "photo", true)
	serde.write_str(b, it.photo)
	serde.key(b, "variant")
	serde.write_opt_str(b, it.variant)
	serde.key(b, "title")
	serde.write_str(b, it.title)
	serde.key(b, "state")
	state.write(b, it.state)
	serde.key(b, "needsMatch")
	serde.write_bool(b, it.needs_match)
	serde.key(b, "storesMatch")
	serde.write_bool(b, it.stores_match)
	serde.key(b, "rating")
	if r, ok := it.rating.?; ok {
		serde.write_i64(b, i64(r))
	} else {
		strings.write_string(b, "null")
	}
	serde.key(b, "fileName")
	serde.write_str(b, it.file_name)
	serde.key(b, "size")
	if s, ok := it.size.?; ok {
		strings.write_byte(b, '[')
		serde.write_f64(b, s[0])
		strings.write_byte(b, ',')
		serde.write_f64(b, s[1])
		strings.write_byte(b, ']')
	} else {
		strings.write_string(b, "null")
	}
	serde.key(b, "look")
	serde.write_opt_str(b, it.look)
	serde.key(b, "status")
	serde.write_str(b, STATUS_KEYS[it.status])
	serde.key(b, "reason")
	serde.write_opt_str(b, it.reason)
	serde.key(b, "written")
	serde.write_opt_str(b, it.written)
	strings.write_byte(b, '}')
}

// `serde_json::to_vec(&journal)`.
write_journal :: proc(b: ^strings.Builder, j: Journal) {
	strings.write_byte(b, '{')
	serde.key(b, "schema", true)
	serde.write_u64(b, u64(j.schema))
	serde.key(b, "destination")
	serde.write_str(b, j.destination)
	serde.key(b, "options")
	write_options(b, j.options)
	serde.key(b, "items")
	strings.write_byte(b, '[')
	for it, i in j.items {
		if i > 0 {
			strings.write_byte(b, ',')
		}
		write_item(b, it)
	}
	strings.write_byte(b, ']')
	strings.write_byte(b, '}')
}

journal_to_string :: proc(j: Journal, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 1024, allocator)
	write_journal(&b, j)
	return strings.to_string(b)
}

// `serde_json::to_vec(&options)`.
options_to_string :: proc(o: Options, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 256, allocator)
	write_options(&b, o)
	return strings.to_string(b)
}
