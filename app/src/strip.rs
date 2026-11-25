//! The film strip under the canvas (`windows/src/ui/strip.rs`,
//! `PhotoBrowser.swift`): the folder's photographs as the library shows them
//! (the filter's, a RAW+JPEG pair as one, duplicates), each frame at its
//! photograph's own proportions on black film, the open one whole with the
//! pencil under it, the rest dimmed; the hand-drawn stars, the pick's box
//! and the reject's cross from `negs-marks`. 158 DIP with the status line's
//! 28 under it: the tray's 186, so the photograph keeps its size when the
//! tray opens (which hides the strip). F6 shows and hides it.
//!
//! Virtualised: `StripLayout::indices` picks the frames in view (plus
//! `STRIP_MARGIN`), all painted by one GPUI `canvas` (no element per
//! frame), so a scroll costs a few dozen sprites and paths. Thumbnails come
//! from `thumbs.rs`. Before the library opens (the catalog is still coming)
//! the folder is listed once.

use std::cell::Cell as StdCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, Font, FontWeight, Hsla, InteractiveElement, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, TextRun, Window, canvas, div, fill, font, point, px, size,
};
use negs_app::model::{Model, dirty, is_photo};
use negs_app::sheet_rules::{self as rules, StripLayout, ThumbSize};
use negs_library::{Flag, Item, Summary};

use crate::NegsApp;
use crate::marks_gpui::{BoxInk, paint_box, paint_cross, paint_dot, paint_pencil, paint_star};
use crate::marks_raster::{MarkSprites, Sprite};
use crate::theme_gpui::{GpuiTheme, hex};
use crate::thumbs::{self, Thumb, Thumbs, Want};
use crate::tile_cache::cover_bounds;

const STAR: f32 = negs_marks::STAR_SIZE as f32;
const STAR_GAP: f32 = negs_marks::STAR_GAP as f32;

/// One frame of the strip (and cell of the grid).
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub want: Want,
    pub name: String,
    pub summary: Summary,
    /// The open photograph.
    pub open: bool,
    /// In the library's selection, for Paste Edits and the rest.
    pub selected: bool,
    /// A RAW with its camera JPEG: the "+J" badge.
    pub pair: bool,
}

pub struct StripState {
    /// Before the library: the folder listed once, and its photographs.
    listed: Option<PathBuf>,
    files: Vec<PathBuf>,
    scan: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
    pub cells: Vec<Cell>,
    pub layout: StripLayout,
    /// DIPs from the row's left edge.
    pub scroll: f64,
    glide: Option<(f64, f64, Instant)>,
    hover: Option<usize>,
    pressed: Option<usize>,
    open: Option<Item>,
    /// The strip's bounds in the window as last painted (the canvas sets it).
    pub bounds: Rc<StdCell<Bounds<Pixels>>>,
    /// `Thumbs::revision` when last painted, and when last synced.
    pub painted: u64,
    pub synced: u64,
}

impl Default for StripState {
    fn default() -> Self {
        Self {
            listed: None,
            files: vec![],
            scan: None,
            cells: vec![],
            layout: StripLayout::default(),
            scroll: 0.0,
            glide: None,
            hover: None,
            pressed: None,
            open: None,
            bounds: Rc::new(StdCell::new(Bounds::default())),
            painted: 0,
            synced: u64::MAX,
        }
    }
}

impl StripState {
    /// The width the strip shows, DIPs (an estimate before its first paint).
    pub fn visible_width(&self) -> f64 {
        f32::from(self.bounds.get().size.width) as f64
    }

    pub fn open_index(&self) -> Option<usize> {
        self.cells.iter().position(|c| c.open)
    }

    /// A frame's rect in the window (DIPs), as painted now.
    pub fn frame_in_window(&self, index: usize) -> Option<Bounds<Pixels>> {
        let b = self.bounds.get();
        if index >= self.layout.count() || f32::from(b.size.width) <= 0.0 {
            return None;
        }
        let x = f32::from(b.origin.x) + (self.layout.xs[index] - self.scroll) as f32;
        let y = f32::from(b.origin.y) + rules::STRIP_TOP as f32;
        Some(Bounds { origin: point(px(x), px(y)), size: size(px(self.layout.widths[index] as f32), px(rules::PICTURE as f32)) })
    }

