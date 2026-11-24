// The `.negs` file beside each photograph (`docs/sidecar-format.md`), as
// `Sidecar.swift` reads, patches and writes it (sidecar.rs).
package library

import "core:fmt"
import "core:slice"
import "core:strings"
import fs "../platform/fs"

// The schema this version reads and writes.
SCHEMA :: 1
// A larger file isn't Negs's and is left alone.
MAXIMUM_SIZE :: 1_048_576
// New sidecars are stored against this defaults table
// (`SidecarDefaults.latest`).
LATEST_DEFAULTS :: 3
// The `edits` keys this version reads and writes; others are kept as found.
EDIT_KEYS := [7]string{"film", "paper", "params", "crop", "border", "brightnessMatch", "roll"}
// What the top of a file may hold for a write to remove it once its one
// variant holds nothing.
@(private = "file")
PLAIN_TOP_KEYS := [4]string{"negs", "app", "defaultsVersion", "variants"}

// A rating's flag; None is unflagged.
Flag :: enum u8 {
	None,
	Pick,
	Reject,
}

flag_name :: proc(f: Flag) -> string {
	switch f {
	case .Pick:
		return "pick"
	case .Reject:
		return "reject"
	case .None:
	}
	return ""
}

flag_parse :: proc(text: string) -> (Flag, bool) {
	switch text {
	case "pick":
		return .Pick, true
	case "reject":
		return .Reject, true
	}
	return .None, false
}

Problem_Kind :: enum u8 {
	Unreadable,
	Newer,
	Too_Large,
}

// Why a sidecar isn't used. It is then never written over either.
Problem :: struct {
	kind:   Problem_Kind,
	reason: string,
}

unreadable :: proc(reason: string) -> Problem {
	return Problem{.Unreadable, reason}
}

// What the app says, as on macOS. `name` is the photograph's file name.
problem_message :: proc(p: Problem, name: string, allocator := context.allocator) -> string {
	switch p.kind {
	case .Unreadable:
		return fmt.aprintf("The edits saved for %s couldn’t be read (%s), so it opens with the defaults. Its .negs file is left as it is.", name, p.reason, allocator = allocator)
	case .Newer:
		return fmt.aprintf("%s was edited in a newer version of Negs, so it opens with the defaults. Update Negs to see its edits; its .negs file is left as it is.", name, allocator = allocator)
	case .Too_Large:
		return fmt.aprintf("The .negs file of %s is too large to be one of Negs’s, so it opens with the defaults and the file is left as it is.", name, allocator = allocator)
	}
	return ""
}

// One copy of the photograph: the first is the photograph itself, the
// others its duplicates. Its strings and edits point into its sidecar's raw.
Variant :: struct {
	id:     string,
	// 0 (unrated) to 5 stars.
	rating: u8,
	flag:   Flag,
	// The stored edits, an object; nil when there are none.
	edits:  Json,
}

// One sidecar file, decoded. `raw` is the whole file, kept so that keys
// this version doesn't know are written back unchanged.
Sidecar :: struct {
	raw:              Json,
	defaults_version: i64,
	variants:         []Variant,
}

sidecar_variant :: proc(s: Sidecar, id: string) -> (Variant, bool) {
	for v in s.variants {
		if v.id == id {
			return v, true
		}
	}
	return {}, false
}

