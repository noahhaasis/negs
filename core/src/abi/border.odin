// The border's C ABI (negs_core.h, "Phase 3b: Picture rules", Border).
package negs_core

import "../rules/border"

@(private = "file")
style_of :: proc "contextless" (s: u32) -> border.Style {
	return border.Style(s) if s <= u32(max(border.Style)) else .Solid
}

@(private = "file")
size2 :: proc "contextless" (out: ^[2]u32, v: [2]u32) {
	if out != nil {
		out^ = v
	}
}

@(export, link_name = "negs_border_style_title")
border_style_title :: proc "c" (style: u32) -> cstring {return border.TITLES[style_of(style)]}

@(export, link_name = "negs_border_style_help")
border_style_help :: proc "c" (style: u32) -> cstring {return border.HELPS[style_of(style)]}

@(export, link_name = "negs_border_style_raw_value")
border_style_raw_value :: proc "c" (style: u32) -> cstring {return border.RAW_VALUES[style_of(style)]}

@(export, link_name = "negs_border_styles")
border_styles :: proc "c" (out: ^[4]u32) {
	if out != nil {
		for s, i in border.STYLES {
			out[i] = u32(s)
		}
	}
}

@(export, link_name = "negs_border_fits")
border_fits :: proc "c" (style: u32, width, height: u32) -> u8 {return u8(border.fits(style_of(style), width, height))}

// -1 when no 35mm format fits.
@(export, link_name = "negs_film_format_of")
border_film_format_of :: proc "c" (width, height: u32) -> i32 {
	f, ok := border.format_of(width, height).?
	return i32(f) if ok else -1
}

@(export, link_name = "negs_film_format_name")
border_film_format_name :: proc "c" (format: u32) -> cstring {
	return border.FORMAT_NAMES[.Half if format == 1 else .Full]
}

@(export, link_name = "negs_border_margin")
border_margin :: proc "c" (width, height: u32, fraction: f64) -> u32 {return border.margin(width, height, fraction)}

@(export, link_name = "negs_border_keyline_width")
border_keyline_width :: proc "c" (width, height: u32, fraction: f64) -> u32 {
	return border.keyline_width(width, height, fraction)
}

@(export, link_name = "negs_border_film_geometry")
border_film_geometry :: proc "c" (width, height: u32, out: ^[2]u32) {size2(out, border.film_geometry(width, height))}

@(export, link_name = "negs_border_instagram_canvas")
border_instagram_canvas :: proc "c" (width, height: u32, style: u32, out: ^[2]u32) {
	size2(out, border.instagram_canvas(width, height, style_of(style)))
}

@(export, link_name = "negs_strip_turned")
border_strip_turned :: proc "c" (format: u32, width, height: u32) -> u8 {
	return u8(border.strip_turned(.Half if format == 1 else .Full, width, height))
}

@(export, link_name = "negs_strip_canvas")
border_strip_canvas :: proc "c" (format: u32, width, height: u32, out: ^[2]u32) {
	size2(out, border.strip_canvas(.Half if format == 1 else .Full, width, height))
}

@(export, link_name = "negs_border_turned")
border_turned :: proc "c" (width, height: u32) -> u8 {return u8(border.turned(width, height))}

@(export, link_name = "negs_border_color_hex")
border_color_hex :: proc "c" (color: ^border.Color, out: ^[8]u8) {
	if color == nil || out == nil {
		return
	}
	h := border.hex(color^)
	for b, i in h {
		out[i] = b
	}
	out[7] = 0
}

@(export, link_name = "negs_border_color_from_hex")
border_color_from_hex :: proc "c" (text: [^]u8, n: uint, out: ^border.Color) -> u8 {
	c, ok := border.from_hex(host_string(text, n)).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = c
	}
	return 1
}

@(export, link_name = "negs_border_color_rgba")
border_color_rgba :: proc "c" (color: ^border.Color, out: ^[4]f32) {
	if color != nil && out != nil {
		out^ = border.rgba(color^)
	}
}

@(private = "file")
settings_of :: proc "contextless" (s: ^border.Settings) -> border.Settings {
	v := s^
	v.style = style_of(u32(v.style))
	return v
}

@(export, link_name = "negs_border_shown")
border_shown :: proc "c" (s: ^border.Settings, width, height: u32) -> u8 {return u8(border.shown(settings_of(s), width, height))}

@(export, link_name = "negs_border_exported")
border_exported :: proc "c" (s: ^border.Settings, width, height: u32) -> u8 {
	return u8(border.exported(settings_of(s), width, height))
}

@(export, link_name = "negs_border_style_width")
border_style_width :: proc "c" (s: ^border.Settings, out: ^f64) -> u8 {
	w, ok := border.style_width(settings_of(s)).?
	if !ok {
		return 0
	}
	if out != nil {
		out^ = w
	}
	return 1
}

@(export, link_name = "negs_border_keyline")
border_keyline :: proc "c" (s: ^border.Settings, width, height: u32) -> u32 {return border.keyline(settings_of(s), width, height)}

@(export, link_name = "negs_border_canvas")
border_canvas :: proc "c" (s: ^border.Settings, width, height: u32, out: ^[2]u32) {
	size2(out, border.canvas(settings_of(s), width, height))
}

@(export, link_name = "negs_border_instagram")
border_instagram :: proc "c" (s: ^border.Settings, width, height: u32, out: ^[2]u32) {
	size2(out, border.instagram(settings_of(s), {width, height}))
}

@(export, link_name = "negs_border_document")
border_document :: proc "c" (s: ^border.Settings, width, height: u32, out: ^[2]u32) {
	size2(out, border.document(settings_of(s), width, height))
}

@(export, link_name = "negs_border_arguments")
border_arguments :: proc "c" (s: ^border.Settings) -> Buf {
	context = core_context()
	text := border.arguments(settings_of(s))
	defer delete(text)
	return buf_text(text)
}

@(export, link_name = "negs_rust_display_f64")
border_rust_display :: proc "c" (v: f64) -> Buf {
	context = core_context()
	text := border.rust_display(v)
	defer delete(text)
	return buf_text(text)
}
