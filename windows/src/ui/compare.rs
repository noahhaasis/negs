//! Compare's view (ticket 46, `CompareCanvas.swift`): it takes the
//! canvas's place while comparing. Side by side, each photograph in its own
//! frame: over it its number in a ring (the edited one's in pencil) and what
//! the camera wrote down, the facts that differ underlined; under it its
//! film, printed on its paper. As a slider, one frame with the first left
//! of a line that drags, the first's words on the left and the second's on
//! the right. A click on the other photograph edits it. The tools (Side by
//! side · Slider, Swap, Done) are in the status line; keys S, ← →, Esc, C.
//!
//! The pictures are drawn through the monitor's profile as the tray's
//! tiles are (`previews::Bitmap`), with GDI's halftone stretch.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::sheet::SheetInk;
use super::{App, post, theme, with_app};
use crate::compare::{self as rules, Layout, Side};
use crate::previews::Bitmap;

/// The print's caption and label styles (`canvas/print.rs`).
const NUMBER: TextStyle = TextStyle::serif(15.0, 600);
const FACT: TextStyle = TextStyle::serif(15.0, 400).italic();
const FILM: TextStyle = TextStyle::serif(24.0, 600).tracking(-0.01);
const KIND: TextStyle = TextStyle::serif(14.0, 400).italic();
const PAPER: TextStyle = TextStyle::serif(17.5, 400);
/// The number's ring, in DIPs.
const RING: f32 = 26.0;

/// What's written round one photograph.
#[derive(Clone, Default, PartialEq)]
struct Words {
    number: String,
    /// The camera's facts, once read.
    facts: Option<crate::photo_facts::PhotoFacts>,
    film: String,
    paper: Option<String>,
}

struct View {
    hwnd: HWND,
    /// Each side's picture, by the pixels it was made from.
    bitmaps: [Option<(usize, Arc<Bitmap>)>; 2],
    aspects: [f64; 2],
    words: [Words; 2],
    comparison: Option<rules::Comparison>,
    /// Where the photographs were last drawn, in DIPs.
    frames: [RectF; 2],
    dragging: bool,
}

impl Default for View {
    fn default() -> Self {
        let none = RectF::new(0.0, 0.0, 0.0, 0.0);
        Self { hwnd: HWND::default(), bitmaps: [None, None], aspects: [1.5; 2], words: Default::default(), comparison: None, frames: [none, none], dragging: false }
    }
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
    /// The camera's facts by file, read on a thread the first time.
    static FACTS: RefCell<HashMap<PathBuf, Option<crate::photo_facts::PhotoFacts>>> = RefCell::new(HashMap::new());
}

