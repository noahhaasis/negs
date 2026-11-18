//! The film and paper tray (IMPLEMENTATION.md §4.5, `Tray.swift`): where
//! the strip and the status line were, a row of tiles, each the photograph
//! developed with a stock, grouped with the group's name written up its
//! left side, the favourites first; under it a foot with the group index
//! (the count while searching), the hint, Search (films), the paper lock's
//! switch (papers) and Done. A click tries a stock and the tray stays open;
//! keeping and going back are the model's (`model/tray.rs`), the rules
//! `tray_rules.rs`.
//!
//! One window, made the first time a tray opens, painted with Direct2D on
//! the GPU as the strip and grid are (`surface.rs`; WARP without a GPU):
//! the tiles by `paint_tiles`, then the titles and the foot. Until the
//! device is made it paints through a GDI back buffer. A group clicked in
//! the foot glides there in 0.25 s. Screen readers see the tiles, their
//! stars, the group index, Search, the lock and Done (`tray_access.rs`).

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::surface::{self, Surface};
use super::theme::{self, Tokens};
use super::{App, post, with_app};
use crate::platform::{pcwstr, wide};
use crate::previews::{Bitmap, Target};
use crate::tray_rules::{self as rules, Escape, EscapeContext, Hit, Key, Section, TrayKey};

const TITLE_STYLE: TextStyle = TextStyle::serif(13.5, 400).italic();
const NAME: TextStyle = TextStyle::serif(14.0, 400);
const NAME_SELECTED: TextStyle = TextStyle::serif(14.0, 600);
const BEFORE: TextStyle = TextStyle::serif(13.0, 400).italic();
const FOOT_WORDS: TextStyle = TextStyle::serif(13.0, 400).italic();
const DONE: TextStyle = TextStyle::serif(14.0, 600);
const NO_MATCH: TextStyle = TextStyle::serif(16.0, 400).italic();
/// The foot's sides, the gap between its parts, and Search's size.
const FOOT_PADDING: f32 = 18.0;
const FOOT_SPACING: f32 = 12.0;
const INDEX_SPACING: f32 = 14.0;
const SEARCH: (f32, f32) = (130.0, 18.0);
const SEARCH_ID: u16 = 1;

/// A tile as drawn: where its picture goes (DIPs in the tray), and how it looks.
#[derive(Clone)]
pub struct TileLook {
    pub picture: RectF,
    pub name: String,
    pub image: Option<Arc<Bitmap>>,
    /// An older edit of this photograph: dimmed.
    pub dimmed: bool,
    /// Being tried: ringed in pencil, its name in `paper` 600.
    pub selected: bool,
    /// The stock the tray opened on, once another is tried.
    pub before: bool,
    pub favourite: bool,
    /// Under the pointer: its name lit and its star shown.
    pub hot: bool,
    /// Words on the blank instead of a picture (Your negative); no star.
    pub words: Option<&'static str>,
}

/// What a click in the foot reaches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
    Index(usize),
    Keep,
    Done,
}

/// What the tray shows, from the model at the last sync.
#[derive(Default)]
struct Shown {
    paper: bool,
    sections: Vec<Section>,
    /// Each tile's thumbnail and whether it's out of date, by section.
    thumbs: Vec<Vec<(Option<Arc<Bitmap>>, bool)>>,
    favourites: Vec<String>,
    current: String,
    before_id: String,
    tried: bool,
    in_favourites: bool,
    query: String,
    before_name: Option<String>,
    keep_paper: bool,
    /// The row's scroll, in DIPs.
    offset: f64,
}

thread_local! {
    static HWND_: Cell<isize> = const { Cell::new(0) };
    static SEARCH_FIELD: Cell<isize> = const { Cell::new(0) };
    /// The open visit's kind as last synced; None while closed. A tray
    /// closes (a sync) before another opens, unless it's the other kind.
    static OPEN: Cell<Option<bool>> = const { Cell::new(None) };
    static SHOWN: RefCell<Shown> = RefCell::new(Shown::default());
    /// Where the foot's parts were painted, in DIPs.
    static PARTS: RefCell<Vec<(Part, RectF)>> = const { RefCell::new(Vec::new()) };
    static HOT_PART: Cell<Option<Part>> = const { Cell::new(None) };
    static HOT_TILE: Cell<Option<Hit>> = const { Cell::new(None) };
    static SURFACE: RefCell<Surface> = RefCell::new(Surface::default());
    static PICTURES: RefCell<Vec<Picture>> = const { RefCell::new(Vec::new()) };
    /// A group clicked in the foot: the row glides from, to, since.
    static GLIDE: Cell<Option<(f64, f64, Instant)>> = const { Cell::new(None) };
}

const TIMER_GLIDE: usize = 1;

fn hwnd() -> HWND {
    HWND(HWND_.with(Cell::get) as *mut _)
}

fn search_field() -> HWND {
    HWND(SEARCH_FIELD.with(Cell::get) as *mut _)
}

/// The tray's window, made (and its class registered) the first time a
/// tray opens, so launch never pays for it.
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
            lpszClassName: w!("NegsTray"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
    let tray = super::create(w!("NegsTray"), "", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), app.main, 0, HFONT::default());
    HWND_.with(|h| h.set(tray.0 as isize));
    tray
}

/// Places the tray at `rect` (pixels in the main window), or hides it.
pub fn layout(app: &App, rect: RECT, shown: bool) {
    if !shown && hwnd().is_invalid() {
        return;
    }
    let tray = ensure(app);
    unsafe {
        let _ = MoveWindow(tray, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, true);
        let _ = ShowWindow(tray, if shown { SW_SHOWNA } else { SW_HIDE });
    }
    if !shown {
        // Closed, it gives back its swap chain and pictures.
        SURFACE.with(|s| s.borrow_mut().release());
        PICTURES.with(|p| p.borrow_mut().clear());
        GLIDE.set(None);
        // Typing in a hidden field would go on searching.
        let field = search_field();
        if !field.is_invalid() && unsafe { GetFocus() } == field {
            unsafe {
                let _ = SetFocus(app.main);
            }
        }
    }
}

