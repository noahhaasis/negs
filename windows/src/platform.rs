//! Small Win32 helpers shared by the app: wide strings, file identity,
//! atomic replacement, timing and the job object that owns the engine's
//! processes.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::PCWSTR;

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

/// Volume serial number and file index: the file's identity, which handles
/// case differences and hard links.
pub fn file_identity(path: &Path) -> Option<(u32, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    Some((info.dwVolumeSerialNumber, (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64))
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

pub fn qpc() -> i64 {
    let mut t = 0;
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
    }
    t
}

pub fn qpc_frequency() -> i64 {
    let mut f = 0;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut f);
    }
    f
}

pub fn qpc_ms(from: i64, to: i64) -> f64 {
    static FREQUENCY: OnceLock<f64> = OnceLock::new();
    let f = *FREQUENCY.get_or_init(|| {
        let mut f = 0;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut f);
        }
        f as f64
    });
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