/// A file's facts, asking for them the first time.
fn facts(path: &std::path::Path) -> Option<crate::photo_facts::PhotoFacts> {
    let known = FACTS.with(|f| f.borrow().get(path).cloned());
    match known {
        Some(facts) => facts,
        None => {
            FACTS.with(|f| f.borrow_mut().insert(path.to_path_buf(), None));
            let path = path.to_path_buf();
            std::thread::Builder::new()
                .name("compare-facts".into())
                .spawn(move || {
                    let facts = crate::photo_facts::PhotoFacts::read(&path).unwrap_or_default();
                    post(move |app: &mut App| {
                        FACTS.with(|f| f.borrow_mut().insert(path, Some(facts)));
                        app.model.dirty |= crate::model::dirty::COMPARE;
                    });
                })
                .ok();
            None
        }
    }
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
    let mut words: [Words; 2] = Default::default();
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
            Side::Original(item) => Words { number: String::new(), facts: facts(&item.photo), film: app.model.original_word().to_string(), paper: None },
            Side::Photo(item) => {
                let (film, paper) = app.model.compared_stocks(item);
                Words { number: app.model.frame_number(item), facts: facts(&item.photo), film, paper }
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

/// The caption's facts; the slider's leave out the camera.
fn items(words: &Words, camera: bool) -> Vec<String> {
    let Some(facts) = &words.facts else { return vec![] };
    let facts = if camera { facts.clone() } else { crate::photo_facts::PhotoFacts { camera: None, ..facts.clone() } };
    facts.items(super::inspector::decimal_separator())
}

/// Over a photograph: its number in a ring, then its facts, those the
/// other's differ in underlined. `right` lays it out leftwards from `edge`.
#[allow(clippy::too_many_arguments)]
fn caption(p: &mut Painter, t: &theme::Tokens, ink: &SheetInk, words: &Words, other: &Words, edge: f32, middle: f32, right: bool, edited: bool, camera: bool, room: f32) {
    const DOT_GAP: f32 = 9.0;
    let dot = "·";
    let dot_width = p.measure(dot, &FACT, 0.0).width;
    let has_number = !words.number.is_empty();
    let number_width = if has_number { RING + 12.0 } else { 0.0 };
    // What fits: without the camera first, then the last facts dropped.
    let mut camera = camera;
    let (mut facts, mut others) = (items(words, camera), items(other, camera));
    let mut widths: Vec<f32>;
    let mut facts_width;
    loop {
        widths = facts.iter().map(|f| p.measure(f, &FACT, 0.0).width).collect();
        facts_width = widths.iter().sum::<f32>() + (facts.len().saturating_sub(1)) as f32 * (dot_width + 2.0 * DOT_GAP);
        if number_width + facts_width <= room || facts.is_empty() {
            break;
        }
        if camera && words.facts.as_ref().is_some_and(|f| f.camera.is_some()) {
            camera = false;
            (facts, others) = (items(words, false), items(other, false));
        } else {
            facts.pop();
        }
    }
    let total = number_width + facts_width;
    let x0 = if right { edge - total } else { edge };
    // The ring, first on the left or last on the right.
    if has_number {
        let rx = if right { edge - RING } else { x0 };
        let r = RectF::new(rx, middle - RING / 2.0, RING, RING);
        if edited {
            p.ring(r, negs_marks::Radii::NUMBER, -6.0, Ink::hex(ink.accent));
        } else {
            p.stroke_round_rect(RectF::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), RING / 2.0 - 1.0, 1.2, Ink::hex(ink.dim));
        }
        let m = p.measure(&words.number, &NUMBER, 0.0);
        p.text(&words.number, &NUMBER, rx + (RING - m.width) / 2.0, middle - m.height / 2.0, Ink::hex(ink.paper));
    }
    let mut x = if right { x0 } else { x0 + number_width };
    for (i, (fact, w)) in facts.iter().zip(&widths).enumerate() {
        if i > 0 {
            let m = p.measure(dot, &FACT, 0.0);
            p.text(dot, &FACT, x + DOT_GAP, middle - m.height / 2.0, Ink::hex(ink.dim));
            x += dot_width + 2.0 * DOT_GAP;
        }
        let m = p.measure(fact, &FACT, 0.0);
        let y = middle - m.height / 2.0;
        p.text(fact, &FACT, x, y, Ink::hex(ink.label));
        // Only what differs, and only once the other's facts are known.
        if i > 0 || !camera {
            if !others.is_empty() && !others.contains(fact) {
                p.underline(RectF::new(x, y + m.baseline + 2.0, *w, 5.0), t, false);
            }
        } else if !others.is_empty() && others.first() != Some(fact) {
            p.underline(RectF::new(x, y + m.baseline + 2.0, *w, 5.0), t, false);
        }
        x += w;
    }
}

/// Under a photograph: its film, and on the next line (or after it, `inline`,
/// when it ends before `limit`) "printed on" its paper.
fn label(p: &mut Painter, ink: &SheetInk, words: &Words, edge: f32, top: f32, right: bool, inline: bool, limit: f32) {
    let fm = p.measure(&words.film, &FILM, 0.0);
    let fx = if right { edge - fm.width } else { edge };
    p.text(&words.film, &FILM, fx, top, Ink::hex(ink.paper));
    let Some(paper) = words.paper.as_ref() else { return };
    let km = p.measure("printed on", &KIND, 0.0);
    let pm = p.measure(paper, &PAPER, 0.0);
    if inline && fx + fm.width + 14.0 + km.width + 8.0 + pm.width > limit {
        return;
    }
    let (x, baseline) = if inline { (fx + fm.width + 14.0, top + fm.baseline) } else { (edge, top + fm.height + 4.0 + pm.baseline) };
    let x = if right && !inline { edge - km.width - 8.0 - pm.width } else { x };
    p.text("printed on", &KIND, x, baseline - km.baseline, Ink::hex(ink.dim));
    p.text(paper, &PAPER, x + km.width + 8.0, baseline - pm.baseline, Ink::hex(ink.paper));
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
        let mut column = false;
        let frames: [RectF; 2] = if slider {
            let f = rules::slider_frame((w, h), v.aspects[0]);
            let r = RectF::new(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32);
            [r, r]
        } else {
            let (arrangement, f) = rules::layout((w, h), (v.aspects[0], v.aspects[1]));
            column = arrangement == rules::Arrangement::Column;
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
        let edited = |i: usize| comparison.active_first == (i == 0) && !matches!(if i == 0 { &comparison.first } else { &comparison.second }, Side::Original(_));
        c.d2d(s, t.plain, |p| {
            if slider {
                let f = frames[0];
                let middle = f.y - 20.0;
                caption(p, &t, &ink, &words[0], &words[1], f.x, middle, false, edited(0), false, f.w / 2.0 - 12.0);
                caption(p, &t, &ink, &words[1], &words[0], f.x + f.w, middle, true, edited(1), false, f.w / 2.0 - 12.0);
                // The first's paper only when it clears the second's film.
                let second = p.measure(&words[1].film, &FILM, 0.0).width;
                label(p, &ink, &words[0], f.x, f.y + f.h + 8.0, false, true, f.x + f.w - second - 24.0);
                label(p, &ink, &words[1], f.x + f.w, f.y + f.h + 8.0, true, true, f32::NEG_INFINITY);
                // The line, with a grip in its middle.
                let x = f.x + f.w * comparison.split as f32;
                let cy = f.y + f.h / 2.0;
                let white = Ink::hex(0xffffff).alpha(0.92);
                p.line((x, f.y), (x, f.y + f.h), 1.5, white);
                p.fill_ellipse(x, cy, 13.0, 13.0, Ink::hex(0x111111).alpha(0.85));
                p.stroke_round_rect(RectF::new(x - 13.0, cy - 13.0, 26.0, 26.0), 13.0, 1.5, white);
                for (a, b, c2) in [((x - 3.0, cy - 4.0), (x - 6.5, cy), (x - 3.0, cy + 4.0)), ((x + 3.0, cy - 4.0), (x + 6.5, cy), (x + 3.0, cy + 4.0))] {
                    p.line(a, b, 1.4, white);
                    p.line(b, c2, 1.4, white);
                }
            } else if column {
                // Stacked: one line over each, the film on its right.
                for i in 0..2 {
                    let f = frames[i];
                    let middle = f.y - rules::STACKED_CAPTION as f32 / 2.0;
                    let film = match &words[i].paper {
                        Some(paper) => format!("{} printed on {}", words[i].film, paper),
                        None => words[i].film.clone(),
                    };
                    let m = p.measure(&film, &FACT, 0.0);
                    p.text(&film, &FACT, f.x + f.w - m.width, middle - m.height / 2.0, Ink::hex(ink.paper));
                    caption(p, &t, &ink, &words[i], &words[1 - i], f.x, middle, false, edited(i), true, f.w - m.width - 24.0);
                }
            } else {
                for i in 0..2 {
                    let f = frames[i];
                    // As wide as its frame, or the room up to the other's.
                    let room = if i == 0 { (frames[1].x - f.x - 24.0).max(f.w) } else { w as f32 - f.x - 16.0 };
                    caption(p, &t, &ink, &words[i], &words[1 - i], f.x, f.y - 20.0, false, edited(i), true, room);
                    label(p, &ink, &words[i], f.x, f.y + f.h + 8.0, false, false, f32::INFINITY);
                }
            }
        });
    });
    c.present(dc);
}

