//! The grid (`LibraryGrid.swift`, `SheetFlight.swift`): the folder's
//! photographs as a contact sheet over the whole window under the title
//! bar, packed edge to edge in rows at their own proportions, the sentence
//! over it ("Showing all ⌄ 12 photographs", the filter's words to change
//! it, × to clear it) and the foot under it (the count, the Size slider).
//! G shows it and hides it, E, Enter, Esc and a double click go back to the
//! photograph; click, Ctrl, Shift, a band and the arrows select; Ctrl+plus,
//! minus and the wheel change the size. It unfolds out of the strip and
//! folds back into it: the sheet grows from the strip's band as the
//! frames fly to their places, drawn in a DirectComposition layer over the
//! window (`Surface::layer`) whose clip is the growing edge; the grid's own
//! window shows once it has landed. The flight is paced by the compositor
//! and holds the thumbnails' developing, which shares the GPU.
//!
//! Made on first use; only the rows in view are painted, through Direct2D
//! on the GPU (`surface.rs`), with the strip's frames and marks. A wheel's
//! notch glides there and eases off; the overlay scroll bar at the right
//! edge shows while it scrolls (and under the pointer), then fades, and
//! can be dragged or clicked above or below its knob to page.

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use negs_library::{Flag, Item};

use super::d2d::{BoxInk, Ink, Painter, RectF, TextStyle};
use super::strip::{self, Cell};
use super::surface::Surface;
use super::theme::{self, Tokens};
use super::{App, with_app};
use crate::model::dirty;
use crate::sheet_rules::{self as rules, Part, Rect, SheetLayout, ThumbSize};

/// The flight's next frame, posted once the compositor has shown one.
const WM_FLIGHT_FRAME: u32 = WM_APP + 1;
/// A wheel's glide and the scroll bar's fade.
const TIMER_SCROLL: usize = 2;
const SENTENCE: TextStyle = TextStyle::serif(17.0, 400).italic();
const FOOT: TextStyle = TextStyle::ui(12.0, 400);
/// The Size slider's track at the foot's right.
const SLIDER: f64 = 72.0;

struct Flight {
    opening: bool,
    started: Instant,
    /// Each cell's place in the strip (window DIPs), by index.
    from: Vec<Option<Rect>>,
    longest: f64,
    current: Option<usize>,
}

#[derive(Default)]
pub struct Grid {
    hwnd: HWND,
    shown: bool,
    cells: Vec<Cell>,
    layout: SheetLayout,
    scroll: f64,
    /// The window's whole rect (pixels, in the main window) and the strip's band.
    rect: RECT,
    band: RECT,
    surface: Surface,
    /// The flight's sheet, over the whole window (`Surface::layer`).
    layer: Surface,
    flight: Option<Flight>,
    /// A band selection: where it started (sheet DIPs) and the selection under it.
    dragging: Option<((f64, f64), (f64, f64), std::collections::HashSet<Item>)>,
    sliding: bool,
    /// The sentence's words that act, as painted.
    places: Vec<(RectF, Act)>,
    painted: u64,
    /// Where a wheel's glide is going, and when it last moved.
    glide: Option<f64>,
    ticked: Option<Instant>,
    /// When the grid last scrolled: the scroll bar shows a second after.
    scrolled_at: Option<Instant>,
    /// The pointer is over the scroll bar's edge (it widens and stays).
    bar_hover: bool,
    /// Dragging the knob: where in it the pointer took it.
    bar_drag: Option<f64>,
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Filter,
    Clear,
    Size,
    /// A batch export's Cancel and Show in the foot (`ui::batch`).
    BatchCancel,
    BatchShow,
}

pub fn shown(app: &App) -> bool {
    app.grid.as_ref().is_some_and(|g| g.shown)
}

fn scale(hwnd: HWND) -> f64 {
    unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

fn register() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsGrid"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            style: CS_DBLCLKS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
}

/// Where the grid goes: `rect` under the title bar; the strip's band for
/// the flight. Nothing before its first use.
pub fn layout(app: &mut App, rect: RECT) {
    let mut band = RECT::default();
    unsafe {
        let _ = GetWindowRect(app.strip, &mut band);
        let mut status = RECT::default();
        let _ = GetWindowRect(app.status, &mut status);
        let mut points = [POINT { x: band.left, y: band.top }, POINT { x: status.right.max(band.right), y: status.bottom.max(band.bottom) }];
        MapWindowPoints(None, app.main, &mut points);
        band = RECT { left: points[0].x, top: points[0].y, right: points[1].x, bottom: points[1].y };
    }
    let Some(grid) = app.grid.as_mut() else { return };
    grid.rect = rect;
    grid.band = band;
    if grid.shown && grid.flight.is_none() {
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        unsafe {
            let _ = SetWindowPos(grid.hwnd, HWND_TOP, rect.left, rect.top, w, h, SWP_NOACTIVATE);
        }
        relayout(app);
    }
}

/// The device is made again.
pub fn surfaces_ready(app: &mut App) {
    if let Some(grid) = &app.grid {
        invalidate(grid.hwnd);
    }
}

fn sheet_size(grid: &Grid) -> (f64, f64) {
    let s = scale(grid.hwnd);
    let (w, h) = ((grid.rect.right - grid.rect.left) as f64 / s, (grid.rect.bottom - grid.rect.top) as f64 / s);
    (w, (h - rules::SENTENCE - rules::FOOT).max(0.0))
}

