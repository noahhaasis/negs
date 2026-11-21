// The library package's own tests (core/src/library): JSON as Foundation
// reads and writes it, SHA-256, Finder's order, the filter's words, the
// sidecar fixtures, and the library on a temporary folder. The vectors are
// crates/negs-library's. Run by `python3 core/build.py test`.
package library_tests

import "core:fmt"
import "core:os"
import "core:strings"
import "core:testing"
import lib "../../src/library"

REPO :: #directory + "/../../.."

@(test)
numbers_as_swift_writes_them :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	cases := [?]struct {
		value: f64,
		text:  string,
	} {
		{1.0, "1"},
		{0.1, "0.1"},
		{-0.0, "-0"},
		{1e20, "1e+20"},
		{123456789012345678.0, "1.2345678901234568e+17"},
		{1e-7, "1e-07"},
		{1e-6, "1e-06"},
		{5e-324, "5e-324"},
		{max(f64), "1.7976931348623157e+308"},
		{1e15, "1000000000000000"},
		{1e16, "1e+16"},
		{9007199254740992.0, "9007199254740992"},
		{9007199254740994.0, "9.007199254740994e+15"},
		{1000000000000000.5, "1000000000000000.5"},
		{0.0001, "0.0001"},
		{9.9999e-5, "9.9999e-05"},
		{2.5e-5, "2.5e-05"},
		{-40.0, "-40"},
		{0.30000000000000004, "0.30000000000000004"},
		{12345678.9, "12345678.9"},
		// Halfway between two shortest forms: Swift takes the even one.
		{1072269968822823.25, "1072269968822823.2"},
	}
	for c in cases {
		testing.expect_value(t, lib.json_number_text(c.value, context.temp_allocator), c.text)
	}
	testing.expect_value(t, lib.swift_double_description(1731859200.0, context.temp_allocator), "1731859200.0")
	testing.expect_value(t, lib.swift_double_description(1731859200.25, context.temp_allocator), "1731859200.25")
}

@(private)
parse :: proc(text: string) -> (lib.Json, bool) {
	v, err := lib.json_parse(transmute([]u8)text, context.temp_allocator)
	return v, err == nil
}

@(test)
writes_as_json_encoder :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	value, ok := parse(`{"b": [], "a": {"z": {}, "y": [1, "x/y"]}, "c": "q\"\\\n\u0001\u007f"}`)
	testing.expect(t, ok)
	expected := "{\n  \"a\" : {\n    \"y\" : [\n      1,\n      \"x/y\"\n    ],\n    \"z\" : {\n\n    }\n  },\n  \"b\" : [\n\n  ],\n  \"c\" : \"q\\\"\\\\\\n\\u0001\x7f\"\n}\n"
	testing.expect_value(t, string(lib.to_sidecar_bytes(value, context.temp_allocator)), expected)
	testing.expect_value(t, lib.to_compact_sorted(value, context.temp_allocator), "{\"a\":{\"y\":[1,\"x\\/y\"],\"z\":{}},\"b\":[],\"c\":\"q\\\"\\\\\\n\\u0001\x7f\"}")
}

@(test)
reads_as_json_decoder :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	bom, bom_ok := parse("\xef\xbb\xbf{\"a\":1}")
	testing.expect(t, bom_ok)
	testing.expect(t, lib.json_equal(lib.json_get(bom, "a"), f64(1)))
	twice, _ := parse(`{"a":1,"a":2}`)
	testing.expect(t, lib.json_equal(lib.json_get(twice, "a"), f64(1)), "the first wins")
	trailing, trailing_ok := parse("[1,]")
	testing.expect(t, trailing_ok)
	testing.expect_value(t, len(trailing.(lib.Json_Array)), 1)
	object, object_ok := parse(`{"a":1,}`)
	testing.expect(t, object_ok && lib.json_equal(lib.json_get(object, "a"), f64(1)))
	emoji, _ := parse(`"😀\/"`)
	testing.expect_value(t, emoji.(string), "😀/")
	for bad in ([]string{`{"a":1e400}`, `{"a":01}`, `{"a":"\ud800"}`, `{"a":1} x`, "\"a\tb\"", "1.", "-", "\"\xff\"", "1e-400", ""}) {
		_, good := parse(bad)
		testing.expectf(t, !good, "%q should be refused", bad)
	}
}

