//! crates/negs-library's API with its state machine in the shared Odin core
//! (core/src/library, through negs_core.h's Library section).
//!
//! What runs where:
//! - In Odin: `Library` (every call), `Selection` (every change), the
//!   thumbnail names (`thumbnail::look`, `unedited_look`, `key`,
//!   `loose_key`), XMP embedding (`xmp::embed`, `embed_in_file`), Finder's
//!   order (`folder::standard_compare`), `library::can_write`.
//! - In Rust, re-exported from crates/negs-library: the plain data types
//!   (`Item`, `Summary`, `EditState`, `EditDefaults`, `Filter`, `Outcome`,
//!   `DeletePlan`, …) with their pure methods, `Json`, the sidecar module's
//!   reading, patching and writing for hosts and tests, `edits`'s
//!   encode/decode/merge, the filter's words, `folder::listing`, SHA-256,
//!   and `FolderBin` (a bin is the host's: Odin calls it back).

mod codec;
mod library_impl;
mod selection_impl;

pub use library_impl::Library;
pub use oracle::{
    BrightnessMatch, Config, CopiedEdits, DefaultsTable, DeletePlan, EditDefaults, EditGroup, EditState, EditsFilter, ExportEdits, Filter,
    FlagFilter, FolderBin, Bin, Flag, Item, Json, Listing, Note, Opened, Outcome, Patch, PhotoFile, Problem, Question, RatingFilter, Roll,
    SavedEdits, Sidecar, Summary, Tables,
};
pub use selection_impl::Selection;

pub mod delete {
    pub use oracle::delete::*;
}
pub mod edits {
    pub use oracle::edits::*;
}
pub mod filter {
    pub use oracle::filter::*;
}
pub mod item {
    pub use oracle::item::*;
}
pub mod json {
    pub use oracle::json::*;
}
pub mod sha256 {
    pub use oracle::sha256::*;
}
pub mod sidecar {
    pub use oracle::sidecar::*;
}

pub mod folder {
    pub use oracle::folder::{is_camera_picture, is_photo, is_raw, listing, paired_raw, Listing, PhotoFile};
    pub use oracle::folder::{CAMERA_PICTURE_EXTENSIONS, IMAGE_EXTENSIONS, RAW_EXTENSIONS};

    /// Finder's order (`localizedStandardCompare`), from the core.
    pub fn standard_compare(a: &str, b: &str) -> std::cmp::Ordering {
        let c = unsafe { negs_core_sys::library::negs_standard_compare(a.as_ptr(), a.len(), b.as_ptr(), b.len()) };
        c.cmp(&0)
    }
}

pub mod library {
    pub use crate::Library;
    pub use oracle::library::{Config, CopiedEdits, ExportEdits, Note, Opened, Outcome};

    /// Whether sidecars can be written in `folder`, from the core.
    pub fn can_write(folder: &std::path::Path) -> bool {
        let text = folder.to_string_lossy();
        unsafe { negs_core_sys::library::negs_can_write(text.as_ptr(), text.len()) != 0 }
    }
}

pub mod selection {
    pub use crate::selection_impl::{next_after, Selection};
}

pub mod thumbnail {
    //! The names edited thumbnails are filed under, from the core.
    use crate::codec;
    use negs_core_sys::{library as sys, take_buf};
    pub use oracle::thumbnail::seconds_since_1970;
    use oracle::{EditState, Json};

    fn text(buf: negs_core_sys::negs_buf_t) -> String {
        String::from_utf8(take_buf(buf)).expect("the core returns UTF-8")
    }

    pub fn look(edits: &Json, defaults_version: i64) -> String {
        let json = edits.to_compact_sorted();
        text(unsafe { sys::negs_thumbnail_look(json.as_ptr(), json.len(), defaults_version) })
    }

    pub fn unedited_look(state: &EditState) -> String {
        let json = codec::state(state).to_compact_sorted();
        text(unsafe { sys::negs_thumbnail_unedited_look(json.as_ptr(), json.len()) })
    }

