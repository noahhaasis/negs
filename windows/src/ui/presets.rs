//! The Presets panel (`PresetInspector.swift`), in the adjustments column in
//! place of the adjustments: a header with New Preset and a "…" menu, and a
//! report-view ListView of the presets with the film → paper below each name.
//! Clicking a row applies it; double-click or Enter applies it again. Rows
//! drag to reorder, files dropped on the panel import, and F2 or the context
//! menu renames inline. Created the first time it is shown.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PWSTR, w};

use super::{App, create, id, post, with_app};
use crate::model::{Model, Pane};
use crate::platform::{pcwstr, wide};
use crate::presets::{PresetFile, Status, name};

const LIST: u16 = 1;
const ADD: u16 = 2;
const MORE: u16 = 3;
const EMPTY_IMPORT: u16 = 4;

/// A row as drawn: what the model says about one file.
#[derive(Clone, PartialEq)]
struct Row {
    file: PresetFile,
    name: String,
    detail: String,
    valid: bool,
    /// The engine refuses the file: greyed.
    refused: bool,
}

pub struct Panel {
    pub window: HWND,
    title: HWND,
    add: HWND,
    more: HWND,
    list: HWND,
    empty_icon: HWND,
    empty_title: HWND,
    empty_text: HWND,
    empty_import: HWND,
    icon_font: HFONT,
    /// The rows the list holds.
    shown: Vec<Row>,
    /// The row the mouse just selected: its click applies it, a drag doesn't.
    click_selected: Option<usize>,
    /// The row being dragged to a new place.
    drag: Option<usize>,
}

thread_local! {
    /// The rows and fonts for drawing, which happens outside the app's borrow.
    static ROWS: RefCell<Vec<Row>> = const { RefCell::new(Vec::new()) };
    static FONTS: Cell<(isize, isize)> = const { Cell::new((0, 0)) };
    /// Where a dragged row or dropped files would go: before this row.
    static INSERT: Cell<Option<usize>> = const { Cell::new(None) };
    /// Files are dragged over the panel.
    static DROP_HIGHLIGHT: Cell<bool> = const { Cell::new(false) };
    /// A finished label edit waits to reach the model; don't start another.
    static EDIT_PENDING: Cell<bool> = const { Cell::new(false) };
    /// The row whose name is being edited: its name isn't drawn under the field.
    static EDITING: Cell<Option<usize>> = const { Cell::new(None) };
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsPresets"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

fn icon_font(dpi: u32) -> HFONT {
    let mut lf = LOGFONTW { lfHeight: -(36 * dpi as i32 / 96), lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
    let face = wide("Segoe MDL2 Assets");
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    unsafe { CreateFontIndirectW(&lf) }
}

fn build(app: &mut App) {
    let fonts = &app.fonts;
    let window = create(w!("NegsPresets"), "Presets", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), app.main, 0, HFONT::default());
    let title = create(w!("STATIC"), "Presets", WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0 | SS_CENTERIMAGE.0), WINDOW_EX_STYLE(0), window, 0, fonts.bold);
    let button = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32);
    let add = create(w!("BUTTON"), "\u{E710}", button, WINDOW_EX_STYLE(0), window, ADD, fonts.glyph_small);
    super::accessible_name(add, "New Preset");
    super::dialogs::tooltip(add, "Save the film, paper and Lab settings as a preset (Ctrl+Shift+S)");
    let more = create(w!("BUTTON"), "\u{E712}", button, WINDOW_EX_STYLE(0), window, MORE, fonts.glyph_small);
    super::accessible_name(more, "More");
    super::dialogs::tooltip(more, "Import presets or show their folder");
    let style = WS_CHILD | WS_TABSTOP | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_EDITLABELS | LVS_NOCOLUMNHEADER);
    let list = create(WC_LISTVIEWW, "Presets", style, WINDOW_EX_STYLE(0), window, LIST, fonts.ui);
    unsafe {
        let extended = LVS_EX_FULLROWSELECT | LVS_EX_INFOTIP | LVS_EX_DOUBLEBUFFER;
        SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, WPARAM(extended as usize), LPARAM(extended as isize));
        let column = LVCOLUMNW { mask: LVCF_WIDTH, cx: 100, ..Default::default() };
        SendMessageW(list, LVM_INSERTCOLUMNW, WPARAM(0), LPARAM(&column as *const _ as isize));
        let _ = SetWindowSubclass(list, Some(list_subclass), 1, 0);
    }
    let center = WS_CHILD | WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0);
    let icon_font = icon_font(app.dpi);
    let empty_icon = create(w!("STATIC"), "\u{E81E}", center, WINDOW_EX_STYLE(0), window, 0, icon_font);
    let empty_title = create(w!("STATIC"), "No Presets", center, WINDOW_EX_STYLE(0), window, 0, fonts.bold);
    let empty_text = create(
        w!("STATIC"),
        "Save the film, paper and Lab settings with +, or import presets made in Negs or spektrafilm. You can also drop them here.",
        center,
        WINDOW_EX_STYLE(0),
        window,
        0,
        fonts.ui,
    );
    let empty_import = create(w!("BUTTON"), "Import Presets…", WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), window, EMPTY_IMPORT, fonts.ui);
    app.presets_view = Some(Panel {
        window,
        title,
        add,
        more,
        list,
        empty_icon,
        empty_title,
        empty_text,
        empty_import,
        icon_font,
        shown: vec![],
        click_selected: None,
        drag: None,
    });
    set_metrics(app);
    super::drop::register_presets(window, list);
}

