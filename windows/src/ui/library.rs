//! The library's menu items, keys and questions (`MainMenu.swift`'s Photo
//! menu, `Delete.swift`'s question, Copy Edits…'s choices): ratings and
//! flags, copying and pasting edits, duplicates, the Recycle Bin, and the
//! strip's filter. Ctrl stands for ⌘; ⌥⌘ shortcuts, which would be AltGr on
//! many keyboards, have none.

use std::cell::Cell;
use std::collections::BTreeSet;

use negs_library::{EditGroup, EditsFilter, Filter, FlagFilter, Item, RatingFilter};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::{App, alert, id, status};
use crate::model::{LibraryKey, library_map};
use crate::platform::{pcwstr, wide};

// Menu commands, clear of the inspector's control ids.
const RATE: u16 = 20000; // ..20005
const RAISE: u16 = 20006;
const LOWER: u16 = 20007;
const PICK: u16 = 20008;
const REJECT: u16 = 20009;
const UNFLAG: u16 = 20010;
pub const COPY_EDITS: u16 = 20011;
pub const COPY_EDITS_CHOOSING: u16 = 20012;
const PASTE_EDITS: u16 = 20013;
const PASTE_PREVIOUS: u16 = 20014;
pub const DUPLICATE: u16 = 20015;
const TRASH: u16 = 20016;
const TRASH_NOW: u16 = 20017;
const DELETE_REJECTED: u16 = 20018;
const ADVANCE: u16 = 20019;
const CLEAR_FILTER: u16 = 20020;
const FILTER_RATING: u16 = 20030; // ..20036, `Filter::rating_choices`
const FILTER_FLAG: u16 = 20040; // ..20042, `FlagFilter::CHOICES`
const FILTER_EDITED: u16 = 20043;
const FILTER_UNEDITED: u16 = 20044;

fn append(menu: HMENU, id: u16, text: &str) {
    let w = wide(text);
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id as usize, pcwstr(&w));
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

fn submenu(menu: HMENU, text: &str) -> HMENU {
    unsafe {
        let sub = CreatePopupMenu().unwrap_or_default();
        let w = wide(text);
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, pcwstr(&w));
        sub
    }
}

/// Photo's items after Show Original, the Mac's in its order. Reset Edits
/// keeps its id (`id::RESET_ALL`).
pub fn add_menu(photo: HMENU) {
    separator(photo);
    let rating = submenu(photo, "R&ating");
    for stars in 0..=5u16 {
        let title = if stars == 0 { "&None\t0".to_string() } else { format!("{}\t{stars}", "★".repeat(stars as usize)) };
        append(rating, RATE + stars, &title);
        if stars == 0 {
            separator(rating);
        }
    }
    separator(rating);
    append(rating, RAISE, "&Raise Rating\t]");
    append(rating, LOWER, "&Lower Rating\t[");
    let flag = submenu(photo, "&Flag");
    append(flag, PICK, "&Pick\tP");
    append(flag, REJECT, "&Reject\tX");
    append(flag, UNFLAG, "&Unflagged\tU");
    separator(photo);
    append(photo, COPY_EDITS, "&Copy Edits\tCtrl+Shift+C");
    append(photo, COPY_EDITS_CHOOSING, "Copy &Edits…");
    append(photo, PASTE_EDITS, "&Paste Edits\tCtrl+Shift+V");
    append(photo, PASTE_PREVIOUS, "Paste from Pre&vious");
    separator(photo);
    append(photo, DUPLICATE, "&Duplicate\tCtrl+D");
    append(photo, id::RESET_ALL, "&Reset Edits\tCtrl+Shift+R");
    separator(photo);
    append(photo, TRASH, "&Move to Recycle Bin…\tDel");
    append(photo, TRASH_NOW, "Move to Recycle Bin\tShift+Del");
    append(photo, DELETE_REJECTED, "Delete Re&jected Photographs…");
    separator(photo);
    append(photo, ADVANCE, "Advance After Ra&ting");
    separator(photo);
    let filter = submenu(photo, "F&ilter");
    add_filter_items(filter);
    append(photo, CLEAR_FILTER, "C&lear Filter");
}

/// The Filter submenu's items, also the grid sentence's menu.
pub fn add_filter_items(filter: HMENU) {
    for (i, (_, title)) in Filter::rating_choices().iter().enumerate() {
        append(filter, FILTER_RATING + i as u16, title);
    }
    separator(filter);
    for (i, (_, title)) in FlagFilter::CHOICES.iter().enumerate() {
        append(filter, FILTER_FLAG + i as u16, title);
    }
    separator(filter);
    append(filter, FILTER_EDITED, "Edited");
    append(filter, FILTER_UNEDITED, "Unedited");
}

