// A photograph's edits as the app holds them, and as a sidecar stores
// them: the film and paper always, everything else as what differs from
// the defaults table (`PhotoSidecar.encodeEdits/decodeEdits`, `EditGroup`,
// `EditorModel.userEdits`, `newPhotoState`, `restoredState`; edits.rs).
//
// The engine's parameters stay JSON: only the defaults they are measured
// from matter here (`Edit_Defaults`).
package library

import "core:fmt"
import "core:math"
import "core:slice"
import "core:strings"

// Prep › Match camera brightness, on.
Brightness_Match :: struct {
	// The solved exposure, once solved (`has_ev`).
	has_ev:            bool,
	ev:                f64,
	// The solve held exposure back for the highlights.
	highlight_limited: bool,
	// `ev` was solved for these very edits.
	solved:            bool,
}

// A scanned negative printed as a frame of a darkroom roll.
Roll :: struct {
	id:        string,
	magenta:   f64,
	yellow:    f64,
	has_paper: bool,
	paper:     string,
}

// Everything a photograph shows. `crop` and `border` are the defaults
// table's shapes, whole; `params` is the engine's parameters, whole.
Edit_State :: struct {
	film:             string,
	paper:            string,
	params:           Json,
	crop:             Json,
	border:           Json,
	has_match:        bool,
	brightness_match: Brightness_Match,
	has_roll:         bool,
	roll:             Roll,
}

// A photograph's edits as read from its sidecar.
Saved_Edits :: struct {
	// The defaults table with the stored changes laid over it.
	state:        Edit_State,
	// The stored brightness match was solved for these edits.
	match_solved: bool,
}

edit_state_clone :: proc(s: Edit_State, allocator := context.allocator) -> Edit_State {
	out := s
	out.film = strings.clone(s.film, allocator)
	out.paper = strings.clone(s.paper, allocator)
	out.params = json_clone(s.params, allocator)
	out.crop = json_clone(s.crop, allocator)
	out.border = json_clone(s.border, allocator)
	out.roll.id = strings.clone(s.roll.id, allocator)
	out.roll.paper = strings.clone(s.roll.paper, allocator)
	return out
}

edit_state_destroy :: proc(s: Edit_State, allocator := context.allocator) {
	delete(s.film, allocator)
	delete(s.paper, allocator)
	json_destroy(s.params, allocator)
	json_destroy(s.crop, allocator)
	json_destroy(s.border, allocator)
	delete(s.roll.id, allocator)
	delete(s.roll.paper, allocator)
}

// Rust's derived PartialEq.
edit_state_equal :: proc(a, b: Edit_State) -> bool {
	if a.film != b.film || a.paper != b.paper || a.has_match != b.has_match || a.has_roll != b.has_roll {
		return false
	}
	if !json_equal(a.params, b.params) || !json_equal(a.crop, b.crop) || !json_equal(a.border, b.border) {
		return false
	}
	if a.has_match {
		x, y := a.brightness_match, b.brightness_match
		if x.has_ev != y.has_ev || (x.has_ev && x.ev != y.ev) || x.highlight_limited != y.highlight_limited || x.solved != y.solved {
			return false
		}
	}
	if a.has_roll {
		x, y := a.roll, b.roll
		if x.id != y.id || x.magenta != y.magenta || x.yellow != y.yellow || x.has_paper != y.has_paper || (x.has_paper && x.paper != y.paper) {
			return false
		}
	}
	return true
}

// The defaults table as it stands: what an edit is measured from.
edit_state_from_table :: proc(t: ^Defaults_Table, allocator := context.allocator) -> Edit_State {
	return Edit_State {
		film = strings.clone(t.film, allocator),
		paper = strings.clone(t.paper, allocator),
		params = json_clone(t.params, allocator),
		crop = json_clone(t.crop, allocator),
		border = json_clone(t.border, allocator),
	}
}