/// Follows the model: the photographs, their marks and shapes, the
/// selection. Lays the sheet out again keeping the photograph in view
/// where it was.
pub fn sync(app: &mut App) {
    if !shown(app) {
        return;
    }
    let model = &app.model;
    let Some(library) = &model.shelf.library else { return close(app, false) };
    let open = library.open_item().cloned();
    let unedited = super::thumbs::unedited_look(model);
    let cells: Vec<Cell> = model
        .shown_items()
        .into_iter()
        .map(|item| Cell {
            want: super::thumbs::want(model, &item, unedited.as_deref()),
            name: library.name(&item),
            summary: library.summary(&item),
            open: open.as_ref() == Some(&item),
            selected: model.shelf.selection.contains(&item),
            pair: library.companion(&item.photo).is_some(),
        })
        .collect();
    let grid = app.grid.as_mut().unwrap();
    let changed = grid.cells != cells;
    grid.cells = cells;
    let laid = relayout(app);
    request(app);
    let grid = app.grid.as_mut().unwrap();
    if changed || laid || grid.painted != app.thumbs.revision {
        invalidate(grid.hwnd);
    }
}

/// Packs the rows anew; true if they moved. The photograph a third of the
/// way down the view keeps its place (`keep_anchor`).
fn relayout(app: &mut App) -> bool {
    let size = app.model.prefs.grid_size;
    let Some(grid) = app.grid.as_mut() else { return false };
    let ratios: Vec<f64> = grid.cells.iter().map(|c| app.thumbs.ratio(&c.want).unwrap_or(rules::FALLBACK_RATIO)).collect();
    let (width, viewport) = sheet_size(grid);
    let layout = SheetLayout::new(&ratios, width, size);
    if layout == grid.layout {
        return false;
    }
    let anchor = grid.layout.hit(width / 2.0, grid.scroll + viewport / 3.0).or_else(|| grid.layout.nearest(width / 2.0, grid.scroll + viewport / 3.0));
    if let (Some(index), true) = (anchor, grid.layout.frames.len() == layout.frames.len()) {
        let f = grid.layout.frames[index];
        let down = ((grid.scroll + viewport / 3.0 - f.y) / f.h.max(1.0)).clamp(0.0, 1.0);
        grid.scroll = rules::keep_anchor(&layout, index, down, f.y + f.h * down - grid.scroll, viewport);
    }
    grid.layout = layout;
    clamp(grid);
    true
}

fn clamp(grid: &mut Grid) {
    let (_, viewport) = sheet_size(grid);
    grid.scroll = grid.scroll.max(0.0).min((grid.layout.height - viewport).max(0.0));
}

fn on_screen(grid: &Grid) -> std::ops::Range<usize> {
    let (_, viewport) = sheet_size(grid);
    grid.layout.indices(grid.scroll, grid.scroll + viewport)
}

/// The rows in view, then ±32 around them; the pump develops what's missing.
fn request(app: &mut App) {
    let Some(grid) = app.grid.as_ref() else { return };
    let range = on_screen(grid);
    let shown: Vec<usize> = range.clone().collect();
    let open = grid.cells.iter().position(|c| c.open);
    let ahead = rules::prefetch_order(grid.cells.len(), &shown, open);
    let wants: Vec<&super::thumbs::Want> = shown.iter().chain(&ahead).map(|&i| &grid.cells[i].want).collect();
    app.thumbs.read(&wants, ThumbSize::for_rows(app.model.prefs.grid_size, scale(grid.hwnd)));
    let all: Vec<super::thumbs::Want> = grid.cells.iter().map(|c| c.want.clone()).collect();
    app.thumbs.pump(&app.model, &all, range, open);
}

// MARK: Showing and hiding

/// G: the grid, unfolding out of the strip (made the first time).
pub fn open(app: &mut App) {
    if shown(app) || app.model.shelf.library.is_none() || app.model.cropping() || app.model.source.is_none() {
        return;
    }
    crate::trace::event("grid_open", || serde_json::json!({}));
    if app.grid.is_none() {
        register();
        let hwnd = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("NegsGrid"), w!("Grid"), WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP, 0, 0, 1, 1, app.main, None, super::instance(), None)
        }
        .unwrap_or_default();
        app.grid = Some(Grid { hwnd, layer: Surface::layer(), ..Default::default() });
        app.layout();
    }
    // The grid shows the open photograph as edited.
    app.model.file_open_thumbnail();
    app.model.shelf.in_grid = true;
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
    // The editor closed: its film side won't be used soon (ticket 41).
    app.model.release_film_cache(crate::film_cache_release::Reason::EditorClosed);
    // Not before the flight lands (`start_flight`).
    app.thumbs.held = true;
    let grid = app.grid.as_mut().unwrap();
    grid.shown = true;
    grid.cells.clear();
    grid.layout = SheetLayout::default();
    sync(app);
    // The open photograph's row in the middle.
    let grid = app.grid.as_mut().unwrap();
    let (_, viewport) = sheet_size(grid);
    if let Some(row) = grid.cells.iter().position(|c| c.open).and_then(|i| grid.layout.row_of(i)) {
        grid.scroll = grid.layout.centre_row(row, viewport);
        clamp(grid);
    }
    start_flight(app, true);
}

/// E, Enter, Esc, a double click: back to the photograph, folding into the
/// strip. `open` the anchor first, if it's another.
pub fn close(app: &mut App, open_anchor: bool) {
    if !shown(app) {
        return;
    }
    if open_anchor {
        let current = app.model.shelf.selection.current().cloned();
        if let Some(item) = current.filter(|i| app.model.library_item().as_ref() != Some(i)) {
            app.model.open_item(item);
        }
    }
    app.model.shelf.in_grid = false;
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
    start_flight(app, false);
    unsafe {
        let _ = SetFocus(app.strip);
    }
}

