//! The wrapper against the core directly: the sidecar fixture gate through
//! the C ABI, the helpers the core runs, and nothing left allocated.

use negs_core_library::{core_json, folder, thumbnail, xmp, Config, FolderBin, Item, Json, Library};
use std::path::{Path, PathBuf};

/// The allocation counts are the process's: one test at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

const FIXTURES: [&str; 7] =
    ["edited.negs", "minimal.negs", "newer-defaults.negs", "newer-schema.negs", "rated-reject.negs", "two-variants.negs", "unknown-keys.negs"];

/// Each fixture, read and rewritten by the core, is the Mac's encoding byte
/// for byte, and the Mac's bytes are the core's fixed point.
#[test]
fn fixtures_rewrite_byte_for_byte_through_the_c_abi() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for name in FIXTURES {
        let bytes = std::fs::read(root().join("docs/sidecar-fixtures").join(name)).unwrap();
        let mac = std::fs::read(root().join("crates/negs-library/tests/mac-encoded").join(name)).unwrap();
        assert_eq!(String::from_utf8(core_json::sidecar_rewrite(&bytes).unwrap()).unwrap(), String::from_utf8(mac.clone()).unwrap(), "{name}");
        assert_eq!(core_json::sidecar_rewrite(&mac).unwrap(), mac, "{name}: a fixed point");
        let value = Json::parse(&bytes).unwrap();
        assert_eq!(core_json::compact_sorted(&bytes).unwrap(), value.to_compact_sorted(), "{name}: compact");
    }
    assert!(core_json::sidecar_rewrite(b"{\"a\": ").is_err());
}

#[test]
fn helpers_match_the_rust_ones() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let edits = Json::parse(br#"{"film":"kodak_portra_400","params":{"prep":{"highlights":-40.5}},"url":"a/b"}"#).unwrap();
    for version in [1, 2] {
        assert_eq!(thumbnail::look(&edits, version), oracle::thumbnail::look(&edits, version));
    }
    for (size, variant) in [(Some(1234), Some("ab12")), (None, None)] {
        assert_eq!(
            thumbnail::key("/a/IMG_1.CR3", size, 1700000000.25, variant, "look", "engine"),
            oracle::thumbnail::key("/a/IMG_1.CR3", size, 1700000000.25, variant, "look", "engine")
        );
        assert_eq!(
            thumbnail::loose_key("/a/IMG_1.CR3", size, 1700000000.25, variant, "look"),
            oracle::thumbnail::loose_key("/a/IMG_1.CR3", size, 1700000000.25, variant, "look")
        );
    }
    let names = ["IMG_2.jpg", "IMG_10.jpg", "a", "B", "x 1", "x01", "é", "e", "File (2).jpg", "file.jpg", "Ä", "z"];
    for a in names {
        for b in names {
            assert_eq!(folder::standard_compare(a, b), oracle::folder::standard_compare(a, b), "{a} {b}");
        }
    }
    let png = [137u8, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 0, 73, 68, 65, 84, 1, 2, 3, 4, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130];
    let embedded = xmp::embed(&png, 4).expect("a PNG");
    assert_eq!(Some(embedded.clone()), oracle::xmp::embed(&png, 4));
    assert_eq!(xmp::read_rating(&embedded), Some(4));
    assert_eq!(xmp::embed(b"not a picture", 4), None);
    let jpeg = [0xFFu8, 0xD8, 0xFF, 0xDB, 0, 4, 1, 2, 0xFF, 0xDA, 0, 2, 9, 9, 0xFF, 0xD9];
    assert_eq!(xmp::embed(&jpeg, -1), oracle::xmp::embed(&jpeg, -1));
}

#[test]
fn a_library_frees_everything() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("negs-core-library-{}", std::process::id()));
    let folder = dir.join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("IMG_1.jpg"), b"jpeg").unwrap();
    let before = negs_core_sys::alloc_stats();
    {
        let mut library = Library::new(
            &folder,
            Config { data: root().join("data"), app: "test".into(), bin: Box::new(FolderBin::new(dir.join("bin"))), pairing: true, sidecars: true, id_seed: Some(1) },
        )
        .unwrap();
        let item = Item::photo(folder.join("IMG_1.jpg"));
        library.set_rating(std::slice::from_ref(&item), 3);
        assert_eq!(library.summary(&item).rating, 3);
        assert_eq!(library.undo_name(), Some("Rating"));
        library.undo().unwrap();
        assert_eq!(library.summary(&item).rating, 0);
    }
    let after = negs_core_sys::alloc_stats();
    if after.tracked == 1 {
        assert_eq!(after.live_count, before.live_count, "the library left allocations");
        assert_eq!(after.bad_frees, 0);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
