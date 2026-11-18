//! The title bar (IMPLEMENTATION.md §3, `TitleBar.swift`): 38 DIP of the
//! theme's `bg` across the window, painted with Direct2D, holding a compact
//! menu row on the left (File Edit Photo View Help), the file name in
//! Newsreader 15 600 `paper` after it ("Negs" with nothing open), and
//! Windows' own minimise, maximise and close on the right, drawn in the
//! theme. There is no toolbar: its items are in the menus (§5.6) and the
//! status line. It is drawn in the client area (`WM_NCCALCSIZE` removes the
//! standard caption; the side and bottom frame stay the system's).
//!
//! The Grid · Photo · Compare switch stands before the caption buttons
//! whenever a photograph is open (`view_switch`), the name ending 16 DIPs
//! before it.
//!
//! The menu row is a toolbar control with a button per menu, so screen
//! readers see "File", "Edit" … as buttons with menus. Its menus are the
//! window's old menu bar's popups, opened with `TrackPopupMenuEx`; a
//! message hook moves between them on Left, Right and the pointer, as a
//! menu bar does. Alt, F10 and Alt+letter arrive as `SC_KEYMENU`.
//!
//! The caption buttons answer `WM_NCHITTEST` with `HTMINBUTTON`,
//! `HTMAXBUTTON` and `HTCLOSE`, so Windows 11 offers snap layouts on the
//! maximise button; their clicks are handled here, since the system would
//! draw classic buttons over them.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::d2d::{self, Ink, Painter, TextStyle};
use super::draw::{self, Canvas, RectF};
use super::theme::{self, Tokens};
use crate::platform::wide;

