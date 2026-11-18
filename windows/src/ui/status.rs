//! The status line (IMPLEMENTATION.md §4.7, `StatusLine.swift`): 28 DIP of
//! words in the system font, 11 DIP `dim`, on the theme's `bg`, under the
//! canvas column. On the left, what's happening: the photograph's size and
//! develop time, an export, cropping, or that it couldn't develop. On the
//! right, the canvas's tools as words that act: Original ⌄ (hold it, or
//! choose Original or Edited from its menu), Fit and 100%, the current zoom
//! in `paper`. Hidden in the empty window.
//!
//! One window painted with Direct2D; its words keep their places from the
//! last paint for clicks, hover and tooltips.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::theme::{self, Tokens};
use super::{App, id, post, with_app};
use crate::platform::{pcwstr, wide};

/// Its height, in DIPs (`Theme.Size.statusLine`).
pub const HEIGHT: f64 = 28.0;
const PADDING: f32 = 18.0;
pub const WORDS: TextStyle = TextStyle::ui(11.0, 400);
const STRONG: TextStyle = TextStyle::ui(11.0, 600);
const CAP: TextStyle = TextStyle::ui(9.5, 400);

/// A word's ink.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Dim,
    Label,
    Paper,
}

/// Some words on the left, in one style.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub strong: bool,
}

impl Span {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self { text: text.into(), tone, strong: false }
    }
    pub fn strong(text: impl Into<String>, tone: Tone) -> Self {
        Self { text: text.into(), tone, strong: true }
    }
}

/// The tools on the right.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tools {
    /// There's an original to show.
    pub original: bool,
    /// It's showing.
    pub holding: bool,
    /// Cropping: the canvas doesn't zoom, so Fit and 100% fade.
    pub cropping: bool,
    /// Couldn't develop: nothing to compare or zoom.
    pub failed: bool,
    /// Comparing, and whether as a slider: Compare's tools instead.
    pub compare: Option<bool>,
}

/// What the line shows.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Line {
    pub left: Vec<Span>,
    /// Cancel, while exporting.
    pub cancel: bool,
    pub tools: Option<Tools>,
}

/// The words that act.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Word {
    Original,
    OriginalMenu,
    Fit,
    Actual,
    Cancel,
    SideBySide,
    Slider,
    Swap,
    Done,
    /// Fit and 100% while comparing: both photographs.
    FitBoth,
    ActualBoth,
}

impl Word {
    const ALL: [Word; 11] = [
        Word::Original,
        Word::OriginalMenu,
        Word::Fit,
        Word::Actual,
        Word::Cancel,
        Word::SideBySide,
        Word::Slider,
        Word::Swap,
        Word::Done,
        Word::FitBoth,
        Word::ActualBoth,
    ];

    fn tip(self) -> &'static str {
        match self {
            Word::Original => "Hold to show the original (\\ or O)",
            Word::OriginalMenu => "Choose what Original does",
            Word::Fit => "Fit photograph (Ctrl+0)",
            Word::Actual => "Actual size (Ctrl+1)",
            Word::Cancel => "Stop exporting",
            Word::SideBySide => "Show the two side by side (S)",
            Word::Slider => "Show the two in one frame with a line between them (S)",
            Word::Swap => "Swap the two photographs' places",
            Word::Done => "Stop comparing (Esc)",
            Word::FitBoth => "Fit both photographs (Ctrl+0)",
            Word::ActualBoth => "Both at actual size (Ctrl+1)",
        }
    }
}

thread_local! {
    static HWND_: Cell<isize> = const { Cell::new(0) };
    static LINE: RefCell<Line> = RefCell::new(Line::default());
    /// The canvas fits the photograph (Fit is the current zoom).
    static FITTING: Cell<bool> = const { Cell::new(true) };
    /// Where each acting word was painted, in DIPs, and whether it acts now.
    static PLACES: RefCell<Vec<(Word, RectF, bool)>> = const { RefCell::new(Vec::new()) };
    static HOT: Cell<Option<Word>> = const { Cell::new(None) };
    /// Original held down with the pointer.
    static HOLDING: Cell<bool> = const { Cell::new(false) };
    static TIPS: Cell<isize> = const { Cell::new(0) };
}

