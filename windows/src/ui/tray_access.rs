//! The tray for screen readers: an `IAccessible` over its window, whose
//! children are what `tray::spoken` lists (each group's name, the tiles and
//! their stars, the foot's index or count, Search, the lock's switch and
//! Done), in the Mac's words. UI Automation reads it through its MSAA
//! bridge, as it reads the rest of the app's controls, which are windows
//! annotated through `IAccPropServices`. The tiles are drawn, not windows,
//! so the tray answers for them itself. A press runs on the tray's own
//! message loop.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::*;
use windows_core::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BSTR, GUID, Interface, PCWSTR, Result, implement};

use super::draw;
use super::tray::{self, Press, Spoken};

/// Posted to the tray to press a child: the child's id in `wParam`.
pub const WM_APP_PRESS: u32 = WM_APP + 40;

const ROLE_GROUPING: i32 = 0x14;
const NAVDIR_NEXT: i32 = 5;
const NAVDIR_PREVIOUS: i32 = 6;
const NAVDIR_FIRSTCHILD: i32 = 7;
const NAVDIR_LASTCHILD: i32 = 8;

#[implement(IAccessible)]
struct Tray {
    hwnd: HWND,
}

/// The tray's answer to `WM_GETOBJECT` for its client area.
pub fn object(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let accessible: IAccessible = Tray { hwnd }.into();
    unsafe { LresultFromObject(&IAccessible::IID, wparam, &accessible) }
}

/// Tells screen readers what changed: the stocks listed (a search, a
/// star), or the one being tried.
pub fn changed(listed: bool, tried: bool) {
    let (hwnd, _) = tray::windows();
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        if listed {
            NotifyWinEvent(EVENT_OBJECT_REORDER, hwnd, OBJID_CLIENT.0, CHILDID_SELF as i32);
        }
        if tried && let Some(id) = selected() {
            NotifyWinEvent(EVENT_OBJECT_SELECTION, hwnd, OBJID_CLIENT.0, id);
        }
    }
}

/// The tile being tried, as a child id.
fn selected() -> Option<i32> {
    let (_, all) = tray::spoken()?;
    all.iter().position(|s| matches!(s.press, Press::Tile(tray_hit) if matches!(tray_hit, crate::tray_rules::Hit::Tile(..))) && s.state & tray::STATE_SELECTED != 0).map(|i| i as i32 + 1)
}

fn child(varchild: &VARIANT) -> Result<i32> {
    i32::try_from(varchild).map_err(|_| E_INVALIDARG.into())
}

impl Tray {
    /// What a child id names: None for the tray itself.
    fn part(&self, varchild: &VARIANT) -> Result<Option<Spoken>> {
        let id = child(varchild)?;
        if id == CHILDID_SELF as i32 {
            return Ok(None);
        }
        let (_, all) = tray::spoken().ok_or(windows::core::Error::from(E_FAIL))?;
        all.get((id - 1) as usize).cloned().map(Some).ok_or_else(|| E_INVALIDARG.into())
    }

    fn count(&self) -> i32 {
        tray::spoken().map_or(0, |(_, all)| all.len() as i32)
    }

    /// A place in DIPs in the tray as screen pixels (left, top, width, height).
    fn screen(&self, r: super::d2d::RectF) -> (i32, i32, i32, i32) {
        let s = draw::dpi_of(self.hwnd);
        let mut at = POINT { x: (r.x * s).round() as i32, y: (r.y * s).round() as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut at);
        }
        (at.x, at.y, (r.w * s).round() as i32, (r.h * s).round() as i32)
    }

    fn search(&self) -> Option<IDispatch> {
        let (_, field) = tray::windows();
        if field.is_invalid() {
            return None;
        }
        let mut object = std::ptr::null_mut();
        unsafe { AccessibleObjectFromWindow(field, OBJID_WINDOW.0 as u32, &IDispatch::IID, &mut object).ok()? };
        (!object.is_null()).then(|| unsafe { IDispatch::from_raw(object) })
    }
}

