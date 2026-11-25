//! Unix (macOS + Linux) implementations of the OS helpers in `crate::platform`.

use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow};

use super::MoveError;

/// Renames `from` over `to`, atomically replacing it.
pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    std::fs::rename(from, to).map_err(|e| anyhow!("Could not move the file to {}: {}", to.display(), e))
}

/// Moves `from` to `to` without replacing a distinct existing file.
/// Case-only renames of the same file (same dev+ino on case-insensitive APFS)
/// are allowed and performed via `rename`.
pub fn move_no_replace(from: &Path, to: &Path) -> std::result::Result<(), MoveError> {
    if let (Some(a), Some(b)) = (file_identity(from), file_identity(to)) {
        if a == b {
            return std::fs::rename(from, to).map_err(|e| MoveError::Other(e.to_string()));
        }
        return Err(MoveError::Exists);
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(from) {
                let _ = std::fs::remove_file(to);
                return Err(MoveError::Other(e.to_string()));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(MoveError::Exists),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) || e.raw_os_error() == Some(libc::EOPNOTSUPP) || e.raw_os_error() == Some(libc::EPERM) => {
            if to.exists() {
                return Err(MoveError::Exists);
            }
            std::fs::rename(from, to).map_err(|err| MoveError::Other(err.to_string()))
        }
        Err(e) => Err(MoveError::Other(e.to_string())),
    }
}

pub fn is_in_use_io_error(_error: &std::io::Error) -> bool {
    false
}

