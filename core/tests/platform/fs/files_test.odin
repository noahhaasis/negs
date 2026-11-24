#+build darwin, linux
// core/src/platform/fs's calls on this Mac (or Linux), in a scratch
// folder: what std::fs does here, and what the frozen oracles' stand-ins
// do for the `windows` crate's calls. The Windows half runs at the
// Windows gate.
package platform_fs_tests

import "core:strings"
import "core:sys/posix"
import "core:testing"
import fs "../../../src/platform/fs"

@(private)
scratch_folder :: proc(tag: string) -> string {
	b := strings.builder_make(context.temp_allocator)
	strings.write_string(&b, "negs-core-fs-")
	strings.write_string(&b, tag)
	strings.write_byte(&b, '-')
	strings.write_u64(&b, u64(fs.process_id()))
	folder := fs.join(fs.temp_dir(context.temp_allocator), strings.to_string(b), allocator = context.temp_allocator)
	remove_tree(folder)
	return folder
}

@(private)
remove_tree :: proc(folder: string) {
	entries, err, _ := fs.read_dir(folder, allocator = context.temp_allocator)
	if err != 0 {
		return
	}
	for e in entries {
		path := fs.join(folder, e.name, allocator = context.temp_allocator)
		if e.kind == .Dir {
			remove_tree(path)
		} else {
			_ = fs.remove_file(path)
		}
	}
	posix.rmdir(strings.clone_to_cstring(folder, context.temp_allocator))
}

@(test)
files_written_read_and_stated :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch_folder("files")
	defer remove_tree(folder)
	deep := fs.join(folder, "a/b/c")
	testing.expect_value(t, fs.create_dir_all(deep), fs.Error(0))
	testing.expect_value(t, fs.create_dir_all(deep), fs.Error(0))
	testing.expect(t, fs.is_dir(deep))
	testing.expect_value(t, fs.create_dir_all(""), fs.Error(0))

	file := fs.join(deep, "x.bin")
	testing.expect_value(t, fs.write(file, transmute([]u8)string("head|"), transmute([]u8)string("tail")), fs.Error(0))
	data, err := fs.read(file)
	testing.expect(t, err == 0 && string(data) == "head|tail")
	part, _ := fs.read(file, 4)
	testing.expect_value(t, string(part), "head")
	none, _ := fs.read(file, 0)
	testing.expect_value(t, len(none), 0)
	// Written again: truncated first.
	testing.expect_value(t, fs.write(file, transmute([]u8)string("x")), fs.Error(0))
	data, _ = fs.read(file)
	testing.expect_value(t, string(data), "x")

	st, serr := fs.stat(file)
	testing.expect_value(t, serr, fs.Error(0))
	testing.expect_value(t, st.kind, fs.Kind.File)
	testing.expect_value(t, st.size, u64(1))
	testing.expect(t, !st.readonly && !st.hidden)
	modified, has_modified := st.modified.?
	testing.expect(t, has_modified)
	now := fs.now()
	testing.expect(t, modified.secs > 1_600_000_000 && modified.secs <= now.secs + 1)
	when ODIN_OS == .Darwin {
		created, has_created := st.created.?
		testing.expect(t, has_created && created.secs > 1_600_000_000)
	}
	folder_st, _ := fs.stat(deep)
	testing.expect_value(t, folder_st.kind, fs.Kind.Dir)
	testing.expect(t, fs.is_file(file) && !fs.is_file(deep) && fs.exists(deep))

	// Not there: ErrorKind::NotFound, in std's words.
	missing := fs.join(folder, "missing")
	_, merr := fs.stat(missing)
	testing.expect(t, fs.is_not_found(merr))
	testing.expect_value(t, fs.error_text(merr), "No such file or directory (os error 2)")
	_, rerr := fs.read(missing)
	testing.expect(t, fs.is_not_found(rerr))
	testing.expect(t, fs.is_not_found(fs.remove_file(missing)))
	testing.expect(t, !fs.exists(missing))

	// A NUL never reaches the OS.
	nul := strings.concatenate({file, "\x00.negs"})
	_, nerr := fs.stat(nul)
	testing.expect_value(t, nerr, fs.NUL_IN_PATH)
	testing.expect_value(t, fs.write(nul, nil), fs.NUL_IN_PATH)
	testing.expect_value(t, fs.remove_file(nul), fs.NUL_IN_PATH)
	testing.expect_value(t, fs.rename(nul, file), fs.NUL_IN_PATH)
	testing.expect_value(t, fs.create_dir_all(nul), fs.NUL_IN_PATH)
	testing.expect(t, fs.exists(file))

	// No write bit at all: read-only, as `permissions().readonly()`.
	c := strings.clone_to_cstring(file)
	posix.chmod(c, {.IRUSR})
	ro, _ := fs.stat(file)
	testing.expect(t, ro.readonly)
	posix.chmod(c, {.IRUSR, .IWUSR})

	// A file in the way of a folder.
	blocked := fs.join(file, "sub")
	testing.expect(t, fs.create_dir_all(blocked) != 0)
}

