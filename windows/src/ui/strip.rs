//! The film strip under the canvas (`PhotoBrowser.swift`): the folder's
//! photographs as the library shows them (the filter's, a RAW+JPEG pair as
//! one, duplicates), each frame at its photograph's own proportions on
//! black film, the open one whole with the pencil under it, the rest
//! dimmed; the hand-drawn stars, the pick's box and the reject's cross
//! from `windows/marks`, their halo a real blur. 158 DIP with the status
//! line's 28 under it: the tray's 186, so the photograph keeps its size
//! when the tray opens. The leader, a tongue at its right end over the
//! canvas, hides and shows it.
//!
//! Painted by Direct2D on the GPU (`surface.rs`), only the frames in view;
//! it repaints when what it shows changes, never for a slider. Thumbnails
//! come from `thumbs.rs`. The library's folder watcher keeps it current;
//! before the library opens (the catalog is still coming) the folder is
//! listed once.

use std::path::{Path, PathBuf};
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use negs_library::{Flag, Item, Summary};

use super::d2d::{BoxInk, Ink, Painter, RectF};
use super::surface::Surface;
use super::theme::{self, Tokens};
use super::thumbs::{self, Thumbs, Want};
use super::{App, library, post, with_app};
use crate::model::{dirty, is_photo};
use crate::platform::{pcwstr, wide};
use crate::sheet_rules::{self as rules, StripLayout, ThumbSize};

const TIMER_GLIDE: usize = 1;
const STAR: f32 = negs_marks::STAR_SIZE as f32;
const STAR_GAP: f32 = negs_marks::STAR_GAP as f32;

/// One frame of the strip.
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

#[derive(Default)]
pub struct Strip {
    /// Before the library: the folder listed once, and its photographs.
    listed: Option<PathBuf>,
    files: Vec<PathBuf>,
    pub cells: Vec<Cell>,
    pub layout: StripLayout,
    /// DIPs from the row's left edge.
    pub scroll: f64,
    glide: Option<(f64, f64, Instant)>,
    hover: Option<usize>,
    pressed: Option<usize>,
    open: Option<Item>,
    surface: Surface,
    /// `Thumbs::revision` when last painted.
    painted: u64,
    leader: HWND,
    leader_hover: bool,
    leader_shown: bool,
    leader_place: (i32, i32),
}

pub fn register() {
    unsafe {
        for (name, proc) in [(w!("NegsStrip"), Some(proc_ as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT)), (w!("NegsStripLeader"), Some(leader_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT))] {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: proc,
                hInstance: super::instance(),
                lpszClassName: name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    }
}

/// Explorer's order: `IMG_2` before `IMG_10`.
fn natural_order(a: &Path, b: &Path) -> std::cmp::Ordering {
    let (x, y) = (wide(a.file_name().unwrap_or_default()), wide(b.file_name().unwrap_or_default()));
    unsafe { StrCmpLogicalW(pcwstr(&x), pcwstr(&y)).cmp(&0) }
}

fn scan(folder: &Path) -> Vec<PathBuf> {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let Ok(read) = std::fs::read_dir(folder) else { return vec![] };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file() && m.file_attributes() & (HIDDEN | SYSTEM) == 0))
        .map(|e| folder.join(e.file_name()))
        .filter(|p| is_photo(p))
        .collect();
    files.sort_by(|a, b| natural_order(a, b));
    files
}

fn scale(hwnd: HWND) -> f64 {
    unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0
}

