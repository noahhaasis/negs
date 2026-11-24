//! windows/src's state.rs and edits.rs as they are now (on the core, Phase
//! 4a), with the inspector view and the files it reads, compiled here so
//! their own unit tests (state.rs's, edits.rs's, inspector_view/tests.rs)
//! run through the wrappers on any OS; the Windows app's `cargo test` runs
//! them on Windows too.
//!
//! `windows` is the state oracle's stand-in (FindNLSStringEx through ICU),
//! so the film search's accents fold here as the oracle's do. state.rs's
//! live-catalog test asks the helper; `engine` below has none, so it
//! skips, as on a PC without a release helper.
#![allow(dead_code, unused_imports)]

// The real crate on Windows under the name windows/src uses (Cargo.toml).
#[cfg(windows)]
extern crate windows_crate as windows;

#[path = "../../../windows/src/state.rs"]
pub mod state;
#[path = "../../../windows/src/edits.rs"]
pub mod edits;
#[path = "../../../windows/src/inspector_view/mod.rs"]
pub mod inspector_view;
#[path = "../../../windows/src/json.rs"]
pub mod json;
#[path = "../../../windows/src/crop.rs"]
pub mod crop;
/// border.rs's own tests read data/ beside the app (windows_units.rs);
/// its rules are the core's.
pub mod border {
    pub use negs_core_rules::border::*;
}
#[path = "../../../windows/src/negative.rs"]
pub mod negative;
#[path = "../../../windows/src/help.rs"]
pub mod help;

pub mod engine {
    pub struct EnginePaths;
    impl EnginePaths {
        pub fn locate() -> Result<Self, ()> {
            Err(())
        }
    }
    pub struct EngineClient;
    impl EngineClient {
        pub fn start(_: EnginePaths) -> Result<Self, ()> {
            Err(())
        }
        pub fn request(&self, _: serde_json::Value, _: std::time::Duration, _: impl FnOnce(Result<(u64, serde_json::Value), String>) + Send + 'static) {}
        pub fn stop(&self) {}
    }
    pub fn timeout_for(_: &str) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
