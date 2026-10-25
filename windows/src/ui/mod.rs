//! The editing window: menu bar, toolbar, the canvas column (photograph or
//! empty state, film strip), the adjustments column and the status bar.
//! Win32 with the common controls (docs/cross-platform-port.md, Windows UI).
//!
//! All state lives in one `App` on the UI thread. Window procedures reach it
//! through `with_app`, which refuses re-entrant calls: notifications caused by
//! the app's own changes to controls are ignored, and work posted from other
//! threads waits until the app is free. The canvas keeps its view state
//! separately (`canvas::VIEW`) so it can paint at any time.

pub mod canvas;
mod dialogs;
mod drop;
mod empty;
pub mod inspector;
mod strip;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::model::{Model, dirty};
use crate::platform::{self, pcwstr, wide};
use crate::session::{EditorSession, Preferences, SessionStore};

/// Annotate icon buttons and numeric editors without changing their visual text.
pub fn accessible_name(hwnd: HWND, name: &str) {
    use windows::Win32::UI::Accessibility::{CAccPropServices, IAccPropServices, PROPID_ACC_NAME};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    thread_local! {
        static PROPS: std::cell::OnceCell<Option<IAccPropServices>> = const { std::cell::OnceCell::new() };
    }
    PROPS.with(|cell| {
        let service = cell.get_or_init(|| unsafe { CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER).ok() });
        if let Some(service) = service {
            let name = wide(name);
            unsafe { let _ = service.SetHwndPropStr(hwnd, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_NAME, pcwstr(&name)); }
        }
    });
}

pub const WM_APP_RUN: u32 = WM_APP + 1;
const WM_APP_BUILD: u32 = WM_APP + 2;
const TIMER_RUN: usize = 1;
const TIMER_EXPORT: usize = 2;
const COPYDATA_OPEN: usize = 0x4e45_4753; // "NEGS"

pub mod id {
    pub const OPEN: u16 = 100;
    pub const SAVE_PREVIEW: u16 = 101;
    pub const EXPORT: u16 = 102;
    pub const SETTINGS: u16 = 103;
    pub const EXIT: u16 = 104;
    pub const RECENT: u16 = 110; // ..119
    pub const UNDO: u16 = 200;
    pub const REDO: u16 = 201;
    pub const CUT: u16 = 202;
    pub const COPY: u16 = 203;
    pub const PASTE: u16 = 204;
    pub const DELETE: u16 = 205;
    pub const SELECT_ALL: u16 = 206;
    pub const RESET_ALL: u16 = 207;
    pub const FILMSTRIP: u16 = 300;
    pub const ADJUSTMENTS: u16 = 301;
    pub const FIT: u16 = 302;
    pub const ACTUAL: u16 = 303;
    pub const ZOOM_IN: u16 = 304;
    pub const ZOOM_OUT: u16 = 305;
    pub const FULL_SCREEN: u16 = 306;
    pub const ROTATE: u16 = 400;
    pub const CROP: u16 = 401;
    pub const CROP_DONE: u16 = 402;
    pub const CROP_CANCEL: u16 = 403;
    pub const UPDATES: u16 = 500;
    pub const ABOUT: u16 = 501;
    pub const CANCEL_EXPORT: u16 = 600;
    pub const ORIGINAL: u16 = 601;
    pub const COPY_STATUS: u16 = 602;
    pub const EMPTY_OPEN: u16 = 700;
    pub const EMPTY_RECENT: u16 = 710; // ..714
}

// Constants windows-rs 0.58 lacks.
pub const TBM_GETPOS: u32 = WM_USER;
pub const TBM_SETRANGE_MIN_MAX: u32 = WM_USER + 6;

type Task = Box<dyn FnOnce(&mut App) + Send>;
static QUEUE: Mutex<VecDeque<Task>> = Mutex::new(VecDeque::new());
static TIMED: Mutex<Vec<(Instant, usize)>> = Mutex::new(Vec::new());
static MAIN: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TIMED_TASKS: RefCell<Vec<(Instant, Box<dyn FnOnce(&mut App)>)>> = const { RefCell::new(Vec::new()) };
}

/// Runs `task` on the UI thread with the app, from any thread.
pub fn post(task: impl FnOnce(&mut App) + Send + 'static) {
    QUEUE.lock().unwrap().push_back(Box::new(task));
    let main = MAIN.load(Ordering::Relaxed);
    if main != 0 {
        unsafe {
            let _ = PostMessageW(HWND(main as *mut _), WM_APP_RUN, WPARAM(0), LPARAM(0));
        }
    }
}

/// Runs `task` on the UI thread after `delay`.
pub fn after(delay: Duration, task: impl FnOnce(&mut App) + 'static) {
    let due = Instant::now() + delay;
    TIMED_TASKS.with(|t| t.borrow_mut().push((due, Box::new(task))));
    let main = MAIN.load(Ordering::Relaxed);
    let _ = TIMED.lock().map(|mut t| t.push((due, 0)));
    unsafe {
        SetTimer(HWND(main as *mut _), TIMER_RUN, delay.as_millis().max(1) as u32, None);
    }
}

/// The app, unless a call further up the stack is using it.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut borrowed = cell.try_borrow_mut().ok()?;
        let app = borrowed.as_mut()?;
        let result = f(app);
        app.refresh();
        Some(result)
    })
}

fn drain() {
    loop {
        let task = QUEUE.lock().unwrap().pop_front();
        let Some(task) = task else { break };
        let mut task = Some(task);
        let ran = with_app(|app| (task.take().unwrap())(app)).is_some();
        if !ran {
            // Busy (a modal dialog runs inside a handler): try again shortly.
            QUEUE.lock().unwrap().push_front(task.take().unwrap());
            unsafe {
                SetTimer(main_hwnd(), TIMER_RUN, 50, None);
            }
            break;
        }
    }
    let now = Instant::now();
    let due: Vec<Box<dyn FnOnce(&mut App)>> = TIMED_TASKS.with(|t| {
        let mut t = t.borrow_mut();
        let (ready, waiting): (Vec<_>, Vec<_>) = t.drain(..).partition(|(at, _)| *at <= now);
        *t = waiting;
        ready.into_iter().map(|(_, f)| f).collect()
    });
    for task in due {
        let mut task = Some(task);
        if with_app(|app| (task.take().unwrap())(app)).is_none() {
            TIMED_TASKS.with(|t| t.borrow_mut().push((now, task.take().unwrap())));
        }
    }
    let next = TIMED_TASKS.with(|t| t.borrow().iter().map(|(at, _)| *at).min());
    unsafe {
        match next {
            Some(at) => {
                SetTimer(main_hwnd(), TIMER_RUN, at.saturating_duration_since(Instant::now()).as_millis().max(1) as u32, None);
            }
            None if QUEUE.lock().unwrap().is_empty() => {
                let _ = KillTimer(main_hwnd(), TIMER_RUN);
            }
            None => {}
        }
    }
}

