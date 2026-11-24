//! windows/src/rendered_preview.rs's file on the shared core (core/src/
//! caches/preview): the key (every file the frame depends on, stamped, and
//! the rendered state), the checksum (DefaultHasher's SipHash-1-3, bit for
//! bit), and the file itself: a serde_json header line, then the pixels,
//! written through a staging file.
//!
//! Kept in Rust: the serial worker (mpsc and its thread, windows/src/
//! rendered_preview.rs) and `Frame` (plain data, serde derives as it had).
//! A state the core doesn't read (a NaN serde wrote as null) is keyed by
//! its `rendered()` value, which the host makes.

use std::path::Path;

use negs_core_sys::caches as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::batch::path_bytes;

fn empty() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

#[derive(Serialize, Deserialize)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub photo_width: f64,
    pub photo_height: f64,
    /// For Image Size readouts: the decoded source and the uncapped working
    /// frame. Required, so records from before them never restore.
    pub source_width: f64,
    pub source_height: f64,
    pub working_width: f64,
    pub working_height: f64,
    pub raw: bool,
    /// The helper's `display_color_space`; records from before it are sRGB.
    #[serde(default)]
    pub display_space: String,
    /// Shows the Lens corrections switch before the first render.
    pub lens_correction_supported: bool,
    #[serde(skip)]
    pub pixels: Vec<u8>,
}

/// What `key` was given for the state.
pub enum KeyState<'a> {
    /// `serde_json::to_vec(&state)`.
    State(&'a [u8]),
    /// `serde_json::to_value(state.rendered())`.
    Rendered(&'a Value),
}

/// The key of `state` for `source` in the engine of `helper` and `data`.
/// `Err(())` for a state the core doesn't read: key it by its rendered
/// value instead. `Ok(None)`: a file can't be stamped.
pub fn key(source: &Path, helper: &Path, data: &Path, state: KeyState) -> Result<Option<String>, ()> {
    let (s, h, d) = (path_bytes(source), path_bytes(helper), path_bytes(data));
    let (text, flags) = match state {
        KeyState::State(bytes) => (bytes.to_vec(), 0),
        KeyState::Rendered(value) => (crate::json::text(value), sys::NEGS_PREVIEW_RENDERED),
    };
    let mut out = empty();
    let ok = unsafe {
        sys::negs_preview_key(s.as_ptr(), s.len(), h.as_ptr(), h.len(), d.as_ptr(), d.len(), text.as_ptr(), text.len(), flags, &mut out)
    };
    let bytes = take_buf(out);
    match ok {
        1 => Ok(Some(String::from_utf8(bytes).expect("the core writes UTF-8"))),
        2 => Err(()),
        _ => Ok(None),
    }
}

/// `DefaultHasher` over `bytes` as `[u8]::hash` feeds it.
pub fn checksum(bytes: &[u8]) -> u64 {
    unsafe { sys::negs_preview_checksum(bytes.as_ptr(), bytes.len()) }
}

/// The frame saved in `file` for `key`, checked as the Rust checked it.
pub fn read(file: &Path, key: &str) -> Result<Frame, String> {
    let f = path_bytes(file);
    let mut raw = sys::negs_preview_frame_t::default();
    let (mut space, mut bytes, mut offset) = (empty(), empty(), 0usize);
    let error = unsafe {
        sys::negs_preview_read(f.as_ptr(), f.len(), key.as_ptr(), key.len(), &mut raw, &mut space, &mut bytes, &mut offset)
    };
    let display_space = String::from_utf8(take_buf(space)).unwrap_or_default();
    let pixels = if bytes.data.is_null() {
        Vec::new()
    } else {
        let all = unsafe { std::slice::from_raw_parts(bytes.data, bytes.len) };
        let pixels = all[offset.min(all.len())..].to_vec();
        unsafe { negs_core_sys::negs_buf_free(bytes) };
        pixels
    };
    let message = match error {
        0 => {
            return Ok(Frame {
                width: raw.width,
                height: raw.height,
                photo_width: raw.photo_width,
                photo_height: raw.photo_height,
                source_width: raw.source_width,
                source_height: raw.source_height,
                working_width: raw.working_width,
                working_height: raw.working_height,
                raw: raw.raw != 0,
                display_space,
                lens_correction_supported: raw.lens_correction_supported != 0,
                pixels,
            });
        }
        1 => "Cannot read the cached preview",
        2 => "Invalid cached preview size",
        3 => "Invalid cached preview header",
        4 => "Unreadable cached preview header",
        5 => "Cached preview does not match",
        6 => "Invalid cached photograph size",
        _ => "Invalid cached preview pixels",
    };
    Err(message.into())
}

/// Saves `frame` for `key` into `file`, through `{stem}.tmp`.
pub fn write(file: &Path, key: &str, frame: &Frame) -> Result<(), String> {
    let f = path_bytes(file);
    let raw = sys::negs_preview_frame_t {
        width: frame.width,
        height: frame.height,
        photo_width: frame.photo_width,
        photo_height: frame.photo_height,
        source_width: frame.source_width,
        source_height: frame.source_height,
        working_width: frame.working_width,
        working_height: frame.working_height,
        raw: frame.raw as u8,
        lens_correction_supported: frame.lens_correction_supported as u8,
    };
    let space = &frame.display_space;
    let code = unsafe {
        sys::negs_preview_write(
            f.as_ptr(),
            f.len(),
            key.as_ptr(),
            key.len(),
            &raw,
            space.as_ptr(),
            space.len(),
            frame.pixels.as_ptr(),
            frame.pixels.len(),
        )
    };
    match code {
        0 => Ok(()),
        -1 => Err("file name contained an unexpected NUL byte".into()),
        n => Err(std::io::Error::from_raw_os_error(n).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips_and_other_keys_miss() {
        let dir = std::env::temp_dir().join(format!("negs-rules-preview-{}", std::process::id()));
        let file = dir.join("frame.cache");
        let frame = Frame {
            width: 2,
            height: 1,
            photo_width: 4000.0,
            photo_height: 2000.0,
            source_width: 2000.0,
            source_height: 1000.0,
            working_width: 8000.0,
            working_height: 4000.0,
            raw: true,
            display_space: "Display P3".into(),
            lens_correction_supported: true,
            pixels: vec![1, 2, 3, 255, 30, 40, 50, 255],
        };
        write(&file, "settings", &frame).unwrap();
        let loaded = read(&file, "settings").unwrap();
        assert_eq!(loaded.pixels, frame.pixels);
        assert_eq!(loaded.display_space, "Display P3");
        assert!(loaded.raw && loaded.lens_correction_supported);
        assert_eq!(read(&file, "other").err().as_deref(), Some("Cached preview does not match"));
        assert!(read(&dir.join("none"), "settings").is_err());
        assert_ne!(checksum(&[]), checksum(&[0]));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
