//! The Windows editor's state against the sidecar's edits: Mac-written
//! fixtures map to the state Windows shows, and nothing is lost or changed
//! on the way back, so opening a photograph and leaving it never writes.

use negs_library::edits::{decode_edits, encode_edits};
use negs_library::sidecar::{self, DefaultsTable, Sidecar, LATEST_DEFAULTS};
use negs_library::{Config, EditDefaults, EditGroup, Filter, FolderBin, Json, Library, Roll};
use negs_windows_library_map::crop::{CropAspect, Rect, Size};
use negs_windows_library_map::library_map::*;
use negs_windows_library_map::state::{self, BrightnessMatch, EditState, Stock};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn data() -> PathBuf {
    root().join("data")
}

fn table(version: i64) -> DefaultsTable {
    DefaultsTable::load(&data(), version).unwrap()
}

/// What the app's catalog gives: the newest table's parameters with lens
/// corrections on (`apply_catalog`), and a few stocks.
fn catalog_defaults() -> Value {
    let mut defaults = to_value(&table(LATEST_DEFAULTS).params);
    negs_windows_library_map::json::set(&mut defaults, state::LENS_CORRECTION, Value::Bool(true));
    defaults
}

fn stocks() -> Vec<Stock> {
    let stock = |id: &str, paper: bool, positive: bool| Stock::new(&json!({"id": id, "name": id, "paper": paper, "positive": positive}));
    vec![
        stock("kodak_gold_200", false, false),
        stock("kodak_portra_400", false, false),
        stock("kodak_trix", false, false),
        stock("fujifilm_provia_100f", false, true),
        stock("kodak_portra_endura", true, false),
        stock("ilford_mgrc_deluxe_grade2", true, false),
    ]
}

fn defaults() -> EditDefaults {
    let base = EditDefaults::for_table(&table(LATEST_DEFAULTS));
    edit_defaults(&base, &catalog_defaults(), &[], &stocks(), (None, None), true)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(root().join("docs/sidecar-fixtures").join(name)).unwrap()
}

/// Each readable variant's edits, restored as opening the photograph does.
fn restored_edits(name: &str) -> Vec<(negs_library::EditState, bool)> {
    let Ok(found) = Sidecar::from_bytes(&fixture(name)) else { return vec![] };
    let Ok(t) = DefaultsTable::load(&data(), found.defaults_version) else { return vec![] };
    found
        .variants
        .iter()
        .filter_map(|v| v.edits.as_ref())
        .filter_map(|e| decode_edits(e, &t).ok())
        .map(|saved| {
            let mut restored = defaults().restored(&saved.state);
            if let Some(m) = &mut restored.brightness_match {
                m.solved = saved.match_solved;
            }
            (restored, saved.match_solved)
        })
        .collect()
}

#[test]
fn a_mac_edited_photograph_shows_its_edits_on_windows() {
    let edits = restored_edits("edited.negs");
    assert_eq!(edits.len(), 1);
    let (lib, solved) = &edits[0];
    assert!(solved);
    let win = from_library(lib, false).unwrap();
    assert_eq!(win.film, "kodak_portra_400");
    assert_eq!(win.paper, "kodak_portra_endura");
    assert_eq!(win.crop.aspect, CropAspect::Freeform);
    assert_eq!(win.crop.rect, Rect::new(0.1, 0.05, 0.8, 0.9));
    assert_eq!(win.crop.custom, Size::new(5.0, 4.0));
    assert!(win.border.enabled);
    assert_eq!(win.border.color.hex(), "#f4efe6");
    assert_eq!(negs_windows_library_map::border::raw_value(win.border.style), "keyline");
    assert_eq!(win.value("io.rotate_cw"), &json!(1));
    assert_eq!(win.value("io.photo_long_side_px"), &json!(4000), "whole numbers stay integers for the engine");
    assert_eq!(win.value("prep.exposure_ev").as_f64(), Some(0.65));
    assert_eq!(win.value("prep.highlights"), &json!(-40));
    assert_eq!(win.value("enlarger.print_exposure").as_f64(), Some(1.2));
    assert_eq!(win.value(state::LENS_CORRECTION), &json!(true));
    let found = win.brightness_match.as_ref().unwrap();
    assert_eq!((found.ev, found.highlight_limited), (Some(0.45), false));
    assert!(
        found.inputs.as_ref().is_some_and(|i| negs_windows_library_map::json::same(i, &state::match_inputs(&win))),
        "a solved match is current for these edits, so opening doesn't solve it again"
    );
    // And back, as stored: the same edits the Mac wrote.
    let t = table(1);
    let back = to_library(&win, None);
    assert_eq!(&back, lib);
    assert_eq!(encode_edits(&back, true, &t), encode_edits(lib, true, &t));
}

