//! Compare's view (ticket 46, `CompareCanvas.swift`): it takes the
//! canvas's place while comparing. Side by side, each photograph in its own
//! frame with its number and stocks under it, the edited one's number
//! ringed in pencil; as a slider, one frame with the first left of a line
//! that drags. A click on the other photograph edits it; along the top,
//! Side by side · Slider, Swap and Done. Keys: S, ← →, Esc, C.
//!
//! The pictures are drawn through the monitor's profile as the tray's
//! tiles are (`previews::Bitmap`), with GDI's halftone stretch.

use std::cell::RefCell;
use std::sync::Arc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::{App, theme, with_app};
use crate::compare::{self as rules, Layout, Side};
use crate::previews::Bitmap;

const NUMBER: TextStyle = TextStyle::serif(24.0, 600);
const STOCKS: TextStyle = TextStyle::serif(15.0, 400).italic();
const TOOL: TextStyle = TextStyle::serif(15.0, 400).italic();
const TOOL_ON: TextStyle = TextStyle::serif(15.0, 600).italic();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    SideBySide,
    Slider,
    Swap,
    Done,
}

const TOOLS: [(Tool, &str, &str); 4] = [
    (Tool::SideBySide, "Side by side", "Show the two side by side (S)"),
    (Tool::Slider, "Slider", "Show the two in one frame, a line between them (S)"),
    (Tool::Swap, "Swap", "Swap the two photographs' places"),
    (Tool::Done, "Done", "Stop comparing (Esc)"),
];

struct View {
    hwnd: HWND,
    /// Each side's picture, by the pixels it was made from.
    bitmaps: [Option<(usize, Arc<Bitmap>)>; 2],
    aspects: [f64; 2],
    words: [(String, String); 2],
    comparison: Option<rules::Comparison>,
    /// Where things were last drawn, in DIPs: the photographs and the tools.
    frames: [RectF; 2],
    tools: Vec<(Tool, RectF)>,
    dragging: bool,
}

impl Default for View {
    fn default() -> Self {
        let none = RectF::new(0.0, 0.0, 0.0, 0.0);
        Self { hwnd: HWND::default(), bitmaps: [None, None], aspects: [1.5; 2], words: Default::default(), comparison: None, frames: [none, none], tools: vec![], dragging: false }
    }
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
}

fn hwnd() -> HWND {
    VIEW.with(|v| v.borrow().hwnd)
}

