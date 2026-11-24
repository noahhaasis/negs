//! Phase 4c: batch, match cache, rendered preview (core/src/caches).

#[allow(unused_imports)]
use super::*;
pub use super::state::negs_str_t;
use std::os::raw::c_char;

pub const NEGS_PREVIEW_RENDERED: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_batch_save_t {
    pub bit_depth: u32,
    pub color_space: u64,
    pub jpeg_quality: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_batch_run_t {
    pub statuses: *const u8,
    pub count: usize,
    pub running: u8,
    pub cancelled: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_batch_counts_t {
    pub total: u64,
    pub remaining: u64,
    pub finished: u64,
    pub exported: u64,
    pub failed: u64,
    pub skipped: u64,
    pub fraction: f64,
    pub exported_part: f64,
    pub other_part: f64,
    pub needs_attention: u8,
}

/// One match cache, owned by the caller until `negs_match_cache_free`.
#[repr(C)]
pub struct negs_match_cache_t {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_preview_frame_t {
    pub width: u32,
    pub height: u32,
    pub photo_width: f64,
    pub photo_height: f64,
    pub source_width: f64,
    pub source_height: f64,
    pub working_width: f64,
    pub working_height: f64,
    pub raw: u8,
    pub lens_correction_supported: u8,
}

unsafe extern "C" {
    pub fn negs_caches_path(
        op: u32,
        style: u32,
        path: *const u8,
        path_len: usize,
        arg: *const u8,
        arg_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_caches_secs_f64(secs: u64, nanos: u32) -> f64;

    pub fn negs_batch_key(table: u32, index: u32) -> *const c_char;
    pub fn negs_batch_from_key(table: u32, key: *const u8, len: usize) -> u32;
    pub fn negs_batch_options_default() -> negs_buf_t;
    pub fn negs_batch_options_save(
        format: *const u8,
        len: usize,
        bit_depth: u32,
        color_space: u64,
        jpeg_quality: u32,
        out: *mut negs_batch_save_t,
    );
    pub fn negs_batch_file_name(
        naming: u32,
        custom: *const u8,
        custom_len: usize,
        name: *const u8,
        name_len: usize,
        film: *const u8,
        film_len: usize,
        format: *const u8,
        format_len: usize,
    ) -> negs_buf_t;
    pub fn negs_batch_name(photo: *const u8, photo_len: usize, title: *const u8, title_len: usize) -> negs_buf_t;
    pub fn negs_batch_numbered_name(file_name: *const u8, len: usize, n: u64) -> negs_buf_t;
    pub fn negs_batch_unique(names: *const negs_str_t, count: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_batch_matched_exposure(
        exposure: *const u8,
        len: usize,
        has_old: u8,
        old: f64,
        ev: f64,
        total: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_batch_matched(
        state: *const u8,
        len: usize,
        ev: f64,
        limited: u8,
        inputs: *const u8,
        inputs_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;

    pub fn negs_batch_run_counts(run: *const negs_batch_run_t, out: *mut negs_batch_counts_t);
    pub fn negs_batch_run_title(run: *const negs_batch_run_t) -> negs_buf_t;
    pub fn negs_batch_run_outcome(run: *const negs_batch_run_t, words: *mut negs_buf_t, failed: *mut negs_buf_t) -> u8;
    pub fn negs_batch_run_finished(run: *const negs_batch_run_t, destination: *const u8, len: usize) -> negs_buf_t;
    pub fn negs_batch_run_progress(
        run: *const negs_batch_run_t,
        lightroom: *const u8,
        len: usize,
        has_lightroom: u8,
    ) -> negs_buf_t;
    pub fn negs_batch_lightroom_title(key: *const u8, len: usize) -> *const c_char;
    pub fn negs_batch_exporting(
        finished: u64,
        total: u64,
        lightroom: *const u8,
        len: usize,
        has_lightroom: u8,
    ) -> negs_buf_t;
    pub fn negs_batch_message(
        count: u64,
        engine: u32,
        edge: u32,
        pixels: u32,
        save: *const negs_batch_save_t,
        preview_space: *const u8,
        len: usize,
    ) -> negs_buf_t;

    pub fn negs_batch_journal_rewrite(text: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_batch_journal_write(
        path: *const u8,
        path_len: usize,
        json: *const u8,
        len: usize,
        error: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_batch_journal_write_bytes(
        path: *const u8,
        path_len: usize,
        bytes: *const u8,
        len: usize,
        error: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_batch_journal_read(path: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_batch_journal_remove(path: *const u8, len: usize);

    pub fn negs_match_key(
        file: *const u8,
        file_len: usize,
        film: *const u8,
        film_len: usize,
        paper: *const u8,
        paper_len: usize,
        params: *const u8,
        params_len: usize,
        engine: *const u8,
        engine_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_match_engine_stamp(
        version: *const u8,
        version_len: usize,
        helper: *const u8,
        helper_len: usize,
        data: *const u8,
        data_len: usize,
    ) -> negs_buf_t;
    pub fn negs_match_cache_limit() -> u64;
    pub fn negs_match_cache_new(file: *const u8, len: usize, limit: u64) -> *mut negs_match_cache_t;
    pub fn negs_match_cache_free(cache: *mut negs_match_cache_t);
    pub fn negs_match_cache_solved(
        cache: *mut negs_match_cache_t,
        key: *const u8,
        len: usize,
        now: f64,
        ev: *mut f64,
        limited: *mut u8,
    ) -> u8;
    pub fn negs_match_cache_store(
        cache: *mut negs_match_cache_t,
        key: *const u8,
        len: usize,
        ev: f64,
        limited: u8,
        now: f64,
    ) -> u8;
    pub fn negs_match_cache_flush_bytes(cache: *mut negs_match_cache_t, out: *mut negs_buf_t) -> u8;
    pub fn negs_match_cache_write(file: *const u8, file_len: usize, bytes: *const u8, len: usize);
    pub fn negs_match_cache_count(cache: *mut negs_match_cache_t) -> u64;

    pub fn negs_preview_key(
        source: *const u8,
        source_len: usize,
        helper: *const u8,
        helper_len: usize,
        data: *const u8,
        data_len: usize,
        state: *const u8,
        state_len: usize,
        flags: u32,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_preview_checksum(bytes: *const u8, len: usize) -> u64;
    pub fn negs_preview_read(
        file: *const u8,
        file_len: usize,
        key: *const u8,
        key_len: usize,
        frame: *mut negs_preview_frame_t,
        display_space: *mut negs_buf_t,
        file_bytes: *mut negs_buf_t,
        pixels_offset: *mut usize,
    ) -> u8;
    pub fn negs_preview_write(
        file: *const u8,
        file_len: usize,
        key: *const u8,
        key_len: usize,
        frame: *const negs_preview_frame_t,
        display_space: *const u8,
        display_len: usize,
        pixels: *const u8,
        len: usize,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn cache_structs_match_the_core() {
        assert_eq!(layout("negs_batch_save_t"), size_of::<negs_batch_save_t>() as i64);
        assert_eq!(layout("negs_batch_save_t.color_space"), offset_of!(negs_batch_save_t, color_space) as i64);
        assert_eq!(layout("negs_batch_save_t.jpeg_quality"), offset_of!(negs_batch_save_t, jpeg_quality) as i64);
        assert_eq!(layout("negs_batch_run_t"), size_of::<negs_batch_run_t>() as i64);
        assert_eq!(layout("negs_batch_run_t.count"), offset_of!(negs_batch_run_t, count) as i64);
        assert_eq!(layout("negs_batch_run_t.cancelled"), offset_of!(negs_batch_run_t, cancelled) as i64);
        assert_eq!(layout("negs_batch_counts_t"), size_of::<negs_batch_counts_t>() as i64);
        assert_eq!(layout("negs_batch_counts_t.fraction"), offset_of!(negs_batch_counts_t, fraction) as i64);
        assert_eq!(layout("negs_batch_counts_t.needs_attention"), offset_of!(negs_batch_counts_t, needs_attention) as i64);
        assert_eq!(layout("negs_preview_frame_t"), size_of::<negs_preview_frame_t>() as i64);
        assert_eq!(layout("negs_preview_frame_t.photo_width"), offset_of!(negs_preview_frame_t, photo_width) as i64);
        assert_eq!(layout("negs_preview_frame_t.raw"), offset_of!(negs_preview_frame_t, raw) as i64);
        assert_eq!(
            layout("negs_preview_frame_t.lens_correction_supported"),
            offset_of!(negs_preview_frame_t, lens_correction_supported) as i64
        );
    }
}
