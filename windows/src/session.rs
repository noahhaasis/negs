//! The editing session saved at quit and the app's preferences. Small JSON
//! files in `%APPDATA%\Negs`; the catalog cache lives in `%LOCALAPPDATA%\Negs`.
//! `NEGS_PROFILE_DIR` moves all three, for tests and benchmarks.
//! Mirrors `Session.swift` and the macOS app's UserDefaults keys.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::crop::MAX_ANGLE;
use crate::state::EditState;

pub fn settings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NEGS_PROFILE_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("Negs")
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NEGS_PROFILE_DIR") {
        return PathBuf::from(dir).join("Cache");
    }
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Negs")
}

/// Writes `data` to a sibling temporary file and renames it over `path`, so a
/// crash never leaves half a file.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staging = path.with_extension("tmp");
    std::fs::write(&staging, data)?;
    crate::platform::replace_file(&staging, path)
}

/// Small, versioned editing state. Pixels, caches and undo history are not saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession {
    pub schema: u32,
    /// The photograph's path. There is no bookmark on Windows.
    pub source: PathBuf,
    pub state: EditState,
    #[serde(default = "yes")]
    pub inspector_visible: bool,
    #[serde(default = "yes")]
    pub fit_image: bool,
}

fn yes() -> bool {
    true
}

impl EditorSession {
    pub fn validate(&self) -> Result<()> {
        let crop = &self.state.crop;
        let values = [crop.rect.x, crop.rect.y, crop.rect.width, crop.rect.height, crop.custom.width, crop.custom.height, crop.angle];
        let rotation = self.state.value("io.rotate_cw").as_f64().unwrap_or(-1.0);
        let ok = self.schema == 1
            && self.source.is_absolute()
            && values.iter().all(|v| v.is_finite())
            && crop.rect.x >= 0.0
            && crop.rect.y >= 0.0
            && crop.rect.width > 0.0
            && crop.rect.height > 0.0
            && crop.rect.max_x() <= 1.000001
            && crop.rect.max_y() <= 1.000001
            && crop.angle.abs() <= MAX_ANGLE
            && crop.custom.width > 0.0
            && crop.custom.height > 0.0
            && self.state.params.is_object()
            && (0.0..=3.0).contains(&rotation)
            && rotation.round() == rotation;
        ensure!(ok, "The previous editing session could not be restored.");
        Ok(())
    }
}

pub struct SessionStore {
    pub file: PathBuf,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self { file: settings_dir().join("session.json") }
    }
}

impl SessionStore {
    pub fn load(&self) -> Result<Option<EditorSession>> {
        let Ok(metadata) = std::fs::metadata(&self.file) else { return Ok(None) };
        if metadata.len() > 1_048_576 {
            bail!("The previous editing session is too large to restore.");
        }
        let data = std::fs::read(&self.file)?;
        let session: EditorSession =
            serde_json::from_slice(&data).context("The previous editing session could not be restored.")?;
        session.validate()?;
        if !session.source.is_file() {
            bail!("The previous photograph is unavailable. Open a photograph to continue.");
        }
        Ok(Some(session))
    }

    pub fn save(&self, session: &EditorSession) -> Result<()> {
        session.validate()?;
        write_atomic(&self.file, &serde_json::to_vec(session)?)
    }
}

/// Preferences, like the macOS app's UserDefaults: one JSON object in
/// `settings.json`, read once at launch and written when a value changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub recent_files: Vec<PathBuf>,
    pub filmstrip_visible: bool,
    /// Comma-separated titles of the expanded groups.
    pub expanded_adjustments: String,
    pub inspector_tab: String,
    pub export_engine: String,
    pub output_format: String,
    pub export_edge: String,
    pub export_edge_pixels: u32,
    pub last_save_folder: Option<PathBuf>,
    /// `WINDOWPLACEMENT` of the editor: show command and normal rectangle in
    /// workspace coordinates.
    pub window: Option<[i32; 5]>,
    /// The adjustments column's width, in DIPs.
    pub inspector_width: f64,
    pub check_for_updates: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            recent_files: vec![],
            filmstrip_visible: true,
            expanded_adjustments: "Enlarger".into(),
            inspector_tab: "Lab".into(),
            export_engine: "fast".into(),
            output_format: "png".into(),
            export_edge: "full".into(),
            export_edge_pixels: 2048,
            last_save_folder: None,
            window: None,
            inspector_width: 340.0,
            check_for_updates: true,
        }
    }
}