/// Fonts for drawing, and the row height: a one-pixel-wide image list of
/// that height is how a report view gets taller rows.
fn set_metrics(app: &mut App) {
    let Some(p) = &app.presets_view else { return };
    FONTS.with(|f| f.set((app.fonts.ui.0 as isize, app.fonts.small.0 as isize)));
    unsafe {
        let images = ImageList_Create(1, app.px(42.0), ILC_COLOR, 1, 0);
        let old = SendMessageW(p.list, LVM_SETIMAGELIST, WPARAM(LVSIL_SMALL as usize), LPARAM(images.0));
        if old.0 != 0 {
            let _ = ImageList_Destroy(HIMAGELIST(old.0));
        }
    }
}

pub fn dpi_changed(app: &mut App) {
    let Some(p) = &mut app.presets_view else { return };
    let icon = icon_font(app.dpi);
    unsafe {
        let fonts = &app.fonts;
        for (c, f) in [(p.title, fonts.bold), (p.add, fonts.glyph_small), (p.more, fonts.glyph_small), (p.list, fonts.ui), (p.empty_title, fonts.bold), (p.empty_text, fonts.ui), (p.empty_import, fonts.ui), (p.empty_icon, icon)] {
            SendMessageW(c, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
        }
        let _ = DeleteObject(std::mem::replace(&mut p.icon_font, icon));
    }
    set_metrics(app);
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
    let px = |dip: f64| app.px(dip);
    let (pad, header, button) = (px(16.0), px(44.0), px(28.0));
    unsafe {
        let top = (header - button) / 2;
        let _ = MoveWindow(p.title, pad, top, w - 2 * pad - 2 * button - px(6.0), button, true);
        let _ = MoveWindow(p.more, w - pad - button, top, button, button, true);
        let _ = MoveWindow(p.add, w - pad - 2 * button - px(6.0), top, button, button, true);
        let _ = MoveWindow(p.list, 0, header, w, (h - header).max(0), true);
        let width = (w - 2 * px(24.0)).max(px(100.0));
        let x = (w - width) / 2;
        let mut y = header + ((h - header) - px(200.0)).max(0) / 3;
        for (control, height) in [(p.empty_icon, 52.0), (p.empty_title, 26.0), (p.empty_text, 84.0)] {
            let _ = MoveWindow(control, x, y, width, px(height), true);
            y += px(height);
        }
        let _ = MoveWindow(p.empty_import, (w - px(150.0)) / 2, y + px(6.0), px(150.0), px(30.0), true);
        fit_column(p.list);
    }
}

fn fit_column(list: HWND) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(list, &mut client);
        SendMessageW(list, LVM_SETCOLUMNWIDTH, WPARAM(0), LPARAM(client.right as isize));
    }
}