pub fn toggle(app: &mut App) {
    if shown(app) { close(app, true) } else { open(app) }
}

/// Closes the grid for Compare, which has no strip to fold the sheet into:
/// at once, without the flight.
pub fn close_for_compare(app: &mut App) {
    if !shown(app) {
        return;
    }
    app.model.shelf.in_grid = false;
    finish_flight(app, false);
}

fn start_flight(app: &mut App, opening: bool) {
    // The thumbnails' developing waits for the flight: it shares the GPU,
    // and a frame waiting behind it is late.
    app.thumbs.held = true;
    let reduce = !unsafe {
        let mut on = BOOL(1);
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        on.as_bool()
    };
    let strip_shown = app.model.prefs.filmstrip_visible;
    let grid = app.grid.as_mut().unwrap();
    let s = scale(grid.hwnd);
    // The strip's frames in the grid window's DIPs.
    let (ox, oy) = ((grid.band.left - grid.rect.left) as f64 / s, (grid.band.top - grid.rect.top) as f64 / s);
    let strip = &app.strip_view;
    let from: Vec<Option<Rect>> = grid
        .cells
        .iter()
        .map(|c| {
            let i = strip.cells.iter().position(|s| s.want.item == c.want.item)?;
            (strip_shown && i < strip.layout.count())
                .then(|| Rect::new(ox + strip.layout.xs[i] - strip.scroll, oy + rules::STRIP_TOP, strip.layout.widths[i], rules::PICTURE))
        })
        .collect();
    let current = grid.cells.iter().position(|c| c.open);
    let range = on_screen(grid);
    let longest = range.map(|i| rules::flight_delay(i, current)).fold(0.0, f64::max);
    if reduce {
        grid.flight = None;
        finish_flight(app, opening);
        return;
    }
    // Folding before it has unfolded: the frames already coming carry on.
    let running = grid.flight.is_some();
    grid.flight = Some(Flight { opening, started: Instant::now(), from, longest, current });
    crate::trace::event("grid_flight_start", || serde_json::json!({"opening": opening, "length_ms": rules::flight_length(longest)}));
    // The sheet flies in a layer over the whole window, clipped to its
    // growing edge by the compositor; the grid's own window shows only once
    // it has landed. Resizing (or reshaping) a swap chain's window every
    // frame stalls its presents for two or three refreshes.
    if !running {
        tick(app);
    }
}