/// Placed over the canvas while comparing.
pub fn layout(app: &App, rect: RECT, shown: bool) {
    if !shown && hwnd().is_invalid() {
        return;
    }
    let hwnd = ensure(app);
    unsafe {
        let _ = MoveWindow(hwnd, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, true);
        let _ = ShowWindow(hwnd, if shown { SW_SHOWNA } else { SW_HIDE });
        if shown {
            let _ = SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn ensure(app: &App) -> HWND {
    let existing = hwnd();
    if !existing.is_invalid() {
        return existing;
    }
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("NegsCompare"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("NegsCompare"), w!("Compare"), WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP, 0, 0, 1, 1, app.main, None, super::instance(), None)
            .unwrap_or_default();
        VIEW.with(|v| v.borrow_mut().hwnd = hwnd);
        hwnd
    }
}

/// Brings the pictures and words up to date with the model.
pub fn sync(app: &mut App) {
    let Some(comparison) = app.model.comparison().cloned() else {
        VIEW.with(|v| {
            let mut v = v.borrow_mut();
            v.comparison = None;
            v.bitmaps = [None, None];
        });
        return;
    };
    let color = app.model.previews.color.clone();
    let mut bitmaps: [Option<(usize, Arc<Bitmap>)>; 2] = VIEW.with(|v| v.borrow().bitmaps.clone());
    let mut aspects = [1.5; 2];
    let mut words: [(String, String); 2] = Default::default();
    for (i, side) in [&comparison.first, &comparison.second].into_iter().enumerate() {
        let shown = app.model.compared_side(side);
        if let Some(shown) = &shown {
            aspects[i] = shown.aspect();
            let key = Arc::as_ptr(&shown.rgba) as usize;
            if bitmaps[i].as_ref().is_none_or(|(k, _)| *k != key) {
                bitmaps[i] = Bitmap::from_rgba(&shown.rgba, shown.width, shown.height, shown.wide, color.as_deref(), 0).ok().map(|b| (key, Arc::new(b)));
            }
        } else {
            bitmaps[i] = None;
        }
        words[i] = match side {
            Side::Original(_) => (app.model.original_word().to_string(), String::new()),
            Side::Photo(item) => {
                let stocks = app.model.compared_stocks(item);
                (app.model.frame_number(item), stocks)
            }
        };
    }
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.bitmaps = bitmaps;
        v.aspects = aspects;
        v.words = words;
        v.comparison = Some(comparison);
    });
    let hwnd = hwnd();
    if !hwnd.is_invalid() {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

fn paint(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let ink = super::sheet::ink();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let (w, h) = (client.right as f64 / s as f64, client.bottom as f64 / s as f64);
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(ink.sheet));
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        let Some(comparison) = v.comparison.clone() else { return };
        let px = |r: RectF| RECT { left: (r.x * s).round() as i32, top: (r.y * s).round() as i32, right: ((r.x + r.w) * s).round() as i32, bottom: ((r.y + r.h) * s).round() as i32 };
        let blank = theme::gdi(t.film.blank);
        let slider = comparison.layout == Layout::Slider;
        let frames: [RectF; 2] = if slider {
            let f = rules::slider_frame((w, h), v.aspects[0]);
            let r = RectF::new(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32);
            [r, r]
        } else {
            let (_, f) = rules::layout((w, h), (v.aspects[0], v.aspects[1]));
            f.map(|f| RectF::new(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32))
        };
        v.frames = frames;
        if slider {
            let frame = px(frames[0]);
            let split = frame.left + ((frame.right - frame.left) as f64 * comparison.split).round() as i32;
            for (i, clip) in [(0, RECT { right: split, ..frame }), (1, RECT { left: split, ..frame })] {
                unsafe {
                    let saved = SaveDC(c.hdc());
                    IntersectClipRect(c.hdc(), clip.left, clip.top, clip.right, clip.bottom);
                    super::inspector::draw_thumbnail(&mut c, frame, 0.0, v.bitmaps[i].as_ref().map(|(_, b)| b.as_ref()), false, blank, blank);
                    let _ = RestoreDC(c.hdc(), saved);
                }
            }
        } else {
            for i in 0..2 {
                super::inspector::draw_thumbnail(&mut c, px(frames[i]), 0.0, v.bitmaps[i].as_ref().map(|(_, b)| b.as_ref()), false, blank, blank);
            }
        }
        let words = v.words.clone();
        let mut tools = vec![];
        c.d2d(s, t.plain, |p| {
            // Under each photograph: its number, ringed when it's the one
            // edited, and its film and paper.
            let places: Vec<(RectF, usize)> = if slider {
                let f = frames[0];
                vec![(RectF::new(f.x, f.y, f.w / 2.0, f.h), 0), (RectF::new(f.x + f.w / 2.0, f.y, f.w / 2.0, f.h), 1)]
            } else {
                vec![(frames[0], 0), (frames[1], 1)]
            };
            for (r, i) in places {
                let (number, stocks) = &words[i];
                let m = p.measure(number, &NUMBER, 0.0);
                let x = if slider && i == 1 { r.x + r.w - m.width } else { r.x };
                let y = r.y + r.h + 10.0;
                p.text(number, &NUMBER, x, y, Ink::hex(ink.paper));
                if comparison.active_first == (i == 0) && !(slider && comparison.against_original()) {
                    p.ring(RectF::new(x - 8.0, y - 4.0, m.width + 16.0, m.height + 8.0), negs_marks::Radii::NUMBER, -6.0, Ink::hex(ink.accent));
                }
                if !stocks.is_empty() {
                    let sm = p.measure(stocks, &STOCKS, 0.0);
                    let sx = if slider && i == 1 { x - 14.0 - sm.width } else { x + m.width + 14.0 };
                    p.text(stocks, &STOCKS, sx, y + m.baseline - sm.baseline, Ink::hex(ink.dim));
                }
            }
            if slider {
                // The line, with a grip in its middle.
                let f = frames[0];
                let x = f.x + f.w * comparison.split as f32;
                p.line((x, f.y), (x, f.y + f.h), 1.5, Ink::hex(0xffffff).alpha(0.9));
                p.fill_rect(RectF::new(x - 3.0, f.y + f.h / 2.0 - 16.0, 6.0, 32.0), Ink::hex(0xffffff).alpha(0.9));
            }
            // The tools along the top, from the right.
            let mut x = (w as f32) - 24.0;
            for (tool, title, _) in TOOLS.iter().rev() {
                let on = matches!((tool, comparison.layout), (Tool::SideBySide, Layout::SideBySide) | (Tool::Slider, Layout::Slider));
                let style = if on { TOOL_ON } else { TOOL };
                let m = p.measure(title, &style, 0.0);
                x -= m.width;
                let y = ((rules::CAPTION as f32) - m.height) / 2.0;
                p.text(title, &style, x, y, Ink::hex(if on { ink.paper } else { ink.label }));
                tools.push((*tool, RectF::new(x - 6.0, y - 4.0, m.width + 12.0, m.height + 8.0)));
                x -= if matches!(tool, Tool::Slider) { 10.0 } else { 26.0 };
            }
        });
        v.tools = tools;
    });
}

fn at(lparam: LPARAM, hwnd: HWND) -> (f32, f32) {
    let s = draw::dpi_of(hwnd);
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f32, ((lparam.0 >> 16) & 0xffff) as i16 as f32);
    (x / s, y / s)
}

