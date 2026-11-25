//! What Windows users expect beyond the window (`docs/windows-ui-features.md`
//! §11, §13, §14), as `windows/` does it:
//! - taskbar progress while exporting (`ITaskbarList3`), indeterminate for
//!   one photograph and counted for a batch, and the button flashing when an
//!   export finishes while Negs is in the background;
//! - errors in a TaskDialog titled "Negs", the exporter's log under
//!   "See details";
//! - the edits and the session saved when Windows logs off, shuts down or
//!   restarts for an update (`WM_ENDSESSION`), where no dialog may ask.
//!
//! Off Windows these do nothing, and errors use GPUI's prompt.

use gpui::Window;

/// What the taskbar button shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    None,
    Busy,
    /// Done and total photographs.
    Count(u64, u64),
}

/// An error's words: its first sentence as the instruction, the rest as
/// content, and an exporter's log (after the first line) as details.
pub fn split_error(message: &str) -> (String, String, Option<String>) {
    let (first, log) = message.split_once('\n').unwrap_or((message, ""));
    let first = first.trim();
    let (instruction, content) = match first.find(". ") {
        Some(end) if !first[end + 2..].trim().is_empty() => (first[..=end].trim_end_matches('.').to_string(), first[end + 2..].trim().to_string()),
        _ => (first.to_string(), String::new()),
    };
    let log = log.trim();
    (instruction, content, (!log.is_empty()).then(|| log.to_string()))
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;

    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Controls::*;
    use windows::Win32::UI::Shell::{DefSubclassProc, ITaskbarList3, SetWindowSubclass, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GetForegroundWindow, WM_ENDSESSION, WM_QUERYENDSESSION};
    use windows::core::{HSTRING, PCWSTR};

    use super::Progress;

    pub fn hwnd(window: &Window) -> Option<HWND> {
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(w) => Some(HWND(w.hwnd.get() as *mut _)),
            _ => None,
        }
    }

    thread_local! {
        static TASKBAR: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
        static SHOWN: RefCell<Progress> = const { RefCell::new(Progress::None) };
        static END_SESSION: RefCell<Option<Box<dyn FnMut()>>> = RefCell::new(None);
    }

    pub fn set_progress(window: &Window, progress: Progress) {
        if SHOWN.with(|s| *s.borrow() == progress) {
            return;
        }
        let Some(hwnd) = hwnd(window) else { return };
        TASKBAR.with(|t| {
            let mut t = t.borrow_mut();
            if t.is_none() && progress != Progress::None {
                *t = unsafe { CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok() }.filter(|list| unsafe { list.HrInit().is_ok() });
            }
            if let Some(list) = t.as_ref() {
                unsafe {
                    let _ = match progress {
                        Progress::None => list.SetProgressState(hwnd, TBPF_NOPROGRESS),
                        Progress::Busy => list.SetProgressState(hwnd, TBPF_INDETERMINATE),
                        Progress::Count(done, total) => list.SetProgressState(hwnd, TBPF_NORMAL).and_then(|_| list.SetProgressValue(hwnd, done, total.max(1))),
                    };
                }
            }
        });
        SHOWN.with(|s| *s.borrow_mut() = progress);
    }

    pub fn flash_if_background(window: &Window) {
        let Some(hwnd) = hwnd(window) else { return };
        unsafe {
            if GetForegroundWindow() != hwnd {
                let info = FLASHWINFO { cbSize: std::mem::size_of::<FLASHWINFO>() as u32, hwnd, dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG, uCount: 3, dwTimeout: 0 };
                let _ = FlashWindowEx(&info);
            }
        }
    }

    /// A TaskDialog titled "Negs"; the index of the button pressed.
    pub fn task_dialog(owner: isize, error: bool, instruction: &str, content: &str, details: Option<&str>, buttons: &[&str]) -> usize {
        let title = HSTRING::from("Negs");
        let instruction = HSTRING::from(instruction);
        let content = HSTRING::from(content);
        let details = details.map(HSTRING::from);
        let labels: Vec<HSTRING> = buttons.iter().map(|b| HSTRING::from(*b)).collect();
        let list: Vec<TASKDIALOG_BUTTON> = labels.iter().enumerate().map(|(i, l)| TASKDIALOG_BUTTON { nButtonID: 100 + i as i32, pszButtonText: PCWSTR(l.as_ptr()) }).collect();
        let mut config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: HWND(owner as *mut _),
            dwFlags: TDF_POSITION_RELATIVE_TO_WINDOW | TDF_SIZE_TO_CONTENT | TDF_ALLOW_DIALOG_CANCELLATION,
            pszWindowTitle: PCWSTR(title.as_ptr()),
            pszMainInstruction: PCWSTR(instruction.as_ptr()),
            pszContent: if content.is_empty() { PCWSTR::null() } else { PCWSTR(content.as_ptr()) },
            cButtons: list.len() as u32,
            pButtons: if list.is_empty() { std::ptr::null() } else { list.as_ptr() },
            dwCommonButtons: if list.is_empty() { TDCBF_OK_BUTTON } else { TASKDIALOG_COMMON_BUTTON_FLAGS(0) },
            ..Default::default()
        };
        config.Anonymous1.pszMainIcon = if error { TD_ERROR_ICON } else { TD_INFORMATION_ICON };
        if let Some(d) = &details {
            config.pszExpandedInformation = PCWSTR(d.as_ptr());
            let see = HSTRING::from("See details");
            let hide = HSTRING::from("Hide details");
            config.pszCollapsedControlText = PCWSTR(see.as_ptr());
            config.pszExpandedControlText = PCWSTR(hide.as_ptr());
            let mut pressed = 0;
            unsafe {
                let _ = TaskDialogIndirect(&config, Some(&mut pressed), None, None);
            }
            return (pressed - 100).max(0) as usize;
        }
        let mut pressed = 0;
        unsafe {
            let _ = TaskDialogIndirect(&config, Some(&mut pressed), None, None);
        }
        (pressed - 100).max(0) as usize
    }

    unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
        match msg {
            // Never block logging off.
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    // Log off, shutdown or an update restart: save, ask nothing.
                    END_SESSION.with(|cb| {
                        if let Some(cb) = cb.borrow_mut().as_mut() {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cb));
                        }
                    });
                }
                LRESULT(0)
            }
            _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        }
    }

    /// `save` runs when the session ends, before Windows ends the process.
    pub fn on_end_session(window: &Window, save: Box<dyn FnMut()>) {
        let Some(hwnd) = hwnd(window) else { return };
        END_SESSION.with(|cb| *cb.borrow_mut() = Some(save));
        unsafe {
            let _: BOOL = SetWindowSubclass(hwnd, Some(subclass), 1, 0);
        }
    }

    pub fn owner(window: &Window) -> isize {
        hwnd(window).map_or(0, |h| h.0 as isize)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Progress;
    use gpui::Window;

    pub fn set_progress(_window: &Window, _progress: Progress) {}
    pub fn flash_if_background(_window: &Window) {}
    pub fn on_end_session(_window: &Window, _save: Box<dyn FnMut()>) {}
}

