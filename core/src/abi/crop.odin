// The crop's C ABI (negs_core.h, "Phase 3b: Picture rules", Crop): plain
// structs in, results through out-pointers; nothing allocates.
package negs_core

import "../marks"
import "../rules/crop"

// crop.Rect and crop.Point are laid out as negs_rect_t and negs_pt_t.
#assert(size_of(crop.Rect) == size_of(marks.Rect) && offset_of(crop.Rect, height) == offset_of(marks.Rect, h))
#assert(size_of(crop.Point) == size_of(marks.Pt) && offset_of(crop.Point, y) == offset_of(marks.Pt, y))

@(private = "file")
maybe_f64 :: proc "contextless" (v: Maybe(f64), out: ^f64) -> u8 {
	x, ok := v.?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = x
	}
	return 1
}

@(private = "file")
store :: proc "contextless" (out: ^$T, v: T) {
	if out != nil {
		out^ = v
	}
}

@(private = "file")
aspect_of :: proc "contextless" (a: u32) -> crop.Aspect {
	return crop.Aspect(a) if a <= u32(max(crop.Aspect)) else .Original
}

@(export, link_name = "negs_crop_max_angle")
crop_max_angle :: proc "c" () -> f64 {return crop.MAX_ANGLE}

@(export, link_name = "negs_crop_aspect_title")
crop_aspect_title :: proc "c" (aspect: u32) -> cstring {return crop.ASPECT_TITLES[aspect_of(aspect)]}

@(export, link_name = "negs_crop_aspect_format")
crop_aspect_format :: proc "c" (aspect: u32) -> cstring {return crop.aspect_format(aspect_of(aspect))}

@(export, link_name = "negs_crop_aspect_ratio")
crop_aspect_ratio :: proc "c" (aspect: u32, photo: crop.Size, out: ^f64) -> u8 {
	return maybe_f64(crop.aspect_ratio(aspect_of(aspect), photo), out)
}

@(export, link_name = "negs_crop_aspect_turns")
crop_aspect_turns :: proc "c" (aspect: u32) -> u8 {return u8(crop.aspect_turns(aspect_of(aspect)))}

@(export, link_name = "negs_crop_cropped")
crop_cropped :: proc "c" (s: ^crop.Settings) -> u8 {return u8(crop.cropped(s^))}

@(export, link_name = "negs_crop_engine_pixels")
crop_engine_pixels :: proc "c" (s: ^crop.Settings, width, height: u32, out: ^[4]u32) {
	store(out, crop.engine_pixels(s^, width, height))
}

@(export, link_name = "negs_crop_straightened_size")
crop_straightened_size :: proc "c" (s: ^crop.Settings, width, height: u32, out: ^[2]u32) {
	store(out, crop.straightened_size(s^, width, height))
}

@(export, link_name = "negs_crop_pixels")
crop_pixels :: proc "c" (s: ^crop.Settings, size: crop.Size, out: ^crop.Rect) {store(out, crop.pixels(s^, size))}

@(export, link_name = "negs_crop_size_of")
crop_size_of :: proc "c" (s: ^crop.Settings, size: crop.Size, out: ^crop.Size) {store(out, crop.size_of_crop(s^, size))}

@(export, link_name = "negs_crop_shown")
crop_shown :: proc "c" (rect: crop.Rect, rotation: i32, flip: u8, out: ^crop.Rect) {
	store(out, crop.shown(rect, rotation, flip != 0))
}

@(export, link_name = "negs_crop_unrotated")
crop_unrotated :: proc "c" (rect: crop.Rect, rotation: i32, flip: u8, out: ^crop.Rect) {
	store(out, crop.unrotated(rect, rotation, flip != 0))
}

@(export, link_name = "negs_crop_mirrored")
crop_mirrored :: proc "c" (rect: crop.Rect, out: ^crop.Rect) {store(out, crop.mirrored(rect))}

