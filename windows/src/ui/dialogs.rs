//! Open and save dialogs, error and question dialogs, Settings and About.
//! Export's options live inside the save dialog (§20.4, decided): the file
//! type filter picks the format, `IFileDialogCustomize` adds Engine, Size and
//! the message.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{GUID, Interface, PCWSTR, PWSTR, implement};

use super::App;
use crate::engine::ExportEngine;
use crate::model::{COLOR_SPACES, Edge, ExportSize, IMAGE_EXTENSIONS, OutputFormat, PIXEL_RANGE, PROPHOTO, QUALITY_RANGE, RAW_EXTENSIONS, SaveSettings};
use crate::platform::{pcwstr, wide};

// Each dialog remembers its own folder.
const OPEN_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a001);
const EXPORT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a002);
const PREVIEW_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a003);
const LUT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a004);
const PRESET_IMPORT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a005);
const PRESET_EXPORT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a006);

thread_local! {
    static TOOLTIP: Cell<isize> = const { Cell::new(0) };
    static TIPS: RefCell<Vec<(isize, Vec<u16>)>> = const { RefCell::new(Vec::new()) };
}

/// Sets `hwnd`'s tooltip, sharing one tooltip window.
pub fn tooltip(hwnd: HWND, text: &str) {
    unsafe {
        let tip = TOOLTIP.with(|t| {
            if t.get() == 0 {
                let tip = CreateWindowExW(
                    WS_EX_TOPMOST,
                    TOOLTIPS_CLASSW,
                    PCWSTR::null(),
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    super::main_hwnd(),
                    None,
                    super::instance(),
                    None,
                )
                .unwrap_or_default();
                SendMessageW(tip, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(420));
                // At the system's half second (a tenth from tip to tip), a
                // pointer resting on a slider before pressing it showed the
                // tip, and the press hid it at once: a box flashed by.
                for delay in [TTDT_INITIAL, TTDT_RESHOW] {
                    SendMessageW(tip, TTM_SETDELAYTIME, WPARAM(delay as usize), LPARAM(1000));
                }
                super::theme::apply_control(tip, true);
                t.set(tip.0 as isize);
            }
            HWND(t.get() as *mut _)
        });
        let text = wide(text);
        let known = TIPS.with(|tips| {
            let mut tips = tips.borrow_mut();
            let entry = tips.iter_mut().find(|(h, _)| *h == hwnd.0 as isize);
            match entry {
                Some((_, existing)) if *existing == text => None,
                Some((_, existing)) => {
                    *existing = text.clone();
                    Some(true)
                }
                None => {
                    tips.push((hwnd.0 as isize, text.clone()));
                    Some(false)
                }
            }
        });
        let Some(known) = known else { return };
        // The text pointer stays valid: TIPS keeps it.
        let pointer = TIPS.with(|tips| tips.borrow().iter().find(|(h, _)| *h == hwnd.0 as isize).map(|(_, t)| t.as_ptr()).unwrap());
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: GetParent(hwnd).unwrap_or_default(),
            uId: hwnd.0 as usize,
            lpszText: PWSTR(pointer as *mut _),
            ..Default::default()
        };
        let message = if known { TTM_UPDATETIPTEXTW } else { TTM_ADDTOOLW };
        SendMessageW(tip, message, WPARAM(0), LPARAM(&info as *const _ as isize));
    }
}

fn task_dialog(owner: HWND, instruction: &str, content: &str, details: Option<&str>, icon: PCWSTR, buttons: &[(i32, &str)], flags: TASKDIALOG_FLAGS) -> i32 {
    let title = wide("Negs");
    let instruction = wide(instruction);
    let content = wide(content);
    let details = details.map(wide);
    let labels: Vec<Vec<u16>> = buttons.iter().map(|(_, l)| wide(l)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = buttons.iter().zip(&labels).map(|((id, _), l)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: pcwstr(l) }).collect();
    let mut config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner,
        hInstance: super::instance(),
        dwFlags: flags | TDF_POSITION_RELATIVE_TO_WINDOW | TDF_SIZE_TO_CONTENT,
        pszWindowTitle: pcwstr(&title),
        pszMainInstruction: pcwstr(&instruction),
        pszContent: pcwstr(&content),
        cButtons: buttons.len() as u32,
        pButtons: if buttons.is_empty() { std::ptr::null() } else { buttons.as_ptr() },
        dwCommonButtons: if buttons.is_empty() { TDCBF_OK_BUTTON } else { TASKDIALOG_COMMON_BUTTON_FLAGS(0) },
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = icon;
    if let Some(details) = &details {
        config.pszExpandedInformation = pcwstr(details);
    }
    let mut pressed = 0;
    unsafe {
        let _ = TaskDialogIndirect(&config, Some(&mut pressed), None, None);
    }
    pressed
}

/// An error, with an exporter log as expandable details.
pub fn error(owner: HWND, message: &str) {
    let (first, rest) = message.split_once('\n').unwrap_or((message, ""));
    let details = (!rest.trim().is_empty()).then_some(rest.trim());
    task_dialog(owner, first, "", details, TD_WARNING_ICON, &[], TASKDIALOG_FLAGS(0));
}

/// A short message with an explanation below it.
pub fn notice(owner: HWND, instruction: &str, content: &str) {
    task_dialog(owner, instruction, content, None, TD_WARNING_ICON, &[], TASKDIALOG_FLAGS(0));
}

/// True to delete the preset.
pub fn confirm_delete_preset(owner: HWND, name: &str) -> bool {
    task_dialog(
        owner,
        &format!("Delete the preset “{name}”?"),
        "This can’t be undone. Photographs edited with it keep their settings.",
        None,
        TD_WARNING_ICON,
        &[(100, "Delete"), (IDCANCEL.0, "Cancel")],
        TDF_ALLOW_DIALOG_CANCELLATION,
    ) == 100
}

