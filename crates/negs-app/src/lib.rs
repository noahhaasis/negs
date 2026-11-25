//! Pure-Rust shared application core for Negs.
//!
//! Contains all UI-toolkit-agnostic domain rules, state, inspector view model,
//! engine client, preview/thumbnail pipeline, theme tokens, color/ICC math,
//! and the `Model` orchestrator behind minimal `platform` and `host`
//! boundaries.

pub mod batch;
pub mod border;
pub mod brightness;
pub mod compare;
pub mod crop;
pub mod edited_store;
pub mod edits;
pub mod embedded_preview;
pub mod engine;
pub mod exact_fit;
pub mod film_cache_release;
pub mod help;
pub mod histogram;
pub mod host;
pub mod icc;
pub mod inspector_view;
pub mod json;
pub mod lightroom;
pub mod match_cache;
pub mod model;
pub mod negative;
pub mod photo_facts;
pub mod picture;
pub mod platform;
pub mod presets;
pub mod previews;
pub mod rendered_preview;
pub mod session;
pub mod sheet_rules;
pub mod state;
pub mod theme;
pub mod thumb_compose;
pub mod thumbnail;
pub mod tone_graph;
pub mod trace;
pub mod tray_rules;