/// Follows the model: the open visit, its tiles, the stock tried, the
/// search; scrolls the row as the Mac's does.
pub fn sync(app: &mut App) {
    let model = &app.model;
    let Some(paper) = model.tray_shown().filter(|_| model.source.is_some()) else {
        OPEN.set(None);
        HOT_TILE.set(None);
        return;
    };
    let tray = ensure(app);
    let visit = model.tray().expect("a shown tray");
    let opened = OPEN.replace(Some(paper)) != Some(paper);
    let sections = model.tray_sections();
    let thumbs = thumbs(model, paper, &sections);
    let current = model.tray_current(paper, visit.read_as);
    let favourites = sections.iter().flat_map(|s| &s.stocks).filter(|e| model.prefs.is_favorite(&e.id, paper)).map(|e| e.id.clone()).collect();
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(tray, &mut client);
    }
    let visible = client.right as f64 / draw::dpi_of(tray) as f64;
    let changed = SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        let content = rules::content_width(&sections);
        let mut offset = shown.offset;
        if opened {
            // It opens on the current stock, in the middle.
            HOT_TILE.set(None);
            offset = rules::reveal(0.0, &current, visit.in_favourites, true, &sections, visible);
        } else if shown.query != visit.query {
            // Each search starts at its first match; clearing it goes back to the stock.
            offset = if visit.query.is_empty() { rules::reveal(offset, &current, visit.in_favourites, true, &sections, visible) } else { 0.0 };
        } else if shown.current != current || shown.in_favourites != visit.in_favourites {
            // Stepped to (or tried from the search's first match): shown if out of view.
            offset = rules::reveal(offset, &current, visit.in_favourites, false, &sections, visible);
        }
        let next = Shown {
            paper,
            sections,
            thumbs,
            favourites,
            tried: visit.tried(&current),
            current,
            before_id: visit.before_id.clone(),
            in_favourites: visit.in_favourites,
            query: visit.query.clone(),
            before_name: model.tray_before_name(),
            keep_paper: model.prefs.keep_paper,
            offset: rules::clamp(offset, visible, content),
        };
        if next.offset != shown.offset {
            // Stepped or searched elsewhere: that wins over a glide.
            GLIDE.set(None);
        }
        let changed = !same(&shown, &next);
        let listed = shown.sections != next.sections || shown.favourites != next.favourites;
        let tried = shown.current != next.current || shown.in_favourites != next.in_favourites;
        *shown = next;
        (changed, listed, tried)
    });
    let (changed, listed, tried) = changed;
    sync_search(app, tray, paper, opened);
    if changed || opened {
        super::tray_access::changed(listed || opened, tried || opened);
        unsafe {
            let _ = InvalidateRect(tray, None, false);
        }
    }
}

/// Brings the tiles up to date with the model's thumbnails.
pub fn redraw(app: &mut App) {
    let model = &app.model;
    let Some(paper) = model.tray_shown() else { return };
    let changed = SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        if shown.paper != paper {
            return false;
        }
        let next = thumbs(model, paper, &shown.sections);
        let changed = !same_thumbs(&shown.thumbs, &next);
        shown.thumbs = next;
        changed
    });
    if changed {
        unsafe {
            let _ = InvalidateRect(hwnd(), None, false);
        }
    }
}

fn thumbs(model: &crate::model::Model, paper: bool, sections: &[Section]) -> Vec<Vec<(Option<Arc<Bitmap>>, bool)>> {
    sections
        .iter()
        .map(|s| {
            s.stocks
                .iter()
                .map(|e| {
                    let thumb = model.previews.thumb(&Target::Stock { id: e.id.clone(), paper });
                    (thumb.and_then(|t| t.image.clone()), thumb.is_some_and(|t| t.dimmed))
                })
                .collect()
        })
        .collect()
}

fn same_thumbs(a: &[Vec<(Option<Arc<Bitmap>>, bool)>], b: &[Vec<(Option<Arc<Bitmap>>, bool)>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.len() == b.len()
                && a.iter().zip(b).all(|((ia, da), (ib, db))| {
                    da == db
                        && match (ia, ib) {
                            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                            (None, None) => true,
                            _ => false,
                        }
                })
        })
}

fn same(a: &Shown, b: &Shown) -> bool {
    a.paper == b.paper
        && a.sections == b.sections
        && same_thumbs(&a.thumbs, &b.thumbs)
        && a.favourites == b.favourites
        && a.current == b.current
        && a.before_id == b.before_id
        && a.tried == b.tried
        && a.in_favourites == b.in_favourites
        && a.query == b.query
        && a.before_name == b.before_name
        && a.keep_paper == b.keep_paper
        && a.offset == b.offset
}

/// Search, for films: made with the first film tray; its text follows the
/// visit's (a new visit's is empty).
fn sync_search(app: &App, tray: HWND, paper: bool, opened: bool) {
    let mut field = search_field();
    if field.is_invalid() && !paper {
        field = super::create(w!("EDIT"), "", WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WINDOW_EX_STYLE(0), tray, SEARCH_ID, app.fonts.ui);
        super::accessible_name(field, "Search films");
        let prompt = wide("Search films");
        unsafe {
            // Shown while focused too, as the Mac's placeholder is.
            SendMessageW(field, EM_SETCUEBANNER, WPARAM(1), LPARAM(prompt.as_ptr() as isize));
            let _ = SetWindowSubclass(field, Some(search_subclass), 1, 0);
        }
        SEARCH_FIELD.with(|f| f.set(field.0 as isize));
    }
    if field.is_invalid() {
        return;
    }
    unsafe {
        if SendMessageW(field, WM_GETFONT, WPARAM(0), LPARAM(0)).0 != app.fonts.ui.0 as isize {
            SendMessageW(field, WM_SETFONT, WPARAM(app.fonts.ui.0 as usize), LPARAM(0));
        }
        let query = app.model.tray().map(|t| t.query.clone()).unwrap_or_default();
        if draw::window_text(field) != query {
            // Its EN_CHANGE finds the app busy, and the query already set.
            let text = wide(&query);
            let _ = SetWindowTextW(field, pcwstr(&text));
        }
        let _ = ShowWindow(field, if paper { SW_HIDE } else { SW_SHOWNA });
        if opened && paper && GetFocus() == field {
            let _ = SetFocus(app.main);
        }
    }
}