    /// The frames in view (no margin), for flights and the bench.
    pub fn on_screen(&self) -> std::ops::Range<usize> {
        let w = self.visible_width();
        self.layout.indices(self.scroll, self.scroll + w)
    }

    pub fn gliding(&self) -> bool {
        self.glide.is_some()
    }

    /// Scrolls to `x` at once (the bench's sweeps).
    pub fn set_scroll(&mut self, x: f64) {
        self.glide = None;
        self.scroll = x;
        self.clamp();
    }

    fn clamp(&mut self) {
        let visible = self.visible_width();
        self.scroll = self.scroll.max(0.0).min((self.layout.width - visible).max(0.0));
    }
}

/// The strip shows: a photograph open, the strip on (F6), no tray open.
pub fn shown(app: &NegsApp) -> bool {
    app.model.prefs.filmstrip_visible && app.model.source.is_some() && app.model.tray_shown().is_none()
}

/// Explorer's / Finder's order: `IMG_2` before `IMG_10`, case folded.
pub fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            (Some(p), Some(q)) => {
                let order = p.to_lowercase().cmp(q.to_lowercase());
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn scan(folder: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(folder) else { return vec![] };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| folder.join(e.file_name()))
        .filter(|p| is_photo(p))
        .collect();
    files.sort_by(|a, b| natural_order(&a.file_name().unwrap_or_default().to_string_lossy(), &b.file_name().unwrap_or_default().to_string_lossy()));
    files
}

/// The cells the model shows: the library's, else the folder's files.
pub fn cells(model: &Model, files: &[PathBuf]) -> Vec<Cell> {
    match &model.shelf.library {
        Some(library) => {
            let open = library.open_item().cloned();
            let unedited = thumbs::unedited_look(model);
            model
                .shown_items()
                .into_iter()
                .map(|item| Cell {
                    want: thumbs::want(model, &item, unedited.as_deref()),
                    name: library.name(&item),
                    summary: library.summary(&item),
                    open: open.as_ref() == Some(&item),
                    selected: model.shelf.selection.contains(&item),
                    pair: library.companion(&item.photo).is_some(),
                })
                .collect()
        }
        None => files
            .iter()
            .map(|path| Cell {
                want: Want { item: Item::photo(path), key: None, loose: None },
                name: negs_app::model::file_name(path),
                summary: Summary::default(),
                open: model.source.as_ref().is_some_and(|s| negs_app::session::same_path(s, path)),
                selected: false,
                pair: false,
            })
            .collect(),
    }
}

/// Follows the model: what it lists, the open frame, the selection and the
/// marks; asks for the thumbnails in view and around it.
pub fn sync(app: &mut NegsApp) {
    if !shown(app) {
        return;
    }
    let folder = app.model.source.as_ref().and_then(|s| s.parent()).map(Path::to_path_buf);
    if app.model.shelf.library.is_none() && folder.is_some() && app.strip.listed != folder {
        app.strip.listed = folder.clone();
        app.strip.files.clear();
        let folder = folder.unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan(&folder));
            negs_app::host::post(|m| m.dirty |= dirty::STRIP);
        });
        app.strip.scan = Some(rx);
    }
    if let Some(rx) = &app.strip.scan {
        if let Ok(files) = rx.try_recv() {
            app.strip.files = files;
            app.strip.scan = None;
        }
    }
    relist(app);
    request(app);
    app.strip.synced = app.thumbs.revision;
}

