// session.rs's Preferences, like the macOS app's UserDefaults: one JSON
// object in `settings.json`, as its serde derive reads and writes it:
//
// - `#[serde(default, rename_all = "camelCase")]` with a few renames; a
//   missing field takes `Preferences::default()`'s value, except appTheme,
//   whose own default is "darkroom".
// - `#[serde(flatten)] library: LibraryPreferences` (model/library_map.rs)
//   makes serde read the whole struct as a map: an array is refused, every
//   value of a key the struct doesn't name is buffered (so it must parse
//   in full, not as IgnoredAny skips it), and LibraryPreferences then reads
//   its fields from the buffer as `Content` (a bool must be a bool there,
//   null is none for an Option, a field twice is an error).
// - Written (`to_vec_pretty`) in declaration order, LibraryPreferences'
//   fields where it sits, its `sidecars` left out when none, doubles that
//   aren't finite as null.
package session

import "core:strings"
import "../../rules/sheet"
import "../../value"

Opt_String :: struct {
	has:   bool,
	value: string,
}

Opt_Strings :: struct {
	has:   bool,
	value: []string,
}

Opt_U32 :: struct {
	has:   bool,
	value: u32,
}

Opt_Bool :: struct {
	has:   bool,
	value: bool,
}

Opt_Ints :: struct {
	has:   bool,
	value: [5]i32,
}

// model/library_map.rs's LibraryPreferences.
Library_Preferences :: struct {
	// Some only when not null (an Option<Value> from JSON is never
	// Some(null)).
	library_filter:        value.Value,
	copy_edit_groups:      Opt_Strings,
	advance_after_rating:  bool,
	new_photo_film:        Opt_String,
	new_photo_paper:       Opt_String,
	separate_camera_jpegs: bool,
	sidecars:              Opt_Bool,
}

LIBRARY_FIELDS := [?]string {
	"libraryFilter",
	"copyEditGroups",
	"advanceAfterRating",
	"newPhotoFilm",
	"newPhotoPaper",
	"separateCameraJPEGs",
	"sidecars",
}

Preferences :: struct {
	recent_files:             []string,
	filmstrip_visible:        bool,
	grid_size:                f64,
	expanded_adjustments:     string,
	inspector_tab:            string,
	inspector_tab_chosen:     bool,
	inspector_pane:           string,
	export_engine:            string,
	output_format:            string,
	export_edge:              string,
	export_edge_pixels:       u32,
	color_space:              string,
	png_bit_depth:            u32,
	tiff_bit_depth:           u32,
	jpeg_quality:             u32,
	last_save_folder:         Opt_String,
	window:                   Opt_Ints,
	window_pixels:            Opt_Ints,
	inspector_width:          f64,
	check_for_updates:        bool,
	image_size_long_side:     Opt_U32,
	previews:                 bool,
	keep_paper:               bool,
	tone_graph_channel:       string,
	fresh_edits:              bool,
	match_new_photos:         bool,
	limit_preview:            Opt_Bool,
	low_memory_export:        bool,
	lightroom_cloud:          Opt_String,
	lightroom_classic:        Opt_String,
	lab_all_controls:         bool,
	more_adjustments:         string,
	scope_mode:               string,
	scope_height:             f64,
	favorite_films:           []string,
	favorite_papers:          []string,
	viewer_background_custom: bool,
	viewer_background:        string,
	viewer_sheet:             string,
	app_theme:                string,
	library:                  Library_Preferences,
	fotufilm_films:           string,
	show_negs_built_films:    bool,
	filter_shift_link:        string,
	original_mode:            string,
	original_reference:       string,
	settings_tab:             string,
	batch_naming:             string,
	batch_custom_name:        string,
	batch_existing:           string,
}

@(private)
Kind :: enum u8 {
	Bool,
	F64,
	U32,
	String,
	Strings,
	Opt_String,
	Opt_U32,
	Opt_Bool,
	Opt_Ints5,
	Opt_Ints3,
	// Where LibraryPreferences' fields are written.
	Flatten,
}

@(private)
Field :: struct {
	name:   string,
	kind:   Kind,
	offset: uintptr,
}

