//! windows/src's pure rules on the shared Odin core. One module per
//! windows/src module, with the same public API.

// Phase 3a: sheet_rules, tray_rules, compare.
pub mod sheet_rules;
pub mod tray_rules;
pub mod compare;
/// model/compare.rs's pure helpers (the rest is Phase 5's frame pipeline).
pub mod model_compare;



// Phase 3b: crop, border, exact_fit, thumb_compose, histogram, negative.
pub mod border;
pub mod crop;
pub mod exact_fit;
pub mod histogram;
pub mod negative;
pub mod thumb_compose;



// Phase 3c: embedded_preview, photo_facts, tone_graph, help.
pub mod embedded_preview;
pub mod help;
pub mod photo_facts;
pub mod tone_graph;



// Phase 4 base: json, state (the value and the edit state).
pub mod json;
pub mod state;



// Phase 4a



// Phase 4b: presets, session (the settings files).
pub mod presets;
pub mod session;



// Phase 4c: batch, match_cache, rendered_preview.
pub mod batch;
pub mod match_cache;
pub mod rendered_preview;
