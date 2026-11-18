//! The Presets pane (`PresetInspector.swift`, IMPLEMENTATION.md §4.12): the
//! presets as tiles in two columns, each the photograph developed through
//! it (unless Settings turns previews off), its name in Newsreader 15, and
//! its film and paper in the label's words. A click applies one; the one
//! applied last to the photograph has the pencil ring. Tiles drag to sort,
//! and preset files dropped on the pane import before the tile they're
//! over. New and Import… are in the pane header (`panes.rs`); the folder is
//! in the File menu and each tile's context menu. Created the first time
//! it shows.
//!
//! Each tile is a push button, so Tab, the arrows, Enter, Del, F2, the
//! context menu key and UI Automation reach it, but the tiles' parent
//! paints them all, so a ring can reach past its tile. Only the tiles on
//! screen, and a row ahead, are developed (`Model::set_visible_presets`).

use std::cell::RefCell;
use std::sync::Arc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::{SS_NOPREFIX, SS_OWNERDRAW};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Align, Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas, Role};
use super::theme;
use super::{App, create, id, ink, post, with_app};
use crate::model::{Model, Pane};
use crate::platform::{pcwstr, wide};
use crate::presets::{PresetFile, Status, name};
use crate::previews::{Bitmap, Target};

const TILE_FIRST: u16 = 100;
const EMPTY_TITLE: u16 = 2;
const EMPTY_TEXT: u16 = 3;
const EMPTY_IMPORT: u16 = 4;
const FOOT: u16 = 5;
const RENAME: u16 = 6;

// The tiles' grid (`PresetTileRules`, `presets.css`), in DIPs.
const COLUMN_GAP: f64 = 14.0;
const ROW_GAP: f64 = 20.0;
const TOP: f64 = 18.0;
const BOTTOM: f64 = 22.0;
/// Under the picture: 6, the name's line, 1, and the film's and paper's.
const CAPTION_GAP: f64 = 6.0;
const NAME_LINE: f64 = 20.0;
const DETAIL_LINE: f64 = 16.0;
const FOOT_HEIGHT: f64 = 34.0;

const NAME: TextStyle = TextStyle::serif(15.0, 400);
const NAME_APPLIED: TextStyle = TextStyle::serif(15.0, 600);
const DETAIL: TextStyle = TextStyle::serif(12.5, 400);
const DETAIL_LEAD: TextStyle = TextStyle::serif(12.5, 400).italic();
const UNREADABLE: TextStyle = TextStyle::serif(13.0, 400).italic();
const EMPTY_HEAD: TextStyle = TextStyle::serif(21.0, 500);
const EMPTY_WORDS: TextStyle = TextStyle::serif(14.0, 400).italic();
const EMPTY_NEW: TextStyle = TextStyle::serif(14.0, 400);
const FOOT_WORDS: TextStyle = TextStyle::serif(13.0, 400).italic();

/// The empty pane's sentence, its parts in their fonts: "New" upright in `label`.
const EMPTY_SENTENCE: [(&str, bool); 3] = [
    ("Save this photograph’s film, paper and Lab settings with ", false),
    ("New", true),
    (", or import presets made in Negs or spektrafilm. You can also drop them here.", false),
];

