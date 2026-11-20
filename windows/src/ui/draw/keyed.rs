//! Text boxes whose background is keyed out, so the marks their window
//! paints under them (the rule, the pencil, Light table's highlighter) show
//! through, as they do over the Mac's fields: an opaque EDIT hid the parts
//! its box covered. The box is `WS_EX_LAYERED`; its background is the
//! colour behind it one step off, so the text's antialiasing is against
//! that colour, as before. The keyed-out pixels let the pointer through to
//! the window beneath, which hands presses there back to the box.

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::super::theme::{self, Color};

thread_local! {
    /// When and where (in the box) a press was last handed on: a second one
    /// soon after, nearby, is its double click.
    static LAST: Cell<Option<(HWND, i32, POINT)>> = const { Cell::new(None) };
}

/// The style a keyed box is made with.
pub const STYLE: WINDOW_EX_STYLE = WS_EX_LAYERED;

/// Keys `field` out in `background` (a GDI colour) and returns the colour
/// to paint its background with (for `WM_CTLCOLOREDIT`). Call it when made
/// too: a layered window isn't shown until it's keyed.
pub fn key(field: HWND, background: Color) -> Color {
    let key = background ^ 0x0001_0000;
    let (mut now, mut flags) = (COLORREF(0), LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
    unsafe {
        let known = GetLayeredWindowAttributes(field, Some(&mut now), None, Some(&mut flags)).is_ok();
        if !known || flags.0 & LWA_COLORKEY.0 == 0 || now.0 != key {
            let _ = SetLayeredWindowAttributes(field, COLORREF(key), 0, LWA_COLORKEY);
        }
    }
    key
}

/// `WM_CTLCOLOREDIT` for a keyed box: `text` on `background`.
pub fn color(dc: HDC, field: HWND, text: Color, background: Color) -> LRESULT {
    let key = key(field, background);
    unsafe {
        SetTextColor(dc, COLORREF(text));
        SetBkColor(dc, COLORREF(key));
    }
    LRESULT(theme::brush(key).0 as isize)
}

/// `at` (client pixels of `from`) in `field`, if it's shown and `at` is in its box.
pub fn hit(field: HWND, from: HWND, at: POINT) -> Option<POINT> {
    if field.is_invalid() || !unsafe { IsWindowVisible(field) }.as_bool() {
        return None;
    }
    let mut p = at;
    let mut r = RECT::default();
    unsafe {
        let _ = MapWindowPoints(from, field, std::slice::from_mut(&mut p));
        let _ = GetClientRect(field, &mut r);
    }
    (p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom).then_some(p)
}

/// Whether `field` is a keyed box and `at` (screen pixels) is in it: a
/// press there is the field's, though it reaches the window beneath.
pub fn covers(field: HWND, at: POINT) -> bool {
    let mut r = RECT::default();
    unsafe {
        let layered = (GetWindowLongW(field, GWL_EXSTYLE) as u32) & STYLE.0 != 0;
        layered && GetWindowRect(field, &mut r).is_ok() && at.x >= r.left && at.x < r.right && at.y >= r.top && at.y < r.bottom
    }
}

/// The pointer, in `from`'s client pixels.
fn pointer(from: HWND) -> POINT {
    let mut at = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut at);
        let _ = ScreenToClient(from, &mut at);
    }
    at
}

/// Handles the pointer messages `from` gets over `field`'s keyed-out
/// pixels: presses and double clicks go to `field` (focused), the
/// I-beam shows, and the context menu is the box's. None if `msg` isn't
/// one of those, or isn't over the box.
pub fn pass_on(field: HWND, from: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let at_lparam = || POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
    match msg {
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
            hit(field, from, pointer(from))?;
            unsafe {
                SetCursor(LoadCursorW(None, IDC_IBEAM).unwrap_or_default());
            }
            Some(LRESULT(1))
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let at = hit(field, from, at_lparam())?;
            press(field, at, wparam);
            Some(LRESULT(0))
        }
        WM_CONTEXTMENU => {
            // From the keyboard (-1, -1) it's the window's own.
            if lparam.0 as i32 == -1 {
                return None;
            }
            hit(field, from, pointer(from))?;
            unsafe { SendMessageW(field, WM_CONTEXTMENU, WPARAM(field.0 as usize), lparam) };
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// A press at `at` (in `field`) that came through to the window beneath.
fn press(field: HWND, at: POINT, wparam: WPARAM) {
    let time = unsafe { GetMessageTime() };
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    let double = LAST.with(Cell::get).is_some_and(|(was, then, p)| {
        was == field && (time.wrapping_sub(then) as u32) <= unsafe { GetDoubleClickTime() } && (p.x - at.x).abs() <= cx / 2 && (p.y - at.y).abs() <= cy / 2
    });
    LAST.with(|c| c.set(if double { None } else { Some((field, time, at)) }));
    let lparam = LPARAM(((at.y as u16 as isize) << 16) | at.x as u16 as isize);
    unsafe {
        let _ = SetFocus(field);
        SendMessageW(field, if double { WM_LBUTTONDBLCLK } else { WM_LBUTTONDOWN }, wparam, lparam);
    }
}
