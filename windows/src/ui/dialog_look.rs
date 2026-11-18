//! The export dialogs' own controls.
//!
//! The file dialogs come up dark in a dark theme and light in a light one
//! (`theme::init`, `theme::dialogs_dark`). The controls `IFileDialogCustomize` adds don't
//! follow. On Windows 10 their texts paint on the light system face and
//! their edit boxes white (Export 8 Photographs, 2026-11-19). Visual
//! groups' labels are drawn by the dialog itself in a grey that reads as
//! disabled, so the export dialogs give each control a text of its own as
//! its label instead (`dialogs.rs`).
//!
//! Once a dialog is up, `dress` finds those windows. When the dialog is
//! dark, their parents answer `WM_CTLCOLOR*` in its dark colours, and the
//! edit boxes take the dark theme of the dialog's own fields. `dress` also
//! finds the batch dialog's example, so the example can wrap to fit.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::Shell::{DefSubclassProc, IFileDialog, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Interface, PCWSTR, w};
use serde_json::json;

use super::theme::{self, Color, rgb};

/// The pane under Windows 10's dark file dialog, where the custom controls
/// sit, and its text, as measured in a screenshot (Darkroom, 2026-11-19).
const PANE: Color = rgb(56, 56, 56);
const TEXT: Color = rgb(255, 255, 255);
const DIM: Color = rgb(128, 128, 128);

const SUBCLASS: usize = 0x4e44;

/// One window of the dialog.
#[derive(Clone, Debug)]
pub struct Child {
    pub hwnd: HWND,
    pub class: String,
    pub text: String,
    pub id: i32,
    pub parent: HWND,
}

/// What `dress` found.
#[derive(Default)]
pub struct Dressed {
    /// The static showing the batch's example, if there is one.
    pub example: Option<HWND>,
}

pub fn window_of(dialog: &IFileDialog) -> Option<HWND> {
    let window: IOleWindow = dialog.cast().ok()?;
    unsafe { window.GetWindow() }.ok().filter(|h| !h.is_invalid())
}

