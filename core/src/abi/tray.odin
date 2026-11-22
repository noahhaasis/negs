// The film and paper tray's rules (negs_core.h, "Phase 3a: Tray"). The
// row comes in as the caller's sections (negs_tray_section_t, borrowed for
// the call, laid out as tray.Section); words go into the caller's buffers.
package negs_core

import "base:runtime"
import "../rules/tray"

Tray_Constants :: struct {
	tile, picture, tile_gap, group_gap, title, title_gap, inset, top: f64,
	name_gap, name_line, row, foot, height, star, star_inset:         f64,
	wheel_notch, glide_ms:                                            f64,
	favourites, scan, your_negative, keep_paper_label:                cstring,
}

@(export, link_name = "negs_tray_constants")
tray_constants :: proc "c" (out: ^Tray_Constants) {
	if out == nil {
		return
	}
	out^ = {
		tile             = tray.TILE,
		picture          = tray.PICTURE,
		tile_gap         = tray.TILE_GAP,
		group_gap        = tray.GROUP_GAP,
		title            = tray.TITLE,
		title_gap        = tray.TITLE_GAP,
		inset            = tray.INSET,
		top              = tray.TOP,
		name_gap         = tray.NAME_GAP,
		name_line        = tray.NAME_LINE,
		row              = tray.ROW,
		foot             = tray.FOOT,
		height           = tray.HEIGHT,
		star             = tray.STAR,
		star_inset       = tray.STAR_INSET,
		wheel_notch      = tray.WHEEL_NOTCH,
		glide_ms         = tray.GLIDE_MS,
		favourites       = tray.FAVOURITES,
		scan             = tray.SCAN,
		your_negative    = tray.YOUR_NEGATIVE,
		keep_paper_label = tray.KEEP_PAPER_LABEL,
	}
}

@(export, link_name = "negs_tray_searching")
tray_searching :: proc "c" (query: [^]u8, n: uint) -> u8 {
	return 1 if tray.searching(host_string(query, n)) else 0
}

@(export, link_name = "negs_tray_sections")
tray_sections :: proc "c" (counts: [^]uint, n: uint, favourite_count: uint, query: [^]u8, query_len: uint, kept: [^]uint, favourites_lead: ^u8) -> uint {
	k, lead := tray.sections(host_slice_3a(counts, n), favourite_count, host_string(query, query_len), host_slice_3a(kept, n))
	if favourites_lead != nil {
		favourites_lead^ = 1 if lead else 0
	}
	return uint(k)
}

@(export, link_name = "negs_tray_with_scan")
tray_with_scan :: proc "c" (paper, read_as: u8, query: [^]u8, query_len: uint) -> u8 {
	return 1 if tray.with_scan(paper != 0, read_as != 0, host_string(query, query_len)) else 0
}

@(export, link_name = "negs_tray_places")
tray_places :: proc "c" (sections: [^]tray.Section, n: uint, out: [^]tray.Place, cap: uint) -> uint {
	s := host_slice_3a(sections, n)
	total := 0
	for section in s {
		total += len(section.stocks)
	}
	if out == nil || uint(total) > cap {
		return uint(total)
	}
	return uint(tray.places(s, out[:total]))
}

@(export, link_name = "negs_tray_step")
tray_step :: proc "c" (current: [^]u8, current_len: uint, in_favourites: u8, offset: i32, places: [^]tray.Place, n: uint) -> i64 {
	return index_3a(tray.step(host_string(current, current_len), in_favourites != 0, offset, host_slice_3a(places, n)))
}

@(export, link_name = "negs_tray_width")
tray_width :: proc "c" (stock_count: uint) -> f64 {
	return tray.width(stock_count)
}

@(export, link_name = "negs_tray_starts")
tray_starts :: proc "c" (sections: [^]tray.Section, n: uint, out: [^]f64) {
	tray.starts(host_slice_3a(sections, n), host_slice_3a(out, n))
}

@(export, link_name = "negs_tray_content_width")
tray_content_width :: proc "c" (sections: [^]tray.Section, n: uint) -> f64 {
	return tray.content_width(host_slice_3a(sections, n))
}

@(export, link_name = "negs_tray_tile_left")
tray_tile_left :: proc "c" (start: f64, index: uint) -> f64 {
	return tray.tile_left(start, index)
}

@(export, link_name = "negs_tray_tile_x")
tray_tile_x :: proc "c" (stock: [^]u8, stock_len: uint, section: uint, sections: [^]tray.Section, n: uint, x: ^f64) -> u8 {
	v, ok := tray.tile_x(host_string(stock, stock_len), section, host_slice_3a(sections, n))
	if x != nil {
		x^ = v
	}
	return 1 if ok else 0
}

@(export, link_name = "negs_tray_section_of")
tray_section_of :: proc "c" (stock: [^]u8, stock_len: uint, sections: [^]tray.Section, n: uint, favourites_first: u8) -> i64 {
	return index_3a(tray.section_of(host_string(stock, stock_len), host_slice_3a(sections, n), favourites_first != 0))
}

@(export, link_name = "negs_tray_current")
tray_current :: proc "c" (scrolled_to: f64, starts: [^]f64, n: uint) -> uint {
	return tray.current(scrolled_to, host_slice_3a(starts, n))
}

@(export, link_name = "negs_tray_centring")
tray_centring :: proc "c" (tile_x, visible, content: f64) -> f64 {
	return tray.centring(tile_x, visible, content)
}

@(export, link_name = "negs_tray_jump")
tray_jump :: proc "c" (start, visible, content: f64) -> f64 {
	return tray.jump(start, visible, content)
}

@(export, link_name = "negs_tray_clamp")
tray_clamp :: proc "c" (x, visible, content: f64) -> f64 {
	return tray.clamp_offset(x, visible, content)
}