// No parameters at all (Swift's `EditState()`).
edit_state_empty :: proc(allocator := context.allocator) -> Edit_State {
	return Edit_State {
		film = strings.clone("kodak_gold_200", allocator),
		paper = strings.clone("kodak_portra_endura", allocator),
		params = json_object(allocator),
		crop = json_object(allocator),
		border = json_object(allocator),
	}
}

// The user's own edits: without the brightness match's solved exposure,
// which isn't one (`EditorModel.userEdits`).
user_edits :: proc(s: Edit_State, allocator := context.allocator) -> Edit_State {
	edits := edit_state_clone(s, allocator)
	if edits.has_match {
		found := edits.brightness_match
		exposure, _ := as_f64(json_at(edits.params, "prep.exposure_ev"))
		offset := exposure - (found.ev if found.has_ev else 0)
		json_set_at(&edits.params, "prep.exposure_ev", round_half_away(offset * 1e9) / 1e9, allocator)
		edits.brightness_match = {}
	}
	return edits
}

@(private = "file")
CROP_ASPECTS := [9]string{"original", "freeform", "square", "16:9", "3:2", "4:3", "5:4", "7:5", "custom"}
@(private = "file")
BORDER_STYLES := [4]string{"white", "film", "keyline", "35mm"}
// `CropSettings.maxAngle`.
MAX_CROP_ANGLE :: 45.0

@(private = "file")
finite :: proc(v: Json) -> (f64, bool) {
	n, ok := v.(f64)
	if !ok || math.is_nan(n) || math.is_inf(n) {
		return 0, false
	}
	return n, true
}

// A point or size as `CGPoint`/`CGSize` decode it: the first two numbers.
@(private = "file")
pair :: proc(v: Json) -> (out: [2]f64, ok: bool) {
	items := v.(Json_Array) or_return
	if len(items) < 2 {
		return
	}
	out[0] = items[0].(f64) or_return
	out[1] = items[1].(f64) or_return
	return out, true
}

@(private = "file")
pair_json :: proc(p: [2]f64, allocator: Allocator) -> Json {
	a := make(Json_Array, 2, 2, allocator)
	a[0] = p[0]
	a[1] = p[1]
	return a
}

// The crop as `CropSettings` decodes it and encodes it again: every field
// there and of its type, others dropped.
@(private = "file")
decode_crop :: proc(crop: Json, allocator: Allocator) -> (result: Json, ok: bool) {
	rect := json_get(crop, "rect").(Json_Array) or_return
	if len(rect) < 2 {
		return nil, false
	}
	origin := pair(rect[0]) or_return
	size := pair(rect[1]) or_return
	aspect := json_get(crop, "aspect").(string) or_return
	if !slice.contains(CROP_ASPECTS[:], aspect) {
		return nil, false
	}
	custom := pair(json_get(crop, "custom")) or_return
	angle := json_get(crop, "angle").(f64) or_return
	r := make(Json_Array, 2, 2, allocator)
	r[0] = pair_json(origin, allocator)
	r[1] = pair_json(size, allocator)
	out := make(Json_Object, 0, 4, allocator)
	object_set(&out, "angle", angle)
	object_set(&out, "aspect", json_string(aspect, allocator))
	object_set(&out, "custom", pair_json(custom, allocator))
	object_set(&out, "rect", r)
	return out, true
}

@(private = "file")
is_hex :: proc(c: u8) -> bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
}

// A `#rrggbb` colour in lower case, as `BorderColor` writes it.
@(private = "file")
decode_color :: proc(v: Json, allocator: Allocator) -> (result: Json, ok: bool) {
	text := v.(string) or_return
	hex := strings.trim_prefix(text, "#")
	if len(hex) != 6 {
		return nil, false
	}
	for i in 0 ..< 6 {
		if !is_hex(hex[i]) {
			return nil, false
		}
	}
	lower := make([]u8, 7, allocator)
	lower[0] = '#'
	for i in 0 ..< 6 {
		c := hex[i]
		lower[i + 1] = c + 32 if c >= 'A' && c <= 'F' else c
	}
	return string(lower), true
}