fn visible_width(hwnd: HWND) -> f64 {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    r.right as f64 / scale(hwnd)
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// Follows the model: what it lists, the open frame, the selection and
/// the marks; asks for the thumbnails in view and around it. Repaints only
/// when one of those changed.
pub fn sync(app: &mut App) {
    let visible = app.model.prefs.filmstrip_visible && app.model.source.is_some();
    unsafe {
        let _ = EnableWindow(app.strip, !app.model.cropping());
    }
    sync_leader(app);
    if !visible {
        app.strip_view.surface.release();
        return;
    }
    // Before the library opens: the folder, listed once on a thread.
    let folder = app.model.source.as_ref().and_then(|s| s.parent()).map(Path::to_path_buf);
    if app.model.shelf.library.is_none() && folder.is_some() && app.strip_view.listed != folder {
        app.strip_view.listed = folder.clone();
        app.strip_view.files.clear();
        let folder = folder.unwrap();
        std::thread::spawn(move || {
            let files = scan(&folder);
            post(move |app: &mut App| {
                if app.strip_view.listed.as_deref() == Some(folder.as_path()) {
                    app.strip_view.files = files;
                    app.model.dirty |= dirty::STRIP;
                }
            });
        });
    }
    let changed = relist(app);
    request(app);
    if changed || app.strip_view.painted != app.thumbs.revision {
        invalidate(app.strip);
    }
}

/// The cells the model shows now; true if they or their layout changed.
fn relist(app: &mut App) -> bool {
    let model = &app.model;
    let source = model.source.clone();
    let cells: Vec<Cell> = match &model.shelf.library {
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
        None => app
            .strip_view
            .files
            .iter()
            .map(|path| Cell {
                want: Want { item: Item::photo(path), key: None, loose: None },
                name: crate::model::file_name(path),
                summary: Summary::default(),
                open: source.as_ref().is_some_and(|s| crate::session::same_path(s, path)),
                selected: false,
                pair: false,
            })
            .collect(),
    };
    let widths: Vec<f64> = cells.iter().map(|c| rules::frame_width(app.thumbs.ratio(&c.want))).collect();
    let strip = &mut app.strip_view;
    let layout = StripLayout::new(widths);
    let visible = visible_width(app.strip);
    let open_index = cells.iter().position(|c| c.open);
    let open_item = open_index.map(|i| cells[i].want.item.clone());
    let same_list = strip.cells.len() == cells.len() && strip.cells.iter().zip(&cells).all(|(a, b)| a.want.item == b.want.item);
    let mut changed = strip.cells != cells || strip.layout != layout;
    if open_item != strip.open {
        // Another photograph opened: it glides to the middle (at once the
        // first time, or when it's far away).
        let target = open_index.map_or(0.0, |i| rules::centred(layout.middle(i), visible, layout.width));
        let near = strip.open.is_some() && (target - strip.scroll).abs() < visible * 1.5;
        strip.glide = None;
        if near {
            strip.glide = Some((strip.scroll, target, Instant::now()));
            unsafe {
                SetTimer(app.strip, TIMER_GLIDE, 15, None);
            }
        } else {
            strip.scroll = target;
        }
        strip.open = open_item;
        changed = true;
    } else if same_list && strip.layout != layout {
        // Shapes were read: the frame in the middle stays where it is.
        let anchor = open_index.or_else(|| strip.layout.index_at(strip.scroll + visible / 2.0)).unwrap_or(0);
        if anchor < layout.count() {
            strip.scroll = rules::hold(&strip.layout, &layout, anchor, strip.scroll, visible);
        }
    }
    strip.cells = cells;
    strip.layout = layout;
    clamp(strip, visible);
    changed
}

fn clamp(strip: &mut Strip, visible: f64) {
    strip.scroll = strip.scroll.max(0.0).min((strip.layout.width - visible).max(0.0));
}

/// The thumbnails in view first, then ±32 around the middle; the pump
/// develops what's missing.
fn request(app: &mut App) {
    let strip = &app.strip_view;
    let visible = visible_width(app.strip);
    let range = strip.layout.indices(strip.scroll, strip.scroll + visible);
    let on_screen: Vec<usize> = range.clone().collect();
    let open = strip.cells.iter().position(|c| c.open);
    let ahead = rules::prefetch_order(strip.cells.len(), &on_screen, open);
    let wants: Vec<&Want> = on_screen.iter().chain(&ahead).map(|&i| &strip.cells[i].want).collect();
    // Launch's develop first: reading previews beside it cost it 3 %.
    if app.model.first_render_shown || app.model.render_failed {
        app.thumbs.read(&wants, ThumbSize::Strip);
    }
    let all: Vec<Want> = strip.cells.iter().map(|c| c.want.clone()).collect();
    app.thumbs.pump(&app.model, &all, range, open);
}

/// The device is made, or made again.
pub fn surfaces_ready(app: &mut App) {
    app.thumbs.device_lost();
    app.model.dirty |= dirty::STRIP;
    let ((column, edge), shown) = (app.strip_view.leader_place, app.strip_view.leader_shown);
    layout_leader(app, column, edge, shown);
    invalidate(app.strip);
    super::grid::surfaces_ready(app);
    super::tray::surfaces_ready();
}

pub fn resized(app: &mut App) {
    let visible = visible_width(app.strip);
    let strip = &mut app.strip_view;
    // After a resize the open frame glides back to the middle.
    if let Some(i) = strip.cells.iter().position(|c| c.open) {
        let target = rules::centred(strip.layout.middle(i), visible, strip.layout.width);
        if (target - strip.scroll).abs() > 0.5 {
            strip.glide = Some((strip.scroll, target, Instant::now()));
            unsafe {
                SetTimer(app.strip, TIMER_GLIDE, 15, None);
            }
        }
    }
    clamp(&mut app.strip_view, visible);
    request(app);
    invalidate(app.strip);
}

fn glide(app: &mut App) {
    let Some((from, to, started)) = app.strip_view.glide else {
        unsafe {
            let _ = KillTimer(app.strip, TIMER_GLIDE);
        }
        return;
    };
    let t = started.elapsed().as_secs_f64() * 1000.0 / rules::GLIDE_MS;
    app.strip_view.scroll = from + (to - from) * rules::smoothstep(t);
    if t >= 1.0 {
        app.strip_view.glide = None;
        unsafe {
            let _ = KillTimer(app.strip, TIMER_GLIDE);
        }
        request(app);
    }
    invalidate(app.strip);
}

fn scroll_by(app: &mut App, delta: f64) {
    let visible = visible_width(app.strip);
    let strip = &mut app.strip_view;
    strip.glide = None;
    strip.scroll += delta;
    clamp(strip, visible);
    request(app);
    invalidate(app.strip);
}

// MARK: Painting

/// One frame: its picture (or bare film until it's read), dimmed as the
/// strip dims it, its marks, the badge, the edited dot. Shared with the
/// grid's flight, which draws the strip's frames as they leave.
pub fn paint_frame(p: &mut Painter, thumbs: &mut Thumbs, cell: &Cell, r: RectF, opacity: f32, t: &Tokens, in_strip: bool) {
    let size = if in_strip { ThumbSize::Strip } else { ThumbSize::for_rows(r.h as f64, 1.0) };
    match thumbs.shown(&cell.want, size) {
        Some(image) => p.picture(image.bitmap(), (image.width, image.height), r, opacity),
        None => p.fill_rect(r, Ink::hex(t.film.panel).alpha(opacity)),
    }
    marks(p, cell, r, t, in_strip);
}

/// `PhotoMarks`: the reject's cross over the picture, the stars at its
/// foot, with their halo; the edited dot and the "+J" badge without.
pub fn marks(p: &mut Painter, cell: &Cell, r: RectF, t: &Tokens, in_strip: bool) {
    let s = &cell.summary;
    let paper = Ink::hex(t.film.paper);
    if s.flag == Some(Flag::Reject) {
        p.cross(&cell.name, r, paper, true);
    }
    for i in 0..s.rating.min(5) as usize {
        let cx = r.x + 10.0 + i as f32 * (STAR + STAR_GAP) + STAR / 2.0;
        let cy = r.y + r.h - 10.0 - STAR / 2.0;
        p.star(&cell.name, i, cx, cy, paper, true);
    }
    if s.edited {
        p.fill_ellipse(r.x + r.w - 11.0 - 3.5, r.y + 11.0 + 3.5, 3.5, 3.5, Ink::hex(t.film.accent));
    }
    if cell.pair {
        let word = if in_strip { "+J" } else { "+JPG" };
        let style = super::d2d::TextStyle::ui(10.0, 600);
        let m = p.measure(word, &style, 0.0);
        let inset = if in_strip { 5.0 } else { 7.0 };
        let badge = RectF::new(r.x + r.w - inset - m.width - 8.0, r.y + r.h - inset - 15.0, m.width + 8.0, 15.0);
        p.fill_round_rect(badge, 3.0, Ink::hex(0x000000).alpha(0.6));
        p.text(word, &style, badge.x + 4.0, badge.y + (15.0 - m.height) / 2.0, paper);
    }
}

fn paint(app: &mut App, hwnd: HWND) {
    let t = theme::tokens();
    let s = scale(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let (w, h) = (client.right as f64 / s, client.bottom as f64 / s);
    let App { strip_view: strip, thumbs, .. } = app;
    let range = strip.layout.indices(strip.scroll - rules::STRIP_MARGIN, strip.scroll + w + rules::STRIP_MARGIN);
    let Strip { cells, layout, scroll, hover, pressed, surface, .. } = strip;
    let painted = surface.paint(hwnd, (client.right.max(1) as u32, client.bottom.max(1) as u32), s as f32, t.plain, |p| {
        p.fill_rect(RectF::new(0.0, 0.0, w as f32, h as f32), Ink::hex(t.film.film));
        let rect = |i: usize| RectF::new((layout.xs[i] - *scroll) as f32, rules::STRIP_TOP as f32, layout.widths[i] as f32, rules::PICTURE as f32);
        for i in range.clone() {
            let cell = &cells[i];
            let r = rect(i);
            let rejected = cell.summary.flag == Some(Flag::Reject);
            let opacity = rules::strip_opacity(cell.open, cell.selected, rejected, *hover == Some(i), *pressed == Some(i));
            paint_frame(p, thumbs, cell, r, opacity as f32, &t, true);
            if cell.open {
                let pencil = RectF::new(r.x, r.y + r.h + rules::PENCIL_DROP as f32, r.w, rules::PENCIL_WIDTH as f32 + 2.0);
                p.underline(pencil, &t, true);
            } else if cell.selected {
                // Selected: at full strength with a plain rule under it, as
                // on the Mac, even alone (Ctrl took the open one out, or a
                // right click chose it). The grid's yellow box, 8 out, ran
                // into the canvas over the strip's 6.
                p.fill_rect(RectF::new(r.x, r.y + r.h + rules::PENCIL_DROP as f32, r.w, 1.5), Ink::hex(t.film.label));
            }
        }
        // A pick's box reaches past its frame, over the gap and the frames
        // beside it, so the boxes are drawn above every frame (PhotoBrowser).
        for i in range.clone() {
            if cells[i].summary.flag == Some(Flag::Pick) {
                p.pick_box(BoxInk::Strip, &cells[i].name, rect(i), Ink::hex(t.film.paper), true);
            }
        }
    });
    if painted {
        app.strip_view.painted = app.thumbs.revision;
        unsafe {
            let _ = ValidateRect(hwnd, None);
        }
    } else {
        // Plain film until the device is made.
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            FillRect(dc, &client, theme::brush(theme::gdi(t.film.film)));
            let _ = EndPaint(hwnd, &ps);
        }
    }
}

// MARK: Input

/// ← and → in the photograph view open the previous and next photograph
/// wherever the keys are, not only once the strip has the focus
/// (`EditorWindowController.arrowsToStep`): with no focus, or on the
/// window, the strip or the canvas. The inspector's controls, the
/// choosers and text keep their arrows (the tray, the grid and Compare
/// take theirs first), and the focused canvas pans with them while the
/// photograph is wider than the view (Windows only: the Mac's doesn't pan).
pub fn arrows_to_step(app: &mut App, msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || super::modifier_down() {
        return false;
    }
    let step = match VIRTUAL_KEY(msg.wParam.0 as u16) {
        VK_LEFT => -1,
        VK_RIGHT => 1,
        _ => return false,
    };
    if app.model.source.is_none() || app.model.cropping() || super::grid::shown(app) || app.model.comparison().is_some() {
        return false;
    }
    let focus = unsafe { GetFocus() };
    let free = focus.0.is_null() || focus == app.main || focus == app.strip || (focus == app.canvas && !super::canvas::pans_sideways());
    if !free {
        return false;
    }
    open_neighbour(app, step);
    true
}

fn open_neighbour(app: &mut App, step: isize) {
    if app.model.cropping() {
        return;
    }
    if app.model.shelf.library.is_some() {
        return app.model.step_photo(step);
    }
    let strip = &app.strip_view;
    let Some(index) = strip.cells.iter().position(|c| c.open) else { return };
    let next = index as isize + step;
    if next >= 0 && (next as usize) < strip.cells.len() {
        let path = strip.cells[next as usize].want.item.photo.clone();
        app.model.open(&path);
    }
}

/// A click (`EditorModel.click`): Ctrl adds or takes it from the selection,
/// Shift selects up to it, and a plain click selects and opens it.
pub fn click(app: &mut App, item: &Item, open: bool) {
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
    let order = app.model.shown_items();
    let selection = &mut app.model.shelf.selection;
    if ctrl {
        selection.toggle(item, Some(&order));
    } else if shift {
        selection.extend_to(item, &order);
    } else {
        selection.select(item);
        if open {
            app.model.open_item(item.clone());
        }
    }
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
}

/// The Photo menu's library commands at the pointer, for the clicked
/// photograph and the selection it's in (else it alone, as in Explorer).
pub fn context_menu(app: &mut App, hwnd: HWND, item: &Item, screen: POINT) {
    if !app.model.shelf.selection.contains(item) {
        app.model.shelf.selection.select(item);
        app.model.dirty |= dirty::STRIP | dirty::CHROME;
    }
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        super::compare::add_offer(app, menu, item);
        super::library::add_menu(menu);
        super::library::update_menu(app, menu);
        super::batch::export_items(app, menu);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, screen.x, screen.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        // Copy Edits copies the photograph clicked, not the open one
        // (`copyEdits(from: item)`); the menu bar's copies the open one.
        let from = item.clone();
        match command.0 as u16 {
            library::COPY_EDITS => post(move |app: &mut App| app.model.copy_edits(Some(from), None)),
            library::COPY_EDITS_CHOOSING => post(move |app: &mut App| library::copy_edits_choosing(app, Some(from))),
            _ if command.0 != 0 => {
                let _ = PostMessageW(app.main, WM_COMMAND, WPARAM(command.0 as usize), LPARAM(0));
            }
            _ => {}
        }
    }
}