impl IDispatch_Impl for Tray_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }
    fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Invoke(&self, _: i32, _: *const GUID, _: u32, _: DISPATCH_FLAGS, _: *const DISPPARAMS, _: *mut VARIANT, _: *mut EXCEPINFO, _: *mut u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IAccessible_Impl for Tray_Impl {
    fn accParent(&self) -> Result<IDispatch> {
        let mut object = std::ptr::null_mut();
        unsafe { AccessibleObjectFromWindow(self.hwnd, OBJID_WINDOW.0 as u32, &IDispatch::IID, &mut object)? };
        if object.is_null() {
            return Err(E_FAIL.into());
        }
        Ok(unsafe { IDispatch::from_raw(object) })
    }
    fn accChildCount(&self) -> Result<i32> {
        Ok(self.count())
    }
    fn get_accChild(&self, varchild: &VARIANT) -> Result<IDispatch> {
        // Search is a window of its own; the rest are simple elements.
        match self.part(varchild)? {
            Some(spoken) if spoken.press == Press::Search => self.search().ok_or_else(|| S_FALSE.into()),
            _ => Err(S_FALSE.into()),
        }
    }
    fn get_accName(&self, varchild: &VARIANT) -> Result<BSTR> {
        Ok(match self.part(varchild)? {
            Some(spoken) => BSTR::from(spoken.name),
            None => BSTR::from(tray::spoken().map_or_else(String::new, |(name, _)| name)),
        })
    }
    fn get_accValue(&self, varchild: &VARIANT) -> Result<BSTR> {
        match self.part(varchild)? {
            Some(spoken) if !spoken.value.is_empty() => Ok(BSTR::from(spoken.value)),
            _ => Err(S_FALSE.into()),
        }
    }
    fn get_accDescription(&self, varchild: &VARIANT) -> Result<BSTR> {
        match self.part(varchild)? {
            Some(spoken) if !spoken.help.is_empty() => Ok(BSTR::from(spoken.help)),
            _ => Err(S_FALSE.into()),
        }
    }
    fn get_accRole(&self, varchild: &VARIANT) -> Result<VARIANT> {
        Ok(VARIANT::from(match self.part(varchild)? {
            Some(spoken) => spoken.role as i32,
            None => ROLE_GROUPING,
        }))
    }
    fn get_accState(&self, varchild: &VARIANT) -> Result<VARIANT> {
        Ok(VARIANT::from(self.part(varchild)?.map_or(0, |s| s.state) as i32))
    }
    fn get_accHelp(&self, varchild: &VARIANT) -> Result<BSTR> {
        self.get_accDescription(varchild)
    }
    fn get_accHelpTopic(&self, _: *mut BSTR, _: &VARIANT) -> Result<i32> {
        Err(S_FALSE.into())
    }
    fn get_accKeyboardShortcut(&self, _: &VARIANT) -> Result<BSTR> {
        Err(S_FALSE.into())
    }
    fn accFocus(&self) -> Result<VARIANT> {
        Ok(VARIANT::new())
    }
    fn accSelection(&self) -> Result<VARIANT> {
        Ok(selected().map_or_else(VARIANT::new, VARIANT::from))
    }
    fn get_accDefaultAction(&self, varchild: &VARIANT) -> Result<BSTR> {
        match self.part(varchild)? {
            Some(spoken) => match spoken.press {
                Press::Part(tray::Part::Keep) => Ok(BSTR::from(if spoken.state & 0x10 != 0 { "Uncheck" } else { "Check" })),
                Press::Search => Ok(BSTR::from("Focus")),
                Press::Nothing => Err(S_FALSE.into()),
                _ => Ok(BSTR::from("Press")),
            },
            None => Err(S_FALSE.into()),
        }
    }
    fn accSelect(&self, _: i32, _: &VARIANT) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn accLocation(&self, left: *mut i32, top: *mut i32, width: *mut i32, height: *mut i32, varchild: &VARIANT) -> Result<()> {
        let (x, y, w, h) = match self.part(varchild)? {
            Some(spoken) => self.screen(spoken.place),
            None => {
                let mut r = RECT::default();
                unsafe { GetWindowRect(self.hwnd, &mut r)? };
                (r.left, r.top, r.right - r.left, r.bottom - r.top)
            }
        };
        unsafe {
            *left = x;
            *top = y;
            *width = w;
            *height = h;
        }
        Ok(())
    }
    fn accNavigate(&self, navdir: i32, varstart: &VARIANT) -> Result<VARIANT> {
        let start = child(varstart)?;
        let count = self.count();
        let to = match navdir {
            NAVDIR_FIRSTCHILD if start == 0 && count > 0 => 1,
            NAVDIR_LASTCHILD if start == 0 && count > 0 => count,
            NAVDIR_NEXT if start > 0 && start < count => start + 1,
            NAVDIR_PREVIOUS if start > 1 => start - 1,
            _ => return Ok(VARIANT::new()),
        };
        Ok(VARIANT::from(to))
    }
    fn accHitTest(&self, x: i32, y: i32) -> Result<VARIANT> {
        let mut at = POINT { x, y };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut at);
        }
        let s = draw::dpi_of(self.hwnd);
        let (px, py) = (at.x as f32 / s, at.y as f32 / s);
        let Some((_, all)) = tray::spoken() else { return Ok(VARIANT::new()) };
        let inside = |r: &super::d2d::RectF| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h;
        // The stars over their tiles first.
        let hit = all
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state & 0x8000 == 0 && inside(&s.place))
            .max_by_key(|(_, s)| matches!(s.press, Press::Tile(crate::tray_rules::Hit::Star(..))) as u8)
            .map(|(i, _)| i as i32 + 1);
        Ok(VARIANT::from(hit.unwrap_or(CHILDID_SELF as i32)))
    }
    fn accDoDefaultAction(&self, varchild: &VARIANT) -> Result<()> {
        let id = child(varchild)?;
        match self.part(varchild)? {
            Some(spoken) if spoken.press != Press::Nothing => {
                unsafe { PostMessageW(self.hwnd, WM_APP_PRESS, WPARAM(id as usize), LPARAM(0))? };
                Ok(())
            }
            _ => Err(DISP_E_MEMBERNOTFOUND.into()),
        }
    }
    fn put_accName(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn put_accValue(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

/// Presses child `id` (`WM_APP_PRESS`), if it's still there.
pub fn press(id: usize) {
    let Some((_, all)) = tray::spoken() else { return };
    if let Some(spoken) = id.checked_sub(1).and_then(|i| all.get(i)) {
        tray::press(spoken.press);
    }
}