// The fields in declaration order, under their serde names.
@(private)
FIELDS := [?]Field {
	{"recentFiles", .Strings, offset_of(Preferences, recent_files)},
	{"filmstripVisible", .Bool, offset_of(Preferences, filmstrip_visible)},
	{"gridSize", .F64, offset_of(Preferences, grid_size)},
	{"expandedAdjustments", .String, offset_of(Preferences, expanded_adjustments)},
	{"inspectorTab", .String, offset_of(Preferences, inspector_tab)},
	{"inspectorTabChosen", .Bool, offset_of(Preferences, inspector_tab_chosen)},
	{"inspectorPane", .String, offset_of(Preferences, inspector_pane)},
	{"exportEngine", .String, offset_of(Preferences, export_engine)},
	{"outputFormat", .String, offset_of(Preferences, output_format)},
	{"exportEdge", .String, offset_of(Preferences, export_edge)},
	{"exportEdgePixels", .U32, offset_of(Preferences, export_edge_pixels)},
	{"colorSpace", .String, offset_of(Preferences, color_space)},
	{"pngBitDepth", .U32, offset_of(Preferences, png_bit_depth)},
	{"tiffBitDepth", .U32, offset_of(Preferences, tiff_bit_depth)},
	{"jpegQuality", .U32, offset_of(Preferences, jpeg_quality)},
	{"lastSaveFolder", .Opt_String, offset_of(Preferences, last_save_folder)},
	{"window", .Opt_Ints5, offset_of(Preferences, window)},
	{"windowPixels", .Opt_Ints3, offset_of(Preferences, window_pixels)},
	{"inspectorWidth", .F64, offset_of(Preferences, inspector_width)},
	{"checkForUpdates", .Bool, offset_of(Preferences, check_for_updates)},
	{"imageSizeLongSide", .Opt_U32, offset_of(Preferences, image_size_long_side)},
	{"previews", .Bool, offset_of(Preferences, previews)},
	{"keep_paper", .Bool, offset_of(Preferences, keep_paper)},
	{"toneGraphChannel", .String, offset_of(Preferences, tone_graph_channel)},
	{"freshEditsForNewPhotos", .Bool, offset_of(Preferences, fresh_edits)},
	{"autoPlaceNewImages", .Bool, offset_of(Preferences, match_new_photos)},
	{"limitPreview", .Opt_Bool, offset_of(Preferences, limit_preview)},
	{"lowMemoryExport", .Bool, offset_of(Preferences, low_memory_export)},
	{"lightroomCloud", .Opt_String, offset_of(Preferences, lightroom_cloud)},
	{"lightroomClassic", .Opt_String, offset_of(Preferences, lightroom_classic)},
	{"labAllControls", .Bool, offset_of(Preferences, lab_all_controls)},
	{"moreAdjustments", .String, offset_of(Preferences, more_adjustments)},
	{"scopeMode", .String, offset_of(Preferences, scope_mode)},
	{"scopeHeight", .F64, offset_of(Preferences, scope_height)},
	{"favoriteFilms", .Strings, offset_of(Preferences, favorite_films)},
	{"favoritePapers", .Strings, offset_of(Preferences, favorite_papers)},
	{"viewerBackgroundCustom", .Bool, offset_of(Preferences, viewer_background_custom)},
	{"viewerBackgroundColor", .String, offset_of(Preferences, viewer_background)},
	{"viewerSheet", .String, offset_of(Preferences, viewer_sheet)},
	{"appTheme", .String, offset_of(Preferences, app_theme)},
	{"", .Flatten, offset_of(Preferences, library)},
	{"fotufilmFilms", .String, offset_of(Preferences, fotufilm_films)},
	{"showNegsBuiltFilms", .Bool, offset_of(Preferences, show_negs_built_films)},
	{"filterShiftLink", .String, offset_of(Preferences, filter_shift_link)},
	{"originalMode", .String, offset_of(Preferences, original_mode)},
	{"originalReference", .String, offset_of(Preferences, original_reference)},
	{"settingsTab", .String, offset_of(Preferences, settings_tab)},
	{"batchNaming", .String, offset_of(Preferences, batch_naming)},
	{"batchCustomName", .String, offset_of(Preferences, batch_custom_name)},
	{"batchExisting", .String, offset_of(Preferences, batch_existing)},
}

@(private)
APP_THEME :: 39

// Starred for new users, as on the web and the Mac.
DEFAULT_FAVORITE_FILMS := [?]string{"kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"}

// The scope's heights in DIPs; it starts at the smallest.
SCOPE_HEIGHT_MIN :: 88.0
SCOPE_HEIGHT_MAX :: 420.0

