//! The app's own sources, compiled without Win32 so the library mapping
//! is tested on any OS. See Cargo.toml.
#![allow(dead_code)]

#[path = "../../src/border.rs"]
pub mod border;
#[path = "../../src/compare.rs"]
pub mod compare;
#[path = "../../src/crop.rs"]
pub mod crop;
#[path = "../../src/json.rs"]
pub mod json;
#[path = "../../src/model/library_map.rs"]
pub mod library_map;
#[path = "../../src/negative.rs"]
pub mod negative;
#[path = "../../src/state.rs"]
pub mod state;