#[test]
fn a_stale_match_and_a_roll_survive() {
    let edits = restored_edits("two-variants.negs");
    let (copy, solved) = &edits[1];
    assert!(!solved);
    let win = from_library(copy, false).unwrap();
    assert_eq!(win.film, "kodak_trix");
    let found = win.brightness_match.as_ref().unwrap();
    assert_eq!((found.ev, found.highlight_limited, found.inputs.is_none()), (Some(-0.3), true, true), "stale: solved again on opening");
    assert_eq!(&to_library(&win, None), copy);
    // A Mac's roll isn't shown on Windows but is kept.
    let mut rolled = copy.clone();
    rolled.roll = Some(Roll { id: "r1".into(), magenta: 10.0, yellow: -5.0, paper: None });
    let win = from_library(&rolled, false).unwrap();
    assert_eq!(to_library(&win, rolled.roll.clone()), rolled);
}

#[test]
fn every_fixture_maps_there_and_back_unchanged() {
    for name in ["edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"] {
        for (lib, _) in restored_edits(name) {
            let win = from_library(&lib, true).unwrap();
            assert_eq!(to_library(&win, lib.roll.clone()), lib, "{name}");
        }
    }
    // A new photograph's state too.
    let plain = defaults().plain_state();
    assert_eq!(to_library(&from_library(&plain, false).unwrap(), None), plain);
}

#[test]
fn windows_state_maps_there_and_back_unchanged() {
    let mut win = from_library(&defaults().plain_state(), false).unwrap();
    win.film = "kodak_portra_400".into();
    negs_windows_library_map::json::set(&mut win.params, "prep.highlights", json!(-37.5));
    negs_windows_library_map::json::set(&mut win.params, "io.rotate_cw", json!(3));
    negs_windows_library_map::json::set(&mut win.params, "film_render.grain.rms_granularity", json!([12.5, 13, 12]));
    negs_windows_library_map::json::set(&mut win.params, "prep.shadows", json!(-0.0));
    win.crop.rect = Rect::new(0.125, 0.0, 0.75, 1.0);
    win.crop.aspect = CropAspect::ThreeTwo;
    win.crop.angle = -2.5;
    win.border.enabled = true;
    win.border.width = 0.045;
    win.border.line_color = negs_windows_library_map::border::BorderColor([0x12, 0xab, 0xef]);
    for inputs_current in [true, false] {
        let mut matched = win.clone();
        let inputs = state::match_inputs(&matched);
        matched.brightness_match = Some(BrightnessMatch { ev: Some(0.4), inputs: inputs_current.then_some(inputs), highlight_limited: true });
        let lib = to_library(&matched, None);
        assert_eq!(lib.brightness_match.as_ref().unwrap().solved, inputs_current);
        let back = from_library(&lib, false).unwrap();
        assert_eq!(back, matched);
        assert_eq!(to_library(&back, None), lib);
    }
    // An edit the match depends on makes it stale, so the sidecar says so.
    let mut moved = win.clone();
    moved.brightness_match = Some(BrightnessMatch { ev: Some(0.4), inputs: Some(state::match_inputs(&moved)), highlight_limited: false });
    moved.film = "kodak_gold_200".into();
    assert!(!to_library(&moved, None).brightness_match.unwrap().solved);
    // Prep edits are the user's own: the match stays solved.
    let mut prep = win.clone();
    prep.brightness_match = Some(BrightnessMatch { ev: Some(0.4), inputs: Some(state::match_inputs(&prep)), highlight_limited: false });
    negs_windows_library_map::json::set(&mut prep.params, "prep.highlights", json!(10));
    assert!(to_library(&prep, None).brightness_match.unwrap().solved);
}

#[test]
fn new_photographs_take_the_settings_stocks_or_the_tables() {
    let base = EditDefaults::for_table(&table(LATEST_DEFAULTS));
    let d = edit_defaults(&base, &catalog_defaults(), &[], &stocks(), (Some("fujifilm_provia_100f"), Some("gone_paper")), false);
    assert_eq!((d.new_photo_film.as_str(), d.new_photo_paper.as_str()), ("fujifilm_provia_100f", "kodak_portra_endura"));
    assert_eq!(d.films.get("fujifilm_provia_100f"), Some(&true));
    assert!(!d.match_new_photos);
    let plain = d.plain_state();
    assert_eq!(plain.params.at("io.scan_film"), &Json::Bool(true), "a slide film scans directly");
    let none = edit_defaults(&base, &catalog_defaults(), &[], &[], (None, None), true);
    assert_eq!(none.new_photo_film, base.table_film);
    // Neither the choice nor the table's in the catalog: its first, by id.
    let first_film = stocks().iter().filter(|s| !s.paper).map(|s| s.id.clone()).min().unwrap();
    let first_paper = stocks().iter().filter(|s| s.paper).map(|s| s.id.clone()).min().unwrap();
    assert_eq!(new_photo_stocks((Some("gone"), None), ("gone_too", "gone_paper"), &stocks()), (first_film, first_paper));
    assert_eq!(new_photo_stocks((None, Some("p")), ("f", "q"), &[]), ("f".to_string(), "p".to_string()));
}