// Checks a file's JSON as `PhotoSidecar(raw:)` does. The variants are made
// in the temporary allocator and point into `raw`.
sidecar_from_raw :: proc(raw: Json) -> (Sidecar, Maybe(Problem)) {
	ta := context.temp_allocator
	top, is_object := raw.(Json_Object)
	if !is_object {
		return {}, unreadable("not a JSON object")
	}
	schema, has_schema := as_integer(json_get(top, "negs"))
	if !has_schema {
		return {}, unreadable("no schema")
	}
	if schema > SCHEMA {
		return {}, Problem{kind = .Newer}
	}
	if schema < 1 {
		return {}, unreadable(fmt.aprintf("schema %d", schema, allocator = ta))
	}
	version, has_version := as_integer(json_get(top, "defaultsVersion"))
	if !has_version || version < 1 {
		return {}, unreadable("no defaults version")
	}
	if version > LATEST_DEFAULTS {
		return {}, Problem{kind = .Newer}
	}
	variants := make([dynamic]Variant, 0, 2, ta)
	entries, _ := as_array(json_get(top, "variants"))
	for entry in entries {
		id, is_string := json_get(entry, "id").(string)
		_, entry_is_object := entry.(Json_Object)
		duplicate := false
		for v in variants {
			if v.id == id {
				duplicate = true
			}
		}
		if !is_string || !entry_is_object || id == "" || duplicate {
			return {}, unreadable("a variant without its own id")
		}
		rating, has_rating := as_integer(json_get(entry, "rating"))
		if !has_rating {
			rating = 0
		}
		if rating < 0 || rating > 5 {
			return {}, unreadable(fmt.aprintf("rating %d", rating, allocator = ta))
		}
		flag := Flag.None
		if name, named := json_get(entry, "flag").(string); named {
			parsed, ok := flag_parse(name)
			if !ok {
				return {}, unreadable(fmt.aprintf("flag “%s”", name, allocator = ta))
			}
			flag = parsed
		}
		edits, has_edits := object_find(entry.(Json_Object), "edits")
		if has_edits {
			if _, ok := edits.(Json_Object); !ok {
				return {}, unreadable("edits that aren’t an object")
			}
		}
		append(&variants, Variant{id, u8(rating), flag, edits})
	}
	return Sidecar{raw, version, variants[:]}, nil
}

// A file's bytes, checked; parsed into the temporary allocator.
sidecar_from_bytes :: proc(bytes: []u8) -> (Sidecar, Maybe(Problem)) {
	if len(bytes) > MAXIMUM_SIZE {
		return {}, Problem{kind = .Too_Large}
	}
	raw, err := json_parse(bytes, context.temp_allocator)
	if err != nil {
		return {}, unreadable("not JSON")
	}
	return sidecar_from_raw(raw)
}

// The sidecar's bytes (temporary), or false if there's none.
sidecar_read_bytes :: proc(photo: string) -> (bytes: []u8, found: bool, problem: Maybe(Problem)) {
	ta := context.temp_allocator
	data, err := fs.read(sidecar_path(photo, ta), MAXIMUM_SIZE + 1, ta)
	if err != 0 {
		if fs.is_not_found(err) {
			return nil, false, nil
		}
		return nil, false, unreadable(fs.error_text(err, ta))
	}
	return data, true, nil
}

// The sidecar's JSON, checked (temporary); nil if there's none.
sidecar_read_raw :: proc(photo: string) -> (Json, Maybe(Problem)) {
	bytes, found, problem := sidecar_read_bytes(photo)
	if problem != nil || !found {
		return nil, problem
	}
	s, p := sidecar_from_bytes(bytes)
	if p != nil {
		return nil, p
	}
	return s.raw, nil
}

// The sidecar (temporary), or false if there's none.
sidecar_read :: proc(photo: string) -> (Sidecar, bool, Maybe(Problem)) {
	bytes, found, problem := sidecar_read_bytes(photo)
	if problem != nil || !found {
		return {}, false, problem
	}
	s, p := sidecar_from_bytes(bytes)
	if p != nil {
		return {}, false, p
	}
	return s, true, nil
}

// Writes `bytes` as the sidecar through a hidden temporary file and a
// rename, or removes the sidecar (`remove`). The bytes are written exactly.
sidecar_write_bytes :: proc(photo: string, bytes: []u8, remove: bool) -> fs.Error {
	ta := context.temp_allocator
	file := sidecar_path(photo, ta)
	if remove {
		err := fs.remove_file(file)
		if err != 0 && fs.is_not_found(err) {
			return 0
		}
		return err
	}
	temporary := temporary_path(photo, ta)
	err := fs.write(temporary, bytes)
	if err == 0 {
		err = fs.rename(temporary, file)
	}
	if err != 0 {
		fs.remove_file(temporary)
	}
	return err
}

