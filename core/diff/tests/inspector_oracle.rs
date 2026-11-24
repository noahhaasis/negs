//! The frozen inspector view's and edit lists' own tests
//! (core/diff/oracle/inspector_view/tests.rs and edits.rs's, on
//! `fixture::Fixture`), run against the frozen copy itself: windows/src's
//! inspector_view/ and edits.rs as they were on b0f1f5b, before Phase 4a
//! moved edits.rs onto the core. They show the oracle 4a's and 4d's
//! differential tests compare with still does what the Windows app did.
//!
//! The modules they name through `crate::` are the state oracle's (frozen
//! state, json, crop, border, negative, help), so the types are the ones
//! the oracle crate's own inspector_view uses.
#![allow(dead_code, unused_imports)]

pub use negs_state_oracle::{border, crop, help, json, negative, state};

#[path = "../oracle/edits.rs"]
pub mod edits;
#[path = "../oracle/inspector_view/mod.rs"]
pub mod inspector_view;
