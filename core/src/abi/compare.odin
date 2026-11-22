// Compare's rules and geometry (negs_core.h, "Phase 3a: Compare"):
// photographs come in as the caller's negs_compare_item_t (borrowed for the
// call, laid out as compare.Item), words go into the caller's buffers.
package negs_core

import "base:runtime"
import "../rules/compare"

Compare_Constants :: struct {
	caption, label, stacked_caption, gap, stacked_gap, side, bottom: f64,
	parts:                                                           [5]cstring,
}

// negs_compare_offer_t
Compare_Offer :: struct {
	kind:          u32,
	enabled:       u8,
	with_original: u8,
}

@(export, link_name = "negs_compare_constants")
compare_constants :: proc "c" (out: ^Compare_Constants) {
	if out == nil {
		return
	}
	out^ = {
		caption         = compare.CAPTION,
		label           = compare.LABEL,
		stacked_caption = compare.STACKED_CAPTION,
		gap             = compare.GAP,
		stacked_gap     = compare.STACKED_GAP,
		side            = compare.SIDE,
		bottom          = compare.BOTTOM,
		parts           = compare.PARTS,
	}
}

@(export, link_name = "negs_compare_edited_is_first")
compare_edited_is_first :: proc "c" (first_original, second_original, active_first: u8) -> u8 {
	return 1 if compare.edited_is_first(first_original != 0, second_original != 0, active_first != 0) else 0
}

@(export, link_name = "negs_compare_against_original")
compare_against_original :: proc "c" (first_original, second_original: u8) -> u8 {
	return 1 if compare.against_original(first_original != 0, second_original != 0) else 0
}

@(export, link_name = "negs_compare_offer")
compare_offer :: proc "c" (target_count: uint, current_other, can_compare, can_with_original: u8, out: ^Compare_Offer) {
	if out == nil {
		return
	}
	o := compare.offer(target_count, current_other != 0, can_compare != 0, can_with_original != 0)
	out^ = {u32(o.kind), 1 if o.enabled else 0, 1 if o.with_original else 0}
}

@(export, link_name = "negs_compare_offer_title")
compare_offer_title :: proc "c" (kind: u32, a: [^]u8, a_len: uint, b: [^]u8, b_len: uint, out: [^]u8, cap: uint) -> uint {
	if kind > u32(max(compare.Offer_Kind)) {
		return 0
	}
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	title := compare.offer_title(compare.Offer_Kind(kind), host_string(a, a_len), host_string(b, b_len), context.temp_allocator)
	return text_out_3a(title, out, cap)
}

@(export, link_name = "negs_compare_word")
compare_word :: proc "c" (selected: uint, can_compare: u8) -> u32 {
	return u32(compare.word(selected, can_compare != 0))
}

@(export, link_name = "negs_compare_word_title")
compare_word_title :: proc "c" (kind: u32, a: [^]u8, a_len: uint, b: [^]u8, b_len: uint, out: [^]u8, cap: uint) -> uint {
	if kind > u32(max(compare.Word_Kind)) {
		return 0
	}
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	title := compare.word_title(compare.Word_Kind(kind), host_string(a, a_len), host_string(b, b_len), context.temp_allocator)
	return text_out_3a(title, out, cap)
}

@(export, link_name = "negs_compare_differences")
compare_differences :: proc "c" (a, b: ^[5]compare.Text, film_a: [^]u8, film_a_len: uint, film_b: [^]u8, film_b_len: uint, paper_a, paper_b: compare.Text, out: [^]u8, cap: uint) -> uint {
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	film := [2]string{host_string(film_a, film_a_len), host_string(film_b, film_b_len)}
	words := compare.differences(a, b, film, {paper_a, paper_b}, context.temp_allocator)
	return text_out_3a(words, out, cap)
}

@(export, link_name = "negs_compare_menu_title")
compare_menu_title :: proc "c" (comparing: u8) -> cstring {
	return compare.menu_title(comparing != 0)
}

@(export, link_name = "negs_compare_step")
compare_step :: proc "c" (from, first: ^compare.Item, order: [^]compare.Item, n: uint, offset: i64) -> i64 {
	if from == nil || first == nil {
		return -1
	}
	return index_3a(compare.step(from^, first^, host_slice_3a(order, n), int(offset)))
}

@(export, link_name = "negs_compare_frame_number")
compare_frame_number :: proc "c" (items: [^]compare.Item, n: uint, item: ^compare.Item, out: [^]u8, cap: uint) -> uint {
	if item == nil {
		return 0
	}
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(compare.frame_number(host_slice_3a(items, n), item^, context.temp_allocator), out, cap)
}