@(private = "file")
field_or :: proc(border, defaults: Json, key: string) -> Json {
	if o, ok := border.(Json_Object); ok {
		if v, found := object_find(o, key); found {
			return v
		}
	}
	return json_get(defaults, key)
}

// The border as `BorderSettings` decodes it: a field that's there must be
// of its type; a missing one takes `defaults`' value.
@(private = "file")
decode_border :: proc(border, defaults: Json, allocator: Allocator) -> (result: Json, ok: bool) {
	out := make(Json_Object, 0, 9, allocator)
	for key in ([]string{"enabled", "export", "instagram"}) {
		b := field_or(border, defaults, key).(bool) or_return
		object_set(&out, key, b)
	}
	for key in ([]string{"width", "filmWidth", "line"}) {
		n := field_or(border, defaults, key).(f64) or_return
		object_set(&out, key, n)
	}
	style := field_or(border, defaults, "style")
	name := style.(string) or_return
	if !slice.contains(BORDER_STYLES[:], name) {
		return nil, false
	}
	object_set(&out, "style", json_clone(style, allocator))
	for key in ([]string{"color", "lineColor"}) {
		color := decode_color(field_or(border, defaults, key), allocator) or_return
		object_set(&out, key, color)
	}
	return out, true
}

// Values a saved edit can't have: a crop outside the photograph, a
// rotation that isn't a quarter turn, a flip that isn't yes or no
// (`EditState.validate`).
validate :: proc(s: Edit_State) -> bool {
	rect := json_get(s.crop, "rect")
	origin, has_origin := pair(json_at(rect, "0"))
	size, has_size := pair(json_at(rect, "1"))
	if !has_origin || !has_size {
		return false
	}
	custom := pair(json_get(s.crop, "custom")) or_return
	angle := finite(json_get(s.crop, "angle")) or_return
	x, y, w, h := origin[0], origin[1], size[0], size[1]
	cw, ch := custom[0], custom[1]
	for v in ([]f64{x, y, w, h, cw, ch}) {
		if math.is_nan(v) || math.is_inf(v) {
			return false
		}
	}
	rotate, _ := as_f64(json_at(s.params, "io.rotate_cw"))
	flip := json_at(s.params, "io.flip")
	_, params_object := s.params.(Json_Object)
	_, flip_bool := flip.(bool)
	return(
		x >= 0 &&
		y >= 0 &&
		w > 0 &&
		h > 0 &&
		x + w <= 1.000001 &&
		y + h <= 1.000001 &&
		abs(angle) <= MAX_CROP_ANGLE &&
		cw > 0 &&
		ch > 0 &&
		params_object &&
		rotate >= 0 &&
		rotate <= 3 &&
		round_half_away(rotate) == rotate &&
		(flip == nil || flip_bool) \
	)
}

// The stored form of `state`: the film and paper always, and only what
// differs from `table` otherwise (`PhotoSidecar.encodeEdits`).
encode_edits :: proc(s: Edit_State, match_solved: bool, t: ^Defaults_Table, allocator := context.allocator) -> Json {
	edits := make(Json_Object, 0, 8, allocator)
	object_set(&edits, "film", json_string(s.film, allocator))
	object_set(&edits, "paper", json_string(s.paper, allocator))
	if params, ok := json_changes_from(s.params, t.params, allocator); ok {
		object_set(&edits, "params", params)
	}
	if crop, ok := json_changes_from(s.crop, t.crop, allocator); ok {
		object_set(&edits, "crop", crop)
	}
	if border, ok := json_changes_from(s.border, t.border, allocator); ok {
		object_set(&edits, "border", border)
	}
	if s.has_match {
		found := s.brightness_match
		stored := make(Json_Object, 0, 3, allocator)
		object_set(&stored, "highlightLimited", found.highlight_limited)
		if found.has_ev {
			object_set(&stored, "ev", found.ev)
			if !match_solved {
				object_set(&stored, "solved", false)
			}
		}
		object_set(&edits, "brightnessMatch", stored)
	}
	if s.has_roll {
		stored := make(Json_Object, 0, 4, allocator)
		object_set(&stored, "id", json_string(s.roll.id, allocator))
		object_set(&stored, "magenta", s.roll.magenta)
		object_set(&stored, "yellow", s.roll.yellow)
		if s.roll.has_paper {
			object_set(&stored, "paper", json_string(s.roll.paper, allocator))
		}
		object_set(&edits, "roll", stored)
	}
	return edits
}

