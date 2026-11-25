//! Updates through WinSparkle on Windows, as `windows/src/updates.rs`:
//! Sparkle's appcast format and EdDSA signatures. Nothing is guessed for
//! local builds: packaging sets both `NEGS_APPCAST_URL` and
//! `NEGS_ED_PUBLIC_KEY` at build time and ships `WinSparkle.dll` next to
//! Negs (`windows/package.ps1 -WinSparkleDll … -AppcastUrl … -EdPublicKey …`).
//! The updater starts 3 s after launch, once the first picture is up, and a
//! downloaded update waits for a running export or save. Elsewhere (the Mac
//! keeps Sparkle in the SwiftUI app until Phase 7b) updates are "not
//! configured".

#[allow(dead_code)]
const FEED: Option<&str> = option_env!("NEGS_APPCAST_URL");
#[allow(dead_code)]
const KEY: Option<&str> = option_env!("NEGS_ED_PUBLIC_KEY");

/// A feed over HTTPS and a 32-byte EdDSA public key, both or nothing.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn valid_configuration(feed: Option<&str>, key: Option<&str>) -> bool {
    let (Some(feed), Some(key)) = (feed, key) else { return false };
    let host = feed.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or("");
    !host.is_empty() && base64_len(key) == Some(32)
}

#[cfg_attr(not(windows), allow(dead_code))]
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

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{PCSTR, PCWSTR};

    use super::{FEED, KEY, valid_configuration};

    static STARTED: AtomicBool = AtomicBool::new(false);
    static BUSY: AtomicBool = AtomicBool::new(false);
    /// WinSparkle asks the app to quit (to install) from its own thread.
    pub static QUIT: OnceLock<async_channel::Sender<()>> = OnceLock::new();

    fn library() -> Option<isize> {
        static LIBRARY: OnceLock<Option<isize>> = OnceLock::new();
        *LIBRARY.get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            let path = exe.parent()?.join("WinSparkle.dll");
            if !path.exists() {
                return None;
            }
            let wide: Vec<u16> = path.as_os_str().encode_wide_null();
            unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())).ok().map(|m| m.0 as isize) }
        })
    }

    trait WideNull {
        fn encode_wide_null(&self) -> Vec<u16>;
    }
    impl WideNull for std::ffi::OsStr {
        fn encode_wide_null(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().chain(Some(0)).collect()
        }
    }

    fn function(name: &str) -> Option<*const c_void> {
        let module = windows::Win32::Foundation::HMODULE(library()? as *mut c_void);
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

    extern "C" fn can_shutdown() -> i32 {
        if BUSY.load(Ordering::Relaxed) { 0 } else { 1 }
    }

    extern "C" fn shutdown_request() {
        if let Some(tx) = QUIT.get() {
            let _ = tx.try_send(());
        }
    }

    pub fn configured() -> bool {
        valid_configuration(FEED, KEY) && library().is_some()
    }

    pub fn start(automatic: bool) -> bool {
        if STARTED.load(Ordering::Relaxed) {
            return true;
        }
        let (Some(feed), Some(key)) = (FEED, KEY) else { return false };
        if !configured() {
            return false;
        }
        let feed = std::ffi::CString::new(feed).unwrap();
        let key = std::ffi::CString::new(key).unwrap();
        call!("win_sparkle_set_appcast_url", fn(*const i8), feed.as_ptr());
        // WinSparkle returns 0 for an invalid key: never start unsigned updates.
        if call!("win_sparkle_set_eddsa_public_key", fn(*const i8) -> i32, key.as_ptr()) != Some(1) {
            return false;
        }
        call!("win_sparkle_set_can_shutdown_callback", fn(extern "C" fn() -> i32), can_shutdown);
        call!("win_sparkle_set_shutdown_request_callback", fn(extern "C" fn()), shutdown_request);
        call!("win_sparkle_set_automatic_check_for_updates", fn(i32), automatic as i32);
        let started = call!("win_sparkle_init", fn(),).is_some();
        STARTED.store(started, Ordering::Relaxed);
        started
    }

    pub fn check(with_ui: bool) {
        if start(true) && with_ui {
            call!("win_sparkle_check_update_with_ui", fn(),);
        }
    }

    pub fn set_automatic(on: bool) {
        call!("win_sparkle_set_automatic_check_for_updates", fn(i32), on as i32);
    }

    pub fn last_check() -> Option<String> {
        let time = call!("win_sparkle_get_last_check_time", fn() -> i64,)?;
        (time > 0).then(|| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            match now - time {
                a if a < 120 => "just now".into(),
                a if a < 7200 => format!("{} minutes ago", a / 60),
                a if a < 172_800 => format!("{} hours ago", a / 3600),
                a => format!("{} days ago", a / 86_400),
            }
        })
    }

    pub fn set_busy(busy: bool) {
        BUSY.store(busy, Ordering::Relaxed);
    }

    pub fn stop() {
        if STARTED.load(Ordering::Relaxed) {
            call!("win_sparkle_cleanup", fn(),);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn configured() -> bool {
        false
    }
    pub fn start(_automatic: bool) -> bool {
        false
    }
    pub fn check(_with_ui: bool) {}
    pub fn set_automatic(_on: bool) {}
    pub fn last_check() -> Option<String> {
        None
    }
    pub fn set_busy(_busy: bool) {}
    pub fn stop() {}
}

pub use imp::*;

/// A channel the app listens on for WinSparkle's "quit to install".
pub fn quit_requests() -> Option<async_channel::Receiver<()>> {
    #[cfg(windows)]
    {
        let (tx, rx) = async_channel::bounded(1);
        imp::QUIT.set(tx).ok()?;
        Some(rx)
    }
    #[cfg(not(windows))]
    None
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
        assert!(!configured(), "a local build has no feed");
    }
}
