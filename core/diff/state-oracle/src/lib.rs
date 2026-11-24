//! Phase 4's oracles (core/diff/oracle, frozen at b05e49e; state.rs, edits.rs
//! and inspector_view/ at b0f1f5b), compiled as they are on macOS.
//!
//! - `copied` holds the windows/src items they name through `crate::`
//!   (model's save and export settings and LibraryPreferences, engine's
//!   EnginePaths and ExportEngine, GRID_DEFAULT, WIDE_DISPLAY_SPACE), copied
//!   verbatim.
//! - `platform` stands in for windows/src/platform.rs's helpers and
//!   `windows` (the stand-in crate) for the Win32 calls; `stand_ins` for
//!   `std::os::windows::fs::MetadataExt` (see build.rs).
//!
//! How they differ from Windows (none of it is what the differential tests
//! compare):
//! - replace_file and presets' move_no_replace are rename(2) and
//!   renamex_np(RENAME_EXCL): atomic on APFS, write-through ignored. macOS
//!   never reports sharing or lock violations, so the "open in another
//!   program" errors can't happen; error messages are strerror's, not
//!   FormatMessageW's.
//! - Hidden files are UF_HIDDEN (chflags), read back through st_flags;
//!   Windows uses FILE_ATTRIBUTE_HIDDEN.
//! - Preset keys normalise with ICU's NFC tables, Windows with its own
//!   (equal for every character both versions know). Film search folds
//!   with ICU's decomposition, lower case and no combining marks; Windows'
//!   invariant linguistic match also folds a few letters (ß, ligatures).
//! - settings_dir and cache_dir read APPDATA and LOCALAPPDATA (unset on a
//!   Mac, so tests set NEGS_PROFILE_DIR); file names compare
//!   case-insensitively on APFS's default volumes, as on NTFS.
//! - match_cache's engine stamp has this crate's CARGO_PKG_VERSION
//!   ("0.0.0"), not the app's.
#![allow(dead_code, unused_imports, clippy::all)]

#[path = "../../oracle/border.rs"]
pub mod border;
#[path = "../../oracle/crop.rs"]
pub mod crop;
#[path = "../../oracle/json.rs"]
pub mod json;
#[path = "../../oracle/state.rs"]
pub mod state;
#[path = "../../oracle/session.rs"]
pub mod session;
pub mod presets {
    include!(concat!(env!("OUT_DIR"), "/presets.rs"));
}
#[path = "../../oracle/batch.rs"]
pub mod batch;
#[path = "../../oracle/match_cache.rs"]
pub mod match_cache;
#[path = "../../oracle/rendered_preview.rs"]
pub mod rendered_preview;
// Phase 4a: edits.rs and the inspector view (windows/src/inspector_view)
// as they were on b0f1f5b, for 4a's and 4d's differential tests, with
// Phase 3's frozen negative and help, which they name. Their own tests run
// in core/diff/tests/inspector_oracle.rs.
#[path = "../../oracle/negative.rs"]
pub mod negative;
#[path = "../../oracle/help.rs"]
pub mod help;
#[path = "../../oracle/edits.rs"]
pub mod edits;
#[path = "../../oracle/inspector_view/mod.rs"]
pub mod inspector_view;

mod copied;
pub use copied::{engine, model, picture, sheet_rules};

/// windows/src/platform.rs's helpers the oracles call, on macOS.
pub mod platform {
    use anyhow::{Result, anyhow};
    use std::ffi::OsStr;
    use std::path::Path;
    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::PCWSTR;

    /// A NUL-terminated UTF-16 string (a path's bytes read as UTF-8).
    pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
        s.as_ref().to_string_lossy().encode_utf16().chain(Some(0)).collect()
    }

    pub fn pcwstr(w: &[u16]) -> PCWSTR {
        PCWSTR(w.as_ptr())
    }

    /// As platform.rs: MoveFileExW with REPLACE_EXISTING | WRITE_THROUGH.
    pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
        let (f, t) = (wide(from), wide(to));
        unsafe { MoveFileExW(pcwstr(&f), pcwstr(&t), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
            .map_err(|e| anyhow!("Could not move the file to {}: {}", to.display(), e.message()))
    }

    unsafe extern "C" {
        fn chflags(path: *const std::ffi::c_char, flags: u32) -> i32;
    }
    pub(crate) const UF_HIDDEN: u32 = 0x8000;

    /// SetFileAttributesW(HIDDEN) as chflags(UF_HIDDEN); errors ignored, as there.
    pub fn hide_file(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
        unsafe {
            let _ = chflags(c.as_ptr(), UF_HIDDEN);
        }
    }
}

/// `std::os::windows::fs::MetadataExt`, for presets.rs (build.rs).
pub mod stand_ins {
    pub trait MetadataExt {
        /// FILE_ATTRIBUTE_HIDDEN (2) when the file has UF_HIDDEN; nothing else.
        fn file_attributes(&self) -> u32;
    }

    impl MetadataExt for std::fs::Metadata {
        fn file_attributes(&self) -> u32 {
            use std::os::macos::fs::MetadataExt;
            if self.st_flags() & crate::platform::UF_HIDDEN != 0 { 2 } else { 0 }
        }
    }
}
