// An unfinished batch, so the next launch can resume it
// (`active-export.json` in the settings folder): written after each
// photograph through a staging file, removed when the batch ends.
package batch

import "../../state"
import "../../value"
import files "../files"

JOURNAL_SCHEMA :: 1

// One photograph of a batch: its edits as they were when Export was
// chosen, and how it went.
Item :: struct {
	photo:        string,
	// A duplicate's variant.
	variant:      Maybe(string),
	// As the strip names it: `IMG_0412.CR3 (2)`.
	title:        string,
	state:        state.Edit_State,
	needs_match:  bool,
	stores_match: bool,
	// `xmp:Rating`: 1–5, or −1 rejected; none writes none.
	rating:       Maybe(i32),
	// The name it gets, before If a File Exists.
	file_name:    string,
	// The photograph's size before its crop, when known.
	size:         Maybe([2]f64),
	look:         Maybe(string),
	status:       Status,
	reason:       Maybe(string),
	// Where it was written.
	written:      Maybe(string),
}

Journal :: struct {
	schema:      u32,
	destination: string,
	options:     Options,
	items:       []Item,
}

@(private = "file")
delete_maybe :: proc(s: Maybe(string), allocator: Allocator) {
	if text, ok := s.?; ok {
		delete(text, allocator)
	}
}

item_destroy :: proc(it: Item, allocator := context.allocator) {
	delete(it.photo, allocator)
	delete_maybe(it.variant, allocator)
	delete(it.title, allocator)
	state.destroy(it.state, allocator)
	delete(it.file_name, allocator)
	delete_maybe(it.look, allocator)
	delete_maybe(it.reason, allocator)
	delete_maybe(it.written, allocator)
}

journal_destroy :: proc(j: Journal, allocator := context.allocator) {
	delete(j.destination, allocator)
	options_destroy(j.options, allocator)
	for it in j.items {
		item_destroy(it, allocator)
	}
	delete(j.items, allocator)
}

// `Journal::remaining`: photographs still to export.
remaining :: proc "contextless" (j: Journal) -> u64 {
	n: u64 = 0
	for it in j.items {
		if it.status == .Waiting {
			n += 1
		}
	}
	return n
}

// `Journal::write`: the folder made, `serde_json::to_vec` into
// `{stem}.json.tmp`, then that moved over `path` (windows/src/platform.rs's
// replace_file). A failed step leaves the staging file, as in Rust.
journal_write :: proc(j: Journal, path: string) -> files.Error {
	text := journal_to_string(j, context.temp_allocator)
	return write_staged(path, "json.tmp", transmute([]u8)text)
}

// The steps of `Journal::write` for bytes already made.
write_staged :: proc(path: string, staging_ext: string, bytes: []u8) -> files.Error {
	if folder, ok := files.parent(path); ok {
		if err := files.create_dir_all(folder); err != 0 {
			return err
		}
	}
	staging := files.with_extension(path, staging_ext, context.temp_allocator)
	if err := files.write(staging, bytes); err != 0 {
		return err
	}
	return files.replace(staging, path)
}

// `Journal::read`: the file's journal if it reads as one of this schema.
journal_read :: proc(path: string, allocator := context.allocator) -> (Journal, bool) {
	bytes, err := files.read(path, context.temp_allocator)
	if err != 0 {
		return {}, false
	}
	j, perr := journal_parse(bytes, allocator)
	if perr != .None {
		return {}, false
	}
	if j.schema != JOURNAL_SCHEMA {
		journal_destroy(j, allocator)
		return {}, false
	}
	return j, true
}

// `Journal::remove`: errors ignored.
journal_remove :: proc(path: string) {
	files.remove(path)
}

// `Journal::new`: this schema. Takes the strings and items as they are.
journal_new :: proc(destination: string, options: Options, items: []Item) -> Journal {
	return Journal{JOURNAL_SCHEMA, destination, options, items}
}

@(private)
Allocator :: value.Allocator
