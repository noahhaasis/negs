//! The top of the column (`PaneHeader`, §4.8): Adjustments · Presets as
//! words, then the edit count and Reset, or New and Import… beside the
//! presets. The title bar's panel button hides and shows the column itself.

use std::sync::Once;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::{SS_NOPREFIX, SS_OWNERDRAW};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::draw::{self, Role};
use super::ink;
use super::theme;
use super::{create as create_control, id, instance, with_app};
use crate::model::Pane;

/// The header's height, in DIPs (`Theme.Size.paneHeader`).
pub const HEIGHT: f64 = ink::PANE_HEADER;

/// The "3 edits" beside Reset; owner-drawn by `ink`.
const EDIT_COUNT: u16 = 330;

pub fn create(main: HWND, font: HFONT) -> HWND {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: instance(),
            lpszClassName: w!("NegsPanes"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
    let band = create_control(w!("NegsPanes"), "Panels", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
    let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    for (i, (command, title, help)) in
        [(id::ADJUSTMENTS, "Adjustments", "Show the adjustments (F7)"), (id::PRESETS, "Presets", "Show the presets (F8)")].into_iter().enumerate()
    {
        let word = create_control(w!("BUTTON"), title, style | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), band, command, font);
        draw::set_role(word, Role::PaneWord);
        draw::track_hover(word);
        super::dialogs::tooltip(word, help);
    }
    let count = create_control(w!("STATIC"), "", WS_CHILD | WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), band, EDIT_COUNT, font);
    draw::set_role(count, Role::EditCount);
    super::accessible_role(count, 0x29);
    let words = [
        (id::RESET_ALL, "Reset", "Restore defaults on all tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this."),
        (id::NEW_PRESET, "New", "Save the film, paper and Lab settings as a preset (Ctrl+Shift+S)"),
        (id::IMPORT_PRESETS, "Import…", "Import presets (Ctrl+Shift+O)"),
    ];
    for (i, (command, title, help)) in words.into_iter().enumerate() {
        let group = if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
        let word = create_control(w!("BUTTON"), title, WS_CHILD | WS_TABSTOP | group | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), band, command, font);
        draw::set_role(word, Role::WordButton);
        draw::track_hover(word);
        super::dialogs::tooltip(word, help);
    }
    band
}

fn button(band: HWND, command: u16) -> HWND {
    unsafe { GetDlgItem(band, command as i32).unwrap_or_default() }
}

fn presets_shown(band: HWND) -> bool {
    unsafe { SendMessageW(button(band, id::PRESETS), BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 }
}

/// Marks the panel the column shows, and shows its words on the right.
pub fn sync(band: HWND, presets: bool) {
    for (command, on) in [(id::ADJUSTMENTS, !presets), (id::PRESETS, presets)] {
        unsafe {
            SendMessageW(button(band, command), BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
            let _ = InvalidateRect(button(band, command), None, false);
        }
    }
    show_words(band, presets);
}

fn show_words(band: HWND, presets: bool) {
    let count = button(band, EDIT_COUNT);
    let counted = unsafe { GetWindowTextLengthW(count) } > 0;
    for (command, shown) in [(id::RESET_ALL, !presets), (EDIT_COUNT, !presets && counted), (id::NEW_PRESET, presets), (id::IMPORT_PRESETS, presets)] {
        unsafe {
            let _ = ShowWindow(button(band, command), if shown { SW_SHOWNA } else { SW_HIDE });
        }
    }
    layout(band);
}

/// The adjustments' edits on all tabs ("3 edits"; none without a
/// photograph), whether Reset can, and whether the presets can change.
pub fn sync_edits(band: HWND, edits: usize, can_reset: bool, can_presets: bool) {
    let count = button(band, EDIT_COUNT);
    let text = match edits {
        0 => String::new(),
        1 => "1 edit".into(),
        n => format!("{n} edits"),
    };
    let old = draw::window_text(count);
    for (command, on) in [(id::RESET_ALL, can_reset), (id::NEW_PRESET, can_presets), (id::IMPORT_PRESETS, can_presets)] {
        let control = button(band, command);
        unsafe {
            if IsWindowEnabled(control).as_bool() != on {
                let _ = EnableWindow(control, on);
            }
        }
    }
    if old != text {
        let wide = crate::platform::wide(&text);
        unsafe {
            let _ = SetWindowTextW(count, crate::platform::pcwstr(&wide));
            let _ = InvalidateRect(count, None, false);
        }
        show_words(band, presets_shown(band));
    }
}

pub fn set_font(band: HWND, font: HFONT) {
    for command in [id::ADJUSTMENTS, id::PRESETS, EDIT_COUNT, id::RESET_ALL, id::NEW_PRESET, id::IMPORT_PRESETS] {
        unsafe {
            SendMessageW(button(band, command), WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }
    layout(band);
}

/// The words on one line 13 DIPs from the top, 16 DIPs apart, from the
/// gutter; each control is the word and the focus box's room around it.
fn layout(band: HWND) {
    let s = draw::dpi_of(band) as f64;
    let px = |dip: f64| (dip * s).round() as i32;
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(band, &mut client);
    }
    let margin = ink::MARGIN;
    let (top, height) = (px(13.0 - margin + 1.0), px(18.0 + 2.0 * margin - 2.0));
    let mut x = ink::GUTTER - margin;
    let place = |command: u16, width: f64, x: f64| unsafe {
        let _ = MoveWindow(button(band, command), px(x), top, px(width).max(0), height, true);
    };
    for (command, title) in [(id::ADJUSTMENTS, "Adjustments"), (id::PRESETS, "Presets")] {
        let width = ink::width(title, &ink::PANE_WORD) as f64 + 2.0 * margin;
        place(command, width, x);
        x += width - 2.0 * margin + 16.0;
    }
    // From the right: Reset or Import…, then the count or New.
    let mut right = client.right as f64 / s - ink::GUTTER + margin;
    let presets = presets_shown(band);
    let words: &[(u16, &str)] = if presets { &[(id::IMPORT_PRESETS, "Import…"), (id::NEW_PRESET, "New")] } else { &[(id::RESET_ALL, "Reset")] };
    for &(command, title) in words {
        let width = ink::width(title, &ink::WORD) as f64 + 2.0 * margin;
        place(command, width, right - width);
        right -= width - 2.0 * margin + 16.0;
    }
    if !presets {
        let text = draw::window_text(button(band, EDIT_COUNT));
        let (number, word) = text.split_once(' ').unwrap_or((&text, ""));
        let width = (ink::width(number, &ink::EDITS_NUMBER) + 4.0 + ink::width(word, &ink::EDITS)) as f64 + 2.0 * margin;
        place(EDIT_COUNT, width, right - width);
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_COMMAND => {
                let (command, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if code == BN_CLICKED {
                    match command {
                        id::ADJUSTMENTS | id::PRESETS => {
                            let pane = if command == id::PRESETS { Pane::Presets } else { Pane::Adjustments };
                            with_app(|app| app.model.show_pane(pane));
                        }
                        // Reset, New and Import… are the menus' commands.
                        _ => {
                            let _ = PostMessageW(GetAncestor(hwnd, GA_ROOT), WM_COMMAND, WPARAM(command as usize), LPARAM(0));
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DRAWITEM => {
                ink::draw_item(&*(lparam.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                // The column's colour; the tabs' rules come below the scope.
                draw::paint_panel(hwnd, theme::palette().panel);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
