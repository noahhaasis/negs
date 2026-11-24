// What each inspector tab holds (windows/src/edits.rs, `TabEdits.swift`):
// the controls that differ from their defaults, in the order the tab shows
// them, for the count beside each tab, the pane header's "3 edits", and the
// list a click on a count opens. And what the Lab keeps back behind
// "N more" (`InspectorReveal`).
package edits

import "../../value"
import "../facts"
import ".."

Tab :: enum u32 {
	Prep,
	Lab,
	Output,
}

@(rodata)
TAB_TITLES := [Tab]cstring {
	.Prep   = "Prep",
	.Lab    = "Lab",
	.Output = "Output",
}

// What a count stands for, and what going back does.
Target_Kind :: enum u32 {
	Param,
	// Prep › Match camera brightness, back to its setting for new photos.
	Match_Brightness,
	// Prep › Exposure, back to the match (or 0).
	Exposure,
	// Lab › Scan film directly, back to what the film is.
	Scan_Film,
	// Lab › Output › Colour space, back to the first of the spaces.
	Preview_Space,
	Border,
	// Camera › Film format: the value, and the Custom pick.
	Film_Format,
}

Edit :: struct {
	title:  string,
	value:  string,
	target: Target_Kind,
	// The parameter for .Param (static).
	path:   cstring,
}

@(rodata)
WHITE_BALANCE_PATHS := [?]cstring{"io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint"}
@(rodata)
INPUT_PATHS := [?]cstring {
	"io.input_interpretation.mode",
	"io.input_interpretation.color_space",
	"io.input_interpretation.linear",
	"io.input_color_space",
	"io.input_cctf_decoding",
}
FILM_DEVELOPMENT :: "film_render.development_time"
PRINT_DEVELOPMENT :: "print_render.development_time"
@(rodata)
CAMERA_LOOSE := [?]cstring{"camera.exposure_compensation_ev", "camera.film_format_mm", "camera.lens_blur_um", state.COLOR_FILTER_PATH}
@(rodata)
AUTO_EXPOSURE := [?]cstring{"camera.auto_exposure", "camera.auto_exposure_method"}
CCTF :: "io.output_cctf_encoding"

@(rodata)
SECTIONS := [?]state.Section{.Camera, .Film, .Print, .Scan}

@(private)
control_modified :: proc(c: state.Adjustment, f: ^facts.Facts) -> bool {
	return c.kind != .Balance_Print && c.kind != .Filter_Link && facts.is_modified(f, string(c.path))
}

// A module's count: its switch, then its controls.
group_count :: proc(index: int, f: ^facts.Facts) -> int {
	g := state.ADJUSTMENT_GROUPS[index]
	n := 0
	if len(g.enable) > 0 && facts.is_modified(f, string(g.enable)) {
		n += 1
	}
	for c in g.controls {
		if control_modified(c, f) {
			n += 1
		}
	}
	return n
}

scan_film_modified :: proc(f: ^facts.Facts) -> bool {
	return value.boolean(facts.value_at(f, state.SCAN_FILM)) != facts.film_positive(f)
}

// The Lab's Output module: the preview's colour space and its curve.
output_count :: proc(f: ^facts.Facts) -> int {
	return int(facts.facts_preview_space(f) != facts.COLOR_SPACES[0]) + int(facts.is_modified(f, CCTF))
}

section_count :: proc(section: state.Section, f: ^facts.Facts) -> int {
	n := 0
	for g, i in state.ADJUSTMENT_GROUPS {
		if g.section == section {
			n += group_count(i, f)
		}
	}
	switch section {
	case .Camera:
		for p in CAMERA_LOOSE {
			n += int(facts.is_modified(f, string(p)))
		}
	case .Film:
		n += int(facts.is_modified(f, FILM_DEVELOPMENT))
	case .Print:
		n += int(scan_film_modified(f)) + int(facts.is_modified(f, PRINT_DEVELOPMENT))
	case .Scan:
		n += output_count(f)
	}
	return n
}

brightness_count :: proc(f: ^facts.Facts) -> int {
	n := int(facts.matches(f) != f.input.prefs.match_new_photos) + int(facts.exposure_offset_modified(f))
	for p in AUTO_EXPOSURE {
		n += int(facts.is_modified(f, string(p)))
	}
	return n
}

@(private)
prep_count :: proc(f: ^facts.Facts) -> int {
	n := brightness_count(f)
	for list in ([3][]state.Adjustment{state.PREP_TONE, state.PREP_COLOR, state.PREP_DETAIL}) {
		for c in list {
			n += int(control_modified(c, f))
		}
	}
	if f.input.is_raw {
		for p in WHITE_BALANCE_PATHS {
			n += int(facts.is_modified(f, string(p)))
		}
		if f.input.lens_supported && facts.is_modified(f, state.LENS_CORRECTION) {
			n += 1
		}
	} else {
		for p in INPUT_PATHS {
			n += int(facts.is_modified(f, string(p)))
		}
	}
	return n
}

count :: proc(tab: Tab, f: ^facts.Facts) -> int {
	switch tab {
	case .Prep:
		return prep_count(f)
	case .Lab:
		n := 0
		for s in SECTIONS {
			n += section_count(s, f)
		}
		return n
	case .Output:
		return int(facts.is_modified(f, state.SIZE_TARGET)) + int(facts.border_enabled(f))
	}
	return 0
}
