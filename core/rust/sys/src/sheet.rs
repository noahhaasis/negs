//! Phase 3a: Sheet, tray and compare rules (core/src/rules: sheet, tray, compare).

use super::marks::negs_rect_t;
use std::os::raw::{c_char, c_void};

// -- Sheet ---------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_sheet_constants_t {
    pub strip: f64,
    pub strip_top: f64,
    pub strip_rebate: f64,
    pub strip_gap: f64,
    pub picture: f64,
    pub status: f64,
    pub band: f64,
    pub strip_dim: f64,
    pub strip_hover: f64,
    pub strip_pressed: f64,
    pub pencil_width: f64,
    pub pencil_drop: f64,
    pub fallback_ratio: f64,
    pub strip_margin: f64,
    pub leader_width: f64,
    pub leader_height: f64,
    pub leader_raised: f64,
    pub leader_slant: f64,
    pub glide_ms: f64,
    pub grid_gap: f64,
    pub grid_top: f64,
    pub grid_side: f64,
    pub grid_bottom: f64,
    pub grid_size_min: f64,
    pub grid_size_max: f64,
    pub grid_default: f64,
    pub grid_size_step: f64,
    pub grid_margin: f64,
    pub sentence: f64,
    pub foot: f64,
    pub box_margin: f64,
    pub selection_outset: f64,
    pub fade_ms: f64,
    pub fade_rise: f64,
    pub wheel_step: f64,
    pub wheel_tau_ms: f64,
    pub bar_width: f64,
    pub bar_wide: f64,
    pub bar_inset: f64,
    pub bar_min: f64,
    pub bar_reach: f64,
    pub bar_shown_ms: f64,
    pub bar_fade_ms: f64,
    pub prefetch_reach: u64,
    pub header_batch: u64,
    pub edited_long_side: u32,
    pub render_long_side: u32,
    pub edited_limit: u64,
    pub renders_limit: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_sheet_row_t {
    pub start: usize,
    pub end: usize,
    pub y: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_sheet_pump_item_t {
    pub look: u8,
    pub on_screen: u8,
    pub open: u8,
    pub done: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_sheet_filter_t {
    pub rating: u8,
    pub stars: u8,
    pub flags: u8,
    pub edits: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_sheet_part_t {
    pub kind: u32,
    pub on: u8,
    pub offset: usize,
    pub len: usize,
}

pub const NEGS_SHEET_PART_WORDS: u32 = 0;
pub const NEGS_SHEET_PART_FILTER: u32 = 1;
pub const NEGS_SHEET_PART_CLEAR: u32 = 2;
pub const NEGS_SHEET_MAX_PARTS: usize = 8;

pub const NEGS_THUMB_STRIP: u32 = 0;
pub const NEGS_THUMB_MEDIUM: u32 = 1;
pub const NEGS_THUMB_LARGE: u32 = 2;

pub type negs_curve_t = unsafe extern "C" fn(user: *mut c_void, t: f64) -> f64;

unsafe extern "C" {
    pub fn negs_sheet_constants(out: *mut negs_sheet_constants_t);
    pub fn negs_sheet_reject_opacity(in_strip: u8, current: u8) -> f64;
    pub fn negs_sheet_strip_opacity(current: u8, selected: u8, rejected: u8, hovered: u8, pressed: u8) -> f64;

    pub fn negs_sheet_kept_ratio(has_known: u8, known: f64, thumbnail: f64) -> f64;
    pub fn negs_sheet_frame_width(has_ratio: u8, ratio: f64) -> f64;
    pub fn negs_sheet_strip_layout(widths: *const f64, count: usize, xs: *mut f64) -> f64;
    pub fn negs_sheet_strip_middle(x: f64, width: f64) -> f64;
    pub fn negs_sheet_strip_indices(xs: *const f64, widths: *const f64, count: usize, min_x: f64, max_x: f64, start: *mut usize, end: *mut usize);
    pub fn negs_sheet_strip_hit(xs: *const f64, widths: *const f64, count: usize, x: f64) -> i64;
    pub fn negs_sheet_strip_index_at(xs: *const f64, widths: *const f64, count: usize, x: f64) -> i64;
    pub fn negs_sheet_centred(middle: f64, visible: f64, content: f64) -> f64;
    pub fn negs_sheet_hold(old_middle: f64, new_middle: f64, new_width: f64, scroll: f64, visible: f64) -> f64;
    pub fn negs_sheet_smoothstep(t: f64) -> f64;

    pub fn negs_sheet_clamp_size(size: f64) -> f64;
    pub fn negs_sheet_layout(
        ratios: *const f64,
        count: usize,
        width: f64,
        target: f64,
        frames: *mut negs_rect_t,
        rows: *mut negs_sheet_row_t,
        row_count: *mut usize,
    ) -> f64;
    pub fn negs_sheet_row_of(rows: *const negs_sheet_row_t, row_count: usize, frame_count: usize, index: usize) -> i64;
    pub fn negs_sheet_vertical(
        frames: *const negs_rect_t,
        frame_count: usize,
        rows: *const negs_sheet_row_t,
        row_count: usize,
        index: usize,
        by: i64,
    ) -> i64;
    pub fn negs_sheet_indices_in(
        frames: *const negs_rect_t,
        frame_count: usize,
        rows: *const negs_sheet_row_t,
        row_count: usize,
        rect: negs_rect_t,
        out: *mut usize,
    ) -> usize;
    pub fn negs_sheet_indices(rows: *const negs_sheet_row_t, row_count: usize, min_y: f64, max_y: f64, start: *mut usize, end: *mut usize);
    pub fn negs_sheet_hit(frames: *const negs_rect_t, frame_count: usize, rows: *const negs_sheet_row_t, row_count: usize, x: f64, y: f64) -> i64;
    pub fn negs_sheet_nearest(frames: *const negs_rect_t, frame_count: usize, rows: *const negs_sheet_row_t, row_count: usize, x: f64, y: f64)
    -> i64;
    pub fn negs_sheet_centre_row(row: negs_sheet_row_t, height: f64, viewport: f64) -> f64;
    pub fn negs_sheet_reveal(frame: negs_rect_t, height: f64, scroll: f64, viewport: f64) -> f64;
    pub fn negs_sheet_keep_anchor(frame: negs_rect_t, height: f64, down: f64, from_top: f64, viewport: f64) -> f64;

    pub fn negs_sheet_thumb_pixels(size: u32) -> u32;
    pub fn negs_sheet_thumb_grid(points: f64, scale: f64) -> u32;
    pub fn negs_sheet_thumb_for_rows(size: f64, scale: f64) -> u32;
    pub fn negs_sheet_thumb_cache_limits(size: u32, bytes: *mut usize, count: *mut usize);
    pub fn negs_sheet_thumbnail_centre(on_screen: *const usize, count: usize, has_open: u8, open: usize) -> usize;
    pub fn negs_sheet_outward(indices: *const usize, count: usize, centre: usize, order: *mut usize);
    pub fn negs_sheet_prefetch_order(
        count: usize,
        on_screen: *const usize,
        on_screen_count: usize,
        has_current: u8,
        current: usize,
        out: *mut usize,
    ) -> usize;
    pub fn negs_sheet_pump_order(items: *const negs_sheet_pump_item_t, count: usize, low_power: u8, out: *mut usize) -> usize;
    pub fn negs_sheet_helpers(cores: usize, memory: u64, low_power: u8) -> usize;
    pub fn negs_sheet_readers(cores: usize) -> usize;

    pub fn negs_sheet_sentence(
        filter: *const negs_sheet_filter_t,
        shown: usize,
        total: usize,
        parts: *mut negs_sheet_part_t,
        parts_cap: usize,
        text: *mut u8,
        text_cap: usize,
        text_len: *mut usize,
    ) -> usize;
    pub fn negs_sheet_foot(shown: usize, total: usize, selected: usize, filtered: u8, out: *mut u8, cap: usize) -> usize;
    pub fn negs_sheet_size_fraction(size: f64) -> f64;
    pub fn negs_sheet_size_at(fraction: f64) -> f64;

    pub fn negs_sheet_look(along: f64, opacity: *mut f64, rise: *mut f64);
    pub fn negs_sheet_cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64;
    pub fn negs_sheet_cubic_pieces(f: negs_curve_t, user: *mut c_void, length: f64, pieces: usize, out: *mut f64);
    pub fn negs_sheet_wheel_glide(at: f64, to: f64, dt_ms: f64) -> f64;
    pub fn negs_sheet_bar_knob(scroll: f64, content: f64, viewport: f64, top: *mut f64, length: *mut f64) -> u8;
    pub fn negs_sheet_bar_scroll(top: f64, content: f64, viewport: f64) -> f64;
    pub fn negs_sheet_bar_opacity(since_ms: f64) -> f64;
}

// -- Tray ----------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tray_constants_t {
    pub tile: f64,
    pub picture: f64,
    pub tile_gap: f64,
    pub group_gap: f64,
    pub title: f64,
    pub title_gap: f64,
    pub inset: f64,
    pub top: f64,
    pub name_gap: f64,
    pub name_line: f64,
    pub row: f64,
    pub foot: f64,
    pub height: f64,
    pub star: f64,
    pub star_inset: f64,
    pub wheel_notch: f64,
    pub glide_ms: f64,
    pub favourites: *const c_char,
    pub scan: *const c_char,
    pub your_negative: *const c_char,
    pub keep_paper_label: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tray_entry_t {
    pub id: *const u8,
    pub id_len: usize,
    pub name: *const u8,
    pub name_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tray_section_t {
    pub title: *const u8,
    pub title_len: usize,
    pub stocks: *const negs_tray_entry_t,
    pub count: usize,
    pub favourites: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tray_place_t {
    pub stock: *const u8,
    pub stock_len: usize,
    pub favourite: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_tray_escape_context_t {
    pub alert: u8,
    pub typing: u8,
    pub tray: u8,
    pub cropping: u8,
    pub grid: u8,
    pub image_only: u8,
    pub photograph: u8,
}

pub const NEGS_TRAY_HIT_NONE: u32 = 0;
pub const NEGS_TRAY_HIT_TILE: u32 = 1;
pub const NEGS_TRAY_HIT_STAR: u32 = 2;
pub const NEGS_TRAY_ENDING_NOTHING: u32 = 0;
pub const NEGS_TRAY_ENDING_STEP: u32 = 1;
pub const NEGS_TRAY_ENDING_RESTORE: u32 = 2;
pub const NEGS_TRAY_SUBMIT_KEEP: u32 = 0;
pub const NEGS_TRAY_SUBMIT_TRY_FIRST: u32 = 1;
pub const NEGS_TRAY_CANCEL_CLEAR: u32 = 0;
pub const NEGS_TRAY_CANCEL_GO_BACK: u32 = 1;
pub const NEGS_TRAY_KEY_ESCAPE: u32 = 0;
pub const NEGS_TRAY_KEY_RETURN: u32 = 1;
pub const NEGS_TRAY_KEY_LEFT: u32 = 2;
pub const NEGS_TRAY_KEY_RIGHT: u32 = 3;
pub const NEGS_TRAY_KEY_OTHER: u32 = 4;
pub const NEGS_TRAY_ACTION_GO_BACK: u32 = 0;
pub const NEGS_TRAY_ACTION_KEEP: u32 = 1;
pub const NEGS_TRAY_ACTION_STEP: u32 = 2;
pub const NEGS_TRAY_ACTION_PASS: u32 = 3;
pub const NEGS_ESCAPE_ALERT: u32 = 0;
pub const NEGS_ESCAPE_TYPING: u32 = 1;
pub const NEGS_ESCAPE_TRAY: u32 = 2;
pub const NEGS_ESCAPE_CROP: u32 = 3;
pub const NEGS_ESCAPE_GRID: u32 = 4;
pub const NEGS_ESCAPE_IMAGE_ONLY: u32 = 5;
pub const NEGS_ESCAPE_SHOW_GRID: u32 = 6;
pub const NEGS_ESCAPE_NOTHING: u32 = 7;

unsafe extern "C" {
    pub fn negs_tray_constants(out: *mut negs_tray_constants_t);
    pub fn negs_tray_searching(query: *const u8, query_len: usize) -> u8;
    pub fn negs_tray_sections(
        counts: *const usize,
        n: usize,
        favourite_count: usize,
        query: *const u8,
        query_len: usize,
        kept: *mut usize,
        favourites_lead: *mut u8,
    ) -> usize;
    pub fn negs_tray_with_scan(paper: u8, read_as: u8, query: *const u8, query_len: usize) -> u8;
    pub fn negs_tray_places(sections: *const negs_tray_section_t, n: usize, out: *mut negs_tray_place_t, cap: usize) -> usize;
    pub fn negs_tray_step(current: *const u8, current_len: usize, in_favourites: u8, offset: i32, places: *const negs_tray_place_t, n: usize) -> i64;
    pub fn negs_tray_width(stock_count: usize) -> f64;
    pub fn negs_tray_starts(sections: *const negs_tray_section_t, n: usize, out: *mut f64);
    pub fn negs_tray_content_width(sections: *const negs_tray_section_t, n: usize) -> f64;
    pub fn negs_tray_tile_left(start: f64, index: usize) -> f64;
    pub fn negs_tray_tile_x(stock: *const u8, stock_len: usize, section: usize, sections: *const negs_tray_section_t, n: usize, x: *mut f64) -> u8;
    pub fn negs_tray_section_of(stock: *const u8, stock_len: usize, sections: *const negs_tray_section_t, n: usize, favourites_first: u8) -> i64;
    pub fn negs_tray_current(scrolled_to: f64, starts: *const f64, n: usize) -> usize;
    pub fn negs_tray_centring(tile_x: f64, visible: f64, content: f64) -> f64;
    pub fn negs_tray_jump(start: f64, visible: f64, content: f64) -> f64;
    pub fn negs_tray_clamp(x: f64, visible: f64, content: f64) -> f64;
    pub fn negs_tray_reveal(
        offset: f64,
        stock: *const u8,
        stock_len: usize,
        in_favourites: u8,
        centre: u8,
        sections: *const negs_tray_section_t,
        n: usize,
        visible: f64,
    ) -> f64;
    pub fn negs_tray_wheel(offset: f64, delta: i32, sideways: u8, visible: f64, content: f64) -> f64;
    pub fn negs_tray_glide(from: f64, to: f64, elapsed_ms: f64) -> f64;
    pub fn negs_tray_hit(sections: *const negs_tray_section_t, n: usize, x: f64, y: f64, section: *mut usize, index: *mut usize) -> u32;
    pub fn negs_tray_star_rect(left: f64, out: *mut [f64; 4]);
    pub fn negs_tray_stand_in_size(image_width: u32, image_height: u32, photo_width: f64, photo_height: f64, out: *mut [f64; 2]);

    pub fn negs_tray_star_help(favourite: u8) -> *const c_char;
    pub fn negs_tray_star_label(name: *const u8, name_len: usize, favourite: u8, out: *mut u8, cap: usize) -> usize;
    pub fn negs_tray_tile_value(selected: u8, before: u8) -> *const c_char;
    pub fn negs_tray_index_label(title: *const u8, title_len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_tray_hint(has_before: u8, before: *const u8, before_len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_tray_matches(count: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_tray_no_match(query: *const u8, query_len: usize, paper: u8, out: *mut u8, cap: usize) -> usize;
    pub fn negs_tray_done_help(paper: u8) -> *const c_char;
    pub fn negs_tray_keep_paper_words(kept: u8) -> *const c_char;

    pub fn negs_tray_visit_action(paper: u8, read_as: u8) -> *const c_char;
    pub fn negs_tray_visit_tried(before_id: *const u8, before_len: usize, current: *const u8, current_len: usize) -> u8;
    pub fn negs_tray_visit_shows_before(
        before_id: *const u8,
        before_len: usize,
        stock: *const u8,
        stock_len: usize,
        current: *const u8,
        current_len: usize,
    ) -> u8;
    pub fn negs_tray_ending(keep: u8, unchanged: u8) -> u32;
    pub fn negs_tray_submit(query: *const u8, query_len: usize) -> u32;
    pub fn negs_tray_cancel(query: *const u8, query_len: usize) -> u32;
    pub fn negs_tray_key(key: u32, command: u8, typing: u8, step: *mut i32) -> u32;
    pub fn negs_tray_escape(context: *const negs_tray_escape_context_t) -> u32;
}

// -- Compare -------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_compare_constants_t {
    pub caption: f64,
    pub label: f64,
    pub stacked_caption: f64,
    pub gap: f64,
    pub stacked_gap: f64,
    pub side: f64,
    pub bottom: f64,
    pub parts: [*const c_char; 5],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_compare_text_t {
    pub text: *const u8,
    pub len: usize,
    pub has: u8,
}

impl negs_compare_text_t {
    pub const NONE: Self = Self { text: std::ptr::null(), len: 0, has: 0 };

    /// A view of `text` (borrowed while the call lasts), or none.
    pub fn of(text: Option<&str>) -> Self {
        match text {
            Some(t) => Self { text: t.as_ptr(), len: t.len(), has: 1 },
            None => Self::NONE,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_compare_item_t {
    pub photo: *const u8,
    pub photo_len: usize,
    pub variant: negs_compare_text_t,
}

pub const NEGS_COMPARE_OFFER_TOO_MANY: u32 = 0;
pub const NEGS_COMPARE_OFFER_PAIR: u32 = 1;
pub const NEGS_COMPARE_OFFER_WITH_CURRENT: u32 = 2;
pub const NEGS_COMPARE_OFFER_WITH_ORIGINAL: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_compare_offer_t {
    pub kind: u32,
    pub enabled: u8,
    pub with_original: u8,
}

pub const NEGS_COMPARE_WORD_PAIR: u32 = 0;
pub const NEGS_COMPARE_WORD_TOO_MANY: u32 = 1;
pub const NEGS_COMPARE_WORD_SELECT_SECOND: u32 = 2;
pub const NEGS_ORIGINAL_HOLD: u32 = 0;
pub const NEGS_ORIGINAL_SPLIT: u32 = 1;
pub const NEGS_ORIGINAL_SIDE_BY_SIDE: u32 = 2;
pub const NEGS_ORIGINAL_RAW: u32 = 0;
pub const NEGS_ORIGINAL_CAMERA_JPEG: u32 = 1;
pub const NEGS_COMPARE_ROW: u32 = 0;
pub const NEGS_COMPARE_COLUMN: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_compare_over_frame_t {
    pub out: [u32; 2],
    pub size: [u32; 2],
    pub at: [i64; 2],
    pub same: u8,
}

unsafe extern "C" {
    pub fn negs_compare_constants(out: *mut negs_compare_constants_t);
    pub fn negs_compare_edited_is_first(first_original: u8, second_original: u8, active_first: u8) -> u8;
    pub fn negs_compare_against_original(first_original: u8, second_original: u8) -> u8;
    pub fn negs_compare_offer(target_count: usize, current_other: u8, can_compare: u8, can_with_original: u8, out: *mut negs_compare_offer_t);
    pub fn negs_compare_offer_title(kind: u32, a: *const u8, a_len: usize, b: *const u8, b_len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_compare_word(selected: usize, can_compare: u8) -> u32;
    pub fn negs_compare_word_title(kind: u32, a: *const u8, a_len: usize, b: *const u8, b_len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_compare_differences(
        a: *const negs_compare_text_t,
        b: *const negs_compare_text_t,
        film_a: *const u8,
        film_a_len: usize,
        film_b: *const u8,
        film_b_len: usize,
        paper_a: negs_compare_text_t,
        paper_b: negs_compare_text_t,
        out: *mut u8,
        cap: usize,
    ) -> usize;
    pub fn negs_compare_menu_title(comparing: u8) -> *const c_char;
    pub fn negs_compare_step(
        from: *const negs_compare_item_t,
        first: *const negs_compare_item_t,
        order: *const negs_compare_item_t,
        n: usize,
        offset: i64,
    ) -> i64;
    pub fn negs_compare_frame_number(items: *const negs_compare_item_t, n: usize, item: *const negs_compare_item_t, out: *mut u8, cap: usize) -> usize;

    pub fn negs_compare_original_mode_key(mode: u32) -> *const c_char;
    pub fn negs_compare_original_mode_from_key(key: *const u8, key_len: usize) -> u32;
    pub fn negs_compare_original_mode_title(mode: u32) -> *const c_char;
    pub fn negs_compare_original_mode_help(mode: u32, original: *const u8, original_len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_compare_original_reference_key(reference: u32) -> *const c_char;
    pub fn negs_compare_original_reference_from_key(key: *const u8, key_len: usize) -> u32;
    pub fn negs_compare_original_reference_title(reference: u32) -> *const c_char;

    pub fn negs_compare_fit(aspect: f64, room_width: f64, room_height: f64, out: *mut [f64; 2]);
    pub fn negs_compare_row_layout(canvas_width: f64, canvas_height: f64, first_aspect: f64, second_aspect: f64, out: *mut [negs_rect_t; 2]);
    pub fn negs_compare_column_layout(canvas_width: f64, canvas_height: f64, first_aspect: f64, second_aspect: f64, out: *mut [negs_rect_t; 2]);
    pub fn negs_compare_layout(canvas_width: f64, canvas_height: f64, first_aspect: f64, second_aspect: f64, out: *mut [negs_rect_t; 2]) -> u32;
    pub fn negs_compare_slider_frame(canvas_width: f64, canvas_height: f64, aspect: f64, out: *mut negs_rect_t);

    pub fn negs_compare_halved(src: *const u8, width: u32, height: u32, out: *mut u8);
    pub fn negs_compare_over_frame(width: u32, height: u32, frame_width: u32, frame_height: u32, out: *mut negs_compare_over_frame_t);
}

/// A static string from the core (NUL-terminated, lives as long as the process).
///
/// # Safety
/// `s` must be one of the core's static strings.
pub unsafe fn static_str(s: *const c_char) -> &'static str {
    if s.is_null() {
        return "";
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes();
    // The core's words are UTF-8.
    std::str::from_utf8(bytes).unwrap_or("")
}

/// Words the core writes into a buffer: `fill(out, cap)` returns the whole
/// length; a second call gets them whole when the first buffer was short.
pub fn text(mut fill: impl FnMut(*mut u8, usize) -> usize) -> String {
    let mut buf = [0u8; 256];
    let n = fill(buf.as_mut_ptr(), buf.len());
    let bytes = if n <= buf.len() {
        buf[..n].to_vec()
    } else {
        let mut v = vec![0u8; n];
        let again = fill(v.as_mut_ptr(), n);
        v.truncate(again.min(n));
        v
    };
    String::from_utf8(bytes).expect("the core writes UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;
    use std::mem::{offset_of, size_of};

    macro_rules! check {
        ($t:ident: $($f:ident),*) => {
            assert_eq!(layout(stringify!($t)), size_of::<$t>() as i64, stringify!($t));
            $(assert_eq!(layout(concat!(stringify!($t), ".", stringify!($f))), offset_of!($t, $f) as i64, concat!(stringify!($t), ".", stringify!($f)));)*
        };
    }

    #[test]
    fn layouts_match_the_core() {
        check!(negs_sheet_constants_t: strip, band, bar_fade_ms, prefetch_reach, header_batch, edited_long_side, render_long_side, edited_limit, renders_limit);
        check!(negs_sheet_row_t: start, end, y, height);
        check!(negs_sheet_pump_item_t: look, on_screen, open, done);
        check!(negs_sheet_filter_t: rating, stars, flags, edits);
        check!(negs_sheet_part_t: kind, on, offset, len);
        check!(negs_tray_constants_t: tile, glide_ms, favourites, scan, your_negative, keep_paper_label);
        check!(negs_tray_entry_t: id, id_len, name, name_len);
        check!(negs_tray_section_t: title, title_len, stocks, count, favourites);
        check!(negs_tray_place_t: stock, stock_len, favourite);
        check!(negs_tray_escape_context_t: alert, typing, tray, cropping, grid, image_only, photograph);
        check!(negs_compare_constants_t: caption, bottom, parts);
        check!(negs_compare_text_t: text, len, has);
        check!(negs_compare_item_t: photo, photo_len, variant);
        check!(negs_compare_offer_t: kind, enabled, with_original);
        check!(negs_compare_over_frame_t: out, size, at, same);
    }
}