fn row(model: &Model, file: &PresetFile) -> Row {
    let stock = |id: &str, paper: bool| model.stocks.iter().find(|s| s.paper == paper && s.id == id).map_or_else(|| id.to_owned(), |s| s.name.clone());
    let status = model.preset_status.get(file);
    let (detail, valid) = match status {
        Some(Status::Valid { film, paper, scan }) => {
            (if *scan { format!("{} · film scan", stock(film, false)) } else { format!("{} → {}", stock(film, false), stock(paper, true)) }, true)
        }
        Some(Status::Invalid(reason)) => (reason.clone(), false),
        _ => (String::new(), false),
    };
    Row { file: file.clone(), name: file.name(), detail, valid, refused: matches!(status, Some(Status::Invalid(_))) }
}

fn editing(list: HWND) -> bool {
    unsafe { SendMessageW(list, LVM_GETEDITCONTROL, WPARAM(0), LPARAM(0)).0 != 0 }
}

fn selected(list: HWND) -> Option<usize> {
    let index = unsafe { SendMessageW(list, LVM_GETNEXTITEM, WPARAM(usize::MAX), LPARAM(LVNI_SELECTED as isize)).0 };
    (index >= 0).then_some(index as usize)
}

fn select(list: HWND, index: usize) {
    let item = LVITEMW { stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0), state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0), ..Default::default() };
    unsafe {
        SendMessageW(list, LVM_SETITEMSTATE, WPARAM(index), LPARAM(&item as *const _ as isize));
        SendMessageW(list, LVM_ENSUREVISIBLE, WPARAM(index), LPARAM(0));
    }
}

