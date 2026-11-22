// Your negative's C ABI (negs_core.h, "Phase 3b: Picture rules",
// Negative): its words and pure rules, the edit state's fields as plain
// values.
package negs_core

import "../rules/crop"
import "../rules/negative"

// negs_negative_text's names.
@(private = "file")
Negative_Text :: enum u32 {
	Route,
	Scene_Route,
	Route_Path,
	Default_Film,
	Stock_Id,
	Name,
	Tile_Text,
	Scan_Illuminant,
	Flat_Field,
	Flat_Field_Help,
}

@(private = "file")
NEGATIVE_TEXTS := [Negative_Text]cstring {
	.Route           = negative.ROUTE,
	.Scene_Route     = negative.SCENE_ROUTE,
	.Route_Path      = negative.ROUTE_PATH,
	.Default_Film    = negative.DEFAULT_FILM,
	.Stock_Id        = negative.STOCK_ID,
	.Name            = negative.NAME,
	.Tile_Text       = negative.TILE_TEXT,
	.Scan_Illuminant = negative.SCAN_ILLUMINANT,
	.Flat_Field      = negative.FLAT_FIELD,
	.Flat_Field_Help = negative.FLAT_FIELD_HELP,
}

@(export, link_name = "negs_negative_text")
negative_text :: proc "c" (which: u32) -> cstring {
	if which > u32(max(Negative_Text)) {
		return nil
	}
	return NEGATIVE_TEXTS[Negative_Text(which)]
}

// Path list `list` (NEGS_NEGATIVE_*_PATHS), its `index`th; NULL past the end.
@(export, link_name = "negs_negative_path")
negative_path :: proc "c" (list: u32, index: uint) -> cstring {
	paths: []cstring
	switch list {
	case 0:
		paths = negative.OWN_PATHS[:]
	case 1:
		paths = negative.FOUND_PATHS[:]
	case 2:
		paths = negative.PRESET_KEEPS[:]
	case 3:
		paths = negative.SCAN_READING[:]
	}
	return paths[index] if index < uint(len(paths)) else nil
}

// Laid out as negs_scan_light_t: shutter (s), aperture, ISO.
Scan_Light_C :: struct {
	value:    [3]f64,
	has:      [3]u8,
	reserved: [5]u8,
}

@(private = "file")
light_of :: proc "contextless" (l: Scan_Light_C) -> negative.Scan_Light {
	out: negative.Scan_Light
	for i in 0 ..< 3 {
		if l.has[i] != 0 {
			out[i] = l.value[i]
		}
	}
	return out
}

@(export, link_name = "negs_negative_scan_stops")
negative_scan_stops :: proc "c" (frame, reference: Scan_Light_C, out: ^f64) -> u8 {
	stops, ok := negative.scan_stops(light_of(frame), light_of(reference)).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = stops
	}
	return 1
}

@(export, link_name = "negs_negative_exposure_ev")
negative_exposure_ev :: proc "c" (roll: f64, has_stops: u8, stops: f64) -> f64 {
	s: Maybe(f64) = stops if has_stops != 0 else nil
	return negative.exposure_ev(roll, s)
}

@(export, link_name = "negs_negative_skipped")
negative_skipped :: proc "c" (count: u64) -> Buf {
	context = core_context()
	text := negative.skipped(uint(count))
	defer delete(text)
	return buf_text(text)
}

@(export, link_name = "negs_negative_is_route")
negative_is_route :: proc "c" (route: [^]u8, n: uint) -> u8 {return u8(negative.is_route(host_string(route, n)))}

@(export, link_name = "negs_negative_readable")
negative_readable :: proc "c" (paper, positive, bw: u8) -> u8 {return u8(negative.readable(paper != 0, positive != 0, bw != 0))}

@(export, link_name = "negs_negative_is_found")
negative_is_found :: proc "c" (path: [^]u8, n: uint) -> u8 {return u8(negative.is_found(host_string(path, n)))}

@(export, link_name = "negs_negative_picture_crop")
negative_picture_crop :: proc "c" (x0, y0, x1, y1, angle: f64, out: ^crop.Settings) -> u8 {
	s, ok := negative.picture_crop(x0, y0, x1, y1, angle).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = s
	}
	return 1
}

@(export, link_name = "negs_flat_field_value")
negative_flat_field_value :: proc "c" (path: [^]u8, n: uint) -> Buf {
	context = core_context()
	return buf_text(negative.flat_field_value(host_string(path, n)))
}

// `used`: -1 not said yet, 0 not used, 1 used. 0 for no hint.
@(export, link_name = "negs_flat_field_hint")
negative_flat_field_hint :: proc "c" (path: [^]u8, n: uint, used: i32, has_reason: u8, reason: [^]u8, reason_len: uint, out: ^Buf) -> u8 {
	context = core_context()
	u: Maybe(bool) = (used != 0) if used >= 0 else nil
	r: Maybe(string) = host_string(reason, reason_len) if has_reason != 0 else nil
	hint, ok := negative.flat_field_hint(host_string(path, n), u, r).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = buf_text(hint)
	}
	return 1
}

@(export, link_name = "negs_negative_not_negative")
negative_not_negative :: proc "c" (raw: u8) -> cstring {return negative.not_negative(raw != 0)}

// The edge as shown; NULL for a word that isn't an edge.
@(export, link_name = "negs_negative_shown_edge")
negative_shown_edge :: proc "c" (o: crop.Orientation, edge: [^]u8, n: uint) -> cstring {
	return negative.shown_edge(o, host_string(edge, n)).? or_else nil
}

@(export, link_name = "negs_base_status_guessed")
negative_base_guessed :: proc "c" (found, picked, by_hand, from_roll: u8) -> u8 {
	return u8(negative.guessed({found = found != 0, picked = picked != 0, by_hand = by_hand != 0, from_roll = from_roll != 0}))
}

@(export, link_name = "negs_base_status_sentence")
negative_base_sentence :: proc "c" (found, picked, by_hand, from_roll, has_edge: u8, edge: [^]u8, edge_len: uint, o: crop.Orientation) -> Buf {
	context = core_context()
	s := negative.Base_Status {
		found     = found != 0,
		picked    = picked != 0,
		by_hand   = by_hand != 0,
		from_roll = from_roll != 0,
	}
	if has_edge != 0 {
		s.edge = host_string(edge, edge_len)
	}
	text := negative.sentence(s, o)
	defer delete(text)
	return buf_text(text)
}

@(export, link_name = "negs_pick_prompt")
negative_pick_prompt :: proc "c" (pick: u32) -> cstring {
	return negative.PICK_PROMPTS[.Neutral if pick == 1 else .Base]
}

@(export, link_name = "negs_negative_same_number")
negative_same_number :: proc "c" (p, q: f64) -> u8 {return u8(negative.same_number(p, q))}
