//! Compare's view (ticket 46, `CompareCanvas.swift`): it takes the
//! canvas's place while comparing. Side by side, each photograph in its own
//! frame: over it its number in a ring (the edited one's in pencil) and what
//! the camera wrote down, the facts that differ underlined; under it its
//! film, printed on its paper. As a slider, one frame with the first left
//! of a line that drags, the first's words on the left and the second's on
//! the right. A click on the other photograph edits it. The tools (Side by
//! side · Slider, Swap, Fit · 100%, Done) are in the status line; keys S,
//! ← →, Esc, C, Ctrl+0 and Ctrl+1.
//!
//! At 100% both are linked (`ZoomLink`): the open photograph one pixel to
//! a screen pixel, the other as much past its fit, both about the same
//! point. A drag pans both; in the slider the knob moves the line. A double
//! click zooms between Fit and 100%.
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
    /// At 100% (`model.fit_image` off), the zoom past each fit, and the
    /// point both are centred on, as fractions of the photographs.
    actual: bool,
    relative: f64,
    centre: (f64, f64),
    /// The zoom command last followed.
    zoom_command: u64,
    /// How far a screen pixel moves the centre across the open photograph.
    pan_unit: (f64, f64),
    /// A press in a frame, and what a drag from it does.
    press: Option<Press>,
    /// The side the open photograph is on, which 100% is a pixel to a pixel.
    open: usize,
    /// Side by side, one over the other.
    column: bool,
}

