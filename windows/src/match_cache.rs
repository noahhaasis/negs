//! Solved Match camera brightness values, kept across photographs and
//! launches (ticket 39, the Mac's `MatchCache.swift`): a photograph without
//! edits has no sidecar, so going A → B → A solved A's match again. Each
//! value is named by everything its solve read: the file as it is on disk
//! (path, size, modification time), the exact request the helper solved
//! (film, paper and every parameter, which include the match inputs), and
//! the engine (app version, helper binary, data folder). A hit is what
//! solving again would give. `BrightnessMatches.json` in the cache folder,
//! never beside the photographs; the least recently used go past `LIMIT`.
//! Read on first use, on the thread that solves, never on the launch path.
//!
//! The shared core's (core/src/caches/match_cache, through
//! negs_core_rules): the key, the engine stamp and the entries run in Odin
//! (core/diff/tests/match_cache.rs); the cache and its delayed write are
//! negs_core_rules::match_cache::MatchCache. The shared instance stays here.

use std::sync::{Arc, OnceLock};

use crate::engine::EnginePaths;
use crate::state::EditState;

pub use negs_core_rules::match_cache::{LIMIT, MatchCache};

/// The app's cache.
pub fn shared() -> &'static Arc<MatchCache> {
    static SHARED: OnceLock<Arc<MatchCache>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(MatchCache::new(crate::session::cache_dir().join("BrightnessMatches.json"), LIMIT)))
}

/// The name of a solve of `request` on `file` in `engine`
/// (`engine_stamp`), or None if the file can't be read (nothing is then
/// looked up or kept).
pub fn key(file: &std::path::Path, request: &EditState, engine: &str) -> Option<String> {
    negs_core_rules::match_cache::key(file, &request.film, &request.paper, &request.params, engine)
}

/// The engine a solve ran in: the app's version and the helper binary as it
/// is on disk, so a new build or release solves afresh. Its data ships with it.
pub fn engine_stamp(paths: &EnginePaths) -> String {
    negs_core_rules::match_cache::engine_stamp(env!("CARGO_PKG_VERSION"), &paths.helper, &paths.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("negs-match-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The key changes with the file, every parameter of the request and
    /// the engine, and nothing else.
    #[test]
    fn the_key_names_the_file_the_request_and_the_engine() {
        let dir = folder("key");
        let photo = dir.join("a.nef");
        std::fs::write(&photo, b"raw").unwrap();
        let request = EditState { film: "kodak_portra_400".into(), paper: "kodak_endura".into(), params: serde_json::json!({"camera": {"exposure_compensation_ev": 0.0}}), ..Default::default() };
        let base = key(&photo, &request, "engine 1").unwrap();
        assert_eq!(key(&photo, &request.clone(), "engine 1").unwrap(), base);
        assert_eq!(base.len(), 64);
        let mut film = request.clone();
        film.film = "kodak_gold_200".into();
        let mut param = request.clone();
        param.params["camera"]["exposure_compensation_ev"] = serde_json::json!(0.5);
        for other in [key(&photo, &film, "engine 1"), key(&photo, &param, "engine 1"), key(&photo, &request, "engine 2")] {
            assert_ne!(other.unwrap(), base);
        }
        std::fs::write(&photo, b"raw, rewritten").unwrap();
        assert_ne!(key(&photo, &request, "engine 1").unwrap(), base);
        assert!(key(&dir.join("missing.nef"), &request, "engine 1").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Solves survive a relaunch; past the limit the least recently used go.
    #[test]
    fn solves_are_kept_on_disk_least_recently_used_out() {
        let dir = folder("lru");
        let file = dir.join("BrightnessMatches.json");
        let cache = Arc::new(MatchCache::new(file.clone(), 10));
        for i in 0..10 {
            cache.store(format!("k{i}"), i as f64 / 10.0, i % 2 == 0);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(cache.solved("k0"), Some((0.0, true)));
        cache.store("k10".into(), 1.0, false);
        // 11 > 10: the oldest two go (one plus a tenth), k0 was just used.
        assert_eq!(cache.count(), 9);
        assert!(cache.solved("k1").is_none() && cache.solved("k2").is_none());
        cache.flush();
        let again = MatchCache::new(file, 10);
        assert_eq!(again.solved("k0"), Some((0.0, true)));
        assert_eq!(again.solved("k10"), Some((1.0, false)));
        assert_eq!(again.count(), 9);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
