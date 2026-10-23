//! What the canvas column shows with nothing open (`EmptyState` in
//! `NegsApp.swift`), and the status bar's Show Original button.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::{App, create, id, post, with_app};
use crate::platform::wide;

#[derive(Default)]
pub struct Empty {
    icon: HWND,
    title: HWND,
    tagline: HWND,
    button: HWND,
    hint: HWND,
    recent_label: HWND,
    recent: Vec<HWND>,
    big_glyph: Option<HFONT>,
    highlighted: bool,
    shown_recent: Vec<std::path::PathBuf>,
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsEmpty"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

fn big_glyph(dpi: u32) -> HFONT {
    let mut lf = LOGFONTW { lfHeight: -(54 * dpi as i32 / 96), lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
    let face = wide("Segoe MDL2 Assets");
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    unsafe { CreateFontIndirectW(&lf) }
}

pub fn build(app: &mut App) {
    let parent = app.empty;
    let fonts = &app.fonts;
    let center = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0);
    let glyph = big_glyph(app.dpi);
    let e = &mut app.empty_view;
    e.icon = create(w!("STATIC"), "\u{E8B9}", center, WINDOW_EX_STYLE(0), parent, 0, glyph);
    e.big_glyph = Some(glyph);
    e.title = create(w!("STATIC"), "Open a Photograph", center, WINDOW_EX_STYLE(0), parent, 0, fonts.title);
    e.tagline = create(w!("STATIC"), "Explore film stocks, shape the print, and make it your own.", center, WINDOW_EX_STYLE(0), parent, 0, fonts.ui);
    e.button = create(w!("BUTTON"), "Open Photograph…", WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id::EMPTY_OPEN, fonts.ui);
    e.hint = create(w!("STATIC"), "Or drop a RAW, TIFF, PNG, or JPEG file here", center, WINDOW_EX_STYLE(0), parent, 0, fonts.small);
    e.recent_label = create(w!("STATIC"), "Recent", WS_CHILD | WINDOW_STYLE(SS_LEFT.0), WINDOW_EX_STYLE(0), parent, 0, fonts.bold);
    sync_recent(app);
    layout(app);
}

pub fn dpi_changed(app: &mut App) {
    let glyph = big_glyph(app.dpi);
    let fonts = &app.fonts;
    let e = &mut app.empty_view;
    unsafe {
        SendMessageW(e.icon, WM_SETFONT, WPARAM(glyph.0 as usize), LPARAM(1));
        if let Some(old) = e.big_glyph.replace(glyph) {
            let _ = DeleteObject(old);
        }
        for (c, f) in [(e.title, fonts.title), (e.tagline, fonts.ui), (e.button, fonts.ui), (e.hint, fonts.small), (e.recent_label, fonts.bold)] {
            SendMessageW(c, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
        }
        for &c in &e.recent {
            SendMessageW(c, WM_SETFONT, WPARAM(fonts.ui.0 as usize), LPARAM(1));
        }
    }
    layout(app);
}

/// Up to five recent photographs as links, the full path as their tooltip.
pub fn sync_recent(app: &mut App) {
    let recent: Vec<_> = app.model.prefs.recent_files.iter().take(5).cloned().collect();
    if recent == app.empty_view.shown_recent && !app.empty_view.recent.is_empty() == !recent.is_empty() {
        return;
    }
    let e = &mut app.empty_view;
    for c in e.recent.drain(..) {
        unsafe {
            let _ = DestroyWindow(c);
        }
    }
    for (i, path) in recent.iter().enumerate() {
        let name = crate::model::file_name(path).replace('&', "&amp;").replace('<', "&lt;");
        let link = create(WC_LINK, &format!("<a>{name}</a>"), WS_CHILD | WS_VISIBLE | WS_TABSTOP, WINDOW_EX_STYLE(0), app.empty, id::EMPTY_RECENT + i as u16, app.fonts.ui);
        super::dialogs::tooltip(link, &path.display().to_string());
        e.recent.push(link);
    }
    e.shown_recent = recent;
    unsafe {
        let _ = ShowWindow(e.recent_label, if e.recent.is_empty() { SW_HIDE } else { SW_SHOWNA });
    }
    layout(app);
}

pub fn layout(app: &mut App) {
    let e = &app.empty_view;
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(app.empty, &mut r);
    }
    let px = |dip: f64| app.px(dip);
    let width = px(460.0).min(r.right - px(40.0)).max(px(200.0));
    let x = (r.right - width) / 2;
    let rows = [(e.icon, 70.0), (e.title, 40.0), (e.tagline, 24.0), (e.button, 0.0), (e.hint, 22.0)];
    let recent = if e.recent.is_empty() { 0.0 } else { 34.0 + 24.0 * e.recent.len() as f64 };
    let total = 70.0 + 40.0 + 24.0 + 46.0 + 22.0 + recent;
    let mut y = ((r.bottom - px(total)) / 2).max(px(20.0));
    unsafe {
        for (hwnd, height) in rows {
            if hwnd == e.button {
                let bw = px(170.0);
                let _ = MoveWindow(hwnd, (r.right - bw) / 2, y + px(8.0), bw, px(32.0), true);
                y += px(46.0);
            } else {
                let _ = MoveWindow(hwnd, x, y, width, px(height), true);
                y += px(height);
            }
        }
        if !e.recent.is_empty() {
            y += px(14.0);
            let left = (r.right - px(260.0)) / 2;
            let _ = MoveWindow(e.recent_label, left, y, px(260.0), px(20.0), true);
            y += px(22.0);
            for &link in &e.recent {
                let _ = MoveWindow(link, left, y, px(260.0), px(22.0), true);
                y += px(24.0);
            }
        }
        let _ = InvalidateRect(app.empty, None, true);
    }
}

pub fn highlight(app: &mut App, on: bool) {
    app.empty_view.highlighted = on;
    unsafe {
        let _ = InvalidateRect(app.empty, None, true);
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                with_app(layout);
                LRESULT(0)
            }
            WM_COMMAND => {
                let command = (wparam.0 & 0xffff) as u16;
                with_app(|app| app.command(command));
                LRESULT(0)
            }
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.code == NM_CLICK || header.code == NM_RETURN {
                    let command = header.idFrom as u16;
                    with_app(|app| app.command(command));
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let grey = with_app(|app| {
                    let control = HWND(lparam.0 as *mut _);
                    control == app.empty_view.tagline || control == app.empty_view.hint || control == app.empty_view.icon
                })
                .unwrap_or(false);
                SetTextColor(dc, if grey { COLORREF(0x707070) } else { COLORREF(GetSysColor(COLOR_WINDOWTEXT)) });
                LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                FillRect(dc, &ps.rcPaint, GetSysColorBrush(COLOR_WINDOW));
                if with_app(|app| app.empty_view.highlighted).unwrap_or(false) {
                    let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as i32;
                    let accent = super::canvas::accent_color();
                    let pen = CreatePen(PS_SOLID, 3 * dpi / 96, COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16));
                    let old = SelectObject(dc, pen);
                    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let inset = 8 * dpi / 96;
                    let _ = RoundRect(dc, r.left + inset, r.top + inset, r.right - inset, r.bottom - inset, 24 * dpi / 96, 24 * dpi / 96);
                    SelectObject(dc, old_brush);
                    SelectObject(dc, old);
                    let _ = DeleteObject(pen);
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Press and hold to compare with the photograph before developing. The
/// decode starts when the pointer arrives; the keyboard (and so screen
/// readers) toggle it instead.
pub fn subclass_original_button(button: HWND) {
    unsafe {
        let _ = SetWindowSubclass(button, Some(original_subclass), 1, 0);
    }
    super::dialogs::tooltip(button, "Hold to show the original (\\ or O)");
}

extern "system" fn original_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, data: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE if data == 0 => {
                let _ = SetWindowSubclass(hwnd, Some(original_subclass), 1, 1);
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                post(|app: &mut App| app.model.prepare_original());
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_MOUSELEAVE => {
                let _ = SetWindowSubclass(hwnd, Some(original_subclass), 1, 0);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                with_app(|app| app.model.hold_original(true));
                SetCapture(hwnd);
                let _ = SetWindowSubclass(hwnd, Some(original_subclass), 1, 2);
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED if data == 2 => {
                let _ = SetWindowSubclass(hwnd, Some(original_subclass), 1, 1);
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                with_app(|app| app.model.hold_original(false));
                LRESULT(0)
            }
            WM_KEYUP if wparam.0 == 0x20 || wparam.0 == 0x0d => {
                with_app(|app| {
                    let held = !app.model.holding_original;
                    app.model.hold_original(held);
                });
                LRESULT(0)
            }
            WM_KEYDOWN | WM_CHAR if wparam.0 == 0x20 || wparam.0 == 0x0d => LRESULT(0),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Buttons inside the status bar report to it; pass them on.
pub fn subclass_status(status: HWND) {
    unsafe {
        let _ = SetWindowSubclass(status, Some(status_subclass), 1, 0);
    }
}

extern "system" fn status_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        if msg == WM_COMMAND {
            return SendMessageW(super::main_hwnd(), msg, wparam, lparam);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
