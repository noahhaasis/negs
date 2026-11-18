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
    let back = to_library(&win);
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
    assert_eq!(&to_library(&win), copy);
    // A roll's frame keeps its roll both ways.
    let mut rolled = copy.clone();
    rolled.roll = Some(Roll { id: "r1".into(), magenta: 10.0, yellow: -5.0, paper: Some("kodak_portra_endura".into()) });
    let win = from_library(&rolled, false).unwrap();
    assert_eq!(win.roll.as_ref().map(|r| (r.id.as_str(), r.magenta, r.yellow)), Some(("r1", 10.0, -5.0)));
    assert_eq!(to_library(&win), rolled);
}

#[test]
fn every_fixture_maps_there_and_back_unchanged() {
    for name in ["edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"] {
        for (lib, _) in restored_edits(name) {
            let win = from_library(&lib, true).unwrap();
            assert_eq!(to_library(&win), lib, "{name}");
        }
    }
    // A new photograph's state too.
    let plain = defaults().plain_state();
    assert_eq!(to_library(&from_library(&plain, false).unwrap()), plain);
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
        let lib = to_library(&matched);
        assert_eq!(lib.brightness_match.as_ref().unwrap().solved, inputs_current);
        let back = from_library(&lib, false).unwrap();
        assert_eq!(back, matched);
        assert_eq!(to_library(&back), lib);
    }
    // An edit the match depends on makes it stale, so the sidecar says so.
    let mut moved = win.clone();
    moved.brightness_match = Some(BrightnessMatch { ev: Some(0.4), inputs: Some(state::match_inputs(&moved)), highlight_limited: false });
    moved.film = "kodak_gold_200".into();
    assert!(!to_library(&moved).brightness_match.unwrap().solved);
    // Prep edits are the user's own: the match stays solved.
    let mut prep = win.clone();
    prep.brightness_match = Some(BrightnessMatch { ev: Some(0.4), inputs: Some(state::match_inputs(&prep)), highlight_limited: false });
    negs_windows_library_map::json::set(&mut prep.params, "prep.highlights", json!(10));
    assert!(to_library(&prep).brightness_match.unwrap().solved);
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
/// The catalog arrives as the helper's JSON text. Read back exactly (serde_json's
/// `float_roundtrip`), its defaults are the table's to the last bit, so a new
/// photograph's first edit stores that edit only: read one ulp off, every
/// edited photograph also stored the glare and scanner levels.
#[test]
fn defaults_read_from_the_catalogs_text_are_the_tables() {
    let t = table(LATEST_DEFAULTS);
    let text = serde_json::to_string(&catalog_defaults()).unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    let mut catalog = to_json(&parsed);
    let mut lens = to_json(&catalog_defaults());
    assert_eq!(catalog, lens);
    // Without the lens corrections `apply_catalog` turns on: the table itself.
    for json in [&mut catalog, &mut lens] {
        if let Json::Object(top) = json {
            let table_lens = t.params.get("io").get("lens_correction").clone();
            if let Some(Json::Object(io)) = top.get_mut("io") {
                io.insert("lens_correction".into(), table_lens);
            }
        }
    }
    assert_eq!(catalog.changes_from(&t.params), None);

    let base = EditDefaults::for_table(&t);
    let defaults = edit_defaults(&base, &parsed, &[], &stocks(), (None, None), true);
    let mut fresh = negs_library::EditState::from_table(&t);
    fresh.params = defaults.params.clone();
    let mut win = from_library(&fresh, false).unwrap();
    negs_windows_library_map::json::set(&mut win.params, "io.rotate_cw", serde_json::json!(1));
    let stored = encode_edits(&to_library(&win), false, &t);
    let params = stored.get("params");
    let Json::Object(groups) = params else { panic!("no params: {stored:?}") };
    let changed: Vec<&String> = groups.keys().collect();
    assert!(changed.iter().all(|g| *g == "io"), "{changed:?}");
    assert_eq!(params.get("io").get("rotate_cw"), &Json::Number(1.0));
}

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
        library.edit(to_library(&win));
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