/// The band's height, in DIPs (`Theme.Size.titleBar`).
pub const HEIGHT: f64 = 38.0;
const CAPTION_BUTTON: f64 = 46.0;
/// Before the menu row, and between it and the file name.
const MENU_INSET: f64 = 6.0;
const NAME_GAP: f64 = 14.0;
const MENU_FIRST: u16 = 800;
/// The file name (§2.2: Newsreader 15, 600, `paper`).
pub const NAME: TextStyle = TextStyle::serif(15.0, 600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caption {
    Minimize,
    Maximize,
    Close,
}

thread_local! {
    static HOT: Cell<Option<Caption>> = const { Cell::new(None) };
    static PRESSED: Cell<Option<Caption>> = const { Cell::new(None) };
    static MENUBAR: Cell<isize> = const { Cell::new(0) };
    static MENU: Cell<isize> = const { Cell::new(0) };
    /// While a menu is open: its index, the one to open next (and whether
    /// from the keyboard), and whether the selection is in a submenu or on
    /// an item that opens one.
    static OPEN: Cell<Option<usize>> = const { Cell::new(None) };
    static NEXT: Cell<Option<(usize, bool)>> = const { Cell::new(None) };
    static IN_SUBMENU: Cell<bool> = const { Cell::new(false) };
    static ON_POPUP_ITEM: Cell<bool> = const { Cell::new(false) };
    /// Alt or F10 pressed alone: the menu bar has the keyboard, at this menu.
    static KEYS: Cell<Option<usize>> = const { Cell::new(None) };
    static HOOK: Cell<isize> = const { Cell::new(0) };
    /// A menu opened with Alt+letter: Alt's release must not close it.
    static ALT_HELD: Cell<bool> = const { Cell::new(false) };
    /// Where the file name goes, in pixels.
    static TITLE_AREA: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
    /// What the band says: the file name, or "Negs".
    static TITLE: RefCell<String> = RefCell::new(String::from("Negs"));
    /// The name as it fits the area last painted: (name, width) → shown.
    static FITTED: RefCell<(String, i32, String)> = const { RefCell::new((String::new(), 0, String::new())) };
    /// Where the inspector column starts (0: hidden): its top rule runs
    /// along the band's foot there (round 25).
    static INSPECTOR_EDGE: Cell<i32> = const { Cell::new(0) };
    /// The menu row's font (after a DPI change too).
    static FONT: Cell<isize> = const { Cell::new(0) };
}

/// The menu row's font.
pub fn set_fonts(menus: HFONT) {
    FONT.with(|f| f.set(menus.0 as isize));
}

fn px(hwnd: HWND, dip: f64) -> i32 {
    (dip * unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0).round() as i32
}

/// The band's height for `main` (in full screen too, without caption buttons).
pub fn height(main: HWND) -> i32 {
    px(main, HEIGHT)
}

fn full_screen(main: HWND) -> bool {
    unsafe { GetWindowLongW(main, GWL_STYLE) as u32 & WS_CAPTION.0 != WS_CAPTION.0 }
}

/// Sets what the band says and repaints it when that changed.
pub fn set_title(main: HWND, title: &str) {
    let changed = TITLE.with(|t| {
        let mut t = t.borrow_mut();
        if *t == title {
            return false;
        }
        *t = title.to_string();
        true
    });
    if changed {
        invalidate(main);
    }
}

/// Where the inspector column starts, or 0 while it's hidden.
pub fn set_inspector_edge(main: HWND, x: i32) {
    if INSPECTOR_EDGE.with(|e| e.replace(x)) != x {
        invalidate(main);
    }
}

/// The menu bar, from the popups of `menu` (the window has no menu bar).
pub fn create_menubar(main: HWND, menu: HMENU, font: HFONT) -> HWND {
    MENU.with(|m| m.set(menu.0 as isize));
    let bar = super::create(
        TOOLBARCLASSNAMEW,
        "Menu bar",
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(TBSTYLE_FLAT | TBSTYLE_LIST | CCS_NODIVIDER as u32 | CCS_NOPARENTALIGN as u32 | CCS_NORESIZE as u32),
        WINDOW_EX_STYLE(0),
        main,
        0,
        font,
    );
    MENUBAR.with(|m| m.set(bar.0 as isize));
    unsafe {
        SendMessageW(bar, TB_BUTTONSTRUCTSIZE, WPARAM(std::mem::size_of::<TBBUTTON>()), LPARAM(0));
        SendMessageW(bar, TB_SETEXTENDEDSTYLE, WPARAM(0), LPARAM(0x80 /* TBSTYLE_EX_DOUBLEBUFFER */));
        SendMessageW(bar, TB_SETBITMAPSIZE, WPARAM(0), LPARAM(0));
        let count = GetMenuItemCount(menu).max(0);
        let mut titles: Vec<Vec<u16>> = vec![];
        for i in 0..count {
            let mut text = [0u16; 64];
            let n = GetMenuStringW(menu, i as u32, Some(&mut text), MF_BYPOSITION) as usize;
            titles.push(wide(String::from_utf16_lossy(&text[..n])));
        }
        let buttons: Vec<TBBUTTON> = titles
            .iter()
            .enumerate()
            .map(|(i, title)| TBBUTTON {
                iBitmap: I_IMAGENONE,
                idCommand: (MENU_FIRST + i as u16) as i32,
                fsState: TBSTATE_ENABLED as u8,
                fsStyle: (BTNS_BUTTON | BTNS_AUTOSIZE | BTNS_SHOWTEXT | BTNS_WHOLEDROPDOWN) as u8,
                iString: title.as_ptr() as isize,
                ..Default::default()
            })
            .collect();
        SendMessageW(bar, TB_ADDBUTTONSW, WPARAM(buttons.len()), LPARAM(buttons.as_ptr() as isize));
        let _ = SetWindowSubclass(bar, Some(menubar_subclass), 0x4e4d, 0);
    }
    set_metrics(main, bar);
    bar
}

/// Button padding for the DPI: compact titles, 28 DIP high.
pub fn set_metrics(main: HWND, bar: HWND) {
    unsafe {
        SendMessageW(bar, TB_SETPADDING, WPARAM(0), LPARAM(((px(main, 16.0) & 0xffff) | (px(main, 12.0) << 16)) as isize));
        SendMessageW(bar, TB_AUTOSIZE, WPARAM(0), LPARAM(0));
    }
}

fn size_of(bar: HWND) -> SIZE {
    let mut size = SIZE::default();
    unsafe {
        SendMessageW(bar, TB_GETMAXSIZE, WPARAM(0), LPARAM(&mut size as *mut _ as isize));
    }
    size
}

/// Places the menu row; the file name goes between it and the caption
/// buttons.
pub fn layout(main: HWND, bar: HWND) {
    let band = height(main);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
        let menus = size_of(bar);
        let x = px(main, MENU_INSET);
        let _ = MoveWindow(bar, x, (band - menus.cy) / 2, menus.cx, menus.cy, true);
        let captions = if full_screen(main) { 0 } else { 3 * px(main, CAPTION_BUTTON) };
        let views = super::view_switch::layout(main, client.right - captions, band);
        let right = if views < client.right - captions { views - px(main, super::view_switch::NAME_GAP) } else { client.right - captions - px(main, 16.0) };
        let area = RECT { left: x + menus.cx + px(main, NAME_GAP), top: 0, right, bottom: band };
        TITLE_AREA.with(|t| t.set(area));
        let _ = InvalidateRect(main, Some(&RECT { left: 0, top: 0, right: client.right, bottom: band }), false);
    }
}