fn hwnd() -> HWND {
    HWND(HWND_.with(Cell::get) as *mut _)
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsStatus"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

pub fn create(main: HWND) -> HWND {
    let status = super::create(w!("NegsStatus"), "", WS_CHILD, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
    HWND_.with(|h| h.set(status.0 as isize));
    status
}

/// Shows `line`, repainting only when it changed. Its words are the
/// window's text, for screen readers.
pub fn set(line: Line) {
    let changed = LINE.with(|l| {
        let mut l = l.borrow_mut();
        if *l == line {
            return false;
        }
        *l = line;
        true
    });
    if changed {
        let words: String = LINE.with(|l| l.borrow().left.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "));
        let text = wide(words);
        unsafe {
            let _ = SetWindowTextW(hwnd(), pcwstr(&text));
            let _ = InvalidateRect(hwnd(), None, false);
        }
    }
}

/// The canvas's zoom: fitted or not. From the canvas, which zooms by itself
/// too (the wheel, a double click).
pub fn set_fitting(fitting: bool) {
    if FITTING.with(|f| f.replace(fitting)) != fitting {
        unsafe {
            let _ = InvalidateRect(hwnd(), None, false);
        }
    }
}

/// The words on the left as one string, for Copy.
pub fn text() -> String {
    LINE.with(|l| l.borrow().left.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "))
}

fn ink(t: &Tokens, tone: Tone) -> u32 {
    match tone {
        Tone::Dim => t.hex.dim,
        Tone::Label => t.hex.label,
        Tone::Paper => t.hex.paper,
    }
}

fn paint(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let line = LINE.with(|l| l.borrow().clone());
    let fitting = FITTING.with(Cell::get);
    let hot = HOT.with(Cell::get);
    let mut places = vec![];
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(t.hex.bg));
    c.d2d(s, t.plain, |p| {
        let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
        // The baseline, by eye in the middle of the line.
        let baseline = (h / 2.0 + WORDS.size * 0.36).round();
        let mut right = w - PADDING;
        // From the right: 100%, Fit, the rule, Original ⌄, then Cancel.
        let mut word = |p: &mut Painter, text: &str, which: Option<Word>, tone: Tone, enabled: bool, right: &mut f32, gap: f32| -> RectF {
            let m = p.measure(text, &WORDS, 0.0);
            let x = *right - m.width;
            let lit = enabled && which.is_some() && hot == which;
            let tone = if lit && tone == Tone::Dim { Tone::Label } else { tone };
            let alpha = if enabled { 1.0 } else { theme::DISABLED_OPACITY };
            p.text(text, &WORDS, x, baseline - m.baseline, Ink::hex(ink(&t, tone)).alpha(alpha));
            let r = RectF::new(x, 0.0, m.width, h);
            if let Some(which) = which {
                places.push((which, RectF::new(x - 4.0, 0.0, m.width + 8.0, h), enabled));
            }
            *right = x - gap;
            r
        };
        // A key in a box after a word, as the Mac's `KeyCap`.
        let cap = |p: &mut Painter, key: &str, right: &mut f32| {
            let m = p.measure(key, &CAP, 0.0);
            let r = RectF::new(*right - m.width - 8.0, (h / 2.0 - 8.0).round(), m.width + 8.0, 16.0);
            p.stroke_round_rect(RectF::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), 3.0, 1.0, Ink::hex(t.hex.track));
            p.text(key, &CAP, r.x + 4.0, baseline - m.baseline, Ink::hex(t.hex.dim));
            *right = r.x - 6.0;
        };
        let rule = |p: &mut Painter, right: &mut f32| {
            p.fill_rect(RectF::new(*right, (h / 2.0 - 5.0).round(), 1.0, 10.0), Ink::hex(t.hex.track));
            *right -= 14.0;
        };
        if let Some(slider) = line.tools.and_then(|t| t.compare) {
            // Side by side  Slider [S] | Swap | Fit  100% | Done [Esc] (`CompareTools`).
            cap(p, "Esc", &mut right);
            word(p, "Done", Some(Word::Done), Tone::Dim, true, &mut right, 14.0);
            rule(p, &mut right);
            word(p, "100%", Some(Word::ActualBoth), if fitting { Tone::Dim } else { Tone::Paper }, true, &mut right, 14.0);
            word(p, "Fit", Some(Word::FitBoth), if fitting { Tone::Paper } else { Tone::Dim }, true, &mut right, 14.0);
            rule(p, &mut right);
            word(p, "Swap", Some(Word::Swap), Tone::Dim, true, &mut right, 14.0);
            rule(p, &mut right);
            cap(p, "S", &mut right);
            word(p, "Slider", Some(Word::Slider), if slider { Tone::Paper } else { Tone::Dim }, true, &mut right, 12.0);
            word(p, "Side by side", Some(Word::SideBySide), if slider { Tone::Dim } else { Tone::Paper }, true, &mut right, 16.0);
        } else if let Some(tools) = line.tools {
            let zoom = !tools.cropping && !tools.failed;
            word(p, "100%", Some(Word::Actual), if fitting { Tone::Dim } else { Tone::Paper }, zoom, &mut right, 14.0);
            word(p, "Fit", Some(Word::Fit), if fitting { Tone::Paper } else { Tone::Dim }, zoom, &mut right, 14.0);
            // A 1 × 10 rule in `track`.
            p.fill_rect(RectF::new(right, (h / 2.0 - 5.0).round(), 1.0, 10.0), Ink::hex(t.hex.track));
            right -= 14.0;
            let original = tools.original && !tools.failed;
            word(p, "⌄", Some(Word::OriginalMenu), Tone::Dim, original, &mut right, 3.0);
            let r = word(p, "Original", Some(Word::Original), if tools.holding { Tone::Paper } else { Tone::Dim }, original, &mut right, 16.0);
            if tools.holding {
                p.underline(RectF::new(r.x, baseline + 1.0, r.w, 5.0), &t, false);
            }
        }
        if line.cancel {
            word(p, "Cancel", Some(Word::Cancel), Tone::Dim, true, &mut right, 16.0);
        }
        // The left words, clipped where the tools begin.
        let room = right - 12.0;
        p.clipped(RectF::new(0.0, 0.0, room.max(0.0), h), |p| {
            let mut x = PADDING;
            for span in &line.left {
                let style = if span.strong { &STRONG } else { &WORDS };
                let m = p.measure(&span.text, style, 0.0);
                p.text(&span.text, style, x, baseline - m.baseline, Ink::hex(ink(&t, span.tone)));
                x += m.width + 5.0;
            }
        });
    });
    c.present(dc);
    PLACES.with(|p| *p.borrow_mut() = places);
    place_tips(hwnd, s);
}

/// One tooltip per acting word, over where it was painted.
fn place_tips(hwnd: HWND, s: f32) {
    unsafe {
        let mut tips = HWND(TIPS.with(Cell::get) as *mut _);
        if tips.is_invalid() {
            let Ok(made) = CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                None,
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                hwnd,
                None,
                super::instance(),
                None,
            ) else {
                return;
            };
            theme::apply_control(made, true);
            for (i, word) in Word::ALL.into_iter().enumerate() {
                // The tooltip keeps the pointer: the text lives as long as it.
                let text: &'static [u16] = Box::leak(wide(word.tip()).into_boxed_slice());
                let info = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    uFlags: TTF_SUBCLASS,
                    hwnd,
                    uId: i + 1,
                    lpszText: windows::core::PWSTR(text.as_ptr() as *mut _),
                    ..Default::default()
                };
                SendMessageW(made, TTM_ADDTOOLW, WPARAM(0), LPARAM(&info as *const _ as isize));
            }
            TIPS.with(|t| t.set(made.0 as isize));
            tips = made;
        }
        let places = PLACES.with(|p| p.borrow().clone());
        for (i, word) in Word::ALL.into_iter().enumerate() {
            let rect = places.iter().find(|(w, _, _)| *w == word).map_or(RECT::default(), |(_, r, _)| RECT {
                left: (r.x * s) as i32,
                top: (r.y * s) as i32,
                right: ((r.x + r.w) * s) as i32,
                bottom: ((r.y + r.h) * s) as i32,
            });
            let info = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, hwnd, uId: i + 1, rect, ..Default::default() };
            SendMessageW(tips, TTM_NEWTOOLRECTW, WPARAM(0), LPARAM(&info as *const _ as isize));
        }
    }
}

