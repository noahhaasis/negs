//! negs_core.h, "Library": a folder's photographs, their sidecars and undo
//! behind an opaque handle, JSON in and out (shapes in the header).

use crate::negs_buf_t;

/// NegsLibrary, opaque.
#[repr(C)]
pub struct NegsLibrary {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct negs_bin_t {
    pub user: *mut std::ffi::c_void,
    pub name: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void) -> negs_buf_t>,
    pub available: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, folder: *const u8, folder_len: usize) -> u8>,
    pub trash: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, file: *const u8, file_len: usize, out: *mut negs_buf_t) -> u8>,
    pub put_back: Option<
        unsafe extern "C" fn(
            user: *mut std::ffi::c_void,
            trashed: *const u8,
            trashed_len: usize,
            original: *const u8,
            original_len: usize,
            error: *mut negs_buf_t,
        ) -> u8,
    >,
    pub remove: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, file: *const u8, file_len: usize, error: *mut negs_buf_t) -> u8>,
    pub release: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct negs_library_config_t {
    pub folder: *const u8,
    pub folder_len: usize,
    pub data: *const u8,
    pub data_len: usize,
    pub app: *const u8,
    pub app_len: usize,
    pub bin: negs_bin_t,
    pub pairing: u8,
    pub sidecars: u8,
    pub has_id_seed: u8,
    pub id_seed: u64,
}

pub type negs_transform_fn =
    unsafe extern "C" fn(user: *mut std::ffi::c_void, item: *const u8, item_len: usize, state: *const u8, state_len: usize) -> negs_buf_t;

