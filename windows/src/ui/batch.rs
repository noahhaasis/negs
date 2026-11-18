//! The batch export's popover (the Mac's `BatchProgressView`, round 16 A):
//! over the status line's or the grid foot's Show, in ink on the panel's
//! colour. How far the batch has come under a pencil rule, what failed and
//! why, and Cancel, Retry Failed and Show in Explorer. A layered popup
//! painted with Direct2D; it closes when the window behind is clicked, on
//! Esc, or when it loses activation, and a finished batch is then forgotten.
//! Also the words the status line and the grid's foot say for a batch.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{self, Align, Ink, Painter, RectF, TextStyle};
use super::theme::{self, Tokens};
use super::{App, post};
use crate::platform::wide;

pub const WIDTH: f32 = 300.0;
const PAD: (f32, f32) = (16.0, 14.0);
const TITLE: TextStyle = TextStyle::serif(17.0, 600);
const LINE: TextStyle = TextStyle::serif(14.0, 400).italic();
const FAILED: TextStyle = TextStyle::serif(15.0, 600);
const NAME: TextStyle = TextStyle::ui(12.5, 400);
const REASON: TextStyle = TextStyle::serif(13.5, 400).italic();
const BUTTON: TextStyle = TextStyle::serif(13.5, 500);
const BUTTON_HEIGHT: f32 = 23.0;
const RADIUS: f32 = 8.0;
/// Failures listed before "and N more".
const LISTED: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Cancel,
    Retry,
    Show,
}

impl Button {
    fn title(self) -> &'static str {
        match self {
            Button::Cancel => "Cancel",
            Button::Retry => "Retry Failed",
            Button::Show => "Show in Explorer",
        }
    }
}

/// What the popover shows, taken from the model's batch.
struct View {
    title: String,
    rule: (f64, f64),
    line: String,
    failures: Vec<(String, String)>,
    skipped: usize,
    running: bool,
}

impl View {
    fn of(app: &App) -> Option<View> {
        let run = app.model.batch.as_ref()?;
        let current = run.current.filter(|_| run.running).and_then(|i| run.journal.items.get(i));
        let folder = run.journal.destination.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        Some(View {
            title: run.title(),
            rule: run.rule_parts(),
            line: current.map_or_else(|| format!("To “{folder}”"), |i| i.title.clone()),
            failures: run.failures().iter().map(|i| (i.title.clone(), i.reason.clone().unwrap_or_else(|| "Unknown error".into()))).collect(),
            skipped: run.skipped(),
            running: run.running,
        })
    }

    fn buttons(&self) -> Vec<Button> {
        let mut buttons = vec![];
        if self.running {
            buttons.push(Button::Cancel);
        } else if !self.failures.is_empty() {
            buttons.push(Button::Retry);
        }
        buttons.push(Button::Show);
        buttons
    }
}

struct Popover {
    hwnd: HWND,
    main: HWND,
    /// The word it hangs over, on screen.
    anchor: RECT,
    hot: Option<Button>,
    pressed: Option<Button>,
    places: Vec<(RectF, Button)>,
}

thread_local! {
    static POPOVER: RefCell<Option<Popover>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

fn register() {
    if REGISTERED.with(|r| r.replace(true)) {
        return;
    }
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsBatchPopover"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

pub fn shown() -> bool {
    POPOVER.with(|p| p.borrow().is_some())
}

/// Show over `anchor` (on screen), or close it if it shows.
pub fn toggle(app: &mut App, anchor: RECT) {
    if shown() {
        return close(app);
    }
    if app.model.batch.is_none() {
        return;
    }
    register();
    let hwnd = unsafe {
        CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW, w!("NegsBatchPopover"), w!("Export"), WS_POPUP, anchor.left, anchor.top, 1, 1, app.main, None, super::instance(), None)
    };
    let Ok(hwnd) = hwnd else { return };
    POPOVER.with(|p| *p.borrow_mut() = Some(Popover { hwnd, main: app.main, anchor, hot: None, pressed: None, places: vec![] }));
    app.model.batch_popover_shown = true;
    render(app);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
    }
}

