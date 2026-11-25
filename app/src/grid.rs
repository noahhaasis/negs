//! The grid (`LibraryGrid.swift`, `windows/src/ui/grid.rs`): the folder's
//! photographs as a contact sheet over the window under the title bar,
//! packed edge to edge in rows at their own proportions
//! (`sheet_rules::SheetLayout`), the sentence over it ("Showing all ⌄ 12
//! photographs", the filter's words to change it, × to clear it) and the
//! foot under it (the count, the Size slider). G shows and hides it; E,
//! Enter, Esc and a double click go back to the photograph; click,
//! ⌘/Ctrl, Shift, a band and the arrows select; ⌘/Ctrl plus and minus and
//! ⌘/Ctrl with the wheel change the size, keeping the photograph a third
//! of the way down where it was (`keep_anchor`). A wheel's notch glides and
//! eases off; the overlay scroll bar at the right shows while it scrolls.
//!
//! **SheetFlight** on both platforms (the Mac's `SheetFlight.swift`): the
//! sheet grows out of the strip's band to the window and each thumbnail on
//! screen in both flies from its frame in the strip to its cell, 480 ms on
//! `cubic-bezier(.22,.8,.24,1)`, those nearer the current photograph
//! first; the others fade up after. Folding plays it backwards and fades
//! the sheet off the strip at the end. Each flyer is one sprite in its own
//! element (GPUI's opacity is per element), the sheet and the flyers
//! clipped to the growing mask. Reduce motion shows and hides it at once.
//!
//! Virtualised as the strip: only the rows in view (plus `GRID_MARGIN`)
//! are painted, all in one `canvas`, the marks as cached sprites.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, Font, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, PathBuilder, Pixels,
    ScrollDelta, ScrollWheelEvent, SharedString, Styled, TextRun, Window, canvas, div, fill, font, point, px, size,
};
use negs_app::model::dirty;
use negs_app::sheet_rules::{self as rules, Part, Rect, SheetLayout, ThumbSize};
use negs_library::{Filter, Flag, Item, Selection};

use crate::NegsApp;
use crate::marks_gpui::{BoxInk, paint_pencil};
use crate::strip::{self, Cell, FramePaint, Inks};
use crate::thumbs::{Thumb, Want};
use crate::tile_cache::cover_bounds;
use crate::titlebar::TITLEBAR_HEIGHT;

/// The title bar above the grid (DIPs).
const TOP: f64 = TITLEBAR_HEIGHT as f64;
/// The flight (`SheetFlight.swift`).
pub const DURATION: f64 = 480.0;
const STAGGER: f64 = 16.0;
const STAGGER_STEPS: usize = 12;
const BOXES_FROM: f64 = 0.35;
const FOLD_FADE: f64 = 180.0;
const MASK_REACH: f64 = 0.25;
/// The Size slider's track at the foot's right.
const SLIDER: f64 = 72.0;

fn ease(p: f64) -> f64 {
    rules::cubic_bezier(0.22, 0.8, 0.24, 1.0, p)
}

/// SwiftUI's `.easeOut`.
fn ease_out(p: f64) -> f64 {
    rules::cubic_bezier(0.0, 0.0, 0.58, 1.0, p)
}

/// `NEGS_FLIGHT_SLOW=<factor>` slows the flight down for frame captures.
fn slow() -> f64 {
    static SLOW: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SLOW.get_or_init(|| std::env::var("NEGS_FLIGHT_SLOW").ok().and_then(|s| s.parse().ok()).filter(|f: &f64| *f >= 1.0).unwrap_or(1.0))
}

