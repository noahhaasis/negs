//! Windows implementations of the OS helpers in `crate::platform`.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::{
    FIND_FROMSTART, FindNLSStringEx, LINGUISTIC_IGNORECASE, LINGUISTIC_IGNOREDIACRITIC,
    LOCALE_NAME_INVARIANT, NormalizationC, NormalizeString,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::EventLog::*;
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::ColorSystem::GetICMProfileW;
use windows::Win32::UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{PCWSTR, PWSTR};

use super::MoveError;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// A NUL-terminated UTF-16 string for Win32.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn pcwstr(w: &[u16]) -> PCWSTR {
    PCWSTR(w.as_ptr())
}

pub fn from_wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// Renames `from` over `to`, replacing it. `rename` alone fails on Windows if
/// the target exists.
pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    let (f, t) = (wide(from), wide(to));
    unsafe { MoveFileExW(pcwstr(&f), pcwstr(&t), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
        .map_err(|e| anyhow!("Could not move the file to {}: {}", to.display(), e.message()))
}

/// Moves `from` to `to` without replacing an existing target file.
pub fn move_no_replace(from: &Path, to: &Path) -> std::result::Result<(), MoveError> {
    let (f, t) = (wide(from), wide(to));
    unsafe { MoveFileExW(pcwstr(&f), pcwstr(&t), MOVEFILE_WRITE_THROUGH) }.map_err(|error| {
        if error.code() == ERROR_ALREADY_EXISTS.to_hresult() || error.code() == ERROR_FILE_EXISTS.to_hresult() {
            MoveError::Exists
        } else if error.code() == ERROR_SHARING_VIOLATION.to_hresult() || error.code() == ERROR_LOCK_VIOLATION.to_hresult() {
            MoveError::InUse
        } else {
            MoveError::Other(error.message())
        }
    })
}

pub fn is_in_use_io_error(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == ERROR_SHARING_VIOLATION.0 as i32 || code == ERROR_LOCK_VIOLATION.0 as i32
    )
}

/// Volume serial number and file index: the file's identity, which handles
/// case differences and hard links.
pub fn file_identity(path: &Path) -> Option<(u64, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    Some((info.dwVolumeSerialNumber as u64, (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64))
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

/// Hides a temporary file next to a destination: a dot prefix doesn't.
pub fn hide_file(path: &Path) {
    let w = wide(path);
    unsafe {
        let _ = SetFileAttributesW(pcwstr(&w), FILE_ATTRIBUTE_HIDDEN);
    }
}

pub fn is_hidden(_path: &Path, metadata: &std::fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN.0 != 0
}

pub fn normalize_nfc(text: &str) -> String {
    let source: Vec<u16> = text.encode_utf16().collect();
    if source.is_empty() {
        return String::new();
    }
    // The estimate can be short; the call says how much it needs.
    let mut length = unsafe { NormalizeString(NormalizationC, &source, None) };
    for _ in 0..3 {
        if length <= 0 {
            break;
        }
        let mut buffer = vec![0u16; length as usize];
        let written = unsafe { NormalizeString(NormalizationC, &source, Some(&mut buffer)) };
        if written > 0 {
            return String::from_utf16_lossy(&buffer[..written as usize]);
        }
        length = -written;
    }
    text.to_owned()
}

pub fn contains_folded(text: &str, word: &str) -> bool {
    let (text, word): (Vec<u16>, Vec<u16>) = (text.encode_utf16().collect(), word.encode_utf16().collect());
    let flags = FIND_FROMSTART | LINGUISTIC_IGNORECASE.0 | LINGUISTIC_IGNOREDIACRITIC.0;
    unsafe { FindNLSStringEx(LOCALE_NAME_INVARIANT, flags, &text, &word, None, None, None, LPARAM(0)) >= 0 }
}

pub fn settings_dir() -> PathBuf {
    if let Some(dir) = super::profile_override() {
        return dir;
    }
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("Negs")
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = super::profile_override() {
        return dir.join("Cache");
    }
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Negs")
}

pub fn pictures_dir() -> PathBuf {
    unsafe {
        if let Ok(path) = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) {
            let found = path.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(path.0 as *const _));
            if let Some(found) = found {
                return found;
            }
        }
    }
    PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Pictures")
}