// Stored edits laid over `table` and checked (`PhotoSidecar.decodeEdits`),
// in the temporary allocator.
decode_edits :: proc(edits: Json, t: ^Defaults_Table) -> (Saved_Edits, Maybe(Problem)) {
	ta := context.temp_allocator
	fields, is_object := edits.(Json_Object)
	if !is_object {
		return {}, unreadable("edits that aren’t an object")
	}
	name :: proc(fields: Json_Object, key, fallback: string) -> (string, Maybe(Problem)) {
		v, found := object_find(fields, key)
		if !found {
			return fallback, nil
		}
		if text, ok := v.(string); ok && text != "" {
			return text, nil
		}
		return "", unreadable(fmt.aprintf("%s isn’t a name", key, allocator = context.temp_allocator))
	}
	empty := json_object(ta)
	group :: proc(fields: Json_Object, key: string, empty: Json) -> Json {
		v, found := object_find(fields, key)
		return v if found else empty
	}
	film, film_problem := name(fields, "film", t.film)
	if film_problem != nil {
		return {}, film_problem
	}
	paper, paper_problem := name(fields, "paper", t.paper)
	if paper_problem != nil {
		return {}, paper_problem
	}
	params := json_overlaid(t.params, group(fields, "params", empty), ta)
	crop, crop_ok := decode_crop(json_overlaid(t.crop, group(fields, "crop", empty), ta), ta)
	if !crop_ok {
		return {}, unreadable("crop")
	}
	border, border_ok := decode_border(json_overlaid(t.border, group(fields, "border", empty), ta), t.border, ta)
	if !border_ok {
		return {}, unreadable("border")
	}
	state := Edit_State {
		film   = strings.clone(film, ta),
		paper  = strings.clone(paper, ta),
		params = params,
		crop   = crop,
		border = border,
	}
	solved := false
	if stored, found := object_find(fields, "brightnessMatch"); found {
		m, ok := stored.(Json_Object)
		if !ok {
			return {}, unreadable("brightness match")
		}
		bm := Brightness_Match{}
		if value, has := object_find(m, "ev"); has {
			n, is_number := value.(f64)
			if !is_number || math.is_nan(n) || math.is_inf(n) || n < -3 || n > 3 {
				return {}, unreadable("brightness match")
			}
			bm.has_ev = true
			bm.ev = n
		}
		solved_value, has_solved := object_find(m, "solved")
		said_unsolved := false
		if has_solved {
			if b, is_bool := solved_value.(bool); is_bool && !b {
				said_unsolved = true
			}
		}
		solved = bm.has_ev && !said_unsolved
		limited, _ := as_bool(json_get(m, "highlightLimited"))
		bm.highlight_limited = limited
		bm.solved = solved
		state.has_match = true
		state.brightness_match = bm
	}
	if stored, found := object_find(fields, "roll"); found {
		id, has_id := json_get(stored, "id").(string)
		if !has_id || id == "" {
			return {}, unreadable("roll")
		}
		magenta, has_magenta := finite(json_get(stored, "magenta"))
		if !has_magenta {
			return {}, unreadable("roll")
		}
		yellow, has_yellow := finite(json_get(stored, "yellow"))
		if !has_yellow {
			return {}, unreadable("roll")
		}
		roll := Roll{id = strings.clone(id, ta), magenta = magenta, yellow = yellow}
		#partial switch p in json_get(stored, "paper") {
		case nil:
		case string:
			if p == "" {
				return {}, unreadable("roll")
			}
			roll.has_paper = true
			roll.paper = strings.clone(p, ta)
		case:
			return {}, unreadable("roll")
		}
		state.has_roll = true
		state.roll = roll
	}
	if !validate(state) {
		return {}, unreadable("edits out of range")
	}
	return Saved_Edits{state, solved}, nil
}