fn lerp(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

fn lerp_rect(a: &Rect, b: &Rect, k: f64) -> Rect {
    Rect::new(lerp(a.x, b.x, k), lerp(a.y, b.y, k), lerp(a.w, b.w, k), lerp(a.h, b.h, k))
}

fn bounds(r: &Rect) -> Bounds<Pixels> {
    Bounds { origin: point(px(r.x as f32), px(r.y as f32)), size: size(px(r.w as f32), px(r.h as f32)) }
}

/// A thumbnail travelling between the strip and the grid (window DIPs).
pub struct Flyer {
    pub index: usize,
    pub thumb: Arc<Thumb>,
    pub from: Rect,
    pub to: Rect,
    pub opacity: (f32, f32),
    pub delay: f64,
}

/// The grid opening out of the strip, or folding back into it.
pub struct Flight {
    pub opening: bool,
    started: Instant,
    /// The strip's band and the window past its top, in window DIPs.
    band: Rect,
    reach: Rect,
    pub flyers: Vec<Flyer>,
    /// The open photograph, between the canvas and its cell, over the
    /// mask (it starts or ends outside it).
    pub canvas_flyer: Option<Flyer>,
    /// The cells on screen as it started: carried by a flyer, and their delay.
    cells: HashMap<usize, (bool, f64)>,
    /// When it's over (ms).
    end: f64,
    /// Held still at this many ms (the bench's mid-flight captures).
    frozen: Option<f64>,
}

impl Flight {
    /// Milliseconds in.
    pub fn elapsed(&self) -> f64 {
        self.frozen.unwrap_or_else(|| self.started.elapsed().as_secs_f64() * 1000.0 / slow())
    }

    /// Holds it at `at` ms, or lets it go on from where it's held.
    pub fn freeze(&mut self, at: Option<f64>) {
        if at.is_none()
            && let Some(held) = self.frozen
        {
            self.started = Instant::now() - Duration::from_secs_f64(held * slow() / 1000.0);
        }
        self.frozen = at;
    }

    pub fn done(&self, e: f64) -> bool {
        e >= self.end
    }

    /// The sheet's mask: growing from the band to the window, or back.
    pub fn mask(&self, e: f64) -> Rect {
        let k = ease((e / DURATION).clamp(0.0, 1.0));
        if self.opening { lerp_rect(&self.band, &self.reach, k) } else { lerp_rect(&self.reach, &self.band, k) }
    }

    /// The whole sheet's opacity: a fold fades it off the strip at the end.
    pub fn sheet_alpha(&self, e: f64) -> f32 {
        if self.opening || e < DURATION - FOLD_FADE {
            return 1.0;
        }
        (1.0 - (e - (DURATION - FOLD_FADE)) / FOLD_FADE).clamp(0.0, 1.0) as f32
    }

    /// A cell's picture and boxes, as shares of how they show: a carried
    /// one waits under its flyer, its boxes fading in as it lands; the rest
    /// fade up once the sheet is mostly open (or out, folding).
    pub fn cell_alpha(&self, index: usize, e: f64) -> (f32, f32) {
        let Some(&(carried, delay)) = self.cells.get(&index) else { return (1.0, 1.0) };
        let clamp = |v: f64| v.clamp(0.0, 1.0);
        if self.opening {
            if carried {
                let picture = if e >= delay + DURATION { 1.0 } else { 0.0 };
                (picture, ease_out(clamp((e - delay - DURATION * BOXES_FROM) / (DURATION * (1.0 - BOXES_FROM)))) as f32)
            } else {
                let a = ease_out(clamp((e - DURATION * 0.35 - delay) / (DURATION * 0.55))) as f32;
                (a, a)
            }
        } else {
            let out = 1.0 - ease_out(clamp(e / (DURATION * 0.4))) as f32;
            if carried { (0.0, out) } else { (out, out) }
        }
    }

    /// A flyer's rect and opacity now; `None` once it's landed (opening).
    pub fn flyer_at(&self, f: &Flyer, e: f64) -> Option<(Rect, f32)> {
        if self.opening && e >= f.delay + DURATION {
            return None;
        }
        let k = ease(((e - f.delay) / DURATION).clamp(0.0, 1.0));
        let r = lerp_rect(&f.from, &f.to, k);
        let mut opacity = lerp(f.opacity.0 as f64, f.opacity.1 as f64, k) as f32;
        if !self.opening && e >= DURATION - FOLD_FADE {
            // Fading into its frame as the sheet fades off it: over the
            // frame (at `op`) under the sheet (at 1 - u) it shows as the
            // frame alone does, neither brighter nor dimmer.
            let op = f.opacity.1 as f64;
            let u = ((e - (DURATION - FOLD_FADE)) / FOLD_FADE).clamp(0.0, 1.0);
            opacity = if op >= 0.999 { 1.0 } else { (op * (1.0 - u) / (1.0 - op * u)) as f32 };
        }
        Some((r, opacity))
    }

    pub fn carries(&self, index: usize) -> bool {
        self.cells.get(&index).is_some_and(|c| c.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Filter,
    Clear,
    Size,
}

#[derive(Default)]
pub struct GridState {
    /// On screen: open, or opening or folding.
    pub shown: bool,
    pub cells: Vec<Cell>,
    pub layout: SheetLayout,
    pub scroll: f64,
    /// The window's size (DIPs) and scale, as last rendered.
    window: (f64, f64),
    scale: f64,
    /// A band selection: where it started and is now (sheet DIPs), and the
    /// selection under it.
    dragging: Option<((f64, f64), (f64, f64), HashSet<Item>)>,
    sliding: bool,
    /// The sentence's words that act, as painted (in the grid's own DIPs).
    places: Rc<RefCell<Vec<(Rect, Act)>>>,
    /// Where a wheel's glide is going, and when it last moved.
    glide: Option<f64>,
    ticked: Option<Instant>,
    /// When the grid last scrolled: the scroll bar fades a second after.
    scrolled_at: Option<Instant>,
    bar_hover: bool,
    /// Dragging the knob: where in it the pointer took it.
    bar_drag: Option<f64>,
    /// The photograph open when the grid opened.
    opened_from: Option<Item>,
    /// A wake-up asked for (the bar's fade).
    wake: Option<Instant>,
    pub flight: Option<Flight>,
    synced: u64,
}

impl GridState {
    fn width(&self) -> f64 {
        self.window.0
    }

    pub fn viewport(&self) -> f64 {
        (self.window.1 - TOP - rules::SENTENCE - rules::FOOT).max(0.0)
    }

    /// A cell's rect in the window (DIPs).
    pub fn cell_in_window(&self, index: usize) -> Rect {
        let f = self.layout.frames[index];
        Rect::new(f.x, TOP + rules::SENTENCE + f.y - self.scroll, f.w, f.h)
    }

    fn viewport_rect(&self) -> Rect {
        Rect::new(0.0, TOP + rules::SENTENCE, self.width(), self.viewport())
    }

    fn clamp(&mut self) {
        self.scroll = self.scroll.max(0.0).min((self.layout.height - self.viewport()).max(0.0));
    }

    pub fn on_screen(&self) -> std::ops::Range<usize> {
        self.layout.indices(self.scroll, self.scroll + self.viewport())
    }

    /// Open and still: the photograph view under it needn't be drawn.
    pub fn covers(&self) -> bool {
        self.shown && self.flight.is_none()
    }

    pub fn flying(&self) -> bool {
        self.flight.is_some()
    }

    /// Scrolls to `y` at once (the bench's sweeps).
    pub fn set_scroll(&mut self, y: f64) {
        self.glide = None;
        self.scroll = y;
        self.clamp();
    }

    fn cell_at(&self, x: f64, y: f64) -> Option<usize> {
        let y = y - TOP;
        (y >= rules::SENTENCE && y < rules::SENTENCE + self.viewport()).then(|| self.layout.hit(x, y - rules::SENTENCE + self.scroll)).flatten()
    }

    fn act_at(&self, x: f64, y: f64) -> Option<Act> {
        let y = y - TOP;
        self.places.borrow().iter().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    fn knob(&self) -> Option<(f64, f64)> {
        rules::bar_knob(self.scroll, self.layout.height, self.viewport()).map(|(top, length)| (TOP + rules::SENTENCE + top, length))
    }

    fn on_bar(&self, x: f64, y: f64) -> bool {
        let top = TOP + rules::SENTENCE;
        self.knob().is_some() && self.flight.is_none() && x >= self.width() - rules::BAR_REACH && y >= top && y < top + self.viewport()
    }

    fn bar_look(&self) -> (f32, bool) {
        if self.flight.is_some() {
            return (0.0, false);
        }
        if self.bar_hover || self.bar_drag.is_some() {
            return (1.0, true);
        }
        let since = self.scrolled_at.map_or(f64::INFINITY, |t| t.elapsed().as_secs_f64() * 1000.0);
        (rules::bar_opacity(since) as f32, false)
    }
}

/// Follows the model: the photographs, their marks and shapes, the
/// selection; packs the rows anew keeping the photograph in view.
pub fn sync(app: &mut NegsApp) {
    if !app.grid.shown {
        return;
    }
    if app.model.shelf.library.is_none() {
        return finish(app, false);
    }
    app.grid.cells = strip::cells(&app.model, &[]);
    relayout(app);
    request(app);
    app.grid.synced = app.thumbs.revision;
}

/// Packs the rows anew; true if they moved. The photograph a third of the
/// way down the view keeps its place (`keep_anchor`).
fn relayout(app: &mut NegsApp) -> bool {
    let size = app.model.prefs.grid_size;
    let ratios: Vec<f64> = app.grid.cells.iter().map(|c| app.thumbs.ratio(&c.want).unwrap_or(rules::FALLBACK_RATIO)).collect();
    let grid = &mut app.grid;
    let (width, viewport) = (grid.width(), grid.viewport());
    if width <= 0.0 {
        return false;
    }
    let layout = SheetLayout::new(&ratios, width, size);
    if layout == grid.layout {
        return false;
    }
    let (ax, ay) = (width / 2.0, grid.scroll + viewport / 3.0);
    let anchor = grid.layout.hit(ax, ay).or_else(|| grid.layout.nearest(ax, ay));
    if let (Some(index), true) = (anchor, grid.layout.frames.len() == layout.frames.len()) {
        let f = grid.layout.frames[index];
        let down = ((ay - f.y) / f.h.max(1.0)).clamp(0.0, 1.0);
        grid.scroll = rules::keep_anchor(&layout, index, down, f.y + f.h * down - grid.scroll, viewport);
    }
    grid.layout = layout;
    grid.clamp();
    true
}

/// Asks for the thumbnails in view now (the bench's scrolling).
pub fn request_now(app: &mut NegsApp) {
    request(app);
}

/// The rows in view, then ±32 around them; the pump develops what's missing.
fn request(app: &mut NegsApp) {
    let grid = &app.grid;
    if grid.layout.frames.len() != grid.cells.len() {
        return;
    }
    let range = grid.on_screen();
    let shown: Vec<usize> = range.clone().collect();
    let open = grid.cells.iter().position(|c| c.open);
    let ahead = rules::prefetch_order(grid.cells.len(), &shown, open);
    let wants: Vec<&Want> = shown.iter().chain(&ahead).map(|&i| &grid.cells[i].want).collect();
    let size = ThumbSize::for_rows(app.model.prefs.grid_size, grid.scale.max(1.0));
    app.thumbs.read(&wants, size);
    let all: Vec<Want> = grid.cells.iter().map(|c| c.want.clone()).collect();
    app.thumbs.pump(&app.model, &all, range, open);
}

// MARK: Showing and hiding

/// G: the grid, opening out of the strip.
pub fn open(app: &mut NegsApp) {
    let returning = app.grid.flight.as_ref().is_some_and(|f| !f.opening);
    if (app.grid.shown && !returning) || app.model.shelf.library.is_none() || app.model.cropping() || app.model.source.is_none() {
        return;
    }
    app.close_menu();
    // The grid shows the open photograph as edited.
    app.model.file_open_thumbnail();
    // The grid keeps its current photograph in view, filtered out or not.
    if app.model.shelf.selection.current().is_none()
        && let Some(open) = app.model.library_item()
    {
        app.model.shelf.selection.select(&open);
    }
    app.model.shelf.in_grid = true;
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
    // The editor closed: its film side won't be used soon (ticket 41).
    app.model.release_film_cache(negs_app::film_cache_release::Reason::EditorClosed);
    app.thumbs.held = true;
    let opened_from = app.model.library_item();
    let grid = &mut app.grid;
    grid.flight = None;
    grid.shown = true;
    grid.opened_from = opened_from;
    if !returning {
        grid.cells.clear();
        grid.layout = SheetLayout::default();
    }
    sync(app);
    let grid = &mut app.grid;
    if !returning && let Some(row) = grid.cells.iter().position(|c| c.open).and_then(|i| grid.layout.row_of(i)) {
        grid.scroll = grid.layout.centre_row(row, grid.viewport());
        grid.clamp();
    }
    start_flight(app, true);
}

/// E, Enter, Esc, a double click: back to the photograph, folding into the
/// strip. `open_anchor`: the selection's current photograph opens first.
pub fn close(app: &mut NegsApp, open_anchor: bool) {
    if !app.grid.shown || app.grid.flight.as_ref().is_some_and(|f| !f.opening) {
        return;
    }
    app.close_menu();
    if open_anchor {
        let current = app.model.shelf.selection.current().cloned();
        if let Some(item) = current.filter(|i| app.model.library_item().as_ref() != Some(i)) {
            app.model.open_item(item);
        }
    }
    app.model.shelf.in_grid = false;
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
    // The strip on the photograph opened, laid out under the folding sheet.
    strip::settle(app);
    app.grid.flight = None;
    start_flight(app, false);
}

pub fn toggle(app: &mut NegsApp) {
    let folding = app.grid.flight.as_ref().is_some_and(|f| !f.opening);
    if app.grid.shown && !folding { close(app, true) } else { open(app) }
}

/// The strip's band in the window (DIPs): the strip and the status line
/// under the canvas, or the status line alone with the strip off.
fn band(app: &NegsApp) -> Rect {
    let (w, h) = app.grid.window;
    let status = rules::STATUS;
    let strip_on = strip::shown(app);
    let b = app.strip.bounds.get();
    if strip_on && f32::from(b.size.width) > 0.0 {
        Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, rules::STRIP + status)
    } else {
        let canvas_w = f32::from(app.canvas.last_canvas_bounds.size.width) as f64;
        Rect::new(0.0, h - status, if canvas_w > 0.0 { canvas_w } else { w }, status)
    }
}

fn start_flight(app: &mut NegsApp, opening: bool) {
    // The thumbnails' developing waits for the flight: it shares the GPU.
    app.thumbs.held = true;
    if crate::motion::reduced() || app.grid.window.0 <= 0.0 {
        return finish(app, opening);
    }
    let band = band(app);
    let (w, h) = app.grid.window;
    let reach = Rect::new(0.0, TOP - (h - TOP) * MASK_REACH, w, (h - TOP) * (1.0 + MASK_REACH));
    let grid = &app.grid;
    let viewport = grid.viewport_rect();
    let order = app.model.shown_items();
    let current = app.model.shelf.selection.current().and_then(|c| order.iter().position(|i| i == c)).or_else(|| grid.cells.iter().position(|c| c.open)).unwrap_or(0);
    // The strip's frames in its band, by photograph.
    let strip_index: HashMap<&Item, usize> = if strip::shown(app) {
        app.strip.cells.iter().enumerate().map(|(i, c)| (&c.want.item, i)).collect()
    } else {
        HashMap::new()
    };
    let grid_size = ThumbSize::for_rows(app.model.prefs.grid_size, grid.scale.max(1.0));
    // The open photograph flies between its place on the canvas and its cell.
    let photo = app.model.library_item();
    let photo_index = photo.as_ref().and_then(|p| grid.cells.iter().position(|c| &c.want.item == p));
    let canvas_rect = |b: Bounds<Pixels>| Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64);
    let on_canvas: Option<(Rect, Arc<Thumb>)> = photo_index.and_then(|i| {
        let want = &grid.cells[i].want;
        let thumb = app.thumbs.shown(want, grid_size).or_else(|| app.thumbs.shown(want, ThumbSize::Strip));
        let drawn = canvas_rect(app.canvas.last_photo_bounds);
        if opening {
            // The canvas's own picture, so nothing changes as it lifts off.
            let image = app.canvas.whole_image().filter(|_| app.canvas.fit && drawn.w > 1.0)?;
            let size = image.size(0);
            let t = Arc::new(Thumb::wrap(image, size.width.0 as u32, size.height.0 as u32));
            Some((drawn, t))
        } else {
            let thumb = thumb?;
            // Where the canvas will draw it: where it was, if it's the same
            // photograph, else fitted to the canvas as the canvas fits it.
            let same = grid.opened_from.as_ref() == photo.as_ref() && drawn.w > 1.0 && app.canvas.fit;
            let rect = if same { drawn } else { fit_rect(canvas_rect(app.canvas.last_canvas_bounds), Some(thumb.ratio()))? };
            Some((rect, thumb))
        }
    });
    let mut flyers = vec![];
    let mut canvas_flyer = None;
    let mut cells = HashMap::new();
    let mut end = DURATION;
    if grid.layout.frames.len() == grid.cells.len() {
        for i in grid.on_screen() {
            let to = grid.cell_in_window(i);
            if !to.intersects(&viewport) {
                continue;
            }
            let cell = &grid.cells[i];
            let delay = if opening { STAGGER * (i.abs_diff(current).min(STAGGER_STEPS)) as f64 } else { 0.0 };
            let from = strip_index.get(&cell.want.item).and_then(|&s| app.strip.frame_in_window(s)).map(|b| {
                Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64)
            });
            let from = from.filter(|r| r.intersects(&band) && r.x >= band.x - 1.0 && r.max_x() <= band.max_x() + r.w);
            let thumb = app.thumbs.shown(&cell.want, grid_size).or_else(|| app.thumbs.shown(&cell.want, ThumbSize::Strip));
            let rejected = cell.summary.flag == Some(Flag::Reject);
            let strip_op = rules::strip_opacity(cell.open, cell.selected, rejected, false, false) as f32;
            let grid_op = if rejected { rules::reject_opacity(false, cell.open) as f32 } else { 1.0 };
            if Some(i) == photo_index
                && let Some((rect, image)) = on_canvas.clone()
            {
                end = end.max(delay + DURATION);
                canvas_flyer = Some(if opening {
                    Flyer { index: i, thumb: image, from: rect, to, opacity: (1.0, grid_op), delay }
                } else {
                    Flyer { index: i, thumb: image, from: to, to: rect, opacity: (grid_op, 1.0), delay: 0.0 }
                });
                cells.insert(i, (true, delay));
                continue;
            }
            let carried = from.is_some() && thumb.is_some();
            if let (true, Some(from), Some(thumb)) = (carried, from, thumb) {
                end = end.max(delay + DURATION);
                flyers.push(if opening {
                    Flyer { index: i, thumb, from, to, opacity: (strip_op, grid_op), delay }
                } else {
                    Flyer { index: i, thumb, from: to, to: from, opacity: (grid_op, strip_op), delay: 0.0 }
                });
            }
            cells.insert(i, (carried, delay));
        }
    }
    if opening {
        end += 20.0;
    }
    app.grid.flight = Some(Flight { opening, started: Instant::now(), band, reach, flyers, canvas_flyer, cells, end, frozen: None });
}

/// A picture of shape `ratio` fitted inside `canvas` as the canvas fits a
/// photograph (inside its padding, never past 100 % is ignored: thumbnails
/// don't know the photograph's size).
fn fit_rect(canvas: Rect, ratio: Option<f64>) -> Option<Rect> {
    let ratio = ratio.filter(|r| *r > 0.0)?;
    let pad = 24.0;
    let (aw, ah) = ((canvas.w - 2.0 * pad).max(64.0), (canvas.h - 2.0 * pad).max(64.0));
    if canvas.w <= 1.0 {
        return None;
    }
    let (w, h) = if aw / ah > ratio { (ah * ratio, ah) } else { (aw, aw / ratio) };
    Some(Rect::new(canvas.x + (canvas.w - w) / 2.0, canvas.y + (canvas.h - h) / 2.0, w, h))
}

fn finish(app: &mut NegsApp, opening: bool) {
    app.grid.flight = None;
    if !opening {
        app.grid.shown = false;
        app.grid.dragging = None;
        app.grid.sliding = false;
        app.grid.bar_drag = None;
    }
    app.thumbs.held = false;
    if opening {
        request(app);
    }
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
}

// MARK: Scrolling and size

pub fn set_size(app: &mut NegsApp, size: f64) {
    let size = rules::clamp_size(size);
    if (size - app.model.prefs.grid_size).abs() < 0.5 {
        return;
    }
    app.model.prefs.grid_size = size;
    app.model.prefs.save();
    relayout(app);
    request(app);
}

fn slide(app: &mut NegsApp, x: f64) {
    let left = app.grid.width() - 18.0 - SLIDER;
    set_size(app, rules::size_at((x - left) / SLIDER));
}

pub fn scroll_by(app: &mut NegsApp, delta: f64) {
    let grid = &mut app.grid;
    grid.glide = None;
    grid.scroll += delta;
    grid.clamp();
    grid.scrolled_at = Some(Instant::now());
    request(app);
}

/// A wheel's notch glides there, adding to a glide under way; a touchpad's
/// fine steps (already coasting) go at once, as everything does with
/// motion reduced.
fn wheel(app: &mut NegsApp, step: f64, notch: bool) {
    if !notch || crate::motion::reduced() {
        return scroll_by(app, step);
    }
    let grid = &mut app.grid;
    let end = (grid.layout.height - grid.viewport()).max(0.0);
    let to = (grid.glide.unwrap_or(grid.scroll) + step).clamp(0.0, end);
    if grid.glide.is_none() {
        grid.ticked = Some(Instant::now());
    }
    grid.glide = Some(to);
    grid.scrolled_at = Some(Instant::now());
}

/// Scrolls `index` into view, with room for its box.
fn reveal(app: &mut NegsApp, index: usize) {
    let grid = &mut app.grid;
    grid.glide = None;
    grid.scroll = grid.layout.reveal(index, grid.scroll, grid.viewport());
    grid.clamp();
    request(app);
}

/// The flight's end, the glide's next step and the bar's fade: asks for the
/// next frame while any of them moves, or a wake-up for the bar's fade.
fn step(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) {
    if let Some(f) = &app.grid.flight {
        let (e, opening) = (f.elapsed(), f.opening);
        if f.done(e) {
            finish(app, opening);
            // The photograph view under it shows again.
            window.refresh();
        } else {
            window.request_animation_frame();
        }
    }
    if !app.grid.shown {
        return;
    }
    let now = Instant::now();
    if let Some(to) = app.grid.glide {
        let grid = &mut app.grid;
        let dt = grid.ticked.map_or(16.0, |t| (now - t).as_secs_f64() * 1000.0);
        grid.ticked = Some(now);
        let before = grid.scroll;
        grid.scroll = rules::wheel_glide(grid.scroll, to, dt);
        grid.clamp();
        let moved = grid.scroll != before;
        if grid.scroll == to || !moved {
            grid.glide = None;
        }
        grid.scrolled_at = Some(now);
        if moved {
            request(app);
        }
        window.request_animation_frame();
    }
    let grid = &mut app.grid;
    let held = grid.bar_hover || grid.bar_drag.is_some();
    if let Some(at) = grid.scrolled_at.filter(|_| !held) {
        let since = (now - at).as_secs_f64() * 1000.0;
        if since < rules::BAR_SHOWN_MS {
            let wake = at + Duration::from_millis(rules::BAR_SHOWN_MS as u64);
            if grid.wake != Some(wake) {
                grid.wake = Some(wake);
                let wait = wake.saturating_duration_since(now);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        } else if since < rules::BAR_SHOWN_MS + rules::BAR_FADE_MS + 50.0 {
            window.request_animation_frame();
        }
    }
}

// MARK: Keys

/// The grid's keys while it's open (and still): Esc, Enter, E back to the
/// photograph; the arrows, Home, End and Shift select; Page Up and Down;
/// ⌘/Ctrl+A everything. True if the key was the grid's.
pub fn key_down(app: &mut NegsApp, event: &KeyDownEvent) -> bool {
    if !app.grid.covers() {
        return false;
    }
    let keystroke = &event.keystroke;
    let m = &keystroke.modifiers;
    let command = if cfg!(target_os = "macos") { m.platform } else { m.control };
    let order = app.model.shown_items();
    let current = app.model.shelf.selection.current().and_then(|c| order.iter().position(|i| i == c)).or_else(|| app.grid.cells.iter().position(|c| c.open));
    let target = match keystroke.key.as_str() {
        "escape" | "enter" => {
            close(app, keystroke.key == "enter");
            return true;
        }
        "e" if !command && !m.alt && !m.control => {
            close(app, true);
            return true;
        }
        "a" if command => {
            app.model.shelf.selection.select_all(&order);
            app.model.dirty |= dirty::STRIP | dirty::CHROME;
            return true;
        }
        "pageup" | "pagedown" => {
            let page = app.grid.viewport();
            scroll_by(app, if keystroke.key == "pageup" { -page } else { page });
            return true;
        }
        key => match moved(key, current, order.len(), &app.grid.layout) {
            Some(target) => target,
            None => return false,
        },
    };
    if let Some(index) = target {
        move_selection(&mut app.model.shelf.selection, &order[index], &order, m.shift);
        app.model.dirty |= dirty::STRIP | dirty::CHROME;
        if index < app.grid.layout.frames.len() {
            reveal(app, index);
        }
    }
    true
}

/// Where an arrow, Home or End takes the current photograph among `count`
/// shown (`None`: not such a key; `Some(None)`: nowhere to go).
fn moved(key: &str, current: Option<usize>, count: usize, layout: &SheetLayout) -> Option<Option<usize>> {
    let target = match key {
        "left" => current.map(|c| c.saturating_sub(1)),
        "right" => current.map(|c| (c + 1).min(count.saturating_sub(1))),
        "up" => current.and_then(|c| layout.vertical(c, -1)),
        "down" => current.and_then(|c| layout.vertical(c, 1)),
        "home" => (count > 0).then_some(0),
        "end" => count.checked_sub(1),
        _ => return None,
    };
    Some(target.filter(|&i| i < count))
}

/// An arrow's move: the photograph alone, or with Shift the range to it.
fn move_selection(selection: &mut Selection, item: &Item, order: &[Item], shift: bool) {
    if shift {
        selection.extend_to(item, order);
    } else {
        selection.select(item);
    }
}

/// The rubber band from `start` to `now` (sheet space): what was selected
/// when it began, and every photograph it touches.
fn band_select(selection: &mut Selection, layout: &SheetLayout, cells: &[Cell], start: (f64, f64), now: (f64, f64), base: &HashSet<Item>) {
    let band = Rect::spanning(start, now);
    let items: Vec<Item> = layout.indices_in(&band).into_iter().filter_map(|i| cells.get(i)).map(|c| c.want.item.clone()).collect();
    selection.band(&items, base);
}

// MARK: Painting

#[derive(Clone)]
struct CellPaint {
    rect: Rect,
    frame: FramePaint,
    opacity: f32,
    /// A flight's shares of the picture and the boxes.
    alpha: (f32, f32),
}

struct SheetPaint {
    cells: Vec<CellPaint>,
    parts: Vec<Part>,
    foot: String,
    size: f64,
    bar: (f32, bool),
    scroll: f64,
    content: f64,
    viewport: f64,
    band: Option<Rect>,
    inks: Inks,
    ground: Hsla,
    dim: Hsla,
    label: Hsla,
    track: Hsla,
    paper: Hsla,
    accent: Hsla,
    serif: SharedString,
    ui: SharedString,
    places: Rc<RefCell<Vec<(Rect, Act)>>>,
}

fn shape(window: &mut Window, text: &str, family: &SharedString, italic: bool, size_: f32, color: Hsla) -> gpui::ShapedLine {
    let font = Font { style: if italic { FontStyle::Italic } else { FontStyle::Normal }, weight: FontWeight::NORMAL, ..font(family.clone()) };
    let run = TextRun { len: text.len(), font, color, background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_line(SharedString::from(text.to_string()), px(size_), &[run], None)
}

/// Paints `line` with its baseline at `baseline`.
fn paint_text(window: &mut Window, cx: &mut App, line: &gpui::ShapedLine, x: f32, baseline: f32) {
    let (ascent, descent) = (f32::from(line.ascent), f32::from(line.descent));
    let _ = line.paint(point(px(x), px(baseline - ascent)), px(ascent + descent), window, cx);
}

fn stroke_line(window: &mut Window, a: (f32, f32), b: (f32, f32), width: f32, color: Hsla) {
    let mut builder = PathBuilder::stroke(px(width));
    builder.move_to(point(px(a.0), px(a.1)));
    builder.line_to(point(px(b.0), px(b.1)));
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

impl SheetPaint {
    fn paint(self, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        window.paint_quad(fill(b, self.ground));
        let at = |r: &Rect| bounds(&Rect::new(r.x + ox as f64, r.y + oy as f64, r.w, r.h));
        let sheet = Bounds { origin: point(px(ox), px(oy + rules::SENTENCE as f32)), size: size(px(w), px(self.viewport as f32)) };
        window.with_content_mask(Some(ContentMask { bounds: sheet }), |window| {
            for c in &self.cells {
                if c.alpha.0 <= 0.0 {
                    continue;
                }
                let r = at(&c.rect);
                strip::paint_picture(window, c.frame.thumb.as_ref(), r, c.opacity, self.ground, self.inks.blank);
                strip::paint_marks(window, cx, &c.frame, r, &self.inks, &self.ui, false);
                if c.alpha.0 < 0.999 {
                    // Fading up (or out) over the sheet's ground.
                    let halo = Bounds { origin: point(r.origin.x - px(4.0), r.origin.y - px(4.0)), size: size(r.size.width + px(8.0), r.size.height + px(8.0)) };
                    window.paint_quad(fill(halo, self.ground.opacity(1.0 - c.alpha.0)));
                }
            }
            // The boxes reach past their pictures, over the gaps and the
            // thumbnails beside them: all the pick boxes, then the selection's.
            for c in &self.cells {
                if c.frame.flag == Some(Flag::Pick) {
                    strip::paint_frame_box(window, &c.frame, at(&c.rect), BoxInk::Grid, self.inks.paper, c.alpha.1);
                }
            }
            for c in &self.cells {
                if c.frame.selected {
                    strip::paint_frame_box(window, &c.frame, at(&c.rect), BoxInk::Selection, self.inks.selection, c.alpha.1);
                }
            }
            if let Some(band) = &self.band {
                let r = at(&Rect::new(band.x, band.y + rules::SENTENCE - self.scroll, band.w, band.h));
                window.paint_quad(fill(r, self.paper.opacity(0.08)).border_widths(px(1.0)).border_color(self.paper.opacity(0.45)));
            }
        });
        // The overlay scroll bar.
        let (opacity, wide) = self.bar;
        if opacity > 0.0
            && let Some((y, length)) = rules::bar_knob(self.scroll, self.content, self.viewport)
        {
            let width = if wide { rules::BAR_WIDE } else { rules::BAR_WIDTH } as f32;
            let r = Bounds {
                origin: point(px(ox + w - rules::BAR_INSET as f32 - width), px(oy + rules::SENTENCE as f32 + y as f32)),
                size: size(px(width), px(length as f32)),
            };
            window.paint_quad(fill(r, self.paper.opacity(0.5 * opacity)).corner_radii(px(width / 2.0)));
        }
        let mut places = vec![];
        // The sentence: dim italic words, the filter's words in the label's
        // ink with the pencil under them while filtered, the × to clear.
        let baseline = oy + 30.0;
        let mut x = ox + 18.0;
        for part in &self.parts {
            match part {
                Part::Words(words) => {
                    let line = shape(window, words, &self.serif, true, 17.0, self.dim);
                    paint_text(window, cx, &line, x, baseline);
                    x += f32::from(line.width);
                }
                Part::Filter { word, on } => {
                    let line = shape(window, word, &self.serif, true, 17.0, self.label);
                    let tw = f32::from(line.width);
                    paint_text(window, cx, &line, x, baseline);
                    if *on {
                        let r = Bounds { origin: point(px(x), px(baseline + 1.0)), size: size(px(tw), px(5.0)) };
                        paint_pencil(window, r, self.inks.plain, negs_marks::PENCIL_WIDTH as f32 * 0.7, self.accent);
                    } else {
                        window.paint_quad(fill(Bounds { origin: point(px(x), px(baseline + 2.0)), size: size(px(tw), px(1.0)) }, self.dim));
                    }
                    // The chevron, 7 × 3.5, drawn: Newsreader has no ⌄.
                    let (cx_, cy) = (x + tw + 8.0, baseline - 4.0);
                    stroke_line(window, (cx_ - 3.5, cy - 1.75), (cx_, cy + 1.75), 1.2, self.dim);
                    stroke_line(window, (cx_, cy + 1.75), (cx_ + 3.5, cy - 1.75), 1.2, self.dim);
                    places.push((Rect::new((x - ox) as f64 - 4.0, 0.0, tw as f64 + 20.0, rules::SENTENCE), Act::Filter));
                    x += tw + 14.0;
                }
                Part::Clear => {
                    // In the UI face: the subsetted serif has no ×.
                    let line = shape(window, "  ×", &self.ui, false, 16.0, self.dim);
                    paint_text(window, cx, &line, x, baseline);
                    places.push((Rect::new((x - ox) as f64, 0.0, f32::from(line.width) as f64 + 6.0, rules::SENTENCE), Act::Clear));
                    x += f32::from(line.width);
                }
            }
        }
        // The foot: the count at the left, Size and its slider at the right.
        let foot_h = rules::FOOT as f32;
        let foot_y = oy + h - foot_h;
        let foot_base = foot_y + (foot_h / 2.0 + 12.0 * 0.36).round();
        let count = shape(window, &self.foot, &self.ui, false, 12.0, self.dim);
        paint_text(window, cx, &count, ox + 14.0, foot_base);
        let track = Rect::new((w - 18.0) as f64 - SLIDER, (foot_y - oy + foot_h / 2.0) as f64, SLIDER, 1.0);
        window.paint_quad(fill(at(&track), self.track));
        let knob = (ox as f64 + track.x + track.w * rules::size_fraction(self.size)) as f32;
        let ky = oy + track.y as f32;
        for (r, color) in [(5.5, self.label), (4.0, self.ground)] {
            window.paint_quad(fill(Bounds { origin: point(px(knob - r), px(ky - r)), size: size(px(2.0 * r), px(2.0 * r)) }, color).corner_radii(px(r)));
        }
        let label = shape(window, "Size", &self.ui, false, 12.0, self.dim);
        paint_text(window, cx, &label, ox + track.x as f32 - 14.0 - f32::from(label.width), foot_base);
        places.push((Rect::new(track.x - 6.0, (foot_y - oy) as f64, track.w + 12.0, rules::FOOT), Act::Size));
        *self.places.borrow_mut() = places;
    }
}

pub fn render_grid(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if !app.grid.shown {
        return None;
    }
    let vp = window.viewport_size();
    let win = (f32::from(vp.width) as f64, f32::from(vp.height) as f64);
    let scale = window.scale_factor() as f64;
    if app.grid.window != win || app.grid.scale != scale {
        app.grid.window = win;
        app.grid.scale = scale;
        relayout(app);
        request(app);
    }
    step(app, window, cx);
    if !app.grid.shown {
        return None;
    }
    if app.grid.synced != app.thumbs.revision || app.model.dirty != 0 || app.grid.layout.frames.len() != app.grid.cells.len() {
        sync(app);
        if !app.grid.shown {
            return None;
        }
    }
    let inks = Inks::of(&app.theme);
    let theme = app.theme.clone();
    let model = &app.model;
    let filter = model.library_filter();
    let total = model.shelf.library.as_ref().map_or(0, |l| l.items().len());
    let selected = model.shelf.selection.selected().len();
    let NegsApp { grid, thumbs, sprites, .. } = app;
    let e = grid.flight.as_ref().map(|f| f.elapsed());
    let viewport = grid.viewport();
    let thumb_size = ThumbSize::for_rows(model.prefs.grid_size, scale.max(1.0));
    let range = if grid.layout.frames.len() == grid.cells.len() {
        grid.layout.indices(grid.scroll - rules::GRID_MARGIN, grid.scroll + viewport + rules::GRID_MARGIN)
    } else {
        0..0
    };
    let cells: Vec<CellPaint> = range
        .map(|i| {
            let f = grid.layout.frames[i];
            let cell = &grid.cells[i];
            let frame = FramePaint::of(cell, thumbs, thumb_size).with_sprites(sprites, f.w as f32, f.h as f32, scale as f32, &inks, true);
            let rejected = cell.summary.flag == Some(Flag::Reject);
            let alpha = match (&grid.flight, e) {
                (Some(flight), Some(e)) => flight.cell_alpha(i, e),
                _ => (1.0, 1.0),
            };
            CellPaint {
                rect: Rect::new(f.x, rules::SENTENCE + f.y - grid.scroll, f.w, f.h),
                frame,
                opacity: if rejected { rules::reject_opacity(false, false) as f32 } else { 1.0 },
                alpha,
            }
        })
        .collect();
    let paint = SheetPaint {
        cells,
        parts: rules::sentence(&filter, grid.cells.len(), total),
        foot: rules::foot(grid.cells.len(), total, selected, filter.active()),
        size: model.prefs.grid_size,
        bar: grid.bar_look(),
        scroll: grid.scroll,
        content: grid.layout.height,
        viewport,
        band: grid.dragging.as_ref().map(|(a, b, _)| Rect::spanning(*a, *b)),
        inks,
        ground: theme.grid,
        dim: theme.dim,
        label: theme.label,
        track: theme.track,
        paper: theme.paper,
        accent: theme.accent,
        serif: theme.serif_family.clone(),
        ui: theme.ui_family.clone(),
        places: grid.places.clone(),
    };

    let (w, h) = win;
    let oh = (h - TOP).max(0.0);
    let (mask, sheet_alpha) = match (&grid.flight, e) {
        (Some(f), Some(e)) => (f.mask(e), f.sheet_alpha(e)),
        _ => (Rect::new(0.0, TOP, w, oh), 1.0),
    };
    let (mx, my) = (mask.x, mask.y - TOP);
    let sheet = div()
        .absolute()
        .left(px(-mx as f32))
        .top(px(-my as f32))
        .w(px(w as f32))
        .h(px(oh as f32))
        .opacity(sheet_alpha)
        .child(canvas(|_, _, _| {}, move |b, _, window, cx| paint.paint(b, window, cx)).size_full());
    let mut masked = div().absolute().left(px(mx as f32)).top(px(my as f32)).w(px(mask.w as f32)).h(px(mask.h.max(0.0) as f32)).overflow_hidden().child(sheet);
    if let (Some(flight), Some(e)) = (&grid.flight, e) {
        for flyer in &flight.flyers {
            let Some((r, opacity)) = flight.flyer_at(flyer, e) else { continue };
            let thumb = flyer.thumb.clone();
            masked = masked.child(
                div()
                    .absolute()
                    .left(px((r.x - mask.x) as f32))
                    .top(px((r.y - mask.y) as f32))
                    .w(px(r.w as f32))
                    .h(px(r.h as f32))
                    .opacity(opacity)
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |b, _, window, _| {
                                window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
                                    let _ = window.paint_image(cover_bounds(b, thumb.width, thumb.height), Default::default(), thumb.image().clone(), 0, false);
                                });
                            },
                        )
                        .size_full(),
                    ),
            );
        }
    }

    let lifted = match (&grid.flight, e) {
        (Some(flight), Some(e)) => flight.canvas_flyer.as_ref().and_then(|f| flight.flyer_at(f, e).map(|(r, o)| (r, o, f.thumb.clone()))),
        _ => None,
    };
    let lifted = lifted.map(|(r, opacity, thumb)| {
        div()
            .absolute()
            .left(px(r.x as f32))
            .top(px((r.y - TOP) as f32))
            .w(px(r.w as f32))
            .h(px(r.h as f32))
            .opacity(opacity)
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
                            let _ = window.paint_image(cover_bounds(b, thumb.width, thumb.height), Default::default(), thumb.image().clone(), 0, false);
                        });
                    },
                )
                .size_full(),
            )
    });

    Some(
        div()
            .id("contact-sheet")
            .absolute()
            .left(px(0.0))
            .top(px(TOP as f32))
            .w(px(w as f32))
            .h(px(oh as f32))
            .overflow_hidden()
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(right_down))
            .on_mouse_move(cx.listener(mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(mouse_up))
            .on_scroll_wheel(cx.listener(scroll_wheel))
            .child(masked)
            .children(lifted)
            .into_any_element(),
    )
}

