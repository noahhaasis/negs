//! session.rs on the core (core/src/settings/session, through the C ABI and
//! negs_core_rules::session) against the frozen windows/src/session.rs
//! (core/diff/oracle/session.rs):
//! - Preferences through serde both ways, byte for byte: random preferences
//!   written compact and pretty; random and mutated texts (keys missing,
//!   null, of the wrong type, unknown, doubled, flattened LibraryPreferences
//!   keys, the array form, bytes changed) accepted or refused alike, and
//!   what is read written alike; the default;
//! - EditorSession the same way, and validate and migrate on random
//!   sessions;
//! - SessionStore, Preferences' load and save and the CatalogCache on
//!   scratch files, each side saving and each reading the other's file;
//! - the folders (NEGS_PROFILE_DIR, APPDATA, LOCALAPPDATA), write_atomic's
//!   errors, the list helpers and `tab`.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing. Every file is in a scratch folder in the
//! temporary one; NEGS_PROFILE_DIR points there.

mod rules_common;
mod state_common;

use std::path::{Path, PathBuf};

use negs_core_rules::session as core;
use negs_core_sys::settings as sys;
use negs_core_sys::take_buf;
use negs_state_oracle::model::LibraryPreferences;
use negs_state_oracle::session as oracle;
use negs_state_oracle::state::EditState;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use serde_json::{Map, Value, json};
use state_common::*;

type CorePrefs = core::Preferences<LibraryPreferences>;
type CoreSession = core::EditorSession<EditState>;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-diff-session-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_env(key: &str, value: Option<&str>) {
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}

// MARK: Random preferences

fn opt<T>(r: &mut Random, f: impl FnOnce(&mut Random) -> T) -> Option<T> {
    if r.chance(3) { None } else { Some(f(r)) }
}

fn strings(r: &mut Random) -> Vec<String> {
    (0..r.index(5)).map(|_| string(r)).collect()
}

fn library_preferences(r: &mut Random) -> LibraryPreferences {
    LibraryPreferences {
        library_filter: opt(r, |r| value(r, 3)).filter(|v| !v.is_null()),
        copy_edit_groups: opt(r, strings),
        advance_after_rating: r.bool(),
        new_photo_film: opt(r, string),
        new_photo_paper: opt(r, string),
        separate_camera_jpegs: r.bool(),
        sidecars: opt(r, |r| r.bool()),
    }
}

fn u32_of(r: &mut Random) -> u32 {
    if r.chance(4) { r.next() as u32 } else { r.below(5000) as u32 }
}

fn i32_of(r: &mut Random) -> i32 {
    if r.chance(4) { r.next() as i32 } else { r.below(4000) as i32 - 2000 }
}

fn f64_of(r: &mut Random) -> f64 {
    if r.chance(40) { [f64::NAN, f64::INFINITY, f64::NEG_INFINITY][r.index(3)] } else { double(r) }
}

