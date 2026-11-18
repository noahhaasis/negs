//! The canvas column with nothing open (IMPLEMENTATION.md §4.14,
//! `SmallStates.swift`): the trim marks where the print will go, "Open a
//! photograph" with Open the word to click (Ctrl+O), a line about the app,
//! where to drop a file, and the recent photographs where the label goes.
//! With a photograph dragged over it, only "drop to open it" and a yellow
//! line round the print's place. One window, painted with Direct2D in the
//! theme; the words that act keep their places from the last paint.
//!
//! Windows opens photographs, not folders, until the library comes (parity
//! phase 6): the words say "a photograph" and leave folders out.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Align, Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::theme::{self, Tokens};
use super::{App, id, with_app};
use crate::platform::{pcwstr, wide};

/// Round the print's place: the canvas's side margins and the caption's
/// and label's bands (§2.3).
const MARGIN: f32 = 48.0;
const CAPTION_BAND: f32 = 54.0;
const LABEL_BAND: f32 = 68.0;
/// The trim marks: 18 × 1.5, reaching 26 outside each corner.
const MARK_LENGTH: f32 = 18.0;
const MARK_REACH: f32 = 26.0;
const MARK_WEIGHT: f32 = 1.5;

pub const HEADING: TextStyle = TextStyle::serif(26.0, 600);
pub const TAGLINE: TextStyle = TextStyle::serif(16.0, 400).italic();
pub const HINT: TextStyle = TextStyle::serif(14.0, 400).italic();
const RECENT: TextStyle = TextStyle::serif(14.0, 400).italic();
const RECENT_FILE: TextStyle = TextStyle::serif(17.5, 400);
const DROP: TextStyle = TextStyle::serif(22.0, 400).italic();

#[derive(Default)]
pub struct Empty {
    highlighted: bool,
    shown_recent: Vec<PathBuf>,
}

/// What acts on the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Open,
    Recent(usize),
}

thread_local! {
    /// The recent photographs shown, and whether a file is over the window,
    /// for painting while the app is busy.
    static RECENT_FILES: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static DROPPING: Cell<bool> = const { Cell::new(false) };
    static PLACES: RefCell<Vec<(Target, RectF)>> = const { RefCell::new(Vec::new()) };
    static HOT: Cell<Option<Target>> = const { Cell::new(None) };
    static TIP: Cell<isize> = const { Cell::new(0) };
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsEmpty"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

pub fn build(app: &mut App) {
    let text = wide("Open a photograph (Ctrl+O)");
    unsafe {
        let _ = SetWindowTextW(app.empty, pcwstr(&text));
    }
    sync_recent(app);
}

pub fn dpi_changed(app: &mut App) {
    layout(app);
}

/// Up to five recent photographs, as many as fit beside "Recent".
pub fn sync_recent(app: &mut App) {
    let recent: Vec<_> = app.model.prefs.recent_files.iter().take(5).cloned().collect();
    if recent == app.empty_view.shown_recent {
        return;
    }
    app.empty_view.shown_recent = recent.clone();
    RECENT_FILES.with(|r| *r.borrow_mut() = recent);
    layout(app);
}

pub fn layout(app: &mut App) {
    unsafe {
        let _ = InvalidateRect(app.empty, None, false);
    }
}

pub fn highlight(app: &mut App, on: bool) {
    app.empty_view.highlighted = on;
    DROPPING.with(|d| d.set(on));
    layout(app);
}

/// The print's place in a `w` × `h` canvas: 3:2, fitted between the bands
/// and the side margins, as a landscape photograph would be (`EmptyPrint`).
pub fn print_place(w: f32, h: f32) -> RectF {
    let room = ((w - 2.0 * MARGIN).max(1.0), (h - CAPTION_BAND - LABEL_BAND).max(1.0));
    let scale = (room.0 / 3.0).min(room.1 / 2.0);
    let (pw, ph) = ((3.0 * scale).round(), (2.0 * scale).round());
    RectF::new(((w - pw) / 2.0).round(), (CAPTION_BAND + (room.1 - ph) / 2.0).round(), pw, ph)
}

/// The four trim marks beside the print's corners, level with its top and
/// bottom edges.
pub fn trim_marks(print: RectF) -> [RectF; 4] {
    let (top, bottom) = (print.y, print.y + print.h - MARK_WEIGHT);
    let (left, right) = (print.x - MARK_REACH, print.x + print.w + MARK_REACH - MARK_LENGTH);
    [top, bottom].map(|y| [RectF::new(left, y, MARK_LENGTH, MARK_WEIGHT), RectF::new(right, y, MARK_LENGTH, MARK_WEIGHT)]).concat().try_into().unwrap()
}

fn paint(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let recent = RECENT_FILES.with(|r| r.borrow().clone());
    let dropping = DROPPING.with(Cell::get);
    let hot = HOT.with(Cell::get);
    let mut places = vec![];
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(t.hex.bg));
    c.d2d(s, t.plain, |p| {
        places.clear();
        let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
        let print = print_place(w, h);
        if dropping {
            let o = 12.0;
            p.stroke_round_rect(RectF::new(print.x - o, print.y - o, print.w + 2.0 * o, print.h + 2.0 * o), 4.0, 1.5, Ink::hex(t.hex.accent));
            let m = p.measure("drop to open it", &DROP, 0.0);
            p.text("drop to open it", &DROP, ((w - m.width) / 2.0).round(), ((h - m.height) / 2.0).round(), Ink::hex(t.hex.paper));
            return;
        }
        for mark in trim_marks(print) {
            p.fill_rect(mark, Ink::hex(t.hex.track));
        }
        words(p, &t, w, h, hot, &mut places);
        recent_row(p, &t, &recent, print, w, hot, &mut places);
    });
    c.present(dc);
    PLACES.with(|p| *p.borrow_mut() = places);
}

