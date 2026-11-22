//! negs-library's own suites run here (Cargo.toml's [[test]] paths) and read
//! `tests/mac-encoded` beside this crate's manifest. That folder is a copy
//! of crates/negs-library/tests/mac-encoded rather than a symlink, which a
//! Windows checkout writes as a text file: the copy must stay the same.

use std::path::Path;

#[test]
fn mac_encoded_is_the_crates() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = here.join("tests/mac-encoded");
    let theirs = here.join("../../crates/negs-library/tests/mac-encoded");
    let names = |dir: &Path| {
        let mut n: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        n.sort();
        n
    };
    assert_eq!(names(&ours), names(&theirs));
    for name in names(&theirs) {
        assert_eq!(std::fs::read(ours.join(&name)).unwrap(), std::fs::read(theirs.join(&name)).unwrap(), "{name} differs: copy it again");
    }
}
