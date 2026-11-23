// presets.rs's Library: preset files in a folder, named after the file,
// holding Python spektrafilm's saved GUI state (docs/presets-design.md).
//
// Writes never replace a file, and renames and deletes act only on the
// file as it was listed. The user's order is `.order.json` in the folder,
// `{"order": [file names]}`; presets it doesn't name follow in name order,
// and added presets go to the end.
//
// Every result and message is in the temporary allocator.
package presets

import "core:slice"
import "core:strings"
import "core:unicode/utf8"
import "../files"
import "../paths"
import "../../value"

// A preset file as listed: its path as found, size and modification time,
// which renames and deletes check first.
Preset_File :: struct {
	path:    string,
	size:    u64,
	seconds: i64,
	nanos:   u32,
}

// Larger files aren't GUI states.
MAX_FILE_SIZE :: 1_048_576

ORDER_FILE :: ".order.json"

// `PresetFile::name`: the file's stem.
file_name_of :: proc(f: Preset_File) -> string {
	stem, _ := paths.file_stem(f.path, paths.HOST)
	return stem
}

@(private)
join :: proc(folder, name: string) -> string {
	return paths.join(folder, name, paths.HOST, context.temp_allocator)
}

@(private)
cat :: proc(parts: ..string) -> string {
	return strings.concatenate(parts, context.temp_allocator)
}

// A file's text for the engine: past a UTF-8 byte order mark, or -1 when
// it isn't UTF-8 ("The file isn’t text.").
text_start :: proc "contextless" (data: []u8) -> int {
	start := 0
	if len(data) >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
		start = 3
	}
	if !utf8.valid_string(string(data[start:])) {
		return -1
	}
	return start
}

// Explorer's preview pane, an editor or a virus scanner has it open.
in_use_error :: proc(name: string) -> string {
	return cat("“", name, "” is open in another program, such as Explorer’s preview pane. Close it and try again.")
}

@(private)
ERROR_SHARING_VIOLATION :: files.Code(32)
@(private)
ERROR_LOCK_VIOLATION :: files.Code(33)
@(private)
ERROR_FILE_EXISTS :: files.Code(80)
@(private)
ERROR_ALREADY_EXISTS :: files.Code(183)

// The Win32 codes of presets.rs's `in_use` and `exists`. On macOS the
// codes are the stand-ins' (see files.move_code), which never report the
// first two.
@(private)
in_use :: proc "contextless" (code: files.Code) -> bool {
	return code == ERROR_SHARING_VIOLATION || code == ERROR_LOCK_VIOLATION
}

@(private)
exists :: proc "contextless" (code: files.Code) -> bool {
	return code == ERROR_ALREADY_EXISTS || code == ERROR_FILE_EXISTS
}

// presets.rs's `file_error`. `raw_os_error` is the errno on macOS, which
// can't be 32 or 33 there... except that it can: EPIPE is 32 and EDOM 33.
// The Rust compares the raw code with the Win32 ones on every platform,
// and so does this.
@(private)
file_error :: proc(code: files.Code, path, what: string) -> string {
	stem, _ := paths.file_stem(path, paths.HOST)
	if in_use(code) {
		return in_use_error(stem)
	}
	return cat(what, " “", stem, "”: ", files.error_text(code), ".")
}

// A file's bytes, within the size limit.
read_file :: proc(path: string) -> (data: []u8, err: string, ok: bool) {
	st, code := files.stat(path)
	if code != 0 {
		return nil, file_error(code, path, "Couldn’t read"), false
	}
	if st.is_dir {
		return nil, "It’s a folder.", false
	}
	if st.size > MAX_FILE_SIZE {
		return nil, "The file is too large to be a preset.", false
	}
	data, code = files.read_file(path)
	if code != 0 {
		return nil, file_error(code, path, "Couldn’t read"), false
	}
	return data, "", true
}

// A regular file's size and modification time, read now.
file_at :: proc(path: string) -> (f: Preset_File, err: string, ok: bool) {
	st, code := files.stat(path)
	if code != 0 {
		return {}, file_error(code, path, "Couldn’t read"), false
	}
	if !st.is_file {
		name, _ := paths.file_name(path, paths.HOST)
		return {}, cat("“", name, "” isn’t a file."), false
	}
	return Preset_File{strings.clone(path, context.temp_allocator), st.size, st.seconds, st.nanos}, "", true
}

@(private)
Keyed :: struct {
	key:  string,
	file: Preset_File,
}