/// Brings the panel up to date with the model: the rows, the empty state,
/// whether it can change presets, and a rename New Preset asked for.
pub fn sync(app: &mut App) {
    let Some(p) = &mut app.presets_view else { return };
    // Hidden: brought up to date when it shows (a layout change).
    if !unsafe { IsWindowVisible(p.window) }.as_bool() {
        return;
    }
    let model = &app.model;
    let rows: Vec<Row> = model.presets.iter().map(|f| row(model, f)).collect();
    let enabled = model.can_change_presets();
    let empty = rows.is_empty();
    unsafe {
        for c in [p.empty_icon, p.empty_title, p.empty_text, p.empty_import] {
            let _ = ShowWindow(c, if empty { SW_SHOWNA } else { SW_HIDE });
        }
        let _ = ShowWindow(p.list, if empty { SW_HIDE } else { SW_SHOWNA });
        for c in [p.add, p.more, p.empty_import, p.list] {
            let _ = EnableWindow(c, enabled);
        }
    }
    if rows != p.shown && !editing(p.list) {
        let same_files = rows.len() == p.shown.len() && rows.iter().zip(&p.shown).all(|(a, b)| a.file.path == b.file.path && a.name == b.name);
        if !same_files {
            let kept = selected(p.list).and_then(|i| p.shown.get(i)).map(|r| r.file.path.clone());
            unsafe {
                SendMessageW(p.list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
                for (i, r) in rows.iter().enumerate() {
                    let mut text = wide(&r.name);
                    let item = LVITEMW { mask: LVIF_TEXT, iItem: i as i32, pszText: PWSTR(text.as_mut_ptr()), ..Default::default() };
                    SendMessageW(p.list, LVM_INSERTITEMW, WPARAM(0), LPARAM(&item as *const _ as isize));
                }
            }
            if let Some(i) = kept.and_then(|path| rows.iter().position(|r| r.file.path == path)) {
                select(p.list, i);
            }
            p.click_selected = None;
        }
        ROWS.with(|r| *r.borrow_mut() = rows.clone());
        p.shown = rows;
        fit_column(p.list);
        unsafe {
            let _ = InvalidateRect(p.list, None, false);
        }
    }
    // New Preset: rename the new row at once.
    let renaming = model.renaming_preset.as_ref().and_then(|path| p.shown.iter().position(|r| &r.file.path == path));
    if let Some(index) = renaming {
        let shown = unsafe { IsWindowVisible(p.list).as_bool() };
        if shown && !editing(p.list) && !EDIT_PENDING.with(Cell::get) {
            select(p.list, index);
            unsafe {
                let _ = SetFocus(p.list);
                SendMessageW(p.list, LVM_EDITLABELW, WPARAM(index), LPARAM(0));
            }
        }
    }
}

/// Applies the row if the engine reads it.
fn apply(app: &mut App, index: usize) {
    let Some(r) = app.presets_view.as_ref().and_then(|p| p.shown.get(index)).cloned() else { return };
    if r.valid {
        app.model.apply_preset(&r.file);
    }
}

fn file_at(app: &App, index: usize) -> Option<PresetFile> {
    app.presets_view.as_ref()?.shown.get(index).map(|r| r.file.clone())
}

fn confirm_delete(app: &mut App, index: usize) {
    let Some(file) = file_at(app, index) else { return };
    if super::dialogs::confirm_delete_preset(app.main, &file.name()) {
        app.model.delete_preset(&file);
    }
}

fn rename(app: &mut App, index: usize) {
    let Some(p) = &app.presets_view else { return };
    unsafe {
        let _ = SetFocus(p.list);
        SendMessageW(p.list, LVM_EDITLABELW, WPARAM(index), LPARAM(0));
    }
}

/// A label edit ended: `text` is None after Escape. An invalid name keeps
/// the old one and says why; Escape on New Preset's row removes it.
fn finish_rename(app: &mut App, file: PresetFile, text: Option<String>) {
    EDIT_PENDING.with(|e| e.set(false));
    let Some(text) = text else { return app.model.cancel_rename(&file) };
    let typed = text.trim();
    match name::problem(typed) {
        None => app.model.rename_preset(&file, typed),
        Some(problem) => {
            let old = file.name();
            app.model.rename_preset(&file, &old);
            if !typed.is_empty() {
                let (main, instruction) = (app.main.0 as isize, format!("“{typed}” can’t be used as a preset name."));
                post(move |_| super::dialogs::notice(HWND(main as *mut _), &instruction, &problem));
            }
        }
    }
}

fn context_menu(app: &mut App, screen: POINT) {
    let Some(p) = &app.presets_view else { return };
    let list = p.list;
    let mut at = screen;
    let index = if screen.x == -1 && screen.y == -1 {
        // From the keyboard: under the selected row.
        let index = selected(list);
        if let Some(i) = index {
            let mut r = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
            unsafe {
                SendMessageW(list, LVM_GETITEMRECT, WPARAM(i), LPARAM(&mut r as *mut _ as isize));
                at = POINT { x: r.left + app.px(24.0), y: r.bottom };
                let _ = ClientToScreen(list, &mut at);
            }
        }
        index
    } else {
        hit(list, screen)
    };
    let Some(index) = index else { return };
    let valid = p.shown.get(index).is_some_and(|r| r.valid) && app.model.can_change_presets();
    let choice = unsafe {
        let menu = CreatePopupMenu().unwrap();
        let add = |cmd: u32, text: &str, on: bool| {
            let w = wide(text);
            let _ = AppendMenuW(menu, MF_STRING | if on { MF_ENABLED } else { MF_GRAYED }, cmd as usize, pcwstr(&w));
        };
        add(1, "&Apply", valid);
        add(2, "&Rename\tF2", true);
        add(3, "&Export…", true);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        add(4, "&Delete…\tDel", true);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, app.main, None).0;
        let _ = DestroyMenu(menu);
        choice
    };
    match choice {
        1 => apply(app, index),
        2 => rename(app, index),
        3 => {
            if let Some(file) = file_at(app, index) {
                super::dialogs::export_preset(app, &file);
            }
        }
        4 => confirm_delete(app, index),
        _ => {}
    }
}

/// The "…" button's menu, under it.
fn more_menu(app: &mut App) {
    let Some(p) = &app.presets_view else { return };
    let mut r = RECT::default();
    let choice = unsafe {
        let _ = GetWindowRect(p.more, &mut r);
        let menu = CreatePopupMenu().unwrap();
        for (cmd, text) in [(id::IMPORT_PRESETS, "&Import Presets…\tCtrl+Shift+O"), (id::PRESETS_FOLDER, "&Show Presets Folder")] {
            let w = wide(text);
            let _ = AppendMenuW(menu, MF_STRING, cmd as usize, pcwstr(&w));
        }
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTALIGN, r.right, r.bottom, 0, app.main, None).0;
        let _ = DestroyMenu(menu);
        choice
    };
    if choice != 0 {
        app.command(choice as u16);
    }
}