@(test)
overlay_changes_and_unknown :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	base, _ := parse(`{"a": {"x": 0.10000000149011612, "y": [1, 2, 3], "z": null}, "b": true, "c": "auto"}`)
	changes_in, _ := parse(`{"a": {"x": 0.1}, "d": 1}`)
	edited := lib.json_overlaid(base, changes_in, ta)
	changes, changed := lib.json_changes_from(edited, base, ta)
	testing.expect(t, changed && lib.json_equal(changes, changes_in))
	testing.expect(t, lib.json_equal(lib.json_overlaid(base, changes, ta), edited))
	_, same := lib.json_changes_from(base, base, ta)
	testing.expect(t, !same)
	stored, _ := parse(`{"a": {"x": 1, "new": 2}, "future": {"k": 1}}`)
	unknown, has := lib.json_unknown_to(stored, {base}, ta)
	want, _ := parse(`{"a": {"new": 2}, "future": {"k": 1}}`)
	testing.expect(t, has && lib.json_equal(unknown, want))
}

@(private)
hex :: proc(bytes: []u8) -> string {
	b := strings.builder_make(context.temp_allocator)
	for x in bytes {
		fmt.sbprintf(&b, "%02x", x)
	}
	return strings.to_string(b)
}

@(test)
sha256_known_vectors :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	empty := lib.sha256(nil)
	testing.expect_value(t, hex(empty[:]), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
	abc := lib.sha256(transmute([]u8)string("abc"))
	testing.expect_value(t, hex(abc[:]), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
	two := lib.sha256(transmute([]u8)string("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))
	testing.expect_value(t, hex(two[:]), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1")
	million := make([]u8, 1_000_000, context.temp_allocator)
	for &c in million {
		c = 'a'
	}
	m := lib.sha256(million)
	testing.expect_value(t, hex(m[:]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
}

@(test)
finder_order :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	// As `localizedStandardCompare` sorted them on macOS 26 (folder.rs).
	sorted := "_a.jpg|0.jpg|00.jpg|000.jpg|1.jpg|9.jpg|10.jpg|a 1.png|a_1.png|a-1.png|a.b.jpg|a.jpg|ä.jpg|Ä.jpg|a.png|a~.jpg|a1.png|a01.png|a001.png|A1.png|a1b2.jpg|a1b10.jpg|a2.png|a10.png|ab.jpg|b.jpg|B.JPG|DSCF5.JPG|DSCF5.NEF|DSCF6.dng|DSCF6.png|e.jpg|é.jpg|f.jpg|IMG_0412.CR3|IMG_0412.JPG|IMG_0413 (2).jpg|img_0413.jpg|IMG_9999.NEF|IMG_10000.NEF|IMG-0413.jpg|photo 2.jpg|photo 10.jpg|photo2.jpg|x,1.jpg|x!1.jpg|x(1).jpg|x[1].jpg|x@1.jpg|x#1.jpg|x+1.jpg|x0.jpg|xa.jpg|z.jpg|Z.jpg"
	names := strings.split(sorted, "|", context.temp_allocator)
	for i in 0 ..< len(names) {
		for j in 0 ..< len(names) {
			c := lib.standard_compare(names[i], names[j])
			want := -1 if i < j else (1 if i > j else 0)
			testing.expectf(t, c == want, "%s vs %s: %d", names[i], names[j], c)
		}
	}
}

@(test)
filter_words_as_the_sheet_says_them :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	f := lib.Filter{rating = {.At_Least, 3}, flags = {.Reject, .Pick}, edits = .Edited}
	words := lib.filter_words(f, ta)
	testing.expect_value(t, len(words), 3)
	testing.expect_value(t, words[0], lib.Filter_Word{"rated ", "★★★ or more"})
	testing.expect_value(t, words[1], lib.Filter_Word{"", "picks or rejected"})
	testing.expect_value(t, words[2], lib.Filter_Word{"", "edited"})
	testing.expect_value(t, lib.filter_words(lib.Filter{rating = {.At_Least, 5}}, ta)[0].word, "★★★★★")
	testing.expect_value(t, lib.count_words(4, 12, 0, true, ta), "4 of 12 photographs")
	testing.expect_value(t, lib.count_words(12, 12, 1, false, ta), "12 photographs · 1 selected")
	testing.expect_value(t, lib.count_words(1, 1, 0, false, ta), "1 photograph")
	titles := [7]string{"Any Rating", "≥ ★", "≥ ★★", "≥ ★★★", "≥ ★★★★", "★★★★★", "Unrated Only"}
	for c, i in lib.rating_choices(ta) {
		testing.expect_value(t, c.title, titles[i])
	}
	mac := lib.Filter{rating = {.At_Least, 2}, flags = {.Reject}, edits = .Unedited}
	testing.expect_value(t, lib.to_compact_sorted(lib.filter_to_json(mac, ta), ta), `{"edits":"unedited","flags":["reject"],"rating":{"atLeast":{"_0":2}}}`)
	testing.expect(t, lib.filter_equal(lib.filter_from_json(lib.filter_to_json(mac, ta)), mac))
	old, _ := parse(`{"rating":{"any":{}},"flags":["pick"],"editedOnly":true}`)
	testing.expect(t, lib.filter_equal(lib.filter_from_json(old), lib.Filter{flags = {.Pick}, edits = .Edited}))
}

FIXTURES :: [7]string{"edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"}

@(private)
read :: proc(path: string) -> []u8 {
	bytes, err := lib.fs_read(path, max(int), context.temp_allocator)
	if err != 0 {
		return nil
	}
	return bytes
}

// Every fixture reads, and writes back as the Mac writes it
// (tests/fixtures.rs `every_fixture_reads_and_rewrites_byte_for_byte`).
@(test)
every_fixture_reads_and_rewrites_byte_for_byte :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	for name in FIXTURES {
		bytes := read(strings.concatenate({REPO, "/docs/sidecar-fixtures/", name}, ta))
		mac := read(strings.concatenate({REPO, "/crates/negs-library/tests/mac-encoded/", name}, ta))
		testing.expectf(t, bytes != nil && mac != nil, "%s: fixture missing", name)
		value, err := lib.json_parse(bytes, ta)
		testing.expectf(t, err == nil, "%s: %v", name, err)
		testing.expectf(t, string(lib.to_sidecar_bytes(value, ta)) == string(mac), "%s: not the Mac's bytes", name)
		mac_text, _ := strings.replace_all(string(mac), "\" : ", "\": ", ta)
		testing.expectf(t, mac_text == string(bytes), "%s: only the colon's spacing differs", name)
		again, _ := lib.json_parse(mac, ta)
		testing.expectf(t, string(lib.to_sidecar_bytes(again, ta)) == string(mac), "%s: the Mac's bytes are a fixed point", name)
	}
	_, problem := lib.sidecar_from_bytes(read(REPO + "/docs/sidecar-fixtures/newer-schema.negs"))
	p, bad := problem.?
	testing.expect(t, bad && p.kind == .Newer)
	_, defaults_problem := lib.sidecar_from_bytes(read(REPO + "/docs/sidecar-fixtures/newer-defaults.negs"))
	q, newer_defaults := defaults_problem.?
	testing.expect(t, newer_defaults && q.kind == .Newer)
}

// MARK: The library on a temporary folder

@(private)
Test_Bin :: struct {
	folder: string,
	count:  int,
}

@(private)
test_bin :: proc(b: ^Test_Bin) -> lib.Bin {
	return lib.Bin {
		user = b,
		available = proc(user: rawptr, folder: string) -> bool {return true},
		trash = proc(user: rawptr, file: string) -> (string, string, bool) {
			b := (^Test_Bin)(user)
			b.count += 1
			os.make_directory(b.folder)
			landed := fmt.aprintf("%s/%d-%s", b.folder, b.count, lib.file_name(file), allocator = context.temp_allocator)
			if err := lib.fs_rename(file, landed); err != 0 {
				return "", lib.io_error_text(err, context.temp_allocator), false
			}
			return landed, "", true
		},
		put_back = proc(user: rawptr, trashed, original: string) -> (string, bool) {
			if err := lib.fs_rename(trashed, original); err != 0 {
				return lib.io_error_text(err, context.temp_allocator), false
			}
			return "", true
		},
	}
}

@(private)
scratch :: proc(tag: string) -> string {
	base := os.get_env("TMPDIR", context.temp_allocator)
	if base == "" {
		base = "/tmp"
	}
	dir := fmt.aprintf("%s/negs-core-library-%s-%d", strings.trim_right(base, "/"), tag, os.get_pid(), allocator = context.temp_allocator)
	os.remove_all(dir)
	os.make_directory_all(dir)
	return dir
}

@(test)
rating_and_undo_leave_the_files_as_they_were :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	ta := context.temp_allocator
	dir := scratch("rate")
	defer os.remove_all(dir)
	photos := strings.concatenate({dir, "/photos"}, ta)
	os.make_directory(photos)
	for name in ([]string{"b.jpg", "a10.png", "a2.png", "notes.txt"}) {
		lib.fs_write(strings.concatenate({photos, "/", name}, ta), transmute([]u8)string("x"))
	}
	bin := Test_Bin{folder = strings.concatenate({dir, "/bin"}, ta)}
	l, err := lib.library_new(photos, lib.Config{data = REPO + "/data", app = "test", bin = test_bin(&bin), pairing = true, sidecars = true, has_id_seed = true, id_seed = 7})
	testing.expectf(t, l != nil, "library_new: %s", err)
	if l == nil {
		return
	}
	defer lib.library_free(l)
	items := lib.library_items(l)
	testing.expect_value(t, len(items), 3)
	testing.expect_value(t, lib.file_name(items[0].photo), "a2.png")
	testing.expect_value(t, lib.file_name(items[2].photo), "b.jpg")
	outcome := lib.library_set_rating(l, items[:2], 4)
	testing.expect_value(t, outcome.note.text, "★★★★")
	testing.expect_value(t, lib.library_summary(l, items[0]).rating, 4)
	sidecar := read(lib.sidecar_path(items[0].photo, ta))
	testing.expect(t, sidecar != nil && strings.contains(string(sidecar), "\"rating\" : 4"))
	name, _ := lib.library_undo_name(l)
	testing.expect_value(t, name, "Rating")
	_, undone := lib.library_undo(l)
	testing.expect(t, undone)
	testing.expect(t, read(lib.sidecar_path(items[0].photo, ta)) == nil, "undone at once, the sidecar is gone again")
	testing.expect_value(t, lib.library_summary(l, items[0]).rating, 0)
	_, redone := lib.library_redo(l)
	testing.expect(t, redone && lib.library_summary(l, items[1]).rating == 4)
	// Duplicate, delete, and undo of both.
	dup := lib.library_duplicate(l, items[2:3])
	testing.expect_value(t, len(dup.select), 1)
	testing.expect_value(t, len(lib.library_items(l)), 4)
	plan := lib.library_delete_plan(l, lib.library_items(l)[2:4])
	testing.expect_value(t, len(plan.files), 1)
	gone := lib.library_delete(l, plan)
	testing.expect(t, !gone.has_error)
	testing.expect_value(t, len(lib.library_items(l)), 2)
	_, back := lib.library_undo(l)
	testing.expect(t, back)
	testing.expect_value(t, len(lib.library_items(l)), 4)
	opened := lib.library_open(l, items[0])
	testing.expect_value(t, opened.state.film, l.defaults.new_photo_film)
	state := lib.edit_state_clone(opened.state, ta)
	lib.json_set_at(&state.params, "prep.exposure_ev", f64(0.5), ta)
	lib.library_edit(l, state)
	testing.expect(t, lib.library_unsaved(l))
	_, saved := lib.library_save(l)
	testing.expect(t, saved && !lib.library_unsaved(l) && lib.library_summary(l, items[0]).edited)
	_, has_look := lib.library_look(l, items[0])
	testing.expect(t, has_look)
}