unsafe extern "C" {
    pub fn negs_library_new(config: *const negs_library_config_t, error: *mut negs_buf_t) -> *mut NegsLibrary;
    pub fn negs_library_free(library: *mut NegsLibrary);

    pub fn negs_library_snapshot(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_items(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_shown(library: *mut NegsLibrary, current: *const u8, current_len: usize) -> negs_buf_t;
    pub fn negs_library_grid(library: *mut NegsLibrary, current: *const u8, current_len: usize) -> negs_buf_t;
    pub fn negs_library_rejected(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_name(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_summary(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_thumbnail_state(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_export_edits(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_store_solved_match(
        library: *mut NegsLibrary,
        item: *const u8,
        item_len: usize,
        look: *const u8,
        look_len: usize,
        state: *const u8,
        state_len: usize,
    ) -> u8;
    pub fn negs_library_unedited_state(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_defaults(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_copied(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_previous_edits(library: *mut NegsLibrary) -> negs_buf_t;

    pub fn negs_library_set_defaults(library: *mut NegsLibrary, defaults: *const u8, defaults_len: usize) -> negs_buf_t;
    pub fn negs_library_set_filter(library: *mut NegsLibrary, filter: *const u8, filter_len: usize) -> u8;
    pub fn negs_library_set_pairing(library: *mut NegsLibrary, pairing: u8);
    pub fn negs_library_set_sidecars(library: *mut NegsLibrary, on: u8);
    pub fn negs_library_refresh(library: *mut NegsLibrary, interacting: u8) -> negs_buf_t;

    pub fn negs_library_state(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_open_edited(library: *mut NegsLibrary) -> u8;
    pub fn negs_library_unsaved(library: *mut NegsLibrary) -> u8;
    pub fn negs_library_open(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_close(library: *mut NegsLibrary, error: *mut negs_buf_t) -> u8;
    pub fn negs_library_save(library: *mut NegsLibrary, error: *mut negs_buf_t) -> u8;
    pub fn negs_library_edit(library: *mut NegsLibrary, state: *const u8, state_len: usize) -> u8;
    pub fn negs_library_opening_state(library: *mut NegsLibrary, item: *const u8, item_len: usize) -> negs_buf_t;
    pub fn negs_library_new_id(library: *mut NegsLibrary) -> negs_buf_t;

    pub fn negs_library_set_rating(library: *mut NegsLibrary, items: *const u8, items_len: usize, stars: i64) -> negs_buf_t;
    pub fn negs_library_change_rating(library: *mut NegsLibrary, items: *const u8, items_len: usize, by: i64) -> negs_buf_t;
    pub fn negs_library_set_flag(library: *mut NegsLibrary, items: *const u8, items_len: usize, flag: u8) -> negs_buf_t;
    pub fn negs_library_copy_edits(library: *mut NegsLibrary, item: *const u8, item_len: usize, groups: *const u8, groups_len: usize) -> negs_buf_t;
    pub fn negs_library_paste_edits(library: *mut NegsLibrary, items: *const u8, items_len: usize) -> negs_buf_t;
    pub fn negs_library_paste_from_previous(library: *mut NegsLibrary, items: *const u8, items_len: usize) -> negs_buf_t;
    pub fn negs_library_reset_edits(library: *mut NegsLibrary, items: *const u8, items_len: usize) -> negs_buf_t;
    pub fn negs_library_change_each_edit(
        library: *mut NegsLibrary,
        items: *const u8,
        items_len: usize,
        action: *const u8,
        action_len: usize,
        note: *const u8,
        note_len: usize,
        transform: negs_transform_fn,
        user: *mut std::ffi::c_void,
    ) -> negs_buf_t;
    pub fn negs_library_duplicate(library: *mut NegsLibrary, items: *const u8, items_len: usize) -> negs_buf_t;

    pub fn negs_library_delete_plan(library: *mut NegsLibrary, items: *const u8, items_len: usize) -> negs_buf_t;
    pub fn negs_library_delete_question(library: *mut NegsLibrary, plan: *const u8, plan_len: usize) -> negs_buf_t;
    pub fn negs_library_delete_is_permanent(library: *mut NegsLibrary, plan: *const u8, plan_len: usize) -> u8;
    pub fn negs_library_delete(library: *mut NegsLibrary, plan: *const u8, plan_len: usize) -> negs_buf_t;

    pub fn negs_library_undo(library: *mut NegsLibrary) -> negs_buf_t;
    pub fn negs_library_redo(library: *mut NegsLibrary) -> negs_buf_t;

    pub fn negs_sidecar_rewrite(data: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_json_compact_sorted(data: *const u8, len: usize, out: *mut negs_buf_t) -> u8;
    pub fn negs_thumbnail_look(edits: *const u8, edits_len: usize, defaults_version: i64) -> negs_buf_t;
    pub fn negs_thumbnail_unedited_look(state: *const u8, state_len: usize) -> negs_buf_t;
    pub fn negs_thumbnail_key(
        path: *const u8,
        path_len: usize,
        has_size: u8,
        size: u64,
        modified: f64,
        variant: *const u8,
        variant_len: usize,
        look: *const u8,
        look_len: usize,
        engine: *const u8,
        engine_len: usize,
    ) -> negs_buf_t;
    pub fn negs_thumbnail_loose_key(
        path: *const u8,
        path_len: usize,
        has_size: u8,
        size: u64,
        modified: f64,
        variant: *const u8,
        variant_len: usize,
        look: *const u8,
        look_len: usize,
    ) -> negs_buf_t;
    pub fn negs_xmp_embed(data: *const u8, len: usize, rating: i32, out: *mut negs_buf_t) -> u8;
    pub fn negs_xmp_embed_in_file(path: *const u8, path_len: usize, rating: i32, error: *mut negs_buf_t) -> u8;
    pub fn negs_filter_words(filter: *const u8, filter_len: usize) -> negs_buf_t;
    pub fn negs_count_words(shown: u64, total: u64, selected: u64, filtered: u8) -> negs_buf_t;
    pub fn negs_standard_compare(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> i32;
    pub fn negs_can_write(folder: *const u8, folder_len: usize) -> u8;
    pub fn negs_selection_apply(state: *const u8, state_len: usize, op: *const u8, op_len: usize) -> negs_buf_t;
}

/// The size and field offsets the header gives the Library structs, for
/// the layout test.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;
    use std::mem::{offset_of, size_of};

    #[test]
    fn library_structs_match_the_core() {
        assert_eq!(layout("negs_bin_t"), size_of::<negs_bin_t>() as i64);
        assert_eq!(layout("negs_bin_t.release"), offset_of!(negs_bin_t, release) as i64);
        assert_eq!(layout("negs_library_config_t"), size_of::<negs_library_config_t>() as i64);
        assert_eq!(layout("negs_library_config_t.bin"), offset_of!(negs_library_config_t, bin) as i64);
        assert_eq!(layout("negs_library_config_t.has_id_seed"), offset_of!(negs_library_config_t, has_id_seed) as i64);
        assert_eq!(layout("negs_library_config_t.id_seed"), offset_of!(negs_library_config_t, id_seed) as i64);
    }
}