fn preferences(r: &mut Random) -> oracle::Preferences {
    if r.chance(20) {
        return oracle::Preferences::default();
    }
    oracle::Preferences {
        recent_files: (0..r.index(4)).map(|_| PathBuf::from(string(r))).collect(),
        filmstrip_visible: r.bool(),
        grid_size: f64_of(r),
        expanded_adjustments: string(r),
        inspector_tab: ["Prep", "Lab", "Post", "Output", ""][r.index(5)].to_string(),
        inspector_tab_chosen: r.bool(),
        inspector_pane: string(r),
        export_engine: string(r),
        output_format: string(r),
        export_edge: string(r),
        export_edge_pixels: u32_of(r),
        color_space: string(r),
        png_bit_depth: u32_of(r),
        tiff_bit_depth: u32_of(r),
        jpeg_quality: u32_of(r),
        last_save_folder: opt(r, |r| PathBuf::from(string(r))),
        window: opt(r, |r| [i32_of(r), i32_of(r), i32_of(r), i32_of(r), i32_of(r)]),
        window_pixels: opt(r, |r| [i32_of(r), i32_of(r), i32_of(r)]),
        inspector_width: f64_of(r),
        check_for_updates: r.bool(),
        image_size_long_side: opt(r, u32_of),
        previews: r.bool(),
        keep_paper: r.bool(),
        tone_graph_channel: string(r),
        fresh_edits: r.bool(),
        match_new_photos: r.bool(),
        limit_preview: opt(r, |r| r.bool()),
        low_memory_export: r.bool(),
        lightroom_cloud: opt(r, |r| PathBuf::from(string(r))),
        lightroom_classic: opt(r, |r| PathBuf::from(string(r))),
        lab_all_controls: r.bool(),
        more_adjustments: string(r),
        scope_mode: string(r),
        scope_height: f64_of(r),
        favorite_films: strings(r),
        favorite_papers: strings(r),
        viewer_background_custom: r.bool(),
        viewer_background: string(r),
        viewer_sheet: string(r),
        app_theme: string(r),
        library: library_preferences(r),
        fotufilm_films: string(r),
        show_negs_built_films: r.bool(),
        filter_shift_link: string(r),
        original_mode: string(r),
        original_reference: string(r),
        settings_tab: string(r),
        batch_naming: string(r),
        batch_custom_name: string(r),
        batch_existing: string(r),
    }
}

/// The keys serde reads, and a value of about their type.
const KEYS: &[(&str, u8)] = &[
    ("recentFiles", b'L'),
    ("filmstripVisible", b'b'),
    ("gridSize", b'f'),
    ("expandedAdjustments", b's'),
    ("inspectorTab", b's'),
    ("inspectorTabChosen", b'b'),
    ("exportEdgePixels", b'u'),
    ("pngBitDepth", b'u'),
    ("jpegQuality", b'u'),
    ("lastSaveFolder", b'S'),
    ("window", b'5'),
    ("windowPixels", b'3'),
    ("inspectorWidth", b'f'),
    ("imageSizeLongSide", b'U'),
    ("previews", b'b'),
    ("keep_paper", b'b'),
    ("keepPaper", b'b'),
    ("freshEditsForNewPhotos", b'b'),
    ("autoPlaceNewImages", b'b'),
    ("limitPreview", b'B'),
    ("lightroomCloud", b'S'),
    ("scopeHeight", b'f'),
    ("favoriteFilms", b'L'),
    ("favoritePapers", b'L'),
    ("viewerBackgroundCustom", b'b'),
    ("viewerBackgroundColor", b's'),
    ("appTheme", b's'),
    ("libraryFilter", b'v'),
    ("copyEditGroups", b'M'),
    ("advanceAfterRating", b'b'),
    ("newPhotoFilm", b'S'),
    ("newPhotoPaper", b'S'),
    ("separateCameraJPEGs", b'b'),
    ("sidecars", b'B'),
    ("settingsTab", b's'),
    ("batchExisting", b's'),
    ("unknown", b'v'),
    ("", b'v'),
];

fn typed(r: &mut Random, kind: u8) -> Value {
    if r.chance(8) {
        return scalar(r);
    }
    let nullable = matches!(kind, b'S' | b'U' | b'B' | b'5' | b'3' | b'M' | b'v');
    if nullable && r.chance(4) {
        return Value::Null;
    }
    match kind {
        b'b' | b'B' => Value::Bool(r.bool()),
        b'f' => number(r),
        b'u' | b'U' => match r.below(6) {
            0 => json!(-1),
            1 => json!(u32::MAX as u64 + r.below(3)),
            2 => json!(2.0),
            _ => json!(r.below(5000)),
        },
        b's' | b'S' => Value::String(string(r)),
        b'L' | b'M' => Value::Array((0..r.index(4)).map(|_| if r.chance(10) { scalar(r) } else { Value::String(string(r)) }).collect()),
        b'5' | b'3' => {
            let n = if kind == b'5' { 5 } else { 3 } + if r.chance(8) { r.index(3) } else { 0 } - if r.chance(8) { 1 } else { 0 };
            Value::Array((0..n).map(|_| if r.chance(10) { number(r) } else { json!(i32_of(r)) }).collect())
        }
        _ => value(r, 3),
    }
}