/// The library's shortcuts with Ctrl, as accelerators: never while a text
/// field has focus. Rating keys and Del are `key`'s.
pub fn accelerators() -> Vec<ACCEL> {
    let shift_ctrl = FCONTROL | FSHIFT | FVIRTKEY;
    [(shift_ctrl, 'C' as u16, COPY_EDITS), (shift_ctrl, 'V' as u16, PASTE_EDITS), (FCONTROL | FVIRTKEY, 'D' as u16, DUPLICATE)]
        .into_iter()
        .map(|(f_virt, key, cmd)| ACCEL { fVirt: f_virt, key, cmd })
        .collect()
}

/// Enables, ticks and titles the library's items as the menu opens.
pub fn update_menu(app: &App, menu: HMENU) {
    let model = &app.model;
    let enable = |id: u16, on: bool| unsafe {
        let _ = EnableMenuItem(menu, id as u32, if on { MF_ENABLED } else { MF_GRAYED });
    };
    let check = |id: u16, on: bool| unsafe {
        CheckMenuItem(menu, id as u32, if on { MF_CHECKED.0 } else { MF_UNCHECKED.0 });
    };
    let retitle = |id: u16, text: &str| unsafe {
        let w = wide(text);
        let _ = ModifyMenuW(menu, id as u32, MF_BYCOMMAND | MF_STRING, id as usize, pcwstr(&w));
    };
    let library = model.shelf.library.as_ref();
    let can = model.can_change_library();
    let summary = library.zip(model.library_item()).map(|(l, i)| l.summary(&i));
    for stars in 0..=5u16 {
        enable(RATE + stars, can);
        check(RATE + stars, summary.as_ref().is_some_and(|s| s.rating as u16 == stars));
    }
    for c in [RAISE, LOWER, PICK, REJECT, UNFLAG] {
        enable(c, can);
    }
    check(PICK, summary.as_ref().is_some_and(|s| s.flag == Some(negs_library::Flag::Pick)));
    check(REJECT, summary.as_ref().is_some_and(|s| s.flag == Some(negs_library::Flag::Reject)));
    check(UNFLAG, summary.as_ref().is_some_and(|s| s.flag.is_none()));
    enable(COPY_EDITS, can);
    enable(COPY_EDITS_CHOOSING, can);
    enable(PASTE_EDITS, model.can_paste_edits());
    enable(PASTE_PREVIOUS, model.can_paste_from_previous());
    enable(DUPLICATE, can);
    if library.is_some() {
        enable(id::RESET_ALL, can);
    }
    let count = model.delete_targets().len();
    let title = if count > 1 { format!("Move {count} Photographs to Recycle Bin") } else { "Move to Recycle Bin".into() };
    retitle(TRASH, &format!("{title}…\tDel"));
    retitle(TRASH_NOW, &format!("{title}\tShift+Del"));
    enable(TRASH, can && count > 0);
    enable(TRASH_NOW, can && count > 0);
    enable(DELETE_REJECTED, can && !model.rejected().is_empty());
    check(ADVANCE, model.prefs.library.advance_after_rating);
    let filter = model.library_filter();
    for (i, (rating, _)) in Filter::rating_choices().iter().enumerate() {
        check(FILTER_RATING + i as u16, filter.rating == *rating);
        enable(FILTER_RATING + i as u16, library.is_some());
    }
    for (i, (flag, _)) in FlagFilter::CHOICES.iter().enumerate() {
        check(FILTER_FLAG + i as u16, filter.flags.contains(flag));
        enable(FILTER_FLAG + i as u16, library.is_some());
    }
    check(FILTER_EDITED, filter.edits == EditsFilter::Edited);
    check(FILTER_UNEDITED, filter.edits == EditsFilter::Unedited);
    enable(FILTER_EDITED, library.is_some());
    enable(FILTER_UNEDITED, library.is_some());
    enable(CLEAR_FILTER, filter.active());
}