/// The row under a point on the screen.
fn hit(list: HWND, screen: POINT) -> Option<usize> {
    let mut info = LVHITTESTINFO { pt: screen, ..Default::default() };
    unsafe {
        let _ = ScreenToClient(list, &mut info.pt);
        let index = SendMessageW(list, LVM_HITTEST, WPARAM(0), LPARAM(&mut info as *mut _ as isize)).0;
        (index >= 0).then_some(index as usize)
    }
}

/// Where a drop at `screen` goes: before the row whose upper half it is in,
/// or after the last row.
pub fn gap(list: HWND, screen: POINT) -> usize {
    let count = ROWS.with(|r| r.borrow().len());
    let mut point = screen;
    unsafe {
        let _ = ScreenToClient(list, &mut point);
    }
    for i in 0..count {
        let mut r = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
        unsafe {
            SendMessageW(list, LVM_GETITEMRECT, WPARAM(i), LPARAM(&mut r as *mut _ as isize));
        }
        if point.y < (r.top + r.bottom) / 2 {
            return i;
        }
    }
    count
}

/// Shows where a drag would go, None to hide it.
pub fn show_insert(list: HWND, at: Option<usize>) {
    if INSERT.with(|i| i.replace(at)) != at {
        unsafe {
            let _ = InvalidateRect(list, None, false);
        }
    }
}

pub fn highlight_drop(window: HWND, on: bool) {
    if DROP_HIGHLIGHT.with(|h| h.replace(on)) != on {
        unsafe {
            let _ = InvalidateRect(window, None, true);
        }
    }
}

/// Scrolls while a drag is near the list's top or bottom edge.
fn scroll_near_edge(list: HWND, y: i32) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(list, &mut client);
        let margin = (client.bottom / 8).max(12);
        let step = if y < margin { -20 } else if y > client.bottom - margin { 20 } else { 0 };
        if step != 0 {
            SendMessageW(list, LVM_SCROLL, WPARAM(0), LPARAM(step));
        }
    }
}

fn drag_moved(app: &mut App) {
    let Some(p) = &app.presets_view else { return };
    if p.drag.is_none() {
        return;
    }
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let mut local = cursor;
        let _ = ScreenToClient(p.list, &mut local);
        scroll_near_edge(p.list, local.y);
    }
    show_insert(p.list, Some(gap(p.list, cursor)));
}

fn drag_ended(app: &mut App, drop: bool) {
    let Some(p) = &mut app.presets_view else { return };
    let Some(from) = p.drag.take() else { return };
    let to = INSERT.with(|i| i.get());
    show_insert(p.list, None);
    let Some(to) = to.filter(|_| drop) else { return };
    if to == from || to == from + 1 {
        return;
    }
    let mut order: Vec<PresetFile> = p.shown.iter().map(|r| r.file.clone()).collect();
    let moved = order.remove(from);
    order.insert(if to > from { to - 1 } else { to }, moved);
    app.model.move_presets(order);
}