// Writes the file's JSON (nil removes it), as `SidecarStore` does.
sidecar_write_raw :: proc(photo: string, raw: Json) -> fs.Error {
	if raw == nil {
		return sidecar_write_bytes(photo, nil, true)
	}
	return sidecar_write_bytes(photo, to_sidecar_bytes(raw, context.temp_allocator), false)
}

// What a writer knows of a stored group (`params`, `crop`, `border`).
Known_Group :: struct {
	group:  string,
	values: []Json,
}

// A change to one variant of a sidecar, applied to the file as it is on
// disk when written, so a rating set by another app meanwhile stays. Only
// the fields it sets change (`SidecarPatch`).
Patch :: struct {
	variant:          string,
	// The file's first variant, whatever its id, and a new one with
	// `variant`'s id only if it has none.
	primary:          bool,
	// The variant's new stored edits (`edits` nil removes them).
	set_edits:        bool,
	edits:            Json,
	// The new rating, 0 to 5.
	set_rating:       bool,
	rating:           i64,
	// The new flag (None unflags).
	set_flag:         bool,
	flag:             Flag,
	// What the writer knows of each stored group, by group name in order:
	// stored values outside them are a newer version's and stay.
	known:            []Known_Group,
	// The whole file instead (`replacement` nil removes it).
	replace:          bool,
	replacement:      Json,
	// The table `edits` is stored against; a new file gets it.
	defaults_version: i64,
	app:              string,
}

patch_new :: proc(variant: string, defaults_version: i64, app: string) -> Patch {
	return Patch{variant = variant, defaults_version = defaults_version, app = app}
}

@(private = "file")
patch_adds :: proc(p: ^Patch) -> bool {
	return (p.set_edits && p.edits != nil) || (p.set_rating && p.rating > 0) || (p.set_flag && p.flag != .None)
}

// The file after this change (temporary), or nil to remove it: a single
// variant with nothing in it and nothing else in the file means the
// photograph has no rating, flag or edits left. A newer or unreadable file
// is never changed. `raw` is nil when there's no file.
patch_applied :: proc(p: ^Patch, raw: Json) -> (Json, Maybe(Problem)) {
	ta := context.temp_allocator
	if p.replace {
		return json_clone(p.replacement, ta), nil
	}
	top: Json_Object
	if o, ok := raw.(Json_Object); ok {
		top = json_clone(o, ta).(Json_Object)
	} else {
		top = make(Json_Object, 0, 4, ta)
		object_set(&top, "negs", f64(SCHEMA))
		object_set(&top, "defaultsVersion", f64(p.defaults_version))
	}
	if raw != nil {
		// Decoding checks the schema and versions.
		s, problem := sidecar_from_raw(top)
		if problem != nil {
			return nil, problem
		}
		// Ratings and flags don't depend on the table; edits are stored against it.
		if p.set_edits && s.defaults_version != p.defaults_version {
			return nil, unreadable("its defaults version changed while it was open")
		}
	}
	variants: Json_Array
	if a, ok := json_get(top, "variants").(Json_Array); ok {
		variants = json_clone(a, ta).(Json_Array)
	} else {
		variants = make(Json_Array, 0, 1, ta)
	}
	index := -1
	if p.primary && len(variants) > 0 {
		index = 0
	} else {
		for v, i in variants {
			if id, ok := json_get(v, "id").(string); ok && id == p.variant {
				index = i
				break
			}
		}
	}
	if index < 0 {
		if !patch_adds(p) {
			return json_clone(raw, ta), nil
		}
		fresh := make(Json_Object, 0, 1, ta)
		object_set(&fresh, "id", json_string(p.variant, ta))
		append(&variants, fresh)
		index = len(variants) - 1
	}
	object_set(&top, "app", json_string(p.app, ta))
	fields: Json_Object
	if o, ok := variants[index].(Json_Object); ok {
		fields = o
	} else {
		fields = make(Json_Object, 0, 2, ta)
	}
	if p.set_edits {
		previous, _ := json_get(fields, "edits").(Json_Object)
		// Keep what this version doesn't know, then lay what it does over it.
		stored := make(Json_Object, 0, len(previous), ta)
		for field in previous {
			if !slice.contains(EDIT_KEYS[:], field.key) {
				append(&stored, Json_Field{field.key, field.value})
			}
		}
		for k in p.known {
			if old, found := object_find(previous, k.group); found {
				if unknown, has := json_unknown_to(old, k.values, ta); has {
					object_set(&stored, k.group, unknown)
				}
			}
		}
		if values, ok := p.edits.(Json_Object); ok {
			for field in values {
				i, found := object_search(stored, field.key)
				if found {
					stored[i].value = json_overlaid(stored[i].value, field.value, ta)
				} else {
					inject_at(&stored, i, Json_Field{strings.clone(field.key, ta), json_clone(field.value, ta)})
				}
			}
		}
		if len(stored) == 0 {
			object_remove(&fields, "edits")
		} else {
			object_set(&fields, "edits", stored)
		}
	}
	if p.set_rating {
		if p.rating > 0 {
			object_set(&fields, "rating", f64(min(p.rating, 5)))
		} else {
			object_remove(&fields, "rating")
		}
	}
	if p.set_flag {
		if p.flag != .None {
			object_set(&fields, "flag", json_string(flag_name(p.flag), ta))
		} else {
			object_remove(&fields, "flag")
		}
	}
	only_id := len(fields) == 1 && object_has(fields, "id")
	variants[index] = fields
	count := len(variants)
	object_set(&top, "variants", variants)
	if count == 1 && only_id && plain_top(top) {
		return nil, nil
	}
	return top, nil
}

