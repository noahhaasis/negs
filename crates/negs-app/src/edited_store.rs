//! Developed thumbnails and kept develops on disk (`EditedThumbnailStore`):
//! JPEGs in the cache folder, each named by everything it depends on (the
//! file as listed, the duplicate, the stored edits' look and the engine:
//! `negs_library::thumbnail::key`, the Mac's filing key), so a changed one
//! is never found, and the least recently used go past the limit. Never
//! needed for correctness: without one, the strip and grid show the
//! embedded preview, and a photograph opens on it.
//!
//! - `Thumbnails`: 512 px on the long side, cropped and turned as the edit
//!   view shows them; 1 GB.
//! - `Renders`: each photograph's last develop, screen-sized (2560 px) and
//!   uncropped, as it was left: it stands in when the photograph next
//!   opens, sharper than its embedded preview, until the develop arrives.
//!   The photograph's size and display space ride after the JPEG's end.
//!
//! Nothing here writes into a photograph's folder: Windows' own thumbnail
//! cache wrote `Thumbs.db` into network folders.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use negs_library::{Item, PhotoFile, thumbnail};

use crate::engine::EnginePaths;

/// The developed thumbnails.
pub fn thumbnails() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Store::new(crate::session::cache_dir().join("Thumbnails"), crate::sheet_rules::EDITED_LIMIT))
}

/// The kept develops.
pub fn renders() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Store::new(crate::session::cache_dir().join("Renders"), crate::sheet_rules::RENDERS_LIMIT))
}

/// Reads the engine's stamp once, on a thread, for both stores; `done` on
/// the UI thread when it's known.
pub fn read_stamp(paths: EnginePaths, done: impl FnOnce(&mut crate::model::Model) + Send + 'static) {
    static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ASKED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("negs-stamp".into()).spawn(move || {
        let stamp = stamp(&paths);
        crate::trace::event("thumbnail_stamp", || serde_json::json!({"stamp": stamp}));
        thumbnails().set_engine(stamp.clone());
        renders().set_engine(stamp);
        crate::host::post(done);
    });
}

/// One folder of JPEGs named by their keys.
pub struct Store {
    pub directory: PathBuf,
    limit: u64,
    /// The helper and its data (`stamp`); until known, no key is made.
    engine: Mutex<Option<String>>,
    writes: AtomicU64,
}

impl Store {
    pub fn new(directory: PathBuf, limit: u64) -> Self {
        Self { directory, limit, engine: Mutex::new(None), writes: AtomicU64::new(0) }
    }

    pub fn engine(&self) -> Option<String> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_engine(&self, stamp: Option<String>) {
        *self.engine.lock().unwrap_or_else(|e| e.into_inner()) = stamp;
    }

    /// The name of `item`'s file with stored edits `look` (`key(for:look:)`).
    pub fn key(&self, file: &PhotoFile, item: &Item, look: &str) -> Option<String> {
        let engine = self.engine()?;
        let modified = thumbnail::seconds_since_1970(file.modified?);
        Some(thumbnail::key(&file.path.to_string_lossy(), file.size, modified, item.variant.as_deref(), look, &engine))
    }

    /// The same under any engine: names a small file holding the key of the
    /// last one saved, which stands in after an update (`looseKey`). A file,
    /// not the Mac's symbolic link, which Windows keeps for administrators.
    pub fn loose_key(file: &PhotoFile, item: &Item, look: &str) -> Option<String> {
        let modified = thumbnail::seconds_since_1970(file.modified?);
        Some(thumbnail::loose_key(&file.path.to_string_lossy(), file.size, modified, item.variant.as_deref(), look))
    }

    pub fn path(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.jpg"))
    }

    fn link(&self, loose: &str) -> PathBuf {
        self.directory.join(format!("{loose}.latest"))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.path(key).is_file()
    }

