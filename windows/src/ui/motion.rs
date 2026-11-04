//! Small motion (about 140 ms): rows slide to their new places. Off when
//! Windows' "Show animations" is off (`SPI_GETCLIENTAREAANIMATION`). It runs
//! on the app's timed tasks and stops when it ends; nothing runs while idle.

use std::time::Duration;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const DURATION: Duration = Duration::from_millis(140);

/// Whether the system shows animations in windows.
pub fn enabled() -> bool {
    let mut on = BOOL(1);
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok();
    !ok || on.as_bool()
}

/// Ease-out: fast, then settling.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}