/// Closes it; a finished batch is then forgotten (`Model::clear_batch`).
pub fn close(app: &mut App) {
    let Some(popover) = POPOVER.with(|p| p.borrow_mut().take()) else { return };
    unsafe {
        let _ = DestroyWindow(popover.hwnd);
    }
    app.model.batch_popover_shown = false;
    app.model.clear_batch();
}

/// The batch moved on: draw it again, if it shows.
pub fn refresh() {
    if shown() {
        post(|app: &mut App| {
            if app.model.batch.is_none() {
                close(app);
            } else {
                render(app);
            }
        });
    }
}

/// The card's height for `view`, as `draw` lays it out.
fn measure(view: &View, plain: bool) -> f32 {
    let m = |text: &str, style: &TextStyle, width: f32| d2d::measure_text(text, style, width, plain).height;
    let inner = WIDTH - 2.0 * PAD.0;
    let mut h = PAD.1 + m(&view.title, &TITLE, inner) + 10.0 + 2.0 + 8.0 + m("Ag", &LINE, 0.0);
    if !view.failures.is_empty() {
        h += 21.0 + m("1 failed", &FAILED, 0.0) + 6.0;
        for (name, reason) in view.failures.iter().take(LISTED) {
            h += m(name, &NAME, 0.0) + 1.0 + m(reason, &REASON, inner).min(REASON_LIMIT) + 7.0;
        }
        if view.failures.len() > LISTED {
            h += m("and more", &REASON, 0.0);
        }
    }
    if view.skipped > 0 {
        h += 8.0 + m("1 skipped: a file of its name exists.", &REASON, inner);
    }
    h + 14.0 + BUTTON_HEIGHT + PAD.1
}

/// A failure's reason shows at most three lines.
const REASON_LIMIT: f32 = 3.0 * 13.5 * 1.3;