fn contains(r: &RectF, (x, y): (f32, f32)) -> bool {
    x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
}

fn act(tool: Tool) {
    with_app(|app| match tool {
        Tool::SideBySide => app.model.set_compare_layout(Layout::SideBySide),
        Tool::Slider => app.model.set_compare_layout(Layout::Slider),
        Tool::Swap => app.model.swap_compared(),
        Tool::Done => leave(app),
    });
}

/// Leaves compare for where it was entered.
pub fn leave(app: &mut App) {
    let origin = app.model.comparison().map(|c| c.origin);
    app.model.leave_compare();
    if origin == Some(rules::Origin::Grid) {
        super::grid::open(app);
    }
}

/// C and View › Compare: in, from the grid's or strip's selection, or out.
pub fn toggle(app: &mut App) {
    if app.model.comparison().is_some() {
        return leave(app);
    }
    let in_grid = super::grid::shown(app);
    let origin = if in_grid { rules::Origin::Grid } else { rules::Origin::Photo };
    app.model.toggle_compare(origin);
    if in_grid && app.model.comparison().is_some() {
        super::grid::close(app, false);
    }
}

/// The context menu's first item (ticket 46): "Compare 3 and 7", "Compare
/// with 3" or "Compare with Original", worded by what it acts on.
pub fn add_offer(app: &mut App, menu: HMENU, item: &negs_library::Item) {
    let offer = app.model.compare_offer(item, grid_shown(app));
    let text = super::wide(&offer.title);
    unsafe {
        let _ = AppendMenuW(menu, if offer.enabled { MF_STRING } else { MF_STRING | MF_GRAYED }, super::id::COMPARE_OFFER as usize, super::pcwstr(&text));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    }
    app.model.offer_compare(offer);
}

fn grid_shown(app: &App) -> bool {
    super::grid::shown(app)
}

/// The context menu's Compare chosen.
pub fn take_offer(app: &mut App) {
    let in_grid = grid_shown(app);
    if app.model.take_compare_offer(if in_grid { rules::Origin::Grid } else { rules::Origin::Photo }) && in_grid {
        super::grid::close(app, false);
    }
}

/// Keys while comparing: S, ← →, Esc and C. True if used.
pub fn key(app: &mut App, msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || app.model.comparison().is_none() {
        return false;
    }
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    match VIRTUAL_KEY(msg.wParam.0 as u16) {
        VK_ESCAPE => leave(app),
        VK_LEFT if !ctrl => app.model.step_compared(-1),
        VK_RIGHT if !ctrl => app.model.step_compared(1),
        k if k.0 == b'S' as u16 && !ctrl => app.model.toggle_compare_layout(),
        _ => return false,
    }
    true
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
            WM_LBUTTONDOWN => {
                let p = at(lparam, hwnd);
                let (tool, frames, comparison) = VIEW.with(|v| {
                    let v = v.borrow();
                    (v.tools.iter().find(|(_, r)| contains(r, p)).map(|(t, _)| *t), v.frames, v.comparison.clone())
                });
                if let Some(tool) = tool {
                    act(tool);
                    return LRESULT(0);
                }
                let Some(comparison) = comparison else { return LRESULT(0) };
                if comparison.layout == Layout::Slider {
                    if contains(&frames[0], p) {
                        VIEW.with(|v| v.borrow_mut().dragging = true);
                        SetCapture(hwnd);
                        let split = ((p.0 - frames[0].x) / frames[0].w.max(1.0)) as f64;
                        with_app(|app| app.model.move_split(split));
                    }
                } else if let Some(i) = frames.iter().position(|r| contains(r, p)) {
                    with_app(|app| app.model.activate_compared(i == 0));
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if VIEW.with(|v| v.borrow().dragging) {
                    let p = at(lparam, hwnd);
                    let frame = VIEW.with(|v| v.borrow().frames[0]);
                    let split = ((p.0 - frame.x) / frame.w.max(1.0)) as f64;
                    with_app(|app| app.model.move_split(split));
                }
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                VIEW.with(|v| v.borrow_mut().dragging = false);
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let s = draw::dpi_of(hwnd);
                let point = (p.x as f32 / s, p.y as f32 / s);
                let (over_tool, over_frame, slider) = VIEW.with(|v| {
                    let v = v.borrow();
                    (v.tools.iter().any(|(_, r)| contains(r, point)), v.frames.iter().any(|r| contains(r, point)), v.comparison.as_ref().is_some_and(|c| c.layout == Layout::Slider))
                });
                let cursor = if over_tool || (over_frame && !slider) { IDC_HAND } else if over_frame { IDC_SIZEWE } else { IDC_ARROW };
                let _ = SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The tools' tooltips, by tool.
#[allow(dead_code)]
fn tool_help(tool: Tool) -> &'static str {
    TOOLS.iter().find(|(t, _, _)| *t == tool).map_or("", |(_, _, help)| help)
}