/// True to quit and cancel the export.
pub fn confirm_quit_during_export(owner: HWND) -> bool {
    task_dialog(
        owner,
        "An export is still in progress.",
        "Quitting cancels it. Any existing file at the destination is left unchanged.",
        None,
        TD_WARNING_ICON,
        &[(100, "Continue exporting"), (101, "Quit")],
        TDF_USE_COMMAND_LINKS,
    ) == 101
}

pub fn about(owner: HWND) {
    let content = format!(
        "Version {}\n\nNegs is built on Andrea Volpato's spektrafilm, the spectral film simulation behind every stock, and turbasvin's spektrafilm-rs, the Rust port that runs it on the GPU.\n\nLicensed under the GNU General Public License, version 3.",
        env!("CARGO_PKG_VERSION")
    );
    task_dialog(owner, "Negs", &content, None, TD_INFORMATION_ICON, &[], TASKDIALOG_FLAGS(0));
}

const SETTINGS_PREVIEWS: u16 = 10;
const SETTINGS_AUTOMATIC: u16 = 11;
const SETTINGS_CHECK: u16 = 12;
const SETTINGS_KEEP_PAPER: u16 = 13;
const SETTINGS_NEW_PHOTOS: u16 = 14;
const SETTINGS_MATCH: u16 = 15;
const SETTINGS_LIMIT: u16 = 16;
const SETTINGS_LOW_MEMORY: u16 = 17;
const SETTINGS_NEW_NOTE: u16 = 18;
const SETTINGS_BACKGROUND: u16 = 19;
const SETTINGS_BACKGROUND_COLOR: u16 = 20;

/// Settings › New photographs: what each choice does.
const NEW_PHOTOS_NOTES: [&str; 2] = [
    "The photograph you open takes the edits of the one before it, apart from crop, rotation and lens corrections.",
    "Each photograph you open starts with default Prep and Lab settings and no border. The film, paper, Image Size and output colour stay.",
];

thread_local! {
    /// The open Settings dialog's result: None while open, then whether OK closed it.
    static SETTINGS_DONE: Cell<Option<bool>> = const { Cell::new(None) };
    /// The dialog's descriptions, drawn grey.
    static SETTINGS_NOTES: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    /// The viewer background colour chosen in the open dialog, and its swatch.
    static SETTINGS_SWATCH: Cell<(isize, [u8; 3])> = const { Cell::new((0, [0; 3])) };
}

