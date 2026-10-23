//! Dropping a photograph on the canvas opens it. The canvas shows an outline
//! while a file is dragged over it; folders and other files are refused.

use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::implement;

use super::{App, with_app};
use crate::model::{dirty, is_photo};

#[implement(IDropTarget)]
struct Target;

/// The first file in a drag, if it is a photograph.
fn photograph(data: Option<&IDataObject>) -> Option<PathBuf> {
    let data = data?;
    let format = FORMATETC {
        cfFormat: 15, // CF_HDROP
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let mut medium = data.GetData(&format).ok()?;
        let drop = HDROP(medium.u.hGlobal.0);
        let length = DragQueryFileW(drop, 0, None);
        let mut buffer = vec![0u16; length as usize + 1];
        DragQueryFileW(drop, 0, Some(&mut buffer));
        ReleaseStgMedium(&mut medium);
        let path = PathBuf::from(crate::platform::from_wide(&buffer));
        (path.is_file() && is_photo(&path)).then_some(path)
    }
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
