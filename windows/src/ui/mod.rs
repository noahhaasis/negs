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
mod chrome;
mod dialogs;
mod draw;
mod drop;
mod empty;
mod folder_watch;
pub mod inspector;
mod motion;
mod panes;
mod presets;
mod strip;
mod theme;
mod titlebar;
mod tone_graph;

use std::cell::{Cell, RefCell};
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
/// Opens a menu bar menu (index, from the keyboard), outside the App borrow.
pub const WM_APP_MENU: u32 = WM_APP + 3;
const TIMER_RUN: usize = 1;
const TIMER_EXPORT: usize = 2;
const COPYDATA_OPEN: usize = 0x4e45_4753; // "NEGS"

pub mod id {
    pub const OPEN: u16 = 100;
    pub const SAVE_PREVIEW: u16 = 101;
    pub const EXPORT: u16 = 102;
    pub const SETTINGS: u16 = 103;
    pub const EXIT: u16 = 104;
    pub const EXPORT_LUT: u16 = 105;
    pub const NEW_PRESET: u16 = 106;
    pub const IMPORT_PRESETS: u16 = 107;
    pub const PRESETS_FOLDER: u16 = 108;
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
    pub const PRESETS: u16 = 307;
    pub const PANEL: u16 = 308;
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

/// The narrowest canvas column and adjustments column, in DIPs (the Mac's
/// split-view minimums). The window keeps room for both, or for the canvas
/// alone while the adjustments are hidden.
const CANVAS_MINIMUM: f64 = 680.0;
const INSPECTOR_MINIMUM: f64 = 300.0;
const INSPECTOR_MAXIMUM: f64 = 440.0;
const SPLITTER: f64 = 5.0;
const MINIMUM_HEIGHT: f64 = 520.0;

/// A first window's frame height in DIPs, before the user sizes it.
const DEFAULT_HEIGHT: f64 = 940.0;

/// The window's narrowest client width in DIPs.
fn minimum_width(inspector_visible: bool) -> f64 {
    if inspector_visible { CANVAS_MINIMUM + SPLITTER + INSPECTOR_MINIMUM } else { CANVAS_MINIMUM }
}

/// The adjustments column's width in DIPs: the chosen width, narrowed to
/// leave the canvas its minimum, but never below its own.
fn inspector_width(chosen: f64, client: f64) -> f64 {
    chosen.clamp(INSPECTOR_MINIMUM, INSPECTOR_MAXIMUM).min(client - CANVAS_MINIMUM - SPLITTER).max(INSPECTOR_MINIMUM)
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// Whether the last layout showed the adjustments, for WM_GETMINMAXINFO,
    /// which can arrive while the app is borrowed.
    static INSPECTOR_SHOWN: Cell<bool> = const { Cell::new(true) };
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

/// Tests: `drain` without a window. The lifecycle harness
/// (`model::harness`) runs posted and timed tasks with a headless `App`,
/// one at a time, as the message loop would. The queue is process-wide, so
/// the harness runs each of its tests in a process of its own.
#[cfg(test)]
pub mod testing {
    use super::*;

    /// Runs the oldest posted task with `app`. False when none was waiting.
    pub fn run_posted(app: &mut App) -> bool {
        let task = QUEUE.lock().unwrap().pop_front();
        match task {
            Some(task) => {
                task(app);
                true
            }
            None => false,
        }
    }

    pub fn posted() -> usize {
        QUEUE.lock().unwrap().len()
    }

    /// Runs the earliest `ui::after` task due by `horizon` (on this thread).
    /// False when none is.
    pub fn run_timer(app: &mut App, horizon: Instant) -> bool {
        let task = TIMED_TASKS.with(|t| {
            let mut t = t.borrow_mut();
            let earliest = t.iter().enumerate().filter(|(_, (at, _))| *at <= horizon).min_by_key(|(_, (at, _))| *at).map(|(i, _)| i);
            earliest.map(|i| t.remove(i).1)
        });
        match task {
            Some(task) => {
                task(app);
                true
            }
            None => false,
        }
    }

    /// When the earliest `ui::after` task on this thread is due.
    pub fn next_timer() -> Option<Instant> {
        TIMED_TASKS.with(|t| t.borrow().iter().map(|(at, _)| *at).min())
    }

    static BACKGROUND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Work on another thread that will post a task: the harness isn't idle
    /// while one is held. Model code holds one where no model flag (such as
    /// `rendering`) already says so.
    pub struct Background(());

    pub fn background() -> Background {
        BACKGROUND.fetch_add(1, Ordering::SeqCst);
        Background(())
    }

    impl Drop for Background {
        fn drop(&mut self) {
            BACKGROUND.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn background_running() -> usize {
        BACKGROUND.load(Ordering::SeqCst)
    }
    /// Drops whatever earlier code on this thread or process left queued.
    pub fn clear() {
        QUEUE.lock().unwrap().clear();
        TIMED_TASKS.with(|t| t.borrow_mut().clear());
    }

    impl App {
        /// An app with no windows around `model`, for the tasks the model posts.
        pub fn headless(model: Model) -> App {
            let none = HWND::default();
            let font = HFONT::default();
            App {
                model,
                main: none,
                tools: none,
                actions: none,
                menubar: none,
                status: none,
                progress: none,
                cancel: none,
                canvas: none,
                empty: none,
                strip: none,
                panes: none,
                inspector: inspector::Inspector::headless(),
                presets_view: None,
                fonts: Fonts { ui: font, bold: font, section: font, small: font, title: font, glyph: font, glyph_small: font, line: 16 },
                dpi: 96,
                menu: HMENU::default(),
                accelerators: HACCEL::default(),
                report: Report { file: None, seen: vec![] },
                splitter_drag: None,
                full_screen: None,
                strip_view: strip::Strip::default(),
                empty_view: empty::Empty::default(),
                taskbar: None,
                original_key: None,
                drop_target_active: false,
                started: Instant::now(),
                quitting: false,
                preview_monitor: 0,
            }
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
    /// Group headers: the UI size, semibold.
    pub bold: HFONT,
    /// Section titles: semibold and a little larger.
    pub section: HFONT,
    pub small: HFONT,
    pub title: HFONT,
    pub glyph: HFONT,
    pub glyph_small: HFONT,
    /// The UI font's line height in pixels.
    pub line: i32,
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
            // The system's size (so text scaling applies) in Segoe UI
            // Variable where Windows 11 has it, unless the face was changed.
            let mut base = metrics.lfMessageFont;
            let mut display = base;
            if platform::from_wide(&base.lfFaceName) == "Segoe UI" && font_exists("Segoe UI Variable Text") {
                set_face(&mut base, "Segoe UI Variable Text");
                set_face(&mut display, "Segoe UI Variable Display");
            }
            let ui = CreateFontIndirectW(&base);
            let bold = CreateFontIndirectW(&LOGFONTW { lfWeight: 600, ..base });
            let section = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 115 / 100, lfWeight: 600, ..display });
            let small = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 85 / 100, ..base });
            let title = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 19 / 10, lfWeight: 600, ..display });
            let glyph_face = icon_face();
            let mut glyph = LOGFONTW { lfHeight: -(16 * dpi as i32 / 96), lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
            let face = wide(glyph_face);
            glyph.lfFaceName[..face.len()].copy_from_slice(&face);
            let glyph_font = CreateFontIndirectW(&glyph);
            let glyph_small = CreateFontIndirectW(&LOGFONTW { lfHeight: -(11 * dpi as i32 / 96), ..glyph });
            let line = line_height(ui);
            Fonts { ui, bold, section, small, title, glyph: glyph_font, glyph_small, line }
        }
    }
}