// MARK: Pointer

fn xy(p: gpui::Point<Pixels>) -> (f64, f64) {
    (f32::from(p.x) as f64, f32::from(p.y) as f64)
}

fn mouse_down(app: &mut NegsApp, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<NegsApp>) {
    app.close_menu();
    window.focus(&app.focus_handle.clone());
    if app.grid.flight.is_some() {
        return;
    }
    let (x, y) = xy(event.position);
    let grid = &mut app.grid;
    if grid.on_bar(x, y) {
        // The knob drags; above or below it pages.
        let Some((top, length)) = grid.knob() else { return };
        if y >= top && y < top + length {
            grid.bar_drag = Some(y - top);
            grid.glide = None;
        } else {
            let page = grid.viewport();
            scroll_by(app, if y < top { -page } else { page });
        }
        return cx.notify();
    }
    match grid.act_at(x, y) {
        Some(Act::Filter) => return app.open_filter_menu(event.position, cx),
        Some(Act::Clear) => {
            app.model.set_library_filter(Filter::default());
            app.after_model_update(cx);
            return cx.notify();
        }
        Some(Act::Size) => {
            grid.sliding = true;
            slide(app, x);
            return cx.notify();
        }
        None => {}
    }
    match grid.cell_at(x, y) {
        Some(index) => {
            let item = grid.cells[index].want.item.clone();
            if event.click_count >= 2 {
                app.model.shelf.selection.select(&item);
                close(app, true);
            } else {
                strip::click(&mut app.model, &item, &event.modifiers, false);
            }
        }
        None if y - TOP >= rules::SENTENCE && y - TOP < rules::SENTENCE + grid.viewport() => {
            // A band from the gaps: ⌘/Ctrl adds to what's selected.
            let sheet = (x, y - TOP - rules::SENTENCE + grid.scroll);
            let adding = if cfg!(target_os = "macos") { event.modifiers.platform } else { event.modifiers.control };
            let base = if adding { app.model.shelf.selection.selected().clone() } else { HashSet::new() };
            grid.dragging = Some((sheet, sheet, base));
            if !adding {
                app.model.shelf.selection.deselect_all();
                app.model.dirty |= dirty::STRIP | dirty::CHROME;
            }
        }
        None => {}
    }
    app.after_model_update(cx);
    cx.notify();
}