/// The tray's keys (`EditorWindowController.trayKeys`): Esc goes back, Return
/// keeps, ← → step; not while typing, nor with Ctrl, Alt or the Windows key
/// (a command). True if handled.
pub fn key(app: &mut App, msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || app.model.tray().is_none() {
        return false;
    }
    let key = match VIRTUAL_KEY(msg.wParam.0 as u16) {
        // Esc's order (§5.2): an alert (modal, so never here), typing, then the tray.
        VK_ESCAPE if escape(app) != Escape::Tray => return false,
        VK_ESCAPE => Key::Escape,
        VK_RETURN => Key::Return,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        _ => Key::Other,
    };
    // A focused slider takes the arrows (and a list or field all keys).
    let focus = super::focused_class();
    let slider = matches!(focus.as_deref(), Some("msctls_trackbar32"));
    let typing = super::text_focus() || (slider && matches!(key, Key::Left | Key::Right));
    match rules::tray_key(key, super::modifier_down(), typing) {
        TrayKey::GoBack => app.model.end_tray(false),
        TrayKey::Keep => app.model.end_tray(true),
        TrayKey::Step(offset) => app.model.step_tray(offset),
        TrayKey::Pass => return false,
    }
    true
}

/// What Esc does now, of all its uses (`escapeAction`).
fn escape(app: &App) -> Escape {
    rules::escape(EscapeContext {
        alert: false,
        typing: super::text_focus(),
        tray: app.model.tray().is_some(),
        cropping: app.model.cropping(),
        // The library grid is phase 6's.
        grid: false,
        image_only: app.view.image_only,
        photograph: app.model.source.is_some(),
    })
}

/// Whether a press at `msg` leaves the tray open: on the tray, or on a
/// name that opens it (the label's names and lock). Any other click keeps
/// the stock and closes it.
pub fn keeps_open(app: &App, msg: &MSG) -> bool {
    let tray = hwnd();
    let target = msg.hwnd;
    if tray.is_invalid() || target == tray || unsafe { IsChild(tray, target) }.as_bool() {
        return true;
    }
    if target == app.canvas {
        let (x, y) = ((msg.lParam.0 & 0xffff) as i16 as i32, ((msg.lParam.0 >> 16) & 0xffff) as i16 as i32);
        return super::canvas::opens_tray_at(x, y);
    }
    false
}

// MARK: Painting

/// `text` cut with an ellipsis to fit `width` DIPs in `style`.
fn fitted(p: &mut Painter, text: &str, style: &TextStyle, width: f32) -> String {
    if p.measure(text, style, 0.0).width <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…";
        if p.measure(&candidate, style, 0.0).width <= width { low = mid } else { high = mid - 1 }
    }
    if low == 0 { String::new() } else { chars[..low].iter().collect::<String>().trim_end().to_string() + "…" }
}

/// The favourite's star (`StarShape`, the mockup's 20 × 20 path) drawn 15
/// square in the 20 square box at (`x`, `y`): filled in the film's pencil on
/// a favourite, else outlined for the photograph, over a dark shadow.
fn star(p: &mut Painter, x: f32, y: f32, favourite: bool, t: &Tokens) {
    const POINTS: [(f64, f64); 10] = [(10.0, 2.6), (12.2, 7.5), (17.4, 8.0), (13.5, 11.5), (14.6, 16.7), (10.0, 14.0), (5.4, 16.7), (6.5, 11.5), (2.6, 8.0), (7.8, 7.5)];
    let shape = || {
        let scale = 15.0 / 14.8;
        let points: Vec<negs_marks::Pt> = POINTS.iter().map(|&(px, py)| negs_marks::Pt { x: 2.5 + (px - 2.6) * scale, y: 2.5 + (py - 2.6) * scale }).collect();
        negs_marks::Path::polygon(&points)
    };
    const ID: u64 = 0x7a57;
    p.path(ID, shape, x, y, Some(3.0), Ink::hex(0).alpha(0.45));
    if favourite {
        p.path(ID, shape, x, y, None, Ink::hex(t.film.accent));
        p.path(ID, shape, x, y, Some(1.2), Ink::hex(t.film.accent));
    } else {
        p.path(ID, shape, x, y, Some(1.4), Ink::hex(t.on_photo()));
    }
}

/// The tiles' pictures on the GPU, made from the thumbnails' pixels (kept
/// by the previews): the ones shown, made again on a new device.
struct Picture {
    thumb: Arc<Bitmap>,
    image: Option<surface::Image>,
}

/// Uploads the pictures `looks` show that aren't on the device yet, then
/// lets go of the least recently shown beyond a few screens' worth.
fn upload(looks: &[TileLook]) {
    const KEPT_BYTES: usize = 48 << 20;
    PICTURES.with(|pictures| {
        let mut pictures = pictures.borrow_mut();
        for thumb in looks.iter().filter_map(|l| l.image.as_ref()) {
            let found = pictures.iter().position(|p| Arc::ptr_eq(&p.thumb, thumb));
            let fresh = found.is_some_and(|i| pictures[i].image.as_ref().is_some_and(|image| image.current()));
            let picture = match found {
                Some(i) if fresh => pictures.remove(i),
                found => {
                    if let Some(i) = found {
                        pictures.remove(i);
                    }
                    Picture { thumb: thumb.clone(), image: surface::upload(&thumb.bgra, thumb.width, thumb.height) }
                }
            };
            // The most recently shown last.
            pictures.push(picture);
        }
        let mut bytes: usize = pictures.iter().filter_map(|p| p.image.as_ref()).map(|i| i.bytes()).sum();
        while bytes > KEPT_BYTES && pictures.len() > looks.len() {
            let gone = pictures.remove(0);
            bytes -= gone.image.as_ref().map_or(0, |i| i.bytes());
        }
    });
}

