// The library's values as the JSON the C ABI passes (negs_core.h, the
// Library section): encoded with the library's own JSON writer, so numbers
// and strings round-trip exactly, and decoded strictly (a wrong shape is
// refused, never guessed).
package negs_core

import lib "../library"
import "core:strings"

@(private)
Json :: lib.Json
@(private)
Json_Object :: lib.Json_Object
@(private)
Json_Array :: lib.Json_Array

// MARK: Writing

@(private)
obj :: proc() -> Json_Object {
	return make(Json_Object, 0, 8, context.temp_allocator)
}

@(private)
arr :: proc(n := 0) -> Json_Array {
	return make(Json_Array, 0, n, context.temp_allocator)
}

@(private)
put :: proc(o: ^Json_Object, key: string, v: Json) {
	lib.object_set(o, key, v)
}

@(private)
str :: proc(s: string) -> Json {
	return strings.clone(s, context.temp_allocator)
}

@(private)
opt_str :: proc(has: bool, s: string) -> Json {
	return str(s) if has else nil
}

@(private)
num :: proc(v: $T) -> Json {
	return f64(v)
}

// A Json value as a block the host owns.
@(private)
buf_json :: proc(v: Json) -> Buf {
	b := strings.builder_make(0, 256, context.temp_allocator)
	lib.write_compact(&b, v)
	return buf_from(b.buf[:])
}

@(private)
buf_text :: proc(s: string) -> Buf {
	return buf_from(transmute([]u8)s)
}

@(private)
enc_item :: proc(i: lib.Item) -> Json {
	o := obj()
	put(&o, "photo", str(i.photo))
	if i.has_variant {
		put(&o, "variant", str(i.variant))
	}
	return o
}

@(private)
enc_items :: proc(items: []lib.Item) -> Json {
	a := arr(len(items))
	for i in items {
		append(&a, enc_item(i))
	}
	return a
}

@(private)
enc_strings :: proc(list: []string) -> Json {
	a := arr(len(list))
	for s in list {
		append(&a, str(s))
	}
	return a
}

@(private)
enc_flag :: proc(f: lib.Flag) -> Json {
	return nil if f == .None else str(lib.flag_name(f))
}

@(private)
enc_summary :: proc(s: lib.Summary) -> Json {
	o := obj()
	put(&o, "rating", num(s.rating))
	put(&o, "flag", enc_flag(s.flag))
	put(&o, "edited", s.edited)
	put(&o, "problem", opt_str(s.has_problem, s.problem))
	return o
}

@(private)
enc_state :: proc(s: lib.Edit_State) -> Json {
	o := obj()
	put(&o, "film", str(s.film))
	put(&o, "paper", str(s.paper))
	put(&o, "params", s.params)
	put(&o, "crop", s.crop)
	put(&o, "border", s.border)
	if s.has_match {
		m := obj()
		put(&m, "ev", num(s.brightness_match.ev) if s.brightness_match.has_ev else nil)
		put(&m, "highlightLimited", s.brightness_match.highlight_limited)
		put(&m, "solved", s.brightness_match.solved)
		put(&o, "brightnessMatch", m)
	} else {
		put(&o, "brightnessMatch", nil)
	}
	if s.has_roll {
		r := obj()
		put(&r, "id", str(s.roll.id))
		put(&r, "magenta", s.roll.magenta)
		put(&r, "yellow", s.roll.yellow)
		put(&r, "paper", opt_str(s.roll.has_paper, s.roll.paper))
		put(&o, "roll", r)
	} else {
		put(&o, "roll", nil)
	}
	return o
}

@(private)
enc_defaults :: proc(d: lib.Edit_Defaults) -> Json {
	o := obj()
	put(&o, "params", d.params)
	put(&o, "nullable", enc_strings(d.nullable))
	put(&o, "crop", d.crop)
	put(&o, "border", d.border)
	films := obj()
	for f in d.films {
		put(&films, f.name, f.positive)
	}
	put(&o, "films", films)
	put(&o, "papers", enc_strings(d.papers))
	put(&o, "tableFilm", str(d.table_film))
	put(&o, "tablePaper", str(d.table_paper))
	put(&o, "newPhotoFilm", str(d.new_photo_film))
	put(&o, "newPhotoPaper", str(d.new_photo_paper))
	put(&o, "matchNewPhotos", d.match_new_photos)
	return o
}