/// Flip (`io.flip`) goes to the sidecar as the Mac writes it and comes back
/// on reopening; the unflipped default isn't stored as an edit.
#[test]
fn a_flip_goes_through_the_sidecar_and_back() {
    let dir = std::env::temp_dir().join(format!("negs-windows-flip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("photos")).unwrap();
    let photo = dir.join("photos").join("F.png");
    std::fs::write(&photo, b"png").unwrap();
    let config = || Config { data: data(), app: "windows-test".into(), bin: Box::new(FolderBin::new(dir.join("bin"))), pairing: true, sidecars: true, id_seed: Some(5) };
    let mut library = Library::new(dir.join("photos"), config()).unwrap();
    library.set_defaults(defaults());
    let item = library.items()[0].clone();
    let opened = library.open(&item);
    let mut win = from_library(&opened.state, false).unwrap();
    assert!(!win.flip(), "a new photograph isn't flipped");
    // As Flip does from a quarter turn: mirrored as shown, the turns conjugated.
    let flipped = win.orientation().flipped_as_shown();
    negs_windows_library_map::json::set(&mut win.params, state::FLIP, json!(flipped.flip));
    negs_windows_library_map::json::set(&mut win.params, "io.rotate_cw", json!(flipped.turns));
    library.edit(to_library(&win));
    library.save().unwrap();
    library.close().unwrap();
    let text = String::from_utf8(sidecar::read_bytes(&photo).unwrap().unwrap()).unwrap();
    assert!(text.contains("\"flip\""), "{text}");
    let mut library = Library::new(dir.join("photos"), config()).unwrap();
    library.set_defaults(defaults());
    let item = library.items()[0].clone();
    let back = from_library(&library.open(&item).state, false).unwrap();
    assert!(back.flip());
    assert_eq!(back, win);
    assert!(back.rendered() == from_library(&opened.state, false).unwrap().rendered(), "a flip doesn't need a new develop");
    library.close().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Enlarger › Shifts move (`FilterLinkTests` on the Mac): the other shift
/// follows by the same change, so nothing jumps on switching, and both stop
/// at either end. The preference uses the Mac's key and words.
#[test]
fn linked_shifts_move_as_the_macs() {
    use state::FilterLink;
    let (m, y) = (FilterLink::MAGENTA, FilterLink::YELLOW);
    assert_eq!(FilterLink::Separate.shifts(m, 15.0, 10.0, -5.0), (15.0, -5.0));
    assert_eq!(FilterLink::Together.shifts(m, 15.0, 10.0, -5.0), (15.0, 0.0));
    assert_eq!(FilterLink::Together.shifts(y, -8.0, 10.0, -5.0), (7.0, -8.0));
    assert_eq!(FilterLink::Opposite.shifts(m, 15.0, 10.0, -5.0), (15.0, -10.0));
    // Yellow reaches 50 after 10 of the 30 asked.
    assert_eq!(FilterLink::Together.shifts(m, 30.0, 0.0, 40.0), (10.0, 50.0));
    assert_eq!(FilterLink::Opposite.shifts(y, 30.0, -45.0, 0.0), (-50.0, 5.0));
    // Moving the other way is never held back.
    assert_eq!(FilterLink::Together.shifts(m, -10.0, 0.0, 50.0), (-10.0, 40.0));
    // Balance Print writes unrounded shifts; the follower isn't snapped.
    let (a, b) = FilterLink::Together.shifts(m, 4.0, 3.4567, -1.2345);
    assert!((a - b - (3.4567 + 1.2345)).abs() < 1e-9);
    for link in FilterLink::ALL {
        assert_eq!(FilterLink::from_key(link.key()), link);
    }
    assert_eq!(FilterLink::ALL.map(FilterLink::title), ["Separately", "Together", "Opposite ways"]);
    assert_eq!(FilterLink::from_key(""), FilterLink::Separate);
}

/// Color Filter's hint, as `ColorFilterPicker.hint` words it: what the
/// glass does, then the stops it takes on this film in the locale's decimals.
#[test]
fn colour_filter_hints_read_as_the_macs() {
    use state::ColorFilter;
    let yellow = ColorFilter::new(&json!({"key": "hoya_y2", "name": "Yellow", "glass": "Hoya Y2", "colour": "#f2c84b"}));
    assert_eq!(yellow.colour, 0xf2c84b);
    assert_eq!(ColorFilter::hint(None, None, "Tri-X", false, '.'), "No glass in front of the lens.");
    assert_eq!(
        ColorFilter::hint(Some(&yellow), Some(1.34), "Ilford HP5 Plus", true, ','),
        "Darkens blue sky a touch so clouds stand out. The everyday filter. Takes 1,3 stops on Ilford HP5 Plus; auto exposure gives it."
    );
    assert_eq!(
        ColorFilter::hint(Some(&yellow), Some(1.02), "Tri-X", false, '.'),
        "Darkens blue sky a touch so clouds stand out. The everyday filter. Takes 1 stop on Tri-X."
    );
    assert_eq!(ColorFilter::hint(Some(&yellow), None, "Portra", false, '.'), ColorFilter::effect("hoya_y2"));
    assert_eq!(ColorFilter::help(Some(&yellow)), "Yellow · Hoya Y2. Darkens blue sky a touch so clouds stand out. The everyday filter.");
}

/// Your negative's rules (`NegativeScan`): what entering and leaving set,
/// what a preset keeps, what finds the base again, the crop to the picture
/// and Prep's sentence about the base.
#[test]
fn a_negative_reads_and_leaves_as_the_macs() {
    use negs_windows_library_map::crop::Orientation;
    use negs_windows_library_map::negative::{self as neg, BaseStatus};
    let defaults = json!({"workflow": {"route": "input > film > print > scan"}, "film_render": {"base": {"scale": 1.0}, "convert": {"scan_illuminant": "D55"}},
        "enlarger": {"m_filter_shift": 0.0, "y_filter_shift": 0.0, "print_exposure": 1.0}});
    let mut negative = EditState::default();
    negative.film = "kodak_portra_400".into();
    negative.params = json!({"workflow": {"route": neg::ROUTE}, "film_render": {"base": {"scale": 1.3}, "convert": {"scan_illuminant": "D50"}},
        "enlarger": {"m_filter_shift": 4.4, "y_filter_shift": -1.8, "print_exposure": 0.87}, "io": {"raw_white_balance": {"mode": "daylight"}}});
    assert!(neg::is_negative(&negative));
    let scene = neg::leaving(&negative, &defaults);
    assert!(!neg::is_negative(&scene));
    assert_eq!(scene.value("enlarger.print_exposure"), &json!(1.0));
    assert_eq!(scene.value("film_render.convert.scan_illuminant"), &json!("D55"));
    assert_eq!(neg::key(&scene), None);

    // A preset brings the look and the paper, never how it's read or its balance.
    let mut applied = scene.clone();
    applied.film = "fuji_velvia".into();
    applied.paper = "endura".into();
    negs_windows_library_map::json::set(&mut applied.params, "film_render.grain.amount", json!(0.5));
    let kept = neg::keeping_negative(&applied, &negative);
    assert_eq!((kept.film.as_str(), kept.paper.as_str()), ("kodak_portra_400", "endura"));
    assert_eq!(kept.value("enlarger.m_filter_shift"), &json!(4.4));
    assert_eq!(kept.value("film_render.grain.amount"), &json!(0.5));
    assert_eq!(kept.value("io.scan_film"), &json!(false));
    assert_eq!(neg::keeping_negative(&applied, &scene), applied, "a scene takes the preset whole");

    // The base is found again for a new film, backlight or white balance; not a paper.
    let mut other = negative.clone();
    other.paper = "endura".into();
    assert_eq!(neg::key(&other), neg::key(&negative));
    negs_windows_library_map::json::set(&mut other.params, neg::SCAN_ILLUMINANT, json!("D65"));
    assert_ne!(neg::key(&other), neg::key(&negative));
    assert!(neg::is_found("enlarger.print_exposure") && neg::is_found("enlarger") && neg::is_found("film_render.base.scale"));
    assert!(!neg::is_found("film_render.grain.amount"));

    let picture = json!({"x0": 0.05, "y0": 0.1, "x1": 0.95, "y1": 0.9, "angle_deg": 50.0});
    let crop = neg::crop(&picture).expect("a picture");
    assert!((crop.rect.width - 0.9).abs() < 1e-12 && crop.angle == 45.0, "levelled at most as far as Straighten goes");
    assert_eq!(neg::bounds(&picture), Some(json!([0.05, 0.1, 0.95, 0.9])));
    assert_eq!(neg::crop(&json!({"x0": 0.5, "y0": 0.1, "x1": 0.4, "y1": 0.9})), None);

    let found = BaseStatus::of(&json!({"found": true, "picked": false, "edge": "left"}));
    assert_eq!(found.sentence(Orientation::default()), "Film base found along the left edge.");
    // As shown: a quarter turn clockwise puts the left edge at the top; a flip on the right.
    assert_eq!(found.sentence(Orientation::new(1, false)), "Film base found along the top edge.");
    assert_eq!(found.sentence(Orientation::new(0, true)), "Film base found along the right edge.");
    let guess = BaseStatus::of(&json!({"found": false, "picked": false, "edge": null}));
    assert!(guess.guessed());
    assert_eq!(guess.sentence(Orientation::default()), "No clear film found: the base is a guess from the thinnest part of the picture.");
    assert!(neg::same_base(&json!({"scale": 1.0}), &json!({"scale": 1.0 + 1e-12})));
}

/// Prep's Flat field row (`NegativeScan.FlatField`): the light frame's name
/// or Off, and why the helper didn't use it, only for that light frame.
#[test]
fn the_flat_field_row_reads_as_the_macs() {
    use negs_windows_library_map::negative::FlatField;
    let off = FlatField::of("", &json!({"path": "", "used": false}));
    assert_eq!((off.is_on(), off.value(), off.hint()), (false, "Off".to_string(), None));
    let light = r"C:\Scans\light.tif";
    let used = FlatField::of(light, &json!({"path": light, "used": true}));
    assert_eq!((used.value(), used.hint()), ("light.tif".to_string(), None));
    let refused = FlatField::of(light, &json!({"path": light, "used": false, "reason": "The light frame is another size."}));
    assert_eq!(refused.hint().as_deref(), Some("The light frame is another size."));
    let silent = FlatField::of("/scans/light.tif", &json!({"path": "/scans/light.tif", "used": false}));
    assert_eq!(silent.hint().as_deref(), Some("The light frame isn't used on this frame."));
    // An answer about another light frame says nothing yet.
    let other = FlatField::of(light, &json!({"path": "/other.tif", "used": false}));
    assert_eq!((other.used, other.hint()), (None, None));
}

/// A darkroom roll's rules (`NegativeScan.joining`, `scanStops`,
/// `NegativeRoll`): a frame joining takes the reference's film, paper,
/// backlight and base tuning and the roll's filtration, keeping its own
/// filtration only if it set one in this roll; and a frame without clear
/// film shifts the roll's exposure by the camera's settings.
#[test]
fn a_roll_joins_as_the_macs() {
    use negs_windows_library_map::negative as neg;
    use negs_windows_library_map::state::Roll as WinRoll;
    let mut reference = EditState::default();
    reference.film = "kodak_portra_400".into();
    reference.paper = "fuji_crystal_archive".into();
    reference.params = json!({"workflow": {"route": neg::ROUTE}, "film_render": {"base": {"scale": 1.3}, "convert": {"scan_illuminant": "D50", "exposure_compensation_ev": 0.4}},
        "enlarger": {"m_filter_shift": 4.0, "y_filter_shift": -2.0, "print_exposure": 0.9}, "io": {"raw_white_balance": {"mode": "daylight"}}});
    let roll = WinRoll { id: "r".into(), magenta: 4.0, yellow: -2.0, paper: Some(reference.paper.clone()) };
    let mut target = EditState::default();
    target.params = json!({"enlarger": {"m_filter_shift": 9.0, "y_filter_shift": 9.0, "print_exposure": 1.4}, "post": {"vibrance": 0.2}});
    let joined = neg::joining(&roll, &reference, &target);
    assert_eq!((joined.film.as_str(), joined.paper.as_str()), ("kodak_portra_400", "fuji_crystal_archive"));
    assert_eq!(joined.value("film_render.convert.scan_illuminant"), &json!("D50"));
    assert_eq!(joined.value("io.raw_white_balance.mode"), &json!("daylight"));
    assert_eq!((joined.value("enlarger.m_filter_shift"), joined.value("enlarger.y_filter_shift")), (&json!(4.0), &json!(-2.0)));
    assert_eq!(joined.value("enlarger.print_exposure"), &json!(1.4), "the exposure is the frame's own");
    assert_eq!(joined.value("post.vibrance"), &json!(0.2));
    assert_eq!(joined.roll.as_ref(), Some(&roll));
    assert!(roll.filtered(&joined));
    // A frame of this roll that set its own filtration keeps it; of another roll, not.
    let mut own = joined.clone();
    negs_windows_library_map::json::set(&mut own.params, "enlarger.m_filter_shift", json!(6.0));
    assert!(!roll.filtered(&own));
    assert_eq!(neg::joining(&roll, &reference, &own).value("enlarger.m_filter_shift"), &json!(6.0));
    own.roll = Some(WinRoll { id: "other".into(), ..roll.clone() });
    assert_eq!(neg::joining(&roll, &reference, &own).value("enlarger.m_filter_shift"), &json!(4.0));
    // Why a frame has its own filtration.
    assert_eq!(roll.own_filtration(&joined), "This frame has its own filtration.");
    let mut other_paper = joined.clone();
    other_paper.paper = "kodak_endura".into();
    assert_eq!(roll.own_filtration(&other_paper), "On another paper than its roll, so it has its own filtration.");
    // Leaving the negative leaves the roll; a preset keeps it.
    let defaults = json!({"workflow": {"route": "input > film > print > scan"}});
    assert_eq!(neg::leaving(&joined, &defaults).roll, None);
    assert_eq!(neg::keeping_negative(&EditState::default(), &joined).roll.as_ref(), Some(&roll));
    // A stop more shutter, a stop less aperture and twice the ISO: +1 −1 +1.
    assert_eq!(neg::scan_stops((Some(1.0 / 60.0), Some(5.6), Some(200.0)), (Some(1.0 / 125.0), Some(4.0), Some(100.0))).map(|s| (s * 100.0).round() / 100.0), Some(1.09));
    assert_eq!(neg::scan_stops((None, None, Some(100.0)), (Some(0.01), None, None)), None);
    assert_eq!(neg::exposure_ev(0.4, Some(1.0)), 0.4 - 1.0);
    assert_eq!(neg::exposure_ev(0.4, None), 0.4);
    assert_eq!(neg::skipped(1), "1 photograph skipped: it doesn't look like a negative.");
    assert_eq!(neg::skipped(3), "3 photographs skipped: they don't look like negatives.");
}