/// A tile as drawn: what the model says about one file.
#[derive(Clone, Default)]
struct Look {
    name: String,
    /// The film's line, and the paper's: "printed on" and the paper, or
    /// "not printed · film scan". None until the engine has read the file.
    film: Option<String>,
    paper: Option<(&'static str, String)>,
    /// Why the engine refuses the file.
    invalid: Option<String>,
    applied: bool,
    image: Option<Arc<Bitmap>>,
    dimmed: bool,
}

impl PartialEq for Look {
    fn eq(&self, other: &Self) -> bool {
        let image = match (&self.image, &other.image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        image && self.name == other.name && self.film == other.film && self.paper == other.paper && self.invalid == other.invalid && self.applied == other.applied && self.dimmed == other.dimmed
    }
}

/// What painting reads, outside the app's borrow.
#[derive(Default)]
struct View {
    looks: Vec<Look>,
    /// Each tile's rectangle in content pixels, in the order shown.
    rects: Vec<RECT>,
    tiles: Vec<isize>,
    scroll: i32,
    pictures: bool,
    enabled: bool,
    /// The tile whose name is being edited, and the name as typed.
    editing: Option<(usize, String)>,
    /// Preset files over the pane.
    dropping: bool,
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
}

/// A tile held to sort: its index among the presets, and the order shown.
struct Drag {
    from: usize,
    order: Vec<usize>,
}

pub struct Panel {
    pub window: HWND,
    tiles_view: HWND,
    tiles: Vec<HWND>,
    files: Vec<PresetFile>,
    empty_title: HWND,
    empty_text: HWND,
    empty_import: HWND,
    foot: HWND,
    rename: HWND,
    rename_font: HFONT,
    /// Where the pointer went down on a tile, until it moves far enough to drag.
    press: Option<(usize, POINT)>,
    drag: Option<Drag>,
    scroll: i32,
    content: i32,
}

pub fn register() {
    unsafe {
        for (class, proc_) in [
            (w!("NegsPresets"), Some(proc_ as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT)),
            (w!("NegsPresetTiles"), Some(tiles_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT)),
        ] {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: proc_,
                hInstance: super::instance(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    }
}

fn px(hwnd: HWND, dip: f64) -> i32 {
    (dip * unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0).round() as i32
}

fn rename_font(dpi: u32) -> HFONT {
    let mut lf = LOGFONTW { lfHeight: -(15 * dpi as i32 / 96), lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
    let face = wide("Segoe UI");
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    unsafe { CreateFontIndirectW(&lf) }
}

fn build(app: &mut App) {
    let font = app.fonts.ui;
    let window = create(w!("NegsPresets"), "Presets", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), app.main, 0, HFONT::default());
    // Not WS_CLIPCHILDREN: it paints its tiles under the buttons that stand for them.
    let tiles_view = create(w!("NegsPresetTiles"), "Presets", WS_CHILD, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), window, 0, HFONT::default());
    let drawn = |text: &str, id: u16| {
        let c = create(w!("STATIC"), text, WS_CHILD | WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), window, id, font);
        super::accessible_role(c, 0x29);
        c
    };
    let empty_title = drawn("No presets yet", EMPTY_TITLE);
    let empty_text = drawn(&EMPTY_SENTENCE.iter().map(|(t, _)| *t).collect::<String>(), EMPTY_TEXT);
    let empty_import = create(w!("BUTTON"), "Import Presets…", WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), window, EMPTY_IMPORT, font);
    draw::set_role(empty_import, Role::Outlined);
    draw::track_hover(empty_import);
    super::dialogs::tooltip(empty_import, "Import presets (Ctrl+Shift+O)");
    let foot = drawn(foot_words(false), FOOT);
    let rename_font = rename_font(app.dpi);
    let rename = create(w!("EDIT"), "", WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WINDOW_EX_STYLE(0), tiles_view, RENAME, rename_font);
    super::accessible_name(rename, "Preset name");
    unsafe {
        SendMessageW(rename, EM_SETLIMITTEXT, WPARAM(name::MAX_LENGTH + 20), LPARAM(0));
        let _ = SetWindowSubclass(rename, Some(rename_subclass), 1, 0);
    }
    app.presets_view = Some(Panel {
        window,
        tiles_view,
        tiles: vec![],
        files: vec![],
        empty_title,
        empty_text,
        empty_import,
        foot,
        rename,
        rename_font,
        press: None,
        drag: None,
        scroll: 0,
        content: 0,
    });
    super::drop::register_presets(window, tiles_view);
}

pub fn dpi_changed(app: &mut App) {
    let Some(p) = &mut app.presets_view else { return };
    let font = rename_font(app.dpi);
    unsafe {
        SendMessageW(p.rename, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        let _ = DeleteObject(std::mem::replace(&mut p.rename_font, font));
    }
}

/// What the foot says: what a click and a drag do, or what letting go of
/// the files will do (`PresetTileRules.foot`).
fn foot_words(dropping: bool) -> &'static str {
    if dropping { "drop to import" } else { "click to apply · drag to sort · drop files to import" }
}

/// Places the panel in the adjustments column, creating it the first time
/// it shows (after the window's first frame, like the adjustments).
pub fn layout(app: &mut App, rect: RECT, visible: bool) {
    if app.presets_view.is_none() {
        if !visible || !app.inspector.built() {
            return;
        }
        build(app);
        app.model.refresh_presets();
    }
    let Some(p) = &app.presets_view else { return };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    unsafe {
        let _ = MoveWindow(p.window, rect.left, rect.top, w, h, true);
        let _ = ShowWindow(p.window, if visible { SW_SHOWNA } else { SW_HIDE });
    }
    if visible {
        place(app);
    }
}

/// The tiles' grid for a view `width` pixels wide: each tile's rectangle
/// in content pixels, and the content's height.
fn grid(hwnd: HWND, count: usize, width: i32, pictures: bool) -> (Vec<RECT>, i32) {
    let px = |dip: f64| px(hwnd, dip);
    let (gutter, gap, row_gap) = (px(ink::GUTTER), px(COLUMN_GAP), px(ROW_GAP));
    let tile_w = ((width - 2 * gutter - gap) / 2).max(px(40.0));
    let picture = if pictures { (tile_w as f64 * 2.0 / 3.0).round() as i32 + px(CAPTION_GAP) } else { 0 };
    let tile_h = picture + px(NAME_LINE + 1.0 + 2.0 * DETAIL_LINE);
    let rects = (0..count)
        .map(|i| {
            let (column, row) = ((i % 2) as i32, (i / 2) as i32);
            let (x, y) = (gutter + column * (tile_w + gap), px(TOP) + row * (tile_h + row_gap));
            RECT { left: x, top: y, right: x + tile_w, bottom: y + tile_h }
        })
        .collect::<Vec<_>>();
    let rows = count.div_ceil(2) as i32;
    let content = if count == 0 { 0 } else { px(TOP) + rows * tile_h + (rows - 1) * row_gap + px(BOTTOM) };
    (rects, content)
}

/// The order shown: as dragged, while a tile is.
fn order(p: &Panel) -> Vec<usize> {
    p.drag.as_ref().map_or_else(|| (0..p.files.len()).collect(), |d| d.order.clone())
}

/// Lays out the pane: the tiles in their grid and the foot under them, or
/// the empty state; scrolls the tiles and tells the model which are seen.
fn place(app: &mut App) {
    let Some(p) = &mut app.presets_view else { return };
    let window = p.window;
    let px = |dip: f64| px(window, dip);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut client);
    }
    let (w, h) = (client.right, client.bottom);
    let empty = p.files.is_empty();
    let rule = px(1.0).max(1);
    let foot = px(FOOT_HEIGHT);
    let view_h = (h - rule - foot - rule).max(0);
    let pictures = app.model.prefs.previews;
    let (rects, content) = grid(window, p.files.len(), w, pictures);
    p.content = content;
    p.scroll = p.scroll.clamp(0, (content - view_h).max(0));
    let order = order(p);
    // Each preset's place: its index in the order shown picks its rectangle.
    let mut placed = vec![RECT::default(); p.files.len()];
    for (slot, &index) in order.iter().enumerate() {
        if let (Some(r), Some(at)) = (rects.get(slot), placed.get_mut(index)) {
            *at = *r;
        }
    }
    unsafe {
        let _ = MoveWindow(p.tiles_view, 0, rule, w, view_h, true);
        let _ = ShowWindow(p.tiles_view, if empty { SW_HIDE } else { SW_SHOWNA });
        let _ = MoveWindow(p.foot, 0, h - foot, w, foot, true);
        let _ = ShowWindow(p.foot, if empty { SW_HIDE } else { SW_SHOWNA });
        for (i, &tile) in p.tiles.iter().enumerate() {
            let r = placed[i];
            let _ = MoveWindow(tile, r.left, r.top - p.scroll, r.right - r.left, r.bottom - r.top, false);
        }
        for c in [p.empty_title, p.empty_text, p.empty_import] {
            let _ = ShowWindow(c, if empty { SW_SHOWNA } else { SW_HIDE });
        }
    }
    if empty {
        // Centred, 44 in from the sides, 40 up from the foot (`PresetsEmpty`).
        let width = (w - 2 * px(44.0)).max(px(120.0));
        let s = draw::dpi_of(window);
        let plain = theme::tokens().plain;
        let head = px(super::d2d::measure_text("No presets yet", &EMPTY_HEAD, 0.0, plain).height as f64);
        let mut measure = |text: &str, style: &TextStyle| super::d2d::measure_text(text, style, 0.0, plain).width;
        let lines = sentence_lines(width as f32 / s, &mut measure).len() as f32;
        let line = sentence_line(super::d2d::measure_text("Save", &EMPTY_WORDS, 0.0, plain).height);
        let text = (lines * line * s).ceil() as i32 + px(2.0);
        let button = px(ink::width("Import Presets…", &ink::OUTLINED) as f64 + 24.0) + 2 * px(ink::MARGIN);
        let button_h = px(23.0) + 2 * px(ink::MARGIN);
        let total = head + px(10.0) + text + px(16.0) + px(23.0);
        let mut y = rule + ((h - rule - px(40.0) - total) / 2).max(0);
        let x = (w - width) / 2;
        unsafe {
            let _ = MoveWindow(p.empty_title, x, y, width, head, true);
            y += head + px(10.0);
            let _ = MoveWindow(p.empty_text, x, y, width, text, true);
            y += text + px(16.0);
            let _ = MoveWindow(p.empty_import, (w - button) / 2, y - px(ink::MARGIN), button, button_h, true);
        }
    }
    // The tiles on screen, and a row either side.
    let mut visible = 0..0;
    if !empty {
        let row = rects.first().map_or(1, |r| r.bottom - r.top + px(ROW_GAP)).max(1);
        let first = ((p.scroll - px(TOP)) / row - 1).max(0) as usize * 2;
        let last = (((p.scroll + view_h - px(TOP)) / row + 2).max(0) as usize * 2).min(p.files.len());
        // Slots shown, as presets.
        let shown: Vec<usize> = order.get(first.min(last)..last).map(|s| s.to_vec()).unwrap_or_default();
        let (low, high) = (shown.iter().min().copied(), shown.iter().max().copied());
        if let (Some(low), Some(high)) = (low, high) {
            visible = low..high + 1;
        }
    }
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.rects = placed;
        v.scroll = p.scroll;
        v.pictures = pictures;
        v.tiles = p.tiles.iter().map(|t| t.0 as isize).collect();
    });
    place_rename(p);
    let tiles_view = p.tiles_view;
    unsafe {
        let _ = InvalidateRect(tiles_view, None, false);
        let _ = InvalidateRect(window, None, false);
    }
    app.model.set_visible_presets(visible);
}