pub use imp::*;

/// Shows an error: its first sentence as the instruction, the rest below
/// it, an exporter's log as details.
pub fn show_error(window: &mut Window, cx: &mut gpui::App, message: &str, error: bool) {
    let (instruction, content, details) = split_error(message);
    show_message(window, cx, &instruction, &content, details, error);
}

/// A dialog with one button that waits for nothing: it runs after the
/// update that raised it.
pub fn show_message(window: &mut Window, cx: &mut gpui::App, instruction: &str, content: &str, details: Option<String>, error: bool) {
    #[cfg(windows)]
    {
        let owner = owner(window);
        let (instruction, content) = (instruction.to_string(), content.to_string());
        cx.foreground_executor()
            .spawn(async move {
                task_dialog(owner, error, &instruction, &content, details.as_deref(), &[]);
            })
            .detach();
    }
    #[cfg(not(windows))]
    {
        let mut detail = content.to_string();
        if let Some(log) = details {
            // The last lines of the log: the dialog has no details pane.
            let tail: Vec<&str> = log.lines().rev().take(12).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            detail = format!("{detail}\n\n{}", tail.join("\n")).trim().to_string();
        }
        let level = if error { gpui::PromptLevel::Critical } else { gpui::PromptLevel::Info };
        let _ = window.prompt(level, instruction, (!detail.is_empty()).then_some(detail.as_str()), &["OK"], cx);
    }
}

#[cfg(test)]
mod tests {
    use super::split_error;

    #[test]
    fn an_error_reads_as_instruction_content_and_log() {
        let (i, c, d) = split_error("Export failed. Try Reference instead.\nwgpu: device lost\nBackend: wgpu");
        assert_eq!(i, "Export failed");
        assert_eq!(c, "Try Reference instead.");
        assert_eq!(d.as_deref(), Some("wgpu: device lost\nBackend: wgpu"));
        let (i, c, d) = split_error("The image engine stopped unexpectedly");
        assert_eq!((i.as_str(), c.as_str(), d), ("The image engine stopped unexpectedly", "", None));
    }
}