@(private = "file")
plain_top :: proc(top: Json_Object) -> bool {
	for field in top {
		if !slice.contains(PLAIN_TOP_KEYS[:], field.key) {
			return false
		}
	}
	return true
}

// `raw` without the variant `id` (temporary); nil when nothing is left in
// it (`PhotoSidecar.removing`). A newer file is never changed.
removing_variant :: proc(id: string, raw: Json) -> (Json, Maybe(Problem)) {
	ta := context.temp_allocator
	if _, problem := sidecar_from_raw(raw); problem != nil {
		return nil, problem
	}
	top, is_object := raw.(Json_Object)
	variants, is_array := json_get(raw, "variants").(Json_Array)
	if !is_object || !is_array {
		return json_clone(raw, ta), nil
	}
	out := json_clone(top, ta).(Json_Object)
	kept := make(Json_Array, 0, len(variants), ta)
	for v in variants {
		if vid, ok := json_get(v, "id").(string); ok && vid == id {
			continue
		}
		append(&kept, json_clone(v, ta))
	}
	plain := len(kept) == 0
	if len(kept) == 1 {
		if o, ok := kept[0].(Json_Object); ok && len(o) == 1 && o[0].key == "id" {
			plain = true
		}
	}
	object_set(&out, "variants", kept)
	if plain && plain_top(out) {
		return nil, nil
	}
	return out, nil
}

// A frozen defaults table (`data/sidecar-defaults/v<n>.json`): what a
// sidecar's edits are stored against. Owned by its `Tables`.
Defaults_Table :: struct {
	version: i64,
	film:    string,
	paper:   string,
	params:  Json,
	border:  Json,
	crop:    Json,
}