fn relist(app: &mut NegsApp) {
    let cells = cells(&app.model, &app.strip.files);
    let widths: Vec<f64> = cells.iter().map(|c| rules::frame_width(app.thumbs.ratio(&c.want))).collect();
    let strip = &mut app.strip;
    let layout = StripLayout::new(widths);
    let visible = strip.visible_width();
    let open_index = cells.iter().position(|c| c.open);
    let open_item = open_index.map(|i| cells[i].want.item.clone());
    let same_list = strip.cells.len() == cells.len() && strip.cells.iter().zip(&cells).all(|(a, b)| a.want.item == b.want.item);
    if open_item != strip.open {
        // Another photograph opened: it glides to the middle (at once the
        // first time, or when it's far away).
        let target = open_index.map_or(0.0, |i| rules::centred(layout.middle(i), visible, layout.width));
        let near = strip.open.is_some() && (target - strip.scroll).abs() < visible * 1.5 && visible > 0.0;
        strip.glide = near.then(|| (strip.scroll, target, Instant::now()));
        if !near {
            strip.scroll = target;
        }
        strip.open = open_item;
    } else if same_list && strip.layout != layout && visible > 0.0 {
        // Shapes were read: the frame in the middle stays where it is.
        let anchor = open_index.or_else(|| strip.layout.index_at(strip.scroll + visible / 2.0)).unwrap_or(0);
        if anchor < layout.count() {
            strip.scroll = rules::hold(&strip.layout, &layout, anchor, strip.scroll, visible);
        }
    }
    strip.cells = cells;
    strip.layout = layout;
    if strip.glide.is_none() {
        strip.clamp();
    }
}

/// Asks for the thumbnails in view now (the bench's scrolling).
pub fn request_now(app: &mut NegsApp) {
    request(app);
}

/// The thumbnails in view first, then ±32 around the middle; the pump
/// develops what's missing.
fn request(app: &mut NegsApp) {
    let strip = &app.strip;
    let range = strip.on_screen();
    let on_screen: Vec<usize> = range.clone().collect();
    let open = strip.open_index();
    let ahead = rules::prefetch_order(strip.cells.len(), &on_screen, open);
    let wants: Vec<&Want> = on_screen.iter().chain(&ahead).map(|&i| &strip.cells[i].want).collect();
    // Launch's develop first: reading previews beside it cost it 3 %.
    if app.model.first_render_shown || app.model.render_failed || app.model.shelf.library.is_some() {
        app.thumbs.read(&wants, ThumbSize::Strip);
    }
    let all: Vec<Want> = strip.cells.iter().map(|c| c.want.clone()).collect();
    app.thumbs.read_headers(&all, open.unwrap_or_else(|| rules::thumbnail_centre(&on_screen, None)));
    app.thumbs.pump(&app.model, &all, range, open);
}

/// Scrolls the open frame to the middle at once.
pub fn centre_open(app: &mut NegsApp) {
    let strip = &mut app.strip;
    if let Some(i) = strip.open_index() {
        let visible = strip.visible_width();
        strip.set_scroll(rules::centred(strip.layout.middle(i), visible, strip.layout.width));
    }
}

/// Brings the strip up to date at once, a glide to its end: under a folding
/// sheet, whose flyers land on its frames.
pub fn settle(app: &mut NegsApp) {
    sync(app);
    if let Some((_, to, _)) = app.strip.glide.take() {
        app.strip.scroll = to;
        app.strip.clamp();
    }
}

fn step_glide(strip: &mut StripState) -> bool {
    let Some((from, to, started)) = strip.glide else { return false };
    let t = started.elapsed().as_secs_f64() * 1000.0 / rules::GLIDE_MS;
    strip.scroll = from + (to - from) * rules::smoothstep(t);
    if t >= 1.0 {
        strip.glide = None;
        strip.clamp();
        return false;
    }
    true
}

pub fn scroll_by(app: &mut NegsApp, delta: f64) {
    app.strip.glide = None;
    app.strip.scroll += delta;
    app.strip.clamp();
    request(app);
}

/// ← and → in the photograph view: the previous or next photograph.
pub fn open_neighbour(app: &mut NegsApp, step: isize) {
    if app.model.cropping() {
        return;
    }
    if app.model.shelf.library.is_some() {
        return app.model.step_photo(step);
    }
    let Some(index) = app.strip.open_index() else { return };
    let next = index as isize + step;
    if next >= 0 && (next as usize) < app.strip.cells.len() {
        let path = app.strip.cells[next as usize].want.item.photo.clone();
        app.model.open(&path);
    }
}

