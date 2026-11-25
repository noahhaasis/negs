//! Whether the system asks for less motion: macOS's Reduce Motion
//! (`accessibilityDisplayShouldReduceMotion`, read from its preference),
//! Windows' "Animation effects" (`SPI_GETCLIENTAREAANIMATION`). Then the
//! grid shows and hides at once and a wheel's notch doesn't glide.
//! `NEGS_REDUCE_MOTION=1` (or `0`) overrides it, for tests and captures.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// 0 unknown, 1 motion, 2 reduced; read again every few seconds.
static CACHED: AtomicU8 = AtomicU8::new(0);
static READ_AT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

pub fn reduced() -> bool {
    if let Ok(v) = std::env::var("NEGS_REDUCE_MOTION") {
        return v == "1";
    }
    let mut at = READ_AT.lock().unwrap_or_else(|e| e.into_inner());
    if at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) || CACHED.load(Ordering::Relaxed) == 0 {
        CACHED.store(if system_reduced() { 2 } else { 1 }, Ordering::Relaxed);
        *at = Some(Instant::now());
    }
    CACHED.load(Ordering::Relaxed) == 2
}

#[cfg(target_os = "macos")]
fn system_reduced() -> bool {
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const std::ffi::c_char, encoding: u32) -> *const c_void;
        fn CFPreferencesGetAppBooleanValue(key: *const c_void, app: *const c_void, valid: *mut u8) -> u8;
        fn CFRelease(cf: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;
    unsafe {
        let key = CFStringCreateWithCString(std::ptr::null(), c"reduceMotion".as_ptr(), UTF8);
        let app = CFStringCreateWithCString(std::ptr::null(), c"com.apple.universalaccess".as_ptr(), UTF8);
        if key.is_null() || app.is_null() {
            return false;
        }
        let mut valid = 0u8;
        let on = CFPreferencesGetAppBooleanValue(key, app, &mut valid);
        CFRelease(key);
        CFRelease(app);
        valid != 0 && on != 0
    }
}

#[cfg(windows)]
fn system_reduced() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut std::ffi::c_void, winini: u32) -> i32;
    }
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    let mut on: i32 = 1;
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut i32 as *mut _, 0) };
    ok != 0 && on == 0
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system_reduced() -> bool {
    false
}