fn index_at(app: &App, hwnd: HWND, lparam: LPARAM) -> Option<usize> {
    let s = scale(hwnd);
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f64 / s, ((lparam.0 >> 16) & 0xffff) as i16 as f64 / s);
    let strip = &app.strip_view;
    (y >= rules::STRIP_TOP && y <= rules::STRIP_TOP + rules::PICTURE).then(|| strip.layout.hit(x + strip.scroll)).flatten()
}

fn set_hover(app: &mut App, hover: Option<usize>) {
    if app.strip_view.hover != hover {
        app.strip_view.hover = hover;
        invalidate(app.strip);
    }
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
            WM_SIZE => {
                with_app(resized);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_GLIDE => {
                with_app(glide);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                with_app(|app| {
                    let hover = index_at(app, hwnd, lparam);
                    set_hover(app, hover);
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|app| {
                    app.strip_view.pressed = None;
                    set_hover(app, None);
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                with_app(|app| {
                    if app.model.cropping() {
                        return;
                    }
                    app.strip_view.pressed = index_at(app, hwnd, lparam);
                    invalidate(hwnd);
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|app| {
                    let pressed = app.strip_view.pressed.take();
                    invalidate(hwnd);
                    let Some(index) = pressed.filter(|&i| index_at(app, hwnd, lparam) == Some(i)) else { return };
                    let cell = app.strip_view.cells[index].clone();
                    if app.model.shelf.library.is_some() {
                        return click(app, &cell.want.item, !cell.open);
                    }
                    if !cell.open {
                        app.model.open(&cell.want.item.photo);
                    }
                });
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_app(|app| {
                    if app.model.cropping() || app.model.shelf.library.is_none() {
                        return;
                    }
                    let Some(index) = index_at(app, hwnd, lparam) else { return };
                    let item = app.strip_view.cells[index].want.item.clone();
                    let mut point = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                    let _ = ClientToScreen(hwnd, &mut point);
                    context_menu(app, hwnd, &item, point);
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f64;
                with_app(|app| {
                    let step = 80.0 * delta / 120.0;
                    scroll_by(app, if msg == WM_MOUSEHWHEEL { step } else { -step });
                });
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_KEYDOWN => {
                match VIRTUAL_KEY(wparam.0 as u16) {
                    VK_LEFT => {
                        with_app(|app| open_neighbour(app, -1));
                    }
                    VK_RIGHT => {
                        with_app(|app| open_neighbour(app, 1));
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                invalidate(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                with_app(|app| app.strip_view.surface.release());
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// MARK: The leader

/// Where the leader stands: on the strip's top edge at its right end, or
/// on the status line's with the strip hidden, over the canvas.
pub fn layout_leader(app: &mut App, column: i32, edge: i32, shown: bool) {
    app.strip_view.leader_place = (column, edge);
    app.strip_view.leader_shown = shown;
    // Made once the window is up (`surfaces_ready`): launch doesn't wait.
    if app.strip_view.leader.is_invalid() && !super::surface::ready() {
        return;
    }
    let leader = ensure_leader(app);
    let s = app.scale();
    let (w, h) = ((rules::LEADER_WIDTH * s).round() as i32, (rules::LEADER_RAISED * s).round() as i32);
    app.strip_view.leader_shown = shown;
    unsafe {
        let _ = SetWindowPos(leader, HWND_TOP, column - w, edge - h, w, h, SWP_NOACTIVATE | if shown { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW });
    }
    shape_leader(app);
    leader_on_canvas(app);
}

/// The canvas draws the tongue under the leader's window, whose shape is
/// aliased: its smooth slanted edge shows just outside the window's.
fn leader_on_canvas(app: &App) {
    let shown = !app.strip_view.leader.is_invalid() && app.strip_view.leader_shown && !app.model.cropping() && !super::grid::shown(app);
    super::canvas::set_leader(shown.then_some(app.strip_view.leader_hover));
}

fn sync_leader(app: &mut App) {
    let shown = app.strip_view.leader_shown && !app.model.cropping() && !super::grid::shown(app);
    let leader = app.strip_view.leader;
    if !leader.is_invalid() {
        unsafe {
            let _ = ShowWindow(leader, if shown { SW_SHOWNA } else { SW_HIDE });
        }
        invalidate(leader);
    }
    leader_on_canvas(app);
}

fn ensure_leader(app: &mut App) -> HWND {
    if app.strip_view.leader.is_invalid() {
        let parent = unsafe { GetParent(app.strip) }.unwrap_or_default();
        app.strip_view.leader = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("NegsStripLeader"), w!("Hide Film Strip"), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 1, 1, parent, None, super::instance(), None)
        }
        .unwrap_or_default();
    }
    app.strip_view.leader
}

/// The tongue as the window's shape: `polygon(0 100%, 28px 0, 100% 0,
/// 100% 100%)`, 24 DIP tall, 30 under the pointer. The slanted edge is 2 px
/// inside the canvas's, whose anti-aliased edge shows round it.
fn shape_leader(app: &App) {
    let leader = app.strip_view.leader;
    let s = app.scale();
    let (w, h) = ((rules::LEADER_WIDTH * s).round() as i32, (rules::LEADER_RAISED * s).round() as i32);
    let tall = ((if app.strip_view.leader_hover { rules::LEADER_RAISED } else { rules::LEADER_HEIGHT }) * s).round() as i32;
    let slant = (rules::LEADER_SLANT * s).round() as i32;
    let inset = 2;
    let points = [POINT { x: inset, y: h }, POINT { x: slant + inset, y: h - tall }, POINT { x: w, y: h - tall }, POINT { x: w, y: h }];
    unsafe {
        let region = CreatePolygonRgn(&points, WINDING);
        SetWindowRgn(leader, region, true);
    }
}

fn paint_leader(app: &App, hwnd: HWND) {
    let t = theme::tokens();
    let s = scale(hwnd) as f32;
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut c = super::draw::Canvas::new(dc, client);
        c.clear(theme::gdi(t.film.film));
        let hover = app.strip_view.leader_hover;
        let shown = app.model.prefs.filmstrip_visible;
        c.d2d(s, t.plain, |p| {
            let w = client.right as f32 / s;
            let top = (rules::LEADER_RAISED - if hover { rules::LEADER_RAISED } else { rules::LEADER_HEIGHT }) as f32;
            // The chevron: 12.7 × 6.4, in a 9 DIP box 22 from the right, 10 down.
            let (cx, cy) = (w - 22.0 - 4.5, top + 10.0 + 4.5 - if hover { 1.0 } else { 0.0 });
            let (dx, dy) = (6.35, if shown { -3.2 } else { 3.2 });
            let ink = Ink::hex(if hover { t.film.paper } else { t.film.label });
            p.line((cx - dx, cy + dy), (cx, cy - dy), 2.0, ink);
            p.line((cx, cy - dy), (cx + dx, cy + dy), 2.0, ink);
        });
        c.present(dc);
        let _ = EndPaint(hwnd, &ps);
    }
}

extern "system" fn leader_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                if with_app(|app| paint_leader(app, hwnd)).is_none() {
                    let mut ps = PAINTSTRUCT::default();
                    BeginPaint(hwnd, &mut ps);
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                with_app(|app| {
                    if !app.strip_view.leader_hover {
                        app.strip_view.leader_hover = true;
                        shape_leader(app);
                        leader_on_canvas(app);
                        invalidate(hwnd);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|app| {
                    app.strip_view.leader_hover = false;
                    shape_leader(app);
                    leader_on_canvas(app);
                    invalidate(hwnd);
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|app| {
                    let _ = PostMessageW(app.main, WM_COMMAND, WPARAM(super::id::FILMSTRIP as usize), LPARAM(0));
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
