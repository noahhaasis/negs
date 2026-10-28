//! Dropping a photograph on the canvas opens it. The canvas shows an outline
//! while a file is dragged over it; folders and other files are refused.
//! Files dropped on the Presets panel import as presets, between the rows
//! they are dropped between.

use std::cell::Cell;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Controls::LVM_GETITEMCOUNT;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, SendMessageW};
use windows::core::implement;

use super::{App, with_app};
use crate::model::{dirty, is_photo};

#[implement(IDropTarget)]
struct Target;

/// The files in a drag.
fn files(data: Option<&IDataObject>) -> Vec<PathBuf> {
    let Some(data) = data else { return vec![] };
    let format = FORMATETC {
        cfFormat: 15, // CF_HDROP
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else { return vec![] };
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let paths = (0..count)
            .map(|i| {
                let length = DragQueryFileW(drop, i, None);
                let mut buffer = vec![0u16; length as usize + 1];
                DragQueryFileW(drop, i, Some(&mut buffer));
                PathBuf::from(crate::platform::from_wide(&buffer))
            })
            .collect();
        ReleaseStgMedium(&mut medium);
        paths
    }
}

/// The first file in a drag, if it is a photograph.
fn photograph(data: Option<&IDataObject>) -> Option<PathBuf> {
    let path = files(data).into_iter().next()?;
    (path.is_file() && is_photo(&path)).then_some(path)
}

fn highlight(on: bool) {
    with_app(|app: &mut App| {
        if app.drop_target_active != on {
            app.drop_target_active = on;
            app.model.dirty |= dirty::CANVAS;
            super::empty::highlight(app, on);
        }
    });
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let accepted = photograph(data).is_some();
        unsafe { *effect = if accepted { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        highlight(accepted);
        Ok(())
    }
    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let accepted = with_app(|app| app.drop_target_active).unwrap_or(false);
        unsafe { *effect = if accepted { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }
    fn DragLeave(&self) -> windows::core::Result<()> {
        highlight(false);
        Ok(())
    }
    fn Drop(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        highlight(false);
        let path = photograph(data);
        unsafe { *effect = if path.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if let Some(path) = path {
            super::post(move |app: &mut App| app.model.open(&path));
        }
        Ok(())
    }
}

pub fn register(app: &mut App) {
    let target: IDropTarget = Target.into();
    unsafe {
        let _ = RegisterDragDrop(app.canvas, &target);
        let _ = RegisterDragDrop(app.empty, &target);
    }
}

/// The Presets panel's target. Any files are offered; the import says which
/// ones the engine refuses.
#[implement(IDropTarget)]
struct PresetTarget {
    panel: HWND,
    list: HWND,
    accepted: Cell<bool>,
}

impl PresetTarget {
    fn rows(&self) -> usize {
        unsafe {
            if !IsWindowVisible(self.list).as_bool() {
                return 0;
            }
            SendMessageW(self.list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0.max(0) as usize
        }
    }

    /// Where files dropped at `point` go: before a row, or None for the end.
    fn position(&self, point: &POINTL) -> Option<usize> {
        let rows = self.rows();
        let at = super::presets::gap(self.list, POINT { x: point.x, y: point.y });
        (rows > 0 && at < rows).then_some(at)
    }

    fn track(&self, point: &POINTL) {
        let rows = self.rows();
        let at = (self.accepted.get() && rows > 0).then(|| self.position(point).unwrap_or(rows));
        super::presets::show_insert(self.list, at);
    }
}

impl IDropTarget_Impl for PresetTarget_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, point: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let accepted = !files(data).is_empty() && with_app(|app| app.model.can_change_presets()).unwrap_or(false);
        self.accepted.set(accepted);
        unsafe { *effect = if accepted { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        super::presets::highlight_drop(self.panel, accepted);
        self.track(point);
        Ok(())
    }
    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, point: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = if self.accepted.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        self.track(point);
        Ok(())
    }
    fn DragLeave(&self) -> windows::core::Result<()> {
        super::presets::highlight_drop(self.panel, false);
        super::presets::show_insert(self.list, None);
        Ok(())
    }
    fn Drop(&self, data: Option<&IDataObject>, _: MODIFIERKEYS_FLAGS, point: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        super::presets::highlight_drop(self.panel, false);
        super::presets::show_insert(self.list, None);
        let paths = if self.accepted.get() { files(data) } else { vec![] };
        unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
        if !paths.is_empty() {
            let position = self.position(point);
            super::post(move |app: &mut App| app.model.import_presets(paths, position));
        }
        Ok(())
    }
}

pub fn register_presets(panel: HWND, list: HWND) {
    let target: IDropTarget = PresetTarget { panel, list, accepted: Cell::new(false) }.into();
    unsafe {
        let _ = RegisterDragDrop(panel, &target);
        let _ = RegisterDragDrop(list, &target);
    }
}