/// A preferences text: serde's of random preferences, or a random object of
/// the keys, then often edited.
fn preferences_text(r: &mut Random) -> Vec<u8> {
    let mut t = match r.below(4) {
        0 => {
            let p = preferences(r);
            if r.bool() { serde_json::to_vec_pretty(&p).unwrap() } else { serde_json::to_vec(&p).unwrap() }
        }
        1 => json_text(r),
        _ => {
            let mut parts = vec![];
            for _ in 0..r.index(8) {
                let (key, kind) = KEYS[r.index(KEYS.len())];
                parts.push(format!("{}:{}", serde_json::to_string(key).unwrap(), serde_json::to_string(&typed(r, kind)).unwrap()));
            }
            if r.chance(10) {
                format!("[{}]", parts.iter().map(|p| p.split_once(':').unwrap().1).collect::<Vec<_>>().join(",")).into_bytes()
            } else {
                format!("{{{}}}", parts.join(",")).into_bytes()
            }
        }
    };
    if r.chance(6) {
        t = mutate(r, &t);
    }
    t
}

fn core_rewrite_prefs(text: &[u8], flags: u32) -> Option<Vec<u8>> {
    let mut out = empty();
    let ok = unsafe { sys::negs_preferences_rewrite(text.as_ptr(), text.len(), flags, &mut out) } != 0;
    let bytes = take_buf(out);
    ok.then_some(bytes)
}

#[test]
fn preferences_through_serde_both_ways() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = cases();
    let default_core = take_buf(unsafe { sys::negs_preferences_default(sys_pretty()) });
    tally.check("default", 0, serde_json::to_vec_pretty(&oracle::Preferences::default()).unwrap(), default_core);
    tally.check(
        "wrapper default",
        0,
        serde_json::to_string(&oracle::Preferences::default()).unwrap(),
        serde_json::to_string(&CorePrefs::default()).unwrap(),
    );
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        let p = preferences(&mut r);
        let compact = serde_json::to_vec(&p).unwrap();
        let readable = serde_json::from_slice::<oracle::Preferences>(&compact).ok().map(|p| serde_json::to_vec_pretty(&p).unwrap());
        tally.check("rewrite pretty", seed, readable.clone(), core_rewrite_prefs(&compact, sys_pretty()));
        tally.check("rewrite compact", seed, readable.map(|_| compact.clone()), core_rewrite_prefs(&compact, 0));
        let text = preferences_text(&mut r);
        let oracle_read = serde_json::from_slice::<oracle::Preferences>(&text).ok().map(|p| serde_json::to_vec(&p).unwrap());
        tally.check("read", seed, oracle_read, core_rewrite_prefs(&text, 0));
    }
    tally.finish("session preferences serde", n);
    leaks.check("session preferences serde");
}

fn sys_pretty() -> u32 {
    negs_core_sys::state::NEGS_JSON_PRETTY
}

// MARK: Random sessions

fn session(r: &mut Random, d: &Defaults, photo: &Path) -> oracle::EditorSession {
    let mut state = edit_state(r, d);
    if r.chance(3) && state.params.is_object() {
        state.params["io"] = json!({"rotate_cw": ([json!(r.below(5)), json!(1.5), json!(-1), json!("1"), Value::Null][r.index(5)].clone())});
    }
    let source = match r.below(6) {
        0 => PathBuf::from(string(r)),
        1 => photo.with_file_name("missing.png"),
        2 => photo.parent().unwrap().to_path_buf(),
        _ => photo.to_path_buf(),
    };
    oracle::EditorSession {
        schema: [1, 2, 2, 2, 3, 0][r.index(6)],
        source,
        state,
        inspector_visible: r.bool(),
        fit_image: r.bool(),
        library: r.bool(),
    }
}

