// Phase 3b's structs in negs_core.h: size_of / offset_of by C name
// (see layout.odin). Picture rules: crop, border, exact fit, thumbnail compose, histogram, negative (core/src/rules).
package negs_core

import "../rules/border"
import "../rules/crop"
import "../rules/histogram"

@(private)
layout_3b := []Layout_Entry {
	{"negs_size_t", size_of(crop.Size)},
	{"negs_size_t.width", i64(offset_of(crop.Size, width))},
	{"negs_size_t.height", i64(offset_of(crop.Size, height))},
	{"negs_crop_settings_t", size_of(crop.Settings)},
	{"negs_crop_settings_t.rect", i64(offset_of(crop.Settings, rect))},
	{"negs_crop_settings_t.custom", i64(offset_of(crop.Settings, custom))},
	{"negs_crop_settings_t.angle", i64(offset_of(crop.Settings, angle))},
	{"negs_crop_settings_t.aspect", i64(offset_of(crop.Settings, aspect))},
	{"negs_crop_settings_t.reserved", i64(offset_of(crop.Settings, reserved))},
	{"negs_orientation_t", size_of(crop.Orientation)},
	{"negs_orientation_t.turns", i64(offset_of(crop.Orientation, turns))},
	{"negs_orientation_t.flip", i64(offset_of(crop.Orientation, flip))},
	{"negs_orientation_t.reserved", i64(offset_of(crop.Orientation, reserved))},
	{"negs_crop_handle_t", size_of(crop.Handle)},
	{"negs_crop_handle_t.x", i64(offset_of(crop.Handle, x))},
	{"negs_crop_handle_t.y", i64(offset_of(crop.Handle, y))},
	{"negs_crop_session_t", size_of(crop.Session)},
	{"negs_crop_session_t.rect", i64(offset_of(crop.Session, rect))},
	{"negs_crop_session_t.custom", i64(offset_of(crop.Session, custom))},
	{"negs_crop_session_t.photo", i64(offset_of(crop.Session, photo))},
	{"negs_crop_session_t.intended", i64(offset_of(crop.Session, intended))},
	{"negs_crop_session_t.angle", i64(offset_of(crop.Session, angle))},
	{"negs_crop_session_t.rotation", i64(offset_of(crop.Session, rotation))},
	{"negs_crop_session_t.aspect", i64(offset_of(crop.Session, aspect))},
	{"negs_crop_session_t.portrait", i64(offset_of(crop.Session, portrait))},
	{"negs_crop_session_t.flip", i64(offset_of(crop.Session, flip))},
	{"negs_crop_session_t.straightening", i64(offset_of(crop.Session, straightening))},
	{"negs_crop_session_t.has_intended", i64(offset_of(crop.Session, has_intended))},
	{"negs_crop_session_t.reserved", i64(offset_of(crop.Session, reserved))},
	{"negs_border_settings_t", size_of(border.Settings)},
	{"negs_border_settings_t.width", i64(offset_of(border.Settings, width))},
	{"negs_border_settings_t.film_width", i64(offset_of(border.Settings, film_width))},
	{"negs_border_settings_t.line", i64(offset_of(border.Settings, line))},
	{"negs_border_settings_t.style", i64(offset_of(border.Settings, style))},
	{"negs_border_settings_t.color", i64(offset_of(border.Settings, color))},
	{"negs_border_settings_t.line_color", i64(offset_of(border.Settings, line_color))},
	{"negs_border_settings_t.enabled", i64(offset_of(border.Settings, enabled))},
	{"negs_border_settings_t.export_border", i64(offset_of(border.Settings, export))},
	{"negs_border_settings_t.instagram", i64(offset_of(border.Settings, instagram))},
	{"negs_border_settings_t.reserved", i64(offset_of(border.Settings, reserved))},
	{"negs_histogram_t", size_of(histogram.Histogram)},
	{"negs_histogram_t.red", i64(offset_of(histogram.Histogram, red))},
	{"negs_histogram_t.green", i64(offset_of(histogram.Histogram, green))},
	{"negs_histogram_t.blue", i64(offset_of(histogram.Histogram, blue))},
	{"negs_scan_light_t", size_of(Scan_Light_C)},
	{"negs_scan_light_t.value", i64(offset_of(Scan_Light_C, value))},
	{"negs_scan_light_t.has", i64(offset_of(Scan_Light_C, has))},
	{"negs_scan_light_t.reserved", i64(offset_of(Scan_Light_C, reserved))},
}
