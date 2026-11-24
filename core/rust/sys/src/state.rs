//! Phase 4 base: value, edit state (core/src/value, core/src/state), and
//! Phase 4a: state rules, edits, inspector, below it.

#[allow(unused_imports)]
use super::*;
pub use super::picture::negs_orientation_t;
use std::os::raw::{c_char, c_void};

// MARK: Phase 4 base

pub const NEGS_JSON_PRETTY: u32 = 1;
pub const NEGS_JSON_SORTED: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_str_t {
    pub data: *const u8,
    pub len: usize,
}

/// One parsed edit state, owned by the caller until `negs_state_free`.
#[repr(C)]
pub struct negs_state_t {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub fn negs_value_rewrite(text: *const u8, len: usize, flags: u32, out: *mut negs_buf_t) -> u8;
    pub fn negs_value_write_f64(v: f64, out: *mut u8, cap: usize) -> usize;
    pub fn negs_value_display_f64(v: f64, out: *mut u8, cap: usize) -> usize;

    pub fn negs_json_get(value: *const u8, len: usize, path: *const u8, path_len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_json_set(
        value: *const u8,
        len: usize,
        path: *const u8,
        path_len: usize,
        item: *const u8,
        item_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_json_number(value: *const u8, len: usize) -> f64;
    pub fn negs_json_boolean(value: *const u8, len: usize) -> u8;
    pub fn negs_json_string(value: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_json_num(v: f64, out: *mut u8, cap: usize) -> usize;
    pub fn negs_json_same(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> u8;
    pub fn negs_json_restoring(
        defaults: *const u8,
        defaults_len: usize,
        saved: *const u8,
        saved_len: usize,
        nullable: *const negs_str_t,
        nullable_count: usize,
        out: *mut negs_buf_t,
    ) -> u8;

    pub fn negs_state_parse(json: *const u8, len: usize) -> *mut negs_state_t;
    pub fn negs_state_default() -> *mut negs_state_t;
    pub fn negs_state_free(state: *mut negs_state_t);
    pub fn negs_state_write(state: *const negs_state_t, flags: u32) -> negs_buf_t;
    pub fn negs_state_eq(a: *const negs_state_t, b: *const negs_state_t) -> u8;
    pub fn negs_state_rendered(state: *const negs_state_t) -> *mut negs_state_t;
    pub fn negs_state_match_inputs(state: *const negs_state_t) -> negs_buf_t;
    pub fn negs_state_rotation(state: *const negs_state_t) -> i32;
    pub fn negs_state_flip(state: *const negs_state_t) -> u8;
    pub fn negs_state_orientation(state: *const negs_state_t, out: *mut negs_orientation_t);
    pub fn negs_state_size_target(state: *const negs_state_t, out: *mut u32) -> u8;
    pub fn negs_state_roll_filtered(state: *const negs_state_t, roll_magenta: f64, roll_yellow: f64) -> u8;
    pub fn negs_state_roll_own_filtration(
        roll_paper: *const u8,
        len: usize,
        has_paper: u8,
        paper: *const u8,
        paper_len: usize,
    ) -> *const c_char;

    pub fn negs_state_rotation_of(value: *const u8, len: usize) -> i32;
    pub fn negs_state_flip_of(value: *const u8, len: usize) -> u8;
    pub fn negs_state_size_target_of(value: *const u8, len: usize, out: *mut u32) -> u8;
    pub fn negs_state_params_size_target(params: *const u8, len: usize, out: *mut u32) -> u8;
    pub fn negs_state_roll_filtered_of(
        roll_magenta: f64,
        roll_yellow: f64,
        magenta: *const u8,
        magenta_len: usize,
        yellow: *const u8,
        yellow_len: usize,
    ) -> u8;
    pub fn negs_state_restored_params(
        defaults: *const u8,
        defaults_len: usize,
        saved: *const u8,
        saved_len: usize,
        nullable: *const negs_str_t,
        nullable_count: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_state_constant(which: u32) -> *const c_char;
}

// MARK: Phase 4a

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_stock_t {
    pub id: negs_str_t,
    pub name: negs_str_t,
    pub target: negs_str_t,
    pub grain: negs_str_t,
    pub times: *const f64,
    pub times_len: usize,
    pub paper: u8,
    pub positive: u8,
    pub bw: u8,
    pub fotufilm: u8,
    pub manufacturer_data: u8,
    pub negs_built: u8,
    pub cine_print: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_color_filter_t {
    pub key: negs_str_t,
    pub name: negs_str_t,
    pub glass: negs_str_t,
    pub colour: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_adjustment_t {
    pub kind: u32,
    pub title: *const c_char,
    pub path: *const c_char,
    pub help: *const c_char,
    pub low: f64,
    pub high: f64,
    pub step: f64,
    pub options: *const *const c_char,
    pub options_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_adjustment_group_t {
    pub title: *const c_char,
    pub section: u32,
    pub enable: *const c_char,
    pub basic: usize,
    pub controls: *const negs_adjustment_t,
    pub controls_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_choice_t {
    pub film: negs_str_t,
    pub paper: negs_str_t,
    pub nulls: u32,
    pub sets_scan: u8,
    pub scan: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_stock_group_t {
    pub title: *const c_char,
    pub start: usize,
    pub count: usize,
}

pub type negs_contains_fn = Option<unsafe extern "C" fn(text: *const u8, text_len: usize, word: *const u8, word_len: usize) -> u8>;

#[repr(C)]
pub struct negs_catalog_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct negs_facts_t {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_facts_prefs_t {
    pub more_adjustments: negs_str_t,
    pub color_space: negs_str_t,
    pub output_format: negs_str_t,
    pub filter_shift_link: negs_str_t,
    pub tone_graph_channel: negs_str_t,
    pub jpeg_quality: u32,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
    pub lab_all_controls: u8,
    pub match_new_photos: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_facts_negative_t {
    pub sentence: negs_str_t,
    pub warning: negs_str_t,
    pub edge: negs_str_t,
    pub flat_path: negs_str_t,
    pub flat_reason: negs_str_t,
    pub has_warning: u8,
    pub has_base: u8,
    pub base_found: u8,
    pub base_picked: u8,
    pub base_by_hand: u8,
    pub base_from_roll: u8,
    pub has_edge: u8,
    pub has_flat_used: u8,
    pub flat_used: u8,
    pub has_flat_reason: u8,
    pub busy: u8,
    pub backlight_shown: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_facts_input_t {
    pub prefs: negs_facts_prefs_t,
    pub photo_size: [f64; 2],
    pub preview_scale: f64,
    pub generation: u64,
    pub failure_generation: u64,
    pub failure_inputs: negs_str_t,
    pub failure_message: negs_str_t,
    pub size_error: negs_str_t,
    pub negative: negs_facts_negative_t,
    pub source_width: u32,
    pub source_height: u32,
    pub is_raw: u8,
    pub lens_supported: u8,
    pub has_source: u8,
    pub image_shown: u8,
    pub has_size_error: u8,
    pub custom_film_format: u8,
    pub balancing_print: u8,
    pub can_balance_print: u8,
    pub can_export: u8,
    pub cropping: u8,
    pub has_failure: u8,
    pub failure_has_inputs: u8,
    pub matching: u8,
    pub has_negative: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct negs_facts_exposure_t {
    pub offset: f64,
    pub modified: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct negs_facts_host_t {
    pub user: *mut c_void,
    pub is_modified: unsafe extern "C" fn(user: *mut c_void, path: *const u8, len: usize) -> u8,
    pub value: unsafe extern "C" fn(user: *mut c_void, path: *const u8, len: usize, out: *mut u8, cap: usize) -> usize,
    pub flags: unsafe extern "C" fn(user: *mut c_void) -> u32,
    pub exposure: unsafe extern "C" fn(user: *mut c_void, out: *mut negs_facts_exposure_t),
    pub color_filter: unsafe extern "C" fn(user: *mut c_void, name: *mut negs_str_t) -> u8,
}

pub const NEGS_FACTS_BORROW: u32 = 1;

pub const NEGS_FACTS_FILM_BW: u32 = 1;
pub const NEGS_FACTS_FILM_POSITIVE: u32 = 2;
pub const NEGS_FACTS_BORDER: u32 = 4;
pub const NEGS_FACTS_MATCHES: u32 = 8;
pub const NEGS_FACTS_ACTIVE: u32 = 16;
pub const NEGS_FACTS_PENDING: u32 = 32;
pub const NEGS_FACTS_FAILED_HERE: u32 = 64;
pub const NEGS_FACTS_OFFSET_MODIFIED: u32 = 128;
pub const NEGS_FACTS_SHOWS_METER: u32 = 256;
pub const NEGS_FACTS_FILM_FORMAT_CUSTOM: u32 = 512;
pub const NEGS_FACTS_NEGATIVE: u32 = 1024;
pub const NEGS_FACTS_COLOR_FILTER_SHOWN: u32 = 2048;
pub const NEGS_FACTS_HAS_CATALOG: u32 = 4096;
pub const NEGS_FACTS_NEGATIVE_BUSY: u32 = 8192;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct negs_facts_brightness_t {
    pub matched_ev: f64,
    pub exposure_offset: f64,
    pub range_low: f64,
    pub range_high: f64,
}

pub const NEGS_TARGET_PARAM: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_edit_t {
    pub title: negs_str_t,
    pub value: negs_str_t,
    pub target: u32,
    pub path: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_more_t {
    pub text: negs_str_t,
    pub key: negs_str_t,
    pub action: u32,
}

unsafe extern "C" {
    pub fn negs_stock_read(json: *const u8, len: usize, out: *mut negs_stock_t, storage: *mut negs_buf_t) -> u8;
    pub fn negs_color_filter_read(json: *const u8, len: usize, out: *mut negs_color_filter_t, storage: *mut negs_buf_t) -> u8;
    pub fn negs_stock_is_motion_picture_print(stock: *const negs_stock_t) -> u8;
    pub fn negs_stock_lists_negs_built(stock: *const negs_stock_t, shown: u8) -> u8;

    pub fn negs_fotufilm_from_key(key: *const u8, len: usize) -> u32;
    pub fn negs_fotufilm_word(choice: u32, which: u32) -> *const c_char;
    pub fn negs_fotufilm_lists(choice: u32, stock: *const negs_stock_t) -> u8;

    pub fn negs_color_filter_effect(key: *const u8, len: usize) -> *const c_char;
    pub fn negs_color_filter_hint(
        filter: *const negs_color_filter_t,
        has_stops: u8,
        stops: f64,
        film: *const u8,
        film_len: usize,
        metered: u8,
        decimal: u32,
        out: *mut negs_buf_t,
    );
    pub fn negs_color_filter_help(filter: *const negs_color_filter_t, out: *mut negs_buf_t);

    pub fn negs_filter_link_from_key(key: *const u8, len: usize) -> u32;
    pub fn negs_filter_link_word(link: u32, which: u32) -> *const c_char;
    pub fn negs_filter_link_links(path: *const u8, len: usize) -> u8;
    pub fn negs_filter_link_shifts(link: u32, path: *const u8, len: usize, value: f64, magenta: f64, yellow: f64, out: *mut [f64; 2]);

    pub fn negs_grain_lane(path: *const u8, len: usize, field: *mut u32, lane: *mut u64) -> u8;
    pub fn negs_color_only(path: *const u8, len: usize) -> u8;
    pub fn negs_bw_title(path: *const u8, len: usize) -> *const c_char;

    pub fn negs_option_title(option: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_snap(value: f64, low: f64, high: f64, step: f64) -> f64;
    pub fn negs_decimals(step: f64) -> usize;
    pub fn negs_format_value(value: f64, step: f64, out: *mut u8, cap: usize) -> usize;
    pub fn negs_film_format(mm: f64) -> i32;
    pub fn negs_ratio_name(width: f64, height: f64, out: *mut u8, cap: usize) -> usize;

    pub fn negs_adjustment_group_count() -> usize;
    pub fn negs_adjustment_group(index: usize, out: *mut negs_adjustment_group_t) -> u8;
    pub fn negs_adjustments(which: u32, count: *mut usize) -> *const negs_adjustment_t;
    pub fn negs_state_words(which: u32, count: *mut usize) -> *const *const c_char;
    pub fn negs_film_format_row(index: usize, name: *mut *const c_char, mm: *mut f64) -> u8;
    pub fn negs_state_rules_constant(which: u32) -> *const c_char;
    pub fn negs_state_rules_number(which: u32) -> f64;

    pub fn negs_choosing_plan(
        film: *const u8,
        film_len: usize,
        paper: *const u8,
        paper_len: usize,
        id: *const u8,
        id_len: usize,
        is_paper: u8,
        keep_paper: u8,
        stocks: *const negs_stock_t,
        count: usize,
        out: *mut negs_choice_t,
    );
    pub fn negs_pair_film_plan(
        film: *const u8,
        film_len: usize,
        paper: *const u8,
        paper_len: usize,
        stocks: *const negs_stock_t,
        count: usize,
        with_paper: u8,
        out: *mut negs_choice_t,
    );
    pub fn negs_choosing(
        state: *const negs_state_t,
        id: *const u8,
        id_len: usize,
        is_paper: u8,
        keep_paper: u8,
        stocks: *const negs_stock_t,
        count: usize,
    ) -> *mut negs_state_t;

    pub fn negs_film_groups(
        films: *const negs_stock_t,
        film_count: usize,
        papers: *const negs_stock_t,
        paper_count: usize,
        out: *mut negs_stock_group_t,
        cap: usize,
        places: *mut u32,
    ) -> usize;
    pub fn negs_paper_groups(papers: *const negs_stock_t, paper_count: usize, out: *mut negs_stock_group_t, cap: usize, places: *mut u32) -> usize;
    pub fn negs_stock_matching(
        query: *const u8,
        query_len: usize,
        titles: *const negs_str_t,
        counts: *const usize,
        group_count: usize,
        stocks: *const negs_stock_t,
        keep: *mut u8,
    ) -> u8;
    pub fn negs_text_set_contains(contains: negs_contains_fn);

    pub fn negs_catalog_new(
        defaults: *const u8,
        defaults_len: usize,
        stocks: *const negs_stock_t,
        stock_count: usize,
        filters: *const negs_color_filter_t,
        filter_count: usize,
        catalog: *const u8,
        catalog_len: usize,
    ) -> *mut negs_catalog_t;
    pub fn negs_catalog_free(catalog: *mut negs_catalog_t);

    pub fn negs_facts_new(catalog: *const negs_catalog_t) -> *mut negs_facts_t;
    pub fn negs_facts_free(facts: *mut negs_facts_t);
    pub fn negs_facts_update(facts: *mut negs_facts_t, state: *const negs_state_t, input: *const negs_facts_input_t, flags: u32);
    pub fn negs_facts_set_host(facts: *mut negs_facts_t, host: *const negs_facts_host_t);
    pub fn negs_facts_value(facts: *mut negs_facts_t, path: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_facts_default_value(facts: *mut negs_facts_t, path: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_facts_is_modified(facts: *mut negs_facts_t, path: *const u8, len: usize) -> u8;
    pub fn negs_facts_flags(facts: *mut negs_facts_t) -> u32;
    pub fn negs_facts_brightness(facts: *mut negs_facts_t, out: *mut negs_facts_brightness_t);
    pub fn negs_facts_note(facts: *mut negs_facts_t, out: *mut negs_str_t) -> u8;
    pub fn negs_facts_color_filter(facts: *mut negs_facts_t, key: *mut negs_str_t) -> i64;
    pub fn negs_facts_color_filter_stops(facts: *mut negs_facts_t, key: *const u8, len: usize, out: *mut f64) -> u8;
    pub fn negs_facts_film_stock(facts: *mut negs_facts_t) -> i64;
    pub fn negs_facts_revealed(facts: *mut negs_facts_t, key: *const u8, len: usize) -> u8;
    pub fn negs_facts_preview_space(facts: *mut negs_facts_t) -> *const c_char;
    pub fn negs_preview_space(chosen: *const u8, len: usize) -> *const c_char;
    pub fn negs_facts_choice(facts: *mut negs_facts_t, which: u32) -> u32;
    pub fn negs_facts_stock_times(facts: *mut negs_facts_t, paper: u8, count: *mut usize) -> *const f64;
    pub fn negs_facts_photo_pixels(facts: *mut negs_facts_t, width: *mut u32, height: *mut u32) -> u8;

    pub fn negs_edits_count(facts: *mut negs_facts_t, tab: u32) -> usize;
    pub fn negs_edits_list(facts: *mut negs_facts_t, tab: u32, decimal: u32, count: *mut usize) -> *const negs_edit_t;
    pub fn negs_edits_group_count(facts: *mut negs_facts_t, index: usize) -> usize;
    pub fn negs_edits_section_count(facts: *mut negs_facts_t, section: u32) -> usize;
    pub fn negs_edits_other_count(facts: *mut negs_facts_t, which: u32) -> usize;
    pub fn negs_edits_shows_section(facts: *mut negs_facts_t, section: u32) -> u8;
    pub fn negs_edits_group_shows(facts: *mut negs_facts_t, index: usize) -> u8;
    pub fn negs_edits_control_shows(facts: *mut negs_facts_t, index: usize, place: usize) -> u8;
    pub fn negs_edits_module_more(facts: *mut negs_facts_t, index: usize, out: *mut negs_more_t) -> u8;
    pub fn negs_edits_section_more(facts: *mut negs_facts_t, section: u32, out: *mut negs_more_t) -> u8;
    pub fn negs_edits_section_title(section: u32) -> *const c_char;
    pub fn negs_edits_tab_title(tab: u32) -> *const c_char;
    pub fn negs_edits_words(text: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;
    pub fn negs_edits_paths(which: u32, count: *mut usize) -> *const *const c_char;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_core() {
        assert_eq!(layout("negs_str_t"), size_of::<negs_str_t>() as i64);
        assert_eq!(layout("negs_str_t.data"), offset_of!(negs_str_t, data) as i64);
        assert_eq!(layout("negs_str_t.len"), offset_of!(negs_str_t, len) as i64);
        // Phase 4a
        assert_eq!(layout("negs_stock_t"), size_of::<negs_stock_t>() as i64);
        assert_eq!(layout("negs_stock_t.id"), offset_of!(negs_stock_t, id) as i64);
        assert_eq!(layout("negs_stock_t.name"), offset_of!(negs_stock_t, name) as i64);
        assert_eq!(layout("negs_stock_t.target"), offset_of!(negs_stock_t, target) as i64);
        assert_eq!(layout("negs_stock_t.grain"), offset_of!(negs_stock_t, grain) as i64);
        assert_eq!(layout("negs_stock_t.times"), offset_of!(negs_stock_t, times) as i64);
        assert_eq!(layout("negs_stock_t.times_len"), offset_of!(negs_stock_t, times_len) as i64);
        assert_eq!(layout("negs_stock_t.paper"), offset_of!(negs_stock_t, paper) as i64);
        assert_eq!(layout("negs_stock_t.positive"), offset_of!(negs_stock_t, positive) as i64);
        assert_eq!(layout("negs_stock_t.bw"), offset_of!(negs_stock_t, bw) as i64);
        assert_eq!(layout("negs_stock_t.fotufilm"), offset_of!(negs_stock_t, fotufilm) as i64);
        assert_eq!(layout("negs_stock_t.manufacturer_data"), offset_of!(negs_stock_t, manufacturer_data) as i64);
        assert_eq!(layout("negs_stock_t.negs_built"), offset_of!(negs_stock_t, negs_built) as i64);
        assert_eq!(layout("negs_stock_t.cine_print"), offset_of!(negs_stock_t, cine_print) as i64);
        assert_eq!(layout("negs_color_filter_t"), size_of::<negs_color_filter_t>() as i64);
        assert_eq!(layout("negs_color_filter_t.key"), offset_of!(negs_color_filter_t, key) as i64);
        assert_eq!(layout("negs_color_filter_t.name"), offset_of!(negs_color_filter_t, name) as i64);
        assert_eq!(layout("negs_color_filter_t.glass"), offset_of!(negs_color_filter_t, glass) as i64);
        assert_eq!(layout("negs_color_filter_t.colour"), offset_of!(negs_color_filter_t, colour) as i64);
        assert_eq!(layout("negs_adjustment_t"), size_of::<negs_adjustment_t>() as i64);
        assert_eq!(layout("negs_adjustment_t.kind"), offset_of!(negs_adjustment_t, kind) as i64);
        assert_eq!(layout("negs_adjustment_t.title"), offset_of!(negs_adjustment_t, title) as i64);
        assert_eq!(layout("negs_adjustment_t.path"), offset_of!(negs_adjustment_t, path) as i64);
        assert_eq!(layout("negs_adjustment_t.help"), offset_of!(negs_adjustment_t, help) as i64);
        assert_eq!(layout("negs_adjustment_t.low"), offset_of!(negs_adjustment_t, low) as i64);
        assert_eq!(layout("negs_adjustment_t.high"), offset_of!(negs_adjustment_t, high) as i64);
        assert_eq!(layout("negs_adjustment_t.step"), offset_of!(negs_adjustment_t, step) as i64);
        assert_eq!(layout("negs_adjustment_t.options"), offset_of!(negs_adjustment_t, options) as i64);
        assert_eq!(layout("negs_adjustment_t.options_len"), offset_of!(negs_adjustment_t, options_len) as i64);
        assert_eq!(layout("negs_adjustment_group_t"), size_of::<negs_adjustment_group_t>() as i64);
        assert_eq!(layout("negs_adjustment_group_t.title"), offset_of!(negs_adjustment_group_t, title) as i64);
        assert_eq!(layout("negs_adjustment_group_t.section"), offset_of!(negs_adjustment_group_t, section) as i64);
        assert_eq!(layout("negs_adjustment_group_t.enable"), offset_of!(negs_adjustment_group_t, enable) as i64);
        assert_eq!(layout("negs_adjustment_group_t.basic"), offset_of!(negs_adjustment_group_t, basic) as i64);
        assert_eq!(layout("negs_adjustment_group_t.controls"), offset_of!(negs_adjustment_group_t, controls) as i64);
        assert_eq!(layout("negs_adjustment_group_t.controls_len"), offset_of!(negs_adjustment_group_t, controls_len) as i64);
        assert_eq!(layout("negs_choice_t"), size_of::<negs_choice_t>() as i64);
        assert_eq!(layout("negs_choice_t.film"), offset_of!(negs_choice_t, film) as i64);
        assert_eq!(layout("negs_choice_t.paper"), offset_of!(negs_choice_t, paper) as i64);
        assert_eq!(layout("negs_choice_t.nulls"), offset_of!(negs_choice_t, nulls) as i64);
        assert_eq!(layout("negs_choice_t.sets_scan"), offset_of!(negs_choice_t, sets_scan) as i64);
        assert_eq!(layout("negs_choice_t.scan"), offset_of!(negs_choice_t, scan) as i64);
        assert_eq!(layout("negs_stock_group_t"), size_of::<negs_stock_group_t>() as i64);
        assert_eq!(layout("negs_stock_group_t.title"), offset_of!(negs_stock_group_t, title) as i64);
        assert_eq!(layout("negs_stock_group_t.start"), offset_of!(negs_stock_group_t, start) as i64);
        assert_eq!(layout("negs_stock_group_t.count"), offset_of!(negs_stock_group_t, count) as i64);
        assert_eq!(layout("negs_facts_prefs_t"), size_of::<negs_facts_prefs_t>() as i64);
        assert_eq!(layout("negs_facts_prefs_t.more_adjustments"), offset_of!(negs_facts_prefs_t, more_adjustments) as i64);
        assert_eq!(layout("negs_facts_prefs_t.color_space"), offset_of!(negs_facts_prefs_t, color_space) as i64);
        assert_eq!(layout("negs_facts_prefs_t.output_format"), offset_of!(negs_facts_prefs_t, output_format) as i64);
        assert_eq!(layout("negs_facts_prefs_t.filter_shift_link"), offset_of!(negs_facts_prefs_t, filter_shift_link) as i64);
        assert_eq!(layout("negs_facts_prefs_t.tone_graph_channel"), offset_of!(negs_facts_prefs_t, tone_graph_channel) as i64);
        assert_eq!(layout("negs_facts_prefs_t.jpeg_quality"), offset_of!(negs_facts_prefs_t, jpeg_quality) as i64);
        assert_eq!(layout("negs_facts_prefs_t.png_bit_depth"), offset_of!(negs_facts_prefs_t, png_bit_depth) as i64);
        assert_eq!(layout("negs_facts_prefs_t.tiff_bit_depth"), offset_of!(negs_facts_prefs_t, tiff_bit_depth) as i64);
        assert_eq!(layout("negs_facts_prefs_t.lab_all_controls"), offset_of!(negs_facts_prefs_t, lab_all_controls) as i64);
        assert_eq!(layout("negs_facts_prefs_t.match_new_photos"), offset_of!(negs_facts_prefs_t, match_new_photos) as i64);
        assert_eq!(layout("negs_facts_negative_t"), size_of::<negs_facts_negative_t>() as i64);
        assert_eq!(layout("negs_facts_negative_t.sentence"), offset_of!(negs_facts_negative_t, sentence) as i64);
        assert_eq!(layout("negs_facts_negative_t.warning"), offset_of!(negs_facts_negative_t, warning) as i64);
        assert_eq!(layout("negs_facts_negative_t.edge"), offset_of!(negs_facts_negative_t, edge) as i64);
        assert_eq!(layout("negs_facts_negative_t.flat_path"), offset_of!(negs_facts_negative_t, flat_path) as i64);
        assert_eq!(layout("negs_facts_negative_t.flat_reason"), offset_of!(negs_facts_negative_t, flat_reason) as i64);
        assert_eq!(layout("negs_facts_negative_t.has_warning"), offset_of!(negs_facts_negative_t, has_warning) as i64);
        assert_eq!(layout("negs_facts_negative_t.has_base"), offset_of!(negs_facts_negative_t, has_base) as i64);
        assert_eq!(layout("negs_facts_negative_t.base_found"), offset_of!(negs_facts_negative_t, base_found) as i64);
        assert_eq!(layout("negs_facts_negative_t.base_picked"), offset_of!(negs_facts_negative_t, base_picked) as i64);
        assert_eq!(layout("negs_facts_negative_t.base_by_hand"), offset_of!(negs_facts_negative_t, base_by_hand) as i64);
        assert_eq!(layout("negs_facts_negative_t.base_from_roll"), offset_of!(negs_facts_negative_t, base_from_roll) as i64);
        assert_eq!(layout("negs_facts_negative_t.has_edge"), offset_of!(negs_facts_negative_t, has_edge) as i64);
        assert_eq!(layout("negs_facts_negative_t.has_flat_used"), offset_of!(negs_facts_negative_t, has_flat_used) as i64);
        assert_eq!(layout("negs_facts_negative_t.flat_used"), offset_of!(negs_facts_negative_t, flat_used) as i64);
        assert_eq!(layout("negs_facts_negative_t.has_flat_reason"), offset_of!(negs_facts_negative_t, has_flat_reason) as i64);
        assert_eq!(layout("negs_facts_negative_t.busy"), offset_of!(negs_facts_negative_t, busy) as i64);
        assert_eq!(layout("negs_facts_negative_t.backlight_shown"), offset_of!(negs_facts_negative_t, backlight_shown) as i64);
        assert_eq!(layout("negs_facts_input_t"), size_of::<negs_facts_input_t>() as i64);
        assert_eq!(layout("negs_facts_input_t.prefs"), offset_of!(negs_facts_input_t, prefs) as i64);
        assert_eq!(layout("negs_facts_input_t.photo_size"), offset_of!(negs_facts_input_t, photo_size) as i64);
        assert_eq!(layout("negs_facts_input_t.preview_scale"), offset_of!(negs_facts_input_t, preview_scale) as i64);
        assert_eq!(layout("negs_facts_input_t.generation"), offset_of!(negs_facts_input_t, generation) as i64);
        assert_eq!(layout("negs_facts_input_t.failure_generation"), offset_of!(negs_facts_input_t, failure_generation) as i64);
        assert_eq!(layout("negs_facts_input_t.failure_inputs"), offset_of!(negs_facts_input_t, failure_inputs) as i64);
        assert_eq!(layout("negs_facts_input_t.failure_message"), offset_of!(negs_facts_input_t, failure_message) as i64);
        assert_eq!(layout("negs_facts_input_t.size_error"), offset_of!(negs_facts_input_t, size_error) as i64);
        assert_eq!(layout("negs_facts_input_t.negative"), offset_of!(negs_facts_input_t, negative) as i64);
        assert_eq!(layout("negs_facts_input_t.source_width"), offset_of!(negs_facts_input_t, source_width) as i64);
        assert_eq!(layout("negs_facts_input_t.source_height"), offset_of!(negs_facts_input_t, source_height) as i64);
        assert_eq!(layout("negs_facts_input_t.is_raw"), offset_of!(negs_facts_input_t, is_raw) as i64);
        assert_eq!(layout("negs_facts_input_t.lens_supported"), offset_of!(negs_facts_input_t, lens_supported) as i64);
        assert_eq!(layout("negs_facts_input_t.has_source"), offset_of!(negs_facts_input_t, has_source) as i64);
        assert_eq!(layout("negs_facts_input_t.image_shown"), offset_of!(negs_facts_input_t, image_shown) as i64);
        assert_eq!(layout("negs_facts_input_t.has_size_error"), offset_of!(negs_facts_input_t, has_size_error) as i64);
        assert_eq!(layout("negs_facts_input_t.custom_film_format"), offset_of!(negs_facts_input_t, custom_film_format) as i64);
        assert_eq!(layout("negs_facts_input_t.balancing_print"), offset_of!(negs_facts_input_t, balancing_print) as i64);
        assert_eq!(layout("negs_facts_input_t.can_balance_print"), offset_of!(negs_facts_input_t, can_balance_print) as i64);
        assert_eq!(layout("negs_facts_input_t.can_export"), offset_of!(negs_facts_input_t, can_export) as i64);
        assert_eq!(layout("negs_facts_input_t.cropping"), offset_of!(negs_facts_input_t, cropping) as i64);
        assert_eq!(layout("negs_facts_input_t.has_failure"), offset_of!(negs_facts_input_t, has_failure) as i64);
        assert_eq!(layout("negs_facts_input_t.failure_has_inputs"), offset_of!(negs_facts_input_t, failure_has_inputs) as i64);
        assert_eq!(layout("negs_facts_input_t.matching"), offset_of!(negs_facts_input_t, matching) as i64);
        assert_eq!(layout("negs_facts_input_t.has_negative"), offset_of!(negs_facts_input_t, has_negative) as i64);
        assert_eq!(layout("negs_facts_exposure_t"), size_of::<negs_facts_exposure_t>() as i64);
        assert_eq!(layout("negs_facts_exposure_t.offset"), offset_of!(negs_facts_exposure_t, offset) as i64);
        assert_eq!(layout("negs_facts_exposure_t.modified"), offset_of!(negs_facts_exposure_t, modified) as i64);
        assert_eq!(layout("negs_facts_host_t"), size_of::<negs_facts_host_t>() as i64);
        assert_eq!(layout("negs_facts_host_t.user"), offset_of!(negs_facts_host_t, user) as i64);
        assert_eq!(layout("negs_facts_host_t.is_modified"), offset_of!(negs_facts_host_t, is_modified) as i64);
        assert_eq!(layout("negs_facts_host_t.value"), offset_of!(negs_facts_host_t, value) as i64);
        assert_eq!(layout("negs_facts_host_t.flags"), offset_of!(negs_facts_host_t, flags) as i64);
        assert_eq!(layout("negs_facts_host_t.exposure"), offset_of!(negs_facts_host_t, exposure) as i64);
        assert_eq!(layout("negs_facts_host_t.color_filter"), offset_of!(negs_facts_host_t, color_filter) as i64);
        assert_eq!(layout("negs_facts_brightness_t"), size_of::<negs_facts_brightness_t>() as i64);
        assert_eq!(layout("negs_facts_brightness_t.matched_ev"), offset_of!(negs_facts_brightness_t, matched_ev) as i64);
        assert_eq!(layout("negs_facts_brightness_t.exposure_offset"), offset_of!(negs_facts_brightness_t, exposure_offset) as i64);
        assert_eq!(layout("negs_facts_brightness_t.range_low"), offset_of!(negs_facts_brightness_t, range_low) as i64);
        assert_eq!(layout("negs_facts_brightness_t.range_high"), offset_of!(negs_facts_brightness_t, range_high) as i64);
        assert_eq!(layout("negs_edit_t"), size_of::<negs_edit_t>() as i64);
        assert_eq!(layout("negs_edit_t.title"), offset_of!(negs_edit_t, title) as i64);
        assert_eq!(layout("negs_edit_t.value"), offset_of!(negs_edit_t, value) as i64);
        assert_eq!(layout("negs_edit_t.target"), offset_of!(negs_edit_t, target) as i64);
        assert_eq!(layout("negs_edit_t.path"), offset_of!(negs_edit_t, path) as i64);
        assert_eq!(layout("negs_more_t"), size_of::<negs_more_t>() as i64);
        assert_eq!(layout("negs_more_t.text"), offset_of!(negs_more_t, text) as i64);
        assert_eq!(layout("negs_more_t.key"), offset_of!(negs_more_t, key) as i64);
        assert_eq!(layout("negs_more_t.action"), offset_of!(negs_more_t, action) as i64);
    }
}