@(export, link_name = "negs_tray_reveal")
tray_reveal :: proc "c" (offset: f64, stock: [^]u8, stock_len: uint, in_favourites, centre: u8, sections: [^]tray.Section, n: uint, visible: f64) -> f64 {
	return tray.reveal(offset, host_string(stock, stock_len), in_favourites != 0, centre != 0, host_slice_3a(sections, n), visible)
}

@(export, link_name = "negs_tray_wheel")
tray_wheel :: proc "c" (offset: f64, delta: i32, sideways: u8, visible, content: f64) -> f64 {
	return tray.wheel(offset, delta, sideways != 0, visible, content)
}

@(export, link_name = "negs_tray_glide")
tray_glide :: proc "c" (from, to, elapsed_ms: f64) -> f64 {
	return tray.glide(from, to, elapsed_ms)
}

@(export, link_name = "negs_tray_hit")
tray_hit :: proc "c" (sections: [^]tray.Section, n: uint, x, y: f64, section, index: ^uint) -> u32 {
	kind, s, i := tray.hit(host_slice_3a(sections, n), x, y)
	if section != nil {
		section^ = s
	}
	if index != nil {
		index^ = i
	}
	return u32(kind)
}

@(export, link_name = "negs_tray_star_rect")
tray_star_rect :: proc "c" (left: f64, out: ^[4]f64) {
	if out != nil {
		out^ = tray.star_rect(left)
	}
}

@(export, link_name = "negs_tray_stand_in_size")
tray_stand_in_size :: proc "c" (image_width, image_height: u32, photo_width, photo_height: f64, out: ^[2]f64) {
	if out != nil {
		out^ = tray.stand_in_size({image_width, image_height}, {photo_width, photo_height})
	}
}

// MARK: Words

@(export, link_name = "negs_tray_star_help")
tray_star_help :: proc "c" (favourite: u8) -> cstring {
	return tray.star_help(favourite != 0)
}

@(export, link_name = "negs_tray_star_label")
tray_star_label :: proc "c" (name: [^]u8, name_len: uint, favourite: u8, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(tray.star_label(host_string(name, name_len), favourite != 0, context.temp_allocator), out, cap)
}

@(export, link_name = "negs_tray_tile_value")
tray_tile_value :: proc "c" (selected, before: u8) -> cstring {
	return tray.tile_value(selected != 0, before != 0)
}

@(export, link_name = "negs_tray_index_label")
tray_index_label :: proc "c" (title: [^]u8, title_len: uint, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(tray.index_label(host_string(title, title_len), context.temp_allocator), out, cap)
}

@(export, link_name = "negs_tray_hint")
tray_hint :: proc "c" (has_before: u8, before: [^]u8, before_len: uint, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(tray.hint(host_string(before, before_len), has_before != 0, context.temp_allocator), out, cap)
}

@(export, link_name = "negs_tray_matches")
tray_matches :: proc "c" (count: uint, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(tray.matches(count, context.temp_allocator), out, cap)
}

@(export, link_name = "negs_tray_no_match")
tray_no_match :: proc "c" (query: [^]u8, query_len: uint, paper: u8, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(tray.no_match(host_string(query, query_len), paper != 0, context.temp_allocator), out, cap)
}

@(export, link_name = "negs_tray_done_help")
tray_done_help :: proc "c" (paper: u8) -> cstring {
	return tray.done_help(paper != 0)
}

@(export, link_name = "negs_tray_keep_paper_words")
tray_keep_paper_words :: proc "c" (kept: u8) -> cstring {
	return tray.keep_paper_words(kept != 0)
}

// MARK: A visit, keys, Esc

@(export, link_name = "negs_tray_visit_action")
tray_visit_action :: proc "c" (paper, read_as: u8) -> cstring {
	return tray.visit_action(paper != 0, read_as != 0)
}

@(export, link_name = "negs_tray_visit_tried")
tray_visit_tried :: proc "c" (before_id: [^]u8, before_len: uint, current: [^]u8, current_len: uint) -> u8 {
	return 1 if tray.visit_tried(host_string(before_id, before_len), host_string(current, current_len)) else 0
}

@(export, link_name = "negs_tray_visit_shows_before")
tray_visit_shows_before :: proc "c" (before_id: [^]u8, before_len: uint, stock: [^]u8, stock_len: uint, current: [^]u8, current_len: uint) -> u8 {
	shows := tray.visit_shows_before(host_string(before_id, before_len), host_string(stock, stock_len), host_string(current, current_len))
	return 1 if shows else 0
}

@(export, link_name = "negs_tray_ending")
tray_ending :: proc "c" (keep, unchanged: u8) -> u32 {
	return u32(tray.ending(keep != 0, unchanged != 0))
}

@(export, link_name = "negs_tray_submit")
tray_submit :: proc "c" (query: [^]u8, n: uint) -> u32 {
	return u32(tray.submit(host_string(query, n)))
}

@(export, link_name = "negs_tray_cancel")
tray_cancel :: proc "c" (query: [^]u8, n: uint) -> u32 {
	return u32(tray.cancel(host_string(query, n)))
}

@(export, link_name = "negs_tray_key")
tray_key :: proc "c" (key: u32, command, typing: u8, step: ^i32) -> u32 {
	k := tray.Key(key) if key <= u32(max(tray.Key)) else .Other
	action, s := tray.tray_key(k, command != 0, typing != 0)
	if step != nil {
		step^ = s
	}
	return u32(action)
}

@(export, link_name = "negs_tray_escape")
tray_escape :: proc "c" (c: ^tray.Escape_Context) -> u32 {
	if c == nil {
		return u32(tray.Escape.Nothing)
	}
	return u32(tray.escape(c^))
}