/// A library command; false if `command` isn't one.
pub fn command(app: &mut App, command: u16) -> bool {
    let model = &mut app.model;
    match command {
        c if (RATE..=RATE + 5).contains(&c) => model.library_key(LibraryKey::Rate((c - RATE) as i64)),
        RAISE => model.library_key(LibraryKey::Raise),
        LOWER => model.library_key(LibraryKey::Lower),
        PICK => model.library_key(LibraryKey::Pick),
        REJECT => model.library_key(LibraryKey::Reject),
        UNFLAG => model.library_key(LibraryKey::Unflag),
        COPY_EDITS => model.copy_edits(None, None),
        COPY_EDITS_CHOOSING => copy_edits_choosing(app, None),
        PASTE_EDITS => model.paste_edits(),
        PASTE_PREVIOUS => model.paste_from_previous(),
        DUPLICATE => model.duplicate_photos(),
        // Without a library (the defaults tables missing), the old reset.
        id::RESET_ALL if model.shelf.library.is_some() => model.reset_photo_edits(),
        TRASH => request_delete(app, None, true),
        TRASH_NOW => request_delete(app, None, false),
        DELETE_REJECTED => {
            let rejected = model.rejected();
            request_delete(app, Some(rejected), true);
        }
        ADVANCE => model.toggle_advance_after_rating(),
        CLEAR_FILTER => model.set_library_filter(Filter::default()),
        c if (FILTER_RATING..FILTER_RATING + 7).contains(&c) => {
            let mut filter = model.library_filter();
            filter.rating = Filter::rating_choices().get((c - FILTER_RATING) as usize).map_or(RatingFilter::Any, |(r, _)| *r);
            model.set_library_filter(filter);
        }
        c if (FILTER_FLAG..FILTER_FLAG + 3).contains(&c) => {
            let mut filter = model.library_filter();
            filter.toggle_flag(FlagFilter::CHOICES[(c - FILTER_FLAG) as usize].0);
            model.set_library_filter(filter);
        }
        FILTER_EDITED | FILTER_UNEDITED => {
            let mut filter = model.library_filter();
            filter.toggle_edits(if command == FILTER_EDITED { EditsFilter::Edited } else { EditsFilter::Unedited });
            model.set_library_filter(filter);
        }
        _ => return false,
    }
    true
}

/// The rating and flag keys, by the character typed (AltGr allowed, so `[`
/// works on a German keyboard), and Del and Shift+Del, while no text field
/// or list that types to select has focus. True if the key was used.
pub fn key(app: &mut App, msg: &MSG) -> bool {
    let focus = super::focused_class();
    if matches!(focus.as_deref(), Some("Edit") | Some("ComboBox") | Some("SysListView32")) {
        return false;
    }
    if app.model.shelf.library.is_none() {
        return false;
    }
    match msg.message {
        WM_CHAR => {
            let Some(key) = char::from_u32(msg.wParam.0 as u32).and_then(library_map::library_key) else { return false };
            // Held down, a key rates once.
            if msg.lParam.0 & (1 << 30) == 0 {
                app.model.library_key(key);
            }
            true
        }
        WM_KEYDOWN if msg.wParam.0 == VK_DELETE.0 as usize => {
            let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) < 0 };
            if down(VK_CONTROL) || down(VK_MENU) {
                return false;
            }
            if msg.lParam.0 & (1 << 30) == 0 {
                request_delete(app, None, !down(VK_SHIFT));
            }
            true
        }
        _ => false,
    }
}

/// Deletes the open photograph, or `items`, asking first when `asking` or
/// when its volume has no Recycle Bin (`requestDelete`).
fn request_delete(app: &mut App, items: Option<Vec<Item>>, asking: bool) {
    if !app.model.can_change_library() {
        return;
    }
    let items = items.unwrap_or_else(|| app.model.delete_targets());
    let Some((plan, question, permanent)) = app.model.delete_question(&items) else { return };
    if asking || permanent {
        let alert = alert::Alert::new(question.message, Some(question.information)).buttons(&[&question.button, "Cancel"], Some(1));
        if alert::ask(app.main, alert) != 0 {
            return;
        }
    }
    app.model.delete_photos(plan);
}