pub fn monotonic_ticks() -> i64 {
    let mut t = 0;
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
    }
    t
}

pub fn ticks_per_second() -> i64 {
    let mut f = 0;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut f);
    }
    f
}

pub fn ticks_ms(from: i64, to: i64) -> f64 {
    static FREQUENCY: OnceLock<f64> = OnceLock::new();
    let f = *FREQUENCY.get_or_init(|| ticks_per_second() as f64);
    (to - from) as f64 * 1000.0 / f
}

/// The job object every engine process joins. Windows kills them when Negs
/// exits for any reason, crash included.
pub struct Job(HANDLE);

unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn get() -> &'static Job {
        static JOB: OnceLock<Job> = OnceLock::new();
        JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).unwrap_or_default();
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let _ = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            );
            Job(job)
        })
    }

    pub fn assign(&self, child: &std::process::Child) {
        unsafe {
            let _ = AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle()));
        }
    }
}

pub fn configure_command_no_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn configure_command_below_normal(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
}

/// Puts `child`'s GPU work below the app's (`D3DKMTSetProcessSchedulingPriorityClass`,
/// below normal), so the compositor and the app's own drawing go first.
/// Best effort: nothing happens where gdi32 hasn't it.
pub fn lower_gpu_priority(child: &std::process::Child) {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{PCSTR, w};
    type SetPriority = unsafe extern "system" fn(HANDLE, i32) -> i32;
    const BELOW_NORMAL: i32 = 1;
    unsafe {
        let Ok(gdi) = LoadLibraryW(w!("gdi32.dll")) else { return };
        let Some(f) = GetProcAddress(gdi, PCSTR(c"D3DKMTSetProcessSchedulingPriorityClass".as_ptr() as *const u8)) else { return };
        let set: SetPriority = std::mem::transmute(f);
        let status = set(HANDLE(child.as_raw_handle()), BELOW_NORMAL);
        crate::trace::event("helper_gpu_priority", || serde_json::json!({"status": status}));
    }
}

pub fn kill_pid(pid: u32) {
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(process, 1);
            let _ = CloseHandle(process);
        }
    }
}

/// The process hasn't exited. The worker keeps its handle until it waits for
/// it, so the id isn't reused meanwhile.
pub fn pid_running(pid: u32) -> bool {
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let running = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        let _ = CloseHandle(process);
        running
    }
}

/// Opens a scratch-space lock file exclusively: `create_new` creates it held
/// against other writers/deleters; `!create_new` probes whether an existing
/// lock is unowned.
pub fn open_exclusive_lock(path: &Path, create_new: bool) -> std::io::Result<std::fs::File> {
    if create_new {
        std::fs::OpenOptions::new().write(true).create_new(true).share_mode(1).open(path)
    } else {
        std::fs::OpenOptions::new().write(true).share_mode(0).open(path)
    }
}

/// Whether Windows logged a display driver reset (System log, event 4101) at or after `since`.
pub fn display_reset_since(since: SystemTime) -> bool {
    let query = format!(
        "*[System[Provider[@Name='Display'] and (EventID=4101) and TimeCreated[@SystemTime>='{}']]]",
        super::iso_utc(since - Duration::from_secs(1))
    );
    let (channel, query) = (wide("System"), wide(query));
    unsafe {
        let Ok(results) = EvtQuery(None, PCWSTR(channel.as_ptr()), PCWSTR(query.as_ptr()), (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0) as u32) else {
            return false;
        };
        let mut events = [0isize; 1];
        let mut returned = 0u32;
        let found = EvtNext(results, &mut events, 0, 0, &mut returned).is_ok() && returned > 0;
        if found {
            let _ = EvtClose(EVT_HANDLE(events[0]));
        }
        let _ = EvtClose(results);
        found
    }
}

