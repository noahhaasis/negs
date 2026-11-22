// The film strip's and the grid's rules (negs_core.h, "Phase 3a: Sheet"):
// plain values in and out, layouts written into the caller's arrays, words
// into the caller's buffers. Nothing here keeps state or returns memory.
package negs_core

import "base:runtime"
import "../library"
import "../rules/sheet"

Sheet_Constants :: struct {
	strip, strip_top, strip_rebate, strip_gap, picture, status, band:            f64,
	strip_dim, strip_hover, strip_pressed, pencil_width, pencil_drop:            f64,
	fallback_ratio, strip_margin:                                                f64,
	leader_width, leader_height, leader_raised, leader_slant, glide_ms:          f64,
	grid_gap, grid_top, grid_side, grid_bottom, grid_size_min, grid_size_max:    f64,
	grid_default, grid_size_step, grid_margin, sentence, foot:                   f64,
	box_margin, selection_outset:                                                f64,
	fade_ms, fade_rise, wheel_step, wheel_tau_ms:                                f64,
	bar_width, bar_wide, bar_inset, bar_min, bar_reach, bar_shown_ms, bar_fade_ms: f64,
	prefetch_reach, header_batch:                                                u64,
	edited_long_side, render_long_side:                                          u32,
	edited_limit, renders_limit:                                                 u64,
}

// negs_sheet_filter_t: the library's filter, flat.
Sheet_Filter :: struct {
	// 0 any, 1 at least `stars`, 2 unrated.
	rating: u8,
	stars:  u8,
	// Bit 0 picks, 1 unflagged, 2 rejects; none shows every one.
	flags:  u8,
	// 0 any, 1 edited, 2 unedited.
	edits:  u8,
}

// negs_sheet_part_t: a part of the sentence, its text in the caller's buffer.
Sheet_Part :: struct {
	kind:   u32,
	on:     u8,
	offset: uint,
	len:    uint,
}

@(export, link_name = "negs_sheet_constants")
sheet_constants :: proc "c" (out: ^Sheet_Constants) {
	if out == nil {
		return
	}
	out^ = {
		strip            = sheet.STRIP,
		strip_top        = sheet.STRIP_TOP,
		strip_rebate     = sheet.STRIP_REBATE,
		strip_gap        = sheet.STRIP_GAP,
		picture          = sheet.PICTURE,
		status           = sheet.STATUS,
		band             = sheet.BAND,
		strip_dim        = sheet.STRIP_DIM,
		strip_hover      = sheet.STRIP_HOVER,
		strip_pressed    = sheet.STRIP_PRESSED,
		pencil_width     = sheet.PENCIL_WIDTH,
		pencil_drop      = sheet.PENCIL_DROP,
		fallback_ratio   = sheet.FALLBACK_RATIO,
		strip_margin     = sheet.STRIP_MARGIN,
		leader_width     = sheet.LEADER_WIDTH,
		leader_height    = sheet.LEADER_HEIGHT,
		leader_raised    = sheet.LEADER_RAISED,
		leader_slant     = sheet.LEADER_SLANT,
		glide_ms         = sheet.GLIDE_MS,
		grid_gap         = sheet.GRID_GAP,
		grid_top         = sheet.GRID_TOP,
		grid_side        = sheet.GRID_SIDE,
		grid_bottom      = sheet.GRID_BOTTOM,
		grid_size_min    = sheet.GRID_SIZE_MIN,
		grid_size_max    = sheet.GRID_SIZE_MAX,
		grid_default     = sheet.GRID_DEFAULT,
		grid_size_step   = sheet.GRID_SIZE_STEP,
		grid_margin      = sheet.GRID_MARGIN,
		sentence         = sheet.SENTENCE,
		foot             = sheet.FOOT,
		box_margin       = sheet.BOX_MARGIN,
		selection_outset = sheet.SELECTION_OUTSET,
		fade_ms          = sheet.FADE_MS,
		fade_rise        = sheet.FADE_RISE,
		wheel_step       = sheet.WHEEL_STEP,
		wheel_tau_ms     = sheet.WHEEL_TAU_MS,
		bar_width        = sheet.BAR_WIDTH,
		bar_wide         = sheet.BAR_WIDE,
		bar_inset        = sheet.BAR_INSET,
		bar_min          = sheet.BAR_MIN,
		bar_reach        = sheet.BAR_REACH,
		bar_shown_ms     = sheet.BAR_SHOWN_MS,
		bar_fade_ms      = sheet.BAR_FADE_MS,
		prefetch_reach   = sheet.PREFETCH_REACH,
		header_batch     = sheet.HEADER_BATCH,
		edited_long_side = sheet.EDITED_LONG_SIDE,
		render_long_side = sheet.RENDER_LONG_SIDE,
		edited_limit     = sheet.EDITED_LIMIT,
		renders_limit    = sheet.RENDERS_LIMIT,
	}
}

