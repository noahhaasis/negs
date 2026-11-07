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
    /// 2: the DIR gammas and halation baseline are null unless set, and a set
    /// value beats the film's preset. Schema 1 stored values for them that the
    /// engine ignored and nothing could set, so `migrate` drops them.
    pub const SCHEMA: u32 = 2;

    pub fn migrate(&mut self) {
        if self.schema != 1 {
            return;
        }
        for path in crate::state::DIR_HALATION_STOCK_PATHS {
            crate::json::set(&mut self.state.params, path, serde_json::Value::Null);
        }
        self.schema = Self::SCHEMA;
    }

    pub fn validate(&self) -> Result<()> {
        let crop = &self.state.crop;
        let values = [crop.rect.x, crop.rect.y, crop.rect.width, crop.rect.height, crop.custom.width, crop.custom.height, crop.angle];
        let rotation = self.state.value("io.rotate_cw").as_f64().unwrap_or(-1.0);
        let ok = self.schema == Self::SCHEMA
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
        let mut session: EditorSession =
            serde_json::from_slice(&data).context("The previous editing session could not be restored.")?;
        session.migrate();
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
    /// A tab was clicked. Before Prep every settings file saved "Lab" as the
    /// default, so without this a "Lab" opens on Prep, as on the Mac.
    pub inspector_tab_chosen: bool,
    /// What the adjustments column shows: "Adjustments" or "Presets".
    pub inspector_pane: String,
    pub export_engine: String,
    pub output_format: String,
    pub export_edge: String,
    pub export_edge_pixels: u32,
    /// The colour space chosen for saves, as colour names it.
    pub color_space: String,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
    pub jpeg_quality: u32,
    pub last_save_folder: Option<PathBuf>,
    /// `WINDOWPLACEMENT` of the editor: show command and normal rectangle in
    /// workspace coordinates.
    pub window: Option<[i32; 5]>,
    /// The adjustments column's width, in DIPs.
    pub inspector_width: f64,
    pub check_for_updates: bool,
    /// The last Image Size Longest side chosen, offered when switching to it
    /// again. Never switches the mode by itself.
    pub image_size_long_side: Option<u32>,
    /// Settings › Film, paper and preset previews (the Mac's `previews`).
    /// Old settings files lack it: on.
    pub previews: bool,
    /// Settings › Keep paper when changing film (the Mac's `keep_paper`).
    /// Old settings files lack it: off, the paper follows the film.
    #[serde(rename = "keep_paper")]
    pub keep_paper: bool,
    /// The tone graph's channel: "rgb", "r", "g" or "b".
    pub tone_graph_channel: String,
    /// Settings › New photographs: Start fresh (true, the default) or keep
    /// the previous photograph's edits (false, as earlier versions did). A
    /// restored session keeps its own edits either way.
    #[serde(rename = "freshEditsForNewPhotos")]
    pub fresh_edits: bool,
    /// Settings › Match new photographs to camera brightness: whether a new
    /// photograph starts with Prep › Match camera brightness on.
    #[serde(rename = "autoPlaceNewImages")]
    pub match_new_photos: bool,
    /// Settings › Limit preview to 12 MP. A preference, not an edit, so Undo
    /// and Reset Adjustments leave it. None until set: a session from before
    /// it was a preference then lends its own value.
    pub limit_preview: Option<bool>,
    /// Settings › Use less memory for export: grain channels one at a time.
    /// Only exports read it; previews always develop the channels together.
    pub low_memory_export: bool,
    /// Lab › Advanced controls. Off, the Lab shows each group's basics and
    /// anything changed from its default.
    pub lab_all_controls: bool,
    /// The scope above the tabs: "Histogram" or "Waveform".
    pub scope_mode: String,
    /// The scope's height in DIPs, dragged with the handle below it.
    pub scope_height: f64,
    /// Starred stock ids, in the order starred. Ids the catalog no longer has
    /// are kept, in case it brings them back.
    pub favorite_films: Vec<String>,
    pub favorite_papers: Vec<String>,
    /// Settings › Custom viewer background (the Mac's key). Off, the canvas
    /// follows the light or dark theme.
    #[serde(rename = "viewerBackgroundCustom")]
    pub viewer_background_custom: bool,
    /// The custom colour behind the photograph, "#rrggbb" in sRGB.
    #[serde(rename = "viewerBackgroundColor")]
    pub viewer_background: String,
}

/// Starred for new users, as on the web and the Mac; once they change the
/// list, theirs is kept, even when empty.
pub const DEFAULT_FAVORITE_FILMS: [&str; 4] = ["kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"];

