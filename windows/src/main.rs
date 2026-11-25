//! Negs for Windows: Win32 with the common controls, wgpu for the photograph.
//! Thin UI shell over the pure-Rust `negs-app` shared core.
//! See docs/windows-app.md.

#![windows_subsystem = "windows"]

pub use negs_app::{
    batch, border, brightness, compare, crop, edited_store, edits, embedded_preview, engine,
    exact_fit, film_cache_release, help, histogram, icc, inspector_view, json, lightroom,
    match_cache, model, negative, photo_facts, picture, platform, presets, previews,
    rendered_preview, session, sheet_rules, state, thumb_compose, thumbnail, tone_graph,
    tray_rules,
};

mod gpu;
mod trace;
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