fn finish_flight(app: &mut App, opening: bool) {
    let grid = app.grid.as_mut().unwrap();
    grid.flight = None;
    let hwnd = grid.hwnd;
    if opening {
        let r = grid.rect;
        unsafe {
            let _ = SetWindowPos(hwnd, HWND_TOP, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        // The window's sheet on screen, under the layer, before the layer goes.
        paint(app, hwnd);
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            let _ = SetFocus(hwnd);
        }
        app.grid.as_mut().unwrap().layer.release();
    } else {
        grid.shown = false;
        grid.surface.release();
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        grid.layer.release();
    }
    app.thumbs.held = false;
    if opening {
        request(app);
    }
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
}

fn pixels(r: &RECT) -> Rect {
    Rect::new(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64)
}

/// One frame of the flight: the layer clipped to the sheet's growing edge,
/// the frame painted and committed, and, once the compositor has shown it,
/// the next one asked for, which paces the flight at the display's refresh.
fn tick(app: &mut App) {
    let grid = app.grid.as_mut().unwrap();
    let Some(flight) = &grid.flight else { return };
    let elapsed = flight.started.elapsed().as_secs_f64() * 1000.0;
    let length = rules::flight_length(flight.longest);
    let opening = flight.opening;
    if elapsed >= length {
        return finish_flight(app, opening);
    }
    let along = if opening { elapsed } else { length - elapsed };
    let mask = rules::mask_at(&pixels(&grid.band), &pixels(&grid.rect), along.min(rules::FLIGHT_MS));
    let (r, hwnd) = (grid.rect, grid.hwnd);
    let place = |grid: &Grid| {
        let (x, y) = ((mask.x - r.left as f64) as f32, (mask.y - r.top as f64) as f32);
        grid.layer.place((r.left as f32, r.top as f32), (x, y, x + mask.w as f32, y + mask.h as f32));
    };
    let started = Instant::now();
    let made = grid.layer.made();
    place(grid);
    if !paint(app, hwnd) {
        // No layer (no GPU, or no DirectComposition): it lands at once.
        return finish_flight(app, opening);
    }
    let grid = app.grid.as_mut().unwrap();
    if !made {
        // The layer was just made: placed once it exists.
        place(grid);
    }
    if !made && !opening {
        // Folding: the sheet in the layer on screen, then the window under it goes.
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    let (draw, commit) = grid.layer.timing;
    let took = started.elapsed();
    crate::trace::event("grid_flight_paint", || {
        serde_json::json!({"at_ms": elapsed, "paint_ms": took.as_secs_f64() * 1000.0, "draw_ms": draw, "commit_ms": commit})
    });
    // The frame waits for the compositor to show it (a layer isn't
    // presented), which paces the flight at the display's refresh.
    let t0 = Instant::now();
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
    crate::trace::event("grid_flight_flush", || serde_json::json!({"ms": t0.elapsed().as_secs_f64() * 1000.0}));
    unsafe {
        let _ = PostMessageW(hwnd, WM_FLIGHT_FRAME, WPARAM(0), LPARAM(0));
    }
}

// MARK: Painting

/// Paints the sheet: in the layer while it flies, else in the window. False
/// if it couldn't (no GPU, the window's painted plainly instead).
fn paint(app: &mut App, hwnd: HWND) -> bool {
    let t = theme::tokens();
    let s = scale(hwnd);
    let App { grid, thumbs, model, .. } = app;
    let Some(grid) = grid.as_mut() else { return false };
    let full = (grid.rect.right - grid.rect.left, grid.rect.bottom - grid.rect.top);
    let (w, h) = (full.0 as f64 / s, full.1 as f64 / s);
    let (_, viewport) = sheet_size(grid);
    let filter = model.library_filter();
    let total = model.shelf.library.as_ref().map_or(0, |l| l.items().len());
    let selected = model.shelf.selection.selected().len();
    let parts = rules::sentence(&filter, grid.cells.len(), total);
    let foot = rules::foot(grid.cells.len(), total, selected, filter.active());
    let size = model.prefs.grid_size;
    let elapsed = grid.flight.as_ref().map(|f| {
        let e = f.started.elapsed().as_secs_f64() * 1000.0;
        if f.opening { e } else { rules::flight_length(f.longest) - e }
    });
    let range = grid.layout.indices(grid.scroll - rules::GRID_MARGIN, grid.scroll + viewport + rules::GRID_MARGIN);
    let (bar, wide) = bar_look(grid);
    let mut places = std::mem::take(&mut grid.places);
    places.clear();
    let Grid { cells, layout, scroll, surface, layer, flight, .. } = grid;
    let flying = flight.is_some();
    let (surface, target) = if flying { (layer, model_main(hwnd)) } else { (surface, hwnd) };
    // A fold's last FOLD_FADE_MS fade the sheet off the strip.
    let fade = match (&*flight, elapsed) {
        (Some(f), Some(e)) if !f.opening => (e / rules::FOLD_FADE_MS).clamp(0.0, 1.0) as f32,
        _ => 1.0,
    };
    let painted = surface.paint(target, (full.0.max(1) as u32, full.1.max(1) as u32), s as f32, t.plain, |p| {
      p.fill_rect(RectF::new(0.0, 0.0, w as f32, h as f32), Ink::hex(t.film.film));
      p.faded(fade, |p| {
        p.fill_rect(RectF::new(0.0, 0.0, w as f32, h as f32), Ink::hex(t.hex.grid));
        let top = rules::SENTENCE - *scroll;
        let boxes = elapsed.map_or(1.0, |e| ((e / rules::FLIGHT_MS - rules::BOXES_FROM) / (1.0 - rules::BOXES_FROM)).clamp(0.0, 1.0)) as f32;
        p.clipped(RectF::new(0.0, rules::SENTENCE as f32, w as f32, viewport as f32), |p| {
            let placed: Vec<(usize, RectF)> = range
                .clone()
                .map(|i| {
                    let f = layout.frames[i];
                    let to = Rect::new(f.x, f.y + top, f.w, f.h);
                    let r = match (elapsed, flight.as_ref().and_then(|fl| fl.from.get(i).copied().flatten())) {
                        (Some(e), Some(from)) => rules::flyer_at(&from, &to, e, rules::flight_delay(i, flight.as_ref().unwrap().current)),
                        _ => to,
                    };
                    (i, RectF::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32))
                })
                .collect();
            for &(i, r) in &placed {
                let cell = &cells[i];
                let opacity = if cell.summary.flag == Some(Flag::Reject) { rules::reject_opacity(false, false) as f32 } else { 1.0 };
                strip::paint_frame(p, thumbs, cell, r, opacity, &t, false);
            }
            // The boxes reach past their pictures, over the gaps and the
            // thumbnails beside them, so they're drawn above every thumbnail:
            // all the pick boxes, then all the selection boxes (LibraryGrid).
            if boxes > 0.0 {
                for &(i, r) in &placed {
                    if cells[i].summary.flag == Some(Flag::Pick) {
                        p.pick_box(BoxInk::Grid, &cells[i].name, r, Ink::hex(t.film.paper).alpha(boxes), true);
                    }
                }
                for &(i, r) in &placed {
                    if cells[i].selected {
                        p.pick_box(BoxInk::Selection, &cells[i].name, r, Ink::hex(t.hex.accent).alpha(boxes), false);
                    }
                }
            }
        });
        paint_scroll_bar(p, &t, *scroll, layout.height, viewport, w as f32, rules::SENTENCE as f32, bar, wide);
        sentence(p, &parts, &t, 0.0, 0.0, &mut places);
        // A batch started here shows its progress here too, then its outcome.
        // Lightroom's words (ticket 48) come after a running batch's.
        let batch = model.batch.as_ref().filter(|b| b.running || b.needs_attention());
        let words = model.lightroom.words.as_deref().filter(|_| !batch.is_some_and(|b| b.running));
        let foot_y = (h - rules::FOOT) as f32;
        foot_line(p, if batch.is_some() || words.is_some() { "" } else { &foot }, size, &t, 0.0, foot_y, w as f32, &mut places);
        if let Some(words) = words {
            let m = p.measure(words, &FOOT, 0.0);
            let baseline = foot_y + (rules::FOOT as f32 / 2.0 + FOOT.size * 0.36).round();
            p.text(words, &FOOT, 14.0, baseline - m.baseline, Ink::hex(t.hex.label));
        } else if let Some(run) = batch {
            batch_foot(p, run, &t, 0.0, foot_y, &mut places);
        }
      });
    });
    grid.places = places;
    if flying {
        // The window shows nothing of its own while the layer flies.
        unsafe {
            let _ = ValidateRect(hwnd, None);
        }
        return painted;
    }
    if painted {
        grid.painted = app.thumbs.revision;
        unsafe {
            let _ = ValidateRect(hwnd, None);
        }
    } else {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            FillRect(dc, &client, theme::brush(theme::gdi(t.hex.grid)));
            let _ = EndPaint(hwnd, &ps);
        }
    }
    painted
}