// The presets in the user's order. Hidden files (a leading period or the
// hidden attribute) and files other than `.json` aren't presets.
list :: proc(folder: string) -> (out: []Preset_File, err: string, ok: bool) {
	names, code := files.read_dir(folder)
	if code != 0 {
		if files.is_not_found(code) {
			return nil, "", true
		}
		return nil, cat("Couldn’t list the presets: ", files.error_text(code), "."), false
	}
	found := make([dynamic]Keyed, 0, len(names), context.temp_allocator)
	for name in names {
		// `file_name()?.to_str()?`: names that aren't UTF-8 are skipped.
		if !utf8.valid_string(name) {
			continue
		}
		path := join(folder, name)
		file_name, has_name := paths.file_name(path, paths.HOST)
		if !has_name {
			continue
		}
		ext, has_ext := paths.extension(path, paths.HOST)
		json := has_ext && strings.equal_fold(ext, "json") && len(ext) == 4
		if strings.has_prefix(file_name, ".") || !json {
			continue
		}
		st, sc := files.stat(path)
		if sc != 0 || st.hidden {
			continue
		}
		f, _, fine := file_at(path)
		if !fine {
			continue
		}
		append(&found, Keyed{key(file_name_of(f)), f})
	}
	slice.stable_sort_by(found[:], proc(a, b: Keyed) -> bool {
		return a.key < b.key
	})
	position := make(map[string]int, 16, context.temp_allocator)
	for name, i in read_order(folder) {
		k := key(name)
		if k not_in position {
			position[k] = i
		}
	}
	Placed :: struct {
		at:   int,
		file: Preset_File,
	}
	placed := make([]Placed, len(found), context.temp_allocator)
	for k, i in found {
		placed[i] = Placed{position[k.key] if k.key in position else max(int), k.file}
	}
	slice.stable_sort_by(placed, proc(a, b: Placed) -> bool {
		return a.at < b.at
	})
	out = make([]Preset_File, len(placed), context.temp_allocator)
	for p, i in placed {
		out[i] = p.file
	}
	return out, "", true
}

// Preset names in the user's order; none if the file is missing or
// unreadable.
read_order :: proc(folder: string) -> []string {
	data, code := files.read_file(join(folder, ORDER_FILE))
	if code != 0 {
		return nil
	}
	v, perr := value.parse(data, context.temp_allocator)
	if perr != .None {
		return nil
	}
	o, is_object := v.(value.Object)
	if !is_object {
		return nil
	}
	names_value, _ := value.object_get(o, "order")
	names, is_array := names_value.(value.Array)
	if !is_array {
		return nil
	}
	out := make([dynamic]string, 0, len(names), context.temp_allocator)
	for n in names {
		if s, is_string := n.(string); is_string {
			append(&out, strings.trim_suffix(s, ".json"))
		}
	}
	return out[:]
}

// `{"order": [names + ".json"]}` as serde_json's to_vec_pretty writes it.
order_text :: proc(names: []string) -> []u8 {
	items := value.make_array(len(names), context.temp_allocator)
	for n in names {
		append(&items, value.Value(cat(n, ".json")))
	}
	o := value.make_object(1, context.temp_allocator)
	value.object_push(&o, "order", items)
	b := strings.builder_make(0, 64 + 32 * len(names), context.temp_allocator)
	value.write_pretty(&b, o)
	return b.buf[:]
}

write_order :: proc(folder: string, names: []string) -> (err: string, ok: bool) {
	path := join(folder, ORDER_FILE)
	if e, fine := files.write_atomic(path, order_text(names)); !fine {
		return cat("Couldn’t save the order of the presets: ", e), false
	}
	// Kept out of Explorer's way, as on the Mac.
	files.hide(path)
	return "", true
}

// Remembers this order of the presets.
set_order :: proc(folder: string, presets: []Preset_File) -> (err: string, ok: bool) {
	names := make([]string, len(presets), context.temp_allocator)
	for p, i in presets {
		names[i] = file_name_of(p)
	}
	return write_order(folder, names)
}

// The file is still the one listed: not replaced, edited or removed since.
verify :: proc(preset: Preset_File) -> (err: string, ok: bool) {
	current, _, fine := file_at(preset.path)
	if fine && current.size == preset.size && current.seconds == preset.seconds && current.nanos == preset.nanos {
		return "", true
	}
	return cat("“", file_name_of(preset), "” changed on disk. The list has been updated; try again."), false
}

@(private)
counter: u64

