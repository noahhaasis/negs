//! presets.rs imports `std::os::windows::fs::MetadataExt`, which no crate
//! can stand in for. This writes a copy into OUT_DIR with that one import
//! pointed at `crate::stand_ins::MetadataExt`, and its inner doc comments
//! (`//!`) as plain ones so `include!` takes it. Nothing else changes.

use std::path::Path;

fn main() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/presets.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = std::fs::read_to_string(&source).expect("core/diff/oracle/presets.rs");
    const IMPORT: &str = "use std::os::windows::fs::MetadataExt;";
    assert_eq!(text.matches(IMPORT).count(), 1, "presets.rs's MetadataExt import moved");
    let text = text.replace(IMPORT, "use crate::stand_ins::MetadataExt;");
    let text: String = text
        .lines()
        .map(|l| if let Some(rest) = l.strip_prefix("//!") { format!("//{rest}\n") } else { format!("{l}\n") })
        .collect();
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("presets.rs");
    std::fs::write(out, text).unwrap();
}
