//! Adjustments | Presets at the top of the column: the switch between its
//! two panels, where they are (the Mac switches them from the toolbar). The
//! title bar's panel button hides and shows the column itself.

use std::sync::Once;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::draw::{self, Canvas, RectF};
use super::theme;
use super::{create as create_control, id, instance, with_app};
use crate::model::Pane;

/// The band's height, in DIPs: the switch and 8 DIPs above and below.
pub const HEIGHT: f64 = 48.0;

pub fn create(main: HWND, font: HFONT) -> HWND {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: instance(),
            lpszClassName: w!("NegsPanes"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
    let band = create_control(w!("NegsPanes"), "Panels", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
    let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    create_control(w!("BUTTON"), "Adjustments", style | WS_GROUP, WINDOW_EX_STYLE(0), band, id::ADJUSTMENTS, font);
    create_control(w!("BUTTON"), "Presets", style, WINDOW_EX_STYLE(0), band, id::PRESETS, font);
    band
}

fn button(band: HWND, command: u16) -> HWND {
    unsafe { GetDlgItem(band, command as i32).unwrap_or_default() }
}

/// Marks the panel the column shows.
pub fn sync(band: HWND, presets: bool) {
    for (command, on) in [(id::ADJUSTMENTS, !presets), (id::PRESETS, presets)] {
        unsafe {
            SendMessageW(button(band, command), BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
        }
    }
}

pub fn set_font(band: HWND, font: HFONT) {
    for command in [id::ADJUSTMENTS, id::PRESETS] {
        unsafe {
            SendMessageW(button(band, command), WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }
}

/// The switch across the column, inset like the adjustments' own.
fn layout(band: HWND) {
    let s = draw::dpi_of(band) as f64;
    let px = |dip: f64| (dip * s).round() as i32;
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(band, &mut client);
    }
    let inner = (client.right - 2 * px(16.0)).max(0);
    let half = inner / 2;
    for (i, command) in [id::ADJUSTMENTS, id::PRESETS].into_iter().enumerate() {
        let x = px(16.0) + half * i as i32;
        let width = if i == 0 { half } else { inner - half };
        unsafe {
            let _ = MoveWindow(button(band, command), x, px(8.0), width, px(32.0), true);
        }
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_COMMAND => {
                let (command, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if code == BN_CLICKED {
                    let pane = if command == id::PRESETS { Pane::Presets } else { Pane::Adjustments };
                    with_app(|app| app.model.show_pane(pane));
                }
                LRESULT(0)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                // The column's colour, with a hairline under the switch.
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let p = theme::palette();
                let s = draw::dpi_of(hwnd);
                let mut c = Canvas::new(dc, client);
                c.clear(p.panel);
                let line = s.max(1.0).floor();
                c.round_rect(RectF::new(0.0, c.h as f32 - line, c.w as f32, line), 0.0, p.border);
                c.present(dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
