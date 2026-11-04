//! Small motion (about 140 ms): panels fade in, rows slide to their new
//! places. Off when Windows' "Show animations" is off
//! (`SPI_GETCLIENTAREAANIMATION`). Each runs on the app's timed tasks and
//! stops when it ends; nothing runs while idle.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{App, after};

pub const DURATION: Duration = Duration::from_millis(140);
const FRAME: Duration = Duration::from_millis(15);

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

/// Fades a child window in: layered while it runs, then plain again.
pub fn fade_in(hwnd: HWND) {
    if !enabled() || hwnd.is_invalid() {
        return;
    }
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as i32);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }
    let start = Instant::now();
    let raw = hwnd.0 as isize;
    fade_step(raw, start);
}

fn fade_step(raw: isize, start: Instant) {
    let hwnd = HWND(raw as *mut _);
    let t = start.elapsed().as_secs_f64() / DURATION.as_secs_f64();
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        if t >= 1.0 {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as i32));
            // Drawn at once: no longer layered, the window shows nothing of
            // its own until it paints, a flash at the fade's end.
            let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME | RDW_UPDATENOW);
            return;
        }
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (ease(t) * 255.0) as u8, LWA_ALPHA);
    }
    after(FRAME, move |_: &mut App| fade_step(raw, start));
}