/// Every window under `top`, at any depth.
pub fn children(top: HWND) -> Vec<Child> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        unsafe {
            let found = &mut *(found.0 as *mut Vec<Child>);
            let mut name = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut name) as usize;
            let mut text = [0u16; 256];
            let t = GetWindowTextW(hwnd, &mut text) as usize;
            found.push(Child {
                hwnd,
                class: String::from_utf16_lossy(&name[..n]),
                text: String::from_utf16_lossy(&text[..t]),
                id: GetDlgCtrlID(hwnd),
                parent: GetParent(hwnd).unwrap_or_default(),
            });
        }
        TRUE
    }
    let mut found: Vec<Child> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(top, Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Whether `child` is one of the texts or edit boxes added through
/// `IFileDialogCustomize`. On Windows 10 each such control sits alone in a
/// `FloatNotifySink` window under the dialog's view, with id 0 (traced
/// 2026-11-19); the dialog's own Folder field and label don't.
fn is_custom(child: &Child, all: &[Child]) -> bool {
    let class = child.class.to_ascii_lowercase();
    (class == "static" || class == "edit") && all.iter().any(|p| p.hwnd == child.parent && p.class.eq_ignore_ascii_case("FloatNotifySink"))
}

/// Once the dialog is up: traces its windows (`NEGS_RENDER_TRACE`), and
/// when the dialog is dark recolours the custom controls' parents' answers
/// and darkens their edit boxes.
pub fn dress(dialog: &IFileDialog) -> Dressed {
    let Some(top) = window_of(dialog) else { return Dressed::default() };
    let all = children(top);
    if crate::trace::enabled() {
        for c in &all {
            let (style, enabled, visible, rect) = unsafe {
                let mut r = RECT::default();
                let _ = GetWindowRect(c.hwnd, &mut r);
                (GetWindowLongW(c.hwnd, GWL_STYLE) as u32, IsWindowEnabled(c.hwnd).as_bool(), IsWindowVisible(c.hwnd).as_bool(), r)
            };
            crate::trace::event("file_dialog_window", || {
                json!({
                    "hwnd": c.hwnd.0 as usize, "parent": c.parent.0 as usize, "class": c.class, "text": c.text, "id": c.id,
                    "style": format!("{style:08x}"), "enabled": enabled, "visible": visible,
                    "rect": [rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top],
                    "custom": is_custom(c, &all),
                })
            });
        }
    }
    let custom: Vec<&Child> = all.iter().filter(|c| is_custom(c, &all)).collect();
    let example = custom.iter().find(|c| c.class.eq_ignore_ascii_case("static") && c.text.starts_with("For example")).map(|c| c.hwnd);
    if theme::dialogs_dark() {
        let mut hosts: Vec<HWND> = custom.iter().map(|c| c.parent).filter(|p| !p.is_invalid()).collect();
        hosts.sort_by_key(|h| h.0 as usize);
        hosts.dedup();
        unsafe {
            for host in hosts {
                let _ = SetWindowSubclass(host, Some(host_subclass), SUBCLASS, 0);
            }
            for c in custom.iter().filter(|c| c.class.eq_ignore_ascii_case("edit")) {
                let _ = SetWindowTheme(c.hwnd, w!("DarkMode_CFD"), PCWSTR::null());
                theme::allow_dark(c.hwnd);
            }
            let _ = RedrawWindow(top, None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
        }
    }
    Dressed { example }
}

/// The custom controls' parents: their statics and edit boxes in the dark
/// dialog's pane and text, dimmed when disabled.
extern "system" fn host_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                let dc = HDC(wparam.0 as *mut _);
                let child = HWND(lparam.0 as *mut _);
                SetTextColor(dc, COLORREF(if IsWindowEnabled(child).as_bool() { TEXT } else { DIM }));
                SetBkColor(dc, COLORREF(PANE));
                LRESULT(theme::brush(PANE).0 as isize)
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(host_subclass), SUBCLASS);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Sets `static_`'s text to `text`, broken into lines that fit its width:
/// at spaces, and after an underscore or before a full stop inside a long
/// file name, which has no spaces to wrap at.
pub fn set_fitted(static_: HWND, text: &str) {
    unsafe {
        let mut client = RECT::default();
        if GetClientRect(static_, &mut client).is_err() || client.right <= 0 {
            return;
        }
        let dc = GetDC(static_);
        let font = HFONT(SendMessageW(static_, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _);
        let old = (!font.is_invalid()).then(|| SelectObject(dc, font));
        let measure = |s: &str| {
            let w: Vec<u16> = s.encode_utf16().collect();
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &w, &mut size);
            size.cx as f32
        };
        let fitted = wrap(text, client.right as f32, measure);
        if let Some(old) = old {
            SelectObject(dc, old);
        }
        ReleaseDC(static_, dc);
        let wide = crate::platform::wide(&fitted);
        let _ = SetWindowTextW(static_, crate::platform::pcwstr(&wide));
    }
}

/// `text` in lines no wider than `width`, joined by CR LF. Breaks at spaces
/// (dropped), after `_` and `-`, and before `.`; a piece wider than the
/// line stays whole.
pub fn wrap(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    // Pieces, each with whether a space came before it.
    let mut pieces: Vec<(bool, String)> = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        let mut piece = String::new();
        let mut first = true;
        for ch in word.chars() {
            if ch == '.' && !piece.is_empty() {
                pieces.push((i > 0 && first, std::mem::take(&mut piece)));
                first = false;
            }
            piece.push(ch);
            if ch == '_' || ch == '-' {
                pieces.push((i > 0 && first, std::mem::take(&mut piece)));
                first = false;
            }
        }
        if !piece.is_empty() {
            pieces.push((i > 0 && first, piece));
        }
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for (spaced, piece) in pieces {
        let joined = if line.is_empty() { piece.clone() } else if spaced { format!("{line} {piece}") } else { format!("{line}{piece}") };
        if line.is_empty() || measure(&joined) <= width {
            line = joined;
        } else {
            lines.push(std::mem::take(&mut line));
            line = piece;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn a_long_file_name_wraps_after_its_underscores() {
        let measure = |s: &str| s.chars().count() as f32;
        assert_eq!(wrap("For example: _NHA9786_kodak_gold_200.png", 30.0, measure), "For example: _NHA9786_kodak_\r\ngold_200.png");
        assert_eq!(wrap("For example: IMG_1.jpg", 30.0, measure), "For example: IMG_1.jpg");
        assert_eq!(wrap("For example: _NHA9786_kodak_gold_200.png", 12.0, measure), "For example:\r\n_NHA9786_\r\nkodak_gold_\r\n200.png");
        // A piece wider than the line stays whole.
        assert_eq!(wrap("ab abcdefgh", 4.0, measure), "ab\r\nabcdefgh");
    }
}