/// The acting word under a point in pixels, if it acts now.
fn word_at(hwnd: HWND, x: i32, y: i32) -> Option<Word> {
    let s = draw::dpi_of(hwnd);
    let (x, y) = (x as f32 / s, y as f32 / s);
    PLACES.with(|p| p.borrow().iter().find(|(_, r, on)| *on && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(w, _, _)| *w))
}

/// Original ⌄: what pressing Original does, ticked (`OriginalMenu`);
/// for a RAW shot as RAW+JPEG, which of the two it shows.
fn original_menu(hwnd: HWND) {
    use crate::compare::{OriginalMode, OriginalReference};
    const REFERENCE: usize = 10;
    let Some((mode, reference, jpeg)) = with_app(|app| (app.model.original_mode(), app.model.original_reference(), app.model.camera_jpeg().is_some())) else { return };
    let place = PLACES.with(|p| p.borrow().iter().find(|(w, _, _)| *w == Word::Original).map(|(_, r, _)| *r));
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, m) in OriginalMode::ALL.into_iter().enumerate() {
            let text = wide(m.title());
            let _ = AppendMenuW(menu, MF_STRING | if m == mode { MF_CHECKED } else { MF_UNCHECKED }, 1 + i, pcwstr(&text));
        }
        if jpeg {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            for (i, r) in OriginalReference::ALL.into_iter().enumerate() {
                let text = wide(r.title());
                let _ = AppendMenuW(menu, MF_STRING | if r == reference { MF_CHECKED } else { MF_UNCHECKED }, REFERENCE + i, pcwstr(&text));
            }
        }
        let s = draw::dpi_of(hwnd);
        let mut at = POINT { x: place.map_or(0, |r| (r.x * s) as i32), y: 0 };
        let _ = ClientToScreen(hwnd, &mut at);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_NONOTIFY, at.x, at.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        match chosen.0 as usize {
            0 => {}
            i if i >= REFERENCE => {
                if let Some(r) = OriginalReference::ALL.get(i - REFERENCE) {
                    with_app(|app| app.model.set_original_reference(*r));
                }
            }
            i => {
                if let Some(m) = OriginalMode::ALL.get(i - 1) {
                    with_app(|app| app.model.set_original_mode(*m));
                }
            }
        }
    }
}

