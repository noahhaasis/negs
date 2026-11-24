//! windows/src/match_cache.rs on the shared core (core/src/caches/
//! match_cache): the key (SHA-256 of what a solve read), the engine stamp,
//! and the LRU entries file. The cache is a core handle behind a Mutex.
//!
//! Kept in Rust: the Mutex and the shared instance's OnceLock (windows/
//! src/match_cache.rs), the clock (`now`, passed in, so the core is
//! deterministic), and the thread that writes a second after a store.
//! A HashMap's order and the LRU's order among equal times are random in
//! the Rust; the core writes keys sorted and breaks ties by key.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use negs_core_sys::caches as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde_json::Value;

use crate::batch::path_bytes;

pub const LIMIT: usize = 2000;

fn empty() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

/// Seconds since 1970, as the Rust's `now`.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The name of a solve of `film`, `paper` and `params` on `file` in
/// `engine` (`engine_stamp`), or None if the file can't be read.
pub fn key(file: &Path, film: &str, paper: &str, params: &Value, engine: &str) -> Option<String> {
    let f = path_bytes(file);
    let p = crate::json::text(params);
    let mut out = empty();
    let ok = unsafe {
        sys::negs_match_key(
            f.as_ptr(),
            f.len(),
            film.as_ptr(),
            film.len(),
            paper.as_ptr(),
            paper.len(),
            p.as_ptr(),
            p.len(),
            engine.as_ptr(),
            engine.len(),
            &mut out,
        )
    };
    let bytes = take_buf(out);
    (ok == 1).then(|| String::from_utf8(bytes).expect("hex"))
}

/// The engine a solve ran in: `version` (the app's), the helper binary as
/// it is on disk, and the data folder.
pub fn engine_stamp(version: &str, helper: &Path, data: &Path) -> String {
    let (h, d) = (path_bytes(helper), path_bytes(data));
    let bytes = take_buf(unsafe { sys::negs_match_engine_stamp(version.as_ptr(), version.len(), h.as_ptr(), h.len(), d.as_ptr(), d.len()) });
    String::from_utf8(bytes).expect("the core writes UTF-8")
}

struct Handle(*mut sys::negs_match_cache_t);

// The handle is only used under the Mutex.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { sys::negs_match_cache_free(self.0) }
    }
}

pub struct MatchCache {
    file: PathBuf,
    state: Mutex<Handle>,
}

impl MatchCache {
    pub fn new(file: PathBuf, limit: usize) -> Self {
        let f = path_bytes(&file);
        let handle = unsafe { sys::negs_match_cache_new(f.as_ptr(), f.len(), limit as u64) };
        Self { file, state: Mutex::new(Handle(handle)) }
    }

    /// The solve kept for `key`, marked as used.
    pub fn solved(&self, key: &str) -> Option<(f64, bool)> {
        self.solved_at(key, now())
    }

    /// `solved` at a given time.
    pub fn solved_at(&self, key: &str, now: f64) -> Option<(f64, bool)> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (mut ev, mut limited) = (0.0, 0u8);
        let found = unsafe { sys::negs_match_cache_solved(state.0, key.as_ptr(), key.len(), now, &mut ev, &mut limited) };
        (found == 1).then_some((ev, limited != 0))
    }

    /// Keeps a solve made on a whole decode; written a second later.
    pub fn store(self: &Arc<Self>, key: String, ev: f64, limited: bool) {
        if self.store_at(&key, ev, limited, now()) {
            let cache = self.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                cache.flush();
            });
        }
    }

    /// `store` at a given time, without the write: true when one should be
    /// scheduled.
    pub fn store_at(&self, key: &str, ev: f64, limited: bool, now: f64) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        unsafe { sys::negs_match_cache_store(state.0, key.as_ptr(), key.len(), ev, limited as u8, now) == 1 }
    }

    /// Writes what isn't on disk yet (also at quit).
    pub fn flush(&self) {
        let bytes = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut out = empty();
            let write = unsafe { sys::negs_match_cache_flush_bytes(state.0, &mut out) };
            let bytes = take_buf(out);
            if write == 0 {
                return;
            }
            bytes
        };
        let f = path_bytes(&self.file);
        unsafe { sys::negs_match_cache_write(f.as_ptr(), f.len(), bytes.as_ptr(), bytes.len()) }
    }

    pub fn count(&self) -> usize {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        unsafe { sys::negs_match_cache_count(state.0) as usize }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn least_recently_used_out_and_kept_on_disk() {
        let dir = std::env::temp_dir().join(format!("negs-rules-match-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("BrightnessMatches.json");
        assert_eq!(unsafe { sys::negs_match_cache_limit() }, LIMIT as u64);
        let cache = MatchCache::new(file.clone(), 10);
        for i in 0..10 {
            cache.store_at(&format!("k{i}"), i as f64 / 10.0, i % 2 == 0, i as f64);
        }
        assert_eq!(cache.solved_at("k0", 10.0), Some((0.0, true)));
        cache.store_at("k10", 1.0, false, 11.0);
        assert_eq!(cache.count(), 9);
        assert!(cache.solved_at("k1", 12.0).is_none() && cache.solved_at("k2", 12.0).is_none());
        cache.flush();
        let again = MatchCache::new(file, 10);
        assert_eq!(again.solved_at("k10", 13.0), Some((1.0, false)));
        assert_eq!(again.count(), 9);
        assert!(key(&dir.join("missing"), "f", "p", &Value::Null, "e").is_none());
        assert_eq!(key(&dir.join("BrightnessMatches.json"), "f", "p", &Value::Null, "e").unwrap().len(), 64);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