/// The tiles: each picture filling its place, blank until made and dimmed
/// while out of date; the pencil ring round the one tried, the names
/// ("before" after the one the tray opened on) and the stars. On the GPU
/// (`surface.rs`), as the strip and grid are, after `upload`.
pub fn paint_tiles(p: &mut Painter, t: &Tokens, looks: &[TileLook]) {
    PICTURES.with(|pictures| {
        let pictures = pictures.borrow();
        for look in looks {
            let r = look.picture;
            p.fill_rect(r, Ink::hex(t.film.blank));
            let image = look.image.as_ref().and_then(|thumb| pictures.iter().find(|p| Arc::ptr_eq(&p.thumb, thumb))?.image.as_ref());
            if let Some(image) = image.filter(|i| i.current()) {
                p.picture(image.bitmap(), (image.width, image.height), r, 1.0);
                if look.dimmed {
                    p.fill_rect(r, Ink::hex(t.film.blank).alpha(0.45));
                }
            }
        }
    });
    tile_marks(p, t, looks);
}

/// The tiles' pictures through GDI, while the GPU device isn't made (or
/// can't paint).
fn paint_tiles_gdi(c: &mut Canvas, s: f32, t: &Tokens, looks: &[TileLook]) {
    let blank = theme::gdi(t.film.blank);
    for look in looks {
        let r = look.picture;
        let frame = RECT { left: (r.x * s).round() as i32, top: (r.y * s).round() as i32, right: ((r.x + r.w) * s).round() as i32, bottom: ((r.y + r.h) * s).round() as i32 };
        super::inspector::draw_thumbnail(c, frame, 0.0, look.image.as_deref(), look.dimmed, blank, blank);
    }
}

/// What's drawn over the tiles' pictures: the ring, the names, the stars.
fn tile_marks(p: &mut Painter, t: &Tokens, looks: &[TileLook]) {
    for look in looks {
        let r = look.picture;
        if look.selected {
            // `PencilRing`, 7 out at the sides, 6 above, 1 below, tilted −1°.
            p.ring(RectF::new(r.x - 7.0, r.y - 6.0, r.w + 14.0, r.h + 7.0), negs_marks::Radii::BOX, -1.0, Ink::hex(t.film.accent));
        }
        let style = if look.selected { NAME_SELECTED } else { NAME };
        let ink = if look.selected { t.film.paper } else if look.before || look.hot { t.film.label } else { t.film.dim };
        let before = if look.before { p.measure("before", &BEFORE, 0.0).width + 6.0 } else { 0.0 };
        let name = fitted(p, &look.name, &style, (r.w - before).max(0.0));
        let top = r.y + r.h + rules::NAME_GAP as f32;
        let m = p.measure(if name.is_empty() { "Ag" } else { &name }, &style, 0.0);
        let y = (top + (rules::NAME_LINE as f32 - m.height) / 2.0).round();
        let drawn = p.text(&name, &style, r.x, y, Ink::hex(ink));
        if look.before {
            let b = p.measure("before", &BEFORE, 0.0);
            p.text("before", &BEFORE, r.x + drawn.width + 6.0, y + m.baseline - b.baseline, Ink::hex(t.film.dim));
        }
        if let Some(words) = look.words {
            // `NegativeTile`: serif 15 italic, centred, 18 in from the sides.
            let style = TextStyle::serif(15.0, 400).italic().line_height(1.25);
            let width = r.w - 36.0;
            let m = p.measure_in(words, &style, width, super::d2d::Align::Center);
            p.text_in(words, &style, RectF::new(r.x + 18.0, r.y + (r.h - m.height) / 2.0, width, m.height + 4.0), super::d2d::Align::Center, Ink::hex(t.film.label));
        }
        if (look.favourite || look.hot) && look.words.is_none() {
            let (sx, sy, _, _) = rules::star_rect(0.0);
            p.clipped(r, |p| star(p, r.x + sx as f32, r.y + (sy - rules::TOP) as f32, look.favourite, t));
        }
    }
}

/// The tiles on screen, as drawn.
fn looks(v: &Shown, w: f32, hot_tile: Option<Hit>) -> Vec<TileLook> {
    let offset = v.offset as f32;
    let starts = rules::starts(&v.sections);
    let visit = rules::Visit::new(v.paper, (), v.before_id.clone());
    let mut looks = vec![];
    for (si, (section, start)) in v.sections.iter().zip(&starts).enumerate() {
        for (i, entry) in section.stocks.iter().enumerate() {
            let x = rules::tile_left(*start, i) as f32 - offset;
            if x + rules::TILE as f32 + 8.0 < 0.0 || x - 8.0 > w {
                continue;
            }
            let (image, dimmed) = v.thumbs.get(si).and_then(|t| t.get(i)).cloned().unwrap_or((None, false));
            let hot = matches!(hot_tile, Some(Hit::Tile(a, b) | Hit::Star(a, b)) if (a, b) == (si, i));
            looks.push(TileLook {
                picture: RectF::new(x, rules::TOP as f32, rules::TILE as f32, rules::PICTURE as f32),
                name: entry.name.clone(),
                image,
                dimmed,
                // Both copies of a favourite, as on the Mac.
                selected: entry.id == v.current,
                before: visit.shows_before(&entry.id, &v.current),
                favourite: v.favourites.contains(&entry.id),
                hot,
                words: (entry.id == rules::YOUR_NEGATIVE).then_some(crate::negative::TILE_TEXT),
            });
        }
    }
    looks
}

/// Where the foot's parts and Search went, as painted.
#[derive(Default)]
struct Painted {
    parts: Vec<(Part, RectF)>,
    search: Option<RectF>,
}