fn line_height(font: HFONT) -> i32 {
    unsafe {
        let dc = GetDC(None);
        let old = SelectObject(dc, font);
        let mut metrics = TEXTMETRICW::default();
        let _ = GetTextMetricsW(dc, &mut metrics);
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        metrics.tmHeight
    }
}

fn set_face(font: &mut LOGFONTW, face: &str) {
    let w = wide(face);
    font.lfFaceName = [0; 32];
    font.lfFaceName[..w.len()].copy_from_slice(&w);
}

/// Segoe Fluent Icons on Windows 11, MDL2 Assets on Windows 10.
pub fn icon_face() -> &'static str {
    if font_exists("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" }
}

/// Whether a font family is installed (asked once per face).
fn font_exists(face: &str) -> bool {
    thread_local! {
        static KNOWN: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
    }
    if let Some(known) = KNOWN.with(|k| k.borrow().iter().find(|(f, _)| f == face).map(|(_, e)| *e)) {
        return known;
    }
    let exists = enumerate_font(face);
    KNOWN.with(|k| k.borrow_mut().push((face.to_string(), exists)));
    exists
}

fn enumerate_font(face: &str) -> bool {
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
    /// Open, after the menus; Export and the panel's button, before the
    /// caption buttons.
    tools: HWND,
    actions: HWND,
    /// The menu bar in the title bar.
    menubar: HWND,
    status: HWND,
    progress: HWND,
    cancel: HWND,
    pub canvas: HWND,
    empty: HWND,
    strip: HWND,
    /// Adjustments | Presets, at the top of the column.
    panes: HWND,
    pub inspector: inspector::Inspector,
    /// Created the first time the Presets panel shows.
    presets_view: Option<presets::Panel>,
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
    /// The monitor the thumbnails were last made for.
    preview_monitor: isize,
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
        if flags & dirty::ERROR != 0 && crate::trace::bench_running() {
            let error = self.model.error.take();
            let notice = self.model.notice.take().map(|(message, _)| message);
            crate::trace::event("dialog_skipped", || serde_json::json!({"error": error, "notice": notice}));
        } else if flags & dirty::ERROR != 0 {
            if let Some(error) = self.model.error.take() {
                // After this handler returns, outside any borrow.
                let main = self.main.0 as isize;
                post(move |_| dialogs::error(HWND(main as *mut _), &error));
            }
            if let Some((message, explanation)) = self.model.notice.take() {
                let main = self.main.0 as isize;
                post(move |_| dialogs::notice(HWND(main as *mut _), &message, &explanation));
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
        if flags & (dirty::STRUCTURE | dirty::INSPECTOR | dirty::CROP | dirty::LAYOUT | dirty::TONE_GRAPH) != 0 || flags & dirty::HISTOGRAM != 0 {
            let structure = flags & (dirty::STRUCTURE | dirty::CROP | dirty::LAYOUT) != 0;
            inspector::sync(self, structure, flags & dirty::HISTOGRAM != 0);
        }
        if flags & (dirty::PRESETS | dirty::CHROME | dirty::LAYOUT) != 0 {
            presets::sync(self);
        }
        // The canvas's tools follow the chrome (the film strip's button).
        if flags & (dirty::CANVAS | dirty::ZOOM | dirty::CROP | dirty::CHROME) != 0 {
            canvas::update_scene(self);
            if self.model.image.is_some() && !self.model.first_render_shown {
                self.model.first_render_shown = true;
                self.report.stamp("first_image_scene");
            }
        }
        if flags & (dirty::STRIP | dirty::CHROME) != 0 {
            strip::sync(self);
        }
        // What the preview tiles show follows the edit, the panels and the main render.
        let previews = dirty::PREVIEWS | dirty::INSPECTOR | dirty::STRUCTURE | dirty::CHROME | dirty::LAYOUT | dirty::PRESETS | dirty::CANVAS;
        if flags & previews != 0 && self.model.prefs.previews {
            // Only once tiles are on screen: reading the profile costs launch time.
            if self.model.previews_on_screen() {
                self.follow_preview_monitor();
            }
            if self.model.pump_previews() || flags & dirty::PREVIEWS != 0 {
                inspector::redraw_previews(self);
                presets::redraw_previews(self);
            }
        }
        if self.model.dirty != 0 {
            self.refresh();
        }
    }

    /// Thumbnails are drawn through the profile of the monitor the window is on.
    fn follow_preview_monitor(&mut self) {
        let monitor = unsafe { MonitorFromWindow(self.main, MONITOR_DEFAULTTONEAREST).0 as isize };
        if monitor != self.preview_monitor {
            self.preview_monitor = monitor;
            self.model.set_preview_color(crate::icc::for_window(self.main).map(std::sync::Arc::new));
        }
    }

    fn update_chrome(&mut self) {
        let model = &self.model;
        let title = match &model.source {
            Some(path) => format!("{} - Negs", crate::model::file_name(path)),
            None => "Negs".into(),
        };
        // Only a new title: setting it hides the window for a moment (see
        // WM_SETTEXT), and the photograph's Vulkan surface then dropped out
        // for a frame, after every render, showing what GDI last drew there.
        if draw::window_text(self.main) != title {
            let title = wide(title);
            unsafe {
                let _ = SetWindowTextW(self.main, pcwstr(&title));
            }
        }
        unsafe {
            SendMessageW(self.actions, TB_ENABLEBUTTON, WPARAM(id::EXPORT as usize), LPARAM(model.can_export() as isize));
            SendMessageW(self.actions, TB_CHECKBUTTON, WPARAM(id::PANEL as usize), LPARAM(model.inspector_visible as isize));
        }
        panes::sync(self.panes, model.pane() == crate::model::Pane::Presets);
        let exporting = model.exporting;
        unsafe {
            let _ = ShowWindow(self.progress, if exporting { SW_SHOWNA } else { SW_HIDE });
            let _ = ShowWindow(self.cancel, if exporting { SW_SHOWNA } else { SW_HIDE });
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
            INSPECTOR_SHOWN.with(|s| s.set(self.model.inspector_visible));
            // Make room before the adjustments appear, so neither column is
            // squeezed below its minimum.
            self.fit_minimum();
            let mut client = RECT::default();
            let _ = GetClientRect(self.main, &mut client);
            titlebar::layout(self.main, self.menubar, self.tools, self.actions);
            SendMessageW(self.status, WM_SIZE, WPARAM(0), LPARAM(0));
            let height = |hwnd: HWND| {
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                if IsWindowVisible(hwnd).as_bool() { r.bottom - r.top } else { 0 }
            };
            let top = titlebar::height(self.main);
            let bottom = client.bottom - height(self.status);
            let inspector_width = if self.model.inspector_visible {
                self.px(inspector_width(self.model.prefs.inspector_width, client.right as f64 / self.scale()))
            } else {
                0
            };
            let splitter = if self.model.inspector_visible { self.px(SPLITTER) } else { 0 };
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
            let shows_presets = presets::shown(&self.model);
            // The switch between the panels; cropping has the column to itself.
            let shows_panes = self.model.inspector_visible && !self.model.cropping();
            let panes_height = if shows_panes { self.px(panes::HEIGHT) } else { 0 };
            let _ = MoveWindow(self.panes, column + splitter, top, inspector_width, panes_height, true);
            let _ = ShowWindow(self.panes, if shows_panes { SW_SHOWNA } else { SW_HIDE });
            let panel_top = top + panes_height;
            self.inspector.layout(column + splitter, panel_top, inspector_width, bottom - panel_top, self.model.inspector_visible && !shows_presets);
            let rect = RECT { left: column + splitter, top: panel_top, right: column + splitter + inspector_width, bottom };
            presets::layout(self, rect, shows_presets);
            canvas::resized(self.canvas);
            // Their own WM_SIZE arrives while the app is busy here.
            empty::layout(self);
            strip::resized(self);
            self.layout_status();
        }
    }

    /// Grows a window narrower or shorter than the minimum (a saved frame
    /// from before it, or one the adjustments are appearing in), keeping its
    /// top-left corner and its monitor's work area. Maximized and full screen
    /// windows already fill the screen.
    fn fit_minimum(&self) {
        unsafe {
            if self.full_screen.is_some() || IsZoomed(self.main).as_bool() || IsIconic(self.main).as_bool() {
                return;
            }
            let (mut client, mut frame) = (RECT::default(), RECT::default());
            let _ = GetClientRect(self.main, &mut client);
            let _ = GetWindowRect(self.main, &mut frame);
            let wanted = (self.px(minimum_width(self.model.inspector_visible)), self.px(MINIMUM_HEIGHT));
            let grow = ((wanted.0 - client.right).max(0), (wanted.1 - client.bottom).max(0));
            if grow == (0, 0) {
                return;
            }
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(MonitorFromWindow(self.main, MONITOR_DEFAULTTONEAREST), &mut info);
            let work = info.rcWork;
            let width = (frame.right - frame.left + grow.0).min(work.right - work.left);
            let height = (frame.bottom - frame.top + grow.1).min(work.bottom - work.top);
            if (width, height) == (frame.right - frame.left, frame.bottom - frame.top) {
                return;
            }
            // Slide left or up rather than off the work area's edge.
            let x = frame.left.min(work.right - width).max(work.left);
            let y = frame.top.min(work.bottom - height).max(work.top);
            let _ = SetWindowPos(self.main, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    fn layout_status(&mut self) {
        unsafe {
            let mut client = RECT::default();
            let _ = GetClientRect(self.status, &mut client);
            let export = if self.model.exporting { self.px(250.0) } else { 0 };
            let stocks = self.px(280.0);
            let right = client.right - self.px(18.0);
            let parts = [(right - export - stocks).max(self.px(80.0)), right - export, -1];
            SendMessageW(self.status, SB_SETPARTS, WPARAM(parts.len()), LPARAM(parts.as_ptr() as isize));
            let mut part = RECT::default();
            SendMessageW(self.status, SB_GETRECT, WPARAM(2), LPARAM(&mut part as *mut _ as isize));
            let inset = self.px(2.0);
            let bar = self.px(90.0);
            let cancel = self.px(64.0);
            let _ = MoveWindow(self.progress, part.right - cancel - bar - inset * 2, part.top + inset * 2, bar, part.bottom - part.top - inset * 4, true);
            let _ = MoveWindow(self.cancel, part.right - cancel - inset, part.top + inset / 2, cancel, part.bottom - part.top - inset, true);
        }
    }

    pub fn command(&mut self, command: u16) {
        let model = &mut self.model;
        match command {
            id::OPEN | id::EMPTY_OPEN => dialogs::open(self),
            id::SAVE_PREVIEW => dialogs::save(self, false),
            id::EXPORT => dialogs::save(self, true),
            id::EXPORT_LUT => dialogs::export_lut(self),
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
            id::ADJUSTMENTS => model.toggle_pane(crate::model::Pane::Adjustments),
            id::PRESETS => model.toggle_pane(crate::model::Pane::Presets),
            id::PANEL => model.toggle_inspector(),
            id::ORIGINAL => {
                let shown = model.holding_original;
                model.hold_original(!shown);
            }
            id::NEW_PRESET => model.new_preset(),
            id::IMPORT_PRESETS => {
                if model.can_change_presets() {
                    model.show_pane(crate::model::Pane::Presets);
                    self.refresh();
                    dialogs::import_presets(self);
                }
            }
            id::PRESETS_FOLDER => show_folder(self.main, &self.model.preset_library.folder),
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
            } else {
                let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let _ = GetWindowPlacement(self.main, &mut placement);
                let style = GetWindowLongW(self.main, GWL_STYLE);
                let monitor = MonitorFromWindow(self.main, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(monitor, &mut info);
                SetWindowLongW(self.main, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as i32));
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
        enable(id::EXPORT_LUT, model.can_export_lut());
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
        check(id::ADJUSTMENTS, model.inspector_visible && !presets::shown(model));
        check(id::PRESETS, presets::shown(model));
        enable(id::NEW_PRESET, model.can_change_presets());
        enable(id::IMPORT_PRESETS, model.can_change_presets());
        check(id::FULL_SCREEN, self.full_screen.is_some());
        check(id::ORIGINAL, model.holding_original);
        enable(id::ORIGINAL, model.can_show_original() && !model.cropping());
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

/// Opens a folder in Explorer, creating it first.
fn show_folder(owner: HWND, folder: &Path) {
    if let Err(error) = std::fs::create_dir_all(folder) {
        dialogs::error(owner, &format!("Couldn’t create the Presets folder: {error}."));
        return;
    }
    let (verb, path) = (wide("open"), wide(folder));
    unsafe {
        ShellExecuteW(owner, pcwstr(&verb), pcwstr(&path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
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
    // Tests open scratch files: keep them out of the user's Recent items.
    if cfg!(test) {
        return;
    }
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
        theme::apply_created(hwnd, class);
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
        add(file, id::EXPORT_LUT, "Export &LUT…\tCtrl+Shift+L");
        separator(file);
        add(file, id::NEW_PRESET, "&New Preset\tCtrl+Shift+S");
        add(file, id::IMPORT_PRESETS, "&Import Presets…\tCtrl+Shift+O");
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
        add(view, id::PRESETS, "&Presets\tF8");
        separator(view);
        add(view, id::FIT, "Fi&t Photograph\tCtrl+0");
        add(view, id::ACTUAL, "Actual &Size\tCtrl+1");
        add(view, id::ZOOM_IN, "Zoom &In\tCtrl+=");
        add(view, id::ZOOM_OUT, "Zoom &Out\tCtrl+-");
        separator(view);
        add(view, id::ORIGINAL, "Show &Original\tHold O");
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
        (shift_ctrl, 'L' as u16, id::EXPORT_LUT),
        (ctrl, 'Z' as u16, id::UNDO),
        (ctrl, 'Y' as u16, id::REDO),
        (shift_ctrl, 'Z' as u16, id::REDO),
        (shift_ctrl, 'R' as u16, id::RESET_ALL),
        (FVIRTKEY, VK_F6.0, id::FILMSTRIP),
        (FVIRTKEY, VK_F7.0, id::ADJUSTMENTS),
        (FVIRTKEY, VK_F8.0, id::PRESETS),
        (shift_ctrl, 'S' as u16, id::NEW_PRESET),
        (shift_ctrl, 'O' as u16, id::IMPORT_PRESETS),
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

/// Segoe icon glyphs drawn at the window's DPI: they size the toolbar's
/// buttons (`chrome` paints them from the palette).
fn toolbar_images(dpi: u32) -> HIMAGELIST {
    let glyphs = chrome::TOOL_GLYPHS;
    let size = (20 * dpi / 96) as i32;
    unsafe {
        let list = ImageList_Create(size, size, ILC_COLOR32, glyphs.len() as i32, 0);
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        let face = icon_face();
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

/// The title bar's tooltips, by command.
fn tool_tip(command: u16) -> &'static str {
    match command {
        id::OPEN => "Open photograph (Ctrl+O)",
        id::EXPORT => "Export photograph (Ctrl+Shift+E)",
        _ => "Show or hide the panel (F7 adjustments, F8 presets)",
    }
}

/// A toolbar in the title bar, of (glyph, command, check button). Buttons
/// with a label (`chrome::TOOL_LABELS`) show it; the tools for the
/// photograph are on the canvas (`canvas::Tool`).
fn build_toolbar(main: HWND, dpi: u32, font: HFONT, buttons: &[(i32, u16, bool)]) -> HWND {
    unsafe {
        let toolbar = create(
            TOOLBARCLASSNAMEW,
            "",
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(TBSTYLE_FLAT | TBSTYLE_LIST | CCS_NODIVIDER as u32 | CCS_NOPARENTALIGN as u32 | CCS_NORESIZE as u32),
            WINDOW_EX_STYLE(0),
            main,
            0,
            font,
        );
        SendMessageW(toolbar, TB_BUTTONSTRUCTSIZE, WPARAM(std::mem::size_of::<TBBUTTON>()), LPARAM(0));
        SendMessageW(toolbar, TB_SETEXTENDEDSTYLE, WPARAM(0), LPARAM(0x80 /* TBSTYLE_EX_DOUBLEBUFFER */));
        SendMessageW(toolbar, TB_SETIMAGELIST, WPARAM(0), LPARAM(toolbar_images(dpi).0));
        // A label shows and names the button (the toolbar keeps a pointer
        // to it, so it lives as long as the toolbar).
        let label = |image: i32| chrome::TOOL_LABELS.get(image as usize).copied().flatten();
        let texts: &'static [Option<Vec<u16>>] =
            Box::leak(buttons.iter().map(|&(image, _, _)| label(image).map(wide)).collect::<Vec<_>>().into_boxed_slice());
        let buttons: Vec<TBBUTTON> = buttons
            .iter()
            .zip(texts)
            .map(|(&(image, cmd, check), text)| TBBUTTON {
                iBitmap: image,
                idCommand: cmd as i32,
                fsState: TBSTATE_ENABLED as u8,
                fsStyle: ((if check { BTNS_CHECK } else { BTNS_BUTTON }) | if text.is_some() { BTNS_AUTOSIZE } else { 0 }) as u8,
                iString: text.as_ref().map_or(-1, |t| t.as_ptr() as isize),
                ..Default::default()
            })
            .collect();
        SendMessageW(toolbar, TB_ADDBUTTONSW, WPARAM(buttons.len()), LPARAM(buttons.as_ptr() as isize));
        titlebar::add_tips(toolbar, buttons.iter().map(|b| (b.idCommand as u16, tool_tip(b.idCommand as u16))).collect());
        titlebar::tool_metrics(main, toolbar);
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
            ..Default::default()
        };
        RegisterClassExW(&class);
        canvas::register();
        empty::register();
        strip::register();
        inspector::register();
        tone_graph::register();
        presets::register();

        theme::init();
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
            None,
            instance(),
            None,
        )
        .unwrap();
        MAIN.store(main.0 as isize, Ordering::Relaxed);
        theme::apply_window(main);
        titlebar::round_corners(main);
        let dpi = GetDpiForWindow(main).max(96);
        restore_placement(main, &model.prefs, dpi);
        report.stamp("main_window");
        let fonts = Fonts::new(dpi);
        report.stamp("fonts");
        let menubar = titlebar::create_menubar(main, menu, fonts.ui);
        titlebar::set_fonts(fonts.ui, fonts.glyph_small);
        let tools = build_toolbar(main, dpi, fonts.ui, &[(0, id::OPEN, false)]);
        let actions = build_toolbar(main, dpi, fonts.ui, &[(1, id::EXPORT, false), (2, id::PANEL, true)]);
        for toolbar in [tools, actions] {
            chrome::subclass_toolbar(toolbar, fonts.glyph);
        }
        report.stamp("toolbar");
        let status = create(STATUSCLASSNAMEW, "", WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WINDOW_STYLE(SBARS_SIZEGRIP), WINDOW_EX_STYLE(0), main, 0, fonts.ui);
        empty::subclass_status(status);
        chrome::subclass_status(status);
        let progress = create(PROGRESS_CLASSW, "", WS_CHILD | WINDOW_STYLE(PBS_MARQUEE), WINDOW_EX_STYLE(0), status, 0, HFONT::default());
        SendMessageW(progress, PBM_SETMARQUEE, WPARAM(1), LPARAM(30));
        let cancel = create(w!("BUTTON"), "Cancel", WS_CHILD | WS_TABSTOP, WINDOW_EX_STYLE(0), status, id::CANCEL_EXPORT, fonts.ui);
        let canvas = create(w!("NegsCanvas"), "Photograph preview", WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let empty = create(w!("NegsEmpty"), "", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
        let strip = create(w!("NegsStrip"), "Film Strip", WS_CHILD | WS_TABSTOP | WS_HSCROLL, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let panes = panes::create(main, fonts.ui);
        let inspector = inspector::Inspector::create(main, &fonts, dpi);
        canvas::init(canvas, dpi);

        let mut app = App {
            model,
            main,
            tools,
            actions,
            menubar,
            status,
            progress,
            cancel,
            canvas,
            empty,
            strip,
            panes,
            inspector,
            presets_view: None,
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
            preview_monitor: 0,
        };
        let _ = recent;
        empty::build(&mut app);
        canvas::set_custom_background(app.canvas, app.model.prefs.custom_viewer_background());
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
        crate::trace::start_bench();
        if let Some(path) = path {
            with_app(|app| app.model.open(&path));
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Tab, arrows in a group or an access key moving the focus
            // scroll the inspector to the newly focused control.
            let keyed = matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_CHAR | WM_SYSCHAR);
            let before = keyed.then(|| GetFocus());
            if !pre_translate(&msg) {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if before.is_some_and(|before| GetFocus() != before) {
                with_app(inspector::reveal_focus);
            }
        }
        APP.with(|cell| cell.borrow_mut().take());
    }
}

fn restore_placement(main: HWND, prefs: &Preferences, dpi: u32) {
    let Some([_, x, y, w, h]) = prefs.window else { return default_height(main, dpi) };
    let s = dpi as f64 / 96.0;
    let rect = RECT { left: x, top: y, right: x + (w as f64 * s) as i32, bottom: y + (h as f64 * s) as i32 };
    unsafe {
        // A monitor that is gone leaves the default placement.
        if MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_invalid() {
            return default_height(main, dpi);
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

/// Without a saved frame the window gets `DEFAULT_HEIGHT`, so an open Film
/// grid leaves the tabs below it in view, kept inside the monitor's work area.
fn default_height(main: HWND, dpi: u32) {
    unsafe {
        let mut frame = RECT::default();
        let _ = GetWindowRect(main, &mut frame);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST), &mut info);
        let work = info.rcWork;
        let height = ((DEFAULT_HEIGHT * dpi as f64 / 96.0).round() as i32).min(work.bottom - work.top);
        let y = frame.top.min(work.bottom - height).max(work.top);
        let _ = SetWindowPos(main, None, frame.left, y, frame.right - frame.left, height, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Keys before dialog navigation: Show Original, crop's Return and Escape,
/// and the accelerators, which never fire while a text field has focus.
fn pre_translate(msg: &MSG) -> bool {
    let main = main_hwnd();
    draw::hover_message(msg);
    if titlebar::keys(main, msg) {
        return true;
    }
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
                Some("ComboBox") | Some("SysListView32") => !modifier_down() && (b'A' as usize..=b'Z' as usize).contains(&msg.wParam.0),
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
            WM_APP_MENU => {
                titlebar::open(hwnd, wparam.0, lparam.0 != 0);
                LRESULT(0)
            }
            WM_NCCALCSIZE => titlebar::nccalcsize(hwnd, wparam, lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_NCHITTEST => titlebar::nchittest(hwnd, wparam, lparam),
            WM_NCMOUSEMOVE | WM_NCMOUSELEAVE | WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCLBUTTONUP => {
                titlebar::ncmouse(hwnd, msg, wparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }
            WM_NCACTIVATE => {
                // No caption to repaint; lParam -1 keeps the system from drawing one.
                titlebar::invalidate(hwnd);
                DefWindowProcW(hwnd, msg, wparam, LPARAM(-1))
            }
            WM_SETTEXT | WM_SETICON => {
                // The system would draw its caption over the band. Hiding
                // the window also takes the canvas's Vulkan surface off the
                // screen for a frame: send these only when something changed.
                let style = GetWindowLongW(hwnd, GWL_STYLE);
                SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_VISIBLE.0 as i32));
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                SetWindowLongW(hwnd, GWL_STYLE, style);
                titlebar::invalidate(hwnd);
                result
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                titlebar::paint(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_SYSCOMMAND if (wparam.0 & 0xfff0) as u32 == SC_KEYMENU && titlebar::key_menu(hwnd, lparam.0 as u32) => LRESULT(0),
            WM_MENUSELECT => {
                titlebar::menu_select(wparam, lparam);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_APP_BUILD => {
                with_app(|app| {
                    app.report.stamp("first_frame_done");
                    inspector::build(app);
                    app.report.stamp("inspector_built");
                    // A remembered Presets panel is built now, like the adjustments.
                    if presets::shown(&app.model) {
                        app.model.dirty |= dirty::LAYOUT;
                    }
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
            WM_ERASEBKGND => draw::erase(hwnd, wparam, theme::palette().panel),
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED => {
                // The app theme ("ImmersiveColorSet"), high contrast or the accent.
                post(|app: &mut App| app.retheme());
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let s = GetDpiForWindow(hwnd).max(96) as f64 / 96.0;
                // The minimum is for the client area; the frame adds to it.
                let client = ((minimum_width(INSPECTOR_SHOWN.with(Cell::get)) * s).round() as i32, (MINIMUM_HEIGHT * s).round() as i32);
                let mut frame = RECT { left: 0, top: 0, right: client.0, bottom: client.1 };
                let mut window = RECT::default();
                let mut inside = RECT::default();
                if GetWindowRect(hwnd, &mut window).is_ok() && GetClientRect(hwnd, &mut inside).is_ok() && inside.right > 0 {
                    frame.right += (window.right - window.left) - inside.right;
                    frame.bottom += (window.bottom - window.top) - inside.bottom;
                }
                // Never more than the monitor's work area, on a small screen.
                let mut monitor = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut monitor);
                let work = monitor.rcWork;
                info.ptMinTrackSize = POINT { x: frame.right.min(work.right - work.left), y: frame.bottom.min(work.bottom - work.top) };
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
                if let Some(result) = titlebar::dropdown(hwnd, lparam) {
                    return result;
                }
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
                    titlebar::deactivated(hwnd);
                }
                titlebar::invalidate(hwnd);
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
                titlebar::ncmouse(hwnd, msg, wparam);
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
        // No wider than the canvas's minimum leaves, so the splitter follows the pointer.
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.main, &mut client);
        }
        self.model.prefs.inspector_width = inspector_width(target, client.right as f64 / self.scale());
        self.model.dirty |= dirty::LAYOUT;
    }

    fn dpi_changed(&mut self, dpi: u32) {
        self.dpi = dpi;
        self.fonts = Fonts::new(dpi);
        unsafe {
            for toolbar in [self.tools, self.actions] {
                SendMessageW(toolbar, TB_SETIMAGELIST, WPARAM(0), LPARAM(toolbar_images(dpi).0));
                SendMessageW(toolbar, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
                chrome::subclass_toolbar(toolbar, self.fonts.glyph);
                titlebar::tool_metrics(self.main, toolbar);
            }
            panes::set_font(self.panes, self.fonts.ui);
            SendMessageW(self.menubar, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
            titlebar::set_metrics(self.main, self.menubar);
            titlebar::set_fonts(self.fonts.ui, self.fonts.glyph_small);
            SendMessageW(self.status, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
            SendMessageW(self.cancel, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
        }
        canvas::set_dpi(dpi);
        inspector::dpi_changed(self);
        presets::dpi_changed(self);
        empty::dpi_changed(self);
        self.model.dirty |= dirty::LAYOUT | dirty::STRUCTURE | dirty::CANVAS | dirty::STRIP;
    }

    /// Follows a change of the system's theme, contrast or accent.
    fn retheme(&mut self) {
        if !theme::refresh() {
            return;
        }
        theme::apply_all();
        presets::retheme(self);
        unsafe {
            let _ = InvalidateRect(self.canvas, None, false);
        }
        self.model.dirty |= dirty::LAYOUT | dirty::STRUCTURE | dirty::STRIP | dirty::CANVAS;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The window keeps room for both columns, or the canvas alone.
    #[test]
    fn minimum_width_follows_the_adjustments() {
        assert_eq!(minimum_width(true), 985.0);
        assert_eq!(minimum_width(false), 680.0);
    }

    /// A narrow window narrows the adjustments before the canvas, down to
    /// their own minimum.
    #[test]
    fn adjustments_give_way_to_the_canvas() {
        assert_eq!(inspector_width(340.0, 1400.0), 340.0);
        assert_eq!(inspector_width(440.0, 1000.0), 315.0);
        assert_eq!(inspector_width(440.0, 900.0), 300.0);
        assert_eq!(inspector_width(500.0, 2000.0), 440.0);
    }
}