/// Watches Windows' low-memory notification and calls `on_low` once per low spell.
pub fn watch_memory_pressure(on_low: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        use windows::Win32::System::Memory::{CreateMemoryResourceNotification, LowMemoryResourceNotification, QueryMemoryResourceNotification};
        let Ok(low) = (unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }) else { return };
        loop {
            if unsafe { WaitForSingleObject(low, INFINITE) } != WAIT_OBJECT_0 {
                return;
            }
            on_low();
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let mut still = BOOL(0);
                if unsafe { QueryMemoryResourceNotification(low, &mut still) }.is_err() || !still.as_bool() {
                    break;
                }
            }
        }
    });
}

/// The ICC profile path assigned to the monitor `hwnd` is on.
pub fn monitor_profile_path(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            return None;
        }
        let dc = CreateDCW(PCWSTR(info.szDevice.as_ptr()), PCWSTR(info.szDevice.as_ptr()), PCWSTR::null(), None);
        if dc.is_invalid() {
            return None;
        }
        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;
        let ok = GetICMProfileW(dc, &mut size, PWSTR(buffer.as_mut_ptr())).as_bool();
        let _ = DeleteDC(dc);
        ok.then(|| PathBuf::from(from_wide(&buffer)))
    }
}

/// Monitor ICC profile bytes for the primary monitor (or current host window).
pub fn monitor_icc_profile_bytes() -> Option<(Vec<u8>, PathBuf)> {
    let hwnd = HWND(crate::host::owner_window_handle() as *mut _);
    let path = monitor_profile_path(hwnd)?;
    let data = std::fs::read(&path).ok()?;
    Some((data, path))
}

/// Monitor ICC profile bytes for a specific display (delegates to the host window's monitor on Windows).
pub fn monitor_icc_profile_bytes_for_display(_display_id: u32) -> Option<(Vec<u8>, PathBuf)> {
    monitor_icc_profile_bytes()
}

// MARK: FolderWatch

struct StopEvent(HANDLE);

unsafe impl Send for StopEvent {}
unsafe impl Sync for StopEvent {}

impl Drop for StopEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct ChangeNotification(HANDLE);

impl Drop for ChangeNotification {
    fn drop(&mut self) {
        unsafe {
            let _ = FindCloseChangeNotification(self.0);
        }
    }
}

pub struct FolderWatch(Arc<StopEvent>);

impl FolderWatch {
    pub fn start(folder: &Path, mut changed: impl FnMut() + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(StopEvent(unsafe { CreateEventW(None, true, false, PCWSTR::null())? }));
        let worker_stop = stop.clone();
        let folder = folder.to_path_buf();
        std::thread::Builder::new().name("filmstrip-folder".into()).spawn(move || unsafe {
            let watch = FindFirstChangeNotificationW(
                pcwstr(&wide(&folder)),
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
            )
            .ok()
            .map(ChangeNotification);
            changed();
            let Some(watch) = watch else { return };
            let handles = [worker_stop.0, watch.0];
            loop {
                if WaitForMultipleObjects(&handles, false, INFINITE) != WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                    return;
                }
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    if FindNextChangeNotification(watch.0).is_err() {
                        return;
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    let delay = deadline.saturating_duration_since(Instant::now()).as_millis().min(200) as u32;
                    match WaitForMultipleObjects(&handles, false, delay) {
                        WAIT_TIMEOUT => break,
                        result if result == WAIT_EVENT(WAIT_OBJECT_0.0 + 1) => {}
                        _ => return,
                    }
                }
                changed();
            }
        })?;
        Ok(Self(stop))
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.0.0);
        }
    }
}