fn model_main(hwnd: HWND) -> HWND {
    unsafe { GetParent(hwnd) }.unwrap_or_default()
}

/// The sentence: dim italic words, the filter's words in the label's ink
/// with the pencil under them while filtered, the × to clear.
fn sentence(p: &mut Painter, parts: &[Part], t: &Tokens, ox: f32, oy: f32, places: &mut Vec<(RectF, Act)>) {
    let baseline = oy + 30.0;
    let mut x = ox + 18.0;
    for part in parts {
        let (text, ink, act) = match part {
            Part::Words(words) => (words.as_str(), t.hex.dim, None),
            Part::Filter { word, .. } => (word.as_str(), t.hex.label, Some(Act::Filter)),
            Part::Clear => ("  ×", t.hex.dim, Some(Act::Clear)),
        };
        let m = p.measure(text, &SENTENCE, 0.0);
        p.text(text, &SENTENCE, x, baseline - m.baseline, Ink::hex(ink));
        if let Part::Filter { on, .. } = part {
            if *on {
                p.underline(RectF::new(x, baseline + 1.0, m.width, 5.0), t, false);
            } else {
                p.fill_rect(RectF::new(x, baseline + 2.0, m.width, 1.0), Ink::hex(t.hex.dim));
            }
            // The chevron, 7 × 3.5, drawn: Newsreader has no ⌄.
            let (cx, cy) = (x + m.width + 8.0, baseline - 4.0);
            p.line((cx - 3.5, cy - 1.75), (cx, cy + 1.75), 1.2, Ink::hex(t.hex.dim));
            p.line((cx, cy + 1.75), (cx + 3.5, cy - 1.75), 1.2, Ink::hex(t.hex.dim));
            places.push((RectF::new(x - 4.0, oy, m.width + 20.0, rules::SENTENCE as f32), Act::Filter));
            x += 14.0;
        } else if let Some(act) = act {
            places.push((RectF::new(x, oy, m.width + 6.0, rules::SENTENCE as f32), act));
        }
        x += m.width;
    }
}

/// The foot: the count at the left, Size and its slider at the right.
#[allow(clippy::too_many_arguments)]
fn foot_line(p: &mut Painter, count: &str, size: f64, t: &Tokens, ox: f32, y: f32, w: f32, places: &mut Vec<(RectF, Act)>) {
    let h = rules::FOOT as f32;
    let m = p.measure(count, &FOOT, 0.0);
    let baseline = y + (h / 2.0 + FOOT.size * 0.36).round();
    p.text(count, &FOOT, ox + 14.0, baseline - m.baseline, Ink::hex(t.hex.dim));
    let track = RectF::new(ox + w - 18.0 - SLIDER as f32, y + h / 2.0, SLIDER as f32, 1.0);
    p.fill_rect(track, Ink::hex(t.hex.track));
    let knob = track.x + track.w * rules::size_fraction(size) as f32;
    p.fill_ellipse(knob, track.y, 5.5, 5.5, Ink::hex(t.hex.label));
    p.fill_ellipse(knob, track.y, 4.0, 4.0, Ink::hex(t.hex.grid));
    let label = p.measure("Size", &FOOT, 0.0);
    p.text("Size", &FOOT, track.x - 14.0 - label.width, baseline - label.baseline, Ink::hex(t.hex.dim));
    places.push((RectF::new(track.x - 6.0, y, track.w + 12.0, h), Act::Size));
}

/// "Exporting 5 of 12 photographs", its rule and Cancel; or "Exported 10
/// of 12 · 2 failed" and Show, which opens the batch's popover.
fn batch_foot(p: &mut Painter, run: &crate::batch::Run, t: &Tokens, ox: f32, y: f32, places: &mut Vec<(RectF, Act)>) {
    const STRONG: TextStyle = TextStyle::ui(12.0, 600);
    let h = rules::FOOT as f32;
    let baseline = y + (h / 2.0 + FOOT.size * 0.36).round();
    let mut x = ox + 14.0;
    let mut word = |p: &mut Painter, text: &str, style: &TextStyle, ink: u32| {
        let m = p.measure(text, style, 0.0);
        p.text(text, style, x, baseline - m.baseline, Ink::hex(ink));
        let r = RectF::new(x, y, m.width, h);
        x += m.width + 5.0;
        r
    };
    if run.running {
        let words = word(p, &run.progress_words(), &FOOT, t.hex.label);
        let left = words.x + words.w + 9.0;
        let middle = (y + h / 2.0).round();
        p.fill_rect(RectF::new(left, middle, 150.0, 1.0), Ink::hex(t.hex.track));
        p.fill_rect(RectF::new(left, middle - 0.5, 150.0 * run.fraction().clamp(0.0, 1.0) as f32, 2.0), Ink::hex(t.hex.accent));
        let m = p.measure("Cancel", &FOOT, 0.0);
        let at = left + 150.0 + 16.0;
        p.text("Cancel", &FOOT, at, baseline - m.baseline, Ink::hex(t.hex.dim));
        places.push((RectF::new(at - 4.0, y, m.width + 8.0, h), Act::BatchCancel));
    } else {
        let (words, failed) = run.outcome_words();
        word(p, &words, &FOOT, t.hex.label);
        if let Some(failed) = failed {
            word(p, "·", &FOOT, t.hex.label);
            word(p, &failed, &STRONG, t.hex.paper);
        }
        let r = word(p, "Show", &FOOT, t.hex.dim);
        p.fill_rect(RectF::new(r.x, baseline + 2.0, r.w, 1.0), Ink::hex(t.hex.track));
        places.push((RectF::new(r.x - 4.0, y, r.w + 8.0, h), Act::BatchShow));
    }
}

