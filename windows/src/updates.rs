//! Updates through WinSparkle (§20.6, decided): Sparkle's appcast format and
//! EdDSA signatures. Like the Mac's `UpdateController`, nothing is guessed for
//! local builds: packaging sets both `NEGS_APPCAST_URL` and
//! `NEGS_ED_PUBLIC_KEY` at build time, and ships `WinSparkle.dll` next to
//! Negs. The updater starts 3 s after launch, once the first render is done,
//! and a downloaded update waits for a running export or save.

use std::cell::Cell;
use std::ffi::c_void;
use std::time::Duration;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{PCSTR, PCWSTR};

use crate::platform::wide;
use crate::ui::{self, App};

const FEED: Option<&str> = option_env!("NEGS_APPCAST_URL");
const KEY: Option<&str> = option_env!("NEGS_ED_PUBLIC_KEY");

/// A feed over HTTPS and a 32-byte EdDSA public key, both or nothing.
pub fn valid_configuration(feed: Option<&str>, key: Option<&str>) -> bool {
    let (Some(feed), Some(key)) = (feed, key) else { return false };
    let host = feed.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or("");
    !host.is_empty() && base64_len(key) == Some(32)
}

fn base64_len(text: &str) -> Option<usize> {
    let text = text.trim();
    if text.is_empty() || text.len() % 4 != 0 {
        return None;
    }
    let padding = text.bytes().rev().take_while(|&b| b == b'=').count();
    if padding > 2 || !text[..text.len() - padding].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/') {
        return None;
    }
    Some(text.len() / 4 * 3 - padding)
}

pub fn configured() -> bool {
    valid_configuration(FEED, KEY) && library().is_some()
}

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

fn library() -> Option<HMODULE> {
    thread_local! {
        static LIBRARY: Cell<Option<Option<isize>>> = const { Cell::new(None) };
    }
    LIBRARY.with(|l| {
        if l.get().is_none() {
            let exe = std::env::current_exe().ok();
            let path = exe.and_then(|e| e.parent().map(|d| d.join("WinSparkle.dll")));
            let module = path.filter(|p| p.exists()).and_then(|p| unsafe { LoadLibraryW(PCWSTR(wide(p).as_ptr())).ok() });
            l.set(Some(module.map(|m| m.0 as isize)));
        }
        l.get().flatten().map(|m| HMODULE(m as *mut c_void))
    })
}

fn function(name: &str) -> Option<*const c_void> {
    let module = library()?;
    let name = std::ffi::CString::new(name).ok()?;
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr() as *const u8)).map(|f| f as *const c_void) }
}

macro_rules! call {
    ($name:literal, fn($($arg:ty),*) $(-> $ret:ty)?, $($value:expr),*) => {
        function($name).map(|f| unsafe {
            let f: extern "C" fn($($arg),*) $(-> $ret)? = std::mem::transmute(f);
            f($($value),*)
        })
    };
}

/// WinSparkle asks before it installs: not while exporting or saving.
extern "C" fn can_shutdown() -> i32 {
    // Called on WinSparkle's thread: the UI thread answers through a flag it keeps.
    BUSY.load(std::sync::atomic::Ordering::Relaxed).then_some(0).unwrap_or(1)
}

static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn shutdown_request() {
    ui::post(|app: &mut App| unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(app.main, windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
    });
}

/// Starts the updater after startup work, never during it.
pub fn schedule() {
    if !configured() {
        return;
    }
    ui::after(Duration::from_secs(3), wait_for_first_render);
}

fn wait_for_first_render(app: &mut App) {
    let model = &app.model;
    let settled = (model.ready && (model.source.is_none() || model.preview_current || model.render_failed)) || model.error.is_some();
    if !settled {
        ui::after(Duration::from_millis(500), wait_for_first_render);
        return;
    }
    start(app.model.prefs.check_for_updates);
}

fn start(automatic: bool) -> bool {
    if STARTED.with(|s| s.get()) {
        return true;
    }
    let (Some(feed), Some(key)) = (FEED, KEY) else { return false };
    if !valid_configuration(Some(feed), Some(key)) {
        return false;
    }
    let feed = std::ffi::CString::new(feed).unwrap();
    let key = std::ffi::CString::new(key).unwrap();
    call!("win_sparkle_set_appcast_url", fn(*const i8), feed.as_ptr());
    // WinSparkle returns 0 for invalid keys. Never start unsigned updates
    // if the DLL is too old or rejects the configured key.
    if call!("win_sparkle_set_eddsa_public_key", fn(*const i8) -> i32, key.as_ptr()) != Some(1) {
        return false;
    }
    call!("win_sparkle_set_can_shutdown_callback", fn(extern "C" fn() -> i32), can_shutdown);
    call!("win_sparkle_set_shutdown_request_callback", fn(extern "C" fn()), shutdown_request);
    call!("win_sparkle_set_automatic_check_for_updates", fn(i32), automatic as i32);
    let started = call!("win_sparkle_init", fn(),).is_some();
    STARTED.with(|s| s.set(started));
    started
}

pub fn check(with_ui: bool) {
    if !configured() {
        return;
    }
    if start(true) && with_ui {
        call!("win_sparkle_check_update_with_ui", fn(),);
    }
}

pub fn automatic() -> bool {
    call!("win_sparkle_get_automatic_check_for_updates", fn() -> i32,).is_some_and(|v| v != 0)
}

pub fn set_automatic(on: bool) {
    call!("win_sparkle_set_automatic_check_for_updates", fn(i32), on as i32);
}

pub fn last_check() -> Option<String> {
    let time = call!("win_sparkle_get_last_check_time", fn() -> i64,)?;
    (time > 0).then(|| {
        let ago = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - time;
        match ago {
            a if a < 120 => "just now".into(),
            a if a < 7200 => format!("{} minutes ago", a / 60),
            a if a < 172_800 => format!("{} hours ago", a / 3600),
            a => format!("{} days ago", a / 86_400),
        }
    })
}

/// The UI thread keeps WinSparkle's view of whether Negs may quit.
pub fn set_busy(busy: bool) {
    BUSY.store(busy, std::sync::atomic::Ordering::Relaxed);
}

pub fn stop() {
    if STARTED.with(|s| s.get()) {
        call!("win_sparkle_cleanup", fn(),);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_feed_and_key_or_nothing() {
        let key = "VGhpcyBpcyAzMiBieXRlcyBsb25nIGtleSBmb3IgZWQ="; // 32 bytes
        assert_eq!(base64_len(key), Some(32));
        assert!(valid_configuration(Some("https://negs.app/appcast.xml"), Some(key)));
        assert!(!valid_configuration(Some("http://negs.app/appcast.xml"), Some(key)));
        assert!(!valid_configuration(Some("https://negs.app/appcast.xml"), None));
        assert!(!valid_configuration(None, Some(key)));
        assert!(!valid_configuration(Some("https://negs.app/a"), Some("short")));
        assert_eq!(base64_len("===="), None);
        assert_eq!(base64_len("a=bc"), None);
    }
}