/// The groups' names up their sides, and the foot.
fn titles_and_foot(p: &mut Painter, t: &Tokens, v: &Shown, w: f32, h: f32, hot_part: Option<Part>, typing: bool) -> Painted {
    let mut painted = Painted::default();
    let offset = v.offset as f32;
    let starts = rules::starts(&v.sections);
    let group = rules::current(v.offset, &starts);
    // Each group's name written up its left side, centred on the pictures.
    for (section, start) in v.sections.iter().zip(&starts) {
        let x = *start as f32 - offset;
        if x + (rules::TITLE as f32) < 0.0 || x > w {
            continue;
        }
        let m = p.measure(&section.title, &TITLE_STYLE, 0.0);
        let y = rules::TOP as f32 + (rules::PICTURE as f32 + m.width) / 2.0;
        p.text_up(&section.title, &TITLE_STYLE, x + ((rules::TITLE as f32 - m.height) / 2.0).round(), y.round(), Ink::hex(t.film.label));
    }
    if v.sections.is_empty() {
        p.text(&rules::no_match(&v.query, v.paper), &NO_MATCH, 30.0, 46.0, Ink::hex(t.film.dim));
    }
    // The foot, from the right: Done, the lock's switch or Search, then the hint.
    let foot = rules::ROW as f32;
    let middle = foot + (rules::FOOT as f32 - 2.0) / 2.0;
    let line = p.measure("Ag", &FOOT_WORDS, 0.0);
    let baseline = (middle - line.height / 2.0 + line.baseline).round();
    let mut right = w - FOOT_PADDING;
    let done = p.measure("Done", &DONE, 0.0);
    let x = right - done.width;
    p.text("Done", &DONE, x, baseline - done.baseline, Ink::hex(t.film.paper));
    painted.parts.push((Part::Done, RectF::new(x - 6.0, foot, done.width + 12.0, h - foot)));
    right = x - 10.0 - FOOT_SPACING;
    if v.paper {
        let words = rules::keep_paper_words(v.keep_paper);
        let lit = hot_part == Some(Part::Keep);
        let m = p.measure(words, &FOOT_WORDS, 0.0);
        let width = 10.0 + 5.0 + m.width;
        let x = right - width;
        let (lock, ink) = if v.keep_paper { (t.film.accent, t.film.paper) } else if lit { (t.film.label, t.film.label) } else { (t.film.dim, t.film.dim) };
        super::canvas::draw_lock(p, v.keep_paper, x, baseline - 11.0, Ink::hex(lock));
        p.text(words, &FOOT_WORDS, x + 15.0, baseline - m.baseline, Ink::hex(ink));
        painted.parts.push((Part::Keep, RectF::new(x - 4.0, foot, width + 8.0, h - foot)));
        right = x - 6.0 - FOOT_SPACING;
    } else {
        let (sw, sh) = SEARCH;
        let x = right - sw;
        let top = (middle - sh / 2.0).round();
        painted.search = Some(RectF::new(x, top, sw, sh));
        if typing {
            p.underline(RectF::new(x, top + sh - 2.0, sw, 5.0), t, true);
        } else {
            p.fill_rect(RectF::new(x, top + sh, sw, 1.0), Ink::hex(t.film.line));
        }
        right = x - 6.0 - FOOT_SPACING;
    }
    // The index, or the count while searching, from the left.
    let mut left = FOOT_PADDING;
    if rules::searching(&v.query) {
        let count = v.sections.iter().map(|s| s.stocks.len()).sum();
        let words = rules::matches(count);
        let shown = fitted(p, &words, &FOOT_WORDS, (right - left).max(0.0));
        left += p.text(&shown, &FOOT_WORDS, left, baseline - line.baseline, Ink::hex(t.film.label)).width;
    } else {
        for (i, section) in v.sections.iter().enumerate() {
            let m = p.measure(&section.title, &FOOT_WORDS, 0.0);
            if left + m.width > right {
                break;
            }
            let selected = i == group;
            if selected {
                p.underline(RectF::new(left, baseline + 1.0, m.width, 5.0), t, true);
            }
            let ink = if selected { t.film.paper } else if hot_part == Some(Part::Index(i)) { t.film.label } else { t.film.dim };
            p.text(&section.title, &FOOT_WORDS, left, baseline - m.baseline, Ink::hex(ink));
            painted.parts.push((Part::Index(i), RectF::new(left - 4.0, foot, m.width + 8.0, h - foot)));
            left += m.width + INDEX_SPACING;
        }
    }
    // The hint, against Search or the switch; only whole (a stub
    // like "clic…" said nothing at a 1440 px window).
    let room = right - (left + FOOT_SPACING);
    let hint = rules::hint(v.before_name.as_deref());
    let m = p.measure(&hint, &FOOT_WORDS, 0.0);
    if m.width <= room {
        p.text(&hint, &FOOT_WORDS, right - m.width, baseline - line.baseline, Ink::hex(t.film.dim));
    }
    painted
}

/// Paints the tray on the GPU, as the strip and grid are (WARP without a
/// GPU); with GDI until the device is made, or if it can't paint.
fn paint(hwnd: HWND) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
    let hot_part = HOT_PART.get();
    let search = search_field();
    let typing = !search.is_invalid() && unsafe { GetFocus() } == search;
    let painted = SHOWN.with(|shown| {
        let Ok(v) = shown.try_borrow() else { return None };
        let looks = looks(&v, w, HOT_TILE.get());
        let mut painted = None;
        if surface::ready() {
            upload(&looks);
            let buffer = (client.right.max(1) as u32, client.bottom.max(1) as u32);
            let on_gpu = SURFACE.with(|surface| {
                let Ok(mut surface) = surface.try_borrow_mut() else { return false };
                surface.paint(hwnd, buffer, s, t.plain, |p| {
                    p.fill_rect(RectF::new(0.0, 0.0, w, h), Ink::hex(t.hex.film));
                    paint_tiles(p, &t, &looks);
                    painted = Some(titles_and_foot(p, &t, &v, w, h, hot_part, typing));
                })
            });
            if on_gpu {
                unsafe {
                    let _ = ValidateRect(hwnd, None);
                }
                return painted;
            }
        }
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut c = Canvas::new(dc, client);
            c.clear(theme::gdi(t.hex.film));
            paint_tiles_gdi(&mut c, s, &t, &looks);
            c.d2d(s, t.plain, |p| {
                tile_marks(p, &t, &looks);
                painted = Some(titles_and_foot(p, &t, &v, w, h, hot_part, typing));
            });
            c.present(dc);
            let _ = EndPaint(hwnd, &ps);
        }
        painted
    });
    let Some(painted) = painted else {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    };
    if let Some(r) = painted.search {
        place_search(r, s);
    }
    PARTS.with(|p| *p.borrow_mut() = painted.parts);
    place_tips(hwnd, s);
}