    pub fn key(path: &str, size: Option<u64>, modified: f64, variant: Option<&str>, look: &str, engine: &str) -> String {
        let v = variant.unwrap_or("");
        text(unsafe {
            sys::negs_thumbnail_key(
                path.as_ptr(),
                path.len(),
                size.is_some() as u8,
                size.unwrap_or(0),
                modified,
                v.as_ptr(),
                v.len(),
                look.as_ptr(),
                look.len(),
                engine.as_ptr(),
                engine.len(),
            )
        })
    }

    pub fn loose_key(path: &str, size: Option<u64>, modified: f64, variant: Option<&str>, look: &str) -> String {
        let v = variant.unwrap_or("");
        text(unsafe {
            sys::negs_thumbnail_loose_key(path.as_ptr(), path.len(), size.is_some() as u8, size.unwrap_or(0), modified, v.as_ptr(), v.len(), look.as_ptr(), look.len())
        })
    }
}

pub mod xmp {
    //! The rating an export carries; embedding runs in the core.
    use negs_core_sys::{library as sys, negs_buf_t, take_buf};
    pub use oracle::xmp::{export_rating, packet, read_rating};

    pub fn embed(bytes: &[u8], rating: i32) -> Option<Vec<u8>> {
        let mut out = negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        let done = unsafe { sys::negs_xmp_embed(bytes.as_ptr(), bytes.len(), rating, &mut out) };
        (done == 1).then(|| take_buf(out))
    }

    pub fn embed_in_file(path: &std::path::Path, rating: i32) -> std::io::Result<bool> {
        let text = path.to_string_lossy();
        let mut error = negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        match unsafe { sys::negs_xmp_embed_in_file(text.as_ptr(), text.len(), rating, &mut error) } {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(std::io::Error::other(String::from_utf8_lossy(&take_buf(error)).into_owned())),
        }
    }
}

/// With the `track` feature: at exit, the process fails unless everything
/// the core allocated was freed (every Library dropped).
#[cfg(feature = "track")]
pub(crate) fn watch_for_leaks() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    unsafe extern "C" {
        fn atexit(f: extern "C" fn()) -> i32;
        fn _exit(code: i32) -> !;
    }
    extern "C" fn check() {
        let stats = negs_core_sys::alloc_stats();
        if stats.tracked == 1 && (stats.live_count != 0 || stats.bad_frees != 0) {
            eprintln!("negs-core-library: the core leaked: {} live allocations ({} bytes), {} bad frees", stats.live_count, stats.live_bytes, stats.bad_frees);
            unsafe { _exit(101) };
        }
    }
    ONCE.call_once(|| unsafe {
        atexit(check);
    });
}

#[cfg(not(feature = "track"))]
pub(crate) fn watch_for_leaks() {}

/// The core's own JSON, for the fixture gate and differential tests.
pub mod core_json {
    use negs_core_sys::{library as sys, negs_buf_t, take_buf};

    fn call(f: unsafe extern "C" fn(*const u8, usize, *mut negs_buf_t) -> u8, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        let done = unsafe { f(bytes.as_ptr(), bytes.len(), &mut out) };
        let out = take_buf(out);
        if done == 1 { Ok(out) } else { Err(String::from_utf8_lossy(&out).into_owned()) }
    }

    /// Sidecar bytes read and written again by the core, as the Mac writes them.
    pub fn sidecar_rewrite(bytes: &[u8]) -> Result<Vec<u8>, String> {
        call(sys::negs_sidecar_rewrite, bytes)
    }

    /// JSON compact with sorted keys, as the core writes it for looks.
    pub fn compact_sorted(bytes: &[u8]) -> Result<String, String> {
        call(sys::negs_json_compact_sorted, bytes).map(|b| String::from_utf8(b).expect("UTF-8"))
    }
}
