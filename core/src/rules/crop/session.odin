// The crop being drawn (CropSession), its geometry: nothing changes the
// edit until Done. The host keeps the edit it began from (Phase 4's state)
// and the pointer's flags; this is the rectangle, the locked ratio and the
// straightening.
package crop

// Laid out as negs_crop_session_t.
Session :: struct {
	// A fraction of the unrotated photograph, like Settings.rect.
	rect:          Rect,
	custom:        Size,
	// The developed photograph as shown, in pixels.
	photo:         Size,
	// The crop when straightening began, as shown: it shrinks from it to
	// stay on the turned photograph, and grows back as the angle returns.
	intended:      Rect,
	// Degrees clockwise as shown: Settings.angle, the other way round on a
	// flipped photograph.
	angle:         f64,
	rotation:      i32,
	aspect:        Aspect,
	// The locked ratio is taller than wide (presets; custom uses its sides).
	portrait:      bool,
	flip:          bool,
	straightening: bool,
	has_intended:  bool,
	reserved:      u32,
}

// A flip mirrors the straightening: clockwise as shown is anticlockwise on
// the unflipped photograph.
@(private = "file")
mirrored_angle :: proc "contextless" (angle: f64, flip: bool) -> f64 {
	return -angle if flip && angle != 0 else angle
}

session_new :: proc "contextless" (start: Settings, photo: Size, rotation: i32, flip: bool) -> Session {
	r := shown(start.rect, rotation, flip)
	portrait := r.height * photo.height > r.width * photo.width
	custom := start.custom
	if start.aspect == .Custom && (custom.height > custom.width) != portrait {
		custom = swapped(custom)
	}
	return Session {
		rect = start.rect,
		aspect = start.aspect,
		portrait = portrait,
		custom = custom,
		angle = mirrored_angle(start.angle, flip),
		photo = photo,
		rotation = rotation,
		flip = flip,
	}
}

session_settings :: proc "contextless" (s: Session) -> Settings {
	if s.angle != 0 {
		return Settings{rect = s.rect, aspect = s.aspect, custom = s.custom, angle = mirrored_angle(s.angle, s.flip)}
	}
	unit := intersection(s.rect, UNIT)
	// Within a thousandth of the whole photograph is no crop.
	whole := fabs(unit.x) < 1e-3 && fabs(unit.y) < 1e-3 && fabs(max_x(unit) - 1.0) < 1e-3 && fabs(max_y(unit) - 1.0) < 1e-3
	return Settings{rect = UNIT if whole else unit, aspect = s.aspect, custom = s.custom, angle = 0}
}

session_modified :: proc "contextless" (s: Session) -> bool {
	return !rect_eq(s.rect, UNIT) || s.aspect != .Original || s.angle != 0
}

session_orientation_portrait :: proc "contextless" (s: Session) -> bool {
	return s.custom.height > s.custom.width if s.aspect == .Custom else s.portrait
}

// Width over height of the locked ratio, as shown; none for freeform.
session_ratio :: proc "contextless" (s: Session) -> Maybe(f64) {
	if s.aspect == .Custom {
		if s.custom.width > 0 && s.custom.height > 0 {
			return s.custom.width / s.custom.height
		}
		return nil
	}
	ratio, ok := aspect_ratio(s.aspect, s.photo).?
	if !ok {
		return nil
	}
	return 1.0 / ratio if s.portrait else ratio
}

// The rectangle in pixels of the photograph as shown.
session_shown :: proc "contextless" (s: Session) -> Rect {
	r := shown(s.rect, s.rotation, s.flip)
	return Rect{r.x * s.photo.width, r.y * s.photo.height, r.width * s.photo.width, r.height * s.photo.height}
}

session_set_shown :: proc "contextless" (s: ^Session, value: Rect) {
	if s.photo.width <= 0 || s.photo.height <= 0 {
		return
	}
	r := Rect{value.x / s.photo.width, value.y / s.photo.height, value.width / s.photo.width, value.height / s.photo.height}
	s.rect = unrotated(r, s.rotation, s.flip)
}

// Where the rectangle can go: the turned photograph's bounding box.
session_bounds :: proc "contextless" (s: Session) -> Rect {
	return bounds(s.photo, s.angle)
}

// A newer render: a quarter turn swaps the locked ratio's orientation, as
// the rectangle turns with the photograph. A flip mirrors the rectangle and
// the straightening with it.
session_update :: proc "contextless" (s: ^Session, photo: Size, rotation: i32, flip: bool) {
	if (rotation - s.rotation) % 2 != 0 {
		s.portrait = !s.portrait
		s.custom = swapped(s.custom)
	}
	if flip != s.flip {
		s.angle = mirrored_angle(s.angle, true)
		s.has_intended = false
		s.intended = {}
	}
	s.photo = photo
	s.rotation = rotation
	s.flip = flip
}

session_choose :: proc "contextless" (s: ^Session, aspect: Aspect) {
	s.aspect = aspect
	if aspect == .Original {
		s.portrait = s.photo.height > s.photo.width
	}
	session_refit(s)
}

session_orient :: proc "contextless" (s: ^Session, portrait: bool) {
	if s.aspect == .Custom {
		if (s.custom.height > s.custom.width) != portrait {
			s.custom = swapped(s.custom)
		}
	} else {
		s.portrait = portrait
	}
	session_refit(s)
}

session_set_custom :: proc "contextless" (s: ^Session, width, height: Maybe(f64)) {
	if w, ok := width.?; ok {
		s.custom.width = clamp_f64(w, 0.1, 1000.0)
	}
	if h, ok := height.?; ok {
		s.custom.height = clamp_f64(h, 0.1, 1000.0)
	}
	session_refit(s)
}

session_reset :: proc "contextless" (s: ^Session) {
	s.rect = UNIT
	s.aspect = .Original
	s.angle = 0
	s.portrait = s.photo.height > s.photo.width
}

session_begin_straightening :: proc "contextless" (s: ^Session) {
	if !s.has_intended {
		s.intended = session_shown(s^)
		s.has_intended = true
	}
	s.straightening = true
}

// Turns the photograph, shrinking the crop as needed to stay on it.
session_straighten :: proc "contextless" (s: ^Session, degrees: f64) {
	base := s.intended if s.has_intended else session_shown(s^)
	s.angle = clamp_f64(round(degrees * 10.0) / 10.0, -MAX_ANGLE, MAX_ANGLE)
	session_set_shown(s, inscribe(base, s.angle, s.photo))
}

session_end_straightening :: proc "contextless" (s: ^Session) {
	s.has_intended = false
	s.intended = {}
	s.straightening = false
}

// The largest rectangle with the locked ratio, around the current centre.
session_refit :: proc "contextless" (s: ^Session) {
	ratio, ok := session_ratio(s^).?
	if !ok {
		return
	}
	current := session_shown(s^)
	fitted := fit(ratio, Point{mid_x(current), mid_y(current)}, of_size(s.photo))
	session_set_shown(s, inscribe(fitted, s.angle, s.photo))
}