impl Default for View {
    fn default() -> Self {
        let none = RectF::new(0.0, 0.0, 0.0, 0.0);
        Self {
            hwnd: HWND::default(),
            bitmaps: [None, None],
            aspects: [1.5; 2],
            words: Default::default(),
            comparison: None,
            frames: [none, none],
            actual: false,
            relative: 1.0,
            centre: (0.5, 0.5),
            zoom_command: 0,
            pan_unit: (0.0, 0.0),
            press: None,
            open: 0,
            column: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Press {
    /// Where it went down, in pixels.
    at: (i32, i32),
    drag: Drag,
    moved: bool,
    /// The centre when it went down.
    centre: (f64, f64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drag {
    /// Moves the slider's line.
    Split,
    Pan,
}

/// The frame's part of `image` shown at `relative` past its fit, centred
/// on `centre` where the photograph allows: (x, y, width, height) in its
/// pixels.
fn shown_part(frame: RECT, image: (f64, f64), relative: f64, centre: (f64, f64)) -> (f64, f64, f64, f64) {
    let (fw, fh) = ((frame.right - frame.left) as f64, (frame.bottom - frame.top) as f64);
    let (iw, ih) = image;
    let scale = (fw / iw).max(fh / ih) * relative.max(1.0);
    let (sw, sh) = ((fw / scale).min(iw), (fh / scale).min(ih));
    let sx = (centre.0 * iw - sw / 2.0).clamp(0.0, iw - sw);
    let sy = (centre.1 * ih - sh / 2.0).clamp(0.0, ih - sh);
    (sx, sy, sw, sh)
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
                        // The captions, and the status line's "differ in".
                        app.model.dirty |= crate::model::dirty::COMPARE | crate::model::dirty::STATUS;
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
            style: CS_DBLCLKS,
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
        let open = |side: &Side| matches!(side, Side::Photo(item) if app.model.library_item().as_ref() == Some(item));
        v.open = if open(&comparison.first) || !open(&comparison.second) { 0 } else { 1 };
        v.comparison = Some(comparison);
        // Fit and 100% (Ctrl+0, Ctrl+1, the status line's words) zoom both.
        if v.zoom_command != app.model.zoom_command {
            v.zoom_command = app.model.zoom_command;
            v.actual = !app.model.fit_image;
            v.centre = (0.5, 0.5);
        }
    });
    super::status::set_fitting(app.model.fit_image);
    sync_note(app);
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
    let end = if inline { fx + fm.width + 14.0 } else { edge } + km.width + 8.0 + pm.width;
    if end > limit {
        return;
    }
    let (x, baseline) = if inline { (fx + fm.width + 14.0, top + fm.baseline) } else { (edge, top + fm.height + 4.0 + pm.baseline) };
    let x = if right && !inline { edge - km.width - 8.0 - pm.width } else { x };
    p.text("printed on", &KIND, x, baseline - km.baseline, Ink::hex(ink.dim));
    p.text(paper, &PAPER, x + km.width + 8.0, baseline - pm.baseline, Ink::hex(ink.paper));
}

/// The columns `x0..x1` of `image` covering `frame` (its largest centred
/// part with the frame's shape, as `draw_thumbnail` draws all of it), or
/// at 100% the part `shown_part` gives.
fn draw_part(c: &mut Canvas, frame: RECT, x0: i32, x1: i32, image: &Bitmap, relative: f64, centre: (f64, f64)) {
    let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
    if fw <= 0 || fh <= 0 || x1 <= x0 {
        return;
    }
    let (sx, sy, sw, sh) = shown_part(frame, (image.width as f64, image.height as f64), relative, centre);
    // The source columns under x0..x1.
    let u0 = sx + (x0 - frame.left) as f64 / fw as f64 * sw;
    let u1 = sx + (x1 - frame.left) as f64 / fw as f64 * sw;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width as i32,
            biHeight: -(image.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let dc = c.hdc();
        let previous = SetStretchBltMode(dc, HALFTONE);
        let _ = SetBrushOrgEx(dc, 0, 0, None);
        StretchDIBits(
            dc,
            x0,
            frame.top,
            x1 - x0,
            fh,
            u0.round() as i32,
            sy.round() as i32,
            ((u1 - u0).round() as i32).max(1),
            sh.round() as i32,
            Some(image.bgra.as_ptr() as *const _),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SetStretchBltMode(dc, STRETCH_BLT_MODE(previous));
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
        let mut column = false;
        let frames: [RectF; 2] = if slider {
            let f = rules::slider_frame((w, h), v.aspects[0]);
            let r = RectF::new(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32);
            [r, r]
        } else {
            let (arrangement, f) = rules::layout((w, h), (v.aspects[0], v.aspects[1]));
            column = arrangement == rules::Arrangement::Column;
            if v.column != column {
                // The note and the status line say top and bottom.
                v.column = column;
                post(|app: &mut App| app.model.dirty |= crate::model::dirty::COMPARE | crate::model::dirty::STATUS);
            }
            f.map(|f| RectF::new(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32))
        };
        v.frames = frames;
        // At 100%: the open photograph, first or second, a pixel to a pixel.
        let open = if v.bitmaps[v.open].is_some() { v.open } else { 1 - v.open };
        let (relative, centre) = match (&v.bitmaps[open], v.actual) {
            (Some((_, image)), true) => {
                let frame = px(frames[open]);
                let (fw, fh) = ((frame.right - frame.left).max(1) as f64, (frame.bottom - frame.top).max(1) as f64);
                let (iw, ih) = (image.width as f64, image.height as f64);
                let relative = (1.0 / (fw / iw).max(fh / ih)).max(1.0);
                let (_, _, sw, sh) = shown_part(frame, (iw, ih), relative, v.centre);
                // The centre where the photograph allows, so a pan starts from what's seen.
                let (hx, hy) = (sw / iw / 2.0, sh / ih / 2.0);
                v.centre = (v.centre.0.clamp(hx, 1.0 - hx), v.centre.1.clamp(hy, 1.0 - hy));
                v.pan_unit = (sw / iw / fw, sh / ih / fh);
                (relative, v.centre)
            }
            _ => (1.0, (0.5, 0.5)),
        };
        v.relative = relative;
        if slider {
            let frame = px(frames[0]);
            let split = frame.left + ((frame.right - frame.left) as f64 * comparison.split).round() as i32;
            c.round_rect(draw::RectF::of(frame), 0.0, blank);
            for (i, (x0, x1)) in [(0, (frame.left, split)), (1, (split, frame.right))] {
                if let Some((_, image)) = &v.bitmaps[i] {
                    draw_part(&mut c, frame, x0, x1, image, relative, centre);
                }
            }
        } else {
            for i in 0..2 {
                let frame = px(frames[i]);
                match &v.bitmaps[i] {
                    Some((_, image)) if relative > 1.0 => draw_part(&mut c, frame, frame.left, frame.right, image, relative, centre),
                    _ => super::inspector::draw_thumbnail(&mut c, frame, 0.0, v.bitmaps[i].as_ref().map(|(_, b)| b.as_ref()), false, blank, blank),
                }
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
                    label(p, &ink, &words[i], f.x, f.y + f.h + 8.0, false, false, w as f32 - 12.0);
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
                let (frames, comparison, actual, centre) = VIEW.with(|v| {
                    let v = v.borrow();
                    (v.frames, v.comparison.clone(), v.relative > 1.0, v.centre)
                });
                let Some(comparison) = comparison else { return LRESULT(0) };
                let slider = comparison.layout == Layout::Slider;
                let Some(i) = frames.iter().position(|r| contains(r, p)) else { return LRESULT(0) };
                if !slider && !actual {
                    // Fitted side by side: a press edits it.
                    with_app(|app| app.model.activate_compared(i == 0));
                    return LRESULT(0);
                }
                // Fitted, a drag anywhere moves the slider's line; at 100% only the knob does.
                let f = frames[0];
                let line = f.x + f.w * comparison.split as f32;
                let on_knob = (p.0 - line).abs() <= 16.0 && (p.1 - (f.y + f.h / 2.0)).abs() <= 16.0;
                let drag = if slider && (!actual || on_knob) { Drag::Split } else { Drag::Pan };
                let pixels = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
                VIEW.with(|v| v.borrow_mut().press = Some(Press { at: pixels, drag, moved: false, centre }));
                SetCapture(hwnd);
                if drag == Drag::Split && on_knob {
                    with_app(|app| app.model.move_split(((p.0 - f.x) / f.w.max(1.0)) as f64));
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let Some(mut press) = VIEW.with(|v| v.borrow().press) else { return LRESULT(0) };
                let pixels = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
                let (dx, dy) = (pixels.0 - press.at.0, pixels.1 - press.at.1);
                if !press.moved && dx.abs() + dy.abs() <= (2.0 * draw::dpi_of(hwnd)).round() as i32 {
                    return LRESULT(0);
                }
                press.moved = true;
                VIEW.with(|v| v.borrow_mut().press = Some(press));
                match press.drag {
                    Drag::Split => {
                        let p = at(lparam, hwnd);
                        let frame = VIEW.with(|v| v.borrow().frames[0]);
                        let split = ((p.0 - frame.x) / frame.w.max(1.0)) as f64;
                        with_app(|app| app.model.move_split(split));
                    }
                    Drag::Pan => {
                        VIEW.with(|v| {
                            let mut v = v.borrow_mut();
                            v.centre = (press.centre.0 - dx as f64 * v.pan_unit.0, press.centre.1 - dy as f64 * v.pan_unit.1);
                        });
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                let press = VIEW.with(|v| v.borrow_mut().press.take());
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                    // A click without a drag edits the photograph under it, or on its side of the line.
                    if press.is_some_and(|p| !p.moved) {
                        let p = at(lparam, hwnd);
                        let (frames, comparison) = VIEW.with(|v| {
                            let v = v.borrow();
                            (v.frames, v.comparison.clone())
                        });
                        if let Some(c) = comparison {
                            let first = if c.layout == Layout::Slider {
                                (p.0 - frames[0].x) / frames[0].w.max(1.0) < c.split as f32
                            } else {
                                !contains(&frames[1], p)
                            };
                            with_app(|app| app.model.activate_compared(first));
                        }
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                // Fit ↔ 100%, both photographs.
                let p = at(lparam, hwnd);
                if VIEW.with(|v| v.borrow().frames.iter().any(|r| contains(r, p))) {
                    with_app(|app| {
                        let fit = !app.model.fit_image;
                        app.model.zoom(fit);
                    });
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
                    let f = v.frames[0];
                    // At 100% only the knob moves the slider's line.
                    let line = v.comparison.as_ref().map(|c| f.x + f.w * c.split as f32);
                    let on_knob = line.is_some_and(|x| (point.0 - x).abs() <= 16.0 && (point.1 - (f.y + f.h / 2.0)).abs() <= 16.0);
                    let slider = v.comparison.as_ref().is_some_and(|c| c.layout == Layout::Slider) && (v.relative <= 1.0 || on_knob);
                    (v.frames.iter().any(|r| contains(r, point)), slider)
                });
                let cursor = if over_frame && !slider { IDC_HAND } else if over_frame { IDC_SIZEWE } else { IDC_ARROW };
                let _ = SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// MARK: The panel's note

/// The note's height under the pane header, in DIPs (`CompareEditingNote`).
pub const NOTE_HEIGHT: f64 = 36.0;
const NOTE_EDIT: u16 = 1;
const EDITING: TextStyle = TextStyle::ui(12.0, 400);
const NOTE_WORDS: TextStyle = TextStyle::serif(14.0, 400).italic();
const NOTE_NUMBER: TextStyle = TextStyle::serif(14.0, 600);

thread_local! {
    static NOTE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn note() -> HWND {
    HWND(NOTE.with(std::cell::Cell::get) as *mut _)
}

/// "Editing ③ the left one" near the top of the panel while comparing,
/// under the pane header. Against the original with the original active,
/// "The original can't be edited." and Edit 3.
pub fn layout_note(app: &App, rect: RECT, shown: bool) {
    if !shown && note().is_invalid() {
        return;
    }
    if note().is_invalid() {
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(note_proc),
                hInstance: super::instance(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("NegsCompareNote"),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
        let hwnd = super::create(w!("NegsCompareNote"), "", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), app.main, 0, HFONT::default());
        let edit = super::create(w!("BUTTON"), "Edit", WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), hwnd, NOTE_EDIT, HFONT::default());
        draw::set_role(edit, draw::Role::WordButton);
        draw::track_hover(edit);
        super::dialogs::tooltip(edit, "Edit the developed photograph");
        NOTE.with(|n| n.set(hwnd.0 as isize));
    }
    unsafe {
        let _ = MoveWindow(note(), rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, true);
        let _ = ShowWindow(note(), if shown { SW_SHOWNA } else { SW_HIDE });
    }
    sync_note(app);
}

/// The note's words, and Edit for the original's other side.
fn note_words(app: &App) -> Option<(bool, String, String)> {
    let c = app.model.comparison()?;
    let original = c.active().is_original();
    let number = |side: &Side| match side {
        Side::Photo(item) | Side::Original(item) => app.model.frame_number(item),
    };
    let column = VIEW.with(|v| v.borrow().column);
    let place = if c.layout == Layout::SideBySide && column {
        if c.active_first { "top" } else { "bottom" }
    } else if c.active_first {
        "left"
    } else {
        "right"
    };
    Some(if original { (true, number(c.inactive()), String::new()) } else { (false, number(c.active()), format!("the {place} one")) })
}

thread_local! {
    static NOTE_WORDS_SHOWN: RefCell<Option<(bool, String, String)>> = const { RefCell::new(None) };
}

fn sync_note(app: &App) {
    let hwnd = note();
    if hwnd.is_invalid() {
        return;
    }
    let words = note_words(app);
    if NOTE_WORDS_SHOWN.with(|w| w.replace(words.clone())) == words {
        return;
    }
    let edit = unsafe { GetDlgItem(hwnd, NOTE_EDIT as i32).unwrap_or_default() };
    let original = words.as_ref().is_some_and(|w| w.0);
    if let Some((_, number, _)) = words.as_ref().filter(|w| w.0) {
        let title = crate::platform::wide(&format!("Edit {number}"));
        unsafe {
            let _ = SetWindowTextW(edit, crate::platform::pcwstr(&title));
        }
    }
    unsafe {
        let _ = ShowWindow(edit, if original { SW_SHOWNA } else { SW_HIDE });
    }
    place_note_edit(hwnd);
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
        let _ = InvalidateRect(edit, None, false);
    }
}

/// Edit at the gutter on the right, its word's width.
fn place_note_edit(hwnd: HWND) {
    let edit = unsafe { GetDlgItem(hwnd, NOTE_EDIT as i32).unwrap_or_default() };
    let s = draw::dpi_of(hwnd) as f64;
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let margin = super::ink::MARGIN;
    let width = super::ink::width(&draw::window_text(edit), &super::ink::WORD) as f64 + 2.0 * margin;
    let right = client.right as f64 / s - super::ink::GUTTER + margin;
    let height = 18.0 + 2.0 * margin;
    unsafe {
        let _ = MoveWindow(edit, ((right - width) * s).round() as i32, (((NOTE_HEIGHT - height) / 2.0) * s).round() as i32, (width * s).round() as i32, (height * s).round() as i32, true);
    }
}

fn paint_note(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let words = NOTE_WORDS_SHOWN.with(|w| w.borrow().clone());
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(t.hex.panel));
    let h = client.bottom as f32 / s;
    c.d2d(s, t.plain, |p| {
        let Some((original, number, place)) = words.clone() else { return };
        let x = super::ink::GUTTER as f32;
        let middle = h / 2.0;
        let centred = |p: &mut Painter, text: &str, style: &TextStyle, x: f32, ink: u32| -> f32 {
            let m = p.measure(text, style, 0.0);
            // The x-height's middle on the row's.
            p.text(text, style, x, (middle + style.size * 0.35 - m.baseline).round(), Ink::hex(ink));
            m.width
        };
        if original {
            centred(p, "The original can\u{2019}t be edited.", &NOTE_WORDS, x, t.hex.label);
            return;
        }
        let mut x = x + centred(p, "Editing", &EDITING, x, t.hex.dim) + 8.0;
        // The number in the pencil's ring (`CompareRing`, active).
        let m = p.measure(&number, &NOTE_NUMBER, 0.0);
        let ring = RectF::new(x, middle - 12.5, (m.width + 12.0).max(28.0), 25.0);
        p.ring(ring, negs_marks::Radii::NUMBER, -0.8, Ink::hex(t.hex.accent));
        p.text(&number, &NOTE_NUMBER, ring.x + (ring.w - m.width) / 2.0, (middle + NOTE_NUMBER.size * 0.35 - m.baseline).round(), Ink::hex(t.hex.paper));
        x += ring.w + 8.0;
        centred(p, &place, &NOTE_WORDS, x, t.hex.label);
    });
    c.present(dc);
}

extern "system" fn note_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_note(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                place_note_edit(hwnd);
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_COMMAND => {
                if (wparam.0 & 0xffff) as u16 == NOTE_EDIT {
                    // The developed one, beside its original.
                    with_app(|app| {
                        if let Some(first) = app.model.comparison().map(|c| !c.active_first) {
                            app.model.activate_compared(first);
                        }
                    });
                }
                LRESULT(0)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// What the two differ in, for the status line ("shutter and aperture"):
/// nothing against the original, or while either's facts are unknown.
pub fn differences(app: &App) -> String {
    let Some(c) = app.model.comparison() else { return String::new() };
    let (Side::Photo(a), Side::Photo(b)) = (&c.first, &c.second) else { return String::new() };
    let (fa, fb) = (facts(&a.photo), facts(&b.photo));
    let (sa, sb) = (app.model.compared_stocks(a), app.model.compared_stocks(b));
    let (pa, pb) = (fa.map(|f| f.parts('.')), fb.map(|f| f.parts('.')));
    rules::differences(pa.as_ref(), pb.as_ref(), (&sa.0, &sb.0), (sa.1.as_deref(), sb.1.as_deref()))
}

/// Stacked: ← → step the bottom one.
pub fn stacked(app: &App) -> bool {
    app.model.comparison().is_some_and(|c| c.layout == Layout::SideBySide) && VIEW.with(|v| v.borrow().column)
}
