//! One blocking OS wait, no idle polling. Only the worker scans; UI updates
//! are supplied by the caller. A stop event cancels even an idle watch.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Threading::*;
use windows::core::PCWSTR;

use crate::platform::{pcwstr, wide};

struct StopEvent(HANDLE);

// Kernel event handles may be signalled and waited on from different threads.
// The Arc keeps this handle alive until both the owner and worker release it.
unsafe impl Send for StopEvent {}
unsafe impl Sync for StopEvent {}

impl Drop for StopEvent {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.0); }
    }
}

struct ChangeNotification(HANDLE);

impl Drop for ChangeNotification {
    fn drop(&mut self) {
        unsafe { let _ = FindCloseChangeNotification(self.0); }
    }
}

pub struct FolderWatch(Arc<StopEvent>);

impl FolderWatch {
    pub fn start(folder: &Path, mut changed: impl FnMut() + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(StopEvent(unsafe { CreateEventW(None, true, false, PCWSTR::null())? }));
        let worker_stop = stop.clone();
        let folder = folder.to_path_buf();
        std::thread::Builder::new().name("filmstrip-folder".into()).spawn(move || unsafe {
            // Arm before the first scan. Also observe size/write changes so a
            // partial copy's failed thumbnail is retried once the file changes.
            let watch = FindFirstChangeNotificationW(pcwstr(&wide(&folder)), false,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE)
                .ok().map(ChangeNotification);
            changed();
            let Some(watch) = watch else { return };
            let handles = [worker_stop.0, watch.0];
            loop {
                if WaitForMultipleObjects(&handles, false, INFINITE) != WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                    return;
                }
                // Debounce bursts for 200 ms, capped at 1 s so a continuing
                // import cannot postpone updates indefinitely.
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    if FindNextChangeNotification(watch.0).is_err() { return; }
                    if Instant::now() >= deadline { break; }
                    let delay = deadline.saturating_duration_since(Instant::now()).as_millis().min(200) as u32;
                    match WaitForMultipleObjects(&handles, false, delay) {
                        WAIT_TIMEOUT => break,
                        result if result == WAIT_EVENT(WAIT_OBJECT_0.0 + 1) => {},
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
        unsafe { let _ = SetEvent(self.0.0); }
    }
}
