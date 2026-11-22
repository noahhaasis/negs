//! windows/src's pure rules on the shared Odin core. One module per
//! windows/src module, with the same public API.

// Phase 3a: sheet_rules, tray_rules, compare.



// Phase 3b: crop, border, exact_fit, thumb_compose, histogram, negative.
pub mod border;
pub mod crop;
pub mod exact_fit;
pub mod histogram;
pub mod negative;
pub mod thumb_compose;



// Phase 3c: embedded_preview, photo_facts, tone_graph, help.
