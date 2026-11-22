// The border's sizes, ported from the engine (spektrafilm-io's
// `border::{margin, keyline_width, film_geometry, instagram_canvas}`,
// `FilmFormat::of` and `film35::Strip`'s canvas): integers and a few f64
// steps, held to the engine's functions bit for bit by
// core/diff/tests/border.rs. The strip's canvas doesn't depend on the film
// (only its edge print does), so it takes the format alone.
package border

import "../crop"

Style :: enum u32 {
	// A plain mat; saved as "white", its name before it took a colour.
	Solid,
	// A scanned 120 film frame: black with an irregular edge.
	Film,
	// A mat with a thin line around the photograph.
	Keyline,
	// A 35mm negative strip, for 3:2 and 3:4 photographs.
	Film35,
}

// The 35mm frame a photograph fills.
Format :: enum u32 {
	Full, // 36 × 24 mm, 3:2
	Half, // 18 × 24 mm, 3:4
}

FORMAT_NAMES := [Format]cstring {
	.Full = "full",
	.Half = "half",
}

// The format whose frame matches a `width` × `height` photograph in either
// orientation, to within 1%.
format_of :: proc "contextless" (width, height: u32) -> Maybe(Format) {
	ratio := f64(max(width, height)) / f64(max(min(width, height), 1))
	four: f64 = 4
	frames := [2]f64{1.5, four / 3.0}
	for frame, i in frames {
		if crop.fabs(ratio / frame - 1.0) <= 0.01 {
			return Format(i)
		}
	}
	return nil
}

// A margin of `fraction` of the long edge, rounded down.
margin :: proc "contextless" (width, height: u32, fraction: f64) -> u32 {
	return crop.sat_u32(crop.floor(f64(max(width, height)) * fraction))
}

// The white margin around film borders on Instagram: 3% of the long edge.
white_margin :: proc "contextless" (width, height: u32) -> u32 {
	return margin(width, height, 0.03)
}

// Width of the keyline's line, `fraction` of the long edge.
keyline_width :: proc "contextless" (width, height: u32, fraction: f64) -> u32 {
	return max(crop.sat_u32(crop.round(f64(max(width, height)) * fraction)), 1)
}

// The film frame's (inset, nine-patch size).
film_geometry :: proc "contextless" (width, height: u32) -> [2]u32 {
	short := f64(min(width, height))
	return {max(crop.sat_u32(crop.round(short * 0.003)), 1), max(crop.sat_u32(crop.round(short * 0.025)), 2)}
}

@(private = "file")
div_ceil :: proc "contextless" (a, b: u32) -> u32 {
	d, r := a / b, a % b
	return d + 1 if r > 0 else d
}

// The 3:4 portrait canvas Instagram shows whole, for a `width` × `height`
// photograph with a `style` border: the film borders get a white margin
// first, then white extends the short way as little as it can.
instagram_canvas :: proc "contextless" (width, height: u32, style: Style) -> [2]u32 {
	m: u32 = 0
	if style == .Film || style == .Film35 {
		m = white_margin(width, height)
	}
	w, h := width + 2 * m, height + 2 * m
	if w * 4 > h * 3 {
		return {w, div_ceil(w * 4, 3)}
	}
	return {div_ceil(h * 3, 4), h}
}

// film35's constants, in mm.
FILM_WIDTH: f32 : 35.0
PERF_PITCH: f32 : 4.75
GATE_HEIGHT: f32 : 24.0

// Strip length (along the film) and gate width, in mm.
strip_size :: proc "contextless" (format: Format) -> [2]f32 {
	pitch: f32 = PERF_PITCH
	if format == .Half {
		return {4.0 * pitch, 18.0}
	}
	return {8.0 * pitch, 36.0}
}

// The gate as fractions of the strip laid out horizontally: [x, y, width, height].
strip_window :: proc "contextless" (format: Format) -> [4]f64 {
	s := strip_size(format)
	length, gate, film := f64(s[0]), f64(s[1]), f64(FILM_WIDTH)
	height := f64(GATE_HEIGHT)
	return {(length - gate) / 2.0 / length, (film - height) / 2.0 / film, gate / length, height / film}
}

// Whether the strip turns to hold a `width` × `height` photograph.
strip_turned :: proc "contextless" (format: Format, width, height: u32) -> bool {
	gate := strip_size(format)[1]
	return (gate > GATE_HEIGHT) != (width > height)
}

// The strip scaled so its gate covers a `width` × `height` photograph.
strip_canvas :: proc "contextless" (format: Format, width, height: u32) -> [2]u32 {
	tw, th := f64(strip_size(format)[0]), f64(FILM_WIDTH)
	window := strip_window(format)
	ww, wh := window[2], window[3]
	if strip_turned(format, width, height) {
		tw, th, ww, wh = th, tw, wh, ww
	}
	k := crop.fmax(f64(width) / (ww * tw), f64(height) / (wh * th))
	return {crop.sat_u32(crop.round(tw * k)), crop.sat_u32(crop.round(th * k))}
}