// MARK: Input

fn point(hwnd: HWND, lparam: LPARAM) -> (f64, f64) {
    let s = scale(hwnd);
    ((lparam.0 & 0xffff) as i16 as f64 / s, ((lparam.0 >> 16) & 0xffff) as i16 as f64 / s)
}

/// The cell under a point in the window, if it's on the sheet.
fn cell_at(grid: &Grid, (x, y): (f64, f64)) -> Option<usize> {
    let (_, viewport) = sheet_size(grid);
    (y >= rules::SENTENCE && y < rules::SENTENCE + viewport).then(|| grid.layout.hit(x, y - rules::SENTENCE + grid.scroll)).flatten()
}

fn act_at(grid: &Grid, (x, y): (f64, f64)) -> Option<Act> {
    let (x, y) = (x as f32, y as f32);
    grid.places.iter().find(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(_, a)| *a)
}

fn set_size(app: &mut App, size: f64) {
    let size = rules::clamp_size(size);
    if (size - app.model.prefs.grid_size).abs() < 0.5 {
        return;
    }
    app.model.prefs.grid_size = size;
    app.model.prefs.save();
    relayout(app);
    request(app);
    if let Some(g) = &app.grid {
        invalidate(g.hwnd);
    }
}

fn slide(app: &mut App, x: f64) {
    let Some(grid) = &app.grid else { return };
    let s = scale(grid.hwnd);
    let w = (grid.rect.right - grid.rect.left) as f64 / s;
    let left = w - 18.0 - SLIDER;
    set_size(app, rules::size_at((x - left) / SLIDER));
}

fn scroll_by(app: &mut App, delta: f64) {
    let Some(grid) = app.grid.as_mut() else { return };
    grid.glide = None;
    grid.scroll += delta;
    clamp(grid);
    scrolled(grid);
    invalidate(grid.hwnd);
    request(app);
}

/// The grid scrolled: the bar shows, and fades a second later.
fn scrolled(grid: &mut Grid) {
    grid.scrolled_at = Some(Instant::now());
    unsafe {
        SetTimer(grid.hwnd, TIMER_SCROLL, 8, None);
    }
}

/// A wheel's turn, in Windows' units (120 a notch): a notch glides there,
/// adding to a glide under way; a touchpad's finer steps, which Windows
/// already coasts, go at once, as everything does with animations off.
fn wheel(app: &mut App, delta: f64) {
    let step = -delta * rules::WHEEL_STEP / 120.0;
    if delta % 120.0 != 0.0 || !super::motion::enabled() {
        return scroll_by(app, step);
    }
    let Some(grid) = app.grid.as_mut() else { return };
    let (_, viewport) = sheet_size(grid);
    let end = (grid.layout.height - viewport).max(0.0);
    let to = (grid.glide.unwrap_or(grid.scroll) + step).clamp(0.0, end);
    if grid.glide.is_none() {
        grid.ticked = Some(Instant::now());
    }
    grid.glide = Some(to);
    scrolled(grid);
}

/// The scroll bar's knob opacity now, and whether it's widened.
fn bar_look(grid: &Grid) -> (f32, bool) {
    if grid.flight.is_some() {
        return (0.0, false);
    }
    if grid.bar_hover || grid.bar_drag.is_some() {
        return (1.0, true);
    }
    let since = grid.scrolled_at.map_or(f64::INFINITY, |t| t.elapsed().as_secs_f64() * 1000.0);
    (rules::bar_opacity(since) as f32, false)
}

/// The glide's next step and the bar's fade; the timer stops once both are done.
fn scroll_tick(app: &mut App) {
    let Some(grid) = app.grid.as_mut() else { return };
    let now = Instant::now();
    let mut moved = false;
    if let Some(to) = grid.glide {
        let dt = grid.ticked.map_or(16.0, |t| (now - t).as_secs_f64() * 1000.0);
        grid.ticked = Some(now);
        let before = grid.scroll;
        grid.scroll = rules::wheel_glide(grid.scroll, to, dt);
        clamp(grid);
        moved = grid.scroll != before;
        if grid.scroll == to || !moved {
            grid.glide = None;
        }
        grid.scrolled_at = Some(now);
    }
    let since = grid.scrolled_at.map_or(f64::INFINITY, |t| (now - t).as_secs_f64() * 1000.0);
    let fading = since > rules::BAR_SHOWN_MS && since < rules::BAR_SHOWN_MS + rules::BAR_FADE_MS + 50.0;
    let held = grid.bar_hover || grid.bar_drag.is_some();
    // Held under the pointer, it waits for the pointer to leave.
    if grid.glide.is_none() && (held || since >= rules::BAR_SHOWN_MS + rules::BAR_FADE_MS + 50.0) {
        unsafe {
            let _ = KillTimer(grid.hwnd, TIMER_SCROLL);
        }
    }
    if moved || (fading && !held) {
        invalidate(grid.hwnd);
    }
    if moved {
        request(app);
    }
}

/// The knob's place in the window (DIPs: top, length), if the sheet scrolls.
fn knob(grid: &Grid) -> Option<(f64, f64)> {
    let (_, viewport) = sheet_size(grid);
    rules::bar_knob(grid.scroll, grid.layout.height, viewport).map(|(top, length)| (rules::SENTENCE + top, length))
}