// MARK: Original ⌄

@(export, link_name = "negs_compare_original_mode_key")
compare_original_mode_key :: proc "c" (mode: u32) -> cstring {
	m := compare.Original_Mode(mode) if mode <= u32(max(compare.Original_Mode)) else .Hold
	return compare.original_mode_key(m)
}

@(export, link_name = "negs_compare_original_mode_from_key")
compare_original_mode_from_key :: proc "c" (key: [^]u8, n: uint) -> u32 {
	return u32(compare.original_mode_from_key(host_string(key, n)))
}

@(export, link_name = "negs_compare_original_mode_title")
compare_original_mode_title :: proc "c" (mode: u32) -> cstring {
	m := compare.Original_Mode(mode) if mode <= u32(max(compare.Original_Mode)) else .Hold
	return compare.original_mode_title(m)
}

@(export, link_name = "negs_compare_original_mode_help")
compare_original_mode_help :: proc "c" (mode: u32, original: [^]u8, original_len: uint, out: [^]u8, cap: uint) -> uint {
	m := compare.Original_Mode(mode) if mode <= u32(max(compare.Original_Mode)) else .Hold
	context = core_context()
	runtime.DEFAULT_TEMP_ALLOCATOR_TEMP_GUARD()
	return text_out_3a(compare.original_mode_help(m, host_string(original, original_len), context.temp_allocator), out, cap)
}

@(export, link_name = "negs_compare_original_reference_key")
compare_original_reference_key :: proc "c" (reference: u32) -> cstring {
	r := compare.Original_Reference(reference) if reference <= u32(max(compare.Original_Reference)) else .Raw
	return compare.original_reference_key(r)
}

@(export, link_name = "negs_compare_original_reference_from_key")
compare_original_reference_from_key :: proc "c" (key: [^]u8, n: uint) -> u32 {
	return u32(compare.original_reference_from_key(host_string(key, n)))
}

@(export, link_name = "negs_compare_original_reference_title")
compare_original_reference_title :: proc "c" (reference: u32) -> cstring {
	r := compare.Original_Reference(reference) if reference <= u32(max(compare.Original_Reference)) else .Raw
	return compare.original_reference_title(r)
}

// MARK: Geometry

@(export, link_name = "negs_compare_fit")
compare_fit :: proc "c" (aspect, room_width, room_height: f64, out: ^[2]f64) {
	if out != nil {
		out^ = compare.fit(aspect, {room_width, room_height})
	}
}

@(export, link_name = "negs_compare_row_layout")
compare_row_layout :: proc "c" (canvas_width, canvas_height, first_aspect, second_aspect: f64, out: ^[2]compare.Frame) {
	if out != nil {
		out^ = compare.row_layout({canvas_width, canvas_height}, {first_aspect, second_aspect})
	}
}

@(export, link_name = "negs_compare_column_layout")
compare_column_layout :: proc "c" (canvas_width, canvas_height, first_aspect, second_aspect: f64, out: ^[2]compare.Frame) {
	if out != nil {
		out^ = compare.column_layout({canvas_width, canvas_height}, {first_aspect, second_aspect})
	}
}

@(export, link_name = "negs_compare_layout")
compare_layout :: proc "c" (canvas_width, canvas_height, first_aspect, second_aspect: f64, out: ^[2]compare.Frame) -> u32 {
	arrangement, frames := compare.layout({canvas_width, canvas_height}, {first_aspect, second_aspect})
	if out != nil {
		out^ = frames
	}
	return u32(arrangement)
}

@(export, link_name = "negs_compare_slider_frame")
compare_slider_frame :: proc "c" (canvas_width, canvas_height, aspect: f64, out: ^compare.Frame) {
	if out != nil {
		out^ = compare.slider_frame({canvas_width, canvas_height}, aspect)
	}
}

// MARK: Pixels

@(export, link_name = "negs_compare_halved")
compare_halved :: proc "c" (src: [^]u8, width, height: u32, out: [^]u8) {
	if src == nil || out == nil || width == 0 || height == 0 {
		return
	}
	w, h := max(width / 2, 1), max(height / 2, 1)
	compare.halved(src[:int(width) * int(height) * 4], width, height, out[:int(w) * int(h) * 4])
}

@(export, link_name = "negs_compare_over_frame")
compare_over_frame :: proc "c" (width, height, frame_width, frame_height: u32, out: ^compare.Over_Frame) {
	if out != nil {
		out^ = compare.over_frame(width, height, {frame_width, frame_height})
	}
}