@(private)
enc_filter :: proc(f: lib.Filter) -> Json {
	o := obj()
	r := obj()
	switch f.rating.kind {
	case .Any:
		put(&r, "kind", str("any"))
	case .At_Least:
		put(&r, "kind", str("atLeast"))
		put(&r, "stars", num(f.rating.stars))
	case .Unrated:
		put(&r, "kind", str("unrated"))
	}
	put(&o, "rating", r)
	flags := arr()
	for flag in lib.Flag_Filter {
		if flag in f.flags {
			append(&flags, str(lib.flag_filter_key(flag)))
		}
	}
	put(&o, "flags", flags)
	edits := "any"
	switch f.edits {
	case .Any:
	case .Edited:
		edits = "edited"
	case .Unedited:
		edits = "unedited"
	}
	put(&o, "edits", str(edits))
	return o
}

@(private)
enc_outcome :: proc(out: lib.Outcome) -> Json {
	o := obj()
	if out.has_note {
		n := obj()
		put(&n, "text", str(out.note.text))
		put(&n, "detail", opt_str(out.note.has_detail, out.note.detail))
		put(&n, "count", num(out.note.count))
		put(&o, "note", n)
	} else {
		put(&o, "note", nil)
	}
	put(&o, "error", opt_str(out.has_error, out.error))
	put(&o, "openChanged", out.open_changed)
	put(&o, "openGone", out.open_gone)
	put(&o, "open", enc_item(out.open) if out.has_open else nil)
	put(&o, "select", enc_items(out.select))
	return o
}

@(private)
enc_plan :: proc(p: lib.Delete_Plan) -> Json {
	o := obj()
	put(&o, "items", enc_items(p.items))
	put(&o, "files", enc_strings(p.files))
	put(&o, "duplicates", enc_items(p.duplicates))
	put(&o, "following", num(p.following))
	companions := arr(len(p.companions))
	for c in p.companions {
		pair := arr(2)
		append(&pair, str(c.file), str(c.picture))
		append(&companions, pair)
	}
	put(&o, "companions", companions)
	renamed := arr(len(p.renamed))
	for r in p.renamed {
		pair := arr(2)
		append(&pair, enc_item(r.from), enc_item(r.to))
		append(&renamed, pair)
	}
	put(&o, "renamed", renamed)
	return o
}

@(private)
enc_groups :: proc(groups: lib.Edit_Groups) -> Json {
	a := arr()
	for g in lib.Edit_Group {
		if g in groups {
			append(&a, str(lib.edit_group_key(g)))
		}
	}
	return a
}

// Nanoseconds since 1970 as [seconds, nanoseconds], the seconds floored.
@(private)
enc_time :: proc(ns: i64) -> Json {
	secs := ns / 1_000_000_000
	rest := ns % 1_000_000_000
	if rest < 0 {
		secs -= 1
		rest += 1_000_000_000
	}
	a := arr(2)
	append(&a, num(secs), num(rest))
	return a
}

@(private)
enc_file :: proc(f: lib.Photo_File) -> Json {
	o := obj()
	put(&o, "path", str(f.path))
	put(&o, "name", str(f.name))
	put(&o, "modified", enc_time(f.modified) if f.has_modified else nil)
	put(&o, "size", num(f.size) if f.has_size else nil)
	return o
}

// MARK: Reading

// The host's JSON, parsed into the temporary allocator; false if it isn't.
@(private)
parse :: proc(data: [^]u8, n: uint) -> (Json, bool) {
	if data == nil || n == 0 {
		return nil, false
	}
	v, err := lib.json_parse(data[:n], context.temp_allocator)
	return v, err == nil
}

@(private)
dec_string :: proc(v: Json) -> (string, bool) {
	s, ok := v.(string)
	return s, ok
}

@(private)
dec_opt_string :: proc(v: Json) -> (s: string, has: bool, ok: bool) {
	if v == nil {
		return "", false, true
	}
	s, ok = v.(string)
	return s, ok, ok
}