/// Device id and inode number: handles case differences and hard links.
pub fn file_identity(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

/// Whether two paths name the same file.
pub fn same_file(a: &Path, b: &Path) -> bool {
    if crate::session::same_path(a, b) {
        return true;
    }
    match (file_identity(a), file_identity(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Marks a file hidden (`UF_HIDDEN` on macOS; dot-prefix convention on Linux).
pub fn hide_file(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) {
            let flags = std::fs::metadata(path).map(|m| std::os::macos::fs::MetadataExt::st_flags(&m)).unwrap_or(0);
            unsafe {
                let _ = libc::chflags(cpath.as_ptr(), flags | libc::UF_HIDDEN);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
    }
}

pub fn is_hidden(path: &Path, metadata: &std::fs::Metadata) -> bool {
    if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        std::os::macos::fs::MetadataExt::st_flags(metadata) & libc::UF_HIDDEN != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = metadata;
        false
    }
}

pub fn normalize_nfc(text: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(text).into_owned()
}

/// Case- and diacritic-insensitive substring match (matches Win32
/// `FindNLSStringEx` with `LINGUISTIC_IGNORECASE | LINGUISTIC_IGNOREDIACRITIC`).
pub fn contains_folded(text: &str, word: &str) -> bool {
    fn fold(s: &str) -> String {
        use icu_properties::CodePointMapData;
        use icu_properties::props::GeneralCategory;
        let nfd = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd();
        let gc = CodePointMapData::<GeneralCategory>::new();
        nfd.normalize(s)
            .chars()
            .filter(|&c| gc.get(c) != GeneralCategory::NonspacingMark)
            .flat_map(char::to_lowercase)
            .collect()
    }
    fold(text).contains(&fold(word))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

pub fn settings_dir() -> PathBuf {
    if let Some(dir) = super::profile_override() {
        return dir;
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library").join("Application Support").join("Negs")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"))
            .join("negs")
    }
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = super::profile_override() {
        return dir.join("Cache");
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library").join("Caches").join("Negs")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".cache"))
            .join("negs")
    }
}

pub fn pictures_dir() -> PathBuf {
    #[cfg(not(target_os = "macos"))]
    if let Some(dir) = std::env::var_os("XDG_PICTURES_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    home_dir().join("Pictures")
}

fn clock_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

pub fn monotonic_ticks() -> i64 {
    clock_epoch().elapsed().as_nanos() as i64
}

pub fn ticks_per_second() -> i64 {
    1_000_000_000
}

pub fn ticks_ms(from: i64, to: i64) -> f64 {
    (to - from) as f64 / 1_000_000.0
}

pub struct Job;

impl Job {
    pub fn get() -> &'static Job {
        static JOB: Job = Job;
        &JOB
    }

    pub fn assign(&self, _child: &std::process::Child) {}
}

pub fn configure_command_no_window(_command: &mut Command) {}

pub fn configure_command_below_normal(_command: &mut Command) {}

pub fn lower_gpu_priority(_child: &std::process::Child) {}

pub fn kill_pid(pid: u32) {
    if pid > 0 {
        unsafe {
            let _ = libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// Whether `pid` is still alive (not exited and not a zombie waiting to be reaped).
pub fn pid_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // Check if our own child has exited without reaping it (`WNOWAIT`), so
    // `helper_running()` immediately sees a killed helper before the worker reaps it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc == 0 {
        // `si_pid != 0` means the child is already exited/zombied; `0` means still running.
        return unsafe { info.si_pid() == 0 };
    }
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Opens a lock file with a non-blocking exclusive `flock`.
/// Because `flock` is per-open-file-description, even two instances in the same
/// process see each other's lock.
pub fn open_exclusive_lock(path: &Path, create_new: bool) -> std::io::Result<std::fs::File> {
    let file = if create_new {
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)?
    } else {
        std::fs::OpenOptions::new().write(true).open(path)?
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

pub fn display_reset_since(_since: SystemTime) -> bool {
    false
}

pub fn watch_memory_pressure(_on_low: impl Fn() + Send + 'static) {}

/// Active monitor's ICC profile bytes (macOS CoreGraphics main display; Linux placeholder).
pub fn monitor_icc_profile_bytes() -> Option<(Vec<u8>, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        monitor_icc_macos(None)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Monitor ICC profile bytes for a specific platform display ID (macOS `CGDirectDisplayID`).
pub fn monitor_icc_profile_bytes_for_display(display_id: u32) -> Option<(Vec<u8>, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        monitor_icc_macos(Some(display_id)).or_else(|| monitor_icc_macos(None))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = display_id;
        None
    }
}

/// macOS CoreGraphics ICC lookup for a specific display ID (or the main display if `None`).
#[cfg(target_os = "macos")]
pub fn monitor_icc_macos(display_id: Option<u32>) -> Option<(Vec<u8>, PathBuf)> {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayCopyColorSpace(display: u32) -> *mut c_void;
        fn CGColorSpaceCopyICCData(space: *const c_void) -> *const c_void;
        fn CFDataGetLength(the_data: *const c_void) -> isize;
        fn CFDataGetBytePtr(the_data: *const c_void) -> *const u8;
        fn CFRelease(cf: *const c_void);
    }

    unsafe {
        let id = display_id.unwrap_or_else(|| CGMainDisplayID());
        let space = CGDisplayCopyColorSpace(id);
        if space.is_null() {
            return None;
        }
        let data = CGColorSpaceCopyICCData(space);
        CFRelease(space);
        if data.is_null() {
            return None;
        }
        let len = CFDataGetLength(data);
        let ptr = CFDataGetBytePtr(data);
        let bytes = if len > 0 && !ptr.is_null() {
            Some(std::slice::from_raw_parts(ptr, len as usize).to_vec())
        } else {
            None
        };
        CFRelease(data);
        bytes.map(|b| {
            let label = crate::icc::profile_name(&b).unwrap_or_else(|| format!("display-{id}.icc"));
            (b, PathBuf::from(label))
        })
    }
}

// MARK: FolderWatch

struct WatchStop {
    stopped: Mutex<bool>,
    condvar: Condvar,
}

/// Lightweight folder watcher for POSIX platforms (notifies immediately on start,
/// then whenever directory contents or mtimes change).
pub struct FolderWatch(Arc<WatchStop>);

impl FolderWatch {
    pub fn start(folder: &Path, mut changed: impl FnMut() + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(WatchStop { stopped: Mutex::new(false), condvar: Condvar::new() });
        let worker_stop = stop.clone();
        let folder = folder.to_path_buf();
        std::thread::Builder::new().name("filmstrip-folder".into()).spawn(move || {
            let mut last = snapshot(&folder);
            changed();
            let mut guard = worker_stop.stopped.lock().unwrap();
            while !*guard {
                let (g, _) = worker_stop.condvar.wait_timeout(guard, Duration::from_millis(500)).unwrap();
                guard = g;
                if *guard {
                    break;
                }
                let next = snapshot(&folder);
                if next != last {
                    last = next;
                    drop(guard);
                    changed();
                    guard = worker_stop.stopped.lock().unwrap();
                }
            }
        })?;
        Ok(Self(stop))
    }
}

fn snapshot(folder: &Path) -> Vec<(String, u64, Option<SystemTime>)> {
    let Ok(entries) = std::fs::read_dir(folder) else { return vec![] };
    let mut list: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), meta.len(), meta.modified().ok()))
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    list
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        let mut stopped = self.0.stopped.lock().unwrap();
        *stopped = true;
        self.0.condvar.notify_all();
    }
}