// `Preferences::default()`. The strings are static; the lists are in the
// context's allocator.
default_preferences :: proc() -> Preferences {
	films := make([]string, len(DEFAULT_FAVORITE_FILMS), context.allocator)
	copy(films, DEFAULT_FAVORITE_FILMS[:])
	return Preferences {
		filmstrip_visible = true,
		grid_size = sheet.GRID_DEFAULT,
		expanded_adjustments = "Enlarger",
		inspector_tab = "Prep",
		inspector_pane = "Adjustments",
		export_engine = "fast",
		output_format = "png",
		export_edge = "full",
		export_edge_pixels = 2048,
		color_space = "sRGB",
		png_bit_depth = 8,
		tiff_bit_depth = 16,
		jpeg_quality = 95,
		// The Mac's column starts at its narrowest.
		inspector_width = 340.0,
		check_for_updates = true,
		previews = true,
		tone_graph_channel = "rgb",
		fresh_edits = true,
		match_new_photos = true,
		scope_mode = "Histogram",
		scope_height = SCOPE_HEIGHT_MIN,
		favorite_films = films,
		// A neutral dark grey, near the dark theme's.
		viewer_background = "#242424",
		fotufilm_films = "all",
		filter_shift_link = "separate",
		original_mode = "hold",
		original_reference = "raw",
		batch_naming = "nameFilm",
		batch_custom_name = "{name}_{film}",
		batch_existing = "number",
		settings_tab = "Photographs",
	}
}

// MARK: Reading

@(private)
read_field :: proc(p: ^Parser, f: Field, base: rawptr) -> bool {
	at := rawptr(uintptr(base) + f.offset)
	switch f.kind {
	case .Bool:
		(^bool)(at)^ = read_bool(p) or_return
	case .F64:
		(^f64)(at)^ = read_f64(p) or_return
	case .U32:
		(^u32)(at)^ = read_u32(p) or_return
	case .String:
		(^string)(at)^ = read_string(p) or_return
	case .Strings:
		(^[]string)(at)^ = read_strings(p) or_return
	case .Opt_String:
		o := (^Opt_String)(at)
		o^ = {}
		if is_null := read_null(p) or_return; !is_null {
			o^ = {true, read_string(p) or_return}
		}
	case .Opt_U32:
		o := (^Opt_U32)(at)
		o^ = {}
		if is_null := read_null(p) or_return; !is_null {
			o^ = {true, read_u32(p) or_return}
		}
	case .Opt_Bool:
		o := (^Opt_Bool)(at)
		o^ = {}
		if is_null := read_null(p) or_return; !is_null {
			o^ = {true, read_bool(p) or_return}
		}
	case .Opt_Ints5, .Opt_Ints3:
		o := (^Opt_Ints)(at)
		o^ = {}
		if is_null := read_null(p) or_return; !is_null {
			o^ = {true, read_ints(p, 5 if f.kind == .Opt_Ints5 else 3) or_return}
		}
	case .Flatten:
	}
	return true
}

// LibraryPreferences' field `index` from its buffered value, as serde's
// ContentDeserializer gives it.
@(private)
library_field :: proc(l: ^Library_Preferences, index: int, v: value.Value) -> bool {
	opt_string :: proc(v: value.Value) -> (Opt_String, bool) {
		if v == nil {
			return {}, true
		}
		s, is_string := v.(string)
		return {true, s}, is_string
	}
	opt_bool :: proc(v: value.Value) -> (Opt_Bool, bool) {
		if v == nil {
			return {}, true
		}
		b, is_bool := v.(bool)
		return {true, b}, is_bool
	}
	ok := true
	switch index {
	case 0:
		l.library_filter = v
	case 1:
		l.copy_edit_groups = {}
		if v != nil {
			items, is_array := v.(value.Array)
			if !is_array {
				return false
			}
			out := make([]string, len(items), context.allocator)
			for item, i in items {
				out[i] = item.(string) or_return
			}
			l.copy_edit_groups = {true, out}
		}
	case 2:
		l.advance_after_rating, ok = v.(bool)
	case 3:
		l.new_photo_film, ok = opt_string(v)
	case 4:
		l.new_photo_paper, ok = opt_string(v)
	case 5:
		l.separate_camera_jpegs, ok = v.(bool)
	case 6:
		l.sidecars, ok = opt_bool(v)
	}
	return ok
}

