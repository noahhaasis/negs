//! windows/src/session.rs on the shared core (core/src/settings/session):
//! the editing session saved at quit and the app's preferences, small JSON
//! files in `%APPDATA%\Negs`, and the catalog cache in `%LOCALAPPDATA%\Negs`
//! (`NEGS_PROFILE_DIR` moves all three).
//!
//! In Odin: the folders, `write_atomic`, reading and writing the files
//! (EditorSession's and Preferences' serde shapes, defaults and renames,
//! the flattened LibraryPreferences), schema 1's migration, validation, the
//! list helpers, `tab` and the CatalogCache. EditorSession and Preferences
//! stay Rust structs with their derives, generic over the state and the
//! library preferences (windows/src names its own types), and cross as the
//! JSON serde writes for them.
//!
//! Kept in Rust: `same_path`, `note_recent`, the favourites, `choose_tab`,
//! `limits_preview`, `save_folder`, `custom_viewer_background` (field
//! accessors), and serde's own error text when a session doesn't parse.
//! Fallbacks: a session or preferences serde writes but the core can't read
//! back (a double that isn't finite, which serde writes as null) is
//! checked and written by the small Rust paths below (`validate_value`,
//! the bytes serde wrote).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use negs_core_sys::settings as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::presets::path_text;

fn empty_buf() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// cfg!(test) of windows/: its tests (and this crate's) use a scratch
/// profile.
const TEST_PROFILE: bool = cfg!(any(test, feature = "host-tests"));

fn place(which: u32) -> PathBuf {
    let mut out = empty_buf();
    unsafe { sys::negs_settings_path(which, TEST_PROFILE as u8, &mut out) };
    PathBuf::from(utf8(take_buf(out)))
}

pub fn settings_dir() -> PathBuf {
    place(0)
}

pub fn cache_dir() -> PathBuf {
    place(1)
}

/// Writes `data` to a sibling temporary file and renames it over `path`, so a
/// crash never leaves half a file.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let p = path_text(path);
    let mut error = empty_buf();
    let ok = unsafe { sys::negs_settings_write_atomic(p.as_ptr(), p.len(), data.as_ptr(), data.len(), &mut error) } != 0;
    if ok { Ok(()) } else { Err(anyhow!(utf8(take_buf(error)))) }
}

/// Small, versioned editing state. Pixels, caches and undo history are not saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession<S = crate::state::EditState> {
    pub schema: u32,
    /// The photograph's path. There is no bookmark on Windows.
    pub source: PathBuf,
    pub state: S,
    #[serde(default = "yes")]
    pub inspector_visible: bool,
    #[serde(default = "yes")]
    pub fit_image: bool,
    /// The photograph's sidecar keeps its edits, and `state` is only what
    /// was on screen. Sessions from before the library lack it.
    #[serde(default)]
    pub library: bool,
}

fn yes() -> bool {
    true
}

const INVALID: &str = "The previous editing session could not be restored.";

impl<S> EditorSession<S> {
    /// 2: the DIR gammas and halation baseline are null unless set, and a set
    /// value beats the film's preset. Schema 1 stored values for them that the
    /// engine ignored and nothing could set, so `migrate` drops them.
    pub const SCHEMA: u32 = 2;
}

impl<S: Serialize + DeserializeOwned> EditorSession<S> {
    pub fn migrate(&mut self) {
        if self.schema != 1 {
            return;
        }
        // The state's JSON, its DIR and halation values nulled by the core.
        // A state serde can't write and read back (not from a file) keeps
        // its values.
        let migrated = serde_json::to_vec(&self.state).ok().and_then(|text| {
            let mut out = empty_buf();
            let ok = unsafe { sys::negs_session_migrate_state(text.as_ptr(), text.len(), &mut out) } != 0;
            let bytes = take_buf(out);
            if ok { serde_json::from_slice::<S>(&bytes).ok() } else { None }
        });
        if let Some(state) = migrated {
            self.state = state;
        }
        self.schema = Self::SCHEMA;
    }

    pub fn validate(&self) -> Result<()> {
        let valid = match serde_json::to_vec(self) {
            Ok(text) => match unsafe { sys::negs_session_validate(text.as_ptr(), text.len()) } {
                0 => false,
                1 => true,
                _ => self.validate_in_rust(),
            },
            Err(_) => self.validate_in_rust(),
        };
        ensure!(valid, INVALID);
        Ok(())
    }

    /// The fallback: the same checks on the state's JSON, where a double
    /// that isn't finite is null.
    fn validate_in_rust(&self) -> bool {
        let state = serde_json::to_value(&self.state).unwrap_or(Value::Null);
        validate_value(self.schema, &self.source, &state)
    }
}