@(private)
dec_item :: proc(v: Json) -> (item: lib.Item, ok: bool) {
	o := v.(Json_Object) or_return
	item.photo = lib.json_get(o, "photo").(string) or_return
	variant := lib.json_get(o, "variant")
	if variant != nil {
		item.variant = variant.(string) or_return
		item.has_variant = true
	}
	return item, true
}

@(private)
dec_items :: proc(v: Json) -> (items: []lib.Item, ok: bool) {
	a := v.(Json_Array) or_return
	out := make([]lib.Item, len(a), context.temp_allocator)
	for e, i in a {
		out[i] = dec_item(e) or_return
	}
	return out, true
}

@(private)
dec_strings :: proc(v: Json) -> (list: []string, ok: bool) {
	a := v.(Json_Array) or_return
	out := make([]string, len(a), context.temp_allocator)
	for e, i in a {
		out[i] = e.(string) or_return
	}
	return out, true
}

@(private)
dec_number :: proc(v: Json) -> (f64, bool) {
	n, ok := v.(f64)
	return n, ok
}

@(private)
dec_bool :: proc(v: Json) -> (bool, bool) {
	b, ok := v.(bool)
	return b, ok
}

@(private)
dec_flag :: proc(v: Json) -> (lib.Flag, bool) {
	if v == nil {
		return .None, true
	}
	s, ok := v.(string)
	if !ok {
		return .None, false
	}
	return lib.flag_parse(s)
}

@(private)
dec_summary :: proc(v: Json) -> (s: lib.Summary, ok: bool) {
	o := v.(Json_Object) or_return
	rating := dec_number(lib.json_get(o, "rating")) or_return
	if rating < 0 || rating > 255 || rating != f64(u8(rating)) {
		return
	}
	s.rating = u8(rating)
	s.flag = dec_flag(lib.json_get(o, "flag")) or_return
	s.edited = dec_bool(lib.json_get(o, "edited")) or_return
	s.problem, s.has_problem = dec_opt_string(lib.json_get(o, "problem")) or_return
	return s, true
}

@(private)
dec_state :: proc(v: Json) -> (s: lib.Edit_State, ok: bool) {
	o := v.(Json_Object) or_return
	s.film = dec_string(lib.json_get(o, "film")) or_return
	s.paper = dec_string(lib.json_get(o, "paper")) or_return
	s.params = lib.json_get(o, "params")
	s.crop = lib.json_get(o, "crop")
	s.border = lib.json_get(o, "border")
	if !lib.object_has(o, "params") || !lib.object_has(o, "crop") || !lib.object_has(o, "border") {
		return
	}
	if m := lib.json_get(o, "brightnessMatch"); m != nil {
		mo := m.(Json_Object) or_return
		ev := lib.json_get(mo, "ev")
		if ev != nil {
			s.brightness_match.ev = dec_number(ev) or_return
			s.brightness_match.has_ev = true
		}
		s.brightness_match.highlight_limited = dec_bool(lib.json_get(mo, "highlightLimited")) or_return
		s.brightness_match.solved = dec_bool(lib.json_get(mo, "solved")) or_return
		s.has_match = true
	}
	if r := lib.json_get(o, "roll"); r != nil {
		ro := r.(Json_Object) or_return
		s.roll.id = dec_string(lib.json_get(ro, "id")) or_return
		s.roll.magenta = dec_number(lib.json_get(ro, "magenta")) or_return
		s.roll.yellow = dec_number(lib.json_get(ro, "yellow")) or_return
		s.roll.paper, s.roll.has_paper = dec_opt_string(lib.json_get(ro, "paper")) or_return
		s.has_roll = true
	}
	return s, true
}