/// A click (`EditorModel.click`): ⌘/Ctrl adds or takes it from the
/// selection, Shift selects up to it, and a plain click selects it and, in
/// the strip, opens it.
pub fn click(model: &mut Model, item: &Item, modifiers: &Modifiers, open: bool) {
    let toggle = if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control };
    let order = model.shown_items();
    let selection = &mut model.shelf.selection;
    if toggle {
        selection.toggle(item, Some(&order));
    } else if modifiers.shift {
        selection.extend_to(item, &order);
    } else {
        selection.select(item);
        if open {
            model.open_item(item.clone());
        }
    }
    model.dirty |= dirty::STRIP | dirty::CHROME;
}

// MARK: Painting

/// The inks a frame needs, resolved once per paint.
#[derive(Clone, Copy)]
pub struct Inks {
    pub film: Hsla,
    pub blank: Hsla,
    pub paper: Hsla,
    pub label: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
    pub plain: bool,
}

impl Inks {
    pub fn of(theme: &GpuiTheme) -> Self {
        let f = theme.tokens.film;
        Self {
            film: hex(f.film),
            blank: hex(f.panel),
            paper: hex(f.paper),
            label: hex(f.label),
            accent: hex(f.accent),
            selection: theme.accent,
            plain: theme.tokens.plain,
        }
    }
}

/// A frame's marks as cached sprites (`marks_raster`); a `None` where one's
/// wanted is drawn as paths this frame (the sprite budget ran out).
#[derive(Clone, Default)]
pub struct FrameSprites {
    pub stars: Option<Arc<Sprite>>,
    pub cross: Option<Arc<Sprite>>,
    pub pick: Option<Arc<Sprite>>,
    pub selection: Option<Arc<Sprite>>,
}

/// What one frame paints, taken from its cell for a paint closure.
#[derive(Clone)]
pub struct FramePaint {
    pub thumb: Option<Arc<Thumb>>,
    pub name: SharedString,
    pub rating: u8,
    pub flag: Option<Flag>,
    pub edited: bool,
    pub pair: bool,
    pub open: bool,
    pub selected: bool,
    pub sprites: FrameSprites,
}

impl FramePaint {
    pub fn of(cell: &Cell, thumbs: &Thumbs, size: ThumbSize) -> Self {
        let thumb = thumbs.shown(&cell.want, size).or_else(|| (size != ThumbSize::Strip).then(|| thumbs.shown(&cell.want, ThumbSize::Strip)).flatten());
        Self {
            thumb,
            name: cell.name.clone().into(),
            rating: cell.summary.rating,
            flag: cell.summary.flag,
            edited: cell.summary.edited,
            pair: cell.pair,
            open: cell.open,
            selected: cell.selected,
            sprites: FrameSprites::default(),
        }
    }

    /// Looks up (or makes, within the frame's budget) the sprites for a
    /// `w` × `h` picture. `grid`: the grid's boxes, else the strip's.
    pub fn with_sprites(mut self, sprites: &mut MarkSprites, w: f32, h: f32, scale: f32, inks: &Inks, grid: bool) -> Self {
        if self.rating > 0 {
            self.sprites.stars = sprites.stars(&self.name, self.rating.min(5), scale, inks.paper);
        }
        match self.flag {
            Some(Flag::Reject) => self.sprites.cross = sprites.cross(&self.name, w, h, scale, inks.paper),
            Some(Flag::Pick) => {
                let kind = if grid { BoxInk::Grid } else { BoxInk::Strip };
                self.sprites.pick = sprites.pick_box(kind, &self.name, w, h, scale, inks.paper);
            }
            None => {}
        }
        if grid && self.selected {
            self.sprites.selection = sprites.pick_box(BoxInk::Selection, &self.name, w, h, scale, inks.selection);
        }
        self
    }
}

/// A thumbnail filling `r` (cropped to fill, as the views do), faded to
/// `opacity` over `under` (the ground it lies on), else bare film.
pub fn paint_picture(window: &mut Window, thumb: Option<&Arc<Thumb>>, r: Bounds<Pixels>, opacity: f32, under: Hsla, blank: Hsla) -> bool {
    match thumb {
        Some(t) => {
            window.with_content_mask(Some(ContentMask { bounds: r }), |window| {
                let _ = window.paint_image(cover_bounds(r, t.width, t.height), Default::default(), t.image().clone(), 0, false);
                if opacity < 0.999 {
                    window.paint_quad(fill(r, under.opacity(1.0 - opacity)));
                }
            });
            true
        }
        None => {
            window.paint_quad(fill(r, blank.opacity(opacity)));
            false
        }
    }
}

