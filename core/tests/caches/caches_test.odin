// Phase 4c's own tests (core/src/caches): std::path's rules on both
// platforms, the batch's names and words, the journal, the match cache and
// the rendered preview's file. core/diff/tests/{batch,match_cache,
// rendered_preview}.rs hold them to the frozen Rust. Run by `build.py test`.
package caches_tests

import "core:fmt"
import "core:os"
import "core:strings"
import "core:testing"
import "../../src/caches/batch"
import files "../../src/caches/files"
import mc "../../src/caches/match_cache"
import "../../src/caches/preview"
import "../../src/state"
import "../../src/value"

@(test)
unix_paths_as_std :: proc(t: ^testing.T) {
	U :: files.Style.Unix
	name :: proc(p: string) -> string {
		n, ok := files.file_name(p, .Unix)
		return n if ok else "<none>"
	}
	parent :: proc(p: string) -> string {
		n, ok := files.parent(p, .Unix)
		return n if ok else "<none>"
	}
	testing.expect_value(t, name("a/b.json/"), "b.json")
	testing.expect_value(t, name("a/."), "a")
	testing.expect_value(t, name(".."), "<none>")
	testing.expect_value(t, name("/"), "<none>")
	testing.expect_value(t, name("./"), "<none>")
	testing.expect_value(t, name(""), "<none>")
	testing.expect_value(t, name("C:\\p\\IMG.CR3"), "C:\\p\\IMG.CR3")
	testing.expect_value(t, parent("a/b"), "a")
	testing.expect_value(t, parent("a/b//"), "a")
	testing.expect_value(t, parent("a"), "")
	testing.expect_value(t, parent("/a"), "/")
	testing.expect_value(t, parent("/"), "<none>")
	testing.expect_value(t, parent("./a"), ".")
	testing.expect_value(t, parent("."), "")
	stem, _ := files.file_stem(".hidden", U)
	_, has_ext := files.extension(".hidden", U)
	testing.expect_value(t, stem, ".hidden")
	testing.expect(t, !has_ext)
	ext, has := files.extension("a.", U)
	testing.expect(t, has && ext == "")
	stem, _ = files.file_stem("..a", U)
	testing.expect_value(t, stem, ".")
	w := files.with_extension("c/rendered-preview.cache", "tmp", context.allocator, U)
	defer delete(w)
	testing.expect_value(t, w, "c/rendered-preview.tmp")
	w2 := files.with_extension("s/active-export.json", "json.tmp", context.allocator, U)
	defer delete(w2)
	testing.expect_value(t, w2, "s/active-export.json.tmp")
	j := files.join("d/", "x", context.allocator, U)
	defer delete(j)
	testing.expect_value(t, j, "d/x")
}

@(test)
windows_paths_as_std :: proc(t: ^testing.T) {
	W :: files.Style.Windows
	name :: proc(p: string) -> string {
		n, ok := files.file_name(p, .Windows)
		return n if ok else "<none>"
	}
	parent :: proc(p: string) -> string {
		n, ok := files.parent(p, .Windows)
		return n if ok else "<none>"
	}
	testing.expect_value(t, name("C:\\p\\IMG.CR3"), "IMG.CR3")
	testing.expect_value(t, name("C:/a/b/"), "b")
	testing.expect_value(t, name("C:"), "<none>")
	testing.expect_value(t, name("\\\\server\\share\\a.txt"), "a.txt")
	testing.expect_value(t, name("\\\\server\\share"), "<none>")
	testing.expect_value(t, name("\\\\?\\C:\\x\\y.z"), "y.z")
	testing.expect_value(t, name("\\\\?\\C:\\x/y"), "x/y")
	testing.expect_value(t, parent("C:\\a"), "C:\\")
	testing.expect_value(t, parent("C:\\"), "<none>")
	testing.expect_value(t, parent("C:a"), "C:")
	testing.expect_value(t, parent("\\\\server\\share\\a.txt"), "\\\\server\\share\\")
	kind, n := files.parse_prefix("\\\\?\\UNC\\srv\\sh\\x")
	testing.expect_value(t, kind, files.Prefix_Kind.Verbatim_UNC)
	testing.expect_value(t, n, 14)
	kind, n = files.parse_prefix("//./COM1")
	testing.expect_value(t, kind, files.Prefix_Kind.Device_NS)
	testing.expect_value(t, n, 8)
	stem, _ := files.file_stem("C:\\p\\IMG.CR3", W)
	testing.expect_value(t, stem, "IMG")
	for c in ([][3]string{{"C:", "a", "C:a"}, {"C:\\d", "a", "C:\\d\\a"}, {"C:\\d\\", "a", "C:\\d\\a"}, {"C:/d/", "*", "C:/d/*"}}) {
		j := files.join(c[0], c[1], context.allocator, W)
		defer delete(j)
		testing.expect_value(t, j, c[2])
	}
	b := batch.batch_name("C:\\p\\IMG.CR3", "IMG.CR3 (2)", context.allocator, W)
	defer delete(b)
	testing.expect_value(t, b, "IMG (2)")
}