impl Preferences {
    fn path() -> PathBuf {
        settings_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = write_atomic(&Self::path(), &data);
        }
    }

    pub fn expanded(&self, title: &str) -> bool {
        self.expanded_adjustments.split(',').any(|t| t == title)
    }

    pub fn set_expanded(&mut self, title: &str, open: bool) {
        let mut titles: Vec<&str> = self.expanded_adjustments.split(',').filter(|t| !t.is_empty() && *t != title).collect();
        if open {
            titles.push(title);
        }
        self.expanded_adjustments = titles.join(",");
    }

    /// Adds a photograph to the top of Recent (at most 10, no duplicates).
    pub fn note_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| !same_path(p, path));
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(10);
    }

    /// The folder of the last save or export, if it still exists.
    pub fn save_folder(&self) -> Option<&Path> {
        self.last_save_folder.as_deref().filter(|p| p.is_dir())
    }
}

/// Paths compare case-insensitively on Windows.
pub fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
}

/// The helper's last catalog, keyed by the helper binary and data folder, so a
/// launch can show the stocks before the helper answers. The helper's own
/// answer always replaces it.
pub struct CatalogCache;

impl CatalogCache {
    fn path() -> PathBuf {
        cache_dir().join("catalog.json")
    }

    fn key(helper: &Path, data: &Path) -> Option<String> {
        let modified = std::fs::metadata(helper).ok()?.modified().ok()?;
        let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
        Some(format!("{}|{since}|{}", helper.display(), data.display()))
    }

    pub fn load(helper: &Path, data: &Path) -> Option<Value> {
        let key = Self::key(helper, data)?;
        let entry: Value = serde_json::from_slice(&std::fs::read(Self::path()).ok()?).ok()?;
        (entry["key"] == Value::from(key) && entry["catalog"]["protocol"].as_f64() == Some(1.0)).then(|| entry["catalog"].clone())
    }

    pub fn save(catalog: &Value, helper: &Path, data: &Path) {
        if let Some(key) = Self::key(helper, data) {
            let entry = serde_json::json!({"key": key, "catalog": catalog});
            let _ = write_atomic(&Self::path(), entry.to_string().as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crop::Rect;
    use serde_json::json;

    fn session() -> EditorSession {
        EditorSession {
            schema: 1,
            source: PathBuf::from(r"C:\photos\DSC_1.NEF"),
            state: EditState { params: json!({"io": {"rotate_cw": 1}}), ..Default::default() },
            inspector_visible: true,
            fit_image: true,
        }
    }

    #[test]
    fn validation_rejects_bad_crops_and_rotations() {
        assert!(session().validate().is_ok());
        let mut s = session();
        s.state.crop.rect = Rect::new(0.5, 0.0, 0.6, 1.0);
        assert!(s.validate().is_err());
        let mut s = session();
        s.state.crop.angle = 46.0;
        assert!(s.validate().is_err());
        let mut s = session();
        s.state.params = json!({"io": {"rotate_cw": 1.5}});
        assert!(s.validate().is_err());
        let mut s = session();
        s.source = PathBuf::from("relative.png");
        assert!(s.validate().is_err());
    }

    #[test]
    fn sessions_round_trip_and_reject_missing_photographs() {
        let dir = std::env::temp_dir().join(format!("negs-session-{}", std::process::id()));
        let store = SessionStore { file: dir.join("session.json") };
        assert!(store.load().unwrap().is_none());
        store.save(&session()).unwrap();
        let text = std::fs::read_to_string(&store.file).unwrap();
        assert!(text.contains(r#""inspectorVisible":true"#) && text.contains(r#""limitPreview":false"#));
        assert!(store.load().unwrap_err().to_string().contains("unavailable"));
        let photo = dir.join("photo.png");
        std::fs::write(&photo, b"x").unwrap();
        let mut s = session();
        s.source = photo;
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap(), Some(s));
        std::fs::write(&store.file, vec![b' '; 1_048_577]).unwrap();
        assert!(store.load().unwrap_err().to_string().contains("too large"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recent_files_and_groups() {
        let mut prefs = Preferences::default();
        for i in 0..12 {
            prefs.note_recent(Path::new(&format!(r"C:\p\{i}.png")));
        }
        prefs.note_recent(Path::new(r"C:\P\5.PNG"));
        assert_eq!(prefs.recent_files.len(), 10);
        assert_eq!(prefs.recent_files[0], PathBuf::from(r"C:\P\5.PNG"));
        assert!(prefs.expanded("Enlarger") && !prefs.expanded("Grain"));
        prefs.set_expanded("Grain", true);
        prefs.set_expanded("Enlarger", false);
        assert_eq!(prefs.expanded_adjustments, "Grain");
    }
}
