//! Lets the helper free the film side it keeps on the GPU between renders
//! (ticket 41, the Mac's `FilmCacheRelease.swift`) when it won't be used
//! soon: the editor went back to the grid, or Windows is short of memory.
//! The helper already frees it when another photograph is decoded, so a
//! photo switch sends nothing, and neither does anything during editing.
//!
//! The command goes through the helper's serial request path like any other,
//! after whatever is already queued; failures are ignored (an older helper
//! doesn't know it), and nothing is sent while no helper runs, which would
//! only start one. The next render after a release develops the film again,
//! as it would have without the cache. (On Windows the helper keeps no film
//! side yet, `film_cache_budget` being 0 off macOS, so it answers 0 bytes.)

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;

use crate::engine::{self, EngineClient};
use crate::host;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    EditorClosed,
    MemoryPressure,
}

impl Reason {
    fn name(self) -> &'static str {
        match self {
            Reason::EditorClosed => "editor_closed",
            Reason::MemoryPressure => "memory_pressure",
        }
    }
}

/// Sends the release, unless no helper runs.
pub fn release(engine: &EngineClient, reason: Reason) -> bool {
    if !engine.helper_running() {
        return false;
    }
    engine.request(json!({"command": "release_film_cache"}), engine::timeout_for("release_film_cache"), move |result| {
        crate::trace::event("film_cache_released", || json!({
            "reason": reason.name(),
            "released_bytes": result.as_ref().ok().map(|(_, r)| r["released_bytes"].clone()),
        }));
    });
    true
}

/// Releases whenever Windows says memory is low
/// (`CreateMemoryResourceNotification`), once per low spell: it then waits
/// until memory is no longer low before it listens again. Started once,
/// after launch's first image.
pub fn watch_memory_pressure() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::platform::watch_memory_pressure(|| {
        host::post(|model| model.release_film_cache(Reason::MemoryPressure));
    });
}
