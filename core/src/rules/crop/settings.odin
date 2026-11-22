// A photograph's crop setting (CropSettings), its aspect ratios, and the
// pixel rounding the exporter crops with (spektrafilm-io's `Crop::pixels`
// and `straightened_size`, ported: core/diff/tests/crop.rs holds them to the
// engine's functions bit for bit).
package crop

// The furthest a photograph can be straightened, in degrees either way
// (spektrafilm-io's `crop::MAX_ANGLE`).
MAX_ANGLE: f64 : 45.0

Aspect :: enum u32 {
	Original,
	Freeform,
	Square,
	Sixteen_Nine,
	Three_Two,
	Four_Three,
	Five_Four,
	Seven_Five,
	Custom,
}

ASPECT_TITLES := [Aspect]cstring {
	.Original     = "Original",
	.Freeform     = "Freeform",
	.Square       = "Square",
	.Sixteen_Nine = "16:9",
	.Three_Two    = "3:2",
	.Four_Three   = "4:3",
	.Five_Four    = "5:4",
	.Seven_Five   = "7:5",
	.Custom       = "Custom",
}

// The film or print format with that shape (nil for none).
ASPECT_FORMATS := [Aspect]cstring {
	.Original     = nil,
	.Freeform     = nil,
	.Square       = "6×6",
	.Sixteen_Nine = nil,
	.Three_Two    = "35mm",
	.Four_Three   = "645",
	.Five_Four    = "4×5",
	.Seven_Five   = "5×7",
	.Custom       = nil,
}

// The long side over the short side, for a photograph shown at `photo`.
aspect_ratio :: proc "contextless" (a: Aspect, photo: Size) -> Maybe(f64) {
	switch a {
	case .Freeform, .Custom:
		return nil
	case .Original:
		if photo.width > 0 && photo.height > 0 {
			return fmax(photo.width, photo.height) / fmin(photo.width, photo.height)
		}
		return nil
	case .Square:
		return 1.0
	case .Sixteen_Nine:
		sixteen: f64 = 16
		return sixteen / 9.0
	case .Three_Two:
		return 1.5
	case .Four_Three:
		four: f64 = 4
		return four / 3.0
	case .Five_Four:
		return 1.25
	case .Seven_Five:
		return 1.4
	}
	return nil
}

// Whether the ratio has a landscape and a portrait orientation.
aspect_turns :: proc "contextless" (a: Aspect) -> bool {
	return a != .Freeform && a != .Square
}

// Laid out as negs_crop_settings_t.
Settings :: struct {
	rect:     Rect,
	// The custom ratio, width by height as shown.
	custom:   Size,
	angle:    f64,
	// The aspect ratio the rectangle was drawn with, offered again next time.
	aspect:   Aspect,
	reserved: u32,
}

DEFAULT_SETTINGS :: Settings {
	rect   = UNIT,
	aspect = .Original,
	custom = Size{5, 4},
	angle  = 0,
}

cropped :: proc "contextless" (s: Settings) -> bool {
	return !rect_eq(s.rect, UNIT) || s.angle != 0
}

// The engine's `Crop::pixels`: the pixel rectangle (x, y, width, height) in
// a `width` × `height` image; edges round to the nearest pixel, and at least
// one remains.
engine_pixels :: proc "contextless" (s: Settings, width, height: u32) -> [4]u32 {
	edge :: proc "contextless" (fraction: f64, size: u32) -> u32 {
		return sat_u32(clamp_f64(round(fraction * f64(size)), 0, f64(size)))
	}
	x0, y0 := edge(s.rect.x, width), edge(s.rect.y, height)
	x1 := min(max(edge(s.rect.x + s.rect.width, width), x0 + 1), width)
	y1 := min(max(edge(s.rect.y + s.rect.height, height), y0 + 1), height)
	x0, y0 = min(x0, x1 - 1), min(y0, y1 - 1)
	return {x0, y0, x1 - x0, y1 - y0}
}

// The engine's `Crop::straightened_size`: the sides round to the nearest
// pixel, keeping at least one.
straightened_size :: proc "contextless" (s: Settings, width, height: u32) -> [2]u32 {
	return {max(sat_u32(round(s.rect.width * f64(width))), 1), max(sat_u32(round(s.rect.height * f64(height))), 1)}
}

// The pixel rectangle in a `size` photograph, rounded like the exporter. A
// straightened crop keeps its origin and rounds its sides.
pixels :: proc "contextless" (s: Settings, sz: Size) -> Rect {
	if !cropped(s) {
		return of_size(sz)
	}
	w, h := sat_u32(round(sz.width)), sat_u32(round(sz.height))
	if w == 0 || h == 0 {
		return of_size(sz)
	}
	if s.angle != 0 {
		side := straightened_size(s, w, h)
		return Rect{s.rect.x * f64(w), s.rect.y * f64(h), f64(side[0]), f64(side[1])}
	}
	p := engine_pixels(s, w, h)
	return Rect{f64(p[0]), f64(p[1]), f64(p[2]), f64(p[3])}
}

// The cropped photograph's size, before rotation.
size_of_crop :: proc "contextless" (s: Settings, sz: Size) -> Size {
	if is_zero(sz) {
		return Size{}
	}
	return size(pixels(s, sz))
}

// The rectangle as the photograph is shown: mirrored left to right if
// `flip`, then after `rotation` clockwise quarter turns.
shown :: proc "contextless" (rect: Rect, rotation: i32, flip: bool) -> Rect {
	r := mirrored(rect) if flip else rect
	for _ in 0 ..< rem4(rotation) {
		r = Rect{1.0 - r.y - r.height, r.x, r.height, r.width}
	}
	return r
}

// The inverse of `shown`.
unrotated :: proc "contextless" (rect: Rect, rotation: i32, flip: bool) -> Rect {
	r := rect
	for _ in 0 ..< rem4(rotation) {
		r = Rect{r.y, 1.0 - r.x - r.width, r.height, r.width}
	}
	return mirrored(r) if flip else r
}

// A fraction of the photograph mirrored left to right.
mirrored :: proc "contextless" (rect: Rect) -> Rect {
	return Rect{1.0 - max_x(rect), rect.y, rect.width, rect.height}
}