/// Settings (the Mac's Settings window): previews, keeping the paper, new
/// photographs, the preview and export, and updates, in a modal dialog with
/// OK and Cancel. Updates show only in builds that have them.
pub fn settings(app: &mut App) {
    let configured = crate::updates::configured();
    let px = |dip: f64| app.px(dip);
    let fonts = &app.fonts;
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_proc),
            hInstance: super::instance(),
            lpszClassName: windows::core::w!("NegsSettings"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
    let (width, pad) = (px(448.0), px(24.0));
    let inner = width - 2 * pad;
    let title = wide("Settings");
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
    let dialog = unsafe {
        CreateWindowExW(ex, windows::core::w!("NegsSettings"), pcwstr(&title), style, 0, 0, width, px(200.0), app.main, None, super::instance(), None).unwrap_or_default()
    };
    super::theme::apply_window(dialog);
    let make = |class: PCWSTR, text: &str, style: WINDOW_STYLE, id: u16, font| super::create(class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), dialog, id, font);
    let label = |text: &str, font| make(windows::core::w!("STATIC"), text, WINDOW_STYLE(windows::Win32::System::SystemServices::SS_LEFT.0 | windows::Win32::System::SystemServices::SS_NOPREFIX.0), 0, font);
    let check = |text: &str, id: u16, on: bool| {
        let c = make(windows::core::w!("BUTTON"), text, WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), id, fonts.ui);
        unsafe {
            SendMessageW(c, BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
        }
        c
    };
    // Each row: the control, its height (None: measure the wrapped text), its
    // indent, the gap after it, and a width other than the whole row's.
    let mut rows: Vec<(HWND, Option<i32>, i32, i32, Option<i32>)> = vec![];
    let mut notes = vec![];
    rows.push((label("Film & Paper", fonts.section), Some(px(24.0)), 0, px(8.0), None));
    let previews = check("Film, paper and preset previews", SETTINGS_PREVIEWS, app.model.prefs.previews);
    rows.push((previews, Some(px(24.0)), 0, 0, None));
    let note = label("Show each film, paper and preset on your photograph. Turn off to use simple menus and lists on slower computers.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(16.0), None));
    let keep_paper = check("Keep paper when changing film", SETTINGS_KEEP_PAPER, app.model.prefs.keep_paper);
    rows.push((keep_paper, Some(px(24.0)), 0, 0, None));
    let note = label("Leave the paper as it is when you choose another film, instead of switching to the paper made for that film.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(24.0), None));
    rows.push((label("New Photographs", fonts.section), Some(px(24.0)), 0, px(8.0), None));
    let new_photos = make(windows::core::w!("COMBOBOX"), "", WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32), SETTINGS_NEW_PHOTOS, fonts.ui);
    for title in ["Keep the previous edits", "Start fresh"] {
        let text = wide(title);
        unsafe {
            SendMessageW(new_photos, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
        }
    }
    let fresh = app.model.prefs.fresh_edits;
    unsafe {
        SendMessageW(new_photos, CB_SETCURSEL, WPARAM(fresh as usize), LPARAM(0));
    }
    super::draw::paint_combo(new_photos);
    super::accessible_name(new_photos, "New photographs");
    // The drop-down's height is its list's; the next row starts 32 below.
    rows.push((new_photos, Some(px(200.0)), 0, px(32.0) - px(200.0), Some(px(224.0))));
    // The note's text follows the choice; it is laid out for the longer one.
    let note = make(windows::core::w!("STATIC"), NEW_PHOTOS_NOTES[1], WINDOW_STYLE(windows::Win32::System::SystemServices::SS_LEFT.0 | windows::Win32::System::SystemServices::SS_NOPREFIX.0), SETTINGS_NEW_NOTE, fonts.small);
    notes.push(note);
    rows.push((note, None, 0, px(16.0), None));
    let match_new = check("Match new photographs to camera brightness", SETTINGS_MATCH, app.model.prefs.match_new_photos);
    rows.push((match_new, Some(px(24.0)), 0, 0, None));
    let note = label("New photographs start with Prep › Match camera brightness on: a RAW develops about as bright as the camera's own preview, a JPEG or TIFF as the file itself.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(24.0), None));
    rows.push((label("Preview & Export", fonts.section), Some(px(24.0)), 0, px(8.0), None));
    let prefs = &app.model.prefs;
    let custom_background = check("Custom viewer background", SETTINGS_BACKGROUND, prefs.viewer_background_custom);
    rows.push((custom_background, Some(px(24.0)), 0, px(4.0), None));
    let choose = make(windows::core::w!("BUTTON"), "Choose Colour…", WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), SETTINGS_BACKGROUND_COLOR, fonts.ui);
    super::accessible_name(choose, "Viewer background colour");
    unsafe {
        let _ = EnableWindow(choose, prefs.viewer_background_custom);
    }
    rows.push((choose, Some(px(32.0)), px(24.0), px(4.0), Some(px(160.0))));
    // The colour, beside its button.
    let swatch = make(windows::core::w!("STATIC"), "", WS_BORDER, 0, fonts.ui);
    let color = crate::border::BorderColor::from_hex(&prefs.viewer_background).map_or([0x24; 3], |c| c.0);
    SETTINGS_SWATCH.with(|s| s.set((swatch.0 as isize, color)));
    let note = label("The colour behind the photograph. Off, it follows the light or dark theme.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(16.0), None));
    let limit = check("Limit preview to 12 MP", SETTINGS_LIMIT, app.model.prefs.limits_preview());
    rows.push((limit, Some(px(24.0)), 0, 0, None));
    let note = label("Develops the preview faster on large photographs. Export keeps the full Image Size.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(16.0), None));
    let low_memory = check("Use less memory for export", SETTINGS_LOW_MEMORY, app.model.prefs.low_memory_export);
    rows.push((low_memory, Some(px(24.0)), 0, 0, None));
    let note = label("Develops the grain one colour at a time. Same result, less memory, longer export.", fonts.small);
    notes.push(note);
    rows.push((note, None, px(24.0), px(24.0), None));
    rows.push((label("Updates", fonts.section), Some(px(24.0)), 0, px(8.0), None));
    let mut automatic = HWND::default();
    if configured {
        automatic = check("Automatically check for updates", SETTINGS_AUTOMATIC, crate::updates::automatic());
        rows.push((automatic, Some(px(24.0)), 0, 0, None));
        let last = crate::updates::last_check().unwrap_or_else(|| "never".into());
        let note = label(&format!("Updates are installed only when you choose to restart Negs. Last checked: {last}."), fonts.small);
        notes.push(note);
        rows.push((note, None, px(24.0), px(16.0), None));
        let button = make(windows::core::w!("BUTTON"), "Check for Updates…", WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), SETTINGS_CHECK, fonts.ui);
        rows.push((button, Some(px(32.0)), px(24.0), px(8.0), Some(px(160.0))));
    } else {
        let note = label("Updates are not configured for this build.", fonts.small);
        notes.push(note);
        rows.push((note, None, 0, px(4.0), None));
    }
    SETTINGS_NOTES.with(|n| *n.borrow_mut() = notes.iter().map(|h| h.0 as isize).collect());
    let mut y = pad;
    for (control, height, indent, gap, fixed) in rows {
        let w = fixed.unwrap_or(inner - indent);
        let h = height.unwrap_or_else(|| wrapped_height(control, w));
        unsafe {
            let _ = MoveWindow(control, pad + indent, y, w, h, false);
            if control == choose {
                let side = px(20.0);
                let _ = MoveWindow(swatch, pad + indent + w + px(12.0), y + (h - side) / 2, side, side, false);
            }
        }
        y += h + gap;
    }
    set_new_photos_note(dialog, fresh);
    let (button_w, button_h) = (px(88.0), px(32.0));
    y += px(16.0);
    let ok = make(windows::core::w!("BUTTON"), "OK", WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), IDOK.0 as u16, fonts.ui);
    let cancel = make(windows::core::w!("BUTTON"), "Cancel", WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), IDCANCEL.0 as u16, fonts.ui);
    unsafe {
        let _ = MoveWindow(ok, width - pad - 2 * button_w - px(8.0), y, button_w, button_h, false);
        let _ = MoveWindow(cancel, width - pad - button_w, y, button_w, button_h, false);
        // Size the window around the client area, centred on the editor.
        let mut frame = RECT { left: 0, top: 0, right: width, bottom: y + button_h + pad };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut frame, style, false, ex, app.dpi);
        let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
        let mut owner = RECT::default();
        let _ = GetWindowRect(app.main, &mut owner);
        let (x, top) = (owner.left + (owner.right - owner.left - fw) / 2, owner.top + (owner.bottom - owner.top - fh) / 3);
        let _ = SetWindowPos(dialog, None, x, top.max(owner.top), fw, fh, SWP_NOZORDER | SWP_NOACTIVATE);
        SETTINGS_DONE.with(|d| d.set(None));
        let _ = EnableWindow(app.main, false);
        let _ = ShowWindow(dialog, SW_SHOW);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(previews);
        let mut msg = MSG::default();
        while SETTINGS_DONE.with(Cell::get).is_none() {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // WM_QUIT: leave it for the main loop.
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if !IsDialogMessageW(dialog, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let accepted = SETTINGS_DONE.with(Cell::get).unwrap_or(false);
        let checked = |c: HWND| SendMessageW(c, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
        let (previews_on, keep_paper_on, automatic_on) = (checked(previews), checked(keep_paper), configured && checked(automatic));
        let fresh_on = SendMessageW(new_photos, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 == 1;
        let (match_on, limit_on, low_memory_on) = (checked(match_new), checked(limit), checked(low_memory));
        let background_on = checked(custom_background);
        let background = crate::border::BorderColor(SETTINGS_SWATCH.with(Cell::get).1);
        let _ = EnableWindow(app.main, true);
        let _ = DestroyWindow(dialog);
        let _ = SetForegroundWindow(app.main);
        if !accepted {
            return;
        }
        if configured {
            crate::updates::set_automatic(automatic_on);
            app.model.prefs.check_for_updates = automatic_on;
            app.model.prefs.save();
        }
        app.model.set_previews(previews_on);
        app.model.set_keep_paper(keep_paper_on);
        app.model.set_fresh_edits(fresh_on);
        app.model.set_match_new_photos(match_on);
        app.model.set_limit_preview(limit_on);
        app.model.set_low_memory_export(low_memory_on);
        let prefs = &mut app.model.prefs;
        if prefs.viewer_background_custom != background_on || prefs.viewer_background != background.hex() {
            prefs.viewer_background_custom = background_on;
            prefs.viewer_background = background.hex();
            prefs.save();
        }
        super::canvas::set_custom_background(app.canvas, app.model.prefs.custom_viewer_background());
    }
}

/// The New photographs note for the choice in the open dialog.
fn set_new_photos_note(dialog: HWND, fresh: bool) {
    unsafe {
        if let Ok(note) = GetDlgItem(dialog, SETTINGS_NEW_NOTE as i32) {
            let text = wide(NEW_PHOTOS_NOTES[fresh as usize]);
            let _ = SetWindowTextW(note, pcwstr(&text));
        }
    }
}

/// The height `control`'s text needs, wrapped at `width`.
fn wrapped_height(control: HWND, width: i32) -> i32 {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let mut text = [0u16; 512];
        let n = GetWindowTextW(control, &mut text) as usize;
        let dc = GetDC(control);
        let font = HFONT(SendMessageW(control, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _);
        let old = SelectObject(dc, font);
        let mut r = RECT { left: 0, top: 0, right: width, bottom: 0 };
        DrawTextW(dc, &mut text[..n], &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, old);
        ReleaseDC(control, dc);
        r.bottom
    }
}

extern "system" fn settings_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = (wparam.0 & 0xffff) as i32;
                if id == IDOK.0 || id == IDCANCEL.0 {
                    SETTINGS_DONE.with(|d| d.set(Some(id == IDOK.0)));
                } else if id == SETTINGS_NEW_PHOTOS as i32 && ((wparam.0 >> 16) & 0xffff) as u32 == CBN_DROPDOWN {
                    super::theme::combo_list(HWND(lparam.0 as *mut _));
                } else if id == SETTINGS_NEW_PHOTOS as i32 && ((wparam.0 >> 16) & 0xffff) as u32 == CBN_SELCHANGE {
                    let fresh = SendMessageW(HWND(lparam.0 as *mut _), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 == 1;
                    set_new_photos_note(hwnd, fresh);
                } else if id == SETTINGS_CHECK as i32 {
                    crate::updates::check(true);
                } else if id == SETTINGS_BACKGROUND as i32 {
                    let on = SendMessageW(HWND(lparam.0 as *mut _), BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
                    if let Ok(choose) = GetDlgItem(hwnd, SETTINGS_BACKGROUND_COLOR as i32) {
                        let _ = EnableWindow(choose, on);
                    }
                    let _ = InvalidateRect(HWND(SETTINGS_SWATCH.with(Cell::get).0 as *mut _), None, true);
                } else if id == SETTINGS_BACKGROUND_COLOR as i32 {
                    let (swatch, color) = SETTINGS_SWATCH.with(Cell::get);
                    if let Some(chosen) = super::inspector::choose_color(hwnd, crate::border::BorderColor(color)) {
                        SETTINGS_SWATCH.with(|s| s.set((swatch, chosen.0)));
                        let _ = InvalidateRect(HWND(swatch as *mut _), None, true);
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                SETTINGS_DONE.with(|d| d.set(Some(false)));
                LRESULT(0)
            }
            WM_CTLCOLORLISTBOX => {
                // The New Photographs list, as the column's lists.
                let dc = HDC(wparam.0 as *mut _);
                let p = super::theme::palette();
                SetTextColor(dc, COLORREF(p.text));
                SetBkColor(dc, COLORREF(p.control));
                LRESULT(super::theme::brush(p.control).0 as isize)
            }
            WM_CTLCOLORSTATIC if lparam.0 == SETTINGS_SWATCH.with(Cell::get).0 => {
                // The chosen colour, faded while the custom background is off.
                let (_, [r, g, b]) = SETTINGS_SWATCH.with(Cell::get);
                let on = GetDlgItem(hwnd, SETTINGS_BACKGROUND as i32).is_ok_and(|c| SendMessageW(c, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1);
                let p = super::theme::palette();
                let color = if on { r as u32 | (g as u32) << 8 | (b as u32) << 16 } else { p.disabled };
                LRESULT(super::theme::brush(color).0 as isize)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let note = SETTINGS_NOTES.with(|n| n.borrow().contains(&lparam.0));
                let p = super::theme::palette();
                SetTextColor(dc, COLORREF(if note { p.secondary } else { p.text }));
                LRESULT(super::theme::brush(p.panel).0 as isize)
            }
            WM_NOTIFY => super::draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => super::draw::erase(hwnd, wparam, super::theme::palette().panel),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn shell_item(path: &std::path::Path) -> Option<IShellItem> {
    let w = wide(path);
    unsafe { SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok() }
}

fn result_path(dialog: &IFileDialog) -> Option<PathBuf> {
    unsafe {
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().ok()?);
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

/// Starts in the current photograph's folder with it selected.
pub fn open(app: &mut App) {
    let photos: String = RAW_EXTENSIONS.iter().chain(IMAGE_EXTENSIONS).map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    let raw: String = RAW_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    let filters = [
        ("Photographs", photos),
        ("RAW photographs", raw),
        ("JPEG", "*.jpg;*.jpeg".into()),
        ("PNG", "*.png".into()),
        ("TIFF", "*.tif;*.tiff".into()),
        ("All files", "*.*".into()),
    ];
    let wide_filters: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(n, p)| (wide(n), wide(p))).collect();
    let specs: Vec<COMDLG_FILTERSPEC> = wide_filters.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: pcwstr(n), pszSpec: pcwstr(p) }).collect();
    let chosen = unsafe {
        (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            dialog.SetClientGuid(&OPEN_GUID).ok()?;
            dialog.SetFileTypes(&specs).ok()?;
            let title = wide("Open Photograph");
            let _ = dialog.SetTitle(pcwstr(&title));
            let _ = dialog.SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM);
            if let Some(source) = &app.model.source {
                if let Some(folder) = source.parent().and_then(shell_item) {
                    let _ = dialog.SetFolder(&folder);
                }
                let name = wide(source.file_name().unwrap_or_default());
                let _ = dialog.SetFileName(pcwstr(&name));
            }
            dialog.Show(app.main).ok()?;
            result_path(&dialog.cast().ok()?)
        })()
    };
    if let Some(path) = chosen {
        app.model.open(&path);
    }
}

const ENGINE: u32 = 1;
const SIZE: u32 = 2;
const PIXELS: u32 = 3;
const PX: u32 = 4;
const MESSAGE: u32 = 5;
const DEPTH: u32 = 6;
const SPACE: u32 = 7;
const QUALITY: u32 = 8;
const QUALITY_RANGE_TEXT: u32 = 9;
const GROUP_ENGINE: u32 = 10;
const GROUP_SIZE: u32 = 11;
const GROUP_DEPTH: u32 = 12;
const GROUP_SPACE: u32 = 13;
const GROUP_QUALITY: u32 = 14;

/// The dialog's message, from what the model says about this save.
struct Messages {
    reference: String,
    fast: String,
    preview: String,
}

/// The encoding choices, shared with `save` to read them back. The colour
/// space is the one picked, which ProPhoto RGB at 8 bits only shows as
/// Adobe RGB, so 16 bits brings it back.
struct Encoding {
    format: Cell<OutputFormat>,
    chosen_space: Cell<usize>,
    /// The colour space the preview develops in.
    preview_space: String,
    png_depth: Cell<u32>,
    tiff_depth: Cell<u32>,
    /// Set while the controls are being brought in line, so their
    /// selection events don't count as choices.
    updating: Cell<bool>,
}

impl Encoding {
    fn depth(&self) -> u32 {
        match self.format.get() {
            OutputFormat::Jpeg => 8,
            OutputFormat::Tiff => self.tiff_depth.get(),
            OutputFormat::Png => self.png_depth.get(),
        }
    }
    fn settings(&self, customize: &IFileDialogCustomize) -> SaveSettings {
        let bit_depth = self.depth();
        SaveSettings { bit_depth, color_space: SaveSettings::effective_space(self.chosen_space.get(), bit_depth), jpeg_quality: read_quality(customize) }
    }
}

#[implement(IFileDialogEvents, IFileDialogControlEvents)]
struct SaveEvents {
    export: bool,
    messages: Messages,
    encoding: Rc<Encoding>,
    extras: String,
}

impl SaveEvents {
    /// Shows the format's bit depth (JPEG's fixed), ProPhoto RGB only at 16
    /// bits, and JPEG quality only for JPEG; then the message.
    fn sync(&self, customize: &IFileDialogCustomize) {
        let encoding = &self.encoding;
        let jpeg = encoding.format.get() == OutputFormat::Jpeg;
        let settings = encoding.settings(customize);
        encoding.updating.set(true);
        unsafe {
            let _ = customize.SetSelectedControlItem(DEPTH, if settings.bit_depth == 16 { 1 } else { 0 });
            let _ = customize.SetControlState(DEPTH, if jpeg { CDCS_VISIBLE } else { CDCS_ENABLEDVISIBLE });
            let _ = customize.SetControlItemState(SPACE, PROPHOTO as u32, if settings.bit_depth == 16 { CDCS_ENABLEDVISIBLE } else { CDCS_VISIBLE });
            let _ = customize.SetSelectedControlItem(SPACE, settings.color_space as u32);
            for id in [GROUP_QUALITY, QUALITY, QUALITY_RANGE_TEXT] {
                let _ = customize.SetControlState(id, if jpeg { CDCS_ENABLEDVISIBLE } else { CDCS_INACTIVE });
            }
        }
        encoding.updating.set(false);
        self.message(customize);
    }

    fn message(&self, customize: &IFileDialogCustomize) {
        unsafe {
            let engine = if self.export {
                Some(if customize.GetSelectedControlItem(ENGINE).unwrap_or(1) == 0 { ExportEngine::Reference } else { ExportEngine::Fast })
            } else {
                None
            };
            let size = read_size(customize);
            let lead = match engine {
                Some(ExportEngine::Reference) => &self.messages.reference,
                Some(ExportEngine::Fast) => &self.messages.fast,
                None => &self.messages.preview,
            };
            let summary = size.summary().map(|s| format!(" {s}.")).unwrap_or_default();
            let space = self.encoding.settings(customize).summary(self.export, &self.encoding.preview_space).map(|s| format!(" {s}.")).unwrap_or_default();
            let text = wide(format!("{lead}{}{summary}{space}", self.extras));
            let _ = customize.SetControlLabel(MESSAGE, pcwstr(&text));
            let _ = customize.SetControlState(PIXELS, if size.edge == Edge::Full { CDCS_VISIBLE } else { CDCS_ENABLEDVISIBLE });
        }
    }
}

fn read_quality(customize: &IFileDialogCustomize) -> u32 {
    unsafe {
        customize
            .GetEditBoxText(QUALITY)
            .ok()
            .map(|p| {
                let text = PWSTR(p).to_string().unwrap_or_default();
                CoTaskMemFree(Some(p as *const _));
                text
            })
            .and_then(|t| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok())
            .unwrap_or(95)
            .clamp(QUALITY_RANGE.0, QUALITY_RANGE.1)
    }
}

fn read_size(customize: &IFileDialogCustomize) -> ExportSize {
    unsafe {
        let edge = Edge::ALL[customize.GetSelectedControlItem(SIZE).unwrap_or(0).min(2) as usize];
        let pixels = customize
            .GetEditBoxText(PIXELS)
            .ok()
            .map(|p| {
                let text = PWSTR(p).to_string().unwrap_or_default();
                CoTaskMemFree(Some(p as *const _));
                text
            })
            .and_then(|t| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok())
            .unwrap_or(ExportSize::FULL.pixels)
            .clamp(PIXEL_RANGE.0, PIXEL_RANGE.1);
        ExportSize { edge, pixels }
    }
}

impl IFileDialogEvents_Impl for SaveEvents_Impl {
    fn OnFileOk(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnFolderChanging(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnFolderChange(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSelectionChange(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnShareViolation(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDESVR_DEFAULT)
    }
    /// The extension follows the format.
    fn OnTypeChange(&self, dialog: Option<&IFileDialog>) -> windows::core::Result<()> {
        let Some(dialog) = dialog else { return Ok(()) };
        unsafe {
            let index = dialog.GetFileTypeIndex()?.clamp(1, 3);
            let format = OutputFormat::ALL[index as usize - 1];
            self.encoding.format.set(format);
            if let Ok(customize) = dialog.cast::<IFileDialogCustomize>() {
                self.sync(&customize);
            }
            let extension = wide(format.extension());
            dialog.SetDefaultExtension(pcwstr(&extension))?;
            if let Ok(name) = dialog.GetFileName() {
                let current = name.to_string().unwrap_or_default();
                CoTaskMemFree(Some(name.0 as *const _));
                let stem = std::path::Path::new(&current).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(current);
                if !stem.is_empty() {
                    let renamed = wide(format!("{stem}.{}", format.extension()));
                    let _ = dialog.SetFileName(pcwstr(&renamed));
                }
            }
        }
        Ok(())
    }
    fn OnOverwrite(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<FDE_OVERWRITE_RESPONSE> {
        Ok(FDEOR_DEFAULT)
    }
}

impl IFileDialogControlEvents_Impl for SaveEvents_Impl {
    fn OnItemSelected(&self, customize: Option<&IFileDialogCustomize>, control: u32, item: u32) -> windows::core::Result<()> {
        let Some(customize) = customize else { return Ok(()) };
        let encoding = &self.encoding;
        if encoding.updating.get() {
            return Ok(());
        }
        match control {
            DEPTH => {
                let depth = if item == 1 { 16 } else { 8 };
                match encoding.format.get() {
                    OutputFormat::Png => encoding.png_depth.set(depth),
                    OutputFormat::Tiff => encoding.tiff_depth.set(depth),
                    OutputFormat::Jpeg => {}
                }
                self.sync(customize);
            }
            SPACE => {
                encoding.chosen_space.set(item as usize);
                self.sync(customize);
            }
            _ => self.message(customize),
        }
        Ok(())
    }
    fn OnButtonClicked(&self, _: Option<&IFileDialogCustomize>, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnCheckButtonToggled(&self, _: Option<&IFileDialogCustomize>, _: u32, _: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnControlActivating(&self, customize: Option<&IFileDialogCustomize>, _: u32) -> windows::core::Result<()> {
        if let Some(customize) = customize {
            self.message(customize);
        }
        Ok(())
    }
}

/// A separate LUT save dialog, available even without a photograph.
pub fn export_lut(app: &mut App) {
    if !app.model.can_export_lut() {
        return;
    }
    let state = &app.model.state;
    let target = if crate::json::boolean(&state.params["io"]["scan_film"]) { "direct_scan" } else { &state.paper };
    let filename = wide(format!("{}_{}.cube", state.film, target));
    let selected = unsafe {
        (|| -> windows::core::Result<PathBuf> {
            let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetClientGuid(&LUT_GUID)?;
            let title = wide("Export LUT");
            dialog.SetTitle(pcwstr(&title))?;
            let name = wide("3D LUT (.cube)");
            let pattern = wide("*.cube");
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: pcwstr(&name), pszSpec: pcwstr(&pattern) }])?;
            let extension = wide("cube");
            dialog.SetDefaultExtension(pcwstr(&extension))?;
            dialog.SetFileName(pcwstr(&filename))?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_NOREADONLYRETURN)?;
            let customize: IFileDialogCustomize = dialog.cast()?;
            let message = wide("33 × 33 × 33 CUBE · sRGB input and output. Uses the current film and paper color settings. Auto exposure, print filter shifts, preflash, grain, halation, glare, diffusion, blur, sharpening, RAW development, and Post edits are excluded.");
            customize.AddText(MESSAGE, pcwstr(&message))?;
            dialog.Show(app.main)?;
            result_path(&dialog.cast()?).ok_or_else(windows::core::Error::from_win32)
        })()
    };
    if let Ok(path) = selected {
        app.model.export_lut(path);
    }
}

/// Save Preview or Export: the format in the type filter, the engine and
/// size beside it, and a message that follows the choices.
pub fn save(app: &mut App, export: bool) {
    let model = &app.model;
    if model.source.is_none() || model.cropping() || (export && !model.can_export()) || (!export && !model.can_save()) {
        return;
    }
    let prefs = &model.prefs;
    let format = OutputFormat::from_key(&prefs.output_format);
    let engine = ExportEngine::from_key(&prefs.export_engine);
    let size = ExportSize { edge: Edge::from_key(&prefs.export_edge), pixels: prefs.export_edge_pixels.clamp(PIXEL_RANGE.0, PIXEL_RANGE.1) };
    let lead = |engine: Option<ExportEngine>| crate::model::Model::save_lead(engine).to_string();
    let extras = model.save_extras();
    let depth = |d: u32| if d == 16 { 16 } else { 8 };
    let encoding = Rc::new(Encoding {
        format: Cell::new(format),
        chosen_space: Cell::new(COLOR_SPACES.iter().position(|s| *s == prefs.color_space).unwrap_or(0)),
        preview_space: model.preview_space().to_string(),
        png_depth: Cell::new(depth(prefs.png_bit_depth)),
        tiff_depth: Cell::new(depth(prefs.tiff_bit_depth)),
        updating: Cell::new(false),
    });
    let quality = prefs.jpeg_quality.clamp(QUALITY_RANGE.0, QUALITY_RANGE.1);
    let events: IFileDialogEvents = SaveEvents {
        export,
        messages: Messages { reference: lead(Some(ExportEngine::Reference)), fast: lead(Some(ExportEngine::Fast)), preview: lead(None) },
        encoding: encoding.clone(),
        extras,
    }
    .into();
    let name = model.default_name(format);
    let folder = prefs.save_folder().map(|p| p.to_path_buf());
    let main = app.main;
    let chosen = unsafe {
        (|| -> Option<(PathBuf, OutputFormat, ExportEngine, ExportSize, SaveSettings)> {
            let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            dialog.SetClientGuid(if export { &EXPORT_GUID } else { &PREVIEW_GUID }).ok()?;
            let names: Vec<(Vec<u16>, Vec<u16>)> = OutputFormat::ALL.iter().map(|f| (wide(f.title()), wide(f.pattern()))).collect();
            let specs: Vec<COMDLG_FILTERSPEC> = names.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: pcwstr(n), pszSpec: pcwstr(p) }).collect();
            dialog.SetFileTypes(&specs).ok()?;
            dialog.SetFileTypeIndex(OutputFormat::ALL.iter().position(|f| *f == format).unwrap_or(0) as u32 + 1).ok()?;
            let extension = wide(format.extension());
            let _ = dialog.SetDefaultExtension(pcwstr(&extension));
            let title = wide(if export { "Export Photograph" } else { "Save Preview" });
            let _ = dialog.SetTitle(pcwstr(&title));
            let options = dialog.GetOptions().unwrap_or_default();
            let _ = dialog.SetOptions(options | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_NOREADONLYRETURN);
            if let Some(folder) = folder.as_deref().and_then(shell_item) {
                let _ = dialog.SetFolder(&folder);
            }
            let file = wide(&name);
            let _ = dialog.SetFileName(pcwstr(&file));
            let customize: IFileDialogCustomize = dialog.cast().ok()?;
            if export {
                let label = wide("Engine:");
                let _ = customize.StartVisualGroup(GROUP_ENGINE, pcwstr(&label));
                let _ = customize.AddComboBox(ENGINE);
                for (i, e) in ExportEngine::ALL.iter().enumerate() {
                    let t = wide(e.title());
                    let _ = customize.AddControlItem(ENGINE, i as u32, pcwstr(&t));
                }
                let _ = customize.SetSelectedControlItem(ENGINE, if engine == ExportEngine::Reference { 0 } else { 1 });
                let _ = customize.EndVisualGroup();
            }
            let label = wide("Size:");
            let _ = customize.StartVisualGroup(GROUP_SIZE, pcwstr(&label));
            let _ = customize.AddComboBox(SIZE);
            for (i, edge) in Edge::ALL.iter().enumerate() {
                let t = wide(edge.title());
                let _ = customize.AddControlItem(SIZE, i as u32, pcwstr(&t));
            }
            let _ = customize.SetSelectedControlItem(SIZE, Edge::ALL.iter().position(|e| *e == size.edge).unwrap_or(0) as u32);
            let pixels = wide(size.pixels.to_string());
            let _ = customize.AddEditBox(PIXELS, pcwstr(&pixels));
            let px = wide("px");
            let _ = customize.AddText(PX, pcwstr(&px));
            let _ = customize.EndVisualGroup();
            let label = wide("Bit depth:");
            let _ = customize.StartVisualGroup(GROUP_DEPTH, pcwstr(&label));
            let _ = customize.AddComboBox(DEPTH);
            for (i, title) in ["8-bit", "16-bit"].iter().enumerate() {
                let t = wide(title);
                let _ = customize.AddControlItem(DEPTH, i as u32, pcwstr(&t));
            }
            let _ = customize.EndVisualGroup();
            let label = wide("Colour space:");
            let _ = customize.StartVisualGroup(GROUP_SPACE, pcwstr(&label));
            let _ = customize.AddComboBox(SPACE);
            for (i, title) in COLOR_SPACES.iter().enumerate() {
                let t = wide(if i == PROPHOTO { "ProPhoto RGB (16-bit)" } else { title });
                let _ = customize.AddControlItem(SPACE, i as u32, pcwstr(&t));
            }
            let _ = customize.EndVisualGroup();
            let label = wide("JPEG quality:");
            let _ = customize.StartVisualGroup(GROUP_QUALITY, pcwstr(&label));
            let text = wide(quality.to_string());
            let _ = customize.AddEditBox(QUALITY, pcwstr(&text));
            let range = wide(format!("({}–{})", QUALITY_RANGE.0, QUALITY_RANGE.1));
            let _ = customize.AddText(QUALITY_RANGE_TEXT, pcwstr(&range));
            let _ = customize.EndVisualGroup();
            let empty = wide("");
            let _ = customize.AddText(MESSAGE, pcwstr(&empty));
            let cookie = dialog.Advise(&events).ok();
            // Set the encoding controls and the message before the dialog shows.
            if let Ok(inner) = events.cast::<IFileDialogEvents>() {
                let _ = inner.OnTypeChange(&dialog);
            }
            let shown = dialog.Show(main);
            if let Some(cookie) = cookie {
                let _ = dialog.Unadvise(cookie);
            }
            shown.ok()?;
            let path = result_path(&dialog.cast().ok()?)?;
            let index = dialog.GetFileTypeIndex().unwrap_or(1).clamp(1, 3);
            let chosen_engine = if export && customize.GetSelectedControlItem(ENGINE).unwrap_or(1) == 0 { ExportEngine::Reference } else { ExportEngine::Fast };
            encoding.format.set(OutputFormat::ALL[index as usize - 1]);
            Some((path, OutputFormat::ALL[index as usize - 1], chosen_engine, read_size(&customize), encoding.settings(&customize)))
        })()
    };
    let Some((path, format, chosen_engine, size, settings)) = chosen else { return };
    let prefs = &mut app.model.prefs;
    prefs.output_format = format.key().into();
    prefs.color_space = COLOR_SPACES[encoding.chosen_space.get()].into();
    prefs.png_bit_depth = encoding.png_depth.get();
    prefs.tiff_bit_depth = encoding.tiff_depth.get();
    prefs.jpeg_quality = settings.jpeg_quality;
    prefs.export_edge = size.edge.key().into();
    prefs.export_edge_pixels = size.pixels;
    prefs.last_save_folder = path.parent().map(|p| p.to_path_buf());
    if export {
        prefs.export_engine = chosen_engine.key().into();
    }
    prefs.save();
    if export {
        app.model.export(path, chosen_engine, size, settings);
    } else {
        app.model.save_preview(path, size, settings);
    }
}

/// Import Presets…: several files, saved by Negs or by spektrafilm.
pub fn import_presets(app: &mut App) {
    if !app.model.can_change_presets() {
        return;
    }
    let filters = [("Presets (*.json, *.txt)", "*.json;*.txt"), ("All files", "*.*")];
    let wide_filters: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(n, p)| (wide(n), wide(p))).collect();
    let specs: Vec<COMDLG_FILTERSPEC> = wide_filters.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: pcwstr(n), pszSpec: pcwstr(p) }).collect();
    let chosen = unsafe {
        (|| -> Option<Vec<PathBuf>> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            dialog.SetClientGuid(&PRESET_IMPORT_GUID).ok()?;
            dialog.SetFileTypes(&specs).ok()?;
            let title = wide("Import Presets");
            let _ = dialog.SetTitle(pcwstr(&title));
            let label = wide("Import");
            let _ = dialog.SetOkButtonLabel(pcwstr(&label));
            let _ = dialog.SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT);
            dialog.Show(app.main).ok()?;
            let items = dialog.GetResults().ok()?;
            let mut paths = vec![];
            for i in 0..items.GetCount().ok()? {
                let Ok(item) = items.GetItemAt(i) else { continue };
                let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else { continue };
                if let Ok(path) = name.to_string() {
                    paths.push(PathBuf::from(path));
                }
                CoTaskMemFree(Some(name.0 as *const _));
            }
            Some(paths)
        })()
    };
    if let Some(paths) = chosen {
        app.model.import_presets(paths, None);
    }
}

