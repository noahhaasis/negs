//! The inspector as data (Phase 4 of the shared core): which rows show on
//! each tab, their words, values, ranges and states, each tab's edits, and
//! what each control does. `ui/inspector.rs` draws it with Win32; macOS and
//! Linux can draw the same view.
//!
//! - `facts`: what it reads from the editor model, as plain data.
//! - `schema`: the rows, in order, and when each shows.
//! - `view`: `build(facts, ui)` → `InspectorView`.
//! - `command`: `dispatch(facts, ui, row, action)` → the model changes, as
//!   data.
//! - `output`: the export's choices it shows (moved from `model.rs`).
//!
//! Platform-free: it uses only `state`, `edits`, `json`, `crop`, `border`,
//! `negative` and `help`, no Win32 and no `Model`, so the shared core's
//! differential tests can compile it as their oracle.

pub mod command;
pub mod facts;
pub mod output;
pub mod schema;
pub mod view;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

// The module's API, for this app and for the shared core's oracle.
#[allow(unused_imports)]
pub use command::{BorderChange, EditChoice, Effect, NegativeAction, RowAction, Track, dispatch, edit_list, edits_head, has_reset, resettable};
#[allow(unused_imports)]
pub use facts::{Brightness, ColorFilters, InspectorFacts, NegativeFacts, Params, Prefs};
#[allow(unused_imports)]
pub use output::{BorderWidth, COLOR_SPACES, OutputFormat, PROPHOTO, PROPHOTO_FALLBACK, QUALITY_RANGE, percent};
#[allow(unused_imports)]
pub use schema::{Kind, More, Note, Row, RowId, When, rows};
#[allow(unused_imports)]
pub use view::{Expanded, InspectorUi, InspectorView, RowView, SliderValue, Swatches, build};
