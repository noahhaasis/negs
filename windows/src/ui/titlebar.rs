//! The title bar with the toolbar in it, like the Mac's unified toolbar:
//! the app icon, the menu bar, the tools, the title and the caption
//! buttons, drawn in the client area (`WM_NCCALCSIZE` removes the
//! standard caption; the side and bottom frame stay the system's).
//!
//! The menu bar is a toolbar control with a button per menu, so screen
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

use super::draw::{self, Canvas, RectF};
use super::theme;
use crate::platform::wide;

/// The band's height, in DIPs.
pub const HEIGHT: f64 = 40.0;
const CAPTION_BUTTON: f64 = 46.0;
const ICON: f64 = 40.0;
const MENU_FIRST: u16 = 800;

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
    /// Where the title goes, and the fonts of the title and caption glyphs.
    static TITLE_AREA: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
    static FONTS: Cell<(isize, isize)> = const { Cell::new((0, 0)) };
    /// The hairline between the menus and Open.
    static DIVIDER: Cell<i32> = const { Cell::new(0) };
    /// The toolbars' tooltips, as (toolbar, tooltip).
    static TIPS: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
}

/// The title's and the caption glyphs' fonts (after a DPI change too).
pub fn set_fonts(title: HFONT, glyphs: HFONT) {
    FONTS.with(|f| f.set((title.0 as isize, glyphs.0 as isize)));
}

fn px(hwnd: HWND, dip: f64) -> i32 {
    (dip * unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0).round() as i32
}

/// The band's height for `main` (none in full screen... the band stays,
/// without caption buttons).
pub fn height(main: HWND) -> i32 {
    px(main, HEIGHT)
}

fn full_screen(main: HWND) -> bool {
    unsafe { GetWindowLongW(main, GWL_STYLE) as u32 & WS_CAPTION.0 != WS_CAPTION.0 }
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

/// Button padding for the DPI: menus 32 DIP high, tools 36 × 32.
pub fn set_metrics(main: HWND, bar: HWND) {
    unsafe {
        SendMessageW(bar, TB_SETPADDING, WPARAM(0), LPARAM(((px(main, 10.0) & 0xffff) | (px(main, 16.0) << 16)) as isize));
        SendMessageW(bar, TB_AUTOSIZE, WPARAM(0), LPARAM(0));
    }
}

pub fn tool_metrics(main: HWND, toolbar: HWND) {
    unsafe {
        let image = px(main, 20.0);
        let (w, h) = (px(main, 36.0), px(main, 32.0));
        SendMessageW(toolbar, TB_SETPADDING, WPARAM(0), LPARAM((((w - image) & 0xffff) | ((h - image) << 16)) as isize));
        SendMessageW(toolbar, TB_SETBUTTONSIZE, WPARAM(0), LPARAM(((w & 0xffff) | (h << 16)) as isize));
        // Labelled buttons size themselves to their words (`BTNS_AUTOSIZE`).
        SendMessageW(toolbar, TB_AUTOSIZE, WPARAM(0), LPARAM(0));
    }
    place_tips(toolbar);
}

/// Tooltips for a toolbar's buttons, by command. The toolbar's own
/// (`TBSTYLE_TOOLTIPS`) never showed in the title bar; a tooltip that
/// watches the toolbar itself (`TTF_SUBCLASS`) does, as on the canvas.
pub fn add_tips(toolbar: HWND, tips: Vec<(u16, &'static str)>) {
    unsafe {
        let Ok(tip) = CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            None,
            WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            toolbar,
            None,
            super::instance(),
            None,
        ) else {
            return;
        };
        theme::apply_control(tip, true);
        for (command, text) in tips {
            // The tooltip keeps the pointer: the text lives as long as it.
            let text: &'static [u16] = Box::leak(wide(text).into_boxed_slice());
            let info = TTTOOLINFOW {
                cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                uFlags: TTF_SUBCLASS,
                hwnd: toolbar,
                uId: command as usize,
                lpszText: windows::core::PWSTR(text.as_ptr() as *mut _),
                ..Default::default()
            };
            SendMessageW(tip, TTM_ADDTOOLW, WPARAM(0), LPARAM(&info as *const _ as isize));
        }
        TIPS.with(|t| t.borrow_mut().push((toolbar.0 as isize, tip.0 as isize)));
    }
}

/// Puts each button's tip over the button, after the toolbar sized them.
fn place_tips(toolbar: HWND) {
    let Some(tip) = TIPS.with(|t| t.borrow().iter().find(|(b, _)| *b == toolbar.0 as isize).map(|(_, tip)| HWND(*tip as *mut _))) else { return };
    unsafe {
        let count = SendMessageW(toolbar, TB_BUTTONCOUNT, WPARAM(0), LPARAM(0)).0 as usize;
        for i in 0..count {
            let mut button = TBBUTTON::default();
            SendMessageW(toolbar, TB_GETBUTTON, WPARAM(i), LPARAM(&mut button as *mut _ as isize));
            let mut rect = RECT::default();
            SendMessageW(toolbar, TB_GETITEMRECT, WPARAM(i), LPARAM(&mut rect as *mut _ as isize));
            let info = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, hwnd: toolbar, uId: button.idCommand as usize, rect, ..Default::default() };
            SendMessageW(tip, TTM_NEWTOOLRECTW, WPARAM(0), LPARAM(&info as *const _ as isize));
        }
    }
}