pub fn main_hwnd() -> HWND {
    HWND(MAIN.load(Ordering::Relaxed) as *mut _)
}

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).unwrap().into() }
}

/// QPC stamps for the startup report (`NEGS_STARTUP_REPORT`).
pub struct Report {
    file: Option<std::fs::File>,
    seen: Vec<&'static str>,
}

impl Report {
    fn new() -> Self {
        let file = std::env::var_os("NEGS_STARTUP_REPORT").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
        Self { file, seen: vec![] }
    }
    pub fn stamp(&mut self, name: &'static str) {
        use std::io::Write;
        if self.seen.contains(&name) {
            return;
        }
        self.seen.push(name);
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{name} {}", platform::qpc());
        }
    }
    pub fn line(&mut self, text: &str) {
        use std::io::Write;
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{text}");
        }
    }
    pub fn active(&self) -> bool {
        self.file.is_some()
    }
}

pub struct Fonts {
    pub ui: HFONT,
    pub bold: HFONT,
    pub small: HFONT,
    pub title: HFONT,
    pub glyph: HFONT,
    pub glyph_small: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        unsafe {
            let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
            let _ = windows::Win32::UI::HiDpi::SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                metrics.cbSize,
                Some(&mut metrics as *mut _ as *mut _),
                0,
                dpi,
            );
            let base = metrics.lfMessageFont;
            let ui = CreateFontIndirectW(&base);
            let bold = CreateFontIndirectW(&LOGFONTW { lfWeight: 600, ..base });
            let small = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 85 / 100, ..base });
            let title = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 19 / 10, lfWeight: 600, ..base });
            let glyph_face = if font_exists("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            let mut glyph = LOGFONTW { lfHeight: -(16 * dpi as i32 / 96), lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
            let face = wide(glyph_face);
            glyph.lfFaceName[..face.len()].copy_from_slice(&face);
            let glyph_font = CreateFontIndirectW(&glyph);
            let glyph_small = CreateFontIndirectW(&LOGFONTW { lfHeight: -(11 * dpi as i32 / 96), ..glyph });
            Fonts { ui, bold, small, title, glyph: glyph_font, glyph_small }
        }
    }
}

fn font_exists(face: &str) -> bool {
    unsafe {
        let dc = GetDC(None);
        let mut lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
        let w = wide(face);
        lf.lfFaceName[..w.len()].copy_from_slice(&w);
        extern "system" fn found(_: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, flag: LPARAM) -> i32 {
            unsafe { *(flag.0 as *mut bool) = true };
            0
        }
        let mut exists = false;
        EnumFontFamiliesExW(dc, &lf, Some(found), LPARAM(&mut exists as *mut bool as isize), 0);
        ReleaseDC(None, dc);
        exists
    }
}

pub struct App {
    pub model: Model,
    pub main: HWND,
    toolbar: HWND,
    status: HWND,
    progress: HWND,
    cancel: HWND,
    original_button: HWND,
    pub canvas: HWND,
    empty: HWND,
    strip: HWND,
    pub inspector: inspector::Inspector,
    pub fonts: Fonts,
    pub dpi: u32,
    menu: HMENU,
    accelerators: HACCEL,
    pub report: Report,
    splitter_drag: Option<(i32, f64)>,
    full_screen: Option<(WINDOWPLACEMENT, i32)>,
    pub strip_view: strip::Strip,
    pub empty_view: empty::Empty,
    taskbar: Option<ITaskbarList3>,
    original_key: Option<u32>,
    pub drop_target_active: bool,
    started: Instant,
    quitting: bool,
}

impl App {
    pub fn scale(&self) -> f64 {
        self.dpi as f64 / 96.0
    }
    pub fn px(&self, dip: f64) -> i32 {
        (dip * self.scale()).round() as i32
    }

    /// Updates every part the model marked as changed.
    pub fn refresh(&mut self) {
        let flags = std::mem::take(&mut self.model.dirty);
        if flags == 0 {
            return;
        }
        if flags & dirty::ERROR != 0 {
            if let Some(error) = self.model.error.take() {
                // After this handler returns, outside any borrow.
                let main = self.main.0 as isize;
                post(move |_| dialogs::error(HWND(main as *mut _), &error));
            }
        }
        if flags & (dirty::CHROME | dirty::STATUS) != 0 {
            self.update_chrome();
            empty::sync_recent(self);
            crate::updates::set_busy(self.model.exporting || self.model.saving);
        }
        if flags & dirty::STATUS != 0 {
            self.update_status();
        }
        if flags & (dirty::LAYOUT | dirty::CHROME) != 0 {
            self.layout();
        }
        if flags & (dirty::STRUCTURE | dirty::INSPECTOR | dirty::CROP | dirty::LAYOUT) != 0 || flags & dirty::HISTOGRAM != 0 {
            let structure = flags & (dirty::STRUCTURE | dirty::CROP | dirty::LAYOUT) != 0;
            inspector::sync(self, structure, flags & dirty::HISTOGRAM != 0);
        }
        if flags & (dirty::CANVAS | dirty::ZOOM | dirty::CROP) != 0 {
            canvas::update_scene(self);
            if self.model.image.is_some() && !self.model.first_render_shown {
                self.model.first_render_shown = true;
                self.report.stamp("first_image_scene");
            }
        }
        if flags & (dirty::STRIP | dirty::CHROME) != 0 {
            strip::sync(self);
        }
        if self.model.dirty != 0 {
            self.refresh();
        }
    }

    fn update_chrome(&mut self) {
        let model = &self.model;
        let photo = model.source.is_some();
        let title = match &model.source {
            Some(path) => format!("{} - Negs", crate::model::file_name(path)),
            None => "Negs".into(),
        };
        let title = wide(title);
        unsafe {
            let _ = SetWindowTextW(self.main, pcwstr(&title));
        }
        let check = |id: u16, on: bool| unsafe {
            SendMessageW(self.toolbar, TB_CHECKBUTTON, WPARAM(id as usize), LPARAM(on as isize));
        };
        let enable = |id: u16, on: bool| unsafe {
            SendMessageW(self.toolbar, TB_ENABLEBUTTON, WPARAM(id as usize), LPARAM(on as isize));
        };
        enable(id::FIT, photo && !model.cropping());
        enable(id::ACTUAL, photo && !model.cropping());
        enable(id::ROTATE, photo);
        enable(id::CROP, model.cropping() || model.image.is_some());
        enable(id::EXPORT, model.can_export());
        check(id::FILMSTRIP, model.prefs.filmstrip_visible);
        check(id::ADJUSTMENTS, model.inspector_visible);
        check(id::CROP, model.cropping());
        let exporting = model.exporting;
        unsafe {
            let _ = ShowWindow(self.progress, if exporting { SW_SHOWNA } else { SW_HIDE });
            let _ = ShowWindow(self.cancel, if exporting { SW_SHOWNA } else { SW_HIDE });
            let _ = EnableWindow(self.original_button, model.can_show_original());
            let _ = ShowWindow(self.original_button, if photo { SW_SHOWNA } else { SW_HIDE });
        }
        self.update_taskbar();
    }