@(export, link_name = "negs_sheet_reject_opacity")
sheet_reject_opacity :: proc "c" (in_strip, current: u8) -> f64 {
	return sheet.reject_opacity(in_strip != 0, current != 0)
}

@(export, link_name = "negs_sheet_strip_opacity")
sheet_strip_opacity :: proc "c" (current, selected, rejected, hovered, pressed: u8) -> f64 {
	return sheet.strip_opacity(current != 0, selected != 0, rejected != 0, hovered != 0, pressed != 0)
}

// MARK: The strip

@(export, link_name = "negs_sheet_kept_ratio")
sheet_kept_ratio :: proc "c" (has_known: u8, known, thumbnail: f64) -> f64 {
	return sheet.kept_ratio(known, has_known != 0, thumbnail)
}

@(export, link_name = "negs_sheet_frame_width")
sheet_frame_width :: proc "c" (has_ratio: u8, ratio: f64) -> f64 {
	return sheet.frame_width(ratio, has_ratio != 0)
}

@(export, link_name = "negs_sheet_strip_layout")
sheet_strip_layout :: proc "c" (widths: [^]f64, count: uint, xs: [^]f64) -> f64 {
	return sheet.strip_layout(host_slice_3a(widths, count), host_slice_3a(xs, count))
}

@(export, link_name = "negs_sheet_strip_middle")
sheet_strip_middle :: proc "c" (x, width: f64) -> f64 {
	return sheet.strip_middle(x, width)
}

@(export, link_name = "negs_sheet_strip_indices")
sheet_strip_indices :: proc "c" (xs, widths: [^]f64, count: uint, min_x, max_x: f64, start, end: ^uint) {
	s, e := sheet.strip_indices(host_slice_3a(xs, count), host_slice_3a(widths, count), min_x, max_x)
	if start != nil {
		start^ = uint(s)
	}
	if end != nil {
		end^ = uint(e)
	}
}

@(export, link_name = "negs_sheet_strip_hit")
sheet_strip_hit :: proc "c" (xs, widths: [^]f64, count: uint, x: f64) -> i64 {
	return index_3a(sheet.strip_hit(host_slice_3a(xs, count), host_slice_3a(widths, count), x))
}

@(export, link_name = "negs_sheet_strip_index_at")
sheet_strip_index_at :: proc "c" (xs, widths: [^]f64, count: uint, x: f64) -> i64 {
	return index_3a(sheet.strip_index_at(host_slice_3a(xs, count), host_slice_3a(widths, count), x))
}

@(export, link_name = "negs_sheet_centred")
sheet_centred :: proc "c" (middle, visible, content: f64) -> f64 {
	return sheet.centred(middle, visible, content)
}

@(export, link_name = "negs_sheet_hold")
sheet_hold :: proc "c" (old_middle, new_middle, new_width, scroll, visible: f64) -> f64 {
	return sheet.hold(old_middle, new_middle, new_width, scroll, visible)
}

@(export, link_name = "negs_sheet_smoothstep")
sheet_smoothstep :: proc "c" (t: f64) -> f64 {
	return sheet.smoothstep(t)
}

// MARK: The grid

@(export, link_name = "negs_sheet_clamp_size")
sheet_clamp_size :: proc "c" (size: f64) -> f64 {
	return sheet.clamp_size(size)
}

@(export, link_name = "negs_sheet_layout")
sheet_layout :: proc "c" (ratios: [^]f64, count: uint, width, target: f64, frames: [^]sheet.Rect, rows: [^]sheet.Row, row_count: ^uint) -> f64 {
	n, height := sheet.layout(host_slice_3a(ratios, count), width, target, host_slice_3a(frames, count), host_slice_3a(rows, count))
	if row_count != nil {
		row_count^ = uint(n)
	}
	return height
}

@(export, link_name = "negs_sheet_row_of")
sheet_row_of :: proc "c" (rows: [^]sheet.Row, row_count, frame_count, index: uint) -> i64 {
	return index_3a(sheet.row_of(host_slice_3a(rows, row_count), int(frame_count), index))
}

@(export, link_name = "negs_sheet_vertical")
sheet_vertical :: proc "c" (frames: [^]sheet.Rect, frame_count: uint, rows: [^]sheet.Row, row_count, index: uint, by: i64) -> i64 {
	return index_3a(sheet.vertical(host_slice_3a(frames, frame_count), host_slice_3a(rows, row_count), index, int(by)))
}