fn right_down(app: &mut NegsApp, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<NegsApp>) {
    if app.grid.flight.is_some() {
        return;
    }
    let (x, y) = xy(event.position);
    let Some(index) = app.grid.cell_at(x, y) else { return };
    let item = app.grid.cells[index].want.item.clone();
    app.open_library_menu(item, event.position, cx);
}

fn mouse_move(app: &mut NegsApp, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<NegsApp>) {
    let (x, y) = xy(event.position);
    let grid = &mut app.grid;
    if grid.flight.is_some() {
        return;
    }
    if grid.sliding {
        slide(app, x);
        return cx.notify();
    }
    if let Some(grab) = grid.bar_drag {
        let viewport = grid.viewport();
        grid.scroll = rules::bar_scroll(y - TOP - rules::SENTENCE - grab, grid.layout.height, viewport);
        grid.clamp();
        grid.scrolled_at = Some(Instant::now());
        request(app);
        return cx.notify();
    }
    let hover = grid.dragging.is_none() && grid.on_bar(x, y);
    if hover != grid.bar_hover {
        grid.bar_hover = hover;
        // It fades a second after the pointer leaves.
        grid.scrolled_at = Some(Instant::now());
        cx.notify();
    }
    let Some((start, _, base)) = grid.dragging.take() else { return };
    let now = (x, y - TOP - rules::SENTENCE + grid.scroll);
    band_select(&mut app.model.shelf.selection, &grid.layout, &grid.cells, start, now, &base);
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
    app.grid.dragging = Some((start, now, base));
    cx.notify();
}

