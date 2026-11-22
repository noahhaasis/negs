// The photo facts' C ABI (negs_core.h, "Phase 3c"): the caption's camera
// name, shutter and parts as text in negs_buf_t blocks.
package negs_core

import "core:strings"
import "../rules/facts"

Photo_Facts_C :: struct {
	camera:           [^]u8,
	camera_len:       uint,
	has_camera:       u8,
	has_iso:          u8,
	has_exposure:     u8,
	has_aperture:     u8,
	has_focal_length: u8,
	reserved:         [3]u8,
	iso:              f64,
	exposure:         f64,
	aperture:         f64,
	focal_length:     f64,
}

Exif_Facts_C :: struct {
	make:                  [^]u8,
	make_len:              uint,
	model:                 [^]u8,
	model_len:             uint,
	has_make:              u8,
	has_model:             u8,
	has_iso_speed_ratings: u8,
	has_iso_speed:         u8,
	has_exposure_time:     u8,
	has_fnumber:           u8,
	has_focal_length:      u8,
	reserved:              u8,
	iso_speed_ratings:     u16,
	reserved2:             u16,
	iso_speed:             u32,
	exposure_time:         [2]u32,
	fnumber:               [2]u32,
	focal_length:          [2]u32,
}

@(private = "file")
text_buf :: proc(s: string) -> Buf {
	return buf_from(transmute([]u8)s)
}

@(export, link_name = "negs_photo_facts_camera")
photo_facts_camera :: proc "c" (make: [^]u8, make_len: uint, has_make: u8, model: [^]u8, model_len: uint, has_model: u8, out: ^Buf) -> u8 {
	context = core_context()
	if out != nil {
		out^ = {}
	}
	name, outcome := facts.camera(host_string(make, make_len), has_make != 0, host_string(model, model_len), has_model != 0)
	if outcome == .Some {
		if out != nil {
			out^ = text_buf(name)
		}
		delete(name)
	}
	return u8(outcome)
}

@(export, link_name = "negs_photo_facts_from_exif")
photo_facts_from_exif :: proc "c" (exif: ^Exif_Facts_C, out: ^Photo_Facts_C, camera: ^Buf) -> u8 {
	context = core_context()
	if exif == nil || out == nil || camera == nil {
		return 0
	}
	out^ = {}
	camera^ = {}
	e := facts.Exif {
		make                  = host_string(exif.make, exif.make_len),
		model                 = host_string(exif.model, exif.model_len),
		has_make              = exif.has_make != 0,
		has_model             = exif.has_model != 0,
		has_iso_speed_ratings = exif.has_iso_speed_ratings != 0,
		has_iso_speed         = exif.has_iso_speed != 0,
		has_exposure_time     = exif.has_exposure_time != 0,
		has_fnumber           = exif.has_fnumber != 0,
		has_focal_length      = exif.has_focal_length != 0,
		iso_speed_ratings     = exif.iso_speed_ratings,
		iso_speed             = exif.iso_speed,
		exposure_time         = exif.exposure_time,
		fnumber               = exif.fnumber,
		focal_length          = exif.focal_length,
	}
	f, outcome := facts.from_exif(e)
	defer if f.has_camera {
		delete(f.camera)
	}
	if outcome != .Some {
		return u8(outcome)
	}
	if f.has_camera {
		camera^ = text_buf(f.camera)
		out.camera, out.camera_len = camera.data, camera.len
	}
	out.has_camera = u8(f.has_camera)
	out.has_iso, out.iso = u8(f.has_iso), f.iso
	out.has_exposure, out.exposure = u8(f.has_exposure), f.exposure
	out.has_aperture, out.aperture = u8(f.has_aperture), f.aperture
	out.has_focal_length, out.focal_length = u8(f.has_focal_length), f.focal_length
	return 1
}

@(export, link_name = "negs_photo_facts_shutter")
photo_facts_shutter :: proc "c" (seconds: f64, decimal: u32) -> Buf {
	context = core_context()
	s := facts.shutter(seconds, rune(decimal))
	defer delete(s)
	return text_buf(s)
}

@(export, link_name = "negs_photo_facts_parts")
photo_facts_parts :: proc "c" (f: ^Photo_Facts_C, decimal: u32, lengths: ^[5]uint) -> Buf {
	context = core_context()
	if f == nil || lengths == nil {
		return {}
	}
	in_facts := facts.Facts {
		camera           = host_string(f.camera, f.camera_len),
		has_camera       = f.has_camera != 0,
		has_iso          = f.has_iso != 0,
		has_exposure     = f.has_exposure != 0,
		has_aperture     = f.has_aperture != 0,
		has_focal_length = f.has_focal_length != 0,
		iso              = f.iso,
		exposure         = f.exposure,
		aperture         = f.aperture,
		focal_length     = f.focal_length,
	}
	p := facts.parts(in_facts, rune(decimal))
	defer facts.parts_destroy(&p)
	b := strings.builder_make()
	defer strings.builder_destroy(&b)
	for i in 0 ..< 5 {
		lengths[i] = max(uint)
		if p.present[i] {
			lengths[i] = uint(len(p.text[i]))
			strings.write_string(&b, p.text[i])
		}
	}
	return buf_from(b.buf[:])
}