/// "Open a photograph", the line about the app and the drop hint, centred.
fn words(p: &mut Painter, t: &Tokens, w: f32, h: f32, hot: Option<Target>, places: &mut Vec<(Target, RectF)>) {
    let open = p.measure("Open", &HEADING, 0.0);
    let rest = p.measure(" a photograph", &HEADING, 0.0);
    let tagline = "Explore film stocks, shape the print, and make it your own.";
    let hint = "or drop a RAW, TIFF, PNG or JPEG file here";
    let room = (w - 48.0).max(1.0);
    let tag = p.measure_in(tagline, &TAGLINE.line_height(1.3), room, Align::Center);
    let hint_m = p.measure_in(hint, &HINT.line_height(1.3), room, Align::Center);
    let total = open.height + 10.0 + tag.height + 6.0 + 10.0 + hint_m.height;
    let mut y = ((h - total) / 2.0).round();
    let x = ((w - open.width - rest.width) / 2.0).round();
    p.text("Open", &HEADING, x, y, Ink::hex(t.hex.paper));
    // Open's underline: `track`, `dim` under the pointer.
    let lit = hot == Some(Target::Open);
    p.fill_rect(RectF::new(x, y + open.baseline + 2.5, open.width, 1.5), Ink::hex(if lit { t.hex.dim } else { t.hex.track }));
    places.push((Target::Open, RectF::new(x - 2.0, y, open.width + 4.0, open.height)));
    p.text(" a photograph", &HEADING, x + open.width, y, Ink::hex(t.hex.paper));
    y += open.height + 10.0;
    p.text_in(tagline, &TAGLINE.line_height(1.3), RectF::new(24.0, y, room, 0.0), Align::Center, Ink::hex(t.hex.label));
    y += tag.height + 16.0;
    p.text_in(hint, &HINT.line_height(1.3), RectF::new(24.0, y, room, 0.0), Align::Center, Ink::hex(t.hex.dim));
}

