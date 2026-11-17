//! The names edited thumbnails are filed under (`PhotoSidecar.look`,
//! `EditedThumbnailStore.key`).

use crate::json::{swift_double_description, Json};
use crate::sha256::digest16;
use std::time::{SystemTime, UNIX_EPOCH};

/// Identifies how stored edits develop: a hash of them and the table
/// they're stored against. The same on every platform for the same file.
pub fn look(edits: &Json, defaults_version: i64) -> String {
    let both = Json::from_pairs([("defaultsVersion", Json::from(defaults_version)), ("edits", edits.clone())]);
    digest16(both.to_compact_sorted().as_bytes())
}

/// Seconds since 1970, as `Date.timeIntervalSince1970`.
pub fn seconds_since_1970(time: SystemTime) -> f64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs_f64(),
        Err(before) => -before.duration().as_secs_f64(),
    }
}

/// The name of an item's thumbnail with stored edits `look`: the file as
/// listed (path, size, modification time), the duplicate, the edits and
/// the engine (`EditedThumbnailStore.key`). `path` is the photograph's own
/// file; `engine` identifies the helper and its data. Thumbnails are a
/// cache per machine, so the path and time are this machine's.
pub fn key(path: &str, size: Option<u64>, modified: f64, variant: Option<&str>, look: &str, engine: &str) -> String {
    let size = size.map(|s| s as i64).unwrap_or(-1);
    let text = format!("1|{path}|{size}|{}|{}|{look}|{engine}", swift_double_description(modified), variant.unwrap_or(""));
    digest16(text.as_bytes())
}

/// The same under any engine: a link to the last one saved, which stands
/// in after an update until the photograph is developed again
/// (`EditedThumbnailStore.looseKey`).
pub fn loose_key(path: &str, size: Option<u64>, modified: f64, variant: Option<&str>, look: &str) -> String {
    let size = size.map(|s| s as i64).unwrap_or(-1);
    let text = format!("loose|1|{path}|{size}|{}|{}|{look}", swift_double_description(modified), variant.unwrap_or(""));
    digest16(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_as_the_mac_gives_them() {
        // Made by the macOS code (CryptoKit, JSONEncoder) on the same inputs.
        let edited = Json::parse(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/sidecar-fixtures/edited.negs")).unwrap()).unwrap();
        assert_eq!(look(edited.at("variants.0.edits"), 1), "985b4de09687919077268fdaea434fc5");
        let unknown = Json::parse(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/sidecar-fixtures/unknown-keys.negs")).unwrap()).unwrap();
        assert_eq!(look(unknown.at("variants.0.edits"), 1), "22852722f050561828671317f713a2dd");
        assert_eq!(key("/Users/x/IMG_0412.CR3", Some(24117248), 1731859200.123456, Some("a1"), "0123abcd", "stamp"), "05cf6e41a8da7e01edd4224ba2e67fba");
        assert_eq!(key("/Users/x/IMG_0412.CR3", Some(24117248), 1731859200.0, None, "0123abcd", "stamp"), "6ef3fce8782ef750c9b799f8fe36ae52");
    }
}