fn look(model: &Model, file: &PresetFile) -> Look {
    let stock = |id: &str, paper: bool| model.stocks.iter().find(|s| s.paper == paper && s.id == id).map_or_else(|| id.to_owned(), |s| s.name.clone());
    let mut look = Look { name: file.name(), applied: model.applied_preset() == Some(&file.path), ..Default::default() };
    match model.preset_status.get(file) {
        Some(Status::Valid { film, paper, scan, .. }) => {
            look.film = Some(stock(film, false));
            look.paper = Some(if *scan { ("not printed · film scan", String::new()) } else { ("printed on", stock(paper, true)) });
        }
        Some(Status::Invalid(reason)) => look.invalid = Some(reason.clone()),
        _ => {}
    }
    if model.prefs.previews
        && let Some(thumb) = model.previews.thumb(&Target::Preset(file.path.clone()))
    {
        look.image = thumb.image.clone();
        look.dimmed = thumb.dimmed;
    }
    look
}

/// What a screen reader says after the name (`accessibilityValue`).
fn spoken(look: &Look) -> String {
    if let Some(reason) = &look.invalid {
        return format!("Can’t be read. {reason}");
    }
    let (Some(film), Some((lead, paper))) = (&look.film, &look.paper) else { return String::new() };
    let print = if paper.is_empty() { lead.to_string() } else { format!("{lead} {paper}") };
    format!("{}{film}, {print}", if look.applied { "Applied last. " } else { "" })
}

/// Brings the pane up to date with the model: the tiles, the empty state,
/// whether it can change presets, and a rename New asked for.
pub fn sync(app: &mut App) {
    let Some(p) = &mut app.presets_view else { return };
    // Hidden: brought up to date when it shows (a layout change).
    if !unsafe { IsWindowVisible(p.window) }.as_bool() {
        return;
    }
    let model = &app.model;
    let files_changed = p.files != model.presets;
    if files_changed && editing(p).is_none() {
        p.drag = None;
        p.press = None;
        let font = app.fonts.ui;
        let tiles_view = p.tiles_view;
        while p.tiles.len() > model.presets.len() {
            let tile = p.tiles.pop().unwrap();
            unsafe {
                let _ = DestroyWindow(tile);
            }
        }
        while p.tiles.len() < model.presets.len() {
            let index = p.tiles.len();
            let style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32) | if index == 0 { WS_GROUP | WS_TABSTOP } else { WINDOW_STYLE(0) };
            let tile = create(w!("BUTTON"), "", style, WINDOW_EX_STYLE(0), tiles_view, TILE_FIRST + index as u16, font);
            draw::track_hover(tile);
            unsafe {
                let _ = SetWindowSubclass(tile, Some(tile_subclass), 1, 0);
            }
            p.tiles.push(tile);
        }
        p.files = model.presets.clone();
    }
    let looks: Vec<Look> = p.files.iter().map(|f| look(model, f)).collect();
    let enabled = model.can_change_presets();
    let changed = VIEW.with(|v| {
        let mut v = v.borrow_mut();
        let changed = v.looks != looks || v.enabled != enabled;
        for (i, look) in looks.iter().enumerate() {
            if v.looks.get(i).is_none_or(|old| old.name != look.name) {
                let tile = p.tiles[i];
                super::draw::window_text(tile).ne(&look.name).then(|| {
                    let text = wide(&look.name);
                    unsafe {
                        let _ = SetWindowTextW(tile, pcwstr(&text));
                    }
                });
                super::dialogs::tooltip(tile, &look.name);
            }
            if v.looks.get(i) != Some(look) {
                super::accessible_value(p.tiles[i], &spoken(look));
            }
        }
        v.looks = looks;
        v.enabled = enabled;
        changed
    });
    unsafe {
        for &c in p.tiles.iter().chain([&p.empty_import]) {
            if IsWindowEnabled(c).as_bool() != enabled {
                let _ = EnableWindow(c, enabled);
            }
        }
    }
    // The applied tile, or the first, is where Tab lands.
    let applied = VIEW.with(|v| v.borrow().looks.iter().position(|l| l.applied));
    set_tab_stop(p, applied.unwrap_or(0), false);
    let renaming = model.renaming_preset.as_ref().and_then(|path| p.files.iter().position(|f| &f.path == path));
    if files_changed || changed {
        place(app);
    }
    if let Some(index) = renaming {
        start_rename(app, index);
    }
}