/// Export…: a byte-for-byte copy of the preset wherever the user picks.
pub fn export_preset(app: &mut App, preset: &crate::presets::PresetFile) {
    let selected = unsafe {
        (|| -> windows::core::Result<PathBuf> {
            let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetClientGuid(&PRESET_EXPORT_GUID)?;
            let title = wide("Export Preset");
            dialog.SetTitle(pcwstr(&title))?;
            let name = wide("Preset (*.json)");
            let pattern = wide("*.json");
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: pcwstr(&name), pszSpec: pcwstr(&pattern) }])?;
            let extension = wide("json");
            dialog.SetDefaultExtension(pcwstr(&extension))?;
            let filename = wide(format!("{}.json", preset.name()));
            dialog.SetFileName(pcwstr(&filename))?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_NOREADONLYRETURN)?;
            let customize: IFileDialogCustomize = dialog.cast()?;
            let message = wide("spektrafilm’s GUI state format: opens in Negs and in spektrafilm.");
            customize.AddText(MESSAGE, pcwstr(&message))?;
            dialog.Show(app.main)?;
            result_path(&dialog.cast()?).ok_or_else(windows::core::Error::from_win32)
        })()
    };
    if let Ok(path) = selected {
        app.model.export_preset(preset, &path);
    }
}
