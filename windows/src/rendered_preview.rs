//! One lossless startup frame. A serial worker orders saves and invalidations;
//! a single atomic file contains its identity, geometry and exact RGBA pixels.
//!
//! The shared core's (core/src/caches/preview, through negs_core_rules): the
//! key, the checksum and the file run in Odin (core/diff/tests/
//! rendered_preview.rs); the worker stays here.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use anyhow::Result;

use crate::engine::EnginePaths;
use crate::state::EditState;

use negs_core_rules::rendered_preview::{self as core, KeyState};
pub use negs_core_rules::rendered_preview::Frame;

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
/// Crop, rotation and border are applied separately by the restored canvas;
/// with an Image Size target the crop's extent is part of `state.rendered()`.
/// A state the core doesn't read (a NaN serde writes as null) is keyed by
/// its `rendered()` value, made here.
pub fn key(source: &Path, state: &EditState, paths: &EnginePaths) -> Option<String> {
    let json = serde_json::to_vec(state).ok()?;
    match core::key(source, &paths.helper, &paths.data, KeyState::State(&json)) {
        Ok(key) => key,
        Err(()) => {
            let rendered = serde_json::to_value(state.rendered()).ok()?;
            core::key(source, &paths.helper, &paths.data, KeyState::Rendered(&rendered)).ok().flatten()
        }
    }
}

fn read(file: &Path, key: &str) -> Result<Frame> {
    core::read(file, key).map_err(anyhow::Error::msg)
}

fn write(file: &Path, key: String, frame: Frame) -> Result<()> {
    core::write(file, &key, &frame).map_err(anyhow::Error::msg)
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
        let frame = Frame {
            width: 2, height: 1, photo_width: 4000.0, photo_height: 2000.0, source_width: 2000.0, source_height: 1000.0,
            working_width: 8000.0, working_height: 4000.0, raw: true,
            display_space: crate::picture::WIDE_DISPLAY_SPACE.into(), lens_correction_supported: true, pixels: pixels.clone(),
        };
        let cache = RenderedPreviewCache::new(file.clone());
        cache.save("settings".into(), frame);
        cache.flush();
        let loaded = read(&file, "settings").unwrap();
        assert_eq!(loaded.pixels, pixels);
        assert_eq!(loaded.photo_width, 4000.0);
        assert_eq!((loaded.source_width, loaded.working_width), (2000.0, 8000.0));
        assert!(loaded.raw && loaded.lens_correction_supported);
        assert_eq!(loaded.display_space, crate::picture::WIDE_DISPLAY_SPACE);
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
        // With a Longest side target the crop's extent is part of the frame.
        let mut sized = canvas.clone();
        sized.params["io"]["photo_long_side_px"] = 3000.into();
        sized.crop.rect = crate::crop::Rect::new(0.1, 0.1, 0.5, 0.5);
        let sized_key = key(&source, &sized, &paths).unwrap();
        assert_ne!(sized_key, original);
        sized.crop.rect.x = 0.4;
        assert_eq!(key(&source, &sized, &paths).unwrap(), sized_key);
        sized.crop.rect.width = 0.4;
        assert_ne!(key(&source, &sized, &paths).unwrap(), sized_key);
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
