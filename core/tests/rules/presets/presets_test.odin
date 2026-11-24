#+build darwin, linux
// core/src/settings/presets: presets.rs's name rules and its tests of the
// folder (in a scratch folder). std::path's joins are tested with the rest
// of the file layer, in core/tests/platform/fs.
package presets_tests

import "core:strings"
import "core:sys/posix"
import "core:testing"
import fs "../../../src/platform/fs"
import "../../../src/settings/presets"

@(test)
names_every_platform_accepts :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	for n in ([]string{"Portra 400 warm", "Kodachrome – 1970s", "Trix (push 2)", "Ünïcødé", "v1.2", "COM0", "LPT10", "Console", "NULL"}) {
		_, bad := presets.problem(n)
		testing.expectf(t, !bad, "%q", n)
	}
	long := strings.repeat("x", 121)
	for n in ([]string{"", "   ", "a/b", "a\\b", "a:b", "what?", "a*", "\"quoted\"", "<x>", "a|b", ".hidden", "trailing.", "trailing ", " leading", "tab\there", "CON", "con", "Lpt1", "NUL.json", "COM9.backup", "aux.tar.gz", "prn .txt", long}) {
		_, bad := presets.problem(n)
		testing.expectf(t, bad, "%q", n)
	}
	_, bad := presets.problem(strings.repeat("x", 120))
	testing.expect(t, !bad)
	message, _ := presets.problem("nul")
	testing.expect_value(t, message, "“nul” is reserved on Windows.")
}

@(test)
names_compare_without_case :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	testing.expect_value(t, presets.key("PORTRA İ"), "portra i\u0307")
	taken := make(map[string]bool)
	taken[presets.key("Portra")] = true
	taken[presets.key("portra 2")] = true
	testing.expect_value(t, presets.unique("PORTRA", taken), "PORTRA 3")
	testing.expect_value(t, presets.unique("Gold", taken), "Gold")
}

@(test)
file_names_become_preset_names :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	testing.expect_value(t, presets.from_file_name("05 P400 natural"), "05 P400 natural")
	testing.expect_value(t, presets.from_file_name("a:b|c"), "a b c")
	testing.expect_value(t, presets.from_file_name("..."), "Preset")
	testing.expect_value(t, presets.from_file_name("CON"), "Preset")
	testing.expect_value(t, presets.from_file_name("lpt3.backup"), "Preset")
	testing.expect_value(t, presets.from_file_name("a    b"), "a  b")
}

@(test)
text_skips_a_byte_order_mark_and_refuses_binary :: proc(t: ^testing.T) {
	testing.expect_value(t, presets.text_start(transmute([]u8)string("\xEF\xBB\xBF{}")), 3)
	testing.expect_value(t, presets.text_start(transmute([]u8)string("{}")), 0)
	testing.expect_value(t, presets.text_start(transmute([]u8)string("\xFF\xFE{")), -1)
	testing.expect_value(t, presets.text_start(transmute([]u8)string("\xED\xA0\x80")), -1)
}

@(private)
scratch :: proc(tag: string) -> string {
	b := strings.builder_make()
	strings.write_string(&b, "negs-core-presets-")
	strings.write_string(&b, tag)
	strings.write_byte(&b, '-')
	strings.write_u64(&b, u64(fs.process_id()))
	return fs.join(fs.temp_dir(), strings.to_string(b), .Unix)
}

@(private)
remove_folder :: proc(folder: string) {
	entries, _, _ := fs.read_dir(folder)
	for e in entries {
		_ = fs.remove_file(fs.join(folder, e.name, .Unix))
	}
	posix.rmdir(strings.clone_to_cstring(folder))
}

@(private)
names_of :: proc(list: []presets.Preset_File) -> []string {
	out := make([]string, len(list))
	for f, i in list {
		out[i] = presets.file_name_of(f)
	}
	return out
}

@(private)
same :: proc(a, b: []string) -> bool {
	if len(a) != len(b) {
		return false
	}
	for s, i in a {
		if s != b[i] {
			return false
		}
	}
	return true
}