@(export, link_name = "negs_rect_inset")
crop_rect_inset :: proc "c" (rect: crop.Rect, dx, dy: f64, out: ^crop.Rect) {store(out, crop.inset(rect, dx, dy))}

@(export, link_name = "negs_rect_contains")
crop_rect_contains :: proc "c" (rect: crop.Rect, p: crop.Point) -> u8 {return u8(crop.contains(rect, p))}

@(export, link_name = "negs_rect_intersection")
crop_rect_intersection :: proc "c" (a, b: crop.Rect, out: ^crop.Rect) {store(out, crop.intersection(a, b))}

@(export, link_name = "negs_orientation_new")
crop_orientation_new :: proc "c" (turns: i32, flip: u8, out: ^crop.Orientation) {
	store(out, crop.orientation(turns, flip != 0))
}

@(export, link_name = "negs_orientation_flipped_as_shown")
crop_orientation_flipped :: proc "c" (o: crop.Orientation, out: ^crop.Orientation) {
	store(out, crop.flipped_as_shown(o))
}

@(export, link_name = "negs_orientation_shown")
crop_orientation_shown :: proc "c" (o: crop.Orientation, p: crop.Point, out: ^crop.Point) {store(out, crop.shown_point(o, p))}

@(export, link_name = "negs_orientation_unshown")
crop_orientation_unshown :: proc "c" (o: crop.Orientation, p: crop.Point, out: ^crop.Point) {store(out, crop.unshown(o, p))}

@(export, link_name = "negs_crop_unshown_point")
crop_unshown_point :: proc "c" (o: crop.Orientation, s: ^crop.Settings, turned: crop.Size, x, y: f64, out: ^crop.Point) -> u8 {
	p, ok := crop.unshown_point(o, s^, turned, {x, y}).?
	if !ok {
		return 0
	}
	store(out, p)
	return 1
}

@(export, link_name = "negs_crop_fit")
crop_fit :: proc "c" (ratio: f64, center: crop.Point, bounds: crop.Rect, out: ^crop.Rect) {
	store(out, crop.fit(ratio, center, bounds))
}

@(export, link_name = "negs_crop_move_rect")
crop_move_rect :: proc "c" (rect: crop.Rect, dx, dy: f64, bounds: crop.Rect, out: ^crop.Rect) {
	store(out, crop.move_rect(rect, dx, dy, bounds))
}

@(export, link_name = "negs_crop_resize")
crop_resize :: proc "c" (rect: crop.Rect, handle: crop.Handle, point: crop.Point, has_ratio: u8, ratio: f64, bounds: crop.Rect, minimum: f64, out: ^crop.Rect) {
	r: Maybe(f64) = ratio if has_ratio != 0 else nil
	store(out, crop.resize(rect, handle, point, r, bounds, minimum))
}

@(export, link_name = "negs_crop_handle_at")
crop_handle_at :: proc "c" (point: crop.Point, frame: crop.Rect, scale: f64, out: ^crop.Handle) -> u8 {
	h, ok := crop.handle_at(point, frame, scale).?
	if !ok {
		return 0
	}
	store(out, h)
	return 1
}

@(export, link_name = "negs_crop_turn")
crop_turn :: proc "c" (point: crop.Point, degrees: f64, center: crop.Point, out: ^crop.Point) {
	store(out, crop.turn(point, degrees, center))
}

@(export, link_name = "negs_crop_corners")
crop_corners :: proc "c" (photo: crop.Size, angle: f64, out: ^[4]crop.Point) {store(out, crop.corners(photo, angle))}

@(export, link_name = "negs_crop_bounds")
crop_bounds :: proc "c" (photo: crop.Size, angle: f64, out: ^crop.Rect) {store(out, crop.bounds(photo, angle))}

@(export, link_name = "negs_crop_fits")
crop_fits :: proc "c" (rect: crop.Rect, angle: f64, photo: crop.Size) -> u8 {return u8(crop.fits(rect, angle, photo))}