fn mouse_up(app: &mut NegsApp, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<NegsApp>) {
    let grid = &mut app.grid;
    let was = grid.dragging.take().is_some() | std::mem::take(&mut grid.sliding);
    if grid.bar_drag.take().is_some() {
        let (x, y) = xy(event.position);
        grid.bar_hover = grid.on_bar(x, y);
        grid.scrolled_at = Some(Instant::now());
        return cx.notify();
    }
    if was {
        app.after_model_update(cx);
        cx.notify();
    }
}

fn scroll_wheel(app: &mut NegsApp, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<NegsApp>) {
    if app.grid.flight.is_some() {
        return;
    }
    let sizing = if cfg!(target_os = "macos") { event.modifiers.platform } else { event.modifiers.control };
    let (dy, notch) = match event.delta {
        ScrollDelta::Pixels(p) => (f32::from(p.y) as f64, false),
        ScrollDelta::Lines(l) => (l.y as f64, true),
    };
    if sizing {
        let notches = if notch { dy } else { dy / 50.0 };
        set_size(app, app.model.prefs.grid_size + rules::GRID_SIZE_STEP * notches);
    } else if notch {
        wheel(app, -dy * rules::WHEEL_STEP, true);
    } else {
        wheel(app, -dy, false);
    }
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight(opening: bool) -> Flight {
        let thumb = Arc::new(Thumb::blank_for_tests(30, 20));
        let mut cells = HashMap::new();
        cells.insert(0, (true, 0.0));
        cells.insert(1, (true, 32.0));
        cells.insert(2, (false, 16.0));
        let band = Rect::new(0.0, 700.0, 900.0, 186.0);
        let reach = Rect::new(0.0, -150.0, 1200.0, 1050.0);
        let flyers = vec![
            Flyer { index: 0, thumb: thumb.clone(), from: Rect::new(10.0, 706.0, 210.0, 140.0), to: Rect::new(16.0, 112.0, 264.0, 176.0), opacity: (1.0, 1.0), delay: 0.0 },
            Flyer { index: 1, thumb, from: Rect::new(226.0, 706.0, 210.0, 140.0), to: Rect::new(286.0, 112.0, 264.0, 176.0), opacity: (rules::STRIP_DIM as f32, 1.0), delay: 32.0 },
        ];
        Flight { opening, started: Instant::now(), band, reach, flyers, canvas_flyer: None, cells, end: if opening { 32.0 + DURATION + 20.0 } else { DURATION }, frozen: None }
    }

    fn roll(n: usize) -> (Vec<Item>, Vec<Cell>) {
        let items: Vec<Item> = (0..n).map(|i| Item::photo(format!("/roll/DSC_{i:04}.NEF"))).collect();
        let cells = items
            .iter()
            .map(|item| Cell { want: Want { item: item.clone(), key: None, loose: None }, name: String::new(), summary: Default::default(), open: false, selected: false, pair: false })
            .collect();
        (items, cells)
    }

    #[test]
    fn arrows_home_and_end_walk_the_sheet() {
        // Ten 3:2 frames, four to a row at this width.
        let layout = SheetLayout::new(&[1.5; 10], 4.0 * 264.0 + 3.0 * 24.0 + 2.0 * rules::GRID_SIDE, 176.0);
        let per_row = layout.vertical(0, 1).expect("a second row");
        assert!(per_row > 1);
        assert_eq!(moved("right", Some(0), 10, &layout), Some(Some(1)));
        assert_eq!(moved("right", Some(9), 10, &layout), Some(Some(9)), "stops at the last");
        assert_eq!(moved("left", Some(0), 10, &layout), Some(Some(0)), "stops at the first");
        assert_eq!(moved("down", Some(1), 10, &layout), Some(Some(1 + per_row)));
        assert_eq!(moved("up", Some(1 + per_row), 10, &layout), Some(Some(1)));
        assert_eq!(moved("up", Some(1), 10, &layout), Some(None), "nothing above the first row");
        assert_eq!(moved("home", Some(5), 10, &layout), Some(Some(0)));
        assert_eq!(moved("end", None, 10, &layout), Some(Some(9)));
        assert_eq!(moved("end", None, 0, &layout), Some(None), "an empty sheet");
        assert_eq!(moved("right", None, 10, &layout), Some(None), "nothing current");
        assert_eq!(moved("x", Some(0), 10, &layout), None, "not a move: the app's binding");
    }

    #[test]
    fn shift_arrows_grow_a_range_from_the_anchor() {
        let (items, _) = roll(6);
        let mut selection = Selection::default();
        move_selection(&mut selection, &items[2], &items, false);
        move_selection(&mut selection, &items[3], &items, true);
        move_selection(&mut selection, &items[4], &items, true);
        assert_eq!(selection.selected().len(), 3);
        assert_eq!(selection.current(), Some(&items[2]), "Shift moves the range, not the current one");
        move_selection(&mut selection, &items[1], &items, true);
        assert_eq!(selection.selected(), &HashSet::from([items[1].clone(), items[2].clone()]), "back past the anchor: the range turns");
        move_selection(&mut selection, &items[5], &items, false);
        assert_eq!(selection.selected(), &HashSet::from([items[5].clone()]));
    }

    #[test]
    fn the_rubber_band_adds_what_it_touches_to_what_was_selected() {
        let (items, cells) = roll(8);
        let layout = SheetLayout::new(&[1.5; 8], 4.0 * 264.0 + 3.0 * 24.0 + 2.0 * rules::GRID_SIDE, 176.0);
        let mut selection = Selection::default();
        selection.select(&items[7]);
        let base = selection.selected().clone();
        // Across the first row's first two frames, from left of the sheet.
        let a = &layout.frames[0];
        let b = &layout.frames[1];
        let start = (a.x - 4.0, a.y - 4.0);
        band_select(&mut selection, &layout, &cells, start, (b.x + 2.0, b.y + 2.0), &base);
        assert_eq!(selection.selected(), &HashSet::from([items[0].clone(), items[1].clone(), items[7].clone()]));
        // Dragged back: only what it touches now.
        band_select(&mut selection, &layout, &cells, start, (a.x + 2.0, a.y + 2.0), &base);
        assert_eq!(selection.selected(), &HashSet::from([items[0].clone(), items[7].clone()]));
        assert_eq!(selection.current(), Some(&items[7]), "the current one stays");
        // Between the rows' frames: nothing but the base.
        band_select(&mut selection, &layout, &cells, start, (a.x - 1.0, a.y - 1.0), &base);
        assert_eq!(selection.selected(), &base);
    }

    #[test]
    fn the_mask_grows_from_the_band_to_past_the_window_top() {
        let f = flight(true);
        assert_eq!(f.mask(0.0), f.band);
        assert_eq!(f.mask(DURATION), f.reach);
        let mid = f.mask(DURATION / 2.0);
        assert!(mid.y < f.band.y && mid.y > f.reach.y);
        // The curve is mostly there by a third of the way.
        let third = f.mask(DURATION / 3.0);
        assert!((third.y - f.reach.y) / (f.band.y - f.reach.y) < 0.3, "{third:?}");
        let fold = flight(false);
        assert_eq!(fold.mask(0.0), fold.reach);
        assert_eq!(fold.mask(DURATION), fold.band);
    }

    #[test]
    fn flyers_stagger_hold_their_start_and_land_on_their_cells() {
        let f = flight(true);
        let (r, o) = f.flyer_at(&f.flyers[1], 10.0).expect("waiting to start");
        assert_eq!(r, f.flyers[1].from, "a staggered flyer holds its start");
        assert!((o - rules::STRIP_DIM as f32).abs() < 1e-6, "it leaves the strip dimmed as the strip shows it");
        let (r, o) = f.flyer_at(&f.flyers[1], 32.0 + DURATION - 1.0).unwrap();
        assert!((r.x - f.flyers[1].to.x).abs() < 1.0 && (o - 1.0).abs() < 0.01);
        assert!(f.flyer_at(&f.flyers[1], 32.0 + DURATION).is_none(), "landed: the cell shows");
        // The carried cell waits hidden under it, its boxes fading in as it lands.
        assert_eq!(f.cell_alpha(1, 32.0 + DURATION - 1.0).0, 0.0);
        assert_eq!(f.cell_alpha(1, 32.0 + DURATION).0, 1.0);
        assert_eq!(f.cell_alpha(1, 32.0 + DURATION * 0.3).1, 0.0);
        assert!(f.cell_alpha(1, 32.0 + DURATION * 0.8).1 > 0.5);
        // The others fade up once the sheet is mostly open; the rest show.
        assert_eq!(f.cell_alpha(2, 100.0), (0.0, 0.0));
        assert!(f.cell_alpha(2, 16.0 + DURATION * 0.9).0 > 0.99);
        assert_eq!(f.cell_alpha(7, 0.0), (1.0, 1.0));
        assert!(!f.done(DURATION) && f.done(32.0 + DURATION + 20.0));
    }

    #[test]
    fn a_fold_fades_off_the_strip_without_brightening_the_frames() {
        let mut f = flight(false);
        for fl in &mut f.flyers {
            std::mem::swap(&mut fl.from, &mut fl.to);
            fl.opacity = (1.0, fl.opacity.0);
            fl.delay = 0.0;
        }
        assert_eq!(f.sheet_alpha(DURATION - FOLD_FADE - 1.0), 1.0);
        assert!((f.sheet_alpha(DURATION - FOLD_FADE / 2.0) - 0.5).abs() < 1e-3);
        assert_eq!(f.sheet_alpha(DURATION), 0.0);
        // The dimmed one's flyer (at a) over its frame, which shows `op`
        // through the fading sheet (at 1 - u): together `op` all the way.
        let op = rules::STRIP_DIM as f32;
        for step in 0..=10 {
            let u = step as f32 / 10.0;
            let e = DURATION - FOLD_FADE + u as f64 * FOLD_FADE;
            let (_, a) = f.flyer_at(&f.flyers[1], e).unwrap();
            let total = a + (1.0 - a) * op * u;
            assert!((total - op).abs() < 1e-4, "u {u}: {total}");
        }
        // Non-carried cells and the carried ones' boxes fade out early.
        assert!(f.cell_alpha(2, DURATION * 0.4).0 < 0.01);
        assert_eq!(f.cell_alpha(0, 0.0).0, 0.0);
        assert!(f.cell_alpha(0, 0.0).1 > 0.99);
        assert!(f.done(DURATION));
    }
}
