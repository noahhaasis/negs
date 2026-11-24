// session.rs's folders, Preferences' load and save and list helpers, and
// the CatalogCache.
package session

import "core:slice"
import "core:strings"
import "../files"
import "../paths"
import "../../value"

// `NEGS_PROFILE_DIR`, or in test builds (`test`) a scratch folder, so no
// test can ever write the real profile.
profile_override :: proc(test: bool) -> (string, bool) {
	if dir, set := files.env("NEGS_PROFILE_DIR", context.allocator); set {
		return dir, true
	}
	if test {
		b := strings.builder_make(0, 40, context.temp_allocator)
		strings.write_string(&b, "negs-test-profile-")
		strings.write_u64(&b, u64(files.process_id()))
		return paths.join(files.temp_dir(context.temp_allocator), strings.to_string(b), paths.HOST, context.allocator), true
	}
	return "", false
}

// `%APPDATA%\Negs`.
settings_dir :: proc(test: bool) -> string {
	if dir, ok := profile_override(test); ok {
		return dir
	}
	base, _ := files.env("APPDATA", context.temp_allocator)
	return paths.join(base, "Negs", paths.HOST, context.allocator)
}

// `%LOCALAPPDATA%\Negs`.
cache_dir :: proc(test: bool) -> string {
	if dir, ok := profile_override(test); ok {
		return paths.join(dir, "Cache", paths.HOST, context.allocator)
	}
	base, _ := files.env("LOCALAPPDATA", context.temp_allocator)
	return paths.join(base, "Negs", paths.HOST, context.allocator)
}

Place :: enum u32 {
	Settings_Dir  = 0,
	Cache_Dir     = 1,
	Presets       = 2,
	Settings_File = 3,
	Session_File  = 4,
	Catalog_File  = 5,
}

// The folders and files session.rs and presets.rs name.
place :: proc(which: Place, test: bool) -> string {
	switch which {
	case .Settings_Dir:
		return settings_dir(test)
	case .Cache_Dir:
		return cache_dir(test)
	case .Presets:
		return paths.join(settings_dir(test), "Presets", paths.HOST, context.allocator)
	case .Settings_File:
		return paths.join(settings_dir(test), "settings.json", paths.HOST, context.allocator)
	case .Session_File:
		return paths.join(settings_dir(test), "session.json", paths.HOST, context.allocator)
	case .Catalog_File:
		return paths.join(cache_dir(test), "catalog.json", paths.HOST, context.allocator)
	}
	return ""
}

// MARK: Preferences

// `Preferences::load` of `file`: false (the defaults) unless it reads.
load_preferences :: proc(file: string) -> (Preferences, bool) {
	data, code := files.read_file(file, context.allocator)
	if code != 0 {
		return {}, false
	}
	return parse_preferences(data)
}

Prefs_Save :: enum u8 {
	Failed     = 0,
	Saved      = 1,
	// The text isn't preferences the core reads: nothing was done.
	Unreadable = 2,
}

// `Preferences::save` of the preferences serde_json wrote as `text`
// (pretty, write errors ignored by the caller as the Rust ignores them).
save_preferences :: proc(file: string, text: []u8) -> Prefs_Save {
	prefs, ok := parse_preferences(text)
	if !ok {
		return .Unreadable
	}
	if _, fine := files.write_atomic(file, preferences_text(prefs)); !fine {
		return .Failed
	}
	return .Saved
}

// `set_expanded` (`sorted` false) and `set_revealed` (true): `item` taken
// out of the comma-separated `list`, put back last when `on`.
list_set :: proc(list, item: string, on, sorted: bool) -> string {
	parts := make([dynamic]string, 0, 8, context.temp_allocator)
	for t in strings.split(list, ",", context.temp_allocator) {
		if t != "" && t != item {
			append(&parts, t)
		}
	}
	if on {
		append(&parts, item)
	}
	if sorted {
		slice.sort(parts[:])
	}
	return strings.join(parts[:], ",", context.allocator)
}

// `expanded`: whether `item` is in the comma-separated `list`.
list_has :: proc(list, item: string) -> bool {
	for t in strings.split(list, ",", context.temp_allocator) {
		if t == item {
			return true
		}
	}
	return false
}

Tab :: enum u8 {
	Prep   = 0,
	Lab    = 1,
	Output = 2,
}

// `Preferences::tab`. Post became Output; settings files from before Prep
// that never chose a tab open on Prep.
tab :: proc(inspector_tab: string, chosen: bool) -> Tab {
	switch inspector_tab {
	case "Post", "Output":
		return .Output
	case "Lab":
		if chosen {
			return .Lab
		}
	}
	return .Prep
}

// MARK: CatalogCache

// `CatalogCache::key`: the helper's path, its modification time in
// nanoseconds since 1970 and the data folder; none when the helper can't
// be read or is dated before 1970.
catalog_key :: proc(helper, data: string) -> (string, bool) {
	st, code := files.stat(helper)
	if code != 0 || st.seconds < 0 {
		return "", false
	}
	nanos := u128(st.seconds) * 1_000_000_000 + u128(st.nanos)
	digits: [48]u8
	n := len(digits)
	for {
		n -= 1
		digits[n] = '0' + u8(nanos % 10)
		nanos /= 10
		if nanos == 0 {
			break
		}
	}
	return strings.concatenate({helper, "|", string(digits[n:]), "|", data}, context.allocator), true
}

// `CatalogCache::load`: the cached catalog when its key is this helper's
// and data folder's and it speaks protocol 1.
catalog_load :: proc(file, helper, data: string) -> (catalog_value: value.Value, ok: bool) {
	key := catalog_key(helper, data) or_return
	text, code := files.read_file(file, context.allocator)
	if code != 0 {
		return nil, false
	}
	entry, err := value.parse(text, context.allocator)
	if err != .None {
		return nil, false
	}
	stored := index(entry, "key")
	catalog := index(entry, "catalog")
	protocol, is_number := value.as_f64(index(catalog, "protocol"))
	if s, is_string := stored.(string); !is_string || s != key || !is_number || protocol != 1 {
		return nil, false
	}
	return catalog, true
}

// serde_json's `value[key]`: null unless an object holds it.
@(private)
index :: proc(v: value.Value, key: string) -> value.Value {
	o, is_object := v.(value.Object)
	if !is_object {
		return nil
	}
	found, _ := value.object_get(o, key)
	return found
}

// `CatalogCache::save`: `{"catalog": …, "key": …}`, compact, errors ignored.
catalog_save :: proc(file: string, catalog: value.Value, helper, data: string) {
	key, ok := catalog_key(helper, data)
	if !ok {
		return
	}
	o := value.make_object(2, context.allocator)
	append(&o, value.Field{"catalog", catalog}, value.Field{"key", key})
	b := strings.builder_make(0, 4096, context.allocator)
	value.write_compact(&b, o)
	_, _ = files.write_atomic(file, b.buf[:])
}