fn to_core(s: &oracle::EditorSession) -> CoreSession {
    CoreSession {
        schema: s.schema,
        source: s.source.clone(),
        state: s.state.clone(),
        inspector_visible: s.inspector_visible,
        fit_image: s.fit_image,
        library: s.library,
    }
}

const SESSION_KEYS: &[&str] = &["schema", "source", "state", "inspectorVisible", "fitImage", "library", "other"];

fn session_text(r: &mut Random, s: &oracle::EditorSession) -> Vec<u8> {
    let mut v = serde_json::to_value(s).unwrap();
    let t = match r.below(6) {
        0 => {
            let map = v.as_object_mut().unwrap();
            for _ in 0..1 + r.index(3) {
                let key = SESSION_KEYS[r.index(SESSION_KEYS.len())];
                match r.below(3) {
                    0 => {
                        map.remove(key);
                    }
                    1 => {
                        map.insert(key.to_string(), scalar(r));
                    }
                    _ => {
                        map.insert(key.to_string(), json!(r.below(4)));
                    }
                }
            }
            serde_json::to_vec(&v).unwrap()
        }
        1 => {
            let t = serde_json::to_string(&v).unwrap();
            let key = SESSION_KEYS[r.index(SESSION_KEYS.len())];
            format!("{{\"{key}\":{},{}", serde_json::to_string(&scalar(r)).unwrap(), &t[1..]).into_bytes()
        }
        2 => {
            let m = v.as_object().unwrap();
            let fields: Vec<Value> = ["schema", "source", "state", "inspectorVisible", "fitImage", "library"].iter().map(|k| m[*k].clone()).collect();
            let keep = r.index(fields.len() + 2);
            serde_json::to_vec(&fields.into_iter().take(keep).collect::<Vec<_>>()).unwrap()
        }
        3 => mutate(r, &serde_json::to_vec(&v).unwrap()),
        _ => serde_json::to_vec_pretty(&v).unwrap(),
    };
    t
}

fn core_rewrite_session(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = empty();
    let ok = unsafe { sys::negs_session_rewrite(text.as_ptr(), text.len(), &mut out) } != 0;
    let bytes = take_buf(out);
    ok.then_some(bytes)
}

#[test]
fn sessions_through_serde_validate_and_migrate() {
    let _serial = serial();
    let leaks = Leaks::start();
    let dir = scratch("serde");
    let photo = dir.join("photo.png");
    std::fs::write(&photo, b"x").unwrap();
    let d = engine_defaults();
    let mut tally = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        let s = session(&mut r, &d, &photo);
        let compact = serde_json::to_vec(&s).unwrap();
        tally.check("rewrite", seed, Some(compact.clone()), core_rewrite_session(&compact));
        let text = session_text(&mut r, &s);
        let oracle_read = serde_json::from_slice::<oracle::EditorSession>(&text).ok().map(|s| serde_json::to_vec(&s).unwrap());
        tally.check("read", seed, oracle_read, core_rewrite_session(&text));
        // validate, through the wrapper (and its Rust fallback).
        let mut v = s.clone();
        if v.schema == 1 || v.schema == 3 {
            v.schema = 2;
        }
        if r.chance(10) {
            v.state.crop.angle = [f64::NAN, f64::INFINITY, 46.0][r.index(3)];
        }
        if r.chance(10) {
            v.state.border.width = f64::NAN;
        }
        tally.check("validate", seed, v.validate().is_ok(), to_core(&v).validate().is_ok());
        // migrate.
        let mut m = s.clone();
        let mut c = to_core(&s);
        m.migrate();
        c.migrate();
        tally.check("migrate", seed, serde_json::to_string(&m).unwrap(), serde_json::to_string(&c).unwrap());
    }
    std::fs::remove_dir_all(&dir).unwrap();
    tally.finish("session serde", n);
    leaks.check("session serde");
}

// MARK: Files

fn result_text<T>(r: anyhow::Result<T>, show: impl Fn(T) -> String) -> Result<String, String> {
    r.map(show).map_err(|e| format!("{e:#}"))
}

