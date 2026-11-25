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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::engine::EnginePaths;
use crate::state::EditState;

pub const LIMIT: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Solved {
    ev: f64,
    limited: bool,
    /// When it was last solved or used, for the LRU.
    used: f64,
}

#[derive(Default)]
struct Entries {
    /// None until read from disk.
    entries: Option<HashMap<String, Solved>>,
    dirty: bool,
    write_scheduled: bool,
}

pub struct MatchCache {
    file: PathBuf,
    limit: usize,
    state: Mutex<Entries>,
}

/// The app's cache.
pub fn shared() -> &'static Arc<MatchCache> {
    static SHARED: OnceLock<Arc<MatchCache>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(MatchCache::new(crate::session::cache_dir().join("BrightnessMatches.json"), LIMIT)))
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn stamp(path: &Path) -> Option<(u64, f64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    Some((metadata.len(), modified))
}

/// The name of a solve of `request` on `file` in `engine`
/// (`engine_stamp`), or None if the file can't be read (nothing is then
/// looked up or kept).
pub fn key(file: &Path, request: &EditState, engine: &str) -> Option<String> {
    let (size, modified) = stamp(file)?;
    let solved = serde_json::json!({"film": request.film, "paper": request.paper, "params": request.params});
    let text = format!("1|{}|{size}|{modified}|{engine}|{}", file.display(), serde_json::to_string(&solved).ok()?);
    Some(negs_library::sha256::sha256(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect())
}

/// The engine a solve ran in: the app's version and the helper binary as it
/// is on disk, so a new build or release solves afresh. Its data ships with it.
pub fn engine_stamp(paths: &EnginePaths) -> String {
    let (size, modified) = stamp(&paths.helper).map_or((u64::MAX, 0.0), |s| s);
    format!("{}|{}|{size}|{modified}|{}", env!("CARGO_PKG_VERSION"), paths.helper.display(), paths.data.display())
}

impl MatchCache {
    pub fn new(file: PathBuf, limit: usize) -> Self {
        Self { file, limit, state: Mutex::new(Entries::default()) }
    }

    fn load(&self, state: &mut Entries) {
        if state.entries.is_none() {
            state.entries =
                Some(std::fs::read(&self.file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default());
        }
    }

    /// The solve kept for `key`, marked as used.
    pub fn solved(&self, key: &str) -> Option<(f64, bool)> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.load(&mut state);
        let entry = state.entries.as_mut()?.get_mut(key)?;
        entry.used = now();
        Some((entry.ev, entry.limited))
    }

    /// Keeps a solve made on a whole decode; written a second later.
    pub fn store(self: &Arc<Self>, key: String, ev: f64, limited: bool) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.load(&mut state);
        let entries = state.entries.get_or_insert_with(HashMap::new);
        entries.insert(key, Solved { ev, limited, used: now() });
        if entries.len() > self.limit {
            // A tenth more than needed, so it isn't sorted on every store.
            let mut oldest: Vec<(String, f64)> = entries.iter().map(|(k, v)| (k.clone(), v.used)).collect();
            oldest.sort_by(|a, b| a.1.total_cmp(&b.1));
            let excess = entries.len() - self.limit + self.limit / 10;
            for (key, _) in oldest.into_iter().take(excess) {
                entries.remove(&key);
            }
        }
        state.dirty = true;
        if !state.write_scheduled {
            state.write_scheduled = true;
            let cache = self.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                cache.flush();
            });
        }
    }

    /// Writes what isn't on disk yet (also at quit).
    pub fn flush(&self) {
        let bytes = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.write_scheduled = false;
            if !state.dirty {
                return;
            }
            state.dirty = false;
            match state.entries.as_ref().map(serde_json::to_vec) {
                Some(Ok(bytes)) => bytes,
                _ => return,
            }
        };
        if let Some(parent) = self.file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let staging = self.file.with_extension("tmp");
        if std::fs::write(&staging, bytes).is_ok() {
            let _ = crate::platform::replace_file(&staging, &self.file);
        }
        let _ = std::fs::remove_file(staging);
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        self.load(&mut state);
        state.entries.as_ref().map_or(0, HashMap::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
