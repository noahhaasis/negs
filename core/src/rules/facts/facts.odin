// What the camera wrote down, for the caption over the photograph: camera,
// ISO, shutter, aperture and focal length (windows/src/photo_facts.rs,
// frozen in core/diff/oracle; `Caption.swift`'s PhotoFacts). Reading the
// EXIF is the engine's (spektrafilm_io), so it stays with the host, which
// hands its values to `from_exif`.
package facts

import "core:strings"
import "core:unicode/utf8"

Facts :: struct {
	camera:           string,
	has_camera:       bool,
	has_iso:          bool,
	has_exposure:     bool,
	has_aperture:     bool,
	has_focal_length: bool,
	iso:              f64,
	// Seconds.
	exposure:         f64,
	aperture:         f64,
	// Millimetres.
	focal_length:     f64,
}

// The EXIF values PhotoFacts::read takes from the engine's metadata.
Exif :: struct {
	make:                  string,
	model:                 string,
	has_make:              bool,
	has_model:             bool,
	has_iso_speed_ratings: bool,
	has_iso_speed:         bool,
	has_exposure_time:     bool,
	has_fnumber:           bool,
	has_focal_length:      bool,
	iso_speed_ratings:     u16,
	iso_speed:             u32,
	exposure_time:         [2]u32,
	fnumber:               [2]u32,
	focal_length:          [2]u32,
}

// What camera() gives: nothing, a name, or the original's panic (a model
// that starts with the maker's name, lowercased, but not at a char
// boundary of its own: "K" (Kelvin) and "kx").
Outcome :: enum u8 {
	None   = 0,
	Some   = 1,
	Panics = 2,
}

// "Nikon Z f" from "NIKON CORPORATION" and "NIKON Z f"; "Sony ILCE-7M3"
// from "SONY" and "ILCE-7M3". The maker's first word, in title case if it's
// shouted, then the model without the maker repeated. In the context's
// allocator.
camera :: proc(make: string, has_make: bool, model: string, has_model: bool, allocator := context.allocator) -> (string, Outcome) {
	model_t := trim(model) if has_model else ""
	model_ok := has_model && len(model_t) > 0
	brand: string
	has_brand: bool
	if has_make {
		brand, has_brand = first_word(make)
	}
	if !has_brand {
		if !model_ok {
			return "", .None
		}
		return strings.clone(model_t, allocator), .Some
	}
	b := strings.builder_make(0, len(brand) + len(model_t) + 1, allocator)
	if is_shouted(brand) && utf8.rune_count_in_string(brand) > 2 {
		_, n := utf8.decode_rune_in_string(brand)
		strings.write_string(&b, brand[:n])
		write_lower(&b, brand[n:])
	} else {
		strings.write_string(&b, brand)
	}
	if !model_ok {
		return strings.to_string(b), .Some
	}
	rest := model_t
	model_lower := lower(model_t, context.allocator)
	brand_lower := lower(brand, context.allocator)
	repeats := strings.has_prefix(model_lower, brand_lower)
	delete(model_lower)
	delete(brand_lower)
	if repeats {
		// model[brand.len()..]: Rust panics off a char boundary.
		if len(brand) > len(model_t) || (len(brand) < len(model_t) && !utf8.rune_start(model_t[len(brand)])) {
			strings.builder_destroy(&b)
			return "", .Panics
		}
		rest = trim(model_t[len(brand):])
	}
	if len(rest) > 0 {
		strings.write_byte(&b, ' ')
		strings.write_string(&b, rest)
	}
	return strings.to_string(b), .Some
}

// PhotoFacts::read's sums on the engine's EXIF: ratios with a zero
// denominator or not above zero are absent, as is an ISO of 0.
from_exif :: proc(e: Exif, allocator := context.allocator) -> (f: Facts, outcome: Outcome) {
	ratio :: proc(has: bool, r: [2]u32) -> (f64, bool) {
		if !has || r[1] == 0 {
			return 0, false
		}
		v := f64(r[0]) / f64(r[1])
		return v, v > 0 && v - v == 0
	}
	name, got := camera(e.make, e.has_make, e.model, e.has_model, allocator)
	if got == .Panics {
		return {}, .Panics
	}
	if got == .Some {
		f.camera, f.has_camera = name, true
	}
	if e.has_iso_speed_ratings {
		f.iso, f.has_iso = f64(e.iso_speed_ratings), true
	} else if e.has_iso_speed {
		f.iso, f.has_iso = f64(e.iso_speed), true
	}
	f.has_iso = f.has_iso && f.iso > 0
	f.exposure, f.has_exposure = ratio(e.has_exposure_time, e.exposure_time)
	f.aperture, f.has_aperture = ratio(e.has_fnumber, e.fnumber)
	f.focal_length, f.has_focal_length = ratio(e.has_focal_length, e.focal_length)
	return f, .Some
}

// "1/1250" under a second (and any whole fraction), else "2 s" or "1,3 s".
write_shutter :: proc(b: ^strings.Builder, seconds: f64, decimal: rune) {
	if seconds < 1.0 {
		denominator := 1.0 / seconds
		if seconds < 0.3 || abs(denominator - round(denominator)) < 0.05 {
			strings.write_string(b, "1/")
			strings.write_i64(b, as_i64(round(denominator)))
			return
		}
	}
	write_one_decimal(b, seconds, decimal)
	strings.write_string(b, " s")
}

shutter :: proc(seconds: f64, decimal: rune, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 24, allocator)
	write_shutter(&b, seconds, decimal)
	return strings.to_string(b)
}

// The caption's items by part (PhotoFacts.parts), in compare::PARTS order:
// camera, ISO, shutter, aperture, focal length. Absent parts are false.
Parts :: struct {
	text:    [5]string,
	present: [5]bool,
}

parts :: proc(f: Facts, decimal: rune, allocator := context.allocator) -> (out: Parts) {
	if f.has_camera {
		out.text[0], out.present[0] = strings.clone(f.camera, allocator), true
	}
	if f.has_iso {
		b := strings.builder_make(0, 12, allocator)
		strings.write_string(&b, "ISO ")
		strings.write_i64(&b, as_i64(round(f.iso)))
		out.text[1], out.present[1] = strings.to_string(b), true
	}
	if f.has_exposure {
		out.text[2], out.present[2] = shutter(f.exposure, decimal, allocator), true
	}
	if f.has_aperture {
		b := strings.builder_make(0, 12, allocator)
		strings.write_string(&b, "f/")
		write_one_decimal(&b, f.aperture, decimal)
		out.text[3], out.present[3] = strings.to_string(b), true
	}
	if f.has_focal_length {
		b := strings.builder_make(0, 12, allocator)
		write_one_decimal(&b, f.focal_length, decimal)
		strings.write_string(&b, " mm")
		out.text[4], out.present[4] = strings.to_string(b), true
	}
	return
}

parts_destroy :: proc(p: ^Parts, allocator := context.allocator) {
	for i in 0 ..< 5 {
		if p.present[i] {
			delete(p.text[i], allocator)
		}
	}
	p^ = {}
}