/// Moves the tab stop to tile `index`, unless a tile has focus (`force`: anyway).
fn set_tab_stop(p: &Panel, index: usize, force: bool) {
    let focused = unsafe { GetFocus() };
    if !force && p.tiles.contains(&focused) {
        return;
    }
    for (i, &tile) in p.tiles.iter().enumerate() {
        unsafe {
            let style = GetWindowLongW(tile, GWL_STYLE) as u32;
            let want = if i == index { style | WS_TABSTOP.0 } else { style & !WS_TABSTOP.0 };
            if want != style {
                SetWindowLongW(tile, GWL_STYLE, want as i32);
            }
        }
    }
}

fn editing(p: &Panel) -> Option<usize> {
    unsafe { IsWindowVisible(p.rename).as_bool() }.then(|| VIEW.with(|v| v.borrow().editing.as_ref().map(|e| e.0))).flatten()
}

/// The pencil-underlined field over tile `index`'s name.
fn start_rename(app: &mut App, index: usize) {
    let Some(p) = &mut app.presets_view else { return };
    if editing(p).is_some() || index >= p.files.len() {
        return;
    }
    let name = p.files[index].name();
    VIEW.with(|v| v.borrow_mut().editing = Some((index, name.clone())));
    reveal(p, index);
    let text = wide(&name);
    unsafe {
        let _ = SetWindowTextW(p.rename, pcwstr(&text));
        SendMessageW(p.rename, EM_SETSEL, WPARAM(0), LPARAM(-1));
    }
    place_rename(p);
    unsafe {
        let _ = ShowWindow(p.rename, SW_SHOW);
        let _ = SetFocus(p.rename);
        let _ = InvalidateRect(p.tiles_view, None, false);
    }
}

/// The field on the name's line of the tile being named.
fn place_rename(p: &Panel) {
    let Some((index, _)) = VIEW.with(|v| v.borrow().editing.clone()) else { return };
    let Some(r) = VIEW.with(|v| v.borrow().rects.get(index).copied()) else { return };
    let px = |dip: f64| px(p.window, dip);
    let top = name_top(p.window, r) - p.scroll;
    unsafe {
        let _ = MoveWindow(p.rename, r.left - px(1.0), top, r.right - r.left + px(1.0), px(NAME_LINE), true);
    }
}

/// The top of a tile's name line, in content pixels.
fn name_top(hwnd: HWND, r: RECT) -> i32 {
    let pictures = VIEW.with(|v| v.borrow().pictures);
    let picture = if pictures { ((r.right - r.left) as f64 * 2.0 / 3.0).round() as i32 + px(hwnd, CAPTION_GAP) } else { 0 };
    r.top + picture
}

/// The field is done: `typed` is None after Escape. An invalid name keeps
/// the old one and says why; Escape on New's preset removes it.
fn finish_rename(app: &mut App, typed: Option<String>) {
    let Some(p) = &mut app.presets_view else { return };
    let Some((index, _)) = VIEW.with(|v| v.borrow_mut().editing.take()) else { return };
    unsafe {
        let _ = ShowWindow(p.rename, SW_HIDE);
        let _ = InvalidateRect(p.tiles_view, None, false);
        if let Some(&tile) = p.tiles.get(index) {
            let _ = SetFocus(tile);
        }
    }
    let Some(file) = p.files.get(index).cloned() else { return };
    let Some(text) = typed else { return app.model.cancel_rename(&file) };
    let typed = text.trim();
    let old = file.name();
    // The renamed tile keeps the focus wherever the list puts it.
    let kept = if name::problem(typed).is_none() { typed.to_owned() } else { old.clone() };
    post(move |app: &mut App| focus_named(app, &kept));
    match name::problem(typed) {
        None => app.model.rename_preset(&file, typed),
        Some(problem) => {
            app.model.rename_preset(&file, &old);
            if !typed.is_empty() {
                let (main, instruction) = (app.main.0 as isize, format!("“{typed}” can’t be used as a preset name."));
                post(move |_| super::dialogs::notice(HWND(main as *mut _), &instruction, &problem));
            }
        }
    }
}

/// Focuses the tile of the preset called `name`, once the pane has the
/// model's list.
fn focus_named(app: &mut App, name: &str) {
    sync(app);
    let Some(p) = &mut app.presets_view else { return };
    let Some(index) = p.files.iter().position(|f| name::key(&f.name()) == name::key(name)) else { return };
    set_tab_stop(p, index, true);
    reveal(p, index);
    unsafe {
        let _ = SetFocus(p.tiles[index]);
    }
}

/// Scrolls so tile `index` is in view.
fn reveal(p: &mut Panel, index: usize) {
    let Some(r) = VIEW.with(|v| v.borrow().rects.get(index).copied()) else { return };
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(p.tiles_view, &mut client);
    }
    let margin = px(p.window, 12.0);
    let target = if r.top - margin < p.scroll {
        r.top - margin
    } else if r.bottom + margin > p.scroll + client.bottom {
        r.bottom + margin - client.bottom
    } else {
        return;
    };
    scroll_to(p, target);
}

fn scroll_to(p: &mut Panel, target: i32) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(p.tiles_view, &mut client);
    }
    let target = target.clamp(0, (p.content - client.bottom).max(0));
    if target == p.scroll {
        return;
    }
    p.scroll = target;
    post(place);
}

/// Applies tile `index` if the engine reads it.
fn apply(app: &mut App, index: usize) {
    let Some(p) = &app.presets_view else { return };
    let Some(file) = p.files.get(index).cloned() else { return };
    if matches!(app.model.preset_status.get(&file), Some(Status::Valid { .. })) {
        app.model.apply_preset(&file);
    }
}

fn confirm_delete(app: &mut App, index: usize) {
    let Some(file) = app.presets_view.as_ref().and_then(|p| p.files.get(index)).cloned() else { return };
    if super::dialogs::confirm_delete_preset(app.main, &file.name()) {
        app.model.delete_preset(&file);
    }
}

