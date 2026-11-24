//! Smoke tests for Phase 4's frozen oracles (negs-state-oracle): each module
//! compiles and does on macOS what windows/src does, with the stand-ins
//! described in core/diff/state-oracle/src/lib.rs. The differential tests
//! (value.rs, edit_state.rs, and 4b's and 4c's) lean on them.

use negs_state_oracle::{batch, engine, json, match_cache, model, presets, rendered_preview, session, state};
use serde_json::json;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-state-oracle-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn state_and_json() {
    let mut s = state::EditState { params: json!({"io": {"rotate_cw": 1, "flip": true}}), ..Default::default() };
    assert_eq!((s.rotation(), s.flip()), (1, true));
    json::set(&mut s.params, "a.b", json!(2));
    assert_eq!(json::get(&s.params, "a.b"), &json!(2));
    let text = serde_json::to_string(&s).unwrap();
    assert!(text.starts_with(r#"{"params":{"#), "{text}");
    assert_eq!(serde_json::from_str::<state::EditState>(&text).unwrap(), s);
    assert_eq!(json::get(&s.rendered().params, "io.rotate_cw"), &serde_json::Value::Null);
    assert!(state::match_inputs(&s).get("film").is_some());
}

/// FindNLSStringEx's stand-in, through `matching`: case and accents fold.
#[test]
fn film_search_folds_case_and_accents() {
    let stock = |id: &str, name: &str| state::Stock {
        id: id.into(), name: name.into(), paper: false, positive: false, bw: false, target: String::new(), times: vec![],
        grain: serde_json::Value::Null, fotufilm: false, manufacturer_data: true, negs_built: false, cine_print: false,
    };
    let films = [stock("cine", "Ciné Noir"), stock("gold", "Gold 200")];
    let f: Vec<&state::Stock> = films.iter().collect();
    let ids = |q: &str| -> Vec<String> { state::matching(q, state::film_groups(&f, &[])).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
    assert_eq!(ids("CINE"), ["cine"]);
    assert_eq!(ids("ciné noir"), ["cine"]);
    assert_eq!(ids("gold"), ["gold"]);
    assert!(ids("silver").is_empty());
}

#[test]
fn session_round_trip() {
    let dir = scratch("session");
    let photo = dir.join("photo.jpg");
    std::fs::write(&photo, b"x").unwrap();
    let store = session::SessionStore { file: dir.join("session.json") };
    let saved = session::EditorSession {
        schema: session::EditorSession::SCHEMA,
        source: photo,
        state: state::EditState { params: json!({"io": {"rotate_cw": 1}}), ..Default::default() },
        inspector_visible: true,
        fit_image: false,
        library: true,
    };
    store.save(&saved).unwrap();
    let loaded = store.load().unwrap().unwrap();
    assert_eq!(loaded.state, saved.state);
    assert!(!loaded.fit_image && loaded.library);
    let prefs: session::Preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(prefs.favorites(false).len(), session::DEFAULT_FAVORITE_FILMS.len());
    std::fs::remove_dir_all(dir).unwrap();
}

/// MoveFileExW without REPLACE_EXISTING, NormalizeString, the hidden flag.
#[test]
fn presets_add_rename_and_refuse_twins() {
    // Library::new is the oracle's test-only door; the app's is its
    // profile folder, which NEGS_PROFILE_DIR moves (no other test here
    // reads it).
    let profile = scratch("presets");
    unsafe { std::env::set_var("NEGS_PROFILE_DIR", &profile) };
    let library = presets::Library::default();
    let a = library.add(br#"{"film": "kodak_gold_200"}"#, "Warm").unwrap();
    assert_eq!(a.name(), "Warm");
    // NFC: "é" composed and decomposed are one preset name.
    assert_eq!(presets::name::key("Caf\u{e9}"), presets::name::key("Cafe\u{301}"));
    let b = library.add(br#"{"film": "kodak_portra_400"}"#, "Cool").unwrap();
    assert!(library.rename(&b, "Warm").is_err(), "a rename onto another preset fails");
    let b = library.rename(&b, "Cold").unwrap();
    assert_eq!(b.name(), "Cold");
    library.set_order(&[b.clone(), a.clone()]).unwrap();
    let names: Vec<String> = library.list().unwrap().iter().map(presets::PresetFile::name).collect();
    assert_eq!(names, ["Cold", "Warm"], "the order file is hidden and not a preset");
    library.delete(&a).unwrap();
    assert_eq!(library.list().unwrap().len(), 1);
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn batch_options_and_words() {
    let options = batch::Options::default();
    assert_eq!(options.output_format(), model::OutputFormat::Jpeg);
    assert_eq!(options.export_engine(), engine::ExportEngine::Reference);
    assert_eq!(batch::exporting(1, 3, None), "Exporting 2 of 3 photographs");
    assert_eq!(batch::numbered("a.jpg", |n| n == "a.jpg"), "a 2.jpg");
    let text = serde_json::to_string(&options).unwrap();
    assert_eq!(serde_json::from_str::<batch::Options>(&text).unwrap().format, options.format);
}

#[test]
fn caches_key_and_keep() {
    let dir = scratch("caches");
    let photo = dir.join("photo.jpg");
    std::fs::write(&photo, b"x").unwrap();
    let s = state::EditState::default();
    let key = match_cache::key(&photo, &s, "engine").unwrap();
    assert_eq!(key.len(), 64);
    assert_eq!(match_cache::key(&photo, &s, "engine").unwrap(), key);
    let cache = std::sync::Arc::new(match_cache::MatchCache::new(dir.join("matches.json"), 10));
    cache.store(key.clone(), 0.5, false);
    cache.flush();
    assert_eq!(match_cache::MatchCache::new(dir.join("matches.json"), 10).solved(&key), Some((0.5, false)));

    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("film.json"), b"{}").unwrap();
    let paths = engine::EnginePaths { helper: photo.clone(), exporter: photo.clone(), data, fast_exporter: None };
    let a = rendered_preview::key(&photo, &s, &paths).unwrap();
    let mut turned = s.clone();
    json::set(&mut turned.params, "io.rotate_cw", json!(1));
    assert_eq!(rendered_preview::key(&photo, &turned, &paths).unwrap(), a, "rotation isn't part of the render");
    std::fs::remove_dir_all(dir).unwrap();
}