/// Search's box over its rule: the text a little in from its edges.
fn place_search(r: RectF, s: f32) {
    let field = search_field();
    if field.is_invalid() {
        return;
    }
    let (x, y, w, h) = ((r.x * s).round() as i32, (r.y * s).round() as i32, (r.w * s).round() as i32, (r.h * s).round() as i32);
    unsafe {
        let mut now = RECT::default();
        let _ = GetWindowRect(field, &mut now);
        let mut at = POINT { x: now.left, y: now.top };
        let _ = ScreenToClient(hwnd(), &mut at);
        if (at.x, at.y, now.right - now.left, now.bottom - now.top) != (x, y, w, h) {
            let _ = MoveWindow(field, x, y, w, h, true);
        }
    }
}

/// The tooltips: Done, the lock's switch, and the tile or star under the pointer.
fn place_tips(hwnd: HWND, s: f32) {
    let px = |r: RectF| RECT { left: (r.x * s) as i32, top: (r.y * s) as i32, right: ((r.x + r.w) * s) as i32, bottom: ((r.y + r.h) * s) as i32 };
    let parts = PARTS.with(|p| p.borrow().clone());
    let place = |part: Part| parts.iter().find(|(p, _)| *p == part).map_or(RECT::default(), |(_, r)| px(*r));
    let (paper, keep, tile) = SHOWN.with(|v| {
        let v = v.borrow();
        let tile = HOT_TILE.get().and_then(|hit| {
            let (Hit::Tile(si, i) | Hit::Star(si, i)) = hit;
            let entry = v.sections.get(si)?.stocks.get(i)?;
            let left = rules::tile_left(rules::starts(&v.sections)[si], i) - v.offset;
            Some((hit, entry.name.clone(), v.favourites.contains(&entry.id), left))
        });
        (v.paper, v.keep_paper, tile)
    });
    super::dialogs::area_tooltip(hwnd, 1, place(Part::Done), &rules::done_help(paper));
    super::dialogs::area_tooltip(hwnd, 2, place(Part::Keep), if keep { crate::help::PAPER_KEPT } else { crate::help::PAPER_LOCK });
    let (mut tile_rect, mut star_rect, mut name, mut star_tip) = (RECT::default(), RECT::default(), String::new(), "");
    if let Some((hit, tile_name, favourite, left)) = tile {
        let (sx, sy, sw, sh) = rules::star_rect(left);
        match hit {
            Hit::Star(..) => star_rect = px(RectF::new(sx as f32, sy as f32, sw as f32, sh as f32)),
            Hit::Tile(..) => tile_rect = px(RectF::new(left as f32, rules::TOP as f32, rules::TILE as f32, (rules::PICTURE + rules::NAME_GAP + rules::NAME_LINE) as f32)),
        }
        name = tile_name;
        star_tip = rules::star_help(favourite);
    }
    super::dialogs::area_tooltip(hwnd, 3, tile_rect, &name);
    super::dialogs::area_tooltip(hwnd, 4, star_rect, star_tip);
}

// MARK: Pointer

fn part_at(x: f32, y: f32) -> Option<Part> {
    PARTS.with(|p| p.borrow().iter().find(|(_, r)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(part, _)| *part))
}

fn tile_at(x: f32, y: f32) -> Option<Hit> {
    SHOWN.with(|v| {
        let v = v.borrow();
        rules::hit(&v.sections, x as f64 + v.offset, y as f64)
    })
}