// The parts of a photograph's edits Copy Edits… offers (`EditGroup`), in
// Rust's order.
Edit_Group :: enum u8 {
	Film_And_Paper,
	Prep,
	Lab,
	Border,
	Output,
	Crop_And_Rotation,
	File,
}

Edit_Groups :: bit_set[Edit_Group]

// What a plain Copy Edits copies.
EDIT_GROUPS_STANDARD :: Edit_Groups{.Film_And_Paper, .Prep, .Lab, .Border, .Output}

// The tick box's words in Copy Edits….
edit_group_title :: proc(g: Edit_Group) -> string {
	switch g {
	case .Film_And_Paper:
		return "Film and paper"
	case .Prep:
		return "Prep (white balance, exposure, Match camera brightness)"
	case .Lab:
		return "Lab"
	case .Border:
		return "Border"
	case .Output:
		return "Image size and output"
	case .Crop_And_Rotation:
		return "Crop, rotation and flip"
	case .File:
		return "Lens correction and input interpretation"
	}
	return ""
}

// The name macOS saves the choice under (`copyEditGroups`).
edit_group_key :: proc(g: Edit_Group) -> string {
	switch g {
	case .Film_And_Paper:
		return "filmAndPaper"
	case .Prep:
		return "prep"
	case .Lab:
		return "lab"
	case .Border:
		return "border"
	case .Output:
		return "output"
	case .Crop_And_Rotation:
		return "cropAndRotation"
	case .File:
		return "file"
	}
	return ""
}

edit_group_from_key :: proc(key: string) -> (Edit_Group, bool) {
	for g in Edit_Group {
		if edit_group_key(g) == key {
			return g, true
		}
	}
	return .Film_And_Paper, false
}

// The group of a parameter.
edit_group_of :: proc(section, key: string) -> Edit_Group {
	switch section {
	case "io":
		switch key {
		case "scan_film":
			return .Film_And_Paper
		case "raw_white_balance":
			return .Prep
		case "output_gamut_compress", "input_gamut_compress":
			return .Lab
		case "photo_long_side_px", "upscale_factor", "output_color_space", "output_cctf_encoding":
			return .Output
		case "rotate_cw", "flip", "crop", "crop_center", "crop_size":
			return .Crop_And_Rotation
		}
		return .File
	case "prep":
		return .Prep
	case "workflow":
		return .Film_And_Paper
	}
	return .Lab
}

// `target` with `groups` taken from `source` (`EditGroup.merge`), a new
// state in `allocator`.
edit_group_merge :: proc(groups: Edit_Groups, source, target: Edit_State, allocator := context.allocator) -> Edit_State {
	merged := edit_state_clone(target, allocator)
	if .Film_And_Paper in groups {
		delete(merged.film, allocator)
		delete(merged.paper, allocator)
		delete(merged.roll.id, allocator)
		delete(merged.roll.paper, allocator)
		merged.film = strings.clone(source.film, allocator)
		merged.paper = strings.clone(source.paper, allocator)
		merged.has_roll = source.has_roll
		merged.roll = source.roll
		merged.roll.id = strings.clone(source.roll.id, allocator)
		merged.roll.paper = strings.clone(source.roll.paper, allocator)
	}
	if .Border in groups {
		json_destroy(merged.border, allocator)
		merged.border = json_clone(source.border, allocator)
	}
	if .Crop_And_Rotation in groups {
		json_destroy(merged.crop, allocator)
		merged.crop = json_clone(source.crop, allocator)
	}
	if .Prep in groups {
		merged.has_match = source.has_match
		merged.brightness_match = {}
	}
	sections, ok := source.params.(Json_Object)
	if !ok {
		return merged
	}
	for field in sections {
		values, is_object := field.value.(Json_Object)
		if is_object && (field.key == "io" || field.key == "prep") {
			for inner in values {
				if edit_group_of(field.key, inner.key) in groups {
					path := strings.concatenate({field.key, ".", inner.key}, context.temp_allocator)
					json_set_at(&merged.params, path, json_clone(inner.value, allocator), allocator)
				}
			}
		} else if edit_group_of(field.key, "") in groups {
			json_set_at(&merged.params, field.key, json_clone(field.value, allocator), allocator)
		}
	}
	return merged
}

