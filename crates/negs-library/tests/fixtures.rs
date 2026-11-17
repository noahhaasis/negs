//! The sidecar fixtures in `docs/sidecar-fixtures`, which the macOS app is
//! tested against too (`SidecarTests.testFixtures`).

use negs_library::edits::{decode_edits, encode_edits};
use negs_library::sidecar::{self, LATEST_DEFAULTS};
use negs_library::{BrightnessMatch, DefaultsTable, EditState, Flag, Json, Patch, Problem, Sidecar};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(root().join("docs/sidecar-fixtures").join(name)).unwrap()
}

/// What the macOS app's encoder (`PhotoSidecar.encode`) writes for the
/// fixture, made with Foundation on macOS 26.
fn mac_encoded(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mac-encoded").join(name)).unwrap()
}

fn table(version: i64) -> DefaultsTable {
    DefaultsTable::load(&root().join("data"), version).unwrap()
}

const FIXTURES: [&str; 7] =
    ["edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"];

/// Every fixture reads, and writes back as the Mac writes it. The fixtures
/// are written with `": "` where Foundation writes `" : "`; that is the only
/// difference, so their content round-trips exactly.
#[test]
fn every_fixture_reads_and_rewrites_byte_for_byte() {
    let names: Vec<String> = std::fs::read_dir(root().join("docs/sidecar-fixtures"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".negs"))
        .collect();
    assert_eq!(names.len(), FIXTURES.len(), "a new fixture needs its Mac encoding in tests/mac-encoded: {names:?}");
    for name in FIXTURES {
        let bytes = fixture(name);
        let mac = mac_encoded(name);
        let value = Json::parse(&bytes).unwrap();
        assert_eq!(String::from_utf8(value.to_sidecar_bytes()).unwrap(), String::from_utf8(mac.clone()).unwrap(), "{name}");
        let mac_text = String::from_utf8(mac.clone()).unwrap();
        assert_eq!(mac_text.replace("\" : ", "\": "), String::from_utf8(bytes).unwrap(), "{name}: only the colon's spacing differs");
        assert_eq!(Json::parse(&mac).unwrap().to_sidecar_bytes(), mac, "{name}: the Mac's bytes are a fixed point");
    }
}

#[test]
fn fixtures_mean_what_the_mac_reads() {
    let table = table(1);
    let minimal = Sidecar::from_bytes(&fixture("minimal.negs")).unwrap();
    assert_eq!(minimal.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["3f9c2a1b"]);
    assert_eq!((minimal.variants[0].rating, minimal.variants[0].flag), (0, None));
    let plain = decode_edits(minimal.variants[0].edits.as_ref().unwrap(), &table).unwrap();
    assert_eq!(plain.state.film, "kodak_portra_400");
    assert_eq!(plain.state.params, table.params);
    assert_eq!(plain.state.crop, table.crop);
    assert_eq!(plain.state.brightness_match, None);

    let edited = Sidecar::from_bytes(&fixture("edited.negs")).unwrap();
    assert_eq!((edited.variants[0].rating, edited.variants[0].flag), (3, Some(Flag::Pick)));
    let saved = decode_edits(edited.variants[0].edits.as_ref().unwrap(), &table).unwrap().state;
    assert_eq!(saved.params.at("prep.highlights"), &Json::Number(-40.0));
    assert_eq!(saved.params.at("prep.exposure_ev"), &Json::Number(0.65));
    assert_eq!(saved.params.at("prep.shadows"), table.params.at("prep.shadows"));
    assert_eq!(saved.params.at("io.rotate_cw"), &Json::Number(1.0));
    assert_eq!(saved.params.at("io.photo_long_side_px"), &Json::Number(4000.0));
    assert_eq!(saved.crop.get("rect"), &Json::parse(b"[[0.1, 0.05], [0.8, 0.9]]").unwrap());
    assert_eq!(saved.crop.get("aspect"), &Json::string("freeform"));
    assert_eq!(saved.border.get("style"), &Json::string("keyline"));
    assert_eq!(saved.border.get("enabled"), &Json::Bool(true));
    assert_eq!(saved.border.get("color"), &Json::string("#f4efe6"));
    assert_eq!(saved.border.get("width"), table.border.get("width"));
    assert_eq!(saved.brightness_match, Some(BrightnessMatch { ev: Some(0.45), highlight_limited: false, solved: true }));
    // Stored again, the same edits.
    assert_eq!(&encode_edits(&saved, true, &table), edited.variants[0].edits.as_ref().unwrap());

    let reject = Sidecar::from_bytes(&fixture("rated-reject.negs")).unwrap();
    assert_eq!((reject.variants[0].rating, reject.variants[0].flag, reject.variants[0].edits.is_none()), (4, Some(Flag::Reject), true));

    let two = Sidecar::from_bytes(&fixture("two-variants.negs")).unwrap();
    assert_eq!(two.variants.iter().map(|v| (v.id.as_str(), v.rating, v.flag)).collect::<Vec<_>>(), [("a1", 2, None), ("b7", 5, Some(Flag::Pick))]);
    let copy = decode_edits(two.variants[1].edits.as_ref().unwrap(), &table).unwrap();
    assert_eq!(copy.state.film, "kodak_trix");
    assert!(!copy.match_solved);
    assert!(copy.state.brightness_match.unwrap().highlight_limited);

    let unknown = Sidecar::from_bytes(&fixture("unknown-keys.negs")).unwrap();
    let future = decode_edits(unknown.variants[0].edits.as_ref().unwrap(), &table).unwrap().state;
    assert_eq!(future.params.at("prep.highlights"), &Json::Number(-20.0));
    assert_eq!(future.crop.at("rect.1.1"), &Json::Number(0.8));

    assert_eq!(Sidecar::from_bytes(&fixture("newer-schema.negs")), Err(Problem::Newer));
    assert_eq!(Sidecar::from_bytes(&fixture("newer-defaults.negs")), Err(Problem::Newer));
}