@(export, link_name = "negs_sheet_indices_in")
sheet_indices_in :: proc "c" (frames: [^]sheet.Rect, frame_count: uint, rows: [^]sheet.Row, row_count: uint, rect: sheet.Rect, out: [^]uint) -> uint {
	return uint(sheet.indices_in(host_slice_3a(frames, frame_count), host_slice_3a(rows, row_count), rect, host_slice_3a(out, frame_count)))
}

@(export, link_name = "negs_sheet_indices")
sheet_indices :: proc "c" (rows: [^]sheet.Row, row_count: uint, min_y, max_y: f64, start, end: ^uint) {
	s, e := sheet.indices(host_slice_3a(rows, row_count), min_y, max_y)
	if start != nil {
		start^ = s
	}
	if end != nil {
		end^ = e
	}
}

@(export, link_name = "negs_sheet_hit")
sheet_hit :: proc "c" (frames: [^]sheet.Rect, frame_count: uint, rows: [^]sheet.Row, row_count: uint, x, y: f64) -> i64 {
	return index_3a(sheet.hit(host_slice_3a(frames, frame_count), host_slice_3a(rows, row_count), x, y))
}

@(export, link_name = "negs_sheet_nearest")
sheet_nearest :: proc "c" (frames: [^]sheet.Rect, frame_count: uint, rows: [^]sheet.Row, row_count: uint, x, y: f64) -> i64 {
	return index_3a(sheet.nearest(host_slice_3a(frames, frame_count), host_slice_3a(rows, row_count), x, y))
}

@(export, link_name = "negs_sheet_centre_row")
sheet_centre_row :: proc "c" (row: sheet.Row, height, viewport: f64) -> f64 {
	return sheet.centre_row(row, height, viewport)
}

@(export, link_name = "negs_sheet_reveal")
sheet_reveal :: proc "c" (frame: sheet.Rect, height, scroll, viewport: f64) -> f64 {
	return sheet.reveal(frame, height, scroll, viewport)
}

@(export, link_name = "negs_sheet_keep_anchor")
sheet_keep_anchor :: proc "c" (frame: sheet.Rect, height, down, from_top, viewport: f64) -> f64 {
	return sheet.keep_anchor(frame, height, down, from_top, viewport)
}

// MARK: Thumbnails

@(export, link_name = "negs_sheet_thumb_pixels")
sheet_thumb_pixels :: proc "c" (size: u32) -> u32 {
	if size > u32(max(sheet.Thumb_Size)) {
		return 0
	}
	return sheet.thumb_pixels(sheet.Thumb_Size(size))
}

@(export, link_name = "negs_sheet_thumb_grid")
sheet_thumb_grid :: proc "c" (points, scale: f64) -> u32 {
	return u32(sheet.thumb_grid(points, scale))
}

@(export, link_name = "negs_sheet_thumb_for_rows")
sheet_thumb_for_rows :: proc "c" (size, scale: f64) -> u32 {
	return u32(sheet.thumb_for_rows(size, scale))
}

@(export, link_name = "negs_sheet_thumb_cache_limits")
sheet_thumb_cache_limits :: proc "c" (size: u32, bytes, count: ^uint) {
	b, c: uint
	if size <= u32(max(sheet.Thumb_Size)) {
		b, c = sheet.thumb_cache_limits(sheet.Thumb_Size(size))
	}
	if bytes != nil {
		bytes^ = b
	}
	if count != nil {
		count^ = c
	}
}

@(export, link_name = "negs_sheet_thumbnail_centre")
sheet_thumbnail_centre :: proc "c" (on_screen: [^]uint, count: uint, has_open: u8, open: uint) -> uint {
	return sheet.thumbnail_centre(host_slice_3a(on_screen, count), open, has_open != 0)
}

@(export, link_name = "negs_sheet_outward")
sheet_outward :: proc "c" (indices: [^]uint, count: uint, centre: uint, order: [^]uint) {
	context = core_context()
	sheet.outward(host_slice_3a(indices, count), centre, host_slice_3a(order, count))
}

@(export, link_name = "negs_sheet_prefetch_order")
sheet_prefetch_order :: proc "c" (count: uint, on_screen: [^]uint, on_screen_count: uint, has_current: u8, current: uint, out: [^]uint) -> uint {
	if out == nil {
		return 0
	}
	return uint(sheet.prefetch_order(count, host_slice_3a(on_screen, on_screen_count), current, has_current != 0, out[:2 * sheet.PREFETCH_REACH + 1]))
}