/// "*Recent* DSC_2650.NEF IMG_0412.CR3 …" under the print's place, as many
/// as fit.
fn recent_row(p: &mut Painter, t: &Tokens, recent: &[PathBuf], print: RectF, w: f32, hot: Option<Target>, places: &mut Vec<(Target, RectF)>) {
    if recent.is_empty() {
        return;
    }
    let names: Vec<String> = recent.iter().map(|path| crate::model::file_name(path)).collect();
    let room = w - print.x - MARGIN;
    let label = p.measure("Recent", &RECENT, 0.0);
    let widths: Vec<f32> = names.iter().map(|n| p.measure(n, &RECENT_FILE, 0.0).width).collect();
    // As many as fit, in order.
    let mut count = names.len();
    while count > 0 && label.width + widths[..count].iter().map(|w| 18.0 + w).sum::<f32>() > room {
        count -= 1;
    }
    if count == 0 {
        return;
    }
    let top = print.y + print.h + 14.0;
    let file = p.measure(&names[0], &RECENT_FILE, 0.0);
    // On one baseline: the names' line sets it.
    let baseline = top + file.baseline;
    p.text("Recent", &RECENT, print.x, baseline - label.baseline, Ink::hex(t.hex.dim));
    let mut x = print.x + label.width + 18.0;
    for (i, name) in names[..count].iter().enumerate() {
        let lit = hot == Some(Target::Recent(i));
        p.text(name, &RECENT_FILE, x, top, Ink::hex(if lit { t.hex.paper } else { t.hex.label }));
        if lit {
            p.fill_rect(RectF::new(x, baseline + 2.0, widths[i], 1.0), Ink::hex(t.hex.track));
        }
        places.push((Target::Recent(i), RectF::new(x, top, widths[i], file.height)));
        x += widths[i] + 18.0;
    }
}

fn target_at(hwnd: HWND, lparam: LPARAM) -> Option<Target> {
    let s = draw::dpi_of(hwnd);
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f32 / s, ((lparam.0 >> 16) & 0xffff) as i16 as f32 / s);
    PLACES.with(|p| p.borrow().iter().find(|(_, r)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(t, _)| *t))
}

/// The tooltip of what's under the pointer: Open's keys, or a recent
/// photograph's full path.
fn update_tip(hwnd: HWND, hot: Option<Target>) {
    unsafe {
        let mut tip = HWND(TIP.with(Cell::get) as *mut _);
        if tip.is_invalid() {
            let Ok(made) = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, None, WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX), CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, hwnd, None, super::instance(), None) else { return };
            theme::apply_control(made, true);
            let info = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, uFlags: TTF_SUBCLASS | TTF_IDISHWND, hwnd, uId: hwnd.0 as usize, lpszText: windows::core::PWSTR(w!("").as_ptr() as *mut _), ..Default::default() };
            SendMessageW(made, TTM_ADDTOOLW, WPARAM(0), LPARAM(&info as *const _ as isize));
            TIP.with(|t| t.set(made.0 as isize));
            tip = made;
        }
        let text = match hot {
            Some(Target::Open) => "Open a photograph (Ctrl+O)".to_string(),
            Some(Target::Recent(i)) => RECENT_FILES.with(|r| r.borrow().get(i).map(|p| p.display().to_string())).unwrap_or_default(),
            None => String::new(),
        };
        let text = wide(text);
        let info = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, uFlags: TTF_IDISHWND, hwnd, uId: hwnd.0 as usize, lpszText: windows::core::PWSTR(text.as_ptr() as *mut _), ..Default::default() };
        SendMessageW(tip, TTM_UPDATETIPTEXTW, WPARAM(0), LPARAM(&info as *const _ as isize));
        SendMessageW(tip, TTM_ACTIVATE, WPARAM(hot.is_some() as usize), LPARAM(0));
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let hot = target_at(hwnd, lparam);
                if HOT.with(|h| h.replace(hot)) != hot {
                    update_tip(hwnd, hot);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                if HOT.with(|h| h.take()).is_some() {
                    update_tip(hwnd, None);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }
            WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
                let hand = HOT.with(Cell::get).is_some();
                let _ = SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
                LRESULT(1)
            }
            WM_LBUTTONUP => {
                let command = match target_at(hwnd, lparam) {
                    Some(Target::Open) => Some(id::EMPTY_OPEN),
                    Some(Target::Recent(i)) => Some(id::EMPTY_RECENT + i as u16),
                    None => None,
                };
                if let Some(command) = command {
                    with_app(|app| app.command(command));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The print's place is 3:2, centred between the bands and margins.
    #[test]
    fn the_print_fits_between_the_bands() {
        let r = print_place(1000.0, 700.0);
        assert!((r.w / r.h - 1.5).abs() < 0.01);
        assert!(r.x >= MARGIN - 0.5 && r.x + r.w <= 1000.0 - MARGIN + 0.5);
        assert!(r.y >= CAPTION_BAND - 0.5 && r.y + r.h <= 700.0 - LABEL_BAND + 0.5);
        let marks = trim_marks(r);
        assert_eq!(marks[0].x, r.x - MARK_REACH);
        assert_eq!(marks[1].x + marks[1].w, r.x + r.w + MARK_REACH);
    }
}
