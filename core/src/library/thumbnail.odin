// The names edited thumbnails are filed under (`PhotoSidecar.look`,
// `EditedThumbnailStore.key`; thumbnail.rs).
package library

import "core:fmt"

// Identifies how stored edits develop: a hash of them and the table
// they're stored against. The same on every platform for the same file.
look :: proc(edits: Json, defaults_version: i64, allocator := context.allocator) -> string {
	ta := context.temp_allocator
	both := make(Json_Object, 0, 2, ta)
	object_set(&both, "defaultsVersion", f64(defaults_version))
	object_set(&both, "edits", edits)
	return digest16(transmute([]u8)to_compact_sorted(both, ta), allocator)
}

// Names the thumbnail of a photograph without stored edits, developed as
// `state` (`EditorModel.uneditedLook`).
unedited_look :: proc(state: Edit_State, allocator := context.allocator) -> string {
	ta := context.temp_allocator
	plain := user_edits(state, ta)
	for path in ([]string{IMAGE_SIZE_PATH, "io.output_color_space", "io.output_cctf_encoding"}) {
		json_set_at(&plain.params, path, nil, ta)
	}
	edits := make(Json_Object, 0, 6, ta)
	object_set(&edits, "film", plain.film)
	object_set(&edits, "paper", plain.paper)
	object_set(&edits, "params", plain.params)
	object_set(&edits, "crop", plain.crop)
	object_set(&edits, "border", plain.border)
	object_set(&edits, "match", plain.has_match)
	outer := make(Json_Object, 0, 1, ta)
	object_set(&outer, "unedited", edits)
	return digest16(transmute([]u8)to_compact_sorted(outer, ta), allocator)
}

// Seconds since 1970, as `Date.timeIntervalSince1970` (from nanoseconds).
seconds_since_1970 :: proc(ns: i64) -> f64 {
	if ns >= 0 {
		return f64(ns / 1_000_000_000) + f64(ns % 1_000_000_000) / 1e9
	}
	d := u64(-(ns + 1)) + 1
	return -(f64(d / 1_000_000_000) + f64(d % 1_000_000_000) / 1e9)
}

// The name of an item's thumbnail with stored edits `look`
// (`EditedThumbnailStore.key`). `size` < 0 is unknown.
thumbnail_key :: proc(path: string, has_size: bool, size: u64, modified: f64, variant: string, look_name, engine: string, allocator := context.allocator) -> string {
	ta := context.temp_allocator
	s := i64(size) if has_size else -1
	text := fmt.aprintf("1|%s|%d|%s|%s|%s|%s", path, s, swift_double_description(modified, ta), variant, look_name, engine, allocator = ta)
	return digest16(transmute([]u8)text, allocator)
}

// The same under any engine (`EditedThumbnailStore.looseKey`).
thumbnail_loose_key :: proc(path: string, has_size: bool, size: u64, modified: f64, variant: string, look_name: string, allocator := context.allocator) -> string {
	ta := context.temp_allocator
	s := i64(size) if has_size else -1
	text := fmt.aprintf("loose|1|%s|%d|%s|%s|%s", path, s, swift_double_description(modified, ta), variant, look_name, allocator = ta)
	return digest16(transmute([]u8)text, allocator)
}
