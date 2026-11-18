//! The photograph's views that come and go with keys (IMPLEMENTATION.md
//! §5.2): View › Image Only (F), which leaves the photograph with its
//! caption and label alone in the window, and holding T, which shows the
//! photograph upside down to judge its balance. Cropping ends both.

use windows::Win32::UI::Input::KeyboardAndMouse::VK_T;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::App;
use crate::model::dirty;

#[derive(Default)]
pub struct ViewModes {
    /// View › Image Only: no film strip, status line or adjustments.
    pub image_only: bool,
    /// T held: the photograph upside down, by the key's scan code.
    upside_down: Option<u32>,
}

impl ViewModes {
    pub fn upside_down(&self) -> bool {
        self.upside_down.is_some()
    }
}

/// View › Image Only, on or off. Starting it outside a photograph does
/// nothing; cropping keeps the panel it needs.
pub fn toggle_image_only(app: &mut App) {
    let on = !app.view.image_only && app.model.source.is_some() && !app.model.cropping();
    set_image_only(app, on);
}

fn set_image_only(app: &mut App, on: bool) {
    if app.view.image_only != on {
        app.view.image_only = on;
        app.model.dirty |= dirty::LAYOUT | dirty::CANVAS | dirty::CHROME;
    }
}

/// Hold T to see the photograph upside down. True if the key was used.
pub fn key(app: &mut App, msg: &MSG, typing: bool) -> bool {
    let scan = ((msg.lParam.0 >> 16) & 0xff) as u32;
    match msg.message {
        WM_KEYUP | WM_SYSKEYUP if app.view.upside_down == Some(scan) => {
            app.view.upside_down = None;
            app.model.dirty |= dirty::CANVAS;
            true
        }
        WM_KEYDOWN if msg.wParam.0 == VK_T.0 as usize && !typing && !super::modifier_down() => {
            if msg.lParam.0 & (1 << 30) == 0 && app.model.image.is_some() && !app.model.cropping() {
                app.view.upside_down = Some(scan);
                app.model.dirty |= dirty::CANVAS;
            }
            true
        }
        _ => false,
    }
}

/// Escape, after an alert, typing and cropping have had it: leaves Image
/// Only. True if it did.
pub fn escape(app: &mut App) -> bool {
    if app.view.image_only {
        set_image_only(app, false);
        return true;
    }
    false
}

/// Cropping and closing the photograph end both views; so does the window
/// losing the keyboard while T is down.
pub fn sync(app: &mut App) {
    if app.model.cropping() || app.model.source.is_none() {
        set_image_only(app, false);
        if app.view.upside_down.take().is_some() {
            app.model.dirty |= dirty::CANVAS;
        }
    }
}

/// The window lost the keyboard: the T held is let go.
pub fn release(app: &mut App) {
    if app.view.upside_down.take().is_some() {
        app.model.dirty |= dirty::CANVAS;
    }
}
