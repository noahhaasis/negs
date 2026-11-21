// The themes' C ABI (negs_core.h, "Theme").
package negs_core

import "../theme"

Theme :: struct {
	hex:         theme.Hex,
	film:        theme.Hex,
	dark:        u8,
	highlighter: u8,
	plain:       u8,
	reserved:    u8,
	id:          u32,
	key:         cstring,
	title:       cstring,
}

Theme_Constants :: struct {
	highlighter_ink:     u32,
	crop_shade:          u32,
	highlighter_opacity: f64,
	selection_opacity:   f64,
	crop_shade_opacity:  f64,
	disabled_opacity:    f64,
	strip_dim:           f64,
	shadow_dark:         f64,
	shadow_light:        f64,
	dimming_dark:        [4]f64,
	dimming_light:       [4]f64,
}

@(export, link_name = "negs_theme_count")
theme_count :: proc "c" () -> u32 {
	return len(theme.Id)
}

@(export, link_name = "negs_theme_get")
theme_get :: proc "c" (id: u32, out: ^Theme) -> u8 {
	if id >= len(theme.Id) || out == nil {
		return 0
	}
	t := theme.Id(id)
	tokens := theme.TOKENS[t]
	out^ = Theme {
		hex         = tokens.hex,
		film        = tokens.film,
		dark        = u8(tokens.dark),
		highlighter = u8(tokens.highlighter),
		plain       = u8(tokens.plain),
		id          = id,
		key         = theme.KEYS[t],
		title       = theme.TITLES[t],
	}
	return 1
}

@(export, link_name = "negs_theme_hint")
theme_hint :: proc "c" (id: u32, platform: u32) -> cstring {
	if id >= len(theme.Id) {
		return ""
	}
	p := theme.Platform(platform) if platform < len(theme.Platform) else .Linux
	return theme.HINTS[p][theme.Id(id)]
}

@(export, link_name = "negs_theme_parse")
theme_parse :: proc "c" (text: [^]u8, n: uint) -> i32 {
	context = core_context()
	id, ok := theme.parse(host_string(text, n))
	return i32(id) if ok else -1
}

@(export, link_name = "negs_theme_luminance")
theme_luminance :: proc "c" (rgb: u32) -> f64 {
	return theme.luminance(rgb)
}

@(export, link_name = "negs_theme_contrast")
theme_contrast :: proc "c" (a, b: u32) -> f64 {
	return theme.contrast(a, b)
}

@(export, link_name = "negs_theme_mix")
theme_mix :: proc "c" (a, b: u32, t: f64) -> u32 {
	return theme.mix(a, b, t)
}

@(export, link_name = "negs_theme_constants")
theme_constants :: proc "c" (out: ^Theme_Constants) {
	if out == nil {
		return
	}
	out^ = {
		highlighter_ink     = theme.HIGHLIGHTER_INK,
		crop_shade          = theme.CROP_SHADE,
		highlighter_opacity = theme.HIGHLIGHTER_OPACITY,
		selection_opacity   = theme.SELECTION_OPACITY,
		crop_shade_opacity  = theme.CROP_SHADE_OPACITY,
		disabled_opacity    = theme.DISABLED_OPACITY,
		strip_dim           = theme.STRIP_DIM,
		shadow_dark         = theme.SHADOW_DARK,
		shadow_light        = theme.SHADOW_LIGHT,
		dimming_dark        = theme.DIMMING_DARK,
		dimming_light       = theme.DIMMING_LIGHT,
	}
}