fn act(word: Word) {
    let command = match word {
        Word::Fit | Word::FitBoth => id::FIT,
        Word::Actual | Word::ActualBoth => id::ACTUAL,
        Word::Cancel => id::CANCEL_EXPORT,
        Word::SideBySide => return compare_tool(super::compare::Tool::SideBySide),
        Word::Slider => return compare_tool(super::compare::Tool::Slider),
        Word::Swap => return compare_tool(super::compare::Tool::Swap),
        Word::Done => return compare_tool(super::compare::Tool::Done),
        Word::Original | Word::OriginalMenu => return,
    };
    with_app(|app| app.command(command));
}

fn compare_tool(tool: super::compare::Tool) {
    with_app(|app| super::compare::act(app, tool));
}

fn set_hot(hwnd: HWND, hot: Option<Word>) {
    if HOT.with(|h| h.replace(hot)) != hot {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
        if hot == Some(Word::Original) {
            // About to compare: decode the original ahead of the press.
            post(|app: &mut App| app.model.prepare_original());
        }
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point = || ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
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
                let (x, y) = point();
                set_hot(hwnd, word_at(hwnd, x, y));
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                set_hot(hwnd, None);
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let (x, y) = point();
                match word_at(hwnd, x, y) {
                    // Split line and Side by side: a click compares with the original.
                    Some(Word::Original) if with_app(|app| app.model.original_mode() != crate::compare::OriginalMode::Hold).unwrap_or(false) => {
                        with_app(|app| app.model.toggle_original_compare());
                    }
                    Some(Word::Original) => {
                        HOLDING.with(|h| h.set(true));
                        SetCapture(hwnd);
                        with_app(|app| app.model.hold_original(true));
                    }
                    Some(Word::OriginalMenu) => original_menu(hwnd),
                    Some(word) => act(word),
                    None => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if HOLDING.with(|h| h.replace(false)) {
                    if msg == WM_LBUTTONUP {
                        let _ = ReleaseCapture();
                    }
                    if with_app(|app| app.model.hold_original(false)).is_none() {
                        post(|app: &mut App| app.model.hold_original(false));
                    }
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_app(|app| app.status_menu());
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
