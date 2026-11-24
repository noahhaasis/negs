// core/src/platform/fs, the parts that are plain logic and so run on any
// platform: std::path's rules in both styles, Display's lossy text, WTF-8
// and UTF-16, std's long paths on Windows (with GetFullPathNameW stood in
// for), and errors' text. Run by `build.py test`.
package platform_fs_tests

import "base:runtime"
import "core:strings"
import "core:testing"
import fs "../../../src/platform/fs"

@(test)
unix_paths_as_std :: proc(t: ^testing.T) {
	U :: fs.Style.Unix
	name :: proc(p: string) -> string {
		n, ok := fs.file_name(p, .Unix)
		return n if ok else "<none>"
	}
	parent :: proc(p: string) -> string {
		n, ok := fs.parent(p, .Unix)
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
	stem, _ := fs.file_stem(".hidden", U)
	_, has_ext := fs.extension(".hidden", U)
	testing.expect_value(t, stem, ".hidden")
	testing.expect(t, !has_ext)
	ext, has := fs.extension("a.", U)
	testing.expect(t, has && ext == "")
	stem, _ = fs.file_stem("..a", U)
	testing.expect_value(t, stem, ".")
	w := fs.with_extension("c/rendered-preview.cache", "tmp", U)
	defer delete(w)
	testing.expect_value(t, w, "c/rendered-preview.tmp")
	w2 := fs.with_extension("s/active-export.json", "json.tmp", U)
	defer delete(w2)
	testing.expect_value(t, w2, "s/active-export.json.tmp")
	j := fs.join("d/", "x", U)
	defer delete(j)
	testing.expect_value(t, j, "d/x")
	testing.expect(t, !fs.is_absolute("relative.png", U))
	testing.expect(t, fs.is_absolute("/a", U))
}

@(test)
windows_paths_as_std :: proc(t: ^testing.T) {
	W :: fs.Style.Windows
	name :: proc(p: string) -> string {
		n, ok := fs.file_name(p, .Windows)
		return n if ok else "<none>"
	}
	parent :: proc(p: string) -> string {
		n, ok := fs.parent(p, .Windows)
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
	p := fs.parse_prefix("\\\\?\\UNC\\srv\\sh\\x", W)
	testing.expect_value(t, p.kind, fs.Prefix_Kind.Verbatim_UNC)
	testing.expect_value(t, p.len, 14)
	p = fs.parse_prefix("//./COM1", W)
	testing.expect_value(t, p.kind, fs.Prefix_Kind.Device_NS)
	testing.expect_value(t, p.len, 8)
	// parse_drive_exact takes `/` as well as `\` after the drive (1.93).
	p = fs.parse_prefix("\\\\?\\C:/x", W)
	testing.expect_value(t, p.kind, fs.Prefix_Kind.Verbatim_Disk)
	p = fs.parse_prefix("\\\\?\\C:x", W)
	testing.expect_value(t, p.kind, fs.Prefix_Kind.Verbatim)
	testing.expect_value(t, p.len, 7)
	// `//?/` isn't verbatim: only real backslashes make it so.
	p = fs.parse_prefix("//?/C:/x", W)
	testing.expect_value(t, p.kind, fs.Prefix_Kind.UNC)
	stem, _ := fs.file_stem("C:\\p\\IMG.CR3", W)
	testing.expect_value(t, stem, "IMG")
	for c in ([][3]string{{"C:", "a", "C:a"}, {"C:\\d", "a", "C:\\d\\a"}, {"C:\\d\\", "a", "C:\\d\\a"}, {"C:/d/", "*", "C:/d/*"}}) {
		j := fs.join(c[0], c[1], W)
		defer delete(j)
		testing.expect_value(t, j, c[2])
	}
}

@(test)
paths_join_as_pathbuf_push :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	Case :: struct {
		base, name, want: string,
		style:            fs.Style,
	}
	cases := []Case {
		{`C:\Users\Noah\AppData\Roaming`, "Negs", `C:\Users\Noah\AppData\Roaming\Negs`, .Windows},
		{`C:\Negs\`, "Presets", `C:\Negs\Presets`, .Windows},
		{"", "Negs", "Negs", .Windows},
		{"C:", "x", "C:x", .Windows},
		{`C:\a`, `D:\b`, `D:\b`, .Windows},
		{`C:\a`, `\b`, `C:\b`, .Windows},
		{`\\server\share`, "x", `\\server\share\x`, .Windows},
		{`\\?\C:\a`, "b", `\\?\C:\a\b`, .Windows},
		// Verbatim paths are rebuilt, `.` and `..` resolved.
		{`\\?\C:\a`, `..\b`, `\\?\C:\b`, .Windows},
		{`\\?\C:\a\`, `.\b`, `\\?\C:\a\b`, .Windows},
		{`\\?\UNC\server\share`, "x", `\\?\UNC\server\share\x`, .Windows},
		{"/tmp", "negs", "/tmp/negs", .Unix},
		{"/tmp/", "negs", "/tmp/negs", .Unix},
		{"/tmp", "/etc", "/etc", .Unix},
		{"", "x", "x", .Unix},
	}
	for c in cases {
		testing.expectf(t, fs.join(c.base, c.name, c.style) == c.want, "%q + %q = %q", c.base, c.name, fs.join(c.base, c.name, c.style))
	}
	testing.expect(t, fs.is_absolute(`C:\photos\DSC_1.NEF`, .Windows))
	testing.expect(t, !fs.is_absolute(`\photos\DSC_1.NEF`, .Windows))
	testing.expect(t, !fs.is_absolute("C:photos", .Windows))
	testing.expect(t, fs.is_absolute(`\\server\share\a`, .Windows))
	testing.expect(t, !fs.is_absolute("relative.png", .Unix))
	testing.expect_value(t, fs.with_extension(`C:\Negs\.order.json`, "tmp", .Windows), `C:\Negs\.order.tmp`)
	testing.expect_value(t, fs.with_extension("/a/settings.json", "tmp", .Unix), "/a/settings.tmp")
	stem, _ := fs.file_stem(`C:\Negs\Presets\Portra.v2.json`, .Windows)
	testing.expect_value(t, stem, "Portra.v2")
}

// Path::with_extension as std 1.93: the old extension's length off the
// end, then set_extension (which cuts after the stem).
@(test)
with_extension_as_std :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	U :: fs.Style.Unix
	testing.expect_value(t, fs.with_extension("a/b.json/", "tmp", U), "a/b.tmp")
	testing.expect_value(t, fs.with_extension("a/b..", "tmp", U), "a/b..tmp")
	testing.expect_value(t, fs.with_extension("a/b", "tmp", U), "a/b.tmp")
	testing.expect_value(t, fs.with_extension("a/b.c", "", U), "a/b")
	testing.expect_value(t, fs.with_extension("/", "tmp", U), "/")
	testing.expect_value(t, fs.with_extension("..", "tmp", U), "..")
	testing.expect_value(t, fs.with_extension(".hidden", "tmp", U), ".hidden.tmp")
	testing.expect_value(t, fs.with_extension(`C:\x\y.CR3`, "tmp", .Windows), `C:\x\y.tmp`)
}

@(test)
lossy_text_as_std :: proc(t: ^testing.T) {
	d := fs.display("a\xe2\x82Ab\xffc\xed\xa0\x80\xf0\x9f\x98\x80", .Unix, context.allocator)
	defer delete(d)
	testing.expect_value(t, d, "a\uFFFDAb\uFFFDc\uFFFD\uFFFD\uFFFD😀")
	// On Windows a path is WTF-8: an unpaired surrogate is one U+FFFD.
	w := fs.display("c\xed\xa0\x80d\xed\xbf\xbf", .Windows, context.allocator)
	defer delete(w)
	testing.expect_value(t, w, "c\uFFFDd\uFFFD")
	v := fs.display("plain é", .Windows, context.allocator)
	defer delete(v)
	testing.expect_value(t, v, "plain é")
}

@(test)
wtf8_and_utf16 :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	units, ok := fs.to_utf16("aé中😀", true)
	testing.expect(t, ok)
	testing.expect_value(t, len(units), 6)
	testing.expect(t, units[3] == 0xD83D && units[4] == 0xDE00 && units[5] == 0)
	back, unicode := fs.from_utf16(units[:5])
	testing.expect(t, unicode)
	testing.expect_value(t, back, "aé中😀")
	// An unpaired surrogate survives the round trip, as OsString keeps it.
	lone := []u16{'x', 0xD800, 'y', 0xDC01}
	s, u := fs.from_utf16(lone)
	testing.expect(t, !u)
	testing.expect_value(t, s, "x\xed\xa0\x80y\xed\xb0\x81")
	again, _ := fs.to_utf16(s, false)
	testing.expect(t, len(again) == 4 && again[1] == 0xD800 && again[3] == 0xDC01)
	_, nul_ok := fs.to_utf16("a\x00b", true)
	testing.expect(t, !nul_ok)
	_, plain := fs.to_utf16("a\x00b", false)
	testing.expect(t, plain)
}

// MARK: Long paths

// GetFullPathNameW, stood in for: each path std's tests give it, and what
// Windows makes of it.
// Per thread: the test runner runs tests side by side.
@(private, thread_local)
full_paths: map[string]string

@(private, thread_local)
full_calls: int

@(private)
fake_full :: proc(path: []u16, allocator: runtime.Allocator) -> ([]u16, fs.Error) {
	full_calls += 1
	text, _ := fs.from_utf16(path[:len(path) - 1], context.temp_allocator)
	out, found := full_paths[text]
	if !found {
		return nil, fs.Error(123) // ERROR_INVALID_NAME
	}
	units, _ := fs.to_utf16(out, false, allocator)
	return units, 0
}

@(private)
verbatim :: proc(path: string, prefer := true) -> (string, fs.Error) {
	units, ok := fs.to_utf16(path, true, context.temp_allocator)
	if !ok {
		return "", fs.NUL_IN_PATH
	}
	long, err := fs.long_path(units, prefer, fake_full, context.temp_allocator)
	if err != 0 {
		return "", err
	}
	if len(long) == 0 || long[len(long) - 1] != 0 {
		return "<no NUL>", 0
	}
	s, _ := fs.from_utf16(long[:len(long) - 1], context.temp_allocator)
	return s, 0
}

// std's own `verbatim` test (sys/path/windows/tests.rs), and the rules
// around it.
@(test)
long_paths_as_std :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	full_paths = make(map[string]string, 16, context.temp_allocator)
	full_calls = 0
	a := strings.repeat("a", 250)
	b := strings.repeat("b", 250)
	check :: proc(t: ^testing.T, path, want: string, loc := #caller_location) {
		got, err := verbatim(path)
		testing.expectf(t, err == 0 && got == want, "%q: got %q (%v), want %q", path, got, err, want, loc = loc)
	}
	// Long paths get their prefix (GetFullPathNameW gives them back as they
	// are, already absolute and tidy).
	long_drive := strings.concatenate({`C:\Program Files\Rust\`, a, `\foo.txt`})
	full_paths[long_drive] = long_drive
	check(t, long_drive, strings.concatenate({`\\?\`, long_drive}))
	long_unc := strings.concatenate({`\\server\share\`, a, `\foo.txt`})
	full_paths[long_unc] = long_unc
	check(t, long_unc, strings.concatenate({`\\?\UNC\server\share\`, a, `\foo.txt`}))
	long_pipe := strings.concatenate({`\\.\PIPE\`, a})
	full_paths[long_pipe] = long_pipe
	check(t, long_pipe, strings.concatenate({`\\?\PIPE\`, a}))
	// `\\?\` and `\??\` are left as they are, without asking Windows.
	calls := full_calls
	already := strings.concatenate({`\\?\verbatim.\`, a, `\`, b, `\foo.txt`})
	check(t, already, already)
	nt := strings.concatenate({`\??\C:\`, a, `\`, b})
	check(t, nt, nt)
	testing.expect_value(t, full_calls, calls)
	// `//?/` isn't a verbatim prefix: Windows normalises it (a trailing dot
	// goes), and the `\\?\` it gives back stays.
	slashed := strings.concatenate({`//?/E:/verbatim.\`, a, `\`, b, `\foo.txt`})
	full_paths[slashed] = strings.concatenate({`\\?\E:\verbatim\`, a, `\`, b, `\foo.txt`})
	check(t, slashed, strings.concatenate({`\\?\E:\verbatim\`, a, `\`, b, `\foo.txt`}))
	// Short absolute paths are left alone, without asking Windows.
	calls = full_calls
	check(t, `C:\Program Files\Rust`, `C:\Program Files\Rust`)
	check(t, `C:/Program Files/Rust`, `C:/Program Files/Rust`)
	check(t, `\\server\share`, `\\server\share`)
	check(t, `\\.\COM1`, `\\.\COM1`)
	check(t, "Z:", "Z:")
	check(t, "", "")
	testing.expect_value(t, full_calls, calls)
	// The threshold: 246 units and the NUL are short; 247 are made verbatim
	// (CreateDirectoryW's limit is 248 with the NUL).
	p246 := strings.concatenate({`C:\`, strings.repeat("a", 243)})
	check(t, p246, p246)
	p247 := strings.concatenate({`C:\`, strings.repeat("a", 244)})
	full_paths[p247] = p247
	check(t, p247, strings.concatenate({`\\?\`, p247}))
	// Relative paths, and paths with a drive or a root but not both, are
	// made absolute by Windows and then verbatim, as maybe_verbatim
	// prefers.
	full_paths["photo.NEF"] = `C:\Users\Noah\photo.NEF`
	check(t, "photo.NEF", `\\?\C:\Users\Noah\photo.NEF`)
	full_paths[`C:photo.NEF`] = `C:\Users\Noah\photo.NEF`
	check(t, `C:photo.NEF`, `\\?\C:\Users\Noah\photo.NEF`)
	full_paths[`\photo.NEF`] = `C:\photo.NEF`
	check(t, `\photo.NEF`, `\\?\C:\photo.NEF`)
	full_paths[`..\a\.\b`] = `C:\Users\a\b`
	check(t, `..\a\.\b`, `\\?\C:\Users\a\b`)
	// GetFullPathNameW's error is the call's.
	_, err := verbatim("not-in-the-table")
	testing.expect_value(t, err, fs.Error(123))
	// A path with a NUL never reaches Windows.
	_, err = verbatim("a\x00b")
	testing.expect_value(t, err, fs.NUL_IN_PATH)
	_, err = fs.maybe_verbatim("\x00", fake_full, context.temp_allocator)
	testing.expect_value(t, err, fs.NUL_IN_PATH)
}

// get_long_path's two steps on their own, and `prefer_verbatim` off.
@(test)
long_path_steps :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	w :: proc(s: string, nul := true) -> []u16 {
		u, _ := fs.to_utf16(s, nul, context.temp_allocator)
		return u
	}
	testing.expect(t, fs.keeps_path(w(`C:\x`)))
	testing.expect(t, fs.keeps_path(w(`C:/x`)))
	testing.expect(t, fs.keeps_path(w("C:")))
	testing.expect(t, fs.keeps_path(w(`//server/share`)))
	testing.expect(t, fs.keeps_path(w(`\\?\anything`)))
	testing.expect(t, fs.keeps_path(w("")))
	testing.expect(t, !fs.keeps_path(w(`C:x`)))
	testing.expect(t, !fs.keeps_path(w(`\x`)))
	testing.expect(t, !fs.keeps_path(w(`/x`)))
	testing.expect(t, !fs.keeps_path(w("x")))
	// The "drive" is any unit but a separator, as std's pattern has it.
	testing.expect(t, fs.keeps_path(w(`1:\x`)))
	testing.expect(t, !fs.keeps_path(w(`\:\x`)))
	testing.expect(t, !fs.keeps_path(w(strings.concatenate({`C:\`, strings.repeat("a", 245)}))))

	prefix, drop := fs.verbatim_prefix(w(`C:\x`, false), true)
	testing.expect(t, prefix == `\\?\` && drop == 0)
	prefix, drop = fs.verbatim_prefix(w(`\\.\COM1`, false), true)
	testing.expect(t, prefix == `\\?\` && drop == 4)
	prefix, drop = fs.verbatim_prefix(w(`\\srv\share\x`, false), true)
	testing.expect(t, prefix == `\\?\UNC\` && drop == 2)
	prefix, drop = fs.verbatim_prefix(w(`\\?\C:\x`, false), true)
	testing.expect(t, prefix == "" && drop == 0)
	prefix, drop = fs.verbatim_prefix(w(`\??\C:\x`, false), true)
	testing.expect(t, prefix == "" && drop == 0)
	// Not preferred: only when the path is long.
	prefix, _ = fs.verbatim_prefix(w(`C:\x`, false), false)
	testing.expect_value(t, prefix, "")
	prefix, _ = fs.verbatim_prefix(w(strings.concatenate({`C:\`, strings.repeat("a", 244)}), false), false)
	testing.expect_value(t, prefix, `\\?\`)
	prefix, _ = fs.verbatim_prefix(w(strings.concatenate({`C:\`, strings.repeat("a", 243)}), false), false)
	testing.expect_value(t, prefix, "")

	full_paths = make(map[string]string, 4, context.temp_allocator)
	full_paths["rel"] = `C:\cwd\rel`
	short, err := verbatim("rel", false)
	testing.expect(t, err == 0 && short == `C:\cwd\rel`)
}

@(test)
errors_in_std_words :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	testing.expect_value(t, fs.error_text(fs.WHOLE_TREE), "failed to create whole tree")
	when ODIN_OS == .Windows {
		testing.expect_value(t, fs.error_text(fs.NUL_IN_PATH), "strings passed to WinAPI cannot contain NULs")
		testing.expect_value(t, fs.error_text(2), "The system cannot find the file specified. (os error 2)")
	} else {
		testing.expect_value(t, fs.error_text(fs.NUL_IN_PATH), "file name contained an unexpected NUL byte")
		testing.expect_value(t, fs.error_text(2), "No such file or directory (os error 2)")
		testing.expect_value(t, fs.error_text(13), "Permission denied (os error 13)")
		testing.expect(t, fs.is_not_found(2))
		testing.expect(t, !fs.is_not_found(13))
	}
	when ODIN_OS == .Darwin {
		// strerror_r's text for a code it doesn't know, as std shows it.
		testing.expect_value(t, fs.error_text(1234), "Unknown error: 1234 (os error 1234)")
	}
}

@(test)
times_as_std :: proc(t: ^testing.T) {
	d, ok := fs.since_epoch({1_700_000_000, 5})
	testing.expect(t, ok && d.secs == 1_700_000_000 && d.nanos == 5)
	_, ok = fs.since_epoch({-1, 999_999_999})
	testing.expect(t, !ok)
	testing.expect_value(t, fs.as_nanos({2, 3}), u128(2_000_000_003))
	testing.expect_value(t, fs.secs_f64({1, 500_000_000}), 1.5)
	testing.expect_value(t, fs.unix_nanos({-1, 999_999_999}), i64(-1))
}