@(test)
moves_as_std_and_the_stand_ins :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch_folder("moves")
	defer remove_tree(folder)
	testing.expect_value(t, fs.create_dir_all(folder), fs.Error(0))
	a := fs.join(folder, "a")
	b := fs.join(folder, "b")
	_ = fs.write(a, transmute([]u8)string("A"))
	_ = fs.write(b, transmute([]u8)string("B"))
	// Refusing to replace: EEXIST (the stand-in calls it 183).
	e := fs.move_no_replace(a, b)
	testing.expect_value(t, e, fs.Error(17))
	testing.expect_value(t, fs.error_text(e), "File exists (os error 17)")
	testing.expect_value(t, fs.win32_error_message(e), "File exists (os error 17)")
	// Replacing.
	testing.expect_value(t, fs.replace_file(a, b), fs.Error(0))
	data, _ := fs.read(b)
	testing.expect_value(t, string(data), "A")
	testing.expect(t, !fs.exists(a))
	testing.expect_value(t, fs.rename(b, a), fs.Error(0))
	testing.expect_value(t, fs.move_no_replace(a, b), fs.Error(0))
	testing.expect(t, fs.exists(b) && !fs.exists(a))

	// create_new refuses a file that is there; a new one is synced.
	n := fs.join(folder, "new")
	testing.expect_value(t, fs.write_new_synced(n, transmute([]u8)string("1")), fs.Error(0))
	testing.expect_value(t, fs.write_new_synced(n, transmute([]u8)string("2")), fs.Error(17))
	data, _ = fs.read(n)
	testing.expect_value(t, string(data), "1")

	// session.rs's write_atomic: the folder made, staged, moved over.
	settings := fs.join(folder, "deeper/settings.json")
	msg, ok := fs.write_atomic(settings, transmute([]u8)string("{}"))
	testing.expect(t, ok && msg == "")
	data, _ = fs.read(settings)
	testing.expect_value(t, string(data), "{}")
	testing.expect(t, !fs.exists(fs.join(folder, "deeper/settings.tmp")))
	msg, ok = fs.write_atomic(fs.join(n, "x.json"), nil)
	// Its parent is a file: mkdir says EEXIST, and it isn't a folder.
	testing.expect(t, !ok && msg == "File exists (os error 17)")
}

@(test)
folders_listed_as_std :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	folder := scratch_folder("list")
	defer remove_tree(folder)
	_ = fs.create_dir_all(fs.join(folder, "sub"))
	_ = fs.write(fs.join(folder, "photo.NEF"), transmute([]u8)string("raw"))
	_ = fs.write(fs.join(folder, ".hidden"), nil)
	hidden := fs.join(folder, "Hidden.json")
	_ = fs.write(hidden, nil)
	fs.hide(hidden)
	link := strings.clone_to_cstring(fs.join(folder, "link"))
	posix.symlink("photo.NEF", link)

	entries, err, stopped := fs.read_dir(folder, true)
	testing.expect(t, err == 0 && stopped == 0)
	testing.expect_value(t, len(entries), 5)
	seen := 0
	for e in entries {
		testing.expect(t, e.kind_ok && e.stat_ok && e.unicode)
		switch e.name {
		case "sub":
			testing.expect_value(t, e.kind, fs.Kind.Dir)
			seen += 1
		case "photo.NEF":
			testing.expect_value(t, e.kind, fs.Kind.File)
			testing.expect_value(t, e.stat.size, u64(3))
			testing.expect(t, e.stat.modified != nil)
			seen += 1
		case "link":
			// file_type() and metadata() don't follow it.
			testing.expect_value(t, e.kind, fs.Kind.Other)
			testing.expect_value(t, e.stat.kind, fs.Kind.Other)
			seen += 1
		case "Hidden.json":
			when ODIN_OS == .Darwin {
				testing.expect(t, e.stat.hidden)
			}
			seen += 1
		case ".hidden":
			seen += 1
		}
	}
	testing.expect_value(t, seen, 5)
	// Without metadata: the kinds only.
	plain, _, _ := fs.read_dir(folder)
	for e in plain {
		testing.expect(t, !e.stat_ok && e.kind_ok)
	}
	when ODIN_OS == .Darwin {
		st, _ := fs.stat(hidden)
		testing.expect(t, st.hidden)
	}
	// Links followed by stat.
	through, _ := fs.stat(fs.join(folder, "link"))
	testing.expect_value(t, through.kind, fs.Kind.File)

	_, merr, _ := fs.read_dir(fs.join(folder, "missing"))
	testing.expect(t, fs.is_not_found(merr))
	_, ferr, _ := fs.read_dir(fs.join(folder, "photo.NEF"))
	testing.expect_value(t, ferr, fs.Error(20))
}

@(test)
the_process :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	_, set := fs.env("NEGS_CORE_FS_TEST_SURELY_UNSET")
	testing.expect(t, !set)
	home, has_home := fs.env("HOME")
	testing.expect(t, has_home && len(home) > 0)
	testing.expect(t, len(fs.temp_dir()) > 0)
	testing.expect(t, fs.process_id() > 0)
}