fn size_of(bar: HWND) -> SIZE {
    let mut size = SIZE::default();
    unsafe {
        SendMessageW(bar, TB_GETMAXSIZE, WPARAM(0), LPARAM(&mut size as *mut _ as isize));
    }
    size
}

/// Places the menu bar, Open after it and the actions (Export, the panel)
/// before the caption buttons; returns where the title may go, between them.
/// The photograph's own tools are on the canvas.
pub fn layout(main: HWND, bar: HWND, tools: HWND, actions: HWND) -> RECT {
    let band = height(main);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
        let menus = size_of(bar);
        let x = px(main, ICON);
        let _ = MoveWindow(bar, x, (band - menus.cy) / 2, menus.cx, menus.cy, true);
        let tools_size = size_of(tools);
        let divider = x + menus.cx + px(main, 8.0);
        DIVIDER.with(|d| d.set(divider));
        let tools_x = divider + px(main, 8.0);
        let _ = MoveWindow(tools, tools_x, (band - tools_size.cy) / 2, tools_size.cx, tools_size.cy, true);
        let captions = if full_screen(main) { 0 } else { 3 * px(main, CAPTION_BUTTON) };
        let actions_size = size_of(actions);
        let actions_x = client.right - captions - px(main, 8.0) - actions_size.cx;
        let _ = MoveWindow(actions, actions_x, (band - actions_size.cy) / 2, actions_size.cx, actions_size.cy, true);
        let area = RECT { left: tools_x + tools_size.cx + px(main, 16.0), top: 0, right: actions_x - px(main, 16.0), bottom: band };
        TITLE_AREA.with(|t| t.set(area));
        let _ = InvalidateRect(main, Some(&RECT { left: 0, top: 0, right: client.right, bottom: band }), false);
        area
    }
}

fn caption_rect(main: HWND, which: Caption) -> RECT {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
    }
    let w = px(main, CAPTION_BUTTON);
    let index = match which {
        Caption::Minimize => 3,
        Caption::Maximize => 2,
        Caption::Close => 1,
    };
    RECT { left: client.right - index * w, top: 0, right: client.right - (index - 1) * w, bottom: height(main) }
}