fn at(lparam: LPARAM, hwnd: HWND) -> (f32, f32) {
    let s = draw::dpi_of(hwnd);
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f32, ((lparam.0 >> 16) & 0xffff) as i16 as f32);
    (x / s, y / s)
}

fn contains(r: &RectF, (x, y): (f32, f32)) -> bool {
    x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
}

/// The status line's tools while comparing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    SideBySide,
    Slider,
    Swap,
    Done,
}

pub fn act(app: &mut App, tool: Tool) {
    match tool {
        Tool::SideBySide => app.model.set_compare_layout(Layout::SideBySide),
        Tool::Slider => app.model.set_compare_layout(Layout::Slider),
        Tool::Swap => app.model.swap_compared(),
        Tool::Done => leave(app),
    }
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
                let (frames, comparison) = VIEW.with(|v| {
                    let v = v.borrow();
                    (v.frames, v.comparison.clone())
                });
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
                let (over_frame, slider) = VIEW.with(|v| {
                    let v = v.borrow();
                    (v.frames.iter().any(|r| contains(r, point)), v.comparison.as_ref().is_some_and(|c| c.layout == Layout::Slider))
                });
                let cursor = if over_frame && !slider { IDC_HAND } else if over_frame { IDC_SIZEWE } else { IDC_ARROW };
                let _ = SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