/// The scope's heights in DIPs; it starts at the smallest, leaving the room
/// to the controls.
pub const SCOPE_HEIGHTS: (f64, f64) = (88.0, 420.0);

impl Default for Preferences {
    fn default() -> Self {
        Self {
            recent_files: vec![],
            filmstrip_visible: true,
            expanded_adjustments: "Enlarger".into(),
            inspector_tab: "Prep".into(),
            inspector_tab_chosen: false,
            inspector_pane: "Adjustments".into(),
            export_engine: "fast".into(),
            output_format: "png".into(),
            export_edge: "full".into(),
            export_edge_pixels: 2048,
            color_space: "sRGB".into(),
            png_bit_depth: 8,
            tiff_bit_depth: 16,
            jpeg_quality: 95,
            last_save_folder: None,
            window: None,
            inspector_width: 340.0,
            check_for_updates: true,
            image_size_long_side: None,
            previews: true,
            keep_paper: false,
            tone_graph_channel: "rgb".into(),
            fresh_edits: true,
            match_new_photos: true,
            limit_preview: None,
            low_memory_export: false,
            lab_all_controls: false,
            scope_mode: "Histogram".into(),
            scope_height: SCOPE_HEIGHTS.0,
            favorite_films: DEFAULT_FAVORITE_FILMS.iter().map(|s| s.to_string()).collect(),
            favorite_papers: vec![],
            viewer_background_custom: false,
            // A neutral dark grey, near the dark theme's.
            viewer_background: "#242424".into(),
        }
    }
}

impl Preferences {
    /// The colour behind the photograph when Custom viewer background is on
    /// and its colour reads.
    pub fn custom_viewer_background(&self) -> Option<[u8; 3]> {
        self.viewer_background_custom.then(|| crate::border::BorderColor::from_hex(&self.viewer_background)).flatten().map(|c| c.0)
    }

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

    /// The tab the inspector shows. Post became Output; settings files from
    /// before Prep that never chose a tab open on Prep.
    pub fn tab(&self) -> &str {
        match self.inspector_tab.as_str() {
            "Post" | "Output" => "Output",
            "Lab" if self.inspector_tab_chosen => "Lab",
            _ => "Prep",
        }
    }

    pub fn choose_tab(&mut self, tab: &str) {
        self.inspector_tab = tab.into();
        self.inspector_tab_chosen = true;
    }

    pub fn limits_preview(&self) -> bool {
        self.limit_preview.unwrap_or(false)
    }

    pub fn favorites(&self, paper: bool) -> &[String] {
        if paper { &self.favorite_papers } else { &self.favorite_films }
    }

    pub fn is_favorite(&self, id: &str, paper: bool) -> bool {
        self.favorites(paper).iter().any(|f| f == id)
    }