/// The caption buttons a window has: all three, or Close alone for a
/// window that can't be minimised (Settings).
fn captions(hwnd: HWND) -> &'static [Caption] {
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    if style & WS_MINIMIZEBOX.0 != 0 { &[Caption::Minimize, Caption::Maximize, Caption::Close] } else { &[Caption::Close] }
}

fn caption_rect(main: HWND, which: Caption) -> RECT {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
    }
    let w = px(main, CAPTION_BUTTON);
    let shown = captions(main);
    let Some(at) = shown.iter().position(|&c| c == which) else { return RECT::default() };
    let index = (shown.len() - at) as i32;
    RECT { left: client.right - index * w, top: 0, right: client.right - (index - 1) * w, bottom: height(main) }
}

fn caption_at(main: HWND, x: i32, y: i32) -> Option<Caption> {
    if full_screen(main) {
        return None;
    }
    captions(main).iter().copied().find(|&c| {
        let r = caption_rect(main, c);
        x >= r.left && x < r.right && y >= r.top && y < r.bottom
    })
}

// MARK: Frame

/// `WM_NCCALCSIZE`: the client area takes the caption's place; the side and
/// bottom frame stay. None: let the system handle it.
pub fn nccalcsize(main: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if full_screen(main) {
        return None;
    }
    unsafe {
        // wParam TRUE: NCCALCSIZE_PARAMS, whose first rectangle is the
        // window's and becomes the client's; FALSE (at creation): that
        // rectangle alone.
        let rect = if wparam.0 != 0 { &mut (*(lparam.0 as *mut NCCALCSIZE_PARAMS)).rgrc[0] } else { &mut *(lparam.0 as *mut RECT) };
        let top = rect.top;
        let result = DefWindowProcW(main, WM_NCCALCSIZE, wparam, lparam);
        rect.top = top;
        if IsZoomed(main).as_bool() {
            // A maximised window reaches past the monitor by its frame.
            let dpi = GetDpiForWindow(main).max(96);
            rect.top += GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        }
        Some(result)
    }
}

/// `WM_NCHITTEST`: the top edge resizes, the caption buttons are themselves,
/// and the rest of the band drags the window.
pub fn nchittest(main: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let hit = DefWindowProcW(main, WM_NCHITTEST, wparam, lparam);
        if hit.0 as u32 != HTCLIENT || full_screen(main) {
            return hit;
        }
        let mut p = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
        let _ = ScreenToClient(main, &mut p);
        if p.y >= height(main) {
            return hit;
        }
        let dpi = GetDpiForWindow(main).max(96);
        let frame = GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        let sizable = GetWindowLongW(main, GWL_STYLE) as u32 & WS_THICKFRAME.0 != 0;
        if sizable && !IsZoomed(main).as_bool() && p.y < frame {
            let mut client = RECT::default();
            let _ = GetClientRect(main, &mut client);
            return LRESULT(if p.x < frame * 2 { HTTOPLEFT } else if p.x >= client.right - frame * 2 { HTTOPRIGHT } else { HTTOP } as isize);
        }
        LRESULT(match caption_at(main, p.x, p.y) {
            Some(Caption::Minimize) => HTMINBUTTON,
            Some(Caption::Maximize) => HTMAXBUTTON,
            Some(Caption::Close) => HTCLOSE,
            None => HTCAPTION,
        } as isize)
    }
}

fn caption_of(hit: WPARAM) -> Option<Caption> {
    match hit.0 as u32 {
        HTMINBUTTON => Some(Caption::Minimize),
        HTMAXBUTTON => Some(Caption::Maximize),
        HTCLOSE => Some(Caption::Close),
        _ => None,
    }
}

fn invalidate_captions(main: HWND) {
    let r = RECT { left: caption_rect(main, captions(main)[0]).left, top: 0, right: caption_rect(main, Caption::Close).right, bottom: height(main) };
    unsafe {
        let _ = InvalidateRect(main, Some(&r), false);
    }
}