fn draw(p: &mut Painter, view: &View, t: &Tokens, h: f32, popover: &mut Popover) {
    let card = RectF::new(0.5, 0.5, WIDTH - 1.0, h - 1.0);
    p.fill_round_rect(RectF::new(0.0, 0.0, WIDTH, h), RADIUS, Ink::hex(t.hex.panel));
    p.stroke_round_rect(card, RADIUS - 0.5, 1.0, Ink::hex(t.hex.line));
    let inner = WIDTH - 2.0 * PAD.0;
    let x = PAD.0;
    let mut y = PAD.1;
    y += p.text_in(&view.title, &TITLE, RectF::new(x, y, inner, 0.0), Align::Leading, Ink::hex(t.hex.paper)).height + 10.0;
    // The rule: `track` across, yellow for what exported, `dim` for the rest.
    p.fill_rect(RectF::new(x, y + 0.5, inner, 1.0), Ink::hex(t.hex.track));
    let exported = inner * view.rule.0 as f32;
    p.fill_rect(RectF::new(x, y, exported, 2.0), Ink::hex(t.hex.accent));
    p.fill_rect(RectF::new(x + exported, y + 0.5, inner * view.rule.1 as f32, 1.0), Ink::hex(t.hex.dim));
    y += 2.0 + 8.0;
    p.clipped(RectF::new(x, y, inner, 40.0), |p| {
        p.text(&view.line, &LINE, x, y, Ink::hex(t.hex.label));
    });
    y += p.measure("Ag", &LINE, 0.0).height;
    if !view.failures.is_empty() {
        y += 10.0;
        p.fill_rect(RectF::new(x, y, inner, 1.0), Ink::hex(t.hex.line));
        y += 11.0;
        let count = view.failures.len();
        let words = if count == 1 { "1 failed".to_string() } else { format!("{count} failed") };
        y += p.text(&words, &FAILED, x, y, Ink::hex(t.hex.paper)).height + 6.0;
        for (name, reason) in view.failures.iter().take(LISTED) {
            p.clipped(RectF::new(x, y, inner, 30.0), |p| {
                p.text(name, &NAME, x, y, Ink::hex(t.hex.label));
            });
            y += p.measure(name, &NAME, 0.0).height + 1.0;
            let height = p.measure(reason, &REASON, inner).height.min(REASON_LIMIT);
            p.clipped(RectF::new(x, y, inner, height), |p| {
                p.text_in(reason, &REASON, RectF::new(x, y, inner, 0.0), Align::Leading, Ink::hex(t.hex.dim));
            });
            y += height + 7.0;
        }
        if count > LISTED {
            y += p.text(&format!("and {} more", count - LISTED), &REASON, x, y, Ink::hex(t.hex.dim)).height;
        }
    }
    if view.skipped > 0 {
        y += 8.0;
        let words = if view.skipped == 1 { "1 skipped: a file of its name exists.".to_string() } else { format!("{} skipped: files of their names exist.", view.skipped) };
        y += p.text_in(&words, &REASON, RectF::new(x, y, inner, 0.0), Align::Leading, Ink::hex(t.hex.dim)).height;
    }
    y += 14.0;
    // Cancel or Retry Failed on the left, Show in Explorer on the right.
    let mut places = vec![];
    for button in view.buttons() {
        let words = p.measure(button.title(), &BUTTON, 0.0);
        let w = words.width + 24.0;
        let left = if button == Button::Show { x + inner - w } else { x };
        let r = RectF::new(left, y, w, BUTTON_HEIGHT);
        let hot = popover.hot == Some(button);
        let pressed = hot && popover.pressed == Some(button);
        if hot {
            p.fill_round_rect(r, BUTTON_HEIGHT / 2.0, Ink::hex(t.hex.paper).alpha(if pressed { 0.09 } else { 0.04 }));
        }
        p.stroke_round_rect(RectF::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), BUTTON_HEIGHT / 2.0 - 0.5, 1.0, Ink::hex(if hot { t.hex.dim } else { t.hex.track }));
        let baseline = r.y + r.h / 2.0 + BUTTON.size * 0.36;
        p.text(button.title(), &BUTTON, r.x + 12.0, baseline - words.baseline, Ink::hex(t.hex.paper));
        places.push((r, button));
    }
    popover.places = places;
}

/// Draws the popover for the batch as it is now, above its anchor.
fn render(app: &mut App) {
    let Some(view) = View::of(app) else { return };
    POPOVER.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(popover) = cell.as_mut() else { return };
        let scale = unsafe { GetDpiForWindow(popover.main) }.max(96) as f32 / 96.0;
        let t = theme::tokens();
        let h = measure(&view, t.plain).max(60.0);
        let (w_px, h_px) = ((WIDTH * scale).ceil() as i32, (h * scale).ceil() as i32);
        // Above the word, its left edge under it, kept on the window's monitor.
        let mut x = popover.anchor.left - (12.0 * scale) as i32;
        let y = popover.anchor.top - h_px - (6.0 * scale) as i32;
        unsafe {
            let monitor = MonitorFromWindow(popover.main, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                x = x.min(info.rcWork.right - w_px).max(info.rcWork.left);
            }
        }
        layered(popover.hwnd, x, y.max(0), w_px, h_px, scale, t.plain, |p| draw(p, &view, &t, h, popover));
    });
}

/// Paints premultiplied pixels into the layered window at `x, y`.
fn layered(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, scale: f32, plain: bool, paint: impl FnMut(&mut Painter)) {
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = SelectObject(dc, bitmap);
            d2d::paint_layer(dc, w, h, scale, plain, paint);
            let _ = GdiFlush();
            let size = SIZE { cx: w, cy: h };
            let at = POINT { x, y };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(hwnd, screen, Some(&at), Some(&size), dc, Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA);
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap);
        }
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
}

fn button_at(lparam: LPARAM) -> Option<Button> {
    POPOVER.with(|p| {
        let p = p.borrow();
        let p = p.as_ref()?;
        let scale = unsafe { GetDpiForWindow(p.main) }.max(96) as f32 / 96.0;
        let (x, y) = ((lparam.0 & 0xffff) as i16 as f32 / scale, ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale);
        p.places.iter().find(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(_, b)| *b)
    })
}