/// Writing this version's edits keeps everything it doesn't know, and what
/// another app set meanwhile (`testAWriteKeepsUnknownKeysAndOtherFields`).
#[test]
fn a_write_keeps_unknown_keys_and_other_fields() {
    let table = table(1);
    let mut raw = Sidecar::from_bytes(&fixture("unknown-keys.negs")).unwrap().raw;
    let variant = raw.at("variants.0").overlaid(&Json::parse(br#"{"rating": 4, "flag": "reject"}"#).unwrap());
    raw.set_at("variants", Json::Array(vec![variant]));
    let mut state = EditState::from_table(&table);
    state.params.set_at("prep.shadows", Json::Number(15.0));
    let patch = Patch { edits: Some(Some(encode_edits(&state, false, &table))), known: table.known(None), ..Patch::new("e5", 1, "0.15.0") };
    let written = patch.applied(Some(&raw)).unwrap().unwrap();
    assert_eq!(written.at("sync.device"), &Json::string("Studio iMac"));
    assert_eq!(written.get("app"), &Json::string("0.15.0"));
    let variant = written.at("variants.0");
    assert_eq!(variant.get("label"), &Json::string("red"));
    assert_eq!(variant.get("tags"), &Json::parse(br#"["street", "night"]"#).unwrap());
    assert_eq!(variant.get("rating"), &Json::Number(4.0));
    assert_eq!(variant.get("flag"), &Json::string("reject"));
    let edits = variant.get("edits");
    assert_eq!(edits.at("futureGroup.amount"), &Json::Number(1.0));
    assert_eq!(edits.at("params.future_stage.strength"), &Json::Number(2.0));
    assert_eq!(edits.at("params.prep.future_knob"), &Json::Number(3.0));
    assert_eq!(edits.at("crop.futureField"), &Json::Bool(true));
    assert_eq!(edits.at("params.prep.highlights"), &Json::Null);
    assert_eq!(edits.at("params.prep.shadows"), &Json::Number(15.0));
    assert_eq!(edits.at("crop.rect"), &Json::Null);
    assert_eq!(edits.at("film"), &Json::string("kodak_gold_200"));
    let mut newer = raw.clone();
    newer.set_at("negs", Json::Number(2.0));
    assert!(patch.applied(Some(&newer)).is_err());
}

/// Each defaults table up to the newest reads; a newer one in the data
/// folder means `LATEST_DEFAULTS` must move with the Mac's
/// `SidecarDefaults.latest`.
#[test]
fn the_defaults_tables_are_there_up_to_the_newest() {
    for version in 1..=LATEST_DEFAULTS {
        let table = table(version);
        assert_eq!(table.version, version);
        assert_eq!(EditState::from_table(&table).params, table.params);
    }
    assert!(!DefaultsTable::path(&root().join("data"), LATEST_DEFAULTS + 1).exists(), "a newer table: update LATEST_DEFAULTS");
    let mac = std::fs::read_to_string(root().join("macos/Sources/NegsApp/Sidecar.swift")).unwrap();
    assert!(mac.contains(&format!("static let latest = {LATEST_DEFAULTS}\n")), "the Mac's SidecarDefaults.latest moved");
}

#[test]
fn a_sidecar_file_round_trips_through_the_disk() {
    let dir = std::env::temp_dir().join(format!("negs-library-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let photo = dir.join("IMG_0412.CR3");
    std::fs::write(&photo, b"raw").unwrap();
    assert_eq!(sidecar::read_raw(&photo).unwrap(), None);
    let raw = Json::parse(&fixture("edited.negs")).unwrap();
    sidecar::write_raw(&photo, Some(&raw)).unwrap();
    assert_eq!(std::fs::read(sidecar::sidecar_path(&photo)).unwrap(), mac_encoded("edited.negs"));
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["IMG_0412.CR3", "IMG_0412.CR3.negs"], "no temporary file left behind");
    assert_eq!(sidecar::read_raw(&photo).unwrap(), Some(raw));
    sidecar::write_raw(&photo, None).unwrap();
    assert!(!sidecar::sidecar_path(&photo).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