/// The non-client mouse messages over the caption buttons. None: not ours.
pub fn ncmouse(main: HWND, msg: u32, wparam: WPARAM) -> Option<LRESULT> {
    unsafe {
        match msg {
            WM_NCMOUSEMOVE => {
                let hot = caption_of(wparam);
                if HOT.with(|h| h.replace(hot)) != hot {
                    invalidate_captions(main);
                }
                if hot.is_some() {
                    let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: main, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut track);
                }
                None
            }
            WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
                if HOT.with(|h| h.take()).is_some() | PRESSED.with(|p| p.take()).is_some() {
                    invalidate_captions(main);
                }
                None
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
                let which = caption_of(wparam)?;
                PRESSED.with(|p| p.set(Some(which)));
                invalidate_captions(main);
                Some(LRESULT(0))
            }
            WM_NCLBUTTONUP => {
                let which = caption_of(wparam)?;
                let pressed = PRESSED.with(|p| p.take());
                invalidate_captions(main);
                if pressed == Some(which) {
                    let command = match which {
                        Caption::Minimize => SC_MINIMIZE,
                        Caption::Maximize if IsZoomed(main).as_bool() => SC_RESTORE,
                        Caption::Maximize => SC_MAXIMIZE,
                        Caption::Close => SC_CLOSE,
                    };
                    let _ = PostMessageW(main, WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0));
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// Paints the band: `bg`, the file name and the caption buttons (the menu
/// row paints itself), and the inspector's top rule over its column.
pub fn paint(main: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(main);
    let band = height(main);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
    }
    let active = unsafe { GetForegroundWindow() } == main || unsafe { GetActiveWindow() } == main;
    let area = TITLE_AREA.with(Cell::get);
    let title = TITLE.with(|t| t.borrow().clone());
    let edge = INSPECTOR_EDGE.with(Cell::get);
    let windows10_edge = !full_screen(main) && !unsafe { IsZoomed(main) }.as_bool() && theme::build() < 22000;
    let mut c = Canvas::new(dc, RECT { left: 0, top: 0, right: client.right, bottom: band });
    c.clear(theme::gdi(t.hex.bg));
    let (w, h) = (c.w as f32 / s, band as f32 / s);
    let drawn = c.d2d(s, t.plain, |p| {
        let name_ink = Ink::hex(if active { t.hex.paper } else { t.hex.dim });
        let room = (area.right - area.left) as f32 / s;
        if room > 0.0 {
            let shown = fitted(p, &title, room, s);
            let m = p.measure(&shown, &NAME, 0.0);
            // Centred by eye: the baseline a third of the cap height below the middle.
            let baseline = h / 2.0 + NAME.size * 0.33;
            p.text(&shown, &NAME, area.left as f32 / s, (baseline - m.baseline).round(), name_ink);
        }
        if !full_screen(main) {
            paint_captions(p, main, &t, active, s);
            if windows10_edge {
                // Windows 10 draws no top edge once the caption is gone.
                p.fill_rect(d2d::RectF::new(0.0, 0.0, w, 1.0 / s), Ink::hex(if active { t.hex.track } else { t.hex.line }));
            }
        }
        if edge > 0 {
            p.fill_rect(d2d::RectF::new(edge as f32 / s, h - 1.0, w - edge as f32 / s, 1.0), Ink::hex(t.hex.line));
        }
    });
    if !drawn {
        // No Direct2D: the name in the system font.
        let font = HFONT(FONT.with(Cell::get) as *mut _);
        c.text(&title, area, font, theme::gdi(t.hex.paper), DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    }
    c.present(dc);
}

/// The band of a window of the app's own, such as Settings: `bg`, its
/// title in Newsreader 15 600 at 16 DIPs, Close, and a rule along its foot.
pub fn paint_band(hwnd: HWND, dc: HDC, title: &str) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let band = height(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let active = unsafe { GetActiveWindow() } == hwnd;
    let windows10_edge = theme::build() < 22000;
    let mut c = Canvas::new(dc, RECT { left: 0, top: 0, right: client.right, bottom: band });
    c.clear(theme::gdi(t.hex.bg));
    let (w, h) = (c.w as f32 / s, band as f32 / s);
    c.d2d(s, t.plain, |p| {
        let m = p.measure(title, &NAME, 0.0);
        let baseline = h / 2.0 + NAME.size * 0.33;
        p.text(title, &NAME, 16.0, (baseline - m.baseline).round(), Ink::hex(if active { t.hex.paper } else { t.hex.dim }));
        paint_captions(p, hwnd, &t, active, s);
        if windows10_edge {
            p.fill_rect(d2d::RectF::new(0.0, 0.0, w, 1.0 / s), Ink::hex(if active { t.hex.track } else { t.hex.line }));
        }
        p.fill_rect(d2d::RectF::new(0.0, h - 1.0, w, 1.0), Ink::hex(t.hex.line));
    });
    c.present(dc);
}

/// Repaints a window's band and its caption buttons (activation).
pub fn invalidate_band(hwnd: HWND) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let _ = InvalidateRect(hwnd, Some(&RECT { left: 0, top: 0, right: client.right, bottom: height(hwnd) }), false);
    }
}