fn show_in_folder(file: &PresetFile) {
    let (verb, explorer) = (wide("open"), wide("explorer.exe"));
    let arguments = wide(&format!("/select,\"{}\"", file.path.display()));
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, pcwstr(&verb), pcwstr(&explorer), pcwstr(&arguments), windows::core::PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Apply, Rename, Export…, Show in Folder and Delete… for tile `index`
/// (`PresetTile.contextMenu`, with Windows' Show in Folder).
fn context_menu(app: &mut App, index: usize, screen: POINT) {
    let Some(p) = &app.presets_view else { return };
    let Some(file) = p.files.get(index).cloned() else { return };
    let tile = p.tiles[index];
    let mut at = screen;
    if screen.x == -1 && screen.y == -1 {
        // From the keyboard: under the tile's name.
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(tile, &mut r);
        }
        at = POINT { x: r.left + app.px(12.0), y: r.bottom };
    }
    let valid = matches!(app.model.preset_status.get(&file), Some(Status::Valid { .. })) && app.model.can_change_presets();
    let choice = unsafe {
        let menu = CreatePopupMenu().unwrap();
        let add = |cmd: u32, text: &str, on: bool| {
            let w = wide(text);
            let _ = AppendMenuW(menu, MF_STRING | if on { MF_ENABLED } else { MF_GRAYED }, cmd as usize, pcwstr(&w));
        };
        add(1, "&Apply", valid);
        add(2, "&Rename\tF2", true);
        add(3, "&Export…", true);
        add(5, "Show in &Folder", true);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        add(4, "&Delete…\tDel", true);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, app.main, None).0;
        let _ = DestroyMenu(menu);
        choice
    };
    match choice {
        1 => apply(app, index),
        2 => start_rename(app, index),
        3 => super::dialogs::export_preset(app, &file),
        4 => confirm_delete(app, index),
        5 => show_in_folder(&file),
        _ => {}
    }
}

/// The preset whose tile is under `screen`, by index, if any: files
/// dropped there go before it.
pub fn drop_position(tiles_view: HWND, screen: POINT) -> Option<usize> {
    let mut point = screen;
    unsafe {
        if !IsWindowVisible(tiles_view).as_bool() {
            return None;
        }
        let _ = ScreenToClient(tiles_view, &mut point);
    }
    tile_at(tiles_view, point)
}

/// The tile at `point` in the view's pixels, the gaps around it included.
fn tile_at(hwnd: HWND, point: POINT) -> Option<usize> {
    let (dx, dy) = (px(hwnd, COLUMN_GAP / 2.0), px(hwnd, ROW_GAP / 2.0));
    VIEW.with(|v| {
        let v = v.borrow();
        let y = point.y + v.scroll;
        v.rects.iter().position(|r| point.x >= r.left - dx && point.x < r.right + dx && y >= r.top - dy && y < r.bottom + dy)
    })
}

/// Preset files over the pane: the loose pencil ring round it, and the
/// foot says what letting go does.
pub fn highlight_drop(window: HWND, on: bool) {
    let changed = VIEW.with(|v| std::mem::replace(&mut v.borrow_mut().dropping, on) != on);
    if changed {
        unsafe {
            let _ = InvalidateRect(window, None, false);
            if let Ok(tiles) = FindWindowExW(window, None, w!("NegsPresetTiles"), None) {
                let _ = InvalidateRect(tiles, None, false);
            }
            if let Ok(foot) = GetDlgItem(window, FOOT as i32) {
                let text = wide(foot_words(on));
                let _ = SetWindowTextW(foot, pcwstr(&text));
                let _ = InvalidateRect(foot, None, false);
            }
        }
    }
}

// MARK: Pointer and keys

fn tile_index(hwnd: HWND) -> Option<usize> {
    let id = unsafe { GetDlgCtrlID(hwnd) } as u16;
    id.checked_sub(TILE_FIRST).map(usize::from)
}

/// The pointer moved with the button down on a tile: past the drag
/// distance it picks the tile up; then the tile moves to where it's held,
/// and the view scrolls near its edges.
fn pointer_moved(app: &mut App, tile: HWND, at: POINT) {
    let Some(p) = &mut app.presets_view else { return };
    if p.drag.is_none() {
        let Some((index, start)) = p.press else { return };
        let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
        if (at.x - start.x).abs() <= dx && (at.y - start.y).abs() <= dy {
            return;
        }
        if !app.model.can_change_presets() {
            return;
        }
        p.drag = Some(Drag { from: index, order: (0..p.files.len()).collect() });
        unsafe {
            SendMessageW(tile, BM_SETSTATE, WPARAM(0), LPARAM(0));
        }
    }
    let mut point = at;
    let mut client = RECT::default();
    unsafe {
        let _ = MapWindowPoints(tile, p.tiles_view, std::slice::from_mut(&mut point));
        let _ = GetClientRect(p.tiles_view, &mut client);
    }
    let margin = (client.bottom / 8).max(px(p.window, 12.0));
    let step = if point.y < margin { -px(p.window, 16.0) } else if point.y > client.bottom - margin { px(p.window, 16.0) } else { 0 };
    if step != 0 {
        let target = p.scroll + step;
        scroll_to(p, target);
    }
    // The slot under the pointer, in the grid as laid out now.
    let Some(over) = tile_at(p.tiles_view, point) else { return };
    let drag = p.drag.as_mut().unwrap();
    let slot_of = |order: &[usize], index: usize| order.iter().position(|&i| i == index);
    let Some(slot) = VIEW.with(|v| v.borrow().rects.get(over).copied()).and_then(|_| slot_of(&drag.order, over)) else { return };
    let Some(from) = slot_of(&drag.order, drag.from) else { return };
    if slot != from {
        let moved = drag.order.remove(from);
        drag.order.insert(slot, moved);
        place(app);
    }
}

/// The button came up: a drag puts the presets in its order.
fn pointer_released(app: &mut App, drop: bool) -> bool {
    let Some(p) = &mut app.presets_view else { return false };
    p.press = None;
    let Some(drag) = p.drag.take() else { return false };
    if drop && drag.order.iter().enumerate().any(|(slot, &i)| slot != i) {
        let order: Vec<PresetFile> = drag.order.iter().filter_map(|&i| p.files.get(i).cloned()).collect();
        // The tile let go of keeps the focus in its new place.
        if let Some(moved) = p.files.get(drag.from).map(PresetFile::name) {
            post(move |app: &mut App| focus_named(app, &moved));
        }
        app.model.move_presets(order);
    }
    place(app);
    true
}