/// EditorSession::validate's checks on the state as JSON.
fn validate_value(schema: u32, source: &Path, state: &Value) -> bool {
    let crop = &state["crop"];
    let (rect, custom) = (&crop["rect"], &crop["custom"]);
    let number = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    let (x, y, width, height) = (number(&rect[0][0]), number(&rect[0][1]), number(&rect[1][0]), number(&rect[1][1]));
    let (custom_width, custom_height, angle) = (number(&custom[0]), number(&custom[1]), number(&crop["angle"]));
    // A state without a crop has the default one.
    let default_crop = crop.is_null();
    let (x, y, width, height, custom_width, custom_height, angle) =
        if default_crop { (0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0) } else { (x, y, width, height, custom_width, custom_height, angle) };
    let values = [x, y, width, height, custom_width, custom_height, angle];
    let rotation = state["params"]["io"]["rotate_cw"].as_f64().unwrap_or(-1.0);
    schema == 2
        && source.is_absolute()
        && values.iter().all(|v| v.is_finite())
        && x >= 0.0
        && y >= 0.0
        && width > 0.0
        && height > 0.0
        && x + width <= 1.000001
        && y + height <= 1.000001
        && angle.abs() <= 45.0
        && custom_width > 0.0
        && custom_height > 0.0
        && state["params"].is_object()
        && (0.0..=3.0).contains(&rotation)
        && rotation.round() == rotation
}

pub struct SessionStore {
    pub file: PathBuf,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self { file: place(4) }
    }
}

impl SessionStore {
    pub fn load<S: Serialize + DeserializeOwned>(&self) -> Result<Option<EditorSession<S>>> {
        let file = path_text(&self.file);
        let mut out = empty_buf();
        let result = unsafe { sys::negs_session_load(file.as_ptr(), file.len(), &mut out) };
        let bytes = take_buf(out);
        match result {
            1 => Ok(None),
            2 => Ok(Some(serde_json::from_slice(&bytes).context(INVALID)?)),
            3 => {
                // Not a session: serde's own message, as the Rust gave it.
                let session: EditorSession<S> = serde_json::from_slice(&bytes).context(INVALID)?;
                // The core refused what serde reads: the Rust path.
                let mut session = session;
                session.migrate();
                session.validate()?;
                if !session.source.is_file() {
                    bail!("The previous photograph is unavailable. Open a photograph to continue.");
                }
                Ok(Some(session))
            }
            _ => Err(anyhow!(utf8(bytes))),
        }
    }

    pub fn save<S: Serialize + DeserializeOwned>(&self, session: &EditorSession<S>) -> Result<()> {
        let text = match serde_json::to_vec(session) {
            Ok(text) => text,
            Err(_) => {
                session.validate()?;
                serde_json::to_vec(session)?
            }
        };
        let file = path_text(&self.file);
        let mut error = empty_buf();
        match unsafe { sys::negs_session_save(file.as_ptr(), file.len(), text.as_ptr(), text.len(), &mut error) } {
            1 => Ok(()),
            2 => {
                // The core can't read serde's text back: checked here,
                // written as serde wrote it.
                session.validate()?;
                write_atomic(&self.file, &text)
            }
            _ => Err(anyhow!(utf8(take_buf(error)))),
        }
    }
}