fn files_of(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// One of the files' states, made the same on both sides.
fn disturb(r: &mut Random, files: [&Path; 2], d: &Defaults, photo: &Path) {
    let make = |f: &dyn Fn(&Path)| {
        for p in files {
            let _ = std::fs::remove_dir_all(p);
            let _ = std::fs::remove_file(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            f(p);
        }
    };
    match r.below(8) {
        0 => make(&|_| {}),
        1 => {
            let s = session(r, d, photo);
            let t = session_text(r, &s);
            make(&|p| std::fs::write(p, &t).unwrap());
        }
        2 => make(&|p| std::fs::create_dir_all(p).unwrap()),
        3 => {
            let size = 1_048_576 + r.below(2) as usize;
            make(&|p| std::fs::write(p, vec![b' '; size]).unwrap());
        }
        4 => {
            let t = json_text(r);
            make(&|p| std::fs::write(p, &t).unwrap());
        }
        _ => {}
    }
}

#[test]
fn stores_save_and_load_alike() {
    let _serial = serial();
    let leaks = Leaks::start();
    let dir = scratch("stores");
    let photo = dir.join("photo.png");
    std::fs::write(&photo, b"x").unwrap();
    let (a, b) = (dir.join("a").join("session.json"), dir.join("b").join("session.json"));
    let oracle_store = oracle::SessionStore { file: a.clone() };
    let core_store = core::SessionStore { file: b.clone() };
    let d = engine_defaults();
    let mut tally = Tally::default();
    let n = cases() / 4;
    let show = |s: Option<oracle::EditorSession>| serde_json::to_string(&s).unwrap();
    let show_core = |s: Option<CoreSession>| serde_json::to_string(&s).unwrap();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        if r.chance(3) {
            disturb(&mut r, [&a, &b], &d, &photo);
        } else {
            let mut s = session(&mut r, &d, &photo);
            if r.chance(20) {
                s.state.border.width = f64::NAN;
            }
            // The stores' folders differ: messages name either as "x".
            let same = |t: Result<String, String>| t.map_err(|e| e.replace(&format!("{}", dir.join("a").display()), "x").replace(&format!("{}", dir.join("b").display()), "x"));
            let saved_oracle = same(result_text(oracle_store.save(&s), |_| String::new()));
            let saved_core = same(result_text(core_store.save(&to_core(&s)), |_| String::new()));
            tally.check("save", seed, saved_oracle, saved_core);
        }
        tally.check("file", seed, files_of(&a), files_of(&b));
        tally.check("load", seed, result_text(oracle_store.load(), show), result_text(core_store.load::<EditState>(), show_core));
        // Each reads the other's file.
        let crossed_oracle = oracle::SessionStore { file: b.clone() };
        let crossed_core = core::SessionStore { file: a.clone() };
        tally.check("load other's", seed, result_text(crossed_oracle.load(), show), result_text(crossed_core.load::<EditState>(), show_core));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    tally.finish("session stores", n);
    leaks.check("session stores");
}

#[test]
fn preferences_files_and_catalog_cache_alike() {
    let _serial = serial();
    let leaks = Leaks::start();
    let dir = scratch("profile");
    set_env("NEGS_PROFILE_DIR", Some(dir.to_str().unwrap()));
    let settings = dir.join("settings.json");
    let catalog = dir.join("Cache").join("catalog.json");
    let helper = dir.join("helper");
    std::fs::write(&helper, b"helper").unwrap();
    let data = dir.join("data");
    let mut tally = Tally::default();
    let n = cases() / 4;
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        // Load: the same file, read by each.
        match r.below(4) {
            0 => {
                let _ = std::fs::remove_file(&settings);
            }
            _ => std::fs::write(&settings, preferences_text(&mut r)).unwrap(),
        }
        tally.check(
            "load",
            seed,
            serde_json::to_vec_pretty(&oracle::Preferences::load()).unwrap(),
            serde_json::to_vec_pretty(&CorePrefs::load()).unwrap(),
        );
        // Save: each in turn, the bytes compared.
        let p = preferences(&mut r);
        let _ = std::fs::remove_file(&settings);
        p.save();
        let saved_oracle = files_of(&settings);
        let _ = std::fs::remove_file(&settings);
        // The same preferences as the wrapper's struct (the doubles set
        // after, as a NaN doesn't cross serde).
        let mut v = serde_json::to_value(&p).unwrap();
        for key in ["gridSize", "inspectorWidth", "scopeHeight"] {
            v[key] = json!(0.0);
        }
        let mut c: CorePrefs = serde_json::from_value(v).unwrap();
        (c.grid_size, c.inspector_width, c.scope_height) = (p.grid_size, p.inspector_width, p.scope_height);
        c.save();
        tally.check("save", seed, saved_oracle, files_of(&settings));
        // The catalog cache.
        let entry = json!({"protocol": ([json!(1), json!(1.0), json!(2), Value::Null][r.index(4)].clone()), "films": value(&mut r, 3)});
        let other = if r.chance(4) { dir.join("other") } else { data.clone() };
        let _ = std::fs::remove_file(&catalog);
        oracle::CatalogCache::save(&entry, &helper, &data);
        let saved_oracle = files_of(&catalog);
        let _ = std::fs::remove_file(&catalog);
        core::CatalogCache::save(&entry, &helper, &data);
        tally.check("catalog save", seed, saved_oracle, files_of(&catalog));
        if r.chance(5) {
            std::fs::write(&catalog, mutate(&mut r, &std::fs::read(&catalog).unwrap())).unwrap();
        }
        tally.check("catalog load", seed, oracle::CatalogCache::load(&helper, &other), core::CatalogCache::load(&helper, &other));
        let missing = dir.join("no-helper");
        tally.check("catalog no helper", seed, oracle::CatalogCache::load(&missing, &data), core::CatalogCache::load(&missing, &data));
    }
    set_env("NEGS_PROFILE_DIR", None);
    std::fs::remove_dir_all(&dir).unwrap();
    tally.finish("session preferences and catalog files", n);
    leaks.check("session preferences and catalog files");
}

#[test]
fn folders_writes_lists_and_tabs_alike() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let saved: Vec<Option<String>> = ["NEGS_PROFILE_DIR", "APPDATA", "LOCALAPPDATA"].iter().map(|k| std::env::var(k).ok()).collect();
    let n = cases();
    let dir = scratch("folders");
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        if seed % 8 == 0 {
            for key in ["NEGS_PROFILE_DIR", "APPDATA", "LOCALAPPDATA"] {
                let v = match r.below(4) {
                    0 => None,
                    1 => Some(String::new()),
                    2 => Some(format!("/tmp/{}", string(&mut r).replace('\0', ""))),
                    _ => Some(string(&mut r).replace('\0', "")),
                };
                set_env(key, v.as_deref());
            }
            tally.check("settings_dir", seed, oracle::settings_dir(), core::settings_dir());
            tally.check("cache_dir", seed, oracle::cache_dir(), core::cache_dir());
        }
        // write_atomic, in a scratch folder: fine, under a file, onto a folder.
        if seed % 16 == 0 {
            let case = dir.join(format!("{seed}"));
            let path = match r.below(4) {
                0 => case.join("x").join("settings.json"),
                1 => {
                    std::fs::create_dir_all(&case).unwrap();
                    std::fs::write(case.join("file"), b"").unwrap();
                    case.join("file").join("settings.json")
                }
                2 => {
                    std::fs::create_dir_all(case.join("dir.json")).unwrap();
                    std::fs::create_dir_all(case.join("dir.tmp")).unwrap();
                    case.join("dir.json")
                }
                _ => {
                    std::fs::create_dir_all(case.join("into")).unwrap();
                    case.join("into").join("settings.json")
                }
            };
            let bytes = string(&mut r).into_bytes();
            let o = result_text(oracle::write_atomic(&path, &bytes), |_| String::new());
            let o_file = files_of(&path);
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("tmp"));
            let c = result_text(core::write_atomic(&path, &bytes), |_| String::new());
            tally.check("write_atomic", seed, o, c);
            tally.check("write_atomic file", seed, o_file, files_of(&path));
        }
        // The list helpers and the tab.
        let items = ["Enlarger", "Grain", "section:Print", "", "a,b", "Grain2"];
        let mut list: Vec<&str> = (0..r.index(4)).map(|_| items[r.index(items.len())]).collect();
        if r.chance(3) {
            list.sort();
        }
        let mut o = oracle::Preferences { expanded_adjustments: list.join(","), more_adjustments: list.join(","), ..Default::default() };
        let mut c = CorePrefs { expanded_adjustments: list.join(","), more_adjustments: list.join(","), ..Default::default() };
        let item = items[r.index(items.len())];
        let on = r.bool();
        tally.check("expanded", seed, o.expanded(item), {
            let l = &c.expanded_adjustments;
            unsafe { sys::negs_preferences_list_has(l.as_ptr(), l.len(), item.as_ptr(), item.len()) != 0 }
        });
        o.set_expanded(item, on);
        c.set_expanded(item, on);
        o.set_revealed(item, on);
        c.set_revealed(item, on);
        tally.check("set_expanded", seed, o.expanded_adjustments.clone(), c.expanded_adjustments.clone());
        tally.check("set_revealed", seed, o.more_adjustments.clone(), c.more_adjustments.clone());
        let tabs = ["Prep", "Lab", "Post", "Output", "", "lab", "Presets"];
        let tab = tabs[r.index(tabs.len())];
        o.inspector_tab = tab.into();
        c.inspector_tab = tab.into();
        o.inspector_tab_chosen = r.bool();
        c.inspector_tab_chosen = o.inspector_tab_chosen;
        tally.check("tab", seed, o.tab().to_string(), c.tab().to_string());
        tally.check("same_path", seed, oracle::same_path(Path::new(tab), Path::new(item)), core::same_path(Path::new(tab), Path::new(item)));
    }
    for (key, v) in ["NEGS_PROFILE_DIR", "APPDATA", "LOCALAPPDATA"].iter().zip(saved) {
        set_env(key, v.as_deref());
    }
    tally.check("constants", 0, (oracle::DEFAULT_FAVORITE_FILMS, oracle::SCOPE_HEIGHTS), (core::DEFAULT_FAVORITE_FILMS, core::SCOPE_HEIGHTS));
    let _ = std::fs::remove_dir_all(&dir);
    tally.finish("session folders, writes, lists", n);
    leaks.check("session folders, writes, lists");
}

