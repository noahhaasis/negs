// edits.rs's C ABI (negs_core.h, "Phase 4a"): the edit counts and lists
// and the Lab's "N more", read from a facts handle. Lists and words come
// back in the handle's scratch, borrowed until the next call on it.
package negs_core

import "base:runtime"
import "core:strings"
import "../state"
import "../state/edits"
import facts "../state/facts"

// negs_edit_t.
Edit_C :: struct {
	title, value: Str_C,
	target:       u32,
	// The parameter for NEGS_TARGET_PARAM (static); NULL otherwise.
	path:         cstring,
}

// negs_more_t.
More_C :: struct {
	text, key: Str_C,
	action:    u32,
}

@(private)
valid_tab :: proc "contextless" (tab: u32) -> bool {
	return tab <= u32(max(edits.Tab))
}

@(private)
valid_section :: proc "contextless" (section: u32) -> bool {
	return section <= u32(max(state.Section))
}

@(private)
valid_group :: proc "contextless" (index: uint) -> bool {
	return index < uint(len(state.ADJUSTMENT_GROUPS))
}

@(export, link_name = "negs_edits_count")
edits_count :: proc "c" (h: ^Facts_Handle, tab: u32) -> uint {
	context = core_context()
	if h == nil || !valid_tab(tab) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return uint(edits.count(edits.Tab(tab), &h.f))
}

// The tab's edits, in its order; borrowed until the next call on `h`.
// `decimal` is the locale's separator (a Unicode scalar).
@(export, link_name = "negs_edits_list")
edits_list :: proc "c" (h: ^Facts_Handle, tab: u32, decimal: u32, count: ^uint) -> [^]Edit_C {
	context = core_context()
	if count != nil {
		count^ = 0
	}
	if h == nil || !valid_tab(tab) {
		return nil
	}
	context.temp_allocator = facts.begin_call(&h.f)
	list := edits.edits(edits.Tab(tab), &h.f, decimal_rune(decimal), context.temp_allocator)
	if len(list) == 0 {
		return nil
	}
	out := make([]Edit_C, len(list), context.temp_allocator)
	for e, i in list {
		out[i] = Edit_C{str_c(e.title), str_c(e.value), u32(e.target), e.path}
	}
	if count != nil {
		count^ = uint(len(out))
	}
	return raw_data(out)
}

@(export, link_name = "negs_edits_group_count")
edits_group_count :: proc "c" (h: ^Facts_Handle, index: uint) -> uint {
	context = core_context()
	if h == nil || !valid_group(index) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return uint(edits.group_count(int(index), &h.f))
}

@(export, link_name = "negs_edits_section_count")
edits_section_count :: proc "c" (h: ^Facts_Handle, section: u32) -> uint {
	context = core_context()
	if h == nil || !valid_section(section) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return uint(edits.section_count(state.Section(section), &h.f))
}