    fn update_status(&mut self) {
        let model = &self.model;
        let mut text = model.status.clone();
        if model.status == "Starting…" && model.source.is_none() {
            text.clear();
        }
        let set = |part: usize, s: &str| unsafe {
            let w = wide(s);
            SendMessageW(self.status, SB_SETTEXTW, WPARAM(part), LPARAM(w.as_ptr() as isize));
        };
        set(0, &text);
        let stocks = if model.source.is_none() || !model.ready {
            String::new()
        } else if crate::json::boolean(model.value("io.scan_film")) {
            model.film_name()
        } else {
            format!("{} → {}", model.film_name(), model.paper_name())
        };
        set(1, &stocks);
        let export = if model.exporting { format!("Exporting · {} s", model.export_started.elapsed().as_secs()) } else { String::new() };
        set(2, &export);
        set(3, "");
        unsafe {
            if model.exporting {
                SetTimer(self.main, TIMER_EXPORT, 1000, None);
            } else {
                let _ = KillTimer(self.main, TIMER_EXPORT);
            }
        }
    }

    /// Indeterminate taskbar progress while exporting.
    fn update_taskbar(&mut self) {
        if self.taskbar.is_none() && self.model.exporting {
            self.taskbar = unsafe { windows::Win32::System::Com::CoCreateInstance(&TaskbarList, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER).ok() };
        }
        if let Some(taskbar) = &self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.main, if self.model.exporting { TBPF_INDETERMINATE } else { TBPF_NOPROGRESS });
            }
        }
    }

    /// Flashes the taskbar button when an export finishes in the background.
    pub fn export_finished(&mut self) {
        unsafe {
            if GetForegroundWindow() != self.main {
                let info = FLASHWINFO {
                    cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                    hwnd: self.main,
                    dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                    uCount: 3,
                    dwTimeout: 0,
                };
                let _ = FlashWindowEx(&info);
            }
        }
    }

    /// Lays out the toolbar, the canvas column, the adjustments and the status bar.
    pub fn layout(&mut self) {
        unsafe {
            let mut client = RECT::default();
            let _ = GetClientRect(self.main, &mut client);
            SendMessageW(self.toolbar, TB_AUTOSIZE, WPARAM(0), LPARAM(0));
            SendMessageW(self.status, WM_SIZE, WPARAM(0), LPARAM(0));
            let height = |hwnd: HWND| {
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                if IsWindowVisible(hwnd).as_bool() { r.bottom - r.top } else { 0 }
            };
            let top = height(self.toolbar);
            let bottom = client.bottom - height(self.status);
            let inspector_width = if self.model.inspector_visible { self.px(self.model.prefs.inspector_width.clamp(300.0, 440.0)) } else { 0 };
            let splitter = if self.model.inspector_visible { self.px(5.0) } else { 0 };
            let column = (client.right - inspector_width - splitter).max(0);
            let show_strip = self.model.prefs.filmstrip_visible && self.model.source.is_some();
            let strip_height = if show_strip { self.px(112.0) } else { 0 };
            let canvas_bottom = (bottom - strip_height).max(top);
            let photo = self.model.source.is_some();
            let _ = MoveWindow(self.canvas, 0, top, column, canvas_bottom - top, true);
            let _ = MoveWindow(self.empty, 0, top, column, canvas_bottom - top, true);
            let _ = ShowWindow(self.canvas, if photo { SW_SHOWNA } else { SW_HIDE });
            let _ = ShowWindow(self.empty, if photo { SW_HIDE } else { SW_SHOWNA });
            let _ = MoveWindow(self.strip, 0, canvas_bottom, column, strip_height, true);
            let _ = ShowWindow(self.strip, if show_strip { SW_SHOWNA } else { SW_HIDE });
            self.inspector.layout(column + splitter, top, inspector_width, bottom - top, self.model.inspector_visible);
            canvas::resized(self.canvas);
            // Their own WM_SIZE arrives while the app is busy here.
            empty::layout(self);
            strip::resized(self);
            self.layout_status();
        }
    }

    fn layout_status(&mut self) {
        unsafe {
            let mut client = RECT::default();
            let _ = GetClientRect(self.status, &mut client);
            let original = self.px(36.0);
            let export = if self.model.exporting { self.px(250.0) } else { 0 };
            let stocks = self.px(280.0);
            let right = client.right - self.px(18.0);
            let parts = [
                (right - original - export - stocks).max(self.px(80.0)),
                right - original - export,
                right - original,
                -1,
            ];
            SendMessageW(self.status, SB_SETPARTS, WPARAM(parts.len()), LPARAM(parts.as_ptr() as isize));
            let mut part = RECT::default();
            SendMessageW(self.status, SB_GETRECT, WPARAM(2), LPARAM(&mut part as *mut _ as isize));
            let inset = self.px(2.0);
            let bar = self.px(90.0);
            let cancel = self.px(64.0);
            let _ = MoveWindow(self.progress, part.right - cancel - bar - inset * 2, part.top + inset * 2, bar, part.bottom - part.top - inset * 4, true);
            let _ = MoveWindow(self.cancel, part.right - cancel - inset, part.top + inset / 2, cancel, part.bottom - part.top - inset, true);
            SendMessageW(self.status, SB_GETRECT, WPARAM(3), LPARAM(&mut part as *mut _ as isize));
            let _ = MoveWindow(self.original_button, part.left + inset, part.top + inset / 2, part.right - part.left - inset * 2, part.bottom - part.top - inset, true);
        }
    }

    pub fn command(&mut self, command: u16) {
        let model = &mut self.model;
        match command {
            id::OPEN | id::EMPTY_OPEN => dialogs::open(self),
            id::SAVE_PREVIEW => dialogs::save(self, false),
            id::EXPORT => dialogs::save(self, true),
            id::SETTINGS => dialogs::settings(self),
            id::EXIT => unsafe {
                let _ = PostMessageW(self.main, WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            c if (id::RECENT..id::RECENT + 10).contains(&c) => {
                if let Some(path) = model.prefs.recent_files.get((c - id::RECENT) as usize).cloned() {
                    model.open(&path);
                }
            }
            c if (id::EMPTY_RECENT..id::EMPTY_RECENT + 5).contains(&c) => {
                if let Some(path) = model.prefs.recent_files.get((c - id::EMPTY_RECENT) as usize).cloned() {
                    model.open(&path);
                }
            }
            id::UNDO => model.perform_undo(),
            id::REDO => model.perform_redo(),
            id::CUT | id::COPY | id::PASTE | id::DELETE | id::SELECT_ALL => edit_command(command),
            id::RESET_ALL => model.reset(),
            id::FILMSTRIP => {
                model.prefs.filmstrip_visible = !model.prefs.filmstrip_visible;
                model.prefs.save();
                model.dirty |= dirty::LAYOUT | dirty::CHROME | dirty::STRIP;
            }
            id::ADJUSTMENTS => {
                model.inspector_visible = !model.inspector_visible;
                model.dirty |= dirty::LAYOUT | dirty::CHROME;
            }
            id::FIT if model.source.is_some() && !model.cropping() => model.zoom(true),
            id::ACTUAL if model.source.is_some() && !model.cropping() => model.zoom(false),
            id::ZOOM_IN if model.source.is_some() && !model.cropping() => canvas::zoom_by(1.25),
            id::ZOOM_OUT if model.source.is_some() && !model.cropping() => canvas::zoom_by(0.8),
            id::FULL_SCREEN => self.toggle_full_screen(),
            id::ROTATE if model.source.is_some() => model.rotate(),
            id::CROP => {
                if model.cropping() {
                    model.end_crop(true)
                } else {
                    model.begin_crop()
                }
            }
            id::CROP_DONE => model.end_crop(true),
            id::CROP_CANCEL => model.end_crop(false),
            id::UPDATES => crate::updates::check(true),
            id::ABOUT => dialogs::about(self.main),
            id::CANCEL_EXPORT => model.cancel_export(),
            id::COPY_STATUS => copy_text(self.main, &self.model.status),
            _ => {}
        }
    }

    fn toggle_full_screen(&mut self) {
        unsafe {
            if let Some((placement, style)) = self.full_screen.take() {
                SetWindowLongW(self.main, GWL_STYLE, style);
                let _ = SetWindowPlacement(self.main, &placement);
                let _ = SetWindowPos(self.main, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
                let _ = SetMenu(self.main, self.menu);
            } else {
                let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let _ = GetWindowPlacement(self.main, &mut placement);
                let style = GetWindowLongW(self.main, GWL_STYLE);
                let monitor = MonitorFromWindow(self.main, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(monitor, &mut info);
                SetWindowLongW(self.main, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as i32));
                let _ = SetMenu(self.main, None);
                let r = info.rcMonitor;
                let _ = SetWindowPos(self.main, HWND_TOP, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED);
                self.full_screen = Some((placement, style));
            }
        }
    }

    /// Enables and titles the menu's items when it opens.
    fn update_menu(&self, menu: HMENU) {
        let model = &self.model;
        let photo = model.source.is_some();
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
        let editing = focused_class().as_deref() == Some("Edit");
        enable(id::SAVE_PREVIEW, model.can_save());
        enable(id::EXPORT, model.can_export());
        enable(id::UPDATES, crate::updates::configured());
        retitle(id::UNDO, &match model.undo_title() {
            Some(action) => format!("&Undo {action}\tCtrl+Z"),
            None => "&Undo\tCtrl+Z".into(),
        });
        retitle(id::REDO, &match model.redo_title() {
            Some(action) => format!("&Redo {action}\tCtrl+Y"),
            None => "&Redo\tCtrl+Y".into(),
        });
        enable(id::UNDO, editing || (model.undo_title().is_some() && !model.cropping()));
        enable(id::REDO, !editing && model.redo_title().is_some() && !model.cropping());
        for c in [id::CUT, id::COPY, id::PASTE, id::DELETE, id::SELECT_ALL] {
            enable(c, editing);
        }
        enable(id::RESET_ALL, model.has_catalog() && !model.cropping());
        check(id::FILMSTRIP, model.prefs.filmstrip_visible);
        check(id::ADJUSTMENTS, model.inspector_visible);
        check(id::FULL_SCREEN, self.full_screen.is_some());
        for c in [id::FIT, id::ACTUAL, id::ZOOM_IN, id::ZOOM_OUT] {
            enable(c, photo && !model.cropping());
        }
        enable(id::ROTATE, photo);
        retitle(id::CROP, if model.cropping() { "Done &Cropping\tC" } else { "&Crop\tC" });
        enable(id::CROP, model.cropping() || model.image.is_some());
    }

    fn update_recent_menu(&self, menu: HMENU) {
        unsafe {
            while GetMenuItemCount(menu) > 0 {
                let _ = DeleteMenu(menu, 0, MF_BYPOSITION);
            }
            if self.model.prefs.recent_files.is_empty() {
                let w = wide("(None)");
                let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, pcwstr(&w));
            }
            for (i, path) in self.model.prefs.recent_files.iter().enumerate() {
                let w = wide(format!("&{} {}", (i + 1) % 10, crate::model::file_name(path).replace('&', "&&")));
                let _ = AppendMenuW(menu, MF_STRING, (id::RECENT + i as u16) as usize, pcwstr(&w));
            }
        }
    }

    /// Hold `\` (by the character typed, AltGr allowed) or `O` to show the
    /// original, unless a text field has focus. True if the key was used.
    fn original_key(&mut self, msg: &MSG) -> bool {
        let scan = ((msg.lParam.0 >> 16) & 0xff) as u32;
        match msg.message {
            WM_KEYUP | WM_SYSKEYUP if self.original_key == Some(scan) => {
                self.original_key = None;
                self.model.hold_original(false);
                true
            }
            WM_CHAR if msg.wParam.0 == '\\' as usize && !text_focus() => {
                if msg.lParam.0 & (1 << 30) == 0 {
                    self.original_key = Some(scan);
                    self.model.hold_original(true);
                }
                true
            }
            WM_KEYDOWN if msg.wParam.0 == VK_O.0 as usize && !text_focus() && !modifier_down() => {
                if msg.lParam.0 & (1 << 30) == 0 {
                    self.original_key = Some(scan);
                    self.model.hold_original(true);
                }
                true
            }
            _ => false,
        }
    }

    fn release_original(&mut self) {
        if self.original_key.take().is_some() || self.model.holding_original {
            self.model.hold_original(false);
        }
    }

    /// Quitting asks while an export runs; the session is saved once.
    fn close(&mut self, ending_session: bool) -> bool {
        if self.quitting {
            return true;
        }
        if !ending_session {
            if self.model.saving {
                return false;
            }
            if self.model.exporting && !dialogs::confirm_quit_during_export(self.main) {
                return false;
            }
        }
        self.save_window();
        if let Some(session) = self.model.session().filter(|_| std::env::var_os("SPEKTRAFILM_BENCHMARK_PHOTO").is_none()) {
            if let Err(error) = SessionStore::default().save(&session) {
                if !ending_session {
                    self.model.error = Some(format!("Could not save the editing session. Negs stayed open. {error:#}"));
                    self.model.dirty |= dirty::ERROR;
                    return false;
                }
            }
        }
        self.quitting = true;
        crate::updates::stop();
        self.model.shutdown();
        true
    }

    fn save_window(&mut self) {
        unsafe {
            let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            if let Some((saved, _)) = &self.full_screen {
                placement = *saved;
            } else {
                let _ = GetWindowPlacement(self.main, &mut placement);
            }
            let r = placement.rcNormalPosition;
            let show = if placement.showCmd == SW_SHOWMAXIMIZED.0 as u32 { 3 } else { 1 };
            let s = self.scale();
            // Saved in DIPs, so a monitor with another scale restores the same size.
            self.model.prefs.window = Some([show, r.left, r.top, ((r.right - r.left) as f64 / s) as i32, ((r.bottom - r.top) as f64 / s) as i32]);
            self.model.prefs.save();
        }
    }
}

fn focused_class() -> Option<String> {
    unsafe {
        let focus = GetFocus();
        if focus.0.is_null() {
            return None;
        }
        let mut name = [0u16; 64];
        let n = GetClassNameW(focus, &mut name);
        Some(String::from_utf16_lossy(&name[..n as usize]))
    }
}

/// A text field or a list that types to select has focus.
fn text_focus() -> bool {
    matches!(focused_class().as_deref(), Some("Edit") | Some("ComboBox"))
}

fn modifier_down() -> bool {
    unsafe { [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].iter().any(|k| GetKeyState(k.0 as i32) < 0) }
}

fn edit_command(command: u16) {
    unsafe {
        let focus = GetFocus();
        let message = match command {
            id::CUT => WM_CUT,
            id::COPY => WM_COPY,
            id::PASTE => WM_PASTE,
            id::DELETE => WM_CLEAR,
            _ => EM_SETSEL,
        };
        if message == EM_SETSEL {
            SendMessageW(focus, EM_SETSEL, WPARAM(0), LPARAM(-1));
        } else {
            SendMessageW(focus, message, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn copy_text(owner: HWND, text: &str) {
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    unsafe {
        if OpenClipboard(owner).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let w = wide(text);
        if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2) {
            let p = GlobalLock(memory) as *mut u16;
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            let _ = GlobalUnlock(memory);
            let _ = SetClipboardData(13 /* CF_UNICODETEXT */, HANDLE(memory.0));
        }
        let _ = CloseClipboard();
    }
}

/// Adds a photograph to Windows' recent documents (the taskbar Jump List).
pub fn note_recent_document(path: &Path) {
    let w = wide(path);
    unsafe { SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(w.as_ptr() as *const _)) };
}

/// Creates a child or top-level window.
pub fn create(class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, parent: HWND, id: u16, font: HFONT) -> HWND {
    let text = wide(text);
    unsafe {
        let hwnd = CreateWindowExW(ex, class, pcwstr(&text), style, 0, 0, 0, 0, parent, HMENU(id as usize as *mut _), instance(), None).unwrap_or_default();
        if !font.is_invalid() {
            SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(0));
        }
        hwnd
    }
}

fn build_menu() -> (HMENU, HMENU) {
    unsafe {
        let bar = CreateMenu().unwrap();
        let add = |menu: HMENU, id: u16, text: &str| {
            let w = wide(text);
            let _ = AppendMenuW(menu, MF_STRING, id as usize, pcwstr(&w));
        };
        let separator = |menu: HMENU| {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        };
        let popup = |title: &str| {
            let menu = CreatePopupMenu().unwrap();
            let w = wide(title);
            let _ = AppendMenuW(bar, MF_POPUP, menu.0 as usize, pcwstr(&w));
            menu
        };
        let file = popup("&File");
        add(file, id::OPEN, "&Open Photograph…\tCtrl+O");
        let recent = CreatePopupMenu().unwrap();
        let w = wide("Recent &Photographs");
        let _ = AppendMenuW(file, MF_POPUP, recent.0 as usize, pcwstr(&w));
        separator(file);
        add(file, id::SAVE_PREVIEW, "&Save Preview…\tCtrl+S");
        add(file, id::EXPORT, "&Export Photograph…\tCtrl+Shift+E");
        separator(file);
        add(file, id::SETTINGS, "Se&ttings…");
        separator(file);
        add(file, id::EXIT, "E&xit\tAlt+F4");
        let edit = popup("&Edit");
        add(edit, id::UNDO, "&Undo\tCtrl+Z");
        add(edit, id::REDO, "&Redo\tCtrl+Y");
        separator(edit);
        add(edit, id::CUT, "Cu&t\tCtrl+X");
        add(edit, id::COPY, "&Copy\tCtrl+C");
        add(edit, id::PASTE, "&Paste\tCtrl+V");
        add(edit, id::DELETE, "&Delete\tDel");
        add(edit, id::SELECT_ALL, "Select &All\tCtrl+A");
        separator(edit);
        add(edit, id::RESET_ALL, "Reset All A&djustments\tCtrl+Shift+R");
        let view = popup("&View");
        add(view, id::FILMSTRIP, "&Film Strip\tF6");
        add(view, id::ADJUSTMENTS, "&Adjustments\tF7");
        separator(view);
        add(view, id::FIT, "Fi&t Photograph\tCtrl+0");
        add(view, id::ACTUAL, "Actual &Size\tCtrl+1");
        add(view, id::ZOOM_IN, "Zoom &In\tCtrl+=");
        add(view, id::ZOOM_OUT, "Zoom &Out\tCtrl+-");
        separator(view);
        add(view, id::FULL_SCREEN, "F&ull Screen\tF11");
        let image = popup("&Image");
        add(image, id::ROTATE, "&Rotate Clockwise\tCtrl+R");
        add(image, id::CROP, "&Crop\tC");
        let help = popup("&Help");
        add(help, id::UPDATES, "Check for &Updates…");
        separator(help);
        add(help, id::ABOUT, "&About Negs");
        (bar, recent)
    }
}

fn accelerators() -> HACCEL {
    let ctrl = FCONTROL | FVIRTKEY;
    let shift_ctrl = FCONTROL | FSHIFT | FVIRTKEY;
    let table = [
        (ctrl, 'O' as u16, id::OPEN),
        (ctrl, 'S' as u16, id::SAVE_PREVIEW),
        (shift_ctrl, 'E' as u16, id::EXPORT),
        (ctrl, 'Z' as u16, id::UNDO),
        (ctrl, 'Y' as u16, id::REDO),
        (shift_ctrl, 'Z' as u16, id::REDO),
        (shift_ctrl, 'R' as u16, id::RESET_ALL),
        (FVIRTKEY, VK_F6.0, id::FILMSTRIP),
        (FVIRTKEY, VK_F7.0, id::ADJUSTMENTS),
        (FVIRTKEY, VK_F11.0, id::FULL_SCREEN),
        (ctrl, '0' as u16, id::FIT),
        (ctrl, VK_NUMPAD0.0, id::FIT),
        (ctrl, '1' as u16, id::ACTUAL),
        (ctrl, VK_NUMPAD1.0, id::ACTUAL),
        (ctrl, VK_OEM_PLUS.0, id::ZOOM_IN),
        (ctrl, VK_ADD.0, id::ZOOM_IN),
        (ctrl, VK_OEM_MINUS.0, id::ZOOM_OUT),
        (ctrl, VK_SUBTRACT.0, id::ZOOM_OUT),
        (ctrl, 'R' as u16, id::ROTATE),
        (ctrl, 'K' as u16, id::CROP),
        (FVIRTKEY, 'C' as u16, id::CROP),
    ];
    let accels: Vec<ACCEL> = table.iter().map(|&(f, key, cmd)| ACCEL { fVirt: f, key, cmd }).collect();
    unsafe { CreateAcceleratorTableW(&accels).unwrap_or_default() }
}

/// Segoe icon glyphs drawn at the window's DPI, for the toolbar.
fn toolbar_images(dpi: u32) -> HIMAGELIST {
    // Open, Film Strip, Fit, Actual Size, Rotate, Crop, Export, Adjustments
    let glyphs = ['\u{E838}', '\u{E8B9}', '\u{E9A6}', '\u{E71E}', '\u{E7AD}', '\u{E7A8}', '\u{E78C}', '\u{E9E9}'];
    let size = (20 * dpi / 96) as i32;
    unsafe {
        let list = ImageList_Create(size, size, ILC_COLOR32, glyphs.len() as i32, 0);
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        let face = if font_exists("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
        let mut lf = LOGFONTW { lfHeight: -(16 * dpi as i32 / 96), lfQuality: ANTIALIASED_QUALITY, ..Default::default() };
        let w = wide(face);
        lf.lfFaceName[..w.len()].copy_from_slice(&w);
        let font = CreateFontIndirectW(&lf);
        for glyph in glyphs {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else { continue };
            let old = SelectObject(dc, bitmap);
            let old_font = SelectObject(dc, font);
            SetBkColor(dc, COLORREF(0));
            SetTextColor(dc, COLORREF(0xffffff));
            let mut r = RECT { left: 0, top: 0, right: size, bottom: size };
            let mut text: Vec<u16> = glyph.to_string().encode_utf16().collect();
            DrawTextW(dc, &mut text, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            let _ = GdiFlush();
            // White on black → dark glyph with the coverage as alpha (premultiplied).
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
            for p in pixels.chunks_exact_mut(4) {
                let a = p[1];
                let ink = (0x20u16 * a as u16 / 255) as u8;
                p[0] = ink;
                p[1] = ink;
                p[2] = ink;
                p[3] = a;
            }
            SelectObject(dc, old_font);
            SelectObject(dc, old);
            ImageList_Add(list, bitmap, None);
            let _ = DeleteObject(bitmap);
        }
        let _ = DeleteObject(font);
        let _ = DeleteDC(dc);
        list
    }
}

fn build_toolbar(main: HWND, dpi: u32) -> HWND {
    unsafe {
        let toolbar = create(
            TOOLBARCLASSNAMEW,
            "",
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(TBSTYLE_FLAT | TBSTYLE_LIST | TBSTYLE_TOOLTIPS | CCS_NODIVIDER as u32 | CCS_TOP as u32),
            WINDOW_EX_STYLE(0),
            main,
            0,
            HFONT::default(),
        );
        SendMessageW(toolbar, TB_BUTTONSTRUCTSIZE, WPARAM(std::mem::size_of::<TBBUTTON>()), LPARAM(0));
        SendMessageW(toolbar, TB_SETEXTENDEDSTYLE, WPARAM(0), LPARAM((TBSTYLE_EX_MIXEDBUTTONS | 0x80 /* TBSTYLE_EX_DOUBLEBUFFER */) as isize));
        SendMessageW(toolbar, TB_SETIMAGELIST, WPARAM(0), LPARAM(toolbar_images(dpi).0));
        let tips: Vec<Vec<u16>> = [
            "Open photograph (Ctrl+O)",
            "Show or hide film strip (F6)",
            "Fit photograph (Ctrl+0)",
            "Actual size (Ctrl+1)",
            "Rotate clockwise (Ctrl+R)",
            "Crop the photograph (C)",
            "Export photograph (Ctrl+Shift+E)",
            "Show or hide adjustments (F7)",
        ]
        .iter()
        .map(wide)
        .collect();
        let button = |image: i32, cmd: u16, style: u32, tip: usize| TBBUTTON {
            iBitmap: image,
            idCommand: cmd as i32,
            fsState: TBSTATE_ENABLED as u8,
            fsStyle: style as u8,
            iString: tips[tip].as_ptr() as isize,
            ..Default::default()
        };
        let separator = TBBUTTON { iBitmap: 8, fsStyle: BTNS_SEP as u8, ..Default::default() };
        let buttons = [
            button(0, id::OPEN, BTNS_BUTTON, 0),
            button(1, id::FILMSTRIP, BTNS_CHECK, 1),
            separator,
            button(2, id::FIT, BTNS_BUTTON, 2),
            button(3, id::ACTUAL, BTNS_BUTTON, 3),
            separator,
            button(4, id::ROTATE, BTNS_BUTTON, 4),
            button(5, id::CROP, BTNS_CHECK, 5),
            separator,
            button(6, id::EXPORT, BTNS_BUTTON, 6),
            button(7, id::ADJUSTMENTS, BTNS_CHECK, 7),
        ];
        SendMessageW(toolbar, TB_ADDBUTTONSW, WPARAM(buttons.len()), LPARAM(buttons.as_ptr() as isize));
        SendMessageW(toolbar, TB_AUTOSIZE, WPARAM(0), LPARAM(0));
        toolbar
    }
}

/// Another Negs is running: hand it the photograph and bring it forward.
fn forward_to_running(path: Option<&Path>) -> bool {
    unsafe {
        let name = w!("Local\\Negs.SingleInstance");
        let _mutex = CreateMutexW(None, true, name);
        if GetLastError() != ERROR_ALREADY_EXISTS {
            std::mem::forget(_mutex);
            return false;
        }
        let Ok(window) = FindWindowW(w!("NegsMainWindow"), PCWSTR::null()) else { return false };
        let mut pid = 0;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(window, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        let text = wide(path.map(|p| p.as_os_str().to_os_string()).unwrap_or_default());
        let data = COPYDATASTRUCT { dwData: COPYDATA_OPEN, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut _ };
        SendMessageW(window, WM_COPYDATA, WPARAM(0), LPARAM(&data as *const _ as isize));
        true
    }
}

/// Startup, in the order `windows-ui-features.md` §17 asks for: helper and
/// catalog first, the window's first frame next, then the adjustments, the
/// GPU and everything else.
pub fn run() {
    // Held until process exit, including test launches with multiple instances.
    // The installer/uninstaller waits for every app to stop before touching data.
    let _app_lifetime_mutex = unsafe { CreateMutexW(None, false, w!("Local\\Negs.AppLifetime")) };
    let launched = platform::qpc();
    let path = std::env::args_os().skip(1).map(PathBuf::from).find(|p| !p.to_string_lossy().starts_with('-') && p.is_file());
    let path = path.map(|p| std::path::absolute(&p).unwrap_or(p));
    if std::env::var_os("NEGS_ALLOW_MULTIPLE").is_none() && forward_to_running(path.as_deref()) {
        return;
    }
    let mut report = Report::new();
    report.line(&format!("launched {launched}"));
    report.line(&format!("frequency {}", platform::qpc_frequency()));
    report.stamp("main");
    let prefs = Preferences::load();
    let mut model = Model::new(prefs);
    model.launch();
    report.stamp("engine_started");
    let benchmark = std::env::var_os("SPEKTRAFILM_BENCHMARK_PHOTO").is_some();
    let explicit = path.is_some();
    if !benchmark && !explicit {
        model.expect_restoration();
        std::thread::spawn(|| {
            let result = SessionStore::default().load();
            post(move |app: &mut App| app.model.restoration_loaded(result));
        });
    }
    unsafe {
        let _ = OleInitialize(None);
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("dev.negs.app"));
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_STANDARD_CLASSES | ICC_WIN95_CLASSES | ICC_LINK_CLASS | ICC_TAB_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let icon = LoadIconW(instance(), PCWSTR(1 as *const u16)).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(main_proc),
            hInstance: instance(),
            lpszClassName: w!("NegsMainWindow"),
            hCursor: cursor,
            hIcon: icon,
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassExW(&class);
        canvas::register();
        empty::register();
        strip::register();
        inspector::register();

        let (menu, recent) = build_menu();
        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("NegsMainWindow"),
            w!("Negs"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1320,
            860,
            None,
            menu,
            instance(),
            None,
        )
        .unwrap();
        MAIN.store(main.0 as isize, Ordering::Relaxed);
        let dpi = GetDpiForWindow(main).max(96);
        restore_placement(main, &model.prefs, dpi);
        report.stamp("main_window");
        let fonts = Fonts::new(dpi);
        report.stamp("fonts");
        let toolbar = build_toolbar(main, dpi);
        report.stamp("toolbar");
        let status = create(STATUSCLASSNAMEW, "", WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SBARS_SIZEGRIP), WINDOW_EX_STYLE(0), main, 0, fonts.ui);
        empty::subclass_status(status);
        let progress = create(PROGRESS_CLASSW, "", WS_CHILD | WINDOW_STYLE(PBS_MARQUEE), WINDOW_EX_STYLE(0), status, 0, HFONT::default());
        SendMessageW(progress, PBM_SETMARQUEE, WPARAM(1), LPARAM(30));
        let cancel = create(w!("BUTTON"), "Cancel", WS_CHILD | WS_TABSTOP, WINDOW_EX_STYLE(0), status, id::CANCEL_EXPORT, fonts.ui);
        let original_button = create(w!("BUTTON"), "\u{E8B9}", WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), status, id::ORIGINAL, fonts.glyph_small);
        accessible_name(original_button, "Show Original (toggle with Space; hold O or backslash)");
        empty::subclass_original_button(original_button);
        let canvas = create(w!("NegsCanvas"), "Photograph preview", WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let empty = create(w!("NegsEmpty"), "", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
        let strip = create(w!("NegsStrip"), "Film Strip", WS_CHILD | WS_TABSTOP | WS_HSCROLL, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let inspector = inspector::Inspector::create(main, &fonts, dpi);
        canvas::init(canvas, dpi);

        let mut app = App {
            model,
            main,
            toolbar,
            status,
            progress,
            cancel,
            original_button,
            canvas,
            empty,
            strip,
            inspector,
            fonts,
            dpi,
            menu,
            accelerators: accelerators(),
            report,
            splitter_drag: None,
            full_screen: None,
            strip_view: strip::Strip::default(),
            empty_view: empty::Empty::default(),
            taskbar: None,
            original_key: None,
            drop_target_active: false,
            started: Instant::now(),
            quitting: false,
        };
        let _ = recent;
        empty::build(&mut app);
        app.layout();
        app.update_chrome();
        app.update_status();
        app.report.stamp("window_created");
        APP.with(|cell| *cell.borrow_mut() = Some(app));

        let show = with_app(|app| app.model.prefs.window.map_or(1, |w| w[0])).unwrap_or(1);
        let _ = ShowWindow(main, if show == 3 { SW_SHOWMAXIMIZED } else { SW_SHOWNORMAL });
        let _ = RedrawWindow(main, None, None, RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW);
        with_app(|app| app.report.stamp("window_shown"));
        // Complete the first composition before creating the inspector. This
        // timestamp bounds visible presentation, rather than a queued WM_PAINT.
        let composited = DwmFlush().is_ok();
        with_app(|app| app.report.stamp(if composited { "first_window_composited" } else { "first_window_flush_failed" }));
        // The adjustments, the GPU and the rest wait for the first frame.
        let _ = PostMessageW(main, WM_APP_BUILD, WPARAM(0), LPARAM(0));
        if let Some(path) = path {
            with_app(|app| app.model.open(&path));
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if pre_translate(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|cell| cell.borrow_mut().take());
    }
}

fn restore_placement(main: HWND, prefs: &Preferences, dpi: u32) {
    let Some([_, x, y, w, h]) = prefs.window else { return };
    let s = dpi as f64 / 96.0;
    let rect = RECT { left: x, top: y, right: x + (w as f64 * s) as i32, bottom: y + (h as f64 * s) as i32 };
    unsafe {
        // A monitor that is gone leaves the default placement.
        if MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_invalid() {
            return;
        }
        let placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: SW_HIDE.0 as u32,
            rcNormalPosition: rect,
            ..Default::default()
        };
        let _ = SetWindowPlacement(main, &placement);
    }
}

/// Keys before dialog navigation: Show Original, crop's Return and Escape,
/// and the accelerators, which never fire while a text field has focus.
fn pre_translate(msg: &MSG) -> bool {
    let main = main_hwnd();
    let mut accelerators = None;
    let handled = with_app(|app| {
        if matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MOUSEWHEEL) {
            app.report.stamp("first_input_received");
        }
        if app.original_key(msg) {
            return true;
        }
        if msg.message == WM_KEYDOWN && app.model.cropping() && !text_focus() {
            if msg.wParam.0 == VK_RETURN.0 as usize {
                app.model.end_crop(true);
                return true;
            }
            if msg.wParam.0 == VK_ESCAPE.0 as usize {
                app.model.end_crop(false);
                return true;
            }
        }
        if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
            let focus = focused_class();
            let skip = match focus.as_deref() {
                Some("Edit") => true,
                // Lists type to select: leave them unmodified letters.
                Some("ComboBox") => !modifier_down() && (b'A' as usize..=b'Z' as usize).contains(&msg.wParam.0),
                _ => false,
            };
            if !skip {
                accelerators = Some(app.accelerators);
            }
        }
        false
    })
    .unwrap_or(false);
    if handled {
        return true;
    }
    // TranslateAccelerator synchronously sends WM_COMMAND. Release the App
    // borrow first or main_proc's with_app will reject the command as reentrant.
    if let Some(table) = accelerators {
        if unsafe { TranslateAcceleratorW(main, table, msg) } != 0 {
            return true;
        }
    }
    unsafe { IsDialogMessageW(main, msg).as_bool() }
}

extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_APP_RUN => {
                drain();
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_RUN => {
                drain();
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_EXPORT => {
                with_app(|app| app.update_status());
                LRESULT(0)
            }
            WM_APP_BUILD => {
                with_app(|app| {
                    app.report.stamp("first_frame_done");
                    inspector::build(app);
                    app.report.stamp("inspector_built");
                    drop::register(app);
                    canvas::start_gpu(app.canvas);
                    crate::updates::schedule();
                });
                // After the inspector's first paint: ready for input.
                let _ = PostMessageW(hwnd, WM_APP_BUILD + 10, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            m if m == WM_APP_BUILD + 10 => {
                with_app(|app| {
                    app.report.stamp("inspector_shown");
                    inspector::report_geometry(app);
                });
                LRESULT(0)
            }
            WM_SIZE => {
                with_app(|app| app.layout());
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                info.ptMinTrackSize = POINT { x: 760 * dpi / 96, y: 520 * dpi / 96 };
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let rect = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, SWP_NOZORDER | SWP_NOACTIVATE);
                with_app(|app| app.dpi_changed((wparam.0 & 0xffff) as u32));
                LRESULT(0)
            }
            WM_COMMAND => {
                let command = (wparam.0 & 0xffff) as u16;
                let code = ((wparam.0 >> 16) & 0xffff) as u32;
                if code == BN_CLICKED || code == 1 || lparam.0 == 0 {
                    with_app(|app| app.command(command));
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.code == NM_CLICK || header.code == NM_RETURN {
                    if let Ok(id) = u16::try_from(header.idFrom) {
                        if (id::EMPTY_RECENT..id::EMPTY_RECENT + 5).contains(&id) {
                            with_app(|app| app.command(id));
                        }
                    }
                }
                if header.code == NM_RCLICK && header.hwndFrom != HWND::default() {
                    with_app(|app| {
                        if header.hwndFrom == app.status {
                            app.status_menu();
                        }
                    });
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_INITMENUPOPUP => {
                let menu = HMENU(wparam.0 as *mut _);
                with_app(|app| {
                    let file = GetSubMenu(app.menu, 0);
                    if menu == GetSubMenu(file, 1) {
                        app.update_recent_menu(menu);
                    } else {
                        app.update_menu(menu);
                    }
                });
                LRESULT(0)
            }
            WM_COPYDATA => {
                let data = &*(lparam.0 as *const COPYDATASTRUCT);
                if data.dwData == COPYDATA_OPEN {
                    let text = std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2);
                    let path = PathBuf::from(platform::from_wide(text));
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                    if path.is_file() {
                        post(move |app: &mut App| app.model.open(&path));
                    }
                }
                LRESULT(1)
            }
            WM_ACTIVATE => {
                if wparam.0 & 0xffff == WA_INACTIVE as usize {
                    with_app(|app| app.release_original());
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_MOVE | WM_DISPLAYCHANGE => {
                canvas::monitor_may_have_changed();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_SETCURSOR => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                if with_app(|app| app.on_splitter(p.x, p.y)).unwrap_or(false) {
                    let _ = SetCursor(LoadCursorW(None, IDC_SIZEWE).unwrap_or_default());
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
                with_app(|app| {
                    if app.on_splitter(x, y) {
                        app.splitter_drag = Some((x, app.model.prefs.inspector_width));
                        SetCapture(hwnd);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                with_app(|app| app.drag_splitter(x));
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                with_app(|app| {
                    if app.splitter_drag.take().is_some() {
                        app.model.prefs.save();
                    }
                });
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                if with_app(|app| app.close(false)).unwrap_or(false) {
                    canvas::shutdown();
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    // Log off, shutdown or an update restart: save, no dialogs.
                    with_app(|app| app.close(true));
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

impl App {
    fn on_splitter(&self, x: i32, y: i32) -> bool {
        if !self.model.inspector_visible {
            return self.splitter_drag.is_some();
        }
        let r = self.inspector.rect;
        self.splitter_drag.is_some() || (x >= r.left - self.px(5.0) && x < r.left && y >= r.top && y < r.bottom)
    }

    /// Dragging the splitter resizes the adjustments; dragging it closed hides them.
    fn drag_splitter(&mut self, x: i32) {
        let Some((start, width)) = self.splitter_drag else { return };
        let target = width + (start - x) as f64 / self.scale();
        if target < 200.0 {
            if self.model.inspector_visible {
                self.model.inspector_visible = false;
                self.model.dirty |= dirty::LAYOUT | dirty::CHROME;
            }
            return;
        }
        if !self.model.inspector_visible {
            self.model.inspector_visible = true;
        }
        self.model.prefs.inspector_width = target.clamp(300.0, 440.0);
        self.model.dirty |= dirty::LAYOUT;
    }

    fn dpi_changed(&mut self, dpi: u32) {
        self.dpi = dpi;
        self.fonts = Fonts::new(dpi);
        unsafe {
            SendMessageW(self.toolbar, TB_SETIMAGELIST, WPARAM(0), LPARAM(toolbar_images(dpi).0));
            SendMessageW(self.status, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
            SendMessageW(self.cancel, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
            SendMessageW(self.original_button, WM_SETFONT, WPARAM(self.fonts.glyph_small.0 as usize), LPARAM(1));
        }
        canvas::set_dpi(dpi);
        inspector::dpi_changed(self);
        empty::dpi_changed(self);
        self.model.dirty |= dirty::LAYOUT | dirty::STRUCTURE | dirty::CANVAS | dirty::STRIP;
    }

    fn status_menu(&mut self) {
        unsafe {
            let menu = CreatePopupMenu().unwrap();
            let w = wide("&Copy");
            let _ = AppendMenuW(menu, MF_STRING, id::COPY_STATUS as usize, pcwstr(&w));
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, p.x, p.y, 0, self.main, None);
            let _ = DestroyMenu(menu);
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
}

pub fn dwm_flush() {
    unsafe {
        let _ = DwmFlush();
    }
}

pub fn current_process_id() -> u32 {
    unsafe { GetCurrentProcessId() }
}

pub fn session_store() -> SessionStore {
    SessionStore::default()
}

pub fn _unused(_: EditorSession) {}