thread_local! {
    static CHOOSING_DONE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Copy Edits…: the groups Paste Edits applies, chosen in a small dialog
/// (`copyEditsChoosing`); the choice is remembered. Of `from`, else the
/// open photograph.
pub fn copy_edits_choosing(app: &mut App, from: Option<Item>) {
    if !app.model.can_change_library() {
        return;
    }
    let chosen = app.model.prefs.library.copy_groups();
    let Some(groups) = choose_groups(app, &chosen) else { return };
    app.model.copy_edits(from, Some(groups));
}

fn choose_groups(app: &App, chosen: &BTreeSet<EditGroup>) -> Option<BTreeSet<EditGroup>> {
    let px = |dip: f64| app.px(dip);
    let fonts = &app.fonts;
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(choosing_proc),
            hInstance: super::instance(),
            lpszClassName: w!("NegsCopyEdits"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
    let (width, pad) = (px(320.0), px(24.0));
    let inner = width - 2 * pad;
    let title = wide("Copy Edits");
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
    let dialog = unsafe { CreateWindowExW(ex, w!("NegsCopyEdits"), pcwstr(&title), style, 0, 0, width, px(200.0), app.main, None, super::instance(), None).unwrap_or_default() };
    super::theme::apply_window(dialog);
    let make = |class: PCWSTR, text: &str, style: WINDOW_STYLE, id: u16, font| super::create(class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), dialog, id, font);
    let mut y = pad;
    let note = make(w!("STATIC"), "Choose the edits Paste Edits applies.", WINDOW_STYLE(windows::Win32::System::SystemServices::SS_LEFT.0 | windows::Win32::System::SystemServices::SS_NOPREFIX.0), 0, fonts.ui);
    unsafe {
        let _ = MoveWindow(note, pad, y, inner, px(20.0), false);
    }
    y += px(20.0) + px(12.0);
    let mut boxes = vec![];
    for (i, group) in EditGroup::ALL.iter().enumerate() {
        let check = make(w!("BUTTON"), group.title(), WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), 100 + i as u16, fonts.ui);
        unsafe {
            SendMessageW(check, BM_SETCHECK, WPARAM(chosen.contains(group) as usize), LPARAM(0));
            let _ = MoveWindow(check, pad, y, inner, px(24.0), false);
        }
        boxes.push(check);
        y += px(26.0);
    }
    y += px(16.0);
    let (button_w, button_h) = (px(88.0), px(32.0));
    let copy = make(w!("BUTTON"), "Copy", WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), IDOK.0 as u16, fonts.ui);
    let cancel = make(w!("BUTTON"), "Cancel", WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), IDCANCEL.0 as u16, fonts.ui);
    unsafe {
        let _ = MoveWindow(copy, width - pad - 2 * button_w - px(8.0), y, button_w, button_h, false);
        let _ = MoveWindow(cancel, width - pad - button_w, y, button_w, button_h, false);
        let mut frame = RECT { left: 0, top: 0, right: width, bottom: y + button_h + pad };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut frame, style, false, ex, app.dpi);
        let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
        let mut owner = RECT::default();
        let _ = GetWindowRect(app.main, &mut owner);
        let (x, top) = (owner.left + (owner.right - owner.left - fw) / 2, owner.top + (owner.bottom - owner.top - fh) / 3);
        let _ = SetWindowPos(dialog, None, x, top.max(owner.top), fw, fh, SWP_NOZORDER | SWP_NOACTIVATE);
        CHOOSING_DONE.with(|d| d.set(None));
        let _ = EnableWindow(app.main, false);
        let _ = ShowWindow(dialog, SW_SHOW);
        let _ = SetFocus(boxes.first().copied().unwrap_or(copy));
        let mut msg = MSG::default();
        while CHOOSING_DONE.with(Cell::get).is_none() {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if !IsDialogMessageW(dialog, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let accepted = CHOOSING_DONE.with(Cell::get).unwrap_or(false);
        let groups: BTreeSet<EditGroup> =
            EditGroup::ALL.iter().zip(&boxes).filter(|(_, b)| SendMessageW(**b, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1).map(|(g, _)| *g).collect();
        let _ = EnableWindow(app.main, true);
        let _ = DestroyWindow(dialog);
        let _ = SetForegroundWindow(app.main);
        accepted.then_some(groups)
    }
}

extern "system" fn choosing_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = (wparam.0 & 0xffff) as i32;
                if id == IDOK.0 || id == IDCANCEL.0 {
                    CHOOSING_DONE.with(|d| d.set(Some(id == IDOK.0)));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                CHOOSING_DONE.with(|d| d.set(Some(false)));
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let p = super::theme::palette();
                SetTextColor(dc, COLORREF(p.text));
                LRESULT(super::theme::brush(p.panel).0 as isize)
            }
            WM_NOTIFY => super::draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => super::draw::erase(hwnd, wparam, super::theme::palette().panel),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The library's note in place of the status line's words (`NoteWords`):
/// the action strong, then how many and why.
pub fn status_words(app: &App) -> Option<Vec<status::Span>> {
    let (note, _) = app.model.shelf.note.as_ref()?;
    let mut words = vec![status::Span::strong(library_map::note_action(&note.text), status::Tone::Paper)];
    let detail = library_map::note_detail(note);
    if !detail.is_empty() {
        words.push(status::Span::new(detail, status::Tone::Label));
    }
    Some(words)
}