/// `PhotoMarks`: the reject's cross over the picture, the stars at its foot
/// with their halo, the edited dot, and the "+J" badge.
pub fn paint_marks(window: &mut Window, cx: &mut App, f: &FramePaint, r: Bounds<Pixels>, inks: &Inks, ui: &SharedString, in_strip: bool) {
    let (x, y, w, h) = (f32::from(r.origin.x), f32::from(r.origin.y), f32::from(r.size.width), f32::from(r.size.height));
    if f.flag == Some(Flag::Reject) {
        match &f.sprites.cross {
            Some(s) => s.paint(window, r.origin),
            None => paint_cross(window, r, &f.name, inks.paper, true),
        }
    }
    if f.rating > 0 {
        let first = point(px(x + 10.0 + STAR / 2.0), px(y + h - 10.0 - STAR / 2.0));
        match &f.sprites.stars {
            Some(s) => s.paint(window, first),
            None => {
                for i in 0..f.rating.min(5) as usize {
                    paint_star(window, &f.name, i, point(first.x + px(i as f32 * (STAR + STAR_GAP)), first.y), inks.paper, true);
                }
            }
        }
    }
    if f.edited {
        paint_dot(window, point(px(x + w - 11.0 - 3.5), px(y + 11.0 + 3.5)), 3.5, inks.accent);
    }
    if f.pair {
        let word: SharedString = if in_strip { "+J".into() } else { "+JPG".into() };
        let run = TextRun {
            len: word.len(),
            font: Font { weight: FontWeight::SEMIBOLD, ..font(ui.clone()) },
            color: inks.paper,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(word, px(10.0), &[run], None);
        let tw = f32::from(line.width);
        let inset = if in_strip { 5.0 } else { 7.0 };
        let badge = Bounds { origin: point(px(x + w - inset - tw - 8.0), px(y + h - inset - 15.0)), size: size(px(tw + 8.0), px(15.0)) };
        window.paint_quad(fill(badge, gpui::black().opacity(0.6)).corner_radii(px(3.0)));
        let _ = line.paint(point(badge.origin.x + px(4.0), badge.origin.y + px(1.5)), px(12.0), window, cx);
    }
}

/// A frame's pick box (or, `selection`, its selection box): a sprite, or
/// paths when faded (a flight's) or past the sprite budget.
pub fn paint_frame_box(window: &mut Window, f: &FramePaint, r: Bounds<Pixels>, kind: BoxInk, color: Hsla, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let sprite = if kind == BoxInk::Selection { &f.sprites.selection } else { &f.sprites.pick };
    match sprite {
        Some(s) if alpha >= 0.999 => s.paint(window, r.origin),
        _ => paint_box(window, kind, r, &f.name, color.opacity(color.a * alpha)),
    }
}

/// One strip frame: picture, dimming, marks, and the open frame's pencil
/// or the selection's plain rule under it. Pick boxes are drawn after all
/// frames (they reach over the neighbours).
pub fn paint_strip_frame(window: &mut Window, cx: &mut App, f: &FramePaint, r: Bounds<Pixels>, opacity: f32, inks: &Inks, ui: &SharedString) {
    paint_picture(window, f.thumb.as_ref(), r, opacity, inks.film, inks.blank);
    paint_marks(window, cx, f, r, inks, ui, true);
    let under_y = r.origin.y + r.size.height + px(rules::PENCIL_DROP as f32);
    if f.open {
        // The 6 DIP box under the frame (`PENCIL_WIDTH + 2`), stroked 2.1 wide.
        let pencil = Bounds { origin: point(r.origin.x, under_y), size: size(r.size.width, px(rules::PENCIL_WIDTH as f32 + 2.0)) };
        paint_pencil(window, pencil, inks.plain, negs_marks::PENCIL_WIDTH as f32, inks.accent);
    } else if f.selected {
        window.paint_quad(fill(Bounds { origin: point(r.origin.x, under_y), size: size(r.size.width, px(1.5)) }, inks.label));
    }
}

/// The strip's opacity for a frame (`rules::strip_opacity`).
pub fn strip_opacity(f: &FramePaint, hovered: bool, pressed: bool) -> f32 {
    rules::strip_opacity(f.open, f.selected, f.flag == Some(Flag::Reject), hovered, pressed) as f32
}

pub fn render_strip(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if !shown(app) {
        return None;
    }
    if step_glide(&mut app.strip) {
        window.request_animation_frame();
    }
    if app.strip.synced != app.thumbs.revision || app.model.dirty != 0 || app.strip.cells.is_empty() {
        sync(app);
    }
    let width = app.strip.visible_width();
    // Before the first paint, the window's width (the canvas column is at most that).
    let width = if width > 0.0 { width } else { f32::from(window.viewport_size().width) as f64 };
    let scale = window.scale_factor();
    let inks = Inks::of(&app.theme);
    let NegsApp { strip, thumbs, sprites, .. } = app;
    let range = strip.layout.indices(strip.scroll - rules::STRIP_MARGIN, strip.scroll + width + rules::STRIP_MARGIN);
    let frames: Vec<(f32, f32, FramePaint, f32)> = range
        .map(|i| {
            let cell = &strip.cells[i];
            let w = strip.layout.widths[i] as f32;
            let f = FramePaint::of(cell, thumbs, ThumbSize::Strip).with_sprites(sprites, w, rules::PICTURE as f32, scale, &inks, false);
            let opacity = strip_opacity(&f, strip.hover == Some(i), strip.pressed == Some(i));
            ((strip.layout.xs[i] - strip.scroll) as f32, w, f, opacity)
        })
        .collect();
    app.strip.painted = app.thumbs.revision;
    let ui = app.theme.ui_family.clone();
    let bounds_cell = app.strip.bounds.clone();

    let picture = canvas(
        move |bounds, window, _cx| {
            let old = bounds_cell.get();
            if (f32::from(old.size.width) - f32::from(bounds.size.width)).abs() > 0.5 || old.origin != bounds.origin {
                bounds_cell.set(bounds);
                window.refresh();
            }
            bounds
        },
        move |bounds: Bounds<Pixels>, _: Bounds<Pixels>, window, cx| {
            window.paint_quad(fill(bounds, inks.film));
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let rect = |x: f32, w: f32| Bounds {
                    origin: point(bounds.origin.x + px(x), bounds.origin.y + px(rules::STRIP_TOP as f32)),
                    size: size(px(w), px(rules::PICTURE as f32)),
                };
                for (x, w, f, opacity) in &frames {
                    paint_strip_frame(window, cx, f, rect(*x, *w), *opacity, &inks, &ui);
                }
                // A pick's box reaches past its frame, over the gap and the
                // frames beside it, so the boxes are drawn above every frame.
                for (x, w, f, _) in &frames {
                    if f.flag == Some(Flag::Pick) {
                        paint_frame_box(window, f, rect(*x, *w), BoxInk::Strip, inks.paper, 1.0);
                    }
                }
            });
        },
    )
    .size_full();

    Some(
        div()
            .id("filmstrip")
            .w_full()
            .h(px(rules::STRIP as f32))
            .flex_shrink_0()
            .bg(app.theme.film)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                let hover = index_at(&this.strip, event.position);
                if this.strip.hover != hover {
                    this.strip.hover = hover;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovering: &bool, _window, cx| {
                if !*hovering && (this.strip.hover.is_some() || this.strip.pressed.is_some()) {
                    this.strip.hover = None;
                    this.strip.pressed = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.close_menu();
                    window.focus(&this.focus_handle.clone());
                    if this.model.cropping() {
                        return;
                    }
                    this.strip.pressed = index_at(&this.strip, event.position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    let pressed = this.strip.pressed.take();
                    cx.notify();
                    let Some(index) = pressed.filter(|&i| index_at(&this.strip, event.position) == Some(i)) else { return };
                    let cell = this.strip.cells[index].clone();
                    if this.model.shelf.library.is_some() {
                        click(&mut this.model, &cell.want.item, &event.modifiers, !cell.open);
                    } else if !cell.open {
                        this.model.open(&cell.want.item.photo);
                    }
                    this.after_model_update(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    if this.model.cropping() || this.model.shelf.library.is_none() {
                        return;
                    }
                    let Some(index) = index_at(&this.strip, event.position) else { return };
                    let item = this.strip.cells[index].want.item.clone();
                    this.open_library_menu(item, event.position, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let (dx, dy) = match event.delta {
                    ScrollDelta::Pixels(p) => (f32::from(p.x) as f64, f32::from(p.y) as f64),
                    ScrollDelta::Lines(l) => (l.x as f64 * 80.0, l.y as f64 * 80.0),
                };
                // Sideways first (a touchpad's), else the wheel's turn.
                let delta = if dx.abs() > dy.abs() { -dx } else { -dy };
                scroll_by(this, delta);
                let _ = window;
                cx.notify();
            }))
            .child(picture)
            .into_any_element(),
    )
}

/// The frame under a window point, if it's on a picture.
pub fn index_at(strip: &StripState, at: Point<Pixels>) -> Option<usize> {
    let b = strip.bounds.get();
    let (x, y) = (f32::from(at.x - b.origin.x) as f64, f32::from(at.y - b.origin.y) as f64);
    (y >= rules::STRIP_TOP && y <= rules::STRIP_TOP + rules::PICTURE && x >= 0.0 && x < f32::from(b.size.width) as f64)
        .then(|| strip.layout.hit(x + strip.scroll))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_matches_explorer_and_finder() {
        let mut names = vec!["IMG_10.jpg", "img_2.JPG", "IMG_1.jpg", "DSC_0100.NEF", "DSC_099.NEF", "a.jpg"];
        names.sort_by(|a, b| natural_order(a, b));
        assert_eq!(names, vec!["a.jpg", "DSC_099.NEF", "DSC_0100.NEF", "IMG_1.jpg", "img_2.JPG", "IMG_10.jpg"]);
    }

    #[test]
    fn hit_testing_follows_scroll_and_ignores_the_rebate() {
        let mut strip = StripState::default();
        strip.layout = StripLayout::new(vec![100.0, 200.0, 150.0]);
        strip.bounds.set(Bounds { origin: point(px(10.0), px(500.0)), size: size(px(300.0), px(rules::STRIP as f32)) });
        let y = px(500.0 + rules::STRIP_TOP as f32 + 20.0);
        assert_eq!(index_at(&strip, point(px(15.0), y)), Some(0));
        assert_eq!(index_at(&strip, point(px(10.0 + 150.0), y)), Some(1));
        assert_eq!(index_at(&strip, point(px(15.0), px(500.0 + rules::STRIP as f32 - 2.0))), None, "the rebate under the frames");
        strip.scroll = 200.0;
        strip.clamp();
        assert_eq!(index_at(&strip, point(px(10.0 + 150.0), y)), Some(2));
        let r = strip.frame_in_window(2).unwrap();
        assert!((f32::from(r.origin.x) - (10.0 + strip.layout.xs[2] as f32 - strip.scroll as f32)).abs() < 0.01);
        assert!(strip.on_screen().contains(&2));
    }

    #[test]
    fn glide_eases_to_its_target() {
        let mut strip = StripState::default();
        strip.layout = StripLayout::new(vec![200.0; 20]);
        strip.bounds.set(Bounds { origin: point(px(0.0), px(0.0)), size: size(px(600.0), px(158.0)) });
        strip.glide = Some((0.0, 1000.0, Instant::now() - std::time::Duration::from_millis(rules::GLIDE_MS as u64 / 2)));
        assert!(step_glide(&mut strip));
        assert!(strip.scroll > 300.0 && strip.scroll < 700.0, "halfway along the smoothstep: {}", strip.scroll);
        strip.glide = Some((0.0, 1000.0, Instant::now() - std::time::Duration::from_millis(rules::GLIDE_MS as u64 + 10)));
        assert!(!step_glide(&mut strip));
        assert!((strip.scroll - 1000.0).abs() < 1e-6);
    }
}