/// Preferences, like the macOS app's UserDefaults: one JSON object in
/// `settings.json`, read once at launch and written when a value changes.
/// `L` is windows/src's LibraryPreferences (model/library_map.rs).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[serde(bound(deserialize = "L: Deserialize<'de> + Default", serialize = "L: Serialize"))]
pub struct Preferences<L> {
    pub recent_files: Vec<PathBuf>,
    pub filmstrip_visible: bool,
    /// The grid's row height (its Size).
    pub grid_size: f64,
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
    /// The normal rectangle's size in pixels and the DPI it was saved at.
    pub window_pixels: Option<[i32; 3]>,
    /// The adjustments column's width, in DIPs.
    pub inspector_width: f64,
    pub check_for_updates: bool,
    /// The last Image Size Longest side chosen.
    pub image_size_long_side: Option<u32>,
    /// Settings › Film, paper and preset previews. Old settings files lack it: on.
    pub previews: bool,
    /// The paper lock on "printed on" (the Mac's `keep_paper`).
    #[serde(rename = "keep_paper")]
    pub keep_paper: bool,
    /// The tone graph's channel: "rgb", "r", "g" or "b".
    pub tone_graph_channel: String,
    /// Start fresh (true, the default) or keep the previous photograph's edits.
    #[serde(rename = "freshEditsForNewPhotos")]
    pub fresh_edits: bool,
    /// Settings › Match new photographs to camera brightness.
    #[serde(rename = "autoPlaceNewImages")]
    pub match_new_photos: bool,
    /// Settings › Limit preview to 12 MP. None until set.
    pub limit_preview: Option<bool>,
    /// Settings › Use less memory for export.
    pub low_memory_export: bool,
    /// Where Lightroom and Lightroom Classic were last found (ticket 48).
    pub lightroom_cloud: Option<PathBuf>,
    pub lightroom_classic: Option<PathBuf>,
    /// View › Show All Controls.
    pub lab_all_controls: bool,
    /// The modules and sections whose "N more" was clicked, comma-separated.
    pub more_adjustments: String,
    /// The scope above the tabs: "Histogram" or "Waveform".
    pub scope_mode: String,
    /// The scope's height in DIPs.
    pub scope_height: f64,
    /// Starred stock ids, in the order starred.
    pub favorite_films: Vec<String>,
    pub favorite_papers: Vec<String>,
    /// Settings › Custom viewer background (the Mac's key).
    #[serde(rename = "viewerBackgroundCustom")]
    pub viewer_background_custom: bool,
    /// The custom colour behind the photograph, "#rrggbb" in sRGB.
    #[serde(rename = "viewerBackgroundColor")]
    pub viewer_background: String,
    /// Settings › Look › Viewer › Sheet, as the Mac stores it.
    pub viewer_sheet: String,
    /// View › Theme, as the macOS app stores it. Old settings files lack it: Darkroom.
    #[serde(default = "darkroom")]
    pub app_theme: String,
    /// The library's settings, under the Mac's keys.
    #[serde(flatten)]
    pub library: L,
    /// Settings › Fotufilm films: "all", "datasheets" or "off".
    pub fotufilm_films: String,
    /// Settings › Show Negs-built films.
    pub show_negs_built_films: bool,
    /// Enlarger › Shifts move: "separate", "together" or "opposite".
    pub filter_shift_link: String,
    /// Original ⌄: "hold", "split" or "sideBySide"; "raw" or "cameraJPEG".
    pub original_mode: String,
    pub original_reference: String,
    /// The Settings tab last shown (the Mac's `settingsTab`).
    pub settings_tab: String,
    /// The batch dialog's File Name, its Custom text and If a File Exists.
    pub batch_naming: String,
    pub batch_custom_name: String,
    pub batch_existing: String,
}

fn darkroom() -> String {
    "darkroom".into()
}

/// Starred for new users, as on the web and the Mac; once they change the
/// list, theirs is kept, even when empty.
pub const DEFAULT_FAVORITE_FILMS: [&str; 4] = ["kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"];

/// The scope's heights in DIPs; it starts at the smallest, leaving the room
/// to the controls.
pub const SCOPE_HEIGHTS: (f64, f64) = (88.0, 420.0);

/// sheet_rules' GRID_DEFAULT, the grid's starting row height.
const GRID_DEFAULT: f64 = 176.0;

/// As session.rs's: a literal, held to the core's default in the tests
/// (`negs_preferences_default`).
impl<L: Default> Default for Preferences<L> {
    fn default() -> Self {
        Self {
            recent_files: vec![],
            filmstrip_visible: true,
            grid_size: GRID_DEFAULT,
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
            window_pixels: None,
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
            lightroom_cloud: None,
            lightroom_classic: None,
            lab_all_controls: false,
            more_adjustments: String::new(),
            scope_mode: "Histogram".into(),
            scope_height: SCOPE_HEIGHTS.0,
            favorite_films: DEFAULT_FAVORITE_FILMS.iter().map(|s| s.to_string()).collect(),
            favorite_papers: vec![],
            viewer_background_custom: false,
            viewer_background: "#242424".into(),
            viewer_sheet: String::new(),
            app_theme: String::new(),
            library: Default::default(),
            fotufilm_films: "all".into(),
            show_negs_built_films: false,
            filter_shift_link: "separate".into(),
            original_mode: "hold".into(),
            original_reference: "raw".into(),
            batch_naming: "nameFilm".into(),
            batch_custom_name: "{name}_{film}".into(),
            batch_existing: "number".into(),
            settings_tab: "Photographs".into(),
        }
    }
}

fn list_set(list: &str, item: &str, on: bool, sorted: bool) -> String {
    utf8(take_buf(unsafe { sys::negs_preferences_list_set(list.as_ptr(), list.len(), item.as_ptr(), item.len(), on as u8, sorted as u8) }))
}

