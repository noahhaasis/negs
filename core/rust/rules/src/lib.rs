//! windows/src's pure rules on the shared Odin core. One module per
//! windows/src module, with the same public API.

// Phase 3a: sheet_rules, tray_rules, compare.
pub mod sheet_rules;
pub mod tray_rules;
pub mod compare;
/// model/compare.rs's pure helpers (the rest is Phase 5's frame pipeline).
pub mod model_compare;



// Phase 3b: crop, border, exact_fit, thumb_compose, histogram, negative.



// Phase 3c: embedded_preview, photo_facts, tone_graph, help.
