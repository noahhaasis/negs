// What the Lab shows (edits.rs's `InspectorReveal` half): each module's
// controls past its basics, and each section's expert modules and rarer
// rows, kept back behind "N more" (round 11, B). View › Show All Controls
// shows everything; a control off its default always shows.
package edits

import "core:strings"
import "../../value"
import facts "../facts"
import ".."

@(rodata)
SECTION_TITLES := [state.Section]cstring {
	.Camera = "Camera",
	.Film   = "Film",
	.Print  = "Print",
	.Scan   = "Scan & Color",
}

// The `more_adjustments` key of a section's "N more", in `allocator`.
section_key :: proc(section: state.Section, allocator := context.allocator) -> string {
	return strings.concatenate({"section:", string(SECTION_TITLES[section])}, allocator)
}

// Whether a section's extra modules and rows show.
shows_section :: proc(f: ^facts.Facts, section: state.Section) -> bool {
	if f.input.prefs.lab_all_controls {
		return true
	}
	buf: [32]u8
	b := strings.builder_from_bytes(buf[:])
	strings.write_string(&b, "section:")
	strings.write_string(&b, string(SECTION_TITLES[section]))
	return facts.revealed(f, strings.to_string(b))
}

// A module shows when it has basics, its section's extras show, or
// something in it changed.
group_shows :: proc(f: ^facts.Facts, index: int) -> bool {
	g := state.ADJUSTMENT_GROUPS[index]
	return g.basic > 0 || shows_section(f, g.section) || group_count(index, f) > 0
}

// Whether a module's control at `place` shows: a basic, revealed, or changed.
control_shows :: proc(f: ^facts.Facts, index: int, place: int) -> bool {
	g := state.ADJUSTMENT_GROUPS[index]
	return f.input.prefs.lab_all_controls || facts.revealed(f, string(g.title)) || place < g.basic || control_modified(g.controls[place], f)
}

// Whether a control draws at all: a B&W film has one grain lane, not three.
@(private)
drawn :: proc(f: ^facts.Facts, c: state.Adjustment) -> bool {
	return !(state.color_only(string(c.path)) && facts.film_is_bw(f))
}

More_Action :: enum u32 {
	Reveal,
	Conceal,
	// Turns View › Show All Controls off.
	Show_Fewer,
}

// A "N more" line: its words, what a click does and the key it reveals or
// conceals (in the allocator).
More :: struct {
	text:   string,
	action: More_Action,
	key:    string,
}

@(private)
plural :: proc "contextless" (n: int) -> string {
	return "" if n == 1 else "s"
}

// A module's "N more controls…", or once clicked "Hide N controls".
module_more :: proc(f: ^facts.Facts, index: int, allocator := context.allocator) -> (More, bool) {
	g := state.ADJUSTMENT_GROUPS[index]
	drawn_n, shown := 0, 0
	for c, i in g.controls {
		if drawn(f, c) {
			drawn_n += 1
			if control_shows(f, index, i) {
				shown += 1
			}
		}
	}
	more := drawn_n - shown
	if more > 0 {
		b := strings.builder_make(0, 24, allocator)
		strings.write_int(&b, more)
		strings.write_string(&b, " more control")
		strings.write_string(&b, plural(more))
		strings.write_string(&b, "…")
		return More{strings.to_string(b), .Reveal, strings.clone(string(g.title), allocator)}, true
	}
	if f.input.prefs.lab_all_controls || !facts.revealed(f, string(g.title)) {
		return {}, false
	}
	fewer := 0
	for c, i in g.controls {
		if drawn(f, c) && i >= g.basic && !control_modified(c, f) {
			fewer += 1
		}
	}
	if fewer == 0 {
		return {}, false
	}
	b := strings.builder_make(0, 24, allocator)
	strings.write_string(&b, "Hide ")
	strings.write_int(&b, fewer)
	strings.write_string(&b, " control")
	strings.write_string(&b, plural(fewer))
	return More{strings.to_string(b), .Conceal, strings.clone(string(g.title), allocator)}, true
}

// A section's "2 more: Scanner, Color Management…" for the modules and
// rows it keeps back; once clicked, "Hide Scanner, Color Management".
section_more :: proc(f: ^facts.Facts, section: state.Section, allocator := context.allocator) -> (More, bool) {
	scanning := value.boolean(facts.value_at(f, state.SCAN_FILM))
	loose: string
	switch section {
	case .Camera:
		if !facts.is_modified(f, "camera.lens_blur_um") {
			loose = "Lens blur"
		}
	case .Film:
	case .Print:
		if scanning {
			return {}, false
		}
		loose = "Scan film directly"
	case .Scan:
		if output_count(f) == 0 {
			loose = "Output"
		}
	}
	names := make([dynamic]string, 0, 8, context.temp_allocator)
	if !shows_section(f, section) {
		for g, i in state.ADJUSTMENT_GROUPS {
			if g.section == section && !group_shows(f, i) {
				append(&names, string(g.title))
			}
		}
		if len(loose) > 0 {
			append(&names, loose)
		}
		if len(names) == 0 {
			return {}, false
		}
		b := strings.builder_make(0, 64, allocator)
		strings.write_int(&b, len(names))
		strings.write_string(&b, " more: ")
		join(&b, names[:])
		strings.write_string(&b, "…")
		return More{strings.to_string(b), .Reveal, section_key(section, allocator)}, true
	}
	key := section_key(section, context.temp_allocator)
	if f.input.prefs.lab_all_controls || !facts.revealed(f, key) {
		return {}, false
	}
	for g, i in state.ADJUSTMENT_GROUPS {
		if g.section == section && g.basic == 0 && group_count(i, f) == 0 {
			append(&names, string(g.title))
		}
	}
	if len(loose) > 0 {
		append(&names, loose)
	}
	if len(names) == 0 {
		return {}, false
	}
	b := strings.builder_make(0, 64, allocator)
	strings.write_string(&b, "Hide ")
	join(&b, names[:])
	return More{strings.to_string(b), .Conceal, section_key(section, allocator)}, true
}

@(private)
join :: proc(b: ^strings.Builder, names: []string) {
	for n, i in names {
		if i > 0 {
			strings.write_string(b, ", ")
		}
		strings.write_string(b, n)
	}
}