IMAGE_SIZE_PATH :: "io.photo_long_side_px"
IMAGE_SIZE_FACTOR_PATH :: "io.upscale_factor"
LENS_CORRECTION_PATH :: "io.lens_correction.enabled"

Film_Entry :: struct {
	name:     string,
	positive: bool,
}

// What working out a photograph's edits needs from the engine's catalog
// and the app's settings (`EditDefaults`). Owned by whoever holds it.
Edit_Defaults :: struct {
	params:           Json,
	nullable:         []string,
	crop:             Json,
	border:           Json,
	// Sorted by name, unique. Empty while the catalog isn't known.
	films:            []Film_Entry,
	// Sorted, unique.
	papers:           []string,
	table_film:       string,
	table_paper:      string,
	new_photo_film:   string,
	new_photo_paper:  string,
	match_new_photos: bool,
}

edit_defaults_for_table :: proc(t: ^Defaults_Table, allocator := context.allocator) -> Edit_Defaults {
	return Edit_Defaults {
		params = json_clone(t.params, allocator),
		nullable = make([]string, 0, allocator),
		crop = json_clone(t.crop, allocator),
		border = json_clone(t.border, allocator),
		films = make([]Film_Entry, 0, allocator),
		papers = make([]string, 0, allocator),
		table_film = strings.clone(t.film, allocator),
		table_paper = strings.clone(t.paper, allocator),
		new_photo_film = strings.clone(t.film, allocator),
		new_photo_paper = strings.clone(t.paper, allocator),
	}
}

edit_defaults_clone :: proc(d: Edit_Defaults, allocator := context.allocator) -> Edit_Defaults {
	out := d
	out.params = json_clone(d.params, allocator)
	out.nullable = make([]string, len(d.nullable), allocator)
	for s, i in d.nullable {
		out.nullable[i] = strings.clone(s, allocator)
	}
	out.crop = json_clone(d.crop, allocator)
	out.border = json_clone(d.border, allocator)
	out.films = make([]Film_Entry, len(d.films), allocator)
	for f, i in d.films {
		out.films[i] = Film_Entry{strings.clone(f.name, allocator), f.positive}
	}
	out.papers = make([]string, len(d.papers), allocator)
	for s, i in d.papers {
		out.papers[i] = strings.clone(s, allocator)
	}
	out.table_film = strings.clone(d.table_film, allocator)
	out.table_paper = strings.clone(d.table_paper, allocator)
	out.new_photo_film = strings.clone(d.new_photo_film, allocator)
	out.new_photo_paper = strings.clone(d.new_photo_paper, allocator)
	return out
}

edit_defaults_destroy :: proc(d: Edit_Defaults, allocator := context.allocator) {
	json_destroy(d.params, allocator)
	for s in d.nullable {
		delete(s, allocator)
	}
	delete(d.nullable, allocator)
	json_destroy(d.crop, allocator)
	json_destroy(d.border, allocator)
	for f in d.films {
		delete(f.name, allocator)
	}
	delete(d.films, allocator)
	for s in d.papers {
		delete(s, allocator)
	}
	delete(d.papers, allocator)
	delete(d.table_film, allocator)
	delete(d.table_paper, allocator)
	delete(d.new_photo_film, allocator)
	delete(d.new_photo_paper, allocator)
}