@(export, link_name = "negs_sheet_pump_order")
sheet_pump_order :: proc "c" (items: [^]sheet.Pump_Item, count: uint, low_power: u8, out: [^]uint) -> uint {
	return uint(sheet.pump_order(host_slice_3a(items, count), low_power != 0, host_slice_3a(out, count)))
}

@(export, link_name = "negs_sheet_helpers")
sheet_helpers :: proc "c" (cores: uint, memory: u64, low_power: u8) -> uint {
	return sheet.helpers(cores, memory, low_power != 0)
}

@(export, link_name = "negs_sheet_readers")
sheet_readers :: proc "c" (cores: uint) -> uint {
	return sheet.readers(cores)
}

// MARK: Words

@(private = "file")
library_filter :: proc "contextless" (f: ^Sheet_Filter) -> (out: library.Filter) {
	if f == nil {
		return
	}
	switch f.rating {
	case 1:
		out.rating = {.At_Least, f.stars}
	case 2:
		out.rating = {.Unrated, 0}
	}
	for flag in library.Flag_Filter {
		if f.flags & (1 << u8(flag)) != 0 {
			out.flags += {flag}
		}
	}
	switch f.edits {
	case 1:
		out.edits = .Edited
	case 2:
		out.edits = .Unedited
	}
	return
}

@(export, link_name = "negs_sheet_sentence")
sheet_sentence :: proc "c" (filter: ^Sheet_Filter, shown, total: uint, parts: [^]Sheet_Part, parts_cap: uint, text: [^]u8, text_cap: uint, text_len: ^uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	made := sheet.sentence(library_filter(filter), shown, total, context.temp_allocator)
	offset: uint
	for part, i in made {
		if parts != nil && uint(i) < parts_cap {
			parts[i] = {u32(part.kind), 1 if part.on else 0, offset, uint(len(part.text))}
		}
		if text != nil && offset < text_cap {
			text_out_3a(part.text, text[offset:], text_cap - offset)
		}
		offset += uint(len(part.text))
	}
	if text_len != nil {
		text_len^ = offset
	}
	return uint(len(made))
}

@(export, link_name = "negs_sheet_foot")
sheet_foot :: proc "c" (shown, total, selected: uint, filtered: u8, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(sheet.foot(shown, total, selected, filtered != 0, context.temp_allocator), out, cap)
}

@(export, link_name = "negs_sheet_size_fraction")
sheet_size_fraction :: proc "c" (size: f64) -> f64 {
	return sheet.size_fraction(size)
}

@(export, link_name = "negs_sheet_size_at")
sheet_size_at :: proc "c" (fraction: f64) -> f64 {
	return sheet.size_at(fraction)
}

// MARK: The fade and scrolling

@(export, link_name = "negs_sheet_look")
sheet_look :: proc "c" (along: f64, opacity, rise: ^f64) {
	o, r := sheet.sheet_look(along)
	if opacity != nil {
		opacity^ = o
	}
	if rise != nil {
		rise^ = r
	}
}

@(export, link_name = "negs_sheet_cubic_bezier")
sheet_cubic_bezier :: proc "c" (x1, y1, x2, y2, t: f64) -> f64 {
	return sheet.cubic_bezier(x1, y1, x2, y2, t)
}

@(export, link_name = "negs_sheet_cubic_pieces")
sheet_cubic_pieces :: proc "c" (f: sheet.Curve, user: rawptr, length: f64, pieces: uint, out: [^][5]f64) {
	if f == nil || out == nil {
		return
	}
	sheet.cubic_pieces(f, user, length, pieces, out[:max(pieces, 1)])
}

@(export, link_name = "negs_sheet_wheel_glide")
sheet_wheel_glide :: proc "c" (at, to, dt_ms: f64) -> f64 {
	return sheet.wheel_glide(at, to, dt_ms)
}

@(export, link_name = "negs_sheet_bar_knob")
sheet_bar_knob :: proc "c" (scroll, content, viewport: f64, top, length: ^f64) -> u8 {
	t, l, ok := sheet.bar_knob(scroll, content, viewport)
	if top != nil {
		top^ = t
	}
	if length != nil {
		length^ = l
	}
	return 1 if ok else 0
}

@(export, link_name = "negs_sheet_bar_scroll")
sheet_bar_scroll :: proc "c" (top, content, viewport: f64) -> f64 {
	return sheet.bar_scroll(top, content, viewport)
}

@(export, link_name = "negs_sheet_bar_opacity")
sheet_bar_opacity :: proc "c" (since_ms: f64) -> f64 {
	return sheet.bar_opacity(since_ms)
}