fn set_hot(hwnd: HWND, part: Option<Part>, tile: Option<Hit>) {
    if HOT_PART.replace(part) != part || HOT_TILE.replace(tile) != tile {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

fn click(x: f32, y: f32) {
    if let Some(part) = part_at(x, y) {
        return press_part(part);
    }
    if let Some(hit) = tile_at(x, y) {
        press_tile(hit);
    }
}

/// Done keeps, the switch locks or frees the paper, a group's name glides
/// the row to it: clicked, or a screen reader's default action.
fn press_part(part: Part) {
    match part {
        Part::Done => {
            with_app(|app| app.model.end_tray(true));
        }
        Part::Keep => {
            with_app(|app| {
                let keep = !app.model.prefs.keep_paper;
                app.model.set_keep_paper(keep);
                app.model.dirty |= crate::model::dirty::CANVAS | crate::model::dirty::TRAY;
            });
        }
        Part::Index(i) => {
            // Glides to the group, 30 in from the left.
            let (starts, content) = SHOWN.with(|v| {
                let v = v.borrow();
                (rules::starts(&v.sections), rules::content_width(&v.sections))
            });
            if let Some(start) = starts.get(i) {
                glide_to(rules::jump(*start, visible_width(), content));
            }
        }
    }
}

/// A tile tries its stock; its star stars or unstars it.
fn press_tile(hit: Hit) {
    let (Hit::Tile(si, i) | Hit::Star(si, i)) = hit;
    let Some((id, favourites, paper)) = SHOWN.with(|v| {
        let v = v.borrow();
        let section = v.sections.get(si)?;
        Some((section.stocks.get(i)?.id.clone(), section.favourites, v.paper))
    }) else {
        return;
    };
    match hit {
        Hit::Star(..) if id != rules::YOUR_NEGATIVE => {
            with_app(|app| {
                app.model.toggle_favorite(&id, paper);
                app.model.dirty |= crate::model::dirty::TRAY;
            });
        }
        Hit::Tile(..) | Hit::Star(..) => {
            with_app(|app| app.model.choose_tile(&id, favourites));
        }
    }
}

// MARK: Screen readers

/// What a screen reader can press (`tray_access.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Press {
    Part(Part),
    Tile(Hit),
    /// Search: its own window.
    Search,
    Nothing,
}

/// One thing in the tray as a screen reader sees it: in the Mac's words
/// (`TrayTile`, `IndexButton`, `KeepPaperSwitch`), its place in DIPs in
/// the tray, and what pressing it does.
#[derive(Clone, Debug)]
pub(super) struct Spoken {
    pub name: String,
    pub value: String,
    pub help: String,
    pub role: u32,
    pub state: u32,
    pub place: RectF,
    pub press: Press,
}

const ROLE_STATICTEXT: u32 = 0x29;
const ROLE_PUSHBUTTON: u32 = 0x2b;
const ROLE_CHECKBUTTON: u32 = 0x2c;
const ROLE_TEXT: u32 = 0x2a;
pub(super) const STATE_SELECTED: u32 = 0x2;
const STATE_CHECKED: u32 = 0x10;
const STATE_INVISIBLE: u32 = 0x8000;
const STATE_OFFSCREEN: u32 = 0x10000;
const STATE_SELECTABLE: u32 = 0x200000;

/// The tray's name for screen readers, and what's in it, in reading order:
/// each group's name, its tiles each with its star, then the foot (the
/// index or the count, Search or the switch, Done). None while it's busy
/// or closed.
pub(super) fn spoken() -> Option<(String, Vec<Spoken>)> {
    OPEN.get()?;
    let parts = PARTS.with(|p| p.try_borrow().ok().map(|p| p.clone()))?;
    let search = search_field();
    SHOWN.with(|v| {
        let v = v.try_borrow().ok()?;
        let visible = visible_width() as f32;
        let offset = v.offset as f32;
        let starts = rules::starts(&v.sections);
        let visit = rules::Visit::new(v.paper, (), v.before_id.clone());
        let shown = |r: RectF| if r.x + r.w <= 0.0 || r.x >= visible { STATE_OFFSCREEN } else { 0 };
        let mut all = vec![];
        for (si, (section, start)) in v.sections.iter().zip(&starts).enumerate() {
            let x = *start as f32 - offset;
            let title = RectF::new(x, rules::TOP as f32, rules::TITLE as f32, rules::PICTURE as f32);
            all.push(Spoken { name: section.title.clone(), value: String::new(), help: String::new(), role: ROLE_STATICTEXT, state: shown(title), place: title, press: Press::Nothing });
            for (i, entry) in section.stocks.iter().enumerate() {
                let left = rules::tile_left(*start, i) - v.offset;
                let tile = RectF::new(left as f32, rules::TOP as f32, rules::TILE as f32, (rules::PICTURE + rules::NAME_GAP + rules::NAME_LINE) as f32);
                let selected = entry.id == v.current;
                let before = visit.shows_before(&entry.id, &v.current);
                let state = shown(tile) | STATE_SELECTABLE | if selected { STATE_SELECTED } else { 0 };
                // Your negative says what it does, and has no star.
                let negative = entry.id == rules::YOUR_NEGATIVE;
                all.push(Spoken {
                    name: entry.name.clone(),
                    value: rules::tile_value(selected, before).into(),
                    help: if negative { crate::negative::TILE_TEXT.into() } else { String::new() },
                    role: ROLE_PUSHBUTTON,
                    state,
                    place: tile,
                    press: Press::Tile(Hit::Tile(si, i)),
                });
                if negative {
                    continue;
                }
                let favourite = v.favourites.contains(&entry.id);
                let (sx, sy, sw, sh) = rules::star_rect(left);
                let star = RectF::new(sx as f32, sy as f32, sw as f32, sh as f32);
                all.push(Spoken {
                    name: rules::star_label(&entry.name, favourite),
                    value: String::new(),
                    help: rules::star_help(favourite).into(),
                    role: ROLE_PUSHBUTTON,
                    state: shown(star),
                    place: star,
                    press: Press::Tile(Hit::Star(si, i)),
                });
            }
        }
        let painted = |part: Part| parts.iter().find(|(p, _)| *p == part).map(|(_, r)| *r);
        let foot = RectF::new(0.0, rules::ROW as f32, visible, rules::FOOT as f32);
        if rules::searching(&v.query) {
            let count = v.sections.iter().map(|s| s.stocks.len()).sum();
            all.push(Spoken { name: rules::matches(count), value: String::new(), help: String::new(), role: ROLE_STATICTEXT, state: 0, place: foot, press: Press::Nothing });
        } else {
            let group = rules::current(v.offset, &starts);
            for (i, section) in v.sections.iter().enumerate() {
                let place = painted(Part::Index(i));
                let state = if place.is_some() { 0 } else { STATE_INVISIBLE } | if i == group { STATE_SELECTED } else { 0 };
                all.push(Spoken {
                    name: rules::index_label(&section.title),
                    value: String::new(),
                    help: String::new(),
                    role: ROLE_PUSHBUTTON,
                    state,
                    place: place.unwrap_or(foot),
                    press: Press::Part(Part::Index(i)),
                });
            }
        }
        if v.paper {
            let place = painted(Part::Keep).unwrap_or(foot);
            let help = if v.keep_paper { crate::help::PAPER_KEPT } else { crate::help::PAPER_LOCK };
            let state = if v.keep_paper { STATE_CHECKED } else { 0 };
            let value = if v.keep_paper { "On" } else { "Off" };
            all.push(Spoken { name: rules::KEEP_PAPER_LABEL.into(), value: value.into(), help: help.into(), role: ROLE_CHECKBUTTON, state, place, press: Press::Part(Part::Keep) });
        } else if !search.is_invalid() {
            all.push(Spoken { name: "Search films".into(), value: v.query.clone(), help: String::new(), role: ROLE_TEXT, state: 0, place: foot, press: Press::Search });
        }
        let place = painted(Part::Done).unwrap_or(foot);
        all.push(Spoken { name: "Done".into(), value: String::new(), help: rules::done_help(v.paper), role: ROLE_PUSHBUTTON, state: 0, place, press: Press::Part(Part::Done) });
        Some((if v.paper { "Papers" } else { "Films" }.to_string(), all))
    })
}

/// A screen reader's press, run from the tray's own message loop.
pub(super) fn press(press: Press) {
    match press {
        Press::Part(part) => press_part(part),
        Press::Tile(hit) => press_tile(hit),
        Press::Search => {
            let field = search_field();
            if !field.is_invalid() {
                unsafe {
                    let _ = SetFocus(field);
                }
            }
        }
        Press::Nothing => {}
    }
}

/// The tray's window and Search's, for `tray_access.rs`.
pub(super) fn windows() -> (HWND, HWND) {
    (hwnd(), search_field())
}

fn visible_width() -> f64 {
    let tray = hwnd();
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(tray, &mut client);
    }
    client.right as f64 / draw::dpi_of(tray) as f64
}

