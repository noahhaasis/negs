//! One lossless startup frame. A serial worker orders saves and invalidations;
//! a single atomic file contains its identity, geometry and exact RGBA pixels.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::engine::EnginePaths;
use crate::state::EditState;

const MAX_PIXELS: usize = 8192 * 8192 * 4;

#[derive(Serialize, Deserialize)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub photo_width: f64,
    pub photo_height: f64,
    pub raw: bool,
    #[serde(skip)]
    pub pixels: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Header {
    key: String,
    frame: Frame,
    checksum: u64,
}

type Work = Box<dyn FnOnce(&Path) + Send>;

pub struct RenderedPreviewCache {
    sender: Sender<Work>,
}

impl Default for RenderedPreviewCache {
    fn default() -> Self {
        Self::new(crate::session::cache_dir().join("rendered-preview.cache"))
    }
}

impl RenderedPreviewCache {
    pub fn new(file: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<Work>();
        std::thread::spawn(move || {
            for work in receiver {
                work(&file);
            }
        });
        Self { sender }
    }

    pub fn invalidate(&self) {
        let _ = self.sender.send(Box::new(|file| {
            let _ = std::fs::remove_file(file);
        }));
    }

    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self
            .sender
            .send(Box::new(move |_| {
                let _ = tx.send(());
            }))
            .is_ok()
        {
            let _ = rx.recv();
        }
    }

    pub fn load(&self, source: PathBuf, state: EditState, paths: EnginePaths, done: impl FnOnce(Option<Frame>) + Send + 'static) {
        let _ = self.sender.send(Box::new(move |file| {
            let frame = key(&source, &state, &paths).and_then(|key| read(file, &key).ok());
            done(frame);
        }));
    }

    pub fn save(&self, key: String, frame: Frame) {
        let _ = self.sender.send(Box::new(move |file| {
            let _ = write(file, key, frame);
        }));
    }
}

/// Compare the entire canonical identity, so settings cannot hash-collide.
/// Crop, rotation and border are applied separately by the restored canvas.
pub fn key(source: &Path, state: &EditState, paths: &EnginePaths) -> Option<String> {
    fn stamp(path: &Path) -> Result<String> {
        let m = std::fs::metadata(path)?;
        let modified = m.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let created = m.created()?.duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        Ok(format!("{}|{}|{modified}|{created}", path.display(), m.len()))
    }
    fn data_stamps(dir: &Path, files: &mut Vec<String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                data_stamps(&entry.path(), files)?;
            } else if kind.is_file() {
                files.push(stamp(&entry.path())?);
            }
        }
        Ok(())
    }
    let mut files = vec![stamp(source).ok()?, stamp(&paths.helper).ok()?, stamp(&paths.data).ok()?];
    data_stamps(&paths.data, &mut files).ok()?;
    files.sort();
    serde_json::to_string(&serde_json::json!({"schema": 1, "state": state.rendered(), "files": files})).ok()
}

fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}

fn read(file: &Path, key: &str) -> Result<Frame> {
    ensure!(std::fs::metadata(file)?.len() <= (MAX_PIXELS + 1_048_576) as u64, "Invalid cached preview size");
    let bytes = std::fs::read(file)?;
    let end = bytes.iter().take(1_048_576).position(|&b| b == b'\n').ok_or_else(|| anyhow::anyhow!("Invalid cached preview header"))?;
    let Header { key: saved, mut frame, checksum: expected } = serde_json::from_slice(&bytes[..end])?;
    ensure!(saved == key && (1..=8192).contains(&frame.width) && (1..=8192).contains(&frame.height), "Cached preview does not match");
    ensure!(frame.photo_width.is_finite() && frame.photo_height.is_finite() && frame.photo_width > 0.0 && frame.photo_height > 0.0, "Invalid cached photograph size");
    let pixels = &bytes[end + 1..];
    ensure!(pixels.len() == frame.width as usize * frame.height as usize * 4 && checksum(pixels) == expected, "Invalid cached preview pixels");
    frame.pixels = pixels.to_vec();
    Ok(frame)
}

fn write(file: &Path, key: String, mut frame: Frame) -> Result<()> {
    use std::io::Write;
    let pixels = std::mem::take(&mut frame.pixels);
    let mut bytes = serde_json::to_vec(&Header { key, frame, checksum: checksum(&pixels) })?;
    bytes.push(b'\n');
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = file.with_extension("tmp");
    let result = (|| {
        let mut output = std::fs::File::create(&staging)?;
        output.write_all(&bytes)?;
        output.write_all(&pixels)?;
        drop(output);
        crate::platform::replace_file(&staging, file)
    })();
    let _ = std::fs::remove_file(staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_atomic_frame_rejects_other_settings_and_corruption() {
        let dir = std::env::temp_dir().join(format!("negs-rendered-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("frame.cache");
        let pixels = vec![1, 2, 3, 255, 30, 40, 50, 255];
        let frame = Frame { width: 2, height: 1, photo_width: 4000.0, photo_height: 2000.0, raw: true, pixels: pixels.clone() };
        let cache = RenderedPreviewCache::new(file.clone());
        cache.save("settings".into(), frame);
        cache.flush();
        let loaded = read(&file, "settings").unwrap();
        assert_eq!(loaded.pixels, pixels);
        assert_eq!(loaded.photo_width, 4000.0);
        assert!(loaded.raw);
        assert!(read(&file, "other settings").is_err());
        let mut bytes = std::fs::read(&file).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        std::fs::write(&file, &bytes).unwrap();
        assert!(read(&file, "settings").is_err());
        cache.save("settings".into(), loaded);
        cache.invalidate();
        cache.flush();
        assert!(!file.exists(), "An earlier save cannot resurrect an invalidated frame");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn identity_tracks_render_settings_source_engine_and_profiles() {
        let dir = std::env::temp_dir().join(format!("negs-preview-key-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let source = dir.join("photo.raw");
        let helper = dir.join("helper");
        let profile = dir.join("data/film.json");
        for file in [&source, &helper, &profile] {
            std::fs::write(file, b"original").unwrap();
        }
        let paths = EnginePaths { helper: helper.clone(), exporter: helper.clone(), data: dir.join("data"), fast_exporter: None };
        let mut state = EditState::default();
        state.params = serde_json::json!({"io": {"rotate_cw": 0}, "exposure": 1.0});
        let original = key(&source, &state, &paths).unwrap();
        let mut canvas = state.clone();
        canvas.border.enabled = true;
        canvas.crop.angle = 2.0;
        canvas.params["io"]["rotate_cw"] = 1.into();
        assert_eq!(key(&source, &canvas, &paths).unwrap(), original);
        for changed in [
            EditState { film: "other".into(), ..state.clone() },
            EditState { paper: "other".into(), ..state.clone() },
            EditState { limit_preview: true, ..state.clone() },
            EditState { params: serde_json::json!({"io": {"rotate_cw": 0}, "exposure": 2.0}), ..state.clone() },
        ] {
            assert_ne!(key(&source, &changed, &paths).unwrap(), original);
        }
        for file in [&source, &helper, &profile] {
            let before = key(&source, &state, &paths).unwrap();
            std::fs::write(file, b"changed file").unwrap();
            assert_ne!(key(&source, &state, &paths).unwrap(), before);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