fn caption_at(main: HWND, x: i32, y: i32) -> Option<Caption> {
    if full_screen(main) {
        return None;
    }
    [Caption::Minimize, Caption::Maximize, Caption::Close].into_iter().find(|&c| {
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
        if !IsZoomed(main).as_bool() && p.y < frame {
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
    let r = RECT { left: caption_rect(main, Caption::Minimize).left, top: 0, right: caption_rect(main, Caption::Close).right, bottom: height(main) };
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

/// Paints the band: icon, title and caption buttons (the menu bar and the
/// tools paint themselves), and a hairline under it.
pub fn paint(main: HWND, dc: HDC) {
    let title_area = TITLE_AREA.with(Cell::get);
    let (font, glyphs) = FONTS.with(Cell::get);
    let (font, glyphs) = (HFONT(font as *mut _), HFONT(glyphs as *mut _));
    let title = draw::window_text(main);
    let icon = app_icon();
    let p = theme::palette();
    let s = draw::dpi_of(main);
    let band = height(main);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(main, &mut client);
    }
    let active = unsafe { GetForegroundWindow() } == main || unsafe { GetActiveWindow() } == main;
    let mut c = Canvas::new(dc, RECT { left: 0, top: 0, right: client.right, bottom: band });
    c.clear(p.panel);
    unsafe {
        let size = px(main, 16.0);
        let x = (px(main, ICON) - size) / 2;
        let _ = DrawIconEx(c.hdc(), x, (band - size) / 2, icon, size, size, 0, None, DI_NORMAL);
    }
    let divider = DIVIDER.with(Cell::get) as f32;
    if divider > 0.0 {
        c.round_rect(RectF::new(divider, band as f32 * 0.3, s.max(1.0).floor(), band as f32 * 0.4), 0.0, p.border);
    }
    // The title, centred in its space when it fits.
    let ink = if active { p.text } else { p.disabled };
    c.text(&title, title_area, font, if active { p.secondary } else { p.disabled }, DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_END_ELLIPSIS);
    if !full_screen(main) {
        let maximized = unsafe { IsZoomed(main) }.as_bool();
        for which in [Caption::Minimize, Caption::Maximize, Caption::Close] {
            let r = caption_rect(main, which);
            let hot = HOT.with(Cell::get) == Some(which);
            let pressed = PRESSED.with(Cell::get) == Some(which) && hot;
            let close = which == Caption::Close;
            let mut glyph_ink = ink;
            if hot || pressed {
                let fill = if close {
                    if pressed { theme::rgb(0xc8, 0x3c, 0x30) } else { theme::rgb(0xc4, 0x2b, 0x1c) }
                } else if pressed {
                    p.subtle_pressed
                } else {
                    p.subtle_hover
                };
                c.round_rect(RectF::of(r), 0.0, fill);
                if close {
                    glyph_ink = theme::rgb(0xff, 0xff, 0xff);
                }
            }
            let glyph = match which {
                Caption::Minimize => "\u{E921}",
                Caption::Maximize if maximized => "\u{E923}",
                Caption::Maximize => "\u{E922}",
                Caption::Close => "\u{E8BB}",
            };
            c.text(glyph, r, glyphs, glyph_ink, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
        }
        if !unsafe { IsZoomed(main) }.as_bool() && theme::build() < 22000 {
            // Windows 10 draws no top edge once the caption is gone.
            c.round_rect(RectF::new(0.0, 0.0, c.w as f32, 1.0), 0.0, if active { p.strong_border } else { p.border });
        }
    }
    c.round_rect(RectF::new(0.0, band as f32 - s.max(1.0).floor(), c.w as f32, s.max(1.0).floor()), 0.0, p.border);
    c.present(dc);
}

fn app_icon() -> HICON {
    thread_local! {
        static ICON: Cell<isize> = const { Cell::new(0) };
    }
    ICON.with(|i| {
        if i.get() == 0 {
            let icon = unsafe { LoadImageW(super::instance(), windows::core::PCWSTR(1 as *const u16), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE | LR_SHARED) };
            i.set(icon.map_or(0, |h| h.0 as isize));
        }
        HICON(i.get() as *mut _)
    })
}

/// Repaints the band (activation, the title).
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
/// where nothing paints opaquely, and the band's text, glyphs and the two
/// toolbars are GDI, which paints opaque. See docs/windows-app.md.
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

/// The menu bar paints its titles from the palette, with the mnemonic
/// underlined while keyboard cues show.
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
    let p = theme::palette();
    let s = draw::dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut c = Canvas::new(dc, client);
        c.clear(p.panel);
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
            if pressed || hot == i as isize {
                c.round_rect(RectF::of(r).inset(0.0, 2.0 * s), 4.0 * s, if pressed { p.subtle_pressed } else { p.subtle_hover });
            }
            let mut text = [0u16; 64];
            let n = SendMessageW(hwnd, TB_GETBUTTONTEXTW, WPARAM(button.idCommand as usize), LPARAM(text.as_mut_ptr() as isize)).0.max(0) as usize;
            let title = String::from_utf16_lossy(&text[..n.min(64)]);
            c.label(&title, r, font, if active || pressed { p.text } else { p.secondary }, DT_SINGLELINE | DT_VCENTER | DT_CENTER, cues);
        }
        c.present(dc);
    }
}