#[test]
fn library_preferences_use_the_macs_keys() {
    let mut prefs = LibraryPreferences::default();
    assert_eq!(prefs.copy_groups(), EditGroup::standard());
    assert_eq!(prefs.filter(), Filter::default());
    let mut filter = Filter::default();
    filter.toggle_flag(negs_library::FlagFilter::Pick);
    prefs.set_filter(&filter);
    prefs.set_copy_groups(&[EditGroup::Lab, EditGroup::CropAndRotation].into_iter().collect());
    prefs.advance_after_rating = true;
    let text = serde_json::to_string(&prefs).unwrap();
    for key in [r#""libraryFilter":{"#, r#""copyEditGroups":["lab","cropAndRotation"]"#, r#""advanceAfterRating":true"#] {
        assert!(text.contains(key), "{text}");
    }
    let again: LibraryPreferences = serde_json::from_str(&text).unwrap();
    assert_eq!(again.filter(), filter);
    assert_eq!(again, prefs);
    let old: LibraryPreferences = serde_json::from_str("{}").unwrap();
    assert!(!old.advance_after_rating);
    prefs.set_filter(&Filter::default());
    assert!(prefs.library_filter.is_none());
}

#[test]
fn keys_by_their_character() {
    assert_eq!(library_key('0'), Some(LibraryKey::Rate(0)));
    assert_eq!(library_key('5'), Some(LibraryKey::Rate(5)));
    assert_eq!(library_key('6'), None);
    assert_eq!(library_key('['), Some(LibraryKey::Lower));
    assert_eq!(library_key(']'), Some(LibraryKey::Raise));
    assert_eq!(library_key('P'), Some(LibraryKey::Pick));
    assert_eq!(library_key('x'), Some(LibraryKey::Reject));
    assert_eq!(library_key('u'), Some(LibraryKey::Unflag));
    assert_eq!(library_key('c'), None);
}

#[test]
fn editor_steps_go_and_library_steps_stay() {
    let edit = || Step::Edit(EditState::default());
    let library = || Step::Library { before: None, after: None };
    let mut steps = vec![(edit(), 1), (library(), 2), (edit(), 3), (library(), 4)];
    drop_editor_steps(&mut steps);
    assert_eq!(steps.iter().map(|s| s.1).collect::<Vec<_>>(), [2, 4]);
}

/// Opening each fixture's photograph in a library, showing it on Windows
/// and handing the editor's state back writes nothing.
#[test]
fn opening_and_leaving_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("negs-windows-map-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("photos")).unwrap();
    let names = ["edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"];
    for (i, name) in names.iter().enumerate() {
        let photo = dir.join("photos").join(format!("P{i}.png"));
        std::fs::write(&photo, b"png").unwrap();
        std::fs::write(sidecar::sidecar_path(&photo), fixture(name)).unwrap();
    }
    std::fs::write(dir.join("photos").join("Plain.png"), b"png").unwrap();
    let mut library = Library::new(
        dir.join("photos"),
        Config { data: data(), app: "windows-test".into(), bin: Box::new(FolderBin::new(dir.join("bin"))), pairing: true, sidecars: true, id_seed: Some(3) },
    )
    .unwrap();
    library.set_defaults(defaults());
    let before: Vec<(PathBuf, Option<Vec<u8>>)> = library.files().iter().map(|f| (f.path.clone(), sidecar::read_bytes(&f.path).ok().flatten())).collect();
    for item in library.items() {
        let opened = library.open(&item);
        let win = from_library(&opened.state, false).unwrap();
        let roll = library.state().and_then(|s| s.roll.clone());
        library.edit(to_library(&win, roll));
        assert!(!library.unsaved(), "{}", library.name(&item));
        library.save().unwrap();
    }
    library.close().unwrap();
    for (photo, bytes) in before {
        assert_eq!(sidecar::read_bytes(&photo).ok().flatten(), bytes, "{}", photo.display());
    }
    assert!(!sidecar::sidecar_path(&dir.join("photos").join("Plain.png")).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn notes_read_as_the_macs_status_line() {
    assert_eq!(note_action("Edits Pasted"), "Edits pasted");
    assert_eq!(note_action("★★★"), "★★★");
    let note = negs_library::Note { text: "Edits Pasted".into(), detail: Some("1 not saved".into()), count: 3 };
    assert_eq!(note_detail(&note), "3 photographs · 1 not saved");
    assert_eq!(note_duration(&note).as_millis(), 2500);
}