/// Whether a point in the window is on the scroll bar's edge.
fn on_bar(grid: &Grid, (x, y): (f64, f64)) -> bool {
    let (w, viewport) = sheet_size(grid);
    knob(grid).is_some() && grid.flight.is_none() && x >= w - rules::BAR_REACH && y >= rules::SENTENCE && y < rules::SENTENCE + viewport
}

/// The overlay scroll bar (macOS's overlay scroller): a rounded knob in
/// the theme's paper at half strength, `opacity` of that, at the right
/// edge `right` of a track `viewport` long from `top`. Shared with the
/// grid bench.
pub fn paint_scroll_bar(p: &mut Painter, t: &Tokens, scroll: f64, content: f64, viewport: f64, right: f32, top: f32, opacity: f32, wide: bool) {
    if opacity <= 0.0 {
        return;
    }
    let Some((y, length)) = rules::bar_knob(scroll, content, viewport) else { return };
    let width = if wide { rules::BAR_WIDE } else { rules::BAR_WIDTH } as f32;
    let r = RectF::new(right - rules::BAR_INSET as f32 - width, top + y as f32, width, length as f32);
    p.fill_round_rect(r, width / 2.0, Ink::hex(t.hex.paper).alpha(0.5 * opacity));
}

/// Scrolls `index` into view, with room for its box.
fn reveal(app: &mut App, index: usize) {
    let Some(grid) = app.grid.as_mut() else { return };
    let (_, viewport) = sheet_size(grid);
    grid.scroll = grid.layout.reveal(index, grid.scroll, viewport);
    clamp(grid);
    invalidate(grid.hwnd);
    request(app);
}

fn filter_menu(app: &mut App, hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        super::library::add_filter_items(menu);
        super::library::update_menu(app, menu);
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD, cursor.x, cursor.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        if command.0 != 0 {
            let _ = PostMessageW(app.main, WM_COMMAND, WPARAM(command.0 as usize), LPARAM(0));
        }
    }
}