// A Preferences at the parser's place, as `#[derive(Deserialize)]` with
// the flattened LibraryPreferences reads it.
read_preferences :: proc(p: ^Parser) -> (out: Preferences, ok: bool) {
	c, any := value.peek_ws(p)
	if !any {
		return out, value.fail(p, .Eof)
	}
	if c != '{' {
		return out, value.fail(p, .Invalid_Type)
	}
	out = default_preferences()
	Buffered :: struct {
		index: int,
		value: value.Value,
	}
	buffered := make([dynamic]Buffered, 0, 8, context.allocator)
	seen: bit_set[0 ..< 64]
	value.enter(p) or_return
	p.i += 1
	first := true
	outer: for {
		more := value.map_next(p, &first) or_return
		if !more {
			break
		}
		key := value.map_key(p) or_return
		for f, i in FIELDS {
			if f.kind != .Flatten && f.name == key {
				if i in seen {
					return out, value.fail(p, .Duplicate_Field)
				}
				read_field(p, f, &out) or_return
				seen += {i}
				continue outer
			}
		}
		// Not the struct's own: buffered in full.
		v := value.parse_value(p) or_return
		for name, i in LIBRARY_FIELDS {
			if name == key {
				append(&buffered, Buffered{i, v})
				break
			}
		}
	}
	value.leave(p)
	value.map_end(p) or_return
	if APP_THEME not_in seen {
		out.app_theme = "darkroom"
	}
	out.library = {}
	library_seen: bit_set[0 ..< 8]
	for b in buffered {
		if b.index in library_seen {
			return out, value.fail(p, .Duplicate_Field)
		}
		if !library_field(&out.library, b.index, b.value) {
			return out, value.fail(p, .Invalid_Type)
		}
		library_seen += {b.index}
	}
	return out, true
}

// `serde_json::from_slice::<Preferences>`.
parse_preferences :: proc(text: []u8) -> (Preferences, bool) {
	p := value.parser_make(text, context.allocator)
	prefs, ok := read_preferences(&p)
	return prefs, ok && value.finish(&p)
}

// MARK: Writing

@(private)
strings_value :: proc(list: []string) -> value.Value {
	a := value.make_array(len(list), context.allocator)
	for s in list {
		append(&a, value.Value(s))
	}
	return a
}

@(private)
field_value :: proc(f: Field, base: rawptr) -> value.Value {
	at := rawptr(uintptr(base) + f.offset)
	switch f.kind {
	case .Bool:
		return (^bool)(at)^
	case .F64:
		return value.from_f64((^f64)(at)^)
	case .U32:
		return u64((^u32)(at)^)
	case .String:
		return (^string)(at)^
	case .Strings:
		return strings_value((^[]string)(at)^)
	case .Opt_String:
		o := (^Opt_String)(at)^
		return o.value if o.has else nil
	case .Opt_U32:
		o := (^Opt_U32)(at)^
		return u64(o.value) if o.has else nil
	case .Opt_Bool:
		o := (^Opt_Bool)(at)^
		return o.value if o.has else nil
	case .Opt_Ints5, .Opt_Ints3:
		o := (^Opt_Ints)(at)^
		if !o.has {
			return nil
		}
		n := 5 if f.kind == .Opt_Ints5 else 3
		a := value.make_array(n, context.allocator)
		for i in 0 ..< n {
			append(&a, value.from_i64(i64(o.value[i])))
		}
		return a
	case .Flatten:
	}
	return nil
}

// The preferences as an object in declaration order (not sorted: the
// writers write objects in the order they hold). Strings and the filter
// are borrowed.
preferences_object :: proc(prefs: Preferences) -> value.Object {
	o := value.make_object(len(FIELDS) + len(LIBRARY_FIELDS), context.allocator)
	prefs := prefs
	for f in FIELDS {
		if f.kind != .Flatten {
			append(&o, value.Field{f.name, field_value(f, &prefs)})
			continue
		}
		l := prefs.library
		append(&o, value.Field{"libraryFilter", l.library_filter})
		append(&o, value.Field{"copyEditGroups", strings_value(l.copy_edit_groups.value) if l.copy_edit_groups.has else nil})
		append(&o, value.Field{"advanceAfterRating", l.advance_after_rating})
		append(&o, value.Field{"newPhotoFilm", l.new_photo_film.value if l.new_photo_film.has else nil})
		append(&o, value.Field{"newPhotoPaper", l.new_photo_paper.value if l.new_photo_paper.has else nil})
		append(&o, value.Field{"separateCameraJPEGs", l.separate_camera_jpegs})
		if l.sidecars.has {
			append(&o, value.Field{"sidecars", l.sidecars.value})
		}
	}
	return o
}

// `serde_json::to_vec_pretty(&prefs)` (or `to_vec`).
preferences_text :: proc(prefs: Preferences, pretty := true) -> []u8 {
	b := strings.builder_make(0, 4096, context.allocator)
	o := preferences_object(prefs)
	if pretty {
		value.write_pretty(&b, o)
	} else {
		value.write_compact(&b, o)
	}
	return b.buf[:]
}