@(test)
lossy_text_as_from_utf8_lossy :: proc(t: ^testing.T) {
	d := files.display("a\xe2\x82Ab\xffc\xed\xa0\x80\xf0\x9f\x98\x80", context.allocator)
	defer delete(d)
	testing.expect_value(t, d, "a\uFFFDAb\uFFFDc\uFFFD\uFFFD\uFFFD😀")
}

@(test)
names_and_words :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	check :: proc(t: ^testing.T, naming: batch.Naming, custom, name, film: string, format: batch.Output_Format, want: string) {
		got := batch.file_name(naming, custom, name, film, format)
		defer delete(got)
		testing.expect_value(t, got, want)
	}
	check(t, .Name_Film, "", "IMG_0412 (2)", "kodak_gold_200", .Jpeg, "IMG_0412 (2)_kodak_gold_200.jpg")
	check(t, .Name, "", "IMG_0412", "x", .Tiff, "IMG_0412.tif")
	check(t, .Custom, "trip/{film}-{name}", "A", "f", .Tiff, "trip-f-A.tif")
	check(t, .Custom, "a:b\\c*{name}?", "A", "f", .Tiff, "a-b-c-A-.tif")
	check(t, .Custom, ".{film}", "A", "f", .Tiff, "A.f.tif")
	check(t, .Custom, "", "A", "f", .Tiff, "A.tif")
	names := []string{"a.jpg", "b.jpg", "A.jpg", "a.jpg"}
	u := batch.unique(names)
	defer {
		for s in u {
			delete(s)
		}
		delete(u)
	}
	testing.expect_value(t, fmt.tprint(u), fmt.tprint([]string{"a.jpg", "b.jpg", "A 2.jpg", "a 3.jpg"}))
	b := batch.batch_name("/p/IMG.CR3", "IMG.CR3 (2)", context.allocator, .Unix)
	defer delete(b)
	testing.expect_value(t, b, "IMG (2)")
	testing.expect_value(t, batch.naming_from_key("custom"), batch.Naming.Custom)
	testing.expect_value(t, batch.existing_from_key("?"), batch.Existing.Number)

	statuses := []batch.Status{.Done, .Failed, .Skipped, .Waiting}
	mixed := batch.run_of(statuses, false, false)
	e, o := batch.rule_parts(mixed)
	testing.expect(t, abs(e - 0.25) < 1e-9 && abs(o - 0.5) < 1e-9)
	words := batch.finished_words(mixed, "/out/Trip")
	defer delete(words)
	testing.expect_value(t, words, "Exported 1 photograph to “Trip”; 1 failed; 1 skipped")
	running := batch.run_of({.Done, .Waiting, .Waiting}, true, false)
	title := batch.title(running)
	defer delete(title)
	testing.expect_value(t, title, "Exporting 2 of 3")
	p := batch.exporting(0, 2, "Lightroom")
	defer delete(p)
	testing.expect_value(t, p, "Sending 1 of 2 photographs to Lightroom")
	m := batch.message(3, .Fast, .Long, 3000, {16, 1, 95}, "sRGB")
	defer delete(m)
	testing.expect_value(
		t,
		m,
		"Each of the 3 photographs with its own edits, crop and border, one after another. Fast looks the same as Reference, in a fraction of the time. Fits 3000 px on the long edge. Develops in Display P3 and embeds its profile; the preview then shows Display P3.",
	)
}

@(test)
matched_moves_the_total_with_the_match :: proc(t: ^testing.T) {
	total, set := batch.matched_exposure(u64(1), 0.5, 1.0)
	testing.expect(t, set)
	testing.expect_value(t, value.number(total), 1.5)
	_, set = batch.matched_exposure(u64(1), 0.5, 0.52)
	testing.expect(t, !set)
	total, _ = batch.matched_exposure(f64(0.5), nil, 9)
	testing.expect_value(t, value.number(total), 3.0)
}