/// Scrolls the row at once (a wheel, a resize), ending a glide.
fn scroll_to(offset: f64) {
    GLIDE.set(None);
    set_offset(offset);
}

/// Glides the row to `offset` over 0.25 s (`TrayScroll.scroll(animated:)`);
/// at once when Windows' animations are off.
fn glide_to(offset: f64) {
    let from = SHOWN.with(|v| v.borrow().offset);
    if !super::motion::enabled() || (offset - from).abs() < 0.5 {
        return scroll_to(offset);
    }
    GLIDE.set(Some((from, offset, Instant::now())));
    unsafe {
        SetTimer(hwnd(), TIMER_GLIDE, 8, None);
    }
}

fn glide_step(hwnd: HWND) {
    let Some((from, to, since)) = GLIDE.get() else {
        unsafe {
            let _ = KillTimer(hwnd, TIMER_GLIDE);
        }
        return;
    };
    let elapsed = since.elapsed().as_secs_f64() * 1000.0;
    set_offset(rules::glide(from, to, elapsed));
    if elapsed >= rules::GLIDE_MS {
        GLIDE.set(None);
        unsafe {
            let _ = KillTimer(hwnd, TIMER_GLIDE);
        }
    }
}

/// The device is made, or made again: the tray repaints through it.
pub fn surfaces_ready() {
    let tray = hwnd();
    if !tray.is_invalid() {
        PICTURES.with(|p| p.borrow_mut().clear());
        unsafe {
            let _ = InvalidateRect(tray, None, false);
        }
    }
}

fn set_offset(offset: f64) {
    let changed = SHOWN.with(|v| {
        let mut v = v.borrow_mut();
        let offset = rules::clamp(offset, visible_width(), rules::content_width(&v.sections));
        std::mem::replace(&mut v.offset, offset) != offset
    });
    if changed {
        HOT_TILE.set(None);
        unsafe {
            let _ = InvalidateRect(hwnd(), None, false);
        }
    }
}

/// Return in Search tries the first match (with nothing typed it keeps);
/// Esc clears it, and in an empty Search goes back, as anywhere in the tray.
extern "system" fn search_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_GETDLGCODE if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_RETURN | VK_ESCAPE) => LRESULT(DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                post(|app: &mut App| app.model.tray_submit());
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                post(|app: &mut App| app.model.tray_cancel_search());
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            WM_SETFOCUS | WM_KILLFOCUS => {
                // The rule under it becomes the pencil while typing.
                let _ = InvalidateRect(GetParent(hwnd).unwrap_or_default(), None, false);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point = || {
        let s = draw::dpi_of(hwnd);
        ((lparam.0 & 0xffff) as i16 as f32 / s, ((lparam.0 >> 16) & 0xffff) as i16 as f32 / s)
    };
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                SURFACE.with(|s| s.borrow_mut().release());
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_GETOBJECT if lparam.0 as i32 == OBJID_CLIENT.0 => super::tray_access::object(hwnd, wparam),
            super::tray_access::WM_APP_PRESS => {
                super::tray_access::press(wparam.0);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_GLIDE => {
                glide_step(hwnd);
                LRESULT(0)
            }
            WM_SIZE => {
                // The row keeps its place, held within the new width.
                let offset = SHOWN.with(|v| v.borrow().offset);
                scroll_to(offset);
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = point();
                set_hot(hwnd, part_at(x, y), tile_at(x, y));
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                set_hot(hwnd, None, None);
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let (x, y) = point();
                click(x, y);
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                // A wheel scrolls the row sideways, as the Mac's does.
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                let (offset, content) = SHOWN.with(|v| {
                    let v = v.borrow();
                    (v.offset, rules::content_width(&v.sections))
                });
                scroll_to(rules::wheel(offset, delta, msg == WM_MOUSEHWHEEL, visible_width(), content));
                LRESULT(0)
            }
            WM_COMMAND => {
                let code = ((wparam.0 >> 16) & 0xffff) as u32;
                if (wparam.0 & 0xffff) as u16 == SEARCH_ID && code == EN_CHANGE {
                    let text = draw::window_text(HWND(lparam.0 as *mut _));
                    with_app(|app| app.model.set_tray_query(&text));
                }
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                // Search on the film: `paper` words.
                let dc = HDC(wparam.0 as *mut _);
                let t = theme::tokens();
                SetTextColor(dc, COLORREF(theme::gdi(t.film.paper)));
                SetBkColor(dc, COLORREF(theme::gdi(t.hex.film)));
                LRESULT(theme::brush(theme::gdi(t.hex.film)).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