impl<L> Preferences<L> {
    /// The colour behind the photograph when Custom viewer background is on
    /// and its colour reads.
    pub fn custom_viewer_background(&self) -> Option<[u8; 3]> {
        self.viewer_background_custom.then(|| crate::border::BorderColor::from_hex(&self.viewer_background)).flatten().map(|c| c.0)
    }

    #[cfg(any(test, feature = "host-tests"))]
    pub fn expanded(&self, title: &str) -> bool {
        let list = &self.expanded_adjustments;
        unsafe { sys::negs_preferences_list_has(list.as_ptr(), list.len(), title.as_ptr(), title.len()) != 0 }
    }

    pub fn set_expanded(&mut self, title: &str, open: bool) {
        self.expanded_adjustments = list_set(&self.expanded_adjustments, title, open, false);
    }

    /// Shows or folds back `key`'s extra controls; kept sorted, as on the Mac.
    pub fn set_revealed(&mut self, key: &str, shown: bool) {
        self.more_adjustments = list_set(&self.more_adjustments, key, shown, true);
    }

    /// The tab the inspector shows. Post became Output; settings files from
    /// before Prep that never chose a tab open on Prep.
    pub fn tab(&self) -> &str {
        let tab = &self.inspector_tab;
        match unsafe { sys::negs_preferences_tab(tab.as_ptr(), tab.len(), self.inspector_tab_chosen as u8) } {
            2 => "Output",
            1 => "Lab",
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

impl<L: Serialize + DeserializeOwned + Default> Preferences<L> {
    pub fn load() -> Self {
        let file = path_text(&place(3));
        let mut out = empty_buf();
        let ok = unsafe { sys::negs_preferences_load(file.as_ptr(), file.len(), &mut out) } != 0;
        let text = take_buf(out);
        if !ok {
            return Self::default();
        }
        serde_json::from_slice(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let Ok(text) = serde_json::to_vec(self) else { return };
        let file = place(3);
        let p = path_text(&file);
        if unsafe { sys::negs_preferences_save(p.as_ptr(), p.len(), text.as_ptr(), text.len()) } == 2 {
            // The core can't read serde's text back: serde's pretty bytes.
            if let Ok(data) = serde_json::to_vec_pretty(self) {
                let _ = write_atomic(&file, &data);
            }
        }
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
    pub fn load(helper: &Path, data: &Path) -> Option<Value> {
        let (file, helper, data) = (path_text(&place(5)), path_text(helper), path_text(data));
        let mut out = empty_buf();
        let ok = unsafe {
            sys::negs_catalog_load(file.as_ptr(), file.len(), helper.as_ptr(), helper.len(), data.as_ptr(), data.len(), &mut out)
        } != 0;
        let text = take_buf(out);
        if !ok {
            return None;
        }
        serde_json::from_slice(&text).ok()
    }

    pub fn save(catalog: &Value, helper: &Path, data: &Path) {
        let (file, helper, data) = (path_text(&place(5)), path_text(helper), path_text(data));
        let text = catalog.to_string();
        unsafe {
            sys::negs_catalog_save(
                file.as_ptr(),
                file.len(),
                text.as_ptr(),
                text.len(),
                helper.as_ptr(),
                helper.len(),
                data.as_ptr(),
                data.len(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EditState;
    use serde_json::json;

    /// model/library_map.rs's LibraryPreferences, as windows/src has it.
    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default, rename_all = "camelCase")]
    struct LibraryPreferences {
        library_filter: Option<Value>,
        copy_edit_groups: Option<Vec<String>>,
        advance_after_rating: bool,
        new_photo_film: Option<String>,
        new_photo_paper: Option<String>,
        #[serde(rename = "separateCameraJPEGs")]
        separate_camera_jpegs: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        sidecars: Option<bool>,
    }

    type Prefs = Preferences<LibraryPreferences>;
    type Session = EditorSession<EditState>;

    fn session() -> Session {
        let state: EditState = serde_json::from_value(json!({"params": {"io": {"rotate_cw": 1}}, "film": "kodak_gold_200", "paper": "kodak_portra_endura"})).unwrap();
        Session { schema: Session::SCHEMA, source: std::env::temp_dir().join("DSC_1.NEF"), state, inspector_visible: true, fit_image: true, library: false }
    }

    #[test]
    fn the_default_is_the_cores() {
        let core = take_buf(unsafe { sys::negs_preferences_default(negs_core_sys::state::NEGS_JSON_PRETTY) });
        assert_eq!(String::from_utf8(core).unwrap(), serde_json::to_string_pretty(&Prefs::default()).unwrap());
    }

    #[test]
    fn validation_rejects_bad_crops_and_rotations() {
        assert!(session().validate().is_ok());
        let mut s = session();
        s.state.crop.rect = crate::crop::Rect::new(0.5, 0.0, 0.6, 1.0);
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
        // NaN: serde writes null, the core can't read it, Rust checks it.
        let mut s = session();
        s.state.crop.angle = f64::NAN;
        assert!(s.validate().is_err());
        let mut s = session();
        s.state.border.width = f64::NAN;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn sessions_round_trip_and_reject_missing_photographs() {
        let dir = std::env::temp_dir().join(format!("negs-rules-session-{}", std::process::id()));
        let store = SessionStore { file: dir.join("session.json") };
        assert!(store.load::<EditState>().unwrap().is_none());
        store.save(&session()).unwrap();
        let text = std::fs::read_to_string(&store.file).unwrap();
        assert!(text.contains(r#""inspectorVisible":true"#) && text.contains(r#""limitPreview":false"#));
        let missing = Session { source: dir.join("missing.png"), ..session() };
        store.save(&missing).unwrap();
        assert!(store.load::<EditState>().unwrap_err().to_string().contains("unavailable"));
        let photo = dir.join("photo.png");
        std::fs::write(&photo, b"x").unwrap();
        let s = Session { source: photo, ..session() };
        store.save(&s).unwrap();
        assert_eq!(serde_json::to_string(&store.load::<EditState>().unwrap().unwrap()).unwrap(), serde_json::to_string(&s).unwrap());
        std::fs::write(&store.file, b"{\"schema\": 2}").unwrap();
        let error = format!("{:#}", store.load::<EditState>().unwrap_err());
        assert!(error.starts_with("The previous editing session could not be restored.: missing field"), "{error}");
        std::fs::write(&store.file, vec![b' '; 1_048_577]).unwrap();
        assert!(store.load::<EditState>().unwrap_err().to_string().contains("too large"));
        // A NaN serde writes as null: saved as serde wrote it.
        let mut odd = s.clone();
        odd.state.border.width = f64::NAN;
        store.save(&odd).unwrap();
        assert!(std::fs::read_to_string(&store.file).unwrap().contains(r#""width":null"#));
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
        assert_eq!(s.schema, Session::SCHEMA);
        assert!(s.validate().is_ok());
        let render = &s.state.params["film_render"];
        assert!(render["dir_couplers"]["gamma_samelayer_rgb"].is_null());
        assert!(render["halation"]["halation_strength"].is_null());
        assert_eq!(render["dir_couplers"]["amount"], json!(0.8));
        assert_eq!(render["grain"]["rms_granularity"], json!([12, 13, 12]));
        let mut current = session();
        current.state.params["film_render"] = json!({"dir_couplers": {"gamma_samelayer_rgb": [0.52, 0.51, 0.26]}});
        let before = serde_json::to_string(&current).unwrap();
        current.migrate();
        assert_eq!(serde_json::to_string(&current).unwrap(), before);
    }

    #[test]
    fn recent_files_and_groups() {
        let mut prefs = Prefs::default();
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
        prefs.set_revealed("section:Print", true);
        prefs.set_revealed("Grain", true);
        assert_eq!(prefs.more_adjustments, "Grain,section:Print");
    }

    #[test]
    fn settings_from_before_prep_take_the_new_defaults() {
        let old: Prefs = serde_json::from_str(r#"{"inspectorTab": "Post"}"#).unwrap();
        assert!(old.fresh_edits && old.match_new_photos && !old.low_memory_export && !old.lab_all_controls);
        assert_eq!((old.tab(), old.limit_preview, old.scope_height), ("Output", None, 88.0));
        let lab: Prefs = serde_json::from_str(r#"{"inspectorTab": "Lab"}"#).unwrap();
        assert_eq!(lab.tab(), "Prep");
        let mut chosen = lab.clone();
        chosen.choose_tab("Lab");
        assert_eq!(chosen.tab(), "Lab");
        assert_eq!(Prefs::default().tab(), "Prep");
    }

    #[test]
    fn the_test_profile_is_a_scratch_folder() {
        if std::env::var_os("NEGS_PROFILE_DIR").is_none() {
            assert!(settings_dir().starts_with(std::env::temp_dir()));
            assert_eq!(cache_dir(), settings_dir().join("Cache"));
        }
        assert_eq!(SessionStore::default().file, settings_dir().join("session.json"));
        assert_eq!(crate::presets::Library::default().folder, settings_dir().join("Presets"));
    }
}