@(private)
scratch :: proc(name: string) -> string {
	dir := fmt.aprintf("%s/negs-caches-test-%s-%d", os.get_env("TMPDIR", context.temp_allocator), name, os.get_pid())
	return dir
}

@(test)
journal_round_trips :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	dir := scratch("journal")
	defer delete(dir)
	path := strings.concatenate({dir, "/j/active-export.json"})
	defer delete(path)
	items := make([]batch.Item, 1)
	items[0] = batch.Item {
		photo     = strings.clone("/p/IMG.CR3"),
		variant   = strings.clone("v2"),
		title     = strings.clone("IMG.CR3 (2)"),
		state     = state.default_state(),
		rating    = -1,
		file_name = strings.clone("IMG (2)_f.jpg"),
		size      = [2]f64{6000, 4000},
	}
	j := batch.journal_new(strings.clone(dir), batch.options_clone(batch.DEFAULT_OPTIONS), items)
	defer batch.journal_destroy(j)
	testing.expect_value(t, batch.journal_write(j, path), files.Error(0))
	back, ok := batch.journal_read(path)
	defer batch.journal_destroy(back)
	testing.expect(t, ok)
	a := batch.journal_to_string(j)
	defer delete(a)
	c := batch.journal_to_string(back)
	defer delete(c)
	testing.expect_value(t, c, a)
	testing.expect(t, strings.contains(a, `"size":[6000.0,4000.0]`))
	testing.expect_value(t, batch.remaining(back), 1)
	batch.journal_remove(path)
	_, again := batch.journal_read(path)
	testing.expect(t, !again)
	// A mutated text is refused without leaks.
	bad := strings.concatenate({a[:len(a) - 3], "x]}"}, context.temp_allocator)
	_, err := batch.journal_parse(transmute([]u8)bad)
	testing.expect(t, err != .None)
	os.remove(strings.concatenate({dir, "/j"}, context.temp_allocator))
	os.remove(dir)
}

@(test)
match_cache_lru_and_file :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	dir := scratch("match")
	defer delete(dir)
	file := strings.concatenate({dir, "/BrightnessMatches.json"})
	defer delete(file)
	c := mc.cache_make(file, 10)
	defer mc.cache_destroy(&c)
	for i in 0 ..< 10 {
		k := fmt.tprintf("k%d", i)
		mc.store(&c, k, f64(i) / 10, i % 2 == 0, f64(i))
	}
	ev, limited, found := mc.solved(&c, "k0", 10)
	testing.expect(t, found && ev == 0 && limited)
	mc.store(&c, "k10", 1, false, 11)
	testing.expect_value(t, mc.count(&c), 9)
	_, _, f1 := mc.solved(&c, "k1", 12)
	_, _, f2 := mc.solved(&c, "k2", 12)
	testing.expect(t, !f1 && !f2)
	bytes, write := mc.flush_bytes(&c)
	defer delete(bytes)
	testing.expect(t, write)
	mc.write_file(file, bytes)
	again := mc.cache_make(file, 10)
	defer mc.cache_destroy(&again)
	testing.expect_value(t, mc.count(&again), 9)
	ev, _, found = mc.solved(&again, "k10", 20)
	testing.expect(t, found && ev == 1)
	os.remove(file)
	os.remove(dir)
}

@(test)
preview_frame_round_trips :: proc(t: ^testing.T) {
	defer free_all(context.temp_allocator)
	dir := scratch("preview")
	defer delete(dir)
	file := strings.concatenate({dir, "/frame.cache"})
	defer delete(file)
	pixels := []u8{1, 2, 3, 255, 30, 40, 50, 255}
	f := preview.Frame{2, 1, 4000, 2000, 2000, 1000, 8000, 4000, 1, 1}
	testing.expect_value(t, preview.write(file, "settings", f, "Rec. 2020", pixels), files.Error(0))
	got, err := preview.read(file, "settings")
	testing.expect_value(t, err, preview.Read_Error.None)
	testing.expect_value(t, got.frame, f)
	testing.expect_value(t, got.display_space, "Rec. 2020")
	testing.expect_value(t, fmt.tprint(got.bytes[got.pixels_offset:]), fmt.tprint(pixels))
	delete(got.bytes)
	delete(got.display_space)
	_, err = preview.read(file, "other")
	testing.expect_value(t, err, preview.Read_Error.Mismatch)
	os.remove(file)
	os.remove(dir)
	testing.expect(t, preview.checksum(nil) != preview.checksum([]u8{0}))
}