fn set(f: impl FnOnce(&mut Popover)) {
    POPOVER.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            f(p);
        }
    });
}

fn press(app: &mut App, button: Button) {
    match button {
        Button::Cancel => app.model.cancel_batch(),
        Button::Retry => app.model.retry_batch(),
        Button::Show => {
            let (folder, written) = app.model.batch_written();
            show_in_explorer(&folder, &written);
        }
    }
    render(app);
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                let hot = button_at(lparam);
                let changed = POPOVER.with(|p| p.borrow_mut().as_mut().is_some_and(|p| std::mem::replace(&mut p.hot, hot) != hot));
                if changed {
                    post(|app: &mut App| render(app));
                }
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                set(|p| p.hot = None);
                post(|app: &mut App| render(app));
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let at = button_at(lparam);
                set(|p| {
                    p.pressed = at;
                    p.hot = at;
                });
                if at.is_some() {
                    SetCapture(hwnd);
                }
                post(|app: &mut App| render(app));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let at = button_at(lparam);
                let pressed = POPOVER.with(|p| p.borrow_mut().as_mut().and_then(|p| p.pressed.take()));
                if let Some(button) = pressed.filter(|b| Some(*b) == at) {
                    post(move |app: &mut App| press(app, button));
                } else {
                    post(|app: &mut App| render(app));
                }
                LRESULT(0)
            }
            WM_KEYDOWN if VIRTUAL_KEY(wparam.0 as u16) == VK_ESCAPE => {
                post(|app: &mut App| close(app));
                LRESULT(0)
            }
            // Transient, as the Mac's: a click elsewhere closes it.
            WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
                post(|app: &mut App| close(app));
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
            WM_SETCURSOR => {
                let _ = SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The batch's files selected in Explorer, or its folder.
pub fn show_in_explorer(folder: &Path, files: &[PathBuf]) {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFindLastID, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW};
    use windows::core::PCWSTR;
    unsafe {
        if files.is_empty() {
            let path = wide(folder);
            ShellExecuteW(None, w!("open"), PCWSTR(path.as_ptr()), None, None, SW_SHOWNORMAL);
            return;
        }
        let parent = files[0].parent().unwrap_or(folder);
        let parent = wide(parent);
        let dir = ILCreateFromPathW(PCWSTR(parent.as_ptr()));
        if dir.is_null() {
            return;
        }
        let items: Vec<_> = files.iter().map(|f| ILCreateFromPathW(PCWSTR(wide(f).as_ptr()))).filter(|i| !i.is_null()).collect();
        let children: Vec<_> = items.iter().map(|i| ILFindLastID(*i) as *const _).collect();
        let _ = SHOpenFolderAndSelectItems(dir, Some(&children), 0);
        for item in items {
            ILFree(Some(item));
        }
        ILFree(Some(dir));
    }
}

/// Offers to resume a batch the last session didn't finish, once launch
/// has settled (the Mac's `offerBatchResume`).
pub fn offer_resume(app: &mut App) {
    let Some(journal) = app.model.unfinished_batch() else { return };
    let count = journal.remaining();
    let photographs = if count == 1 { "1 photograph".to_string() } else { format!("{count} photographs") };
    let folder = journal.destination.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let title = match journal.options.lightroom.as_deref() {
        Some(kind) => format!("Resume sending {photographs} to {}?", crate::batch::lightroom_title(kind)),
        None => format!("Resume exporting {photographs} to “{folder}”?"),
    };
    let alert = super::alert::Alert::new(title, Some("Negs quit before the export finished. Each photograph exports with its edits as they were when you chose Export.".into()))
        .buttons(&["Resume", "Discard"], Some(1));
    post(move |app: &mut App| {
        let answer = super::alert::ask(app.main, alert);
        app.model.resume_batch(journal, answer == 0);
    });
}