// 0 brightness_count, 1 output_count, 2 scan_film_modified.
@(export, link_name = "negs_edits_other_count")
edits_other_count :: proc "c" (h: ^Facts_Handle, which: u32) -> uint {
	context = core_context()
	if h == nil {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	switch which {
	case 0:
		return uint(edits.brightness_count(&h.f))
	case 1:
		return uint(edits.output_count(&h.f))
	case 2:
		return uint(edits.scan_film_modified(&h.f))
	}
	return 0
}

@(export, link_name = "negs_edits_shows_section")
edits_shows_section :: proc "c" (h: ^Facts_Handle, section: u32) -> u8 {
	context = core_context()
	if h == nil || !valid_section(section) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return u8(edits.shows_section(&h.f, state.Section(section)))
}

@(export, link_name = "negs_edits_group_shows")
edits_group_shows :: proc "c" (h: ^Facts_Handle, index: uint) -> u8 {
	context = core_context()
	if h == nil || !valid_group(index) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return u8(edits.group_shows(&h.f, int(index)))
}

@(export, link_name = "negs_edits_control_shows")
edits_control_shows :: proc "c" (h: ^Facts_Handle, index: uint, place: uint) -> u8 {
	context = core_context()
	if h == nil || !valid_group(index) || place >= uint(len(state.ADJUSTMENT_GROUPS[index].controls)) {
		return 0
	}
	context.temp_allocator = facts.begin_call(&h.f)
	return u8(edits.control_shows(&h.f, int(index), int(place)))
}

@(private)
more_c :: proc "contextless" (m: edits.More, ok: bool, out: ^More_C) -> u8 {
	if out != nil {
		out^ = More_C{str_c(m.text), str_c(m.key), u32(m.action)} if ok else {}
	}
	return u8(ok)
}

// A module's "N more controls…" or "Hide N controls"; borrowed until the
// next call on `h`. 0 for none.
@(export, link_name = "negs_edits_module_more")
edits_module_more :: proc "c" (h: ^Facts_Handle, index: uint, out: ^More_C) -> u8 {
	context = core_context()
	if h == nil || !valid_group(index) {
		return more_c({}, false, out)
	}
	context.temp_allocator = facts.begin_call(&h.f)
	m, ok := edits.module_more(&h.f, int(index), context.temp_allocator)
	return more_c(m, ok, out)
}

@(export, link_name = "negs_edits_section_more")
edits_section_more :: proc "c" (h: ^Facts_Handle, section: u32, out: ^More_C) -> u8 {
	context = core_context()
	if h == nil || !valid_section(section) {
		return more_c({}, false, out)
	}
	context.temp_allocator = facts.begin_call(&h.f)
	m, ok := edits.section_more(&h.f, state.Section(section), context.temp_allocator)
	return more_c(m, ok, out)
}

// Static; NULL past the sections.
@(export, link_name = "negs_edits_section_title")
edits_section_title :: proc "c" (section: u32) -> cstring {
	if !valid_section(section) {
		return nil
	}
	return edits.SECTION_TITLES[state.Section(section)]
}

// Static; NULL past the tabs.
@(export, link_name = "negs_edits_tab_title")
edits_tab_title :: proc "c" (tab: u32) -> cstring {
	if !valid_tab(tab) {
		return nil
	}
	return edits.TAB_TITLES[edits.Tab(tab)]
}

// edits.rs's `words`, into the caller's buffer.
@(export, link_name = "negs_edits_words")
edits_words :: proc "c" (text: [^]u8, n: uint, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	w := edits.words(host_string(text, n), context.temp_allocator)
	return into(transmute([]u8)w, out, cap)
}

// The edit lists' path tables: 0 WHITE_BALANCE_PATHS, 1 INPUT_PATHS,
// 2 CAMERA_LOOSE, 3 the two development times (film, print). Static.
@(export, link_name = "negs_edits_paths")
edits_paths :: proc "c" (which: u32, count: ^uint) -> [^]cstring {
	@(static, rodata)
	DEVELOPMENT := [2]cstring{edits.FILM_DEVELOPMENT, edits.PRINT_DEVELOPMENT}
	list: []cstring
	switch which {
	case 0:
		list = edits.WHITE_BALANCE_PATHS[:]
	case 1:
		list = edits.INPUT_PATHS[:]
	case 2:
		list = edits.CAMERA_LOOSE[:]
	case 3:
		list = DEVELOPMENT[:]
	}
	if count != nil {
		count^ = uint(len(list))
	}
	return raw_data(list) if len(list) > 0 else nil
}

// edits.rs's `Format::number` (up to two decimals) or, `signed` 1,
// `Format::signed`, into the caller's buffer.
@(export, link_name = "negs_edits_number")
edits_number :: proc "c" (v: f64, decimal: u32, signed: u8, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	buf: [512]u8
	b := strings.builder_from_bytes(buf[:])
	if signed != 0 {
		edits.write_signed(&b, v, decimal_rune(decimal))
	} else {
		edits.write_number(&b, v, decimal_rune(decimal))
	}
	return into(b.buf[:], out, cap)
}
