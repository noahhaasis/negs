//! Negs for Windows: Win32 with the common controls, wgpu for the photograph.
//! See docs/windows-app.md.

#![windows_subsystem = "windows"]

mod border;
mod crop;
mod engine;
mod gpu;
mod histogram;
mod icc;
mod json;
mod model;
mod picture;
mod platform;
mod platform_events;
mod session;
mod state;
mod text;
mod thumbnail;
mod ui;
mod updates;

fn main() {
    ui::run();
}