    /// Stars `id` (last among the favourites) or unstars it.
    pub fn toggle_favorite(&mut self, id: &str, paper: bool) {
        let ids = if paper { &mut self.favorite_papers } else { &mut self.favorite_films };
        match ids.iter().position(|f| f == id) {
            Some(index) => {
                ids.remove(index);
            }
            None => ids.push(id.to_string()),
        }
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
            schema: EditorSession::SCHEMA,
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
    fn schema_1_drops_dir_and_halation_values_that_never_applied() {
        let mut s = session();
        s.schema = 1;
        s.state.params = json!({"io": {"rotate_cw": 1}, "film_render": {
            "dir_couplers": {"gamma_samelayer_rgb": [0.341, 0.324, 0.273], "amount": 0.8},
            "halation": {"halation_strength": [0.05, 0.015, 0.0]},
            "grain": {"rms_granularity": [12, 13, 12]}}});
        s.migrate();
        assert_eq!(s.schema, EditorSession::SCHEMA);
        assert!(s.validate().is_ok());
        let render = &s.state.params["film_render"];
        assert!(render["dir_couplers"]["gamma_samelayer_rgb"].is_null());
        assert!(render["halation"]["halation_strength"].is_null());
        // Values that applied in schema 1 stay.
        assert_eq!(render["dir_couplers"]["amount"], json!(0.8));
        assert_eq!(render["grain"]["rms_granularity"], json!([12, 13, 12]));
        // A current session keeps set values.
        let mut current = session();
        current.state.params["film_render"] = json!({"dir_couplers": {"gamma_samelayer_rgb": [0.52, 0.51, 0.26]}});
        let before = current.clone();
        current.migrate();
        assert_eq!(current, before);
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

    #[test]
    fn settings_from_before_previews_turn_them_on() {
        let old: Preferences = serde_json::from_str(r#"{"recentFiles": [], "checkForUpdates": false}"#).unwrap();
        assert!(old.previews && !old.check_for_updates);
        let off: Preferences = serde_json::from_str(r#"{"previews": false}"#).unwrap();
        assert!(!off.previews);
        assert!(serde_json::to_string(&off).unwrap().contains(r#""previews":false"#));
        // Keep paper: off unless saved on, under the Mac's key.
        assert!(!old.keep_paper);
        let kept: Preferences = serde_json::from_str(r#"{"keep_paper": true}"#).unwrap();
        assert!(kept.keep_paper);
        assert!(serde_json::to_string(&kept).unwrap().contains(r#""keep_paper":true"#));
    }

    /// Settings files from before Prep: start fresh, match new photographs,
    /// open on Prep (or Output where Post was chosen), and lend no preview limit.
    #[test]
    fn settings_from_before_prep_take_the_new_defaults() {
        let old: Preferences = serde_json::from_str(r#"{"inspectorTab": "Post"}"#).unwrap();
        assert!(old.fresh_edits && old.match_new_photos && !old.low_memory_export && !old.lab_all_controls);
        assert_eq!((old.tab(), old.limit_preview, old.scope_height), ("Output", None, 88.0));
        let lab: Preferences = serde_json::from_str(r#"{"inspectorTab": "Lab"}"#).unwrap();
        assert_eq!(lab.tab(), "Prep", "Every file saved Lab as the default");
        let mut chosen = lab.clone();
        chosen.choose_tab("Lab");
        assert_eq!(chosen.tab(), "Lab");
        assert_eq!(Preferences::default().tab(), "Prep");
        let kept: Preferences = serde_json::from_str(r#"{"freshEditsForNewPhotos": false, "autoPlaceNewImages": false, "limitPreview": true}"#).unwrap();
        assert!(!kept.fresh_edits && !kept.match_new_photos && kept.limits_preview());
        let text = serde_json::to_string(&kept).unwrap();
        assert!(text.contains(r#""freshEditsForNewPhotos":false"#) && text.contains(r#""autoPlaceNewImages":false"#));
    }

    #[test]
    fn favourites_keep_the_order_starred() {
        // A user who cleared the defaults; new users start with four (below).
        let mut prefs = Preferences { favorite_films: vec![], ..Preferences::default() };
        prefs.toggle_favorite("portra", false);
        prefs.toggle_favorite("gold", false);
        prefs.toggle_favorite("endura", true);
        assert_eq!(prefs.favorites(false), ["portra", "gold"]);
        assert!(prefs.is_favorite("endura", true) && !prefs.is_favorite("endura", false));
        prefs.toggle_favorite("portra", false);
        prefs.toggle_favorite("portra", false);
        assert_eq!(prefs.favorites(false), ["gold", "portra"]);
    }

    /// New users start with the four films, and papers with none (`f182c00`).
    #[test]
    fn new_users_start_with_four_favourite_films() {
        let fresh = Preferences::default();
        assert_eq!(fresh.favorites(false), ["kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"]);
        assert!(fresh.favorites(true).is_empty());
    }

    /// The saved list is the user's, as on the Mac and Linux: a file without
    /// it (from before favourites) gets the four, an empty one stays empty,
    /// and a custom one is kept in its order, with ids the catalog lacks.
    #[test]
    fn saved_favourites_are_kept_as_saved() {
        let defaults: Vec<String> = DEFAULT_FAVORITE_FILMS.iter().map(|s| s.to_string()).collect();
        let empty_file: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(empty_file.favorite_films, defaults);
        let older: Preferences = serde_json::from_str(r#"{"recentFiles": [], "previews": true}"#).unwrap();
        assert_eq!((older.favorite_films, older.favorite_papers), (defaults, vec![]));
        let cleared: Preferences = serde_json::from_str(r#"{"favoriteFilms": [], "favoritePapers": []}"#).unwrap();
        assert!(cleared.favorite_films.is_empty() && cleared.favorite_papers.is_empty(), "an emptied list stays empty");
        let custom: Preferences = serde_json::from_str(r#"{"favoriteFilms": ["kodak_vision3_250d", "a_film_from_a_later_catalog"], "favoritePapers": ["kodak_endura_premier"]}"#).unwrap();
        assert_eq!(custom.favorite_films, ["kodak_vision3_250d", "a_film_from_a_later_catalog"]);
        assert_eq!(custom.favorite_papers, ["kodak_endura_premier"]);
        // Saved again, all of it comes back, including the empty list.
        for prefs in [cleared, custom] {
            let again: Preferences = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
            assert_eq!((again.favorite_films, again.favorite_papers), (prefs.favorite_films, prefs.favorite_papers));
        }
    }
}
