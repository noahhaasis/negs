//! windows/src's batch, match_cache and rendered_preview as they are now
//! (on the core, Phase 4c), compiled here on the oracle's EditState, model
//! and engine types so their own unit tests run on the core on any OS; the
//! Windows app's `cargo test` runs them on Windows too.
//!
//! The differential tests (batch.rs, match_cache.rs, rendered_preview.rs)
//! compile the same files without libtest (`harness = false`), so these
//! unit tests, whose core allocations come and go on other threads (a
//! match cache's delayed write holds its cache for a second), never run
//! beside their leak checks.
#![allow(dead_code)]

mod state {
    pub use negs_state_oracle::state::*;
}
mod json {
    pub use negs_state_oracle::json::*;
}
mod engine {
    pub use negs_state_oracle::engine::*;
}
mod model {
    pub use negs_state_oracle::model::*;
}
mod session {
    pub use negs_state_oracle::session::*;
}
mod picture {
    pub use negs_state_oracle::picture::*;
}
mod crop {
    pub use negs_state_oracle::crop::*;
}

#[path = "../../../windows/src/batch.rs"]
mod batch;
#[path = "../../../windows/src/match_cache.rs"]
mod match_cache;
#[path = "../../../windows/src/rendered_preview.rs"]
mod rendered_preview;