/// Minimise, maximise or restore, and close: Windows' own buttons, 46 DIP
/// wide, with their glyphs drawn as lines in the theme's ink. Close turns
/// Windows' red under the pointer.
fn paint_captions(p: &mut Painter, main: HWND, t: &Tokens, active: bool, s: f32) {
    let maximized = unsafe { IsZoomed(main) }.as_bool();
    let ink = if active { t.hex.label } else { t.hex.dim };
    for &which in captions(main) {
        let r = caption_rect(main, which);
        let r = d2d::RectF::new(r.left as f32 / s, r.top as f32 / s, (r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s);
        let hot = HOT.with(Cell::get) == Some(which);
        let pressed = PRESSED.with(Cell::get) == Some(which) && hot;
        let close = which == Caption::Close;
        let mut glyph = Ink::hex(ink);
        if hot || pressed {
            if close {
                p.fill_rect(r, Ink::hex(if pressed { 0xc83c30 } else { 0xc42b1c }));
                glyph = Ink::hex(0xffffff);
            } else {
                p.fill_rect(r, Ink::hex(t.hex.paper).alpha(if pressed { 0.14 } else { 0.08 }));
                glyph = Ink::hex(t.hex.paper);
            }
        }
        // A 10 DIP glyph, its lines on whole pixels.
        let snap = |v: f32| ((v * s).round() + 0.5) / s;
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let line = 1.0 / s * s.round().max(1.0);
        match which {
            Caption::Minimize => p.fill_rect(d2d::RectF::new(cx - 5.0, snap(cy) - line / 2.0, 10.0, line), glyph),
            Caption::Maximize if maximized => {
                let (x, y) = (snap(cx - 5.0), snap(cy - 3.0));
                p.stroke_rect(d2d::RectF::new(x, y, 8.0, 8.0), line, glyph);
                // The window behind: its top and right edges.
                p.line((x + 2.0, y - 2.0), (x + 10.0, y - 2.0), line, glyph);
                p.line((x + 10.0, y - 2.0), (x + 10.0, y + 6.0), line, glyph);
            }
            Caption::Maximize => p.stroke_rect(d2d::RectF::new(snap(cx - 5.0), snap(cy - 5.0), 10.0, 10.0), line, glyph),
            Caption::Close => {
                p.line((cx - 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), line, glyph);
                p.line((cx + 5.0, cy - 5.0), (cx - 5.0, cy + 5.0), line, glyph);
            }
        }
    }
}

/// `title` as fits `room` DIPs: whole, or with its middle given up to an
/// ellipsis, as the macOS band truncates (cached for the last width).
fn fitted(p: &mut Painter, title: &str, room: f32, s: f32) -> String {
    let key = (room * s) as i32;
    if let Some(shown) = FITTED.with(|f| {
        let f = f.borrow();
        (f.0 == title && f.1 == key).then(|| f.2.clone())
    }) {
        return shown;
    }
    let chars: Vec<char> = title.chars().collect();
    let fits = |p: &mut Painter, text: &str| p.measure(text, &NAME, 0.0).width <= room;
    let mut shown = title.to_string();
    if !fits(p, &shown) {
        // The most characters kept, half from each end, that fit.
        let (mut low, mut high) = (0usize, chars.len());
        let cut = |keep: usize| {
            let head = keep.div_ceil(2);
            let tail = keep / 2;
            let mut s: String = chars[..head].iter().collect();
            s.push('…');
            s.extend(&chars[chars.len() - tail..]);
            s
        };
        while low < high {
            let mid = (low + high).div_ceil(2);
            if fits(p, &cut(mid)) { low = mid } else { high = mid - 1 }
        }
        shown = if low == 0 { String::new() } else { cut(low) };
    }
    FITTED.with(|f| *f.borrow_mut() = (title.to_string(), key, shown.clone()));
    shown
}

/// Repaints the band (activation, the title) and the menu row.
pub fn invalidate(main: HWND) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(main, &mut client);
        let _ = InvalidateRect(main, Some(&RECT { left: 0, top: 0, right: client.right, bottom: height(main) }), false);
        let _ = InvalidateRect(menubar(), None, false);
    }
}