/// Moves the focus from tile `index` by `step` (±1 across, ±2 down).
fn step_focus(app: &mut App, index: usize, step: isize) {
    let Some(p) = &mut app.presets_view else { return };
    let order = order(p);
    let Some(slot) = order.iter().position(|&i| i == index) else { return };
    let next = slot as isize + step;
    if next < 0 || next as usize >= order.len() {
        return;
    }
    let to = order[next as usize];
    set_tab_stop(p, to, true);
    reveal(p, to);
    unsafe {
        let _ = SetFocus(p.tiles[to]);
    }
}

extern "system" fn tile_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        let Some(index) = tile_index(hwnd) else { return DefSubclassProc(hwnd, msg, wparam, lparam) };
        match msg {
            // The view paints the tile: its own paint only asks the view to.
            // The view paints the tile (it isn't WS_CLIPCHILDREN): whatever
            // makes the tile look different asks the view (`repaint_tile`).
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SETFOCUS | WM_KILLFOCUS | WM_UPDATEUISTATE | BM_SETSTATE | WM_ENABLE => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                if msg == WM_SETFOCUS {
                    post(move |app: &mut App| {
                        if let Some(p) = &mut app.presets_view {
                            set_tab_stop(p, index, true);
                            reveal(p, index);
                        }
                    });
                }
                repaint_tile(hwnd);
                result
            }
            WM_LBUTTONDOWN => {
                let at = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                with_app(|app| {
                    if let Some(p) = &mut app.presets_view {
                        p.press = Some((index, at));
                    }
                });
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_MOUSEMOVE if wparam.0 & 0x0001 != 0 => {
                let at = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                let dragging = with_app(|app| {
                    pointer_moved(app, hwnd, at);
                    app.presets_view.as_ref().is_some_and(|p| p.drag.is_some())
                })
                .unwrap_or(false);
                if dragging { LRESULT(0) } else { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
            WM_LBUTTONUP => {
                let dragged = with_app(|app| pointer_released(app, true)).unwrap_or(false);
                if dragged {
                    let _ = ReleaseCapture();
                    LRESULT(0)
                } else {
                    DefSubclassProc(hwnd, msg, wparam, lparam)
                }
            }
            WM_CAPTURECHANGED => {
                // Lost to another window (Alt+Tab): the drag goes back.
                if with_app(|app| pointer_released(app, false)).is_none() {
                    post(|app: &mut App| {
                        pointer_released(app, false);
                    });
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_GETDLGCODE => {
                let mut code = DefSubclassProc(hwnd, msg, wparam, lparam).0 | DLGC_WANTARROWS as isize;
                if lparam.0 != 0 && (*(lparam.0 as *const MSG)).wParam.0 == VK_RETURN.0 as usize {
                    code |= DLGC_WANTMESSAGE as isize;
                }
                LRESULT(code)
            }
            WM_KEYDOWN => {
                let key = VIRTUAL_KEY(wparam.0 as u16);
                let step = match key {
                    VK_LEFT => Some(-1),
                    VK_RIGHT => Some(1),
                    VK_UP => Some(-2),
                    VK_DOWN => Some(2),
                    _ => None,
                };
                if let Some(step) = step {
                    post(move |app: &mut App| step_focus(app, index, step));
                    return LRESULT(0);
                }
                match key {
                    VK_RETURN => post(move |app: &mut App| apply(app, index)),
                    VK_DELETE => post(move |app: &mut App| confirm_delete(app, index)),
                    VK_F2 => post(move |app: &mut App| start_rename(app, index)),
                    _ => return DefSubclassProc(hwnd, msg, wparam, lparam),
                }
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                let at = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                post(move |app: &mut App| context_menu(app, index, at));
                LRESULT(0)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Asks the view to paint `tile`'s place, ring and all, if it's a tile.
pub fn repaint_tile(tile: HWND) {
    unsafe {
        let Ok(view) = GetParent(tile) else { return };
        if draw::class_of(view) != "NegsPresetTiles" {
            return;
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(tile, &mut r);
        let mut corners = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        let _ = MapWindowPoints(None, view, &mut corners);
        let (ox, oy) = (px(view, 12.0), px(view, 10.0));
        let area = RECT { left: corners[0].x - ox, top: corners[0].y - oy, right: corners[1].x + ox, bottom: corners[1].y + oy };
        let _ = InvalidateRect(view, Some(&area), false);
    }
}

extern "system" fn rename_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_GETDLGCODE => LRESULT(DefSubclassProc(hwnd, msg, wparam, lparam).0 | DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                let text = draw::window_text(hwnd);
                post(move |app: &mut App| finish_rename(app, Some(text)));
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                post(|app: &mut App| finish_rename(app, None));
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            WM_KILLFOCUS => {
                // Clicking elsewhere keeps the name, as Return does.
                let text = draw::window_text(hwnd);
                post(move |app: &mut App| finish_rename(app, Some(text)));
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
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

/// The view: each tile on screen, the field's pencil, and the drop ring.
fn paint_tiles(hwnd: HWND, dc: HDC, dirty: RECT) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    VIEW.with(|v| {
        let Ok(v) = v.try_borrow() else { return };
        let mut c = Canvas::new(dc, client);
        c.clear(theme::gdi(t.hex.panel));
        let in_dirty = |r: &RECT| {
            let ring = (12.0 * s) as i32;
            r.bottom - v.scroll + ring >= dirty.top && r.top - v.scroll - ring <= dirty.bottom
        };
        let picture_of = |r: &RECT| {
            let h = ((r.right - r.left) as f64 * 2.0 / 3.0).round() as i32;
            RECT { left: r.left, top: r.top - v.scroll, right: r.right, bottom: r.top - v.scroll + h }
        };
        // The pictures first, through GDI; then words and marks in one pass.
        if v.pictures {
            for (i, r) in v.rects.iter().enumerate() {
                let Some(look) = v.looks.get(i) else { continue };
                if !in_dirty(r) {
                    continue;
                }
                let frame = picture_of(r);
                let image = if look.invalid.is_none() { look.image.as_deref() } else { None };
                super::inspector::draw_thumbnail(&mut c, frame, 0.0, image, look.dimmed, theme::gdi(t.hex.blank), theme::gdi(t.hex.blank));
            }
        }
        let focus = unsafe { GetFocus() };
        c.d2d(s, t.plain, |p| {
            let dip = |v: i32| v as f32 / s;
            for (i, r) in v.rects.iter().enumerate() {
                let Some(look) = v.looks.get(i) else { continue };
                if !in_dirty(r) {
                    continue;
                }
                let tile = HWND(v.tiles.get(i).copied().unwrap_or(0) as *mut _);
                let hot = draw::is_hot(tile);
                let focused = focus == tile && draw::focus_cues(tile);
                let alpha = if v.enabled { 1.0 } else { theme::DISABLED_OPACITY };
                let (x, w) = (dip(r.left), dip(r.right - r.left));
                let mut y = dip(r.top - v.scroll);
                if v.pictures {
                    let frame = picture_of(r);
                    let pic = RectF::new(x, dip(frame.top), w, dip(frame.bottom - frame.top));
                    if look.invalid.is_some() {
                        let m = p.measure("can’t be read", &UNREADABLE, 0.0);
                        p.text("can’t be read", &UNREADABLE, (pic.x + (pic.w - m.width) / 2.0).round(), (pic.y + (pic.h - m.height) / 2.0).round(), Ink::hex(t.hex.dim));
                    }
                    if look.applied {
                        let (ox, oy) = (negs_marks::RING_OUTSET.0 as f32, negs_marks::RING_OUTSET.1 as f32);
                        p.ring(RectF::new(pic.x - ox, pic.y - oy, pic.w + 2.0 * ox, pic.h + 2.0 * oy), negs_marks::Radii::BOX, -1.0, Ink::hex(t.hex.accent).alpha(alpha));
                    }
                    y = pic.y + pic.h + CAPTION_GAP as f32;
                }
                let editing = v.editing.as_ref().filter(|e| e.0 == i);
                if let Some((_, typed)) = editing {
                    // The field's pencil, as wide as the name (`namingUnderline`).
                    let width = (p.measure(typed, &NAME, 0.0).width + 8.0).min(w);
                    p.underline(RectF::new(x, y + NAME_LINE as f32 - 1.0, width, 5.0), &t, false);
                } else {
                    let style = if look.applied { NAME_APPLIED } else { NAME };
                    let ink = if look.invalid.is_some() { t.hex.dim } else if look.applied || hot { t.hex.paper } else { t.hex.label };
                    let shown = fitted(p, &look.name, &style, w);
                    let m = p.measure(&shown, &style, 0.0);
                    p.text(&shown, &style, x, (y + (NAME_LINE as f32 - m.height) / 2.0).round(), Ink::hex(ink).alpha(alpha));
                }
                y += NAME_LINE as f32 + 1.0;
                let dim = Ink::hex(t.hex.dim).alpha(alpha);
                if let Some(reason) = &look.invalid {
                    p.clipped(RectF::new(x, y, w, 2.0 * DETAIL_LINE as f32), |p| {
                        p.text_in(reason, &DETAIL.line_height(1.3), RectF::new(x, y, w, 0.0), Align::Leading, dim);
                    });
                } else if let (Some(film), Some((lead, paper))) = (&look.film, &look.paper) {
                    let shown = fitted(p, film, &DETAIL, w);
                    p.text(&shown, &DETAIL, x, y, dim);
                    let y = y + DETAIL_LINE as f32;
                    let lead_m = p.measure(lead, &DETAIL_LEAD, 0.0);
                    let lead_shown = fitted(p, lead, &DETAIL_LEAD, w);
                    p.text(&lead_shown, &DETAIL_LEAD, x, y, dim);
                    if !paper.is_empty() {
                        let space = p.measure(" ", &DETAIL, 0.0).width;
                        let left = x + lead_m.width + space;
                        let shown = fitted(p, paper, &DETAIL, (x + w - left).max(0.0));
                        let detail_m = p.measure(&shown, &DETAIL, 0.0);
                        p.text(&shown, &DETAIL, left, y + lead_m.baseline - detail_m.baseline, dim);
                    }
                }
                if focused {
                    ink::focus_box(p, RectF::new(x - 5.0, dip(r.top - v.scroll) - 5.0, w + 10.0, dip(r.bottom - r.top) + 10.0), &t);
                }
            }
            if v.dropping {
                // The loose ring round the pane (`PresetsEmpty.dropRing`), 9 in.
                let (w, h) = (dip(client.right), dip(client.bottom));
                p.ring(RectF::new(9.0, 9.0, w - 18.0, h - 18.0), DROP_RING, 0.0, Ink::hex(t.hex.accent));
            }
        });
        unsafe {
            // The name field paints itself.
            if let Ok(field) = GetDlgItem(hwnd, RENAME as i32)
                && IsWindowVisible(field).as_bool()
            {
                let mut r = RECT::default();
                let _ = GetWindowRect(field, &mut r);
                let mut corners = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
                let _ = MapWindowPoints(None, hwnd, &mut corners);
                let _ = ExcludeClipRect(dc, corners[0].x, corners[0].y, corners[1].x, corners[1].y);
            }
        }
        c.present(dc);
    });
}

const DROP_RING: negs_marks::Radii = negs_marks::Radii { horizontal: [18.0, 26.0, 16.0, 24.0], vertical: [22.0, 14.0, 26.0, 16.0], relative: false };

/// The empty pane's sentence broken into centred lines: each line's
/// words, upright or italic.
fn sentence_lines(width: f32, measure: &mut dyn FnMut(&str, &TextStyle) -> f32) -> Vec<Vec<(String, bool)>> {
    let mut lines: Vec<Vec<(String, bool)>> = vec![vec![]];
    let mut line_width = 0.0;
    for (part, upright) in EMPTY_SENTENCE {
        let style = if upright { EMPTY_NEW } else { EMPTY_WORDS };
        // Words with the space after them; a part's start may glue to the last word (", or").
        for word in part.split_inclusive(' ') {
            let w = measure(word.trim_end(), &style);
            let advance = measure(word, &style);
            let glued = !word.starts_with(' ') && lines.last().unwrap().last().is_some_and(|(w, _)| !w.ends_with(' '));
            if line_width + w > width && line_width > 0.0 && !glued {
                lines.push(vec![]);
                line_width = 0.0;
            }
            lines.last_mut().unwrap().push((word.to_string(), upright));
            line_width += advance;
        }
    }
    lines
}

/// The sentence's line height: its size and 3 more (`lineSpacing(3)`),
/// from the height of a word of it.
fn sentence_line(word_height: f32) -> f32 {
    word_height + 3.0
}

fn paint_drawn(item: &DRAWITEMSTRUCT) {
    let hwnd = item.hwndItem;
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let r = item.rcItem;
    let (w, h) = ((r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s);
    let dropping = VIEW.with(|v| v.borrow().dropping);
    let mut c = Canvas::new(item.hDC, r);
    c.clear(theme::gdi(t.hex.panel));
    let id = unsafe { GetDlgCtrlID(hwnd) } as u16;
    c.d2d(s, t.plain, |p| match id {
        EMPTY_TITLE => {
            let m = p.measure("No presets yet", &EMPTY_HEAD, 0.0);
            p.text("No presets yet", &EMPTY_HEAD, ((w - m.width) / 2.0).round(), 0.0, Ink::hex(t.hex.paper));
        }
        EMPTY_TEXT => {
            let line = sentence_line(p.measure("Save", &EMPTY_WORDS, 0.0).height);
            let lines = sentence_lines(w, &mut |text, style| p.measure(text, style, 0.0).width);
            for (n, words) in lines.iter().enumerate() {
                let style = |upright: bool| if upright { EMPTY_NEW } else { EMPTY_WORDS };
                let total: f32 = words.iter().enumerate().map(|(i, (word, upright))| {
                    let text = if i + 1 == words.len() { word.trim_end() } else { word.as_str() };
                    p.measure(text, &style(*upright), 0.0).width
                }).sum();
                let mut x = ((w - total) / 2.0).round();
                let y = n as f32 * line;
                let base = p.measure("Save", &EMPTY_WORDS, 0.0).baseline;
                for (word, upright) in words {
                    let m = p.measure(word, &style(*upright), 0.0);
                    let ink = if *upright { t.hex.label } else { t.hex.dim };
                    p.text(word, &style(*upright), x, y + base - m.baseline, Ink::hex(ink));
                    x += m.width;
                }
            }
        }
        FOOT => {
            // Under a rule, centred.
            p.fill_rect(RectF::new(0.0, 0.0, w, 1.0), Ink::hex(t.hex.line));
            let words = foot_words(dropping);
            let shown = fitted(p, words, &FOOT_WORDS, w - 16.0);
            let m = p.measure(&shown, &FOOT_WORDS, 0.0);
            p.text(&shown, &FOOT_WORDS, ((w - m.width) / 2.0).round(), ((h - m.height) / 2.0 + 0.5).round(), Ink::hex(if dropping { t.hex.paper } else { t.hex.dim }));
        }
        _ => {}
    });
    c.present(item.hDC);
}

extern "system" fn tiles_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if id == RENAME && code == EN_CHANGE {
                    let typed = draw::window_text(HWND(lparam.0 as *mut _));
                    VIEW.with(|v| {
                        if let Some(e) = &mut v.borrow_mut().editing {
                            e.1 = typed;
                        }
                    });
                    let _ = InvalidateRect(hwnd, None, false);
                } else if code == BN_CLICKED
                    && let Some(index) = tile_index(HWND(lparam.0 as *mut _))
                {
                    with_app(|app| apply(app, index));
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.code == NM_CUSTOMDRAW && tile_index(header.hwndFrom).is_some() {
                    // The view paints the tiles.
                    return LRESULT(CDRF_SKIPDEFAULT as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                with_app(|app| {
                    if let Some(p) = &mut app.presets_view {
                        let target = p.scroll - delta * px(hwnd, 60.0) / 120;
                        scroll_to(p, target);
                    }
                });
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                let dc = HDC(wparam.0 as *mut _);
                let t = theme::tokens();
                SetTextColor(dc, COLORREF(theme::gdi(t.hex.paper)));
                SetBkColor(dc, COLORREF(theme::gdi(t.hex.panel)));
                LRESULT(theme::brush(theme::gdi(t.hex.panel)).0 as isize)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_tiles(hwnd, dc, ps.rcPaint);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                if (wparam.0 & 0xffff) as u16 == EMPTY_IMPORT && ((wparam.0 >> 16) & 0xffff) as u32 == BN_CLICKED {
                    with_app(|app| app.command(id::IMPORT_PRESETS));
                }
                LRESULT(0)
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                if !ink::draw_item(item) {
                    paint_drawn(item);
                }
                LRESULT(1)
            }
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_SIZE => {
                if with_app(place).is_none() {
                    post(place);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                // The rule along the top; with no presets, the drop ring round the pane.
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let t = theme::tokens();
                let s = draw::dpi_of(hwnd);
                let dropping = VIEW.with(|v| v.borrow().dropping);
                let mut c = Canvas::new(dc, client);
                c.clear(theme::gdi(t.hex.panel));
                let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
                c.d2d(s, t.plain, |p| {
                    p.fill_rect(RectF::new(0.0, 0.0, w, 1.0), Ink::hex(t.hex.line));
                    if dropping {
                        p.ring(RectF::new(9.0, 9.0, w - 18.0, h - 18.0), DROP_RING, 0.0, Ink::hex(t.hex.accent));
                    }
                });
                c.present(dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The thumbnails changed: the tiles show the model's.
pub fn redraw_previews(app: &mut App) {
    if app.presets_view.is_some() {
        sync(app);
    }
}

/// Whether the column shows the presets: not while cropping, which needs
/// the crop panel.
pub fn shown(model: &Model) -> bool {
    model.inspector_visible && model.pane() == Pane::Presets && !model.cropping()
}

/// After a theme change: the words may set in another font.
pub fn retheme(app: &mut App) {
    if app.presets_view.is_some() {
        place(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "click to apply · drag to sort · drop files to import", or "drop to import".
    #[test]
    fn the_foot_says_what_a_drop_does() {
        assert_eq!(foot_words(false), "click to apply · drag to sort · drop files to import");
        assert_eq!(foot_words(true), "drop to import");
    }

    #[test]
    fn a_tile_speaks_its_film_and_print() {
        let look = Look { name: "Evening".into(), film: Some("Portra 400".into()), paper: Some(("printed on", "Endura".into())), applied: true, ..Default::default() };
        assert_eq!(spoken(&look), "Applied last. Portra 400, printed on Endura");
        let scan = Look { film: Some("Velvia 50".into()), paper: Some(("not printed · film scan", String::new())), ..Default::default() };
        assert_eq!(spoken(&scan), "Velvia 50, not printed · film scan");
    }
}