// presets.rs's `unique_id`: the process, the time and a counter, in hex.
@(private)
unique_id :: proc() -> string {
	b := strings.builder_make(0, 48, context.temp_allocator)
	strings.write_u64(&b, u64(files.process_id()), 16)
	strings.write_byte(&b, '-')
	nanos := files.now_nanos()
	strings.write_u64(&b, u64(nanos) if nanos > 0 else 0, 16)
	strings.write_byte(&b, '-')
	strings.write_u64(&b, counter, 16)
	counter += 1
	return strings.to_string(b)
}

// Writes `data` as a new preset named `name`, or "name 2"… if taken.
add :: proc(folder: string, data: []u8, name: string) -> (f: Preset_File, err: string, ok: bool) {
	if p, bad := problem(name); bad {
		return {}, p, false
	}
	if code := files.create_dir_all(folder); code != 0 {
		return {}, cat("Couldn’t create the Presets folder: ", files.error_text(code), "."), false
	}
	temporary := join(folder, cat(".", unique_id(), ".tmp"))
	if code := files.write_new_synced(temporary, data); code != 0 {
		_ = files.remove_file(temporary)
		return {}, cat("Couldn’t save the preset: ", files.error_text(code), "."), false
	}
	listed, list_err, listed_ok := list(folder)
	if !listed_ok {
		return {}, list_err, false
	}
	taken := make(map[string]bool, len(listed) + 4, context.temp_allocator)
	for p in listed {
		taken[key(file_name_of(p))] = true
	}
	for _ in 0 ..< 1000 {
		candidate := unique(name, taken)
		path := join(folder, cat(candidate, ".json"))
		// Never replaces: a file created meanwhile fails it.
		code, message := files.move_no_replace(temporary, path)
		if code == 0 {
			now, now_err, now_ok := list(folder)
			if !now_ok {
				return {}, now_err, false
			}
			candidate_key := key(candidate)
			order := make([dynamic]string, 0, len(now) + 1, context.temp_allocator)
			for p in now {
				n := file_name_of(p)
				if key(n) != candidate_key {
					append(&order, n)
				}
			}
			append(&order, candidate)
			if e, fine := write_order(folder, order[:]); !fine {
				return {}, e, false
			}
			return file_at(path)
		}
		if exists(code) {
			taken[key(candidate)] = true
			continue
		}
		_ = files.remove_file(temporary)
		return {}, cat("Couldn’t save the preset: ", message, "."), false
	}
	_ = files.remove_file(temporary)
	return {}, "Couldn’t find a free name for the preset.", false
}

rename :: proc(folder: string, preset: Preset_File, name: string) -> (f: Preset_File, err: string, ok: bool) {
	if p, bad := problem(name); bad {
		return {}, p, false
	}
	if e, fine := verify(preset); !fine {
		return {}, e, false
	}
	path := join(folder, cat(name, ".json"))
	own := file_name_of(preset)
	name_key := key(name)
	same := name_key == key(own)
	// Another case of its own name is the same file there, which the move
	// renames; another preset with the name refuses it.
	if !same {
		listed, list_err, listed_ok := list(folder)
		if !listed_ok {
			return {}, list_err, false
		}
		for p in listed {
			if key(file_name_of(p)) == name_key {
				return {}, cat("A preset named “", name, "” already exists."), false
			}
		}
	}
	if code, message := files.move_no_replace(preset.path, path); code != 0 {
		if exists(code) {
			return {}, cat("A preset named “", name, "” already exists."), false
		}
		if in_use(code) {
			return {}, in_use_error(own), false
		}
		trimmed := strings.trim_right(message, ".\r\n")
		return {}, cat("Couldn’t rename the preset: ", trimmed, "."), false
	}
	order := read_order(folder)
	own_key := key(own)
	for n, i in order {
		if key(n) == own_key {
			order[i] = name
			if e, fine := write_order(folder, order); !fine {
				return {}, e, false
			}
			break
		}
	}
	return file_at(path)
}

delete_preset :: proc(folder: string, preset: Preset_File) -> (err: string, ok: bool) {
	if e, fine := verify(preset); !fine {
		return e, false
	}
	if code := files.remove_file(preset.path); code != 0 {
		return file_error(code, preset.path, "Couldn’t delete"), false
	}
	own_key := key(file_name_of(preset))
	order := make([dynamic]string, 0, 16, context.temp_allocator)
	for n in read_order(folder) {
		if key(n) != own_key {
			append(&order, n)
		}
	}
	return write_order(folder, order[:])
}

read :: proc(preset: Preset_File) -> (data: []u8, err: string, ok: bool) {
	if e, fine := verify(preset); !fine {
		return nil, e, false
	}
	return read_file(preset.path)
}