/// Windows 11 (22000 and later): rounded corners, asked for explicitly.
/// Mica (`DWMWA_SYSTEMBACKDROP_TYPE`, 22621) is left out: it shows only
/// where nothing paints opaquely, and the band is opaque `bg`.
pub fn round_corners(main: HWND) {
    if theme::build() < 22000 {
        return;
    }
    let round = 2i32; // DWMWCP_ROUND
    unsafe {
        let _ = DwmSetWindowAttribute(main, DWMWINDOWATTRIBUTE(33), &round as *const _ as *const _, 4);
    }
}

// MARK: Menus

fn menubar() -> HWND {
    HWND(MENUBAR.with(Cell::get) as *mut _)
}

/// `TBN_DROPDOWN` from the menu bar: open that menu.
pub fn dropdown(main: HWND, lparam: LPARAM) -> Option<LRESULT> {
    unsafe {
        let header = &*(lparam.0 as *const NMHDR);
        if header.hwndFrom != menubar() || header.code != TBN_DROPDOWN {
            return None;
        }
        let toolbar = &*(lparam.0 as *const NMTOOLBARW);
        let index = (toolbar.iItem as u16).wrapping_sub(MENU_FIRST) as usize;
        // After this notification returns: the menu runs its own loop.
        let _ = PostMessageW(main, super::WM_APP_MENU, WPARAM(index), LPARAM(0));
        Some(LRESULT(TBDDRET_DEFAULT as isize))
    }
}

/// Opens menu `index` under its button, then the next one the pointer or
/// Left and Right ask for, until one closes for good. Runs outside the App
/// borrow: `WM_INITMENUPOPUP` reaches the app while it is open.
pub fn open(main: HWND, index: usize, keyboard: bool) {
    let bar = menubar();
    let menu = HMENU(MENU.with(Cell::get) as *mut _);
    let mut index = index;
    let mut keyboard = keyboard;
    end_keys(main);
    unsafe {
        let hook = SetWindowsHookExW(WH_MSGFILTER, Some(menu_hook), None, windows::Win32::System::Threading::GetCurrentThreadId()).unwrap_or_default();
        HOOK.with(|h| h.set(hook.0 as isize));
        loop {
            let popup = GetSubMenu(menu, index as i32);
            if popup.is_invalid() {
                break;
            }
            let id = (MENU_FIRST + index as u16) as usize;
            let mut r = RECT::default();
            SendMessageW(bar, TB_GETRECT, WPARAM(id), LPARAM(&mut r as *mut _ as isize));
            let mut points = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
            MapWindowPoints(bar, None, &mut points);
            let exclude = RECT { left: points[0].x, top: points[0].y, right: points[1].x, bottom: points[1].y };
            SendMessageW(bar, TB_PRESSBUTTON, WPARAM(id), LPARAM(1));
            OPEN.with(|o| o.set(Some(index)));
            NEXT.with(|n| n.set(None));
            IN_SUBMENU.with(|s| s.set(false));
            ON_POPUP_ITEM.with(|s| s.set(false));
            ALT_HELD.with(|a| a.set(keyboard && GetKeyState(VK_MENU.0 as i32) < 0));
            if keyboard {
                // Opened from the keyboard: its first item is selected.
                let _ = PostMessageW(main, WM_KEYDOWN, WPARAM(VK_DOWN.0 as usize), LPARAM(0));
            }
            let params = TPMPARAMS { cbSize: std::mem::size_of::<TPMPARAMS>() as u32, rcExclude: exclude };
            let _ = TrackPopupMenuEx(popup, (TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL | TPM_LEFTBUTTON).0, exclude.left, exclude.bottom, main, Some(&params));
            SendMessageW(bar, TB_PRESSBUTTON, WPARAM(id), LPARAM(0));
            OPEN.with(|o| o.set(None));
            match NEXT.with(|n| n.take()) {
                Some((next, by_keys)) => {
                    index = next;
                    keyboard = by_keys;
                }
                None => break,
            }
        }
        let _ = UnhookWindowsHookEx(HHOOK(HOOK.with(|h| h.replace(0)) as *mut _));
        let _ = InvalidateRect(bar, None, false);
    }
}

/// `WM_MENUSELECT` while a menu bar menu is open: whether the selection is
/// in a submenu, or on an item that opens one.
pub fn menu_select(wparam: WPARAM, lparam: LPARAM) {
    if OPEN.with(Cell::get).is_none() {
        return;
    }
    let flags = ((wparam.0 >> 16) & 0xffff) as u32;
    let menu = HMENU(lparam.0 as *mut _);
    let top = OPEN.with(Cell::get).map(|i| unsafe { GetSubMenu(HMENU(MENU.with(Cell::get) as *mut _), i as i32) });
    IN_SUBMENU.with(|s| s.set(top.is_some_and(|t| t != menu)));
    ON_POPUP_ITEM.with(|s| s.set(flags & MF_POPUP.0 != 0));
}

