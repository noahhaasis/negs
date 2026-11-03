//! The toolbar and status bar painted from the palette. Both stay the
//! common controls (hit testing, tooltips, hot tracking, accessibility);
//! only `WM_PAINT` is ours, so they look right in light, dark and high
//! contrast.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::draw::{self, Canvas, RectF};
use super::theme;

/// The toolbar's glyphs by image index: Open, Film Strip, Fit, Actual Size,
/// Rotate, Crop, Export, Adjustments, Presets.
pub const TOOL_GLYPHS: [char; 9] = ['\u{E838}', '\u{E8B9}', '\u{E9A6}', '\u{E71E}', '\u{E7AD}', '\u{E7A8}', '\u{E78C}', '\u{E9E9}', '\u{E81E}'];

/// Paints `toolbar` with `glyphs` (the font, updated on DPI changes).
pub fn subclass_toolbar(toolbar: HWND, glyphs: HFONT) {
    unsafe {
        let _ = SetWindowSubclass(toolbar, Some(toolbar_subclass), 0x4e54, glyphs.0 as usize);
    }
}

extern "system" fn toolbar_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, font: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_toolbar(hwnd, dc, HFONT(font as *mut _));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PRINTCLIENT => {
                paint_toolbar(hwnd, HDC(wparam.0 as *mut _), HFONT(font as *mut _));
                LRESULT(0)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn paint_toolbar(hwnd: HWND, dc: HDC, font: HFONT) {
    let p = theme::palette();
    let s = draw::dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut c = Canvas::new(dc, client);
        c.clear(p.panel);
        let count = SendMessageW(hwnd, TB_BUTTONCOUNT, WPARAM(0), LPARAM(0)).0 as usize;
        let hot = SendMessageW(hwnd, TB_GETHOTITEM, WPARAM(0), LPARAM(0)).0;
        for i in 0..count {
            let mut button = TBBUTTON::default();
            SendMessageW(hwnd, TB_GETBUTTON, WPARAM(i), LPARAM(&mut button as *mut _ as isize));
            let mut r = RECT::default();
            SendMessageW(hwnd, TB_GETITEMRECT, WPARAM(i), LPARAM(&mut r as *mut _ as isize));
            if button.fsState & TBSTATE_HIDDEN as u8 != 0 {
                continue;
            }
            let rf = RectF::of(r);
            if button.fsStyle & BTNS_SEP as u8 != 0 {
                let x = (rf.x + rf.w / 2.0).round();
                c.round_rect(RectF::new(x, rf.y + rf.h * 0.25, s.max(1.0), rf.h * 0.5), 0.0, p.border);
                continue;
            }
            let enabled = button.fsState & TBSTATE_ENABLED as u8 != 0;
            let checked = button.fsState & TBSTATE_CHECKED as u8 != 0;
            let pressed = button.fsState & TBSTATE_PRESSED as u8 != 0;
            let is_hot = hot == i as isize && enabled;
            let face = rf.inset(2.0 * s, 2.0 * s);
            if pressed {
                c.round_rect(face, 4.0 * s, p.subtle_pressed);
            } else if checked {
                c.round_rect(face, 4.0 * s, if is_hot { theme::mix(p.subtle_hover, p.accent, 0.22) } else { theme::mix(p.panel, p.accent, 0.16) });
            } else if is_hot {
                c.round_rect(face, 4.0 * s, p.subtle_hover);
            }
            let glyph = TOOL_GLYPHS.get(button.iBitmap as usize).copied().unwrap_or(' ');
            let ink = if !enabled { p.disabled } else if checked { theme::mix(p.text, p.accent, 0.6) } else { p.text };
            c.text(&glyph.to_string(), r, font, ink, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        c.present(dc);
    }
}

/// Paints the status bar: its texts in the secondary grey on the panel,
/// a hairline above, and a quiet size grip.
pub fn subclass_status(status: HWND) {
    unsafe {
        let _ = SetWindowSubclass(status, Some(status_subclass), 0x4e53, 0);
    }
}

extern "system" fn status_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_status(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn paint_status(hwnd: HWND, dc: HDC) {
    let p = theme::palette();
    let s = draw::dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut c = Canvas::new(dc, client);
        c.clear(p.panel);
        c.round_rect(RectF::new(0.0, 0.0, c.w as f32, s.max(1.0).floor()), 0.0, p.border);
        let mut edges = [0i32; 8];
        let parts = (SendMessageW(hwnd, SB_GETPARTS, WPARAM(edges.len()), LPARAM(edges.as_mut_ptr() as isize)).0 as usize).min(edges.len());
        let font = draw::font_of(hwnd);
        let mut left = 0;
        for (i, &edge) in edges[..parts].iter().enumerate() {
            let right = if edge < 0 { client.right } else { edge };
            let length = SendMessageW(hwnd, SB_GETTEXTLENGTHW, WPARAM(i), LPARAM(0)).0 as usize & 0xffff;
            if length > 0 {
                let mut text = vec![0u16; length + 1];
                SendMessageW(hwnd, SB_GETTEXTW, WPARAM(i), LPARAM(text.as_mut_ptr() as isize));
                let text = String::from_utf16_lossy(&text[..length]);
                let pad = (8.0 * s) as i32;
                let r = RECT { left: left + pad, top: 0, right: right - pad, bottom: client.bottom };
                c.text(&text, r, font, if i == 0 { p.text } else { p.secondary }, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | if i == 0 { DT_LEFT } else { DT_RIGHT });
            }
            left = right;
        }
        let parent = GetParent(hwnd).unwrap_or_default();
        if !IsZoomed(parent).as_bool() {
            // The size grip: six dots.
            let (gx, gy, step) = (c.w as f32 - 5.0 * s, c.h as f32 - 5.0 * s, 4.0 * s);
            for (i, j) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (0, 2)] {
                c.circle(gx - i as f32 * step, gy - j as f32 * step, 0.9 * s, p.strong_border);
            }
        }
        c.present(dc);
    }
}