/// The grid's keys, before the window's own. G anywhere in the edit view.
pub fn key(app: &mut App, msg: &MSG, typing: bool) -> bool {
    if msg.message != WM_KEYDOWN || typing {
        return false;
    }
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) < 0 };
    let key = VIRTUAL_KEY(msg.wParam.0 as u16);
    if key == VIRTUAL_KEY(b'G' as u16) && !down(VK_CONTROL) && !down(VK_MENU) {
        toggle(app);
        return true;
    }
    if !shown(app) {
        return false;
    }
    let ctrl = down(VK_CONTROL);
    let shift = down(VK_SHIFT);
    let order = app.model.shown_items();
    let current = app.model.shelf.selection.current().and_then(|c| order.iter().position(|i| i == c)).or_else(|| app.grid.as_ref()?.cells.iter().position(|c| c.open));
    let layout_step = |by: isize| -> Option<usize> { app.grid.as_ref()?.layout.vertical(current?, by) };
    let target = match key {
        VK_ESCAPE | VK_RETURN => {
            close(app, key == VK_RETURN);
            return true;
        }
        k if k == VIRTUAL_KEY(b'E' as u16) && !ctrl => {
            close(app, true);
            return true;
        }
        k if k == VIRTUAL_KEY(b'A' as u16) && ctrl => {
            app.model.shelf.selection.select_all(&order);
            app.model.dirty |= dirty::STRIP | dirty::CHROME;
            return true;
        }
        VK_OEM_PLUS | VK_ADD if ctrl => {
            set_size(app, app.model.prefs.grid_size + rules::GRID_SIZE_STEP);
            return true;
        }
        VK_OEM_MINUS | VK_SUBTRACT if ctrl => {
            set_size(app, app.model.prefs.grid_size - rules::GRID_SIZE_STEP);
            return true;
        }
        VK_LEFT => current.map(|c| c.saturating_sub(1)),
        VK_RIGHT => current.map(|c| (c + 1).min(order.len().saturating_sub(1))),
        VK_UP => layout_step(-1),
        VK_DOWN => layout_step(1),
        VK_HOME => Some(0),
        VK_END => order.len().checked_sub(1),
        VK_PRIOR | VK_NEXT => {
            let Some(grid) = &app.grid else { return true };
            let (_, viewport) = sheet_size(grid);
            scroll_by(app, if key == VK_PRIOR { -viewport } else { viewport });
            return true;
        }
        _ => return false,
    };
    if let Some(index) = target.filter(|&i| i < order.len()) {
        let item = &order[index];
        if shift {
            app.model.shelf.selection.extend_to(item, &order);
        } else {
            app.model.shelf.selection.select(item);
        }
        app.model.dirty |= dirty::STRIP | dirty::CHROME;
        reveal(app, index);
    }
    true
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                if with_app(|app| paint(app, hwnd)).is_none() {
                    let mut ps = PAINTSTRUCT::default();
                    BeginPaint(hwnd, &mut ps);
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_FLIGHT_FRAME => {
                // What the fold uncovers paints first: frames posted back to
                // back would keep the editor's WM_PAINT waiting.
                let t0 = std::time::Instant::now();
                let _ = RedrawWindow(model_main(hwnd), None, None, RDW_UPDATENOW | RDW_ALLCHILDREN);
                crate::trace::event("grid_flight_redraw", || serde_json::json!({"ms": t0.elapsed().as_secs_f64() * 1000.0}));
                with_app(tick);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_SCROLL => {
                with_app(scroll_tick);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|app| {
                    let Some(grid) = app.grid.as_mut() else { return };
                    if grid.bar_hover && grid.bar_drag.is_none() {
                        grid.bar_hover = false;
                        // It fades a second after the pointer leaves.
                        scrolled(grid);
                        invalidate(grid.hwnd);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                SetCapture(hwnd);
                with_app(|app| {
                    let Some(grid) = app.grid.as_mut() else { return };
                    if grid.flight.is_some() {
                        return;
                    }
                    let at = point(hwnd, lparam);
                    if on_bar(grid, at) {
                        // The knob drags; above or below it pages.
                        let Some((top, length)) = knob(grid) else { return };
                        if at.1 >= top && at.1 < top + length {
                            grid.bar_drag = Some(at.1 - top);
                            grid.glide = None;
                            return invalidate(hwnd);
                        }
                        let (_, viewport) = sheet_size(grid);
                        return scroll_by(app, if at.1 < top { -viewport } else { viewport });
                    }
                    match act_at(grid, at) {
                        Some(Act::Filter) => return filter_menu(app, hwnd),
                        Some(Act::Clear) => return app.model.set_library_filter(negs_library::Filter::default()),
                        Some(Act::Size) => {
                            grid.sliding = true;
                            return slide(app, at.0);
                        }
                        Some(Act::BatchCancel) => {
                            let _ = ReleaseCapture();
                            return app.model.cancel_batch();
                        }
                        Some(Act::BatchShow) => {
                            let _ = ReleaseCapture();
                            let Some((r, _)) = grid.places.iter().find(|(_, a)| *a == Act::BatchShow).copied() else { return };
                            let s = scale(hwnd) as f32;
                            let mut corner = POINT { x: ((r.x + 4.0) * s) as i32, y: (r.y * s) as i32 };
                            let _ = ClientToScreen(hwnd, &mut corner);
                            let anchor = RECT { left: corner.x, top: corner.y, right: corner.x + (r.w * s) as i32, bottom: corner.y + (r.h * s) as i32 };
                            return super::batch::toggle(app, anchor);
                        }
                        None => {}
                    }
                    match cell_at(grid, at) {
                        Some(index) => {
                            let item = grid.cells[index].want.item.clone();
                            strip::click(app, &item, false);
                        }
                        None => {
                            // A band from the gaps: Ctrl adds to what's selected.
                            let sheet = (at.0, at.1 - rules::SENTENCE + grid.scroll);
                            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                            let base = if ctrl { app.model.shelf.selection.selected().clone() } else { Default::default() };
                            grid.dragging = Some((sheet, sheet, base));
                            if !ctrl {
                                app.model.shelf.selection.deselect_all();
                                app.model.dirty |= dirty::STRIP | dirty::CHROME;
                            }
                        }
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                with_app(|app| {
                    let Some(grid) = app.grid.as_mut() else { return };
                    let at = point(hwnd, lparam);
                    if grid.sliding {
                        return slide(app, at.0);
                    }
                    if let Some(grab) = grid.bar_drag {
                        let (_, viewport) = sheet_size(grid);
                        grid.scroll = rules::bar_scroll(at.1 - rules::SENTENCE - grab, grid.layout.height, viewport);
                        clamp(grid);
                        scrolled(grid);
                        invalidate(hwnd);
                        return request(app);
                    }
                    let hover = grid.dragging.is_none() && on_bar(grid, at);
                    if hover != grid.bar_hover {
                        grid.bar_hover = hover;
                        if hover {
                            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                            let _ = TrackMouseEvent(&mut track);
                        }
                        scrolled(grid);
                        invalidate(hwnd);
                    }
                    let Some((start, _, base)) = grid.dragging.clone() else { return };
                    let now = (at.0, at.1 - rules::SENTENCE + grid.scroll);
                    grid.dragging = Some((start, now, base.clone()));
                    let band = Rect::spanning(start, now);
                    let items: Vec<Item> = grid.layout.indices_in(&band).into_iter().map(|i| grid.cells[i].want.item.clone()).collect();
                    app.model.shelf.selection.band(&items, &base);
                    app.model.dirty |= dirty::STRIP | dirty::CHROME;
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                with_app(|app| {
                    if let Some(grid) = app.grid.as_mut() {
                        grid.dragging = None;
                        grid.sliding = false;
                        if grid.bar_drag.take().is_some() {
                            let at = point(hwnd, lparam);
                            grid.bar_hover = on_bar(grid, at);
                            scrolled(grid);
                            invalidate(hwnd);
                        }
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                with_app(|app| {
                    let Some(grid) = app.grid.as_ref() else { return };
                    if let Some(index) = cell_at(grid, point(hwnd, lparam)) {
                        let item = grid.cells[index].want.item.clone();
                        app.model.shelf.selection.select(&item);
                        close(app, true);
                    }
                });
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_app(|app| {
                    let Some(grid) = app.grid.as_ref() else { return };
                    let Some(index) = cell_at(grid, point(hwnd, lparam)) else { return };
                    let item = grid.cells[index].want.item.clone();
                    let mut p = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                    let _ = ClientToScreen(hwnd, &mut p);
                    strip::context_menu(app, hwnd, &item, p);
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f64;
                with_app(|app| {
                    if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                        set_size(app, app.model.prefs.grid_size + rules::GRID_SIZE_STEP * delta / 120.0);
                    } else {
                        wheel(app, delta);
                    }
                });
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_DESTROY => {
                with_app(|app| {
                    if let Some(g) = app.grid.as_mut() {
                        g.surface.release();
                    }
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