    /// The last one saved for `loose` under another engine, if it's still there.
    pub fn previous(&self, loose: &str) -> Option<String> {
        let key = std::fs::read_to_string(self.link(loose)).ok()?.trim().to_string();
        (key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()) && self.contains(&key)).then_some(key)
    }

    /// Marks `key` recently used (the last to be removed), at most once a day.
    pub fn touch(&self, key: &str) {
        let path = self.path(key);
        let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else { return };
        if SystemTime::now().duration_since(modified).unwrap_or_default() > Duration::from_secs(86_400) {
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
                let _ = file.set_modified(SystemTime::now());
            }
        }
    }

    /// Writes `jpeg` as `key` through a hidden temporary file and a rename,
    /// and points `loose` at it.
    pub fn save(&self, jpeg: &[u8], key: &str, loose: Option<&str>) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let temporary = self.directory.join(format!(".{key}-{}.tmp", std::process::id()));
        std::fs::write(&temporary, jpeg)?;
        if let Err(error) = std::fs::rename(&temporary, self.path(key)) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Some(loose) = loose {
            let temporary = self.directory.join(format!(".{loose}-{}.tmp", std::process::id()));
            if std::fs::write(&temporary, key).is_err() || std::fs::rename(&temporary, self.link(loose)).is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
        }
        if self.writes.fetch_add(1, Ordering::Relaxed) % 200 == 199 {
            self.prune();
        }
        Ok(())
    }

    /// Removes the least recently used while there are more than the limit's
    /// bytes, down to three quarters of it; then the links to what's gone.
    pub fn prune(&self) {
        let Ok(read) = std::fs::read_dir(&self.directory) else { return };
        let mut entries: Vec<(PathBuf, u64, SystemTime)> = read
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "jpg"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((e.path(), m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            })
            .collect();
        let mut total: u64 = entries.iter().map(|e| e.1).sum();
        if total <= self.limit {
            return;
        }
        entries.sort_by_key(|e| e.2);
        for (path, size, _) in entries {
            if total <= self.limit * 3 / 4 {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
        if let Ok(read) = std::fs::read_dir(&self.directory) {
            for link in read.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "latest")) {
                let gone = std::fs::read_to_string(&link).map_or(true, |key| !self.contains(key.trim()));
                if gone {
                    let _ = std::fs::remove_file(link);
                }
            }
        }
    }

}

/// Identifies the helper and every file of its data, which every thumbnail
/// depends on (`EditedThumbnailStore.stamp`). Read once a session, off the
/// UI thread.
pub fn stamp(paths: &EnginePaths) -> Option<String> {
    fn line(path: &Path) -> Option<String> {
        let m = std::fs::metadata(path).ok()?;
        let modified = m.modified().map(thumbnail::seconds_since_1970).unwrap_or(0.0);
        Some(format!("{}|{}|{modified}", path.display(), m.len()))
    }
    let mut lines = vec![line(&paths.helper)?];
    let mut folders = vec![paths.data.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).ok()?.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => folders.push(path),
                Ok(t) if t.is_file() => lines.push(line(&path)?),
                _ => {}
            }
        }
    }
    lines.sort();
    Some(negs_library::sha256::digest16(lines.join("\n").as_bytes()))
}

// MARK: Kept develops

/// After a kept develop's JPEG: "NEGSSIZE", the photograph's laid-out width
/// and height (f64, little-endian), and its display space's name.
const TRAILER: &[u8; 8] = b"NEGSSIZE";

/// A kept develop as stored: the JPEG with the photograph's size and the
/// pixels' display space after it.
pub fn render_file(jpeg: Vec<u8>, photo: (f64, f64), space: &str) -> Vec<u8> {
    let mut file = jpeg;
    let name = space.as_bytes();
    file.extend_from_slice(name);
    file.extend_from_slice(&(name.len() as u16).to_le_bytes());
    file.extend_from_slice(&photo.0.to_le_bytes());
    file.extend_from_slice(&photo.1.to_le_bytes());
    file.extend_from_slice(TRAILER);
    file
}

/// The photograph's size and display space a kept develop was stored with.
pub fn render_trailer(file: &[u8]) -> Option<((f64, f64), String)> {
    let n = file.len();
    if n < 26 || &file[n - 8..] != TRAILER {
        return None;
    }
    let number = |at: usize| f64::from_le_bytes(file[at..at + 8].try_into().unwrap());
    let (w, h) = (number(n - 24), number(n - 16));
    let len = u16::from_le_bytes([file[n - 26], file[n - 25]]) as usize;
    let space = std::str::from_utf8(file.get(n - 26 - len..n - 26)?).ok()?.to_string();
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some(((w, h), space))
}

/// Decodes a stored JPEG (a kept develop's trailer is past its end) as
/// opaque BGRA.
pub fn decode(file: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let image = image::load_from_memory_with_format(file, image::ImageFormat::Jpeg).ok()?.to_rgb8();
    let (w, h) = image.dimensions();
    let bgra = image.pixels().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    Some((bgra, w, h))
}

/// Encodes opaque RGBA as a JPEG at 85 %, `longest` px on its long side at most.
pub fn jpeg(rgba: &[u8], width: u32, height: u32, longest: u32) -> Option<Vec<u8>> {
    use image::{RgbImage, imageops};
    if width == 0 || height == 0 || rgba.len() < width as usize * height as usize * 4 {
        return None;
    }
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut image = RgbImage::from_raw(width, height, rgb)?;
    let scale = longest as f64 / width.max(height) as f64;
    if scale < 1.0 {
        let (w, h) = (((width as f64 * scale).round() as u32).max(1), ((height as f64 * scale).round() as u32).max(1));
        image = imageops::resize(&image, w, h, imageops::FilterType::Lanczos3);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&image).ok()?;
    Some(out)
}