@(test)
adding_never_replaces_and_renaming_checks_names :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch("add")
	remove_folder(folder)
	defer remove_folder(folder)
	first, _, ok := presets.add(folder, transmute([]u8)string("1"), "Portra")
	testing.expect(t, ok)
	second, _, ok2 := presets.add(folder, transmute([]u8)string("2"), "portra")
	testing.expect(t, ok2)
	testing.expect_value(t, presets.file_name_of(second), "portra 2")
	_ = fs.write(fs.join(folder, ".partial.tmp", .Unix), nil)
	_ = fs.write(fs.join(folder, "notes.txt", .Unix), nil)
	hidden := fs.join(folder, "Hidden.json", .Unix)
	_ = fs.write(hidden, transmute([]u8)string("{}"))
	fs.hide(hidden)
	listed, _, _ := presets.list(folder)
	testing.expect(t, same(names_of(listed), {"Portra", "portra 2"}))
	_, clash, renamed := presets.rename(folder, second, "PORTRA")
	testing.expect(t, !renamed && strings.contains(clash, "already exists"))
	_, _, renamed = presets.rename(folder, second, "a/b")
	testing.expect(t, !renamed)
	recased, _, ok3 := presets.rename(folder, first, "PORTRA")
	testing.expect(t, ok3)
	listed, _, _ = presets.list(folder)
	testing.expect(t, same(names_of(listed), {"PORTRA", "portra 2"}))
	warm, _, ok4 := presets.rename(folder, recased, "Portra warm")
	testing.expect(t, ok4)
	data, _, ok5 := presets.read(warm)
	testing.expect(t, ok5 && string(data) == "1")
}

@(test)
the_users_order_is_remembered :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch("order")
	remove_folder(folder)
	defer remove_folder(folder)
	for n in ([]string{"Zeta", "Alpha", "Mid"}) {
		_, _, ok := presets.add(folder, transmute([]u8)n, n)
		testing.expect(t, ok)
	}
	listed, _, _ := presets.list(folder)
	testing.expect(t, same(names_of(listed), {"Zeta", "Alpha", "Mid"}))
	_, ok := presets.set_order(folder, {listed[2], listed[0], listed[1]})
	testing.expect(t, ok)
	_ = fs.write(fs.join(folder, "Copied B.json", .Unix), transmute([]u8)string("x"))
	_ = fs.write(fs.join(folder, "Copied A.json", .Unix), transmute([]u8)string("x"))
	listed, _, _ = presets.list(folder)
	testing.expect(t, same(names_of(listed), {"Mid", "Zeta", "Alpha", "Copied A", "Copied B"}))
	_, _, ok = presets.rename(folder, listed[1], "Omega")
	testing.expect(t, ok)
	_, ok = presets.delete_preset(folder, listed[0])
	testing.expect(t, ok)
	_, _, ok = presets.add(folder, transmute([]u8)string("n"), "Newest")
	listed, _, _ = presets.list(folder)
	testing.expect(t, same(names_of(listed), {"Omega", "Alpha", "Copied A", "Copied B", "Newest"}))
	order, _ := fs.read(fs.join(folder, ".order.json", .Unix))
	testing.expect(t, strings.has_prefix(string(order), "{\n  \"order\": [\n    \"Omega.json\",\n"))
}

@(test)
files_changed_on_disk_are_left_alone :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch("changed")
	remove_folder(folder)
	defer remove_folder(folder)
	preset, _, _ := presets.add(folder, transmute([]u8)string("original"), "Gold")
	_ = fs.write(preset.path, transmute([]u8)string("edited elsewhere"))
	err, ok := presets.delete_preset(folder, preset)
	testing.expect(t, !ok && strings.contains(err, "changed on disk"))
	_, _, ok = presets.read(preset)
	testing.expect(t, !ok)
	listed, _, _ := presets.list(folder)
	_, ok = presets.delete_preset(folder, listed[0])
	testing.expect(t, ok)
	listed, _, _ = presets.list(folder)
	testing.expect_value(t, len(listed), 0)
}
