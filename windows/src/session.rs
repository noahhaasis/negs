//! The editing session saved at quit and the app's preferences. Small JSON
//! files in `%APPDATA%\Negs`; the catalog cache lives in `%LOCALAPPDATA%\Negs`.
//! `NEGS_PROFILE_DIR` moves all three, for tests and benchmarks.
//! Mirrors `Session.swift` and the macOS app's UserDefaults keys.
//!
//! On the shared core: `negs_core_rules::session` (core/src/settings/session)
//! reads and writes the files. EditorSession and Preferences are its
//! generic structs with this app's EditState and LibraryPreferences; the
//! Rust original is frozen in core/diff/oracle/session.rs.

#[cfg(test)]
use std::path::{Path, PathBuf};

use anyhow::Result;

#[cfg(test)]
use crate::state::EditState;

#[allow(unused_imports)]
pub use negs_core_rules::session::{CatalogCache, DEFAULT_FAVORITE_FILMS, SCOPE_HEIGHTS, cache_dir, same_path, settings_dir, write_atomic};

/// Small, versioned editing state. Pixels, caches and undo history are not saved.
pub type EditorSession = negs_core_rules::session::EditorSession<crate::state::EditState>;

/// Preferences, like the macOS app's UserDefaults: one JSON object in
/// `settings.json`, read once at launch and written when a value changes.
pub type Preferences = negs_core_rules::session::Preferences<crate::model::LibraryPreferences>;

/// The session file, `session.json` in the settings folder by default.
pub struct SessionStore {
    pub file: std::path::PathBuf,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self { file: negs_core_rules::session::SessionStore::default().file }
    }
}

impl SessionStore {
    fn store(&self) -> negs_core_rules::session::SessionStore {
        negs_core_rules::session::SessionStore { file: self.file.clone() }
    }

    pub fn load(&self) -> Result<Option<EditorSession>> {
        self.store().load()
    }

    pub fn save(&self, session: &EditorSession) -> Result<()> {
        self.store().save(session)
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
            library: false,
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