@(private)
dec_defaults :: proc(v: Json) -> (d: lib.Edit_Defaults, ok: bool) {
	o := v.(Json_Object) or_return
	d.params = lib.json_get(o, "params")
	d.crop = lib.json_get(o, "crop")
	d.border = lib.json_get(o, "border")
	if !lib.object_has(o, "params") || !lib.object_has(o, "crop") || !lib.object_has(o, "border") {
		return
	}
	d.nullable = dec_strings(lib.json_get(o, "nullable")) or_return
	films := lib.json_get(o, "films").(Json_Object) or_return
	d.films = make([]lib.Film_Entry, len(films), context.temp_allocator)
	for f, i in films {
		d.films[i] = lib.Film_Entry{f.key, dec_bool(f.value) or_return}
	}
	papers := dec_strings(lib.json_get(o, "papers")) or_return
	// A set: sorted, unique.
	d.papers = sorted_unique(papers)
	d.table_film = dec_string(lib.json_get(o, "tableFilm")) or_return
	d.table_paper = dec_string(lib.json_get(o, "tablePaper")) or_return
	d.new_photo_film = dec_string(lib.json_get(o, "newPhotoFilm")) or_return
	d.new_photo_paper = dec_string(lib.json_get(o, "newPhotoPaper")) or_return
	d.match_new_photos = dec_bool(lib.json_get(o, "matchNewPhotos")) or_return
	return d, true
}

@(private)
sorted_unique :: proc(list: []string) -> []string {
	out := make([dynamic]string, 0, len(list), context.temp_allocator)
	append(&out, ..list)
	for i in 1 ..< len(out) {
		for j := i; j > 0 && out[j] < out[j - 1]; j -= 1 {
			out[j], out[j - 1] = out[j - 1], out[j]
		}
	}
	kept := 0
	for s in out {
		if kept == 0 || out[kept - 1] != s {
			out[kept] = s
			kept += 1
		}
	}
	return out[:kept]
}

@(private)
dec_filter :: proc(v: Json) -> (f: lib.Filter, ok: bool) {
	o := v.(Json_Object) or_return
	r := lib.json_get(o, "rating").(Json_Object) or_return
	switch dec_string(lib.json_get(r, "kind")) or_return {
	case "any":
	case "atLeast":
		stars := dec_number(lib.json_get(r, "stars")) or_return
		if stars < 0 || stars > 255 || stars != f64(u8(stars)) {
			return
		}
		f.rating = {.At_Least, u8(stars)}
	case "unrated":
		f.rating = {.Unrated, 0}
	case:
		return
	}
	flags := dec_strings(lib.json_get(o, "flags")) or_return
	for name in flags {
		found := false
		for flag in lib.Flag_Filter {
			if lib.flag_filter_key(flag) == name {
				f.flags += {flag}
				found = true
			}
		}
		if !found {
			return
		}
	}
	switch dec_string(lib.json_get(o, "edits")) or_return {
	case "any":
	case "edited":
		f.edits = .Edited
	case "unedited":
		f.edits = .Unedited
	case:
		return
	}
	return f, true
}

@(private)
dec_groups :: proc(v: Json) -> (groups: lib.Edit_Groups, ok: bool) {
	names := dec_strings(v) or_return
	for name in names {
		g := lib.edit_group_from_key(name) or_return
		groups += {g}
	}
	return groups, true
}

@(private)
dec_count :: proc(v: Json) -> (int, bool) {
	n, ok := v.(f64)
	if !ok || n < 0 || n != f64(int(n)) {
		return 0, false
	}
	return int(n), true
}

@(private)
dec_plan :: proc(v: Json) -> (p: lib.Delete_Plan, ok: bool) {
	o := v.(Json_Object) or_return
	p.items = dec_items(lib.json_get(o, "items")) or_return
	p.files = dec_strings(lib.json_get(o, "files")) or_return
	p.duplicates = dec_items(lib.json_get(o, "duplicates")) or_return
	p.following = dec_count(lib.json_get(o, "following")) or_return
	companions := lib.json_get(o, "companions").(Json_Array) or_return
	p.companions = make([]lib.Companion, len(companions), context.temp_allocator)
	for c, i in companions {
		pair := c.(Json_Array) or_return
		if len(pair) != 2 {
			return
		}
		p.companions[i] = lib.Companion{dec_string(pair[0]) or_return, dec_string(pair[1]) or_return}
	}
	renamed := lib.json_get(o, "renamed").(Json_Array) or_return
	p.renamed = make([]lib.Rename, len(renamed), context.temp_allocator)
	for r, i in renamed {
		pair := r.(Json_Array) or_return
		if len(pair) != 2 {
			return
		}
		p.renamed[i] = lib.Rename{dec_item(pair[0]) or_return, dec_item(pair[1]) or_return}
	}
	return p, true
}
