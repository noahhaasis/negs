//! Phase 4b: presets, session (core/src/settings, negs_core.h "Phase 4b:
//! presets, session").

#[allow(unused_imports)]
use super::*;
pub use super::state::negs_str_t;

/// `negs_text_fn`: writes up to `cap` bytes of the result, returns its
/// whole length.
pub type negs_text_fn = unsafe extern "C" fn(user: *mut std::ffi::c_void, text: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_preset_file_t {
    pub path: *const u8,
    pub path_len: usize,
    pub size: u64,
    pub modified_s: i64,
    pub modified_ns: u32,
}

/// Owned by the caller until `negs_preset_list_free`.
#[repr(C)]
#[derive(Debug)]
pub struct negs_preset_list_t {
    pub files: *mut negs_preset_file_t,
    pub count: usize,
}

impl Default for negs_preset_list_t {
    fn default() -> Self {
        Self { files: std::ptr::null_mut(), count: 0 }
    }
}

unsafe extern "C" {
    pub fn negs_settings_text_functions(nfc: Option<negs_text_fn>, lowercase: Option<negs_text_fn>, user: *mut std::ffi::c_void);
    pub fn negs_preset_list_free(list: negs_preset_list_t);
    pub fn negs_presets_limit(which: u32) -> u64;
    pub fn negs_presets_list(folder: *const u8, folder_len: usize, out: *mut negs_preset_list_t, error: *mut negs_buf_t) -> u8;
    pub fn negs_presets_add(
        folder: *const u8,
        folder_len: usize,
        data: *const u8,
        data_len: usize,
        name: *const u8,
        name_len: usize,
        out: *mut negs_preset_list_t,
        error: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_presets_rename(
        folder: *const u8,
        folder_len: usize,
        preset: *const negs_preset_file_t,
        name: *const u8,
        name_len: usize,
        out: *mut negs_preset_list_t,
        error: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_presets_delete(folder: *const u8, folder_len: usize, preset: *const negs_preset_file_t, error: *mut negs_buf_t) -> u8;
    pub fn negs_presets_read(preset: *const negs_preset_file_t, out: *mut negs_buf_t, error: *mut negs_buf_t) -> u8;
    pub fn negs_presets_read_file(path: *const u8, path_len: usize, out: *mut negs_buf_t, error: *mut negs_buf_t) -> u8;
    pub fn negs_presets_set_order(
        folder: *const u8,
        folder_len: usize,
        presets: *const negs_preset_file_t,
        count: usize,
        error: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_presets_name_problem(name: *const u8, name_len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_presets_name_key(name: *const u8, name_len: usize) -> negs_buf_t;
    pub fn negs_presets_name_unique(name: *const u8, name_len: usize, taken: *const negs_str_t, count: usize) -> negs_buf_t;
    pub fn negs_presets_name_from_file_name(name: *const u8, name_len: usize) -> negs_buf_t;
    pub fn negs_presets_text_start(data: *const u8, len: usize) -> i64;

    pub fn negs_settings_path(which: u32, test: u8, out: *mut negs_buf_t) -> u8;
    pub fn negs_settings_write_atomic(path: *const u8, path_len: usize, data: *const u8, data_len: usize, error: *mut negs_buf_t) -> u8;

    pub fn negs_session_validate(text: *const u8, len: usize) -> u8;
    pub fn negs_session_migrate_state(text: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_session_rewrite(text: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_session_load(file: *const u8, file_len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_session_save(file: *const u8, file_len: usize, text: *const u8, len: usize, error: *mut negs_buf_t) -> u8;

    pub fn negs_preferences_rewrite(text: *const u8, len: usize, flags: u32, out: *mut negs_buf_t) -> u8;
    pub fn negs_preferences_default(flags: u32) -> negs_buf_t;
    pub fn negs_preferences_load(file: *const u8, file_len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_preferences_save(file: *const u8, file_len: usize, text: *const u8, len: usize) -> u8;
    pub fn negs_preferences_list_set(list: *const u8, list_len: usize, item: *const u8, item_len: usize, on: u8, sorted: u8) -> negs_buf_t;
    pub fn negs_preferences_list_has(list: *const u8, list_len: usize, item: *const u8, item_len: usize) -> u8;
    pub fn negs_preferences_tab(tab: *const u8, tab_len: usize, chosen: u8) -> u8;

    pub fn negs_catalog_load(
        file: *const u8,
        file_len: usize,
        helper: *const u8,
        helper_len: usize,
        data: *const u8,
        data_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_catalog_save(
        file: *const u8,
        file_len: usize,
        catalog: *const u8,
        catalog_len: usize,
        helper: *const u8,
        helper_len: usize,
        data: *const u8,
        data_len: usize,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_core() {
        assert_eq!(layout("negs_preset_file_t"), size_of::<negs_preset_file_t>() as i64);
        assert_eq!(layout("negs_preset_file_t.path"), offset_of!(negs_preset_file_t, path) as i64);
        assert_eq!(layout("negs_preset_file_t.path_len"), offset_of!(negs_preset_file_t, path_len) as i64);
        assert_eq!(layout("negs_preset_file_t.size"), offset_of!(negs_preset_file_t, size) as i64);
        assert_eq!(layout("negs_preset_file_t.modified_s"), offset_of!(negs_preset_file_t, modified_s) as i64);
        assert_eq!(layout("negs_preset_file_t.modified_ns"), offset_of!(negs_preset_file_t, modified_ns) as i64);
        assert_eq!(layout("negs_preset_list_t"), size_of::<negs_preset_list_t>() as i64);
        assert_eq!(layout("negs_preset_list_t.files"), offset_of!(negs_preset_list_t, files) as i64);
        assert_eq!(layout("negs_preset_list_t.count"), offset_of!(negs_preset_list_t, count) as i64);
    }
}