@(export, link_name = "negs_crop_constrain")
crop_constrain :: proc "c" (start, target: crop.Rect, angle: f64, photo: crop.Size, out: ^crop.Rect) {
	store(out, crop.constrain(start, target, angle, photo))
}

@(export, link_name = "negs_crop_inscribe")
crop_inscribe :: proc "c" (rect: crop.Rect, angle: f64, photo: crop.Size, out: ^crop.Rect) {
	store(out, crop.inscribe(rect, angle, photo))
}

// The session's geometry.

@(export, link_name = "negs_crop_session_new")
crop_session_new :: proc "c" (start: ^crop.Settings, photo: crop.Size, rotation: i32, flip: u8, out: ^crop.Session) {
	s := start^
	s.aspect = aspect_of(u32(s.aspect))
	store(out, crop.session_new(s, photo, rotation, flip != 0))
}

@(export, link_name = "negs_crop_session_settings")
crop_session_settings :: proc "c" (s: ^crop.Session, out: ^crop.Settings) {store(out, crop.session_settings(s^))}

@(export, link_name = "negs_crop_session_modified")
crop_session_modified :: proc "c" (s: ^crop.Session) -> u8 {return u8(crop.session_modified(s^))}

@(export, link_name = "negs_crop_session_orientation_portrait")
crop_session_orientation_portrait :: proc "c" (s: ^crop.Session) -> u8 {return u8(crop.session_orientation_portrait(s^))}

@(export, link_name = "negs_crop_session_ratio")
crop_session_ratio :: proc "c" (s: ^crop.Session, out: ^f64) -> u8 {return maybe_f64(crop.session_ratio(s^), out)}

@(export, link_name = "negs_crop_session_shown")
crop_session_shown :: proc "c" (s: ^crop.Session, out: ^crop.Rect) {store(out, crop.session_shown(s^))}

@(export, link_name = "negs_crop_session_set_shown")
crop_session_set_shown :: proc "c" (s: ^crop.Session, value: crop.Rect) {crop.session_set_shown(s, value)}

@(export, link_name = "negs_crop_session_bounds")
crop_session_bounds :: proc "c" (s: ^crop.Session, out: ^crop.Rect) {store(out, crop.session_bounds(s^))}

@(export, link_name = "negs_crop_session_update")
crop_session_update :: proc "c" (s: ^crop.Session, photo: crop.Size, rotation: i32, flip: u8) {
	crop.session_update(s, photo, rotation, flip != 0)
}

@(export, link_name = "negs_crop_session_choose")
crop_session_choose :: proc "c" (s: ^crop.Session, aspect: u32) {crop.session_choose(s, aspect_of(aspect))}

@(export, link_name = "negs_crop_session_orient")
crop_session_orient :: proc "c" (s: ^crop.Session, portrait: u8) {crop.session_orient(s, portrait != 0)}

@(export, link_name = "negs_crop_session_set_custom")
crop_session_set_custom :: proc "c" (s: ^crop.Session, has_width: u8, width: f64, has_height: u8, height: f64) {
	w: Maybe(f64) = width if has_width != 0 else nil
	h: Maybe(f64) = height if has_height != 0 else nil
	crop.session_set_custom(s, w, h)
}

@(export, link_name = "negs_crop_session_reset")
crop_session_reset :: proc "c" (s: ^crop.Session) {crop.session_reset(s)}

@(export, link_name = "negs_crop_session_begin_straightening")
crop_session_begin_straightening :: proc "c" (s: ^crop.Session) {crop.session_begin_straightening(s)}

@(export, link_name = "negs_crop_session_straighten")
crop_session_straighten :: proc "c" (s: ^crop.Session, degrees: f64) {crop.session_straighten(s, degrees)}

@(export, link_name = "negs_crop_session_end_straightening")
crop_session_end_straightening :: proc "c" (s: ^crop.Session) {crop.session_end_straightening(s)}
