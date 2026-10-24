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
    // GUI subsystem builds have no console: keep a local diagnostic for a
    // panic, including panics in window procedures that must abort at the ABI.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let directory = session::settings_dir();
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(directory.join("panic.log")) {
            use std::io::Write;
            let _ = writeln!(log, "{info}\n{}", std::backtrace::Backtrace::force_capture());
        }
        previous(info);
    }));
    ui::run();
}