// The table in `json`, copied into `allocator`; the error in the temporary
// allocator.
defaults_table_from_json :: proc(json: Json, version: i64, allocator := context.allocator) -> (^Defaults_Table, string) {
	damaged := fmt.aprintf("The defaults table v%d is damaged. Reinstall Negs.", version, allocator = context.temp_allocator)
	if v, ok := as_integer(json_get(json, "defaultsVersion")); !ok || v != version {
		return nil, damaged
	}
	film, has_film := json_get(json, "film").(string)
	paper, has_paper := json_get(json, "paper").(string)
	if !has_film || !has_paper {
		return nil, damaged
	}
	for key in ([]string{"params", "border", "crop"}) {
		if _, ok := json_get(json, key).(Json_Object); !ok {
			return nil, damaged
		}
	}
	t := new(Defaults_Table, allocator)
	t^ = Defaults_Table {
		version = version,
		film = strings.clone(film, allocator),
		paper = strings.clone(paper, allocator),
		params = json_clone(json_get(json, "params"), allocator),
		border = json_clone(json_get(json, "border"), allocator),
		crop = json_clone(json_get(json, "crop"), allocator),
	}
	return t, ""
}

defaults_table_destroy :: proc(t: ^Defaults_Table, allocator := context.allocator) {
	delete(t.film, allocator)
	delete(t.paper, allocator)
	json_destroy(t.params, allocator)
	json_destroy(t.border, allocator)
	json_destroy(t.crop, allocator)
	free(t, allocator)
}

defaults_table_path :: proc(data: string, version: i64, allocator := context.temp_allocator) -> string {
	folder := join(data, "sidecar-defaults", context.temp_allocator)
	return join(folder, fmt.aprintf("v%d.json", version, allocator = context.temp_allocator), allocator)
}

defaults_table_load :: proc(data: string, version: i64, allocator := context.allocator) -> (^Defaults_Table, string) {
	ta := context.temp_allocator
	bytes, err := fs.read(defaults_table_path(data, version), max(int), ta)
	if err != 0 {
		return nil, fmt.aprintf("The defaults table v%d can’t be read: %s", version, fs.error_text(err, ta), allocator = ta)
	}
	json, parse_err := json_parse(bytes, ta)
	if parse_err != nil {
		return nil, fmt.aprintf("The defaults table v%d is damaged. Reinstall Negs.", version, allocator = ta)
	}
	return defaults_table_from_json(json, version, allocator)
}

// What edits stored against this table know of each group, by group name:
// stored values outside them are a newer version's and stay. `engine` is
// the engine's own defaults, when known (nil otherwise). Temporary.
defaults_table_known :: proc(t: ^Defaults_Table, engine: Json, has_engine: bool) -> []Known_Group {
	ta := context.temp_allocator
	params := make([dynamic]Json, 0, 2, ta)
	append(&params, t.params)
	if has_engine {
		append(&params, engine)
	}
	out := make([]Known_Group, 3, ta)
	out[0] = Known_Group{"border", slice.clone([]Json{t.border}, ta)}
	out[1] = Known_Group{"crop", slice.clone([]Json{t.crop}, ta)}
	out[2] = Known_Group{"params", params[:]}
	return out
}

// The defaults tables of a data folder, each read once.
Tables :: struct {
	data:      string,
	loaded:    map[i64]^Defaults_Table,
	allocator: Allocator,
}

tables_init :: proc(t: ^Tables, data: string, allocator := context.allocator) {
	t.allocator = allocator
	t.data = strings.clone(data, allocator)
	t.loaded = make(map[i64]^Defaults_Table, allocator)
}

tables_destroy :: proc(t: ^Tables) {
	for _, table in t.loaded {
		defaults_table_destroy(table, t.allocator)
	}
	delete(t.loaded)
	delete(t.data, t.allocator)
}

// The table, or why it can't be had (temporary).
tables_get :: proc(t: ^Tables, version: i64) -> (^Defaults_Table, string) {
	if table, ok := t.loaded[version]; ok {
		return table, ""
	}
	table, err := defaults_table_load(t.data, version, t.allocator)
	if table == nil {
		return nil, err
	}
	t.loaded[version] = table
	return table, ""
}

tables_latest :: proc(t: ^Tables) -> (^Defaults_Table, string) {
	return tables_get(t, LATEST_DEFAULTS)
}
