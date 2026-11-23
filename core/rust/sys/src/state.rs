//! Phase 4 base: value, edit state (core/src/value, core/src/state), and
//! Phase 4a: state rules, edits, inspector, below it.

#[allow(unused_imports)]
use super::*;
pub use super::picture::negs_orientation_t;
use std::os::raw::c_char;

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
    }
}