// Whether the catalog has `film`, and whether it is a positive.
film_lookup :: proc(d: Edit_Defaults, film: string) -> (positive: bool, found: bool) {
	i, ok := slice.binary_search_by(d.films, film, proc(e: Film_Entry, key: string) -> slice.Ordering {
		return slice.Ordering(strings.compare(e.name, key))
	})
	if !ok {
		return false, false
	}
	return d.films[i].positive, true
}

paper_known :: proc(d: Edit_Defaults, paper: string) -> bool {
	_, ok := slice.binary_search(d.papers, paper)
	return ok
}

// The state a photograph without saved edits opens with
// (`EditorModel.newPhotoState`), in `allocator`.
new_photo_state :: proc(d: Edit_Defaults, previous: Edit_State, allocator := context.allocator) -> Edit_State {
	retained := [3]struct {
		path:  string,
		value: Json,
	}{{IMAGE_SIZE_PATH, nil}, {"io.output_color_space", nil}, {"io.output_cctf_encoding", nil}}
	for &r in retained {
		r.value = json_at(previous.params, r.path)
	}
	next := Edit_State {
		film   = strings.clone(d.new_photo_film, allocator),
		paper  = strings.clone(d.new_photo_paper, allocator),
		params = json_clone(d.params, allocator),
	}
	for r in retained {
		if r.value != nil {
			json_set_at(&next.params, r.path, json_clone(r.value, allocator), allocator)
		}
	}
	positive, _ := film_lookup(d, next.film)
	json_set_at(&next.params, "io.scan_film", positive, allocator)
	next.border = json_clone(d.border, allocator)
	json_set_at(&next.params, "io.rotate_cw", f64(0), allocator)
	json_set_at(&next.params, "io.flip", false, allocator)
	interpretation := json_at(d.params, "io.input_interpretation")
	if interpretation != nil {
		json_set_at(&next.params, "io.input_interpretation", json_clone(interpretation, allocator), allocator)
	}
	json_set_at(&next.params, LENS_CORRECTION_PATH, json_clone(json_at(d.params, LENS_CORRECTION_PATH), allocator), allocator)
	next.crop = json_clone(d.crop, allocator)
	next.has_match = d.match_new_photos
	return next
}

// A photograph without edits, not following any other (`plainState`).
plain_state :: proc(d: Edit_Defaults, allocator := context.allocator) -> Edit_State {
	return new_photo_state(d, edit_state_empty(context.temp_allocator), allocator)
}

// Saved edits under the engine's current defaults
// (`EditorModel.restoredState`).
restored_state :: proc(d: Edit_Defaults, saved: Edit_State, allocator := context.allocator) -> Edit_State {
	restored := edit_state_clone(saved, allocator)
	json_destroy(restored.params, allocator)
	restored.params = json_restoring(d.params, saved.params, d.nullable, allocator)
	// Older edits may carry the old working-resolution factor.
	factor := json_at(d.params, IMAGE_SIZE_FACTOR_PATH)
	if factor != nil {
		json_set_at(&restored.params, IMAGE_SIZE_FACTOR_PATH, json_clone(factor, allocator), allocator)
	}
	// The inspector no longer edits Post tone or colour.
	post := json_get(d.params, "post")
	if post != nil {
		json_set_at(&restored.params, "post", json_clone(post, allocator), allocator)
	}
	return restored
}

// After pasting `groups` from a photograph without edits: whether `pasted`
// has none either (`holdsNoEdits`).
holds_no_edits :: proc(d: Edit_Defaults, pasted: Edit_State, groups: Edit_Groups) -> bool {
	if .Film_And_Paper not_in groups {
		return false
	}
	ta := context.temp_allocator
	merged := edit_group_merge(groups, pasted, plain_state(d, ta), ta)
	return edit_state_equal(user_edits(pasted, ta), user_edits(merged, ta))
}