/// The oracle state's DIR and halation paths are the core's (migrate).
#[test]
fn migration_paths_are_the_oracles() {
    let _serial = serial();
    let mut params = Map::new();
    for path in negs_state_oracle::state::DIR_HALATION_STOCK_PATHS {
        params.insert(path.to_string(), json!(1));
    }
    let text = serde_json::to_vec(&json!({"params": {"film_render": {"dir_couplers": {"gamma_samelayer_rgb": 1, "gamma_interlayer_r_to_gb": 1, "gamma_interlayer_g_to_rb": 1, "gamma_interlayer_b_to_rg": 1}, "halation": {"halation_strength": 1, "halation_first_sigma_um": 1, "other": 1}}}})).unwrap();
    let mut out = empty();
    assert_eq!(unsafe { sys::negs_session_migrate_state(text.as_ptr(), text.len(), &mut out) }, 1);
    let migrated: Value = serde_json::from_slice(&take_buf(out)).unwrap();
    let mut expected: Value = serde_json::from_slice(&text).unwrap();
    for path in negs_state_oracle::state::DIR_HALATION_STOCK_PATHS {
        negs_state_oracle::json::set(&mut expected["params"], path, Value::Null);
    }
    assert_eq!(migrated, expected);
    assert_eq!(params.len(), 6);
}
