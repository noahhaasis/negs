//! The app's tray rules, compiled without Win32 so they are tested on any
//! OS. See Cargo.toml.
#![allow(dead_code)]

#[path = "../../src/tray_rules.rs"]
pub mod tray_rules;