fn count() -> usize {
    unsafe { SendMessageW(menubar(), TB_BUTTONCOUNT, WPARAM(0), LPARAM(0)).0 as usize }
}

unsafe extern "system" fn menu_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == MSGF_MENU as i32 {
            let msg = &*(lparam.0 as *const MSG);
            let current = OPEN.with(Cell::get);
            if let Some(current) = current {
                let n = count().max(1);
                match msg.message {
                    WM_KEYUP | WM_SYSKEYUP if msg.wParam.0 == VK_MENU.0 as usize && ALT_HELD.with(|a| a.replace(false)) => {
                        return LRESULT(1);
                    }
                    WM_MOUSEMOVE => {
                        let mut p = msg.pt;
                        let bar = menubar();
                        let _ = ScreenToClient(bar, &mut p);
                        let hit = SendMessageW(bar, TB_HITTEST, WPARAM(0), LPARAM(&p as *const _ as isize)).0;
                        if hit >= 0 && hit as usize != current && (hit as usize) < n {
                            NEXT.with(|x| x.set(Some((hit as usize, false))));
                            let _ = EndMenu();
                        }
                    }
                    WM_LBUTTONDOWN => {
                        // A click on the open menu's own title closes it.
                        let mut p = msg.pt;
                        let bar = menubar();
                        let _ = ScreenToClient(bar, &mut p);
                        let hit = SendMessageW(bar, TB_HITTEST, WPARAM(0), LPARAM(&p as *const _ as isize)).0;
                        if hit >= 0 && hit as usize == current {
                            let _ = EndMenu();
                            return LRESULT(1);
                        }
                    }
                    WM_KEYDOWN if msg.wParam.0 == VK_LEFT.0 as usize && !IN_SUBMENU.with(Cell::get) => {
                        NEXT.with(|x| x.set(Some(((current + n - 1) % n, true))));
                        let _ = EndMenu();
                        return LRESULT(1);
                    }
                    WM_KEYDOWN if msg.wParam.0 == VK_RIGHT.0 as usize && !IN_SUBMENU.with(Cell::get) && !ON_POPUP_ITEM.with(Cell::get) => {
                        NEXT.with(|x| x.set(Some(((current + 1) % n, true))));
                        let _ = EndMenu();
                        return LRESULT(1);
                    }
                    _ => {}
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

/// `WM_SYSCOMMAND` `SC_KEYMENU`: Alt or F10 alone (`ch` 0) gives the menu
/// bar the keyboard; Alt+letter opens that menu. False: not a menu bar key
/// (Alt+Space opens the system menu).
pub fn key_menu(main: HWND, ch: u32) -> bool {
    if ch == ' ' as u32 {
        return false;
    }
    if ch == 0 {
        if KEYS.with(Cell::get).is_some() {
            end_keys(main);
        } else {
            begin_keys(main);
        }
        return true;
    }
    // An Alt+letter no menu has: the system beeps, as it would.
    let Some(index) = mnemonic(ch) else { return false };
    unsafe {
        let _ = PostMessageW(main, super::WM_APP_MENU, WPARAM(index), LPARAM(1));
    }
    true
}

fn mnemonic(ch: u32) -> Option<usize> {
    let mut id = 0u32;
    let found = unsafe { SendMessageW(menubar(), TB_MAPACCELERATORW, WPARAM(ch as usize), LPARAM(&mut id as *mut _ as isize)).0 } != 0;
    found.then(|| (id as u16).wrapping_sub(MENU_FIRST) as usize).filter(|&i| i < count())
}

fn begin_keys(main: HWND) {
    KEYS.with(|k| k.set(Some(0)));
    unsafe {
        SendMessageW(main, WM_CHANGEUISTATE, WPARAM(((UISF_HIDEACCEL | UISF_HIDEFOCUS) << 16 | UIS_CLEAR) as usize), LPARAM(0));
        SendMessageW(menubar(), TB_SETHOTITEM, WPARAM(0), LPARAM(0));
    }
}

fn end_keys(_main: HWND) {
    if KEYS.with(|k| k.take()).is_some() {
        unsafe {
            SendMessageW(menubar(), TB_SETHOTITEM, WPARAM(usize::MAX), LPARAM(0));
        }
    }
}

/// Keys while the menu bar has the keyboard. True: used.
pub fn keys(main: HWND, msg: &MSG) -> bool {
    // Alt+letter of a menu, before dialog navigation (which swallows it).
    if msg.message == WM_SYSCHAR && KEYS.with(Cell::get).is_none() {
        if let Some(index) = mnemonic(msg.wParam.0 as u32) {
            unsafe {
                let _ = PostMessageW(main, super::WM_APP_MENU, WPARAM(index), LPARAM(1));
            }
            return true;
        }
    }
    let Some(hot) = KEYS.with(Cell::get) else { return false };
    let n = count().max(1);
    let set = |i: usize| {
        KEYS.with(|k| k.set(Some(i)));
        unsafe {
            SendMessageW(menubar(), TB_SETHOTITEM, WPARAM(i), LPARAM(0));
        }
    };
    match msg.message {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let key = VIRTUAL_KEY(msg.wParam.0 as u16);
            match key {
                VK_LEFT => set((hot + n - 1) % n),
                VK_RIGHT => set((hot + 1) % n),
                VK_DOWN | VK_UP | VK_RETURN | VK_SPACE => unsafe {
                    let _ = PostMessageW(main, super::WM_APP_MENU, WPARAM(hot), LPARAM(1));
                },
                VK_ESCAPE | VK_F10 => end_keys(main),
                // Alt leaves the menu bar, and goes on: Alt+F still opens File.
                VK_MENU => {
                    end_keys(main);
                    return false;
                }
                _ => {
                    let ch = unsafe { MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_CHAR) } & 0xffff;
                    match (ch != 0).then(|| mnemonic(ch)).flatten() {
                        Some(index) => unsafe {
                            let _ = PostMessageW(main, super::WM_APP_MENU, WPARAM(index), LPARAM(1));
                        },
                        None => {
                            end_keys(main);
                            return false;
                        }
                    }
                }
            }
            true
        }
        // The characters of keys used above, and Alt's own release.
        WM_CHAR | WM_SYSCHAR | WM_KEYUP | WM_SYSKEYUP => true,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_NCLBUTTONDOWN => {
            end_keys(main);
            false
        }
        _ => false,
    }
}

/// The window lost activation: the menu bar lets go of the keyboard.
pub fn deactivated(main: HWND) {
    end_keys(main);
    HOT.with(|h| h.set(None));
    PRESSED.with(|p| p.set(None));
}

/// The menu row paints its titles in the theme on the band's `bg`: `label`,
/// `paper` when hot or open (on a faint `paper` wash), `dim` while the
/// window is inactive; the mnemonic underlined while keyboard cues show.
extern "system" fn menubar_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_menubar(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn paint_menubar(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let (bg, paper) = (theme::gdi(t.hex.bg), theme::gdi(t.hex.paper));
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut c = Canvas::new(dc, client);
        c.clear(bg);
        let hot = SendMessageW(hwnd, TB_GETHOTITEM, WPARAM(0), LPARAM(0)).0;
        let main = GetParent(hwnd).unwrap_or_default();
        let cues = draw::accel_cues(main) || KEYS.with(Cell::get).is_some();
        let active = GetForegroundWindow() == main;
        let font = draw::font_of(hwnd);
        for i in 0..count() {
            let mut button = TBBUTTON::default();
            SendMessageW(hwnd, TB_GETBUTTON, WPARAM(i), LPARAM(&mut button as *mut _ as isize));
            let mut r = RECT::default();
            SendMessageW(hwnd, TB_GETITEMRECT, WPARAM(i), LPARAM(&mut r as *mut _ as isize));
            let pressed = button.fsState & TBSTATE_PRESSED as u8 != 0;
            let lit = pressed || hot == i as isize;
            if lit {
                c.round_rect(RectF::of(r).inset(0.0, 2.0 * s), 4.0 * s, theme::mix(bg, paper, if pressed { 0.14 } else { 0.08 }));
            }
            let mut text = [0u16; 64];
            let n = SendMessageW(hwnd, TB_GETBUTTONTEXTW, WPARAM(button.idCommand as usize), LPARAM(text.as_mut_ptr() as isize)).0.max(0) as usize;
            let title = String::from_utf16_lossy(&text[..n.min(64)]);
            let ink = if lit { t.hex.paper } else if active { t.hex.label } else { t.hex.dim };
            c.label(&title, r, font, theme::gdi(ink), DT_SINGLELINE | DT_VCENTER | DT_CENTER, cues);
        }
        c.present(dc);
    }
}
