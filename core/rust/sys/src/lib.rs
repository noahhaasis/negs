//! Raw declarations of core/include/negs_core.h. Each section of the header
//! has its module here; the safe wrappers live in core/rust/<module>.

#![allow(non_camel_case_types)]

use std::os::raw::c_char;

pub mod marks;
pub mod library;
pub mod theme;

// Phase 3a
pub mod sheet;

// Phase 3b
pub mod picture;

// Phase 3c
pub mod media;

/// NEGS_CORE_ABI_VERSION of the header these declarations follow.
pub const NEGS_CORE_ABI_VERSION: u32 = 4;

#[repr(C)]
#[derive(Debug)]
pub struct negs_buf_t {
    pub data: *mut u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_alloc_stats_t {
    pub live_bytes: i64,
    pub live_count: i64,
    pub total_count: i64,
    pub bad_frees: i64,
    pub tracked: u8,
}

unsafe extern "C" {
    pub fn negs_core_abi_version() -> u32;
    pub fn negs_core_version() -> *const c_char;
    pub fn negs_core_alloc_stats(out: *mut negs_alloc_stats_t);
    pub fn negs_buf_copy(data: *const u8, len: usize) -> negs_buf_t;
    pub fn negs_buf_free(buf: negs_buf_t);
    pub fn negs_core_layout(name: *const c_char, name_len: usize) -> i64;
}

/// The core's allocations now (zeros unless built with the `track` feature).
pub fn alloc_stats() -> negs_alloc_stats_t {
    let mut stats = negs_alloc_stats_t::default();
    unsafe { negs_core_alloc_stats(&mut stats) };
    stats
}

/// Takes a block the core returned: its bytes, and the block freed.
pub fn take_buf(buf: negs_buf_t) -> Vec<u8> {
    if buf.data.is_null() {
        return Vec::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(buf.data, buf.len) }.to_vec();
    unsafe { negs_buf_free(buf) };
    bytes
}

/// The layout the core reports for a header name ("negs_buf_t.len").
pub fn layout(name: &str) -> i64 {
    unsafe { negs_core_layout(name.as_ptr().cast(), name.len()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_the_core_it_was_written_for() {
        assert_eq!(unsafe { negs_core_abi_version() }, NEGS_CORE_ABI_VERSION);
        let version = unsafe { std::ffi::CStr::from_ptr(negs_core_version()) }.to_str().unwrap();
        assert!(version.contains("odin"), "{version}");
        assert_eq!(layout("negs_buf_t"), std::mem::size_of::<negs_buf_t>() as i64);
        assert_eq!(layout("negs_buf_t.len"), std::mem::offset_of!(negs_buf_t, len) as i64);
        assert_eq!(layout("negs_alloc_stats_t"), std::mem::size_of::<negs_alloc_stats_t>() as i64);
        assert_eq!(layout("negs_alloc_stats_t.tracked"), std::mem::offset_of!(negs_alloc_stats_t, tracked) as i64);
    }

    #[test]
    fn blocks_come_back_whole_and_are_freed() {
        let before = alloc_stats();
        let buf = unsafe { negs_buf_copy([4u8, 5, 6].as_ptr(), 3) };
        assert_eq!(take_buf(buf), vec![4, 5, 6]);
        let after = alloc_stats();
        if after.tracked == 1 {
            assert_eq!(after.live_count, before.live_count);
            assert_eq!(after.total_count, before.total_count + 1);
        }
    }
}