extern "system" fn list_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            // Enter applies, not the window's default button.
            WM_GETDLGCODE if lparam.0 != 0 && (*(lparam.0 as *const MSG)).wParam.0 == VK_RETURN.0 as usize => {
                LRESULT(DefSubclassProc(hwnd, msg, wparam, lparam).0 | DLGC_WANTMESSAGE as isize)
            }
            WM_KEYDOWN if [VK_RETURN, VK_DELETE, VK_F2].iter().any(|k| wparam.0 == k.0 as usize) => {
                let key = VIRTUAL_KEY(wparam.0 as u16);
                let Some(index) = selected(hwnd) else { return LRESULT(0) };
                post(move |app: &mut App| match key {
                    VK_RETURN => apply(app, index),
                    VK_DELETE => confirm_delete(app, index),
                    _ => rename(app, index),
                });
                LRESULT(0)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Keeps the label edit where `LVN_BEGINLABELEDITW` placed it: `data` holds
/// its x and y.
extern "system" fn edit_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, data: usize) -> LRESULT {
    unsafe {
        if msg == WM_WINDOWPOSCHANGING {
            let pos = &mut *(lparam.0 as *mut WINDOWPOS);
            if pos.flags.0 & SWP_NOMOVE.0 == 0 {
                pos.x = (data >> 32) as u32 as i32;
                pos.y = data as u32 as i32;
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// Draws a row: the name, and the film → paper or why the file is refused
/// below it, greyed when refused.
fn draw_row(draw: &NMLVCUSTOMDRAW) {
    let index = draw.nmcd.dwItemSpec;
    let Some(r) = ROWS.with(|rows| rows.borrow().get(index).cloned()) else { return };
    let list = draw.nmcd.hdr.hwndFrom;
    let dc = draw.nmcd.hdc;
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(list).max(96) as i32;
        let px = |dip: i32| dip * dpi / 96;
        let mut bounds = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
        SendMessageW(list, LVM_GETITEMRECT, WPARAM(index), LPARAM(&mut bounds as *mut _ as isize));
        let state = SendMessageW(list, LVM_GETITEMSTATE, WPARAM(index), LPARAM(LVIS_SELECTED.0 as isize)).0 as u32;
        let chosen = state & LVIS_SELECTED.0 != 0;
        let focused = GetFocus() == list;
        let (background, text, detail) = match (chosen, focused) {
            (true, true) => (GetSysColor(COLOR_HIGHLIGHT), GetSysColor(COLOR_HIGHLIGHTTEXT), GetSysColor(COLOR_HIGHLIGHTTEXT)),
            (true, false) => (0xe5e5e5, GetSysColor(COLOR_WINDOWTEXT), 0x606060),
            _ => (GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT), 0x707070),
        };
        let text = if !r.refused || (chosen && focused) { text } else { GetSysColor(COLOR_GRAYTEXT) };
        let brush = CreateSolidBrush(COLORREF(background));
        FillRect(dc, &bounds, brush);
        let _ = DeleteObject(brush);
        SetBkMode(dc, TRANSPARENT);
        let (ui, small) = FONTS.with(Cell::get);
        let inner = RECT { left: bounds.left + px(16), right: bounds.right - px(12), ..bounds };
        let middle = (bounds.top + bounds.bottom) / 2;
        let old = SelectObject(dc, HGDIOBJ(ui as *mut _));
        SetTextColor(dc, COLORREF(text));
        if EDITING.with(Cell::get) != Some(index) {
            draw_line(dc, &r.name, RECT { top: bounds.top + px(3), bottom: middle + px(1), ..inner }, DT_BOTTOM);
        }
        SelectObject(dc, HGDIOBJ(small as *mut _));
        SetTextColor(dc, COLORREF(detail));
        draw_line(dc, &r.detail, RECT { top: middle + px(2), bottom: bounds.bottom - px(3), ..inner }, DT_TOP);
        SelectObject(dc, old);
    }
}

/// One line, cut with an ellipsis. Nothing for empty text: `DrawTextW`
/// reads an empty slice's dangling pointer.
fn draw_line(dc: HDC, text: &str, mut rect: RECT, align: DRAW_TEXT_FORMAT) {
    if text.is_empty() {
        return;
    }
    let mut text: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        DrawTextW(dc, &mut text, &mut rect, DT_LEFT | align | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
    }
}

/// The line where a dragged row or dropped files would go.
fn draw_insert(list: HWND, dc: HDC) {
    let Some(at) = INSERT.with(Cell::get) else { return };
    let count = ROWS.with(|r| r.borrow().len());
    if count == 0 {
        return;
    }
    unsafe {
        let mut r = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
        SendMessageW(list, LVM_GETITEMRECT, WPARAM(at.min(count - 1)), LPARAM(&mut r as *mut _ as isize));
        let y = if at >= count { r.bottom } else { r.top };
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(list).max(96) as i32;
        let accent = super::canvas::accent_color();
        let brush = CreateSolidBrush(COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16));
        let thick = 2 * dpi / 96;
        let line = RECT { left: r.left + 8 * dpi / 96, top: y - thick / 2 - thick % 2, right: r.right - 8 * dpi / 96, bottom: y + thick / 2 };
        FillRect(dc, &line, brush);
        let _ = DeleteObject(brush);
    }
}

fn notify(hwnd: HWND, header: &NMHDR, lparam: LPARAM) -> Option<LRESULT> {
    unsafe {
        match header.code {
            NM_CUSTOMDRAW => {
                let draw = &*(lparam.0 as *const NMLVCUSTOMDRAW);
                Some(LRESULT(match draw.nmcd.dwDrawStage {
                    CDDS_PREPAINT => (CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize,
                    CDDS_ITEMPREPAINT => {
                        draw_row(draw);
                        CDRF_SKIPDEFAULT as isize
                    }
                    CDDS_POSTPAINT => {
                        draw_insert(header.hwndFrom, draw.nmcd.hdc);
                        CDRF_DODEFAULT as isize
                    }
                    _ => CDRF_DODEFAULT as isize,
                }))
            }
            LVN_ITEMCHANGED => {
                let change = &*(lparam.0 as *const NMLISTVIEW);
                let now = change.uNewState & LVIS_SELECTED.0 != 0 && change.uOldState & LVIS_SELECTED.0 == 0;
                if now && change.iItem >= 0 {
                    let index = change.iItem as usize;
                    let mouse = GetKeyState(VK_LBUTTON.0 as i32) < 0 || GetKeyState(VK_RBUTTON.0 as i32) < 0;
                    with_app(|app| {
                        if mouse {
                            if let Some(p) = &mut app.presets_view {
                                p.click_selected = (GetKeyState(VK_LBUTTON.0 as i32) < 0).then_some(index);
                            }
                        } else {
                            // Arrow keys: the selected preset applies.
                            apply(app, index);
                        }
                    });
                }
                Some(LRESULT(0))
            }
            NM_CLICK => {
                let click = &*(lparam.0 as *const NMITEMACTIVATE);
                with_app(|app| {
                    let pending = app.presets_view.as_mut().and_then(|p| p.click_selected.take());
                    if click.iItem >= 0 && pending == Some(click.iItem as usize) {
                        apply(app, click.iItem as usize);
                    }
                });
                Some(LRESULT(0))
            }
            NM_DBLCLK => {
                let click = &*(lparam.0 as *const NMITEMACTIVATE);
                if click.iItem >= 0 {
                    with_app(|app| apply(app, click.iItem as usize));
                }
                Some(LRESULT(0))
            }
            LVN_BEGINDRAG => {
                let change = &*(lparam.0 as *const NMLISTVIEW);
                with_app(|app| {
                    if let Some(p) = &mut app.presets_view {
                        p.click_selected = None;
                        if app.model.can_change_presets() {
                            p.drag = Some(change.iItem as usize);
                            SetCapture(hwnd);
                        }
                    }
                });
                Some(LRESULT(0))
            }
            LVN_BEGINLABELEDITW => {
                let allowed = with_app(|app| app.model.can_change_presets()).unwrap_or(true);
                if allowed {
                    let info = &*(lparam.0 as *const NMLVDISPINFOW);
                    let list = header.hwndFrom;
                    let edit = HWND(SendMessageW(list, LVM_GETEDITCONTROL, WPARAM(0), LPARAM(0)).0 as *mut _);
                    SendMessageW(edit, EM_SETLIMITTEXT, WPARAM(name::MAX_LENGTH + 20), LPARAM(0));
                    // On the name's line, not centred in the two-line row.
                    let mut r = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
                    SendMessageW(list, LVM_GETITEMRECT, WPARAM(info.item.iItem.max(0) as usize), LPARAM(&mut r as *mut _ as isize));
                    let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(list).max(96) as i32;
                    let (x, y) = (r.left + 13 * dpi / 96, r.top + 3 * dpi / 96);
                    let _ = SetWindowSubclass(edit, Some(edit_subclass), 2, (x as u32 as usize) << 32 | y as u32 as usize);
                    EDITING.with(|e| e.set(Some(info.item.iItem.max(0) as usize)));
                    let _ = InvalidateRect(list, Some(&r), false);
                }
                Some(LRESULT(!allowed as isize))
            }
            LVN_ENDLABELEDITW => {
                let info = &*(lparam.0 as *const NMLVDISPINFOW);
                EDITING.with(|e| e.set(None));
                let _ = InvalidateRect(header.hwndFrom, None, false);
                let text = (!info.item.pszText.is_null()).then(|| info.item.pszText.to_string().unwrap_or_default());
                let file = ROWS.with(|r| r.borrow().get(info.item.iItem.max(0) as usize).map(|r| r.file.clone()));
                if let Some(file) = file {
                    EDIT_PENDING.with(|e| e.set(true));
                    let mut work = Some((file, text));
                    // Ended by the app's own change (focus moved): later.
                    if with_app(|app| {
                        let (file, text) = work.take().unwrap();
                        finish_rename(app, file, text);
                    })
                    .is_none()
                    {
                        let (file, text) = work.take().unwrap();
                        post(move |app: &mut App| finish_rename(app, file, text));
                    }
                }
                // The list shows the name on disk after the rename.
                Some(LRESULT(0))
            }
            LVN_GETINFOTIPW => {
                let tip = &mut *(lparam.0 as *mut NMLVGETINFOTIPW);
                let text = ROWS.with(|r| r.borrow().get(tip.iItem.max(0) as usize).map(|r| if r.detail.is_empty() { r.name.clone() } else { format!("{}\n{}", r.name, r.detail) }));
                if let (Some(text), false) = (text, tip.pszText.is_null()) {
                    let w: Vec<u16> = text.encode_utf16().take(tip.cchTextMax.max(1) as usize - 1).chain(Some(0)).collect();
                    std::ptr::copy_nonoverlapping(w.as_ptr(), tip.pszText.0, w.len());
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let command = (wparam.0 & 0xffff) as u16;
                match command {
                    ADD => {
                        with_app(|app| app.command(id::NEW_PRESET));
                    }
                    MORE => {
                        with_app(more_menu);
                    }
                    EMPTY_IMPORT => {
                        with_app(|app| app.command(id::IMPORT_PRESETS));
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.idFrom == LIST as usize {
                    if let Some(result) = notify(hwnd, header, lparam) {
                        return result;
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CONTEXTMENU => {
                let point = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                with_app(|app| {
                    if app.presets_view.as_ref().is_some_and(|p| p.list.0 as usize == wparam.0) {
                        context_menu(app, point);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                with_app(drag_moved);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|app| drag_ended(app, true));
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                // Lost to another window (Alt+Tab): the drag is cancelled.
                if with_app(|app| drag_ended(app, false)).is_none() {
                    post(|app: &mut App| drag_ended(app, false));
                }
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(list) = GetDlgItem(hwnd, LIST as i32).ok() {
                    fit_column(list);
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let grey = with_app(|app| app.presets_view.as_ref().is_some_and(|p| [p.empty_text, p.empty_icon].contains(&HWND(lparam.0 as *mut _)))).unwrap_or(false);
                SetTextColor(dc, if grey { COLORREF(0x707070) } else { COLORREF(GetSysColor(COLOR_WINDOWTEXT)) });
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                FillRect(dc, &ps.rcPaint, GetSysColorBrush(COLOR_BTNFACE));
                if DROP_HIGHLIGHT.with(Cell::get) {
                    let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as i32;
                    let accent = super::canvas::accent_color();
                    let pen = CreatePen(PS_SOLID, 3 * dpi / 96, COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16));
                    let old = SelectObject(dc, pen);
                    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let inset = 3 * dpi / 96;
                    let _ = RoundRect(dc, r.left + inset, r.top + inset, r.right - inset, r.bottom - inset, 12 * dpi / 96, 12 * dpi / 96);
                    SelectObject(dc, old_brush);
                    SelectObject(dc, old);
                    let _ = DeleteObject(pen);
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Whether the column shows the presets: not while cropping, which needs
/// the crop panel.
pub fn shown(model: &Model) -> bool {
    model.inspector_visible && model.pane() == Pane::Presets && !model.cropping()
}
