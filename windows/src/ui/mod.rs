//! The editing window: the title bar with its menus, the canvas column
//! (photograph or empty state, film strip, status line) and the adjustments
//! column. No toolbar: its items are in the menus and the status line.
//! Win32 with the common controls (docs/cross-platform-port.md, Windows UI).
//!
//! All state lives in one `App` on the UI thread. Window procedures reach it
//! through `with_app`, which refuses re-entrant calls: notifications caused by
//! the app's own changes to controls are ignored, and work posted from other
//! threads waits until the app is free. The canvas keeps its view state
//! separately (`canvas::VIEW`) so it can paint at any time.

mod alert;
pub mod batch;
pub mod canvas;
mod color_filter;
pub mod compare;
mod d2d;
mod dialog_look;
mod dialogs;
mod draw;
mod drop;
mod empty;
mod grid;
mod grid_bench;
mod ink;
pub mod folder_watch;
pub mod inspector;
mod library;
mod motion;
mod panes;
mod presets;
mod sampler;
mod settings;
mod sheet;
mod status;
mod strip;
mod surface;
mod theme;
mod thumbs;
mod titlebar;
mod tone_graph;
mod tray;
mod tray_access;
mod view_modes;
mod view_switch;

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
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::model::{Model, dirty};
use crate::platform::{self, pcwstr, wide};
use crate::session::{Preferences, SessionStore};

/// Annotate icon buttons and numeric editors without changing their visual text.
pub fn accessible_name(hwnd: HWND, name: &str) {
    use windows::Win32::UI::Accessibility::PROPID_ACC_NAME;
    with_props(|service| {
        let name = wide(name);
        unsafe { let _ = service.SetHwndPropStr(hwnd, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_NAME, pcwstr(&name)); }
    });
}

/// Gives `hwnd` an accessible value: a slider reads its value, not the
/// trackbar's step.
pub fn accessible_value(hwnd: HWND, value: &str) {
    use windows::Win32::UI::Accessibility::PROPID_ACC_VALUE;
    with_props(|service| {
        let value = wide(value);
        unsafe { let _ = service.SetHwndPropStr(hwnd, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_VALUE, pcwstr(&value)); }
    });
}

/// Gives `hwnd` an accessible role (`ROLE_SYSTEM_*`): owner-drawn statics
/// otherwise read as graphics.
pub fn accessible_role(hwnd: HWND, role: u32) {
    use windows::Win32::UI::Accessibility::PROPID_ACC_ROLE;
    with_props(|service| unsafe {
        let _ = service.SetHwndProp(hwnd, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_ROLE, &windows_core::VARIANT::from(role as i32));
    });
}

fn with_props(f: impl FnOnce(&windows::Win32::UI::Accessibility::IAccPropServices)) {
    use windows::Win32::UI::Accessibility::{CAccPropServices, IAccPropServices};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    thread_local! {
        static PROPS: std::cell::OnceCell<Option<IAccPropServices>> = const { std::cell::OnceCell::new() };
    }
    PROPS.with(|cell| {
        if let Some(service) = cell.get_or_init(|| unsafe { CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER).ok() }) {
            f(service);
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
    /// Open in Lightroom, Open in Lightroom Classic (`lightroom::Kind::ALL`).
    pub const LIGHTROOM: u16 = 130; // ..131
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
    /// Every control in every module of the Lab (`toggleAllControls`).
    pub const ALL_CONTROLS: u16 = 309;
    pub const IMAGE_ONLY: u16 = 310;
    /// View › Theme, in `ThemeName::ALL`'s order.
    pub const THEME: u16 = 320; // ..325
    /// View › Viewer Sheet, in `ViewerSheet::ALL`'s order.
    pub const SHEET: u16 = 330; // ..337
    pub const ROTATE: u16 = 400;
    pub const CROP: u16 = 401;
    pub const CROP_DONE: u16 = 402;
    pub const CROP_CANCEL: u16 = 403;
    /// View › Flip Horizontal: mirrors as shown, while cropping (`io.flip`).
    pub const FLIP: u16 = 404;
    pub const COMPARE: u16 = 405;
    pub const GRID: u16 = 406;
    pub const COMPARE_OFFER: u16 = 407;
    /// The title bar's Grid · Photo · Compare (`view_switch`).
    pub const VIEW_GRID: u16 = 408;
    pub const VIEW_PHOTO: u16 = 409;
    pub const VIEW_COMPARE: u16 = 410;
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

type Task = Box<dyn FnOnce(&mut App) + Send>;
static QUEUE: Mutex<VecDeque<Task>> = Mutex::new(VecDeque::new());
static TIMED: Mutex<Vec<(Instant, usize)>> = Mutex::new(Vec::new());
static MAIN: AtomicIsize = AtomicIsize::new(0);

/// The narrowest canvas column and adjustments column, in DIPs (the Mac's
/// split-view minimums, `EditorWindowController`). The window keeps room
/// for both, or for the canvas alone while the adjustments are hidden.
const CANVAS_MINIMUM: f64 = 640.0;
const INSPECTOR_MINIMUM: f64 = 340.0;
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
                menubar: none,
                status: none,
                canvas: none,
                empty: none,
                strip: none,
                panes: none,
                inspector: inspector::Inspector::headless(),
                presets_view: None,
                settings: None,
                fonts: Fonts { ui: font, bold: font, small: font, line: 16 },
                dpi: 96,
                menu: HMENU::default(),
                accelerators: HACCEL::default(),
                report: Report { file: None, seen: vec![] },
                splitter_drag: None,
                full_screen: None,
                strip_view: strip::Strip::default(),
                thumbs: thumbs::Thumbs::default(),
                grid: None,
                empty_view: empty::Empty::default(),
                taskbar: None,
                original_key: None,
                drop_target_active: false,
                view: Default::default(),
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
    pub small: HFONT,
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
            if platform::from_wide(&base.lfFaceName) == "Segoe UI" && font_exists("Segoe UI Variable Text") {
                set_face(&mut base, "Segoe UI Variable Text");
            }
            let ui = CreateFontIndirectW(&base);
            let bold = CreateFontIndirectW(&LOGFONTW { lfWeight: 600, ..base });
            let small = CreateFontIndirectW(&LOGFONTW { lfHeight: base.lfHeight * 85 / 100, ..base });
            let line = line_height(ui);
            Fonts { ui, bold, small, line }
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
    /// The menu row in the title bar.
    menubar: HWND,
    /// The status line under the canvas column.
    status: HWND,
    pub canvas: HWND,
    empty: HWND,
    strip: HWND,
    /// Adjustments | Presets, at the top of the column.
    panes: HWND,
    pub inspector: inspector::Inspector,
    /// Created the first time the Presets panel shows.
    presets_view: Option<presets::Panel>,
    /// File › Settings…, built the first time it opens.
    settings: Option<settings::Settings>,
    pub fonts: Fonts,
    pub dpi: u32,
    menu: HMENU,
    accelerators: HACCEL,
    pub report: Report,
    splitter_drag: Option<(i32, f64)>,
    full_screen: Option<(WINDOWPLACEMENT, i32)>,
    pub strip_view: strip::Strip,
    pub thumbs: thumbs::Thumbs,
    /// The grid, made on first use.
    pub grid: Option<grid::Grid>,
    pub empty_view: empty::Empty,
    taskbar: Option<ITaskbarList3>,
    original_key: Option<u32>,
    pub drop_target_active: bool,
    /// Image Only and the photograph held upside down.
    pub view: view_modes::ViewModes,
    quitting: bool,
    /// The monitor the thumbnails were last made for.
    preview_monitor: isize,
}

impl App {
    /// The strip and grid's device, after the canvas's: launch never waits
    /// for it, and the two drivers' set-ups don't contend. Until it's made
    /// the strip is bare film, and no thumbnail is read.
    pub fn prepare_surfaces(&mut self) {
        surface::prepare(|app, ms, hardware| {
            crate::trace::event("surface_ready", || serde_json::json!({"ms": ms, "hardware": hardware}));
            app.surfaces_ready();
        });
    }

    /// The strip and grid's device is made (again): they repaint through it.
    pub fn surfaces_ready(&mut self) {
        strip::surfaces_ready(self);
    }

    pub fn scale(&self) -> f64 {
        self.dpi as f64 / 96.0
    }
    pub fn px(&self, dip: f64) -> i32 {
        (dip * self.scale()).round() as i32
    }

    /// Updates every part the model marked as changed.
    pub fn refresh(&mut self) {
        view_modes::sync(self);
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
        // While cropping it says the draft's aspect and size.
        if flags & (dirty::STATUS | dirty::CROP) != 0 {
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
        settings::sync(self);
        // The canvas's tools follow the chrome (the film strip's button).
        if flags & (dirty::CANVAS | dirty::ZOOM | dirty::CROP | dirty::CHROME) != 0 {
            canvas::update_scene(self);
            if self.model.image.is_some() && !self.model.first_render_shown {
                self.model.first_render_shown = true;
                self.report.stamp("first_image_scene");
                // The strip's thumbnails waited for it.
                self.model.dirty |= dirty::STRIP;
                crate::film_cache_release::watch_memory_pressure();
                // A batch the last session didn't finish, once launch has settled.
                batch::offer_resume(self);
                // Which Lightroom is installed, off the launch path (ticket 48).
                self.model.refresh_lightroom(std::time::Duration::from_secs(2));
            }
        }
        if flags & (dirty::STRIP | dirty::CHROME) != 0 {
            strip::sync(self);
            grid::sync(self);
        }
        if flags & (dirty::COMPARE | dirty::CANVAS | dirty::LAYOUT | dirty::CHROME) != 0 {
            compare::sync(self);
        }
        if flags & (dirty::TRAY | dirty::PREVIEWS | dirty::CANVAS | dirty::LAYOUT | dirty::CHROME | dirty::STRUCTURE) != 0 {
            tray::sync(self);
        }
        // What the preview tiles show follows the edit, the panels and the main render.
        let previews = dirty::PREVIEWS | dirty::INSPECTOR | dirty::STRUCTURE | dirty::CHROME | dirty::LAYOUT | dirty::PRESETS | dirty::CANVAS;
        if flags & previews != 0 && self.model.prefs.previews {
            // Only once tiles are on screen: reading the profile costs launch time.
            if self.model.previews_on_screen() {
                self.follow_preview_monitor();
            }
            if self.model.pump_previews() || flags & dirty::PREVIEWS != 0 {
                tray::redraw(self);
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
        // The band says the file's name alone (`TitleBar.swift`).
        let name = model.compare_title().or_else(|| model.source.as_deref().map(crate::model::file_name)).unwrap_or_else(|| "Negs".into());
        titlebar::set_title(self.main, &name);
        panes::sync(self.panes, model.pane() == crate::model::Pane::Presets);
        self.update_taskbar();
        view_switch::sync(self);
    }

    /// The status line (§4.7): what's happening on the left, the canvas's
    /// tools on the right.
    fn update_status(&mut self) {
        use status::{Span, Tone};
        let model = &self.model;
        let mut left = vec![];
        let batch = model.batch.as_ref();
        let batch_running = batch.is_some_and(|b| b.running);
        let mut progress = None;
        let mut show = false;
        if let Some(words) = library::status_words(self) {
            // "Edits pasted", "★★★": a library action's note, for a moment.
            left = words;
        } else if let Some(run) = batch.filter(|b| b.running) {
            // "Exporting 5 of 12 photographs" and its rule.
            left.push(Span::new(run.progress_words(), Tone::Label));
            progress = Some(run.fraction());
        } else if let Some(words) = &model.lightroom.words {
            // "Sending 3 photographs to Lightroom…", "Starting Lightroom…".
            left.push(Span::new(words.clone(), Tone::Label));
        } else if let Some(run) = batch.filter(|b| b.needs_attention()) {
            // "Exported 10 of 12 · 2 failed  Show".
            let (words, failed) = run.outcome_words();
            left.push(Span::new(words, Tone::Label));
            if let Some(failed) = failed {
                left.push(Span::new("·", Tone::Label));
                left.push(Span::strong(failed, Tone::Paper));
            }
            show = true;
        } else if model.exporting {
            let name = model.source.as_deref().map(crate::model::file_name).unwrap_or_default();
            left.push(Span::new(format!("Exporting {name}"), Tone::Label));
            left.push(Span::new(format!("· {} s", model.export_started.elapsed().as_secs()), Tone::Dim));
        } else if model.render_failed && model.develop_error.is_some() {
            let name = model.source.as_deref().map(crate::model::file_name).unwrap_or_default();
            left.push(Span::strong("Couldn’t develop", Tone::Label));
            left.push(Span::new(name, Tone::Dim));
        } else if let Some(session) = &model.crop_session {
            // "Cropping · Original · 6016 × 4016 px": the draft's aspect and
            // the pixels an export develops, as the crop pane's Size says.
            let size = match model.crop_draft_size() {
                Some(Ok(size)) => (size.photo.0 as f64, size.photo.1 as f64),
                _ => (session.shown().width, session.shown().height),
            };
            left.push(Span::new(
                format!("Cropping · {} · {} × {} px", session.aspect.title(), size.0.round() as i64, size.1.round() as i64),
                Tone::Dim,
            ));
        } else if let Some(c) = model.comparison() {
            let differences = compare::differences(self);
            let summary = model.compare_summary();
            left.push(Span::new(if differences.is_empty() { summary } else { format!("{summary} · differ in {differences}") }, Tone::Label));
            if !c.against_original() && model.shown_items().len() > 2 {
                let place = if compare::stacked(self) { "bottom" } else { "right" };
                left.push(Span::hint(format!("    ← → steps the {place} one"), Tone::Dim));
            }
        } else if !(model.status == "Starting…" && model.source.is_none()) && !model.status.is_empty() {
            left.push(Span::new(model.status.clone(), Tone::Dim));
        }
        let tools = (model.source.is_some() && !batch_running).then(|| status::Tools {
            original: model.can_show_original() && !model.cropping(),
            holding: model.holding_original,
            cropping: model.cropping(),
            failed: model.render_failed && model.develop_error.is_some(),
            compare: model.comparison().map(|c| c.layout == crate::compare::Layout::Slider),
        });
        status::set(status::Line { left, cancel: model.exporting || batch_running, tools, progress, show });
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
            // Image Only leaves the photograph alone in the window.
            let image_only = self.view.image_only;
            let shows_inspector = self.model.inspector_visible && !image_only;
            INSPECTOR_SHOWN.with(|s| s.set(shows_inspector));
            // Make room before the adjustments appear, so neither column is
            // squeezed below its minimum.
            self.fit_minimum();
            let mut client = RECT::default();
            let _ = GetClientRect(self.main, &mut client);
            titlebar::layout(self.main, self.menubar);
            let top = titlebar::height(self.main);
            let bottom = client.bottom;
            let inspector_width = if shows_inspector {
                self.px(inspector_width(self.model.prefs.inspector_width, client.right as f64 / self.scale()))
            } else {
                0
            };
            let splitter = if shows_inspector { self.px(SPLITTER) } else { 0 };
            let column = (client.right - inspector_width - splitter).max(0);
            let photo = self.model.source.is_some();
            // The film or paper tray takes the strip's and the status line's place.
            let shows_tray = photo && !self.drop_target_active && self.model.tray_shown().is_some();
            let tray_height = if shows_tray { self.px(crate::tray_rules::HEIGHT) } else { 0 };
            // The status line under the strip, with a photograph open; both
            // step aside in Image Only and while a file is dragged in.
            let chrome = photo && !image_only && !self.drop_target_active && !shows_tray;
            let status_height = if chrome { self.px(status::HEIGHT) } else { 0 };
            // Compare has no strip: its height goes to the photographs.
            let comparing = photo && self.model.comparison().is_some();
            let show_strip = self.model.prefs.filmstrip_visible && chrome && !comparing;
            // 158 + 28: the tray's 186, so the canvas keeps its size when it opens.
            let strip_height = if show_strip { self.px(crate::sheet_rules::BAND) - status_height } else { 0 };
            let canvas_bottom = (bottom - status_height - strip_height - tray_height).max(top);
            tray::layout(self, RECT { left: 0, top: bottom - tray_height, right: column, bottom }, shows_tray);
            let _ = MoveWindow(self.canvas, 0, top, column, canvas_bottom - top, true);
            let _ = MoveWindow(self.empty, 0, top, column, bottom - top, true);
            // Compare takes the canvas's place.
            compare::layout(self, RECT { left: 0, top, right: column, bottom: canvas_bottom }, comparing);
            let _ = ShowWindow(self.canvas, if photo && !comparing { SW_SHOWNA } else { SW_HIDE });
            let _ = ShowWindow(self.empty, if photo { SW_HIDE } else { SW_SHOWNA });
            let _ = MoveWindow(self.strip, 0, canvas_bottom, column, strip_height, true);
            let _ = ShowWindow(self.strip, if show_strip { SW_SHOWNA } else { SW_HIDE });
            // The leader stands on the strip's top edge, or the status line's.
            strip::layout_leader(self, column, canvas_bottom, chrome && !self.model.cropping() && !comparing);
            grid::layout(self, RECT { left: 0, top, right: client.right, bottom });
            let _ = MoveWindow(self.status, 0, bottom - status_height, column, status_height, true);
            let _ = ShowWindow(self.status, if chrome { SW_SHOWNA } else { SW_HIDE });
            titlebar::set_inspector_edge(self.main, if shows_inspector { column + splitter } else { 0 });
            let shows_presets = presets::shown(&self.model);
            // The switch between the panels; cropping has the column to itself.
            let shows_panes = shows_inspector && !self.model.cropping();
            let panes_height = if shows_panes { self.px(panes::HEIGHT) } else { 0 };
            let _ = MoveWindow(self.panes, column + splitter, top, inspector_width, panes_height, true);
            let _ = ShowWindow(self.panes, if shows_panes { SW_SHOWNA } else { SW_HIDE });
            // Which photograph the panel edits, while comparing.
            let note_height = if shows_panes && comparing { self.px(compare::NOTE_HEIGHT) } else { 0 };
            let note = RECT { left: column + splitter, top: top + panes_height, right: column + splitter + inspector_width, bottom: top + panes_height + note_height };
            compare::layout_note(self, note, note_height > 0);
            let panel_top = top + panes_height + note_height;
            self.inspector.layout(column + splitter, panel_top, inspector_width, bottom - panel_top, shows_inspector && !shows_presets);
            let rect = RECT { left: column + splitter, top: panel_top, right: column + splitter + inspector_width, bottom };
            presets::layout(self, rect, shows_presets && !image_only);
            canvas::resized(self.canvas);
            // Their own WM_SIZE arrives while the app is busy here.
            empty::layout(self);
            strip::resized(self);
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

    pub fn command(&mut self, command: u16) {
        // A command keeps the stock being tried and closes the tray, but
        // editing the text being typed doesn't. The library's commands too
        // (Undo with a library step on top undoes after the visit's step).
        let editing = matches!(command, id::CUT | id::COPY | id::PASTE | id::DELETE | id::SELECT_ALL) && text_focus();
        if !editing {
            self.model.end_tray(true);
        }
        if library::command(self, command) {
            return;
        }
        let model = &mut self.model;
        match command {
            id::OPEN | id::EMPTY_OPEN => dialogs::open(self),
            id::SAVE_PREVIEW => dialogs::save(self, false),
            id::EXPORT => {
                // The grid's selection exports as a batch (`dialogs::batch`).
                if self.model.exports_single() {
                    dialogs::save(self, true)
                } else {
                    dialogs::batch(self)
                }
            }
            c if (id::LIGHTROOM..id::LIGHTROOM + 2).contains(&c) => {
                model.open_in_lightroom(crate::lightroom::Kind::ALL[(c - id::LIGHTROOM) as usize]);
            }
            id::EXPORT_LUT => dialogs::export_lut(self),
            id::SETTINGS => settings::open(self),
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
            id::ALL_CONTROLS => {
                model.prefs.lab_all_controls = !model.prefs.lab_all_controls;
                model.prefs.save();
                model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR | dirty::CHROME;
            }
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
            id::ROTATE if model.source.is_some() => {
                // As on the Mac, a turn starts cropping, where Cancel undoes it.
                model.begin_crop();
                model.rotate()
            }
            // Like a turn, a flip starts cropping, where Cancel undoes it.
            id::FLIP if model.source.is_some() => model.flip(),
            id::COMPARE => compare::toggle(self),
            id::COMPARE_OFFER => compare::take_offer(self),
            id::GRID => grid::toggle(self),
            id::VIEW_GRID | id::VIEW_PHOTO | id::VIEW_COMPARE => view_switch::act(self, command),
            id::IMAGE_ONLY => view_modes::toggle_image_only(self),
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
            id::CANCEL_EXPORT => {
                model.cancel_export();
                model.cancel_batch();
            }
            id::COPY_STATUS => copy_text(self.main, &status::text()),
            c if (id::THEME..id::THEME + theme::ThemeName::ALL.len() as u16).contains(&c) => {
                self.choose_theme(theme::ThemeName::ALL[(c - id::THEME) as usize]);
            }
            c if (id::SHEET..id::SHEET + sheet::ViewerSheet::ALL.len() as u16).contains(&c) => {
                self.choose_sheet(sheet::ViewerSheet::ALL[(c - id::SHEET) as usize]);
            }
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
        enable(id::EXPORT, model.can_export_any());
        retitle(id::EXPORT, &format!("&{}\tCtrl+Shift+E", model.export_title()));
        // Ticket 48: after Export, only the Lightroom apps installed.
        batch::lightroom_items(self, menu, id::EXPORT_LUT, false);
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
        check(id::ALL_CONTROLS, model.prefs.lab_all_controls);
        enable(id::NEW_PRESET, model.can_change_presets());
        enable(id::IMPORT_PRESETS, model.can_change_presets());
        check(id::FULL_SCREEN, self.full_screen.is_some());
        check(id::IMAGE_ONLY, self.view.image_only);
        enable(id::IMAGE_ONLY, photo && !model.cropping());
        check(id::ORIGINAL, model.holding_original);
        enable(id::ORIGINAL, model.can_show_original() && !model.cropping());
        for c in [id::FIT, id::ACTUAL, id::ZOOM_IN, id::ZOOM_OUT] {
            enable(c, photo && !model.cropping());
        }
        enable(id::ROTATE, photo);
        enable(id::FLIP, photo);
        enable(id::COMPARE, model.can_toggle_compare());
        retitle(id::COMPARE, crate::compare::menu_title(model.comparison().is_some()));
        enable(id::GRID, model.shelf.library.is_some() && !model.cropping());
        retitle(id::CROP, if model.cropping() { "Done &Cropping\tCtrl+K" } else { "&Crop\tCtrl+K" });
        enable(id::CROP, model.cropping() || model.image.is_some());
        // View › Theme: the one chosen ticked, as a radio choice (an
        // environment override shows what it forces).
        let shown = if theme::forced_by_environment() { theme::shown() } else { theme::chosen() };
        let count = theme::ThemeName::ALL.len() as u32;
        unsafe {
            if let Some(i) = theme::ThemeName::ALL.iter().position(|&t| t == shown) {
                let _ = CheckMenuRadioItem(menu, id::THEME as u32, id::THEME as u32 + count - 1, id::THEME as u32 + i as u32, MF_BYCOMMAND.0);
            }
            // View › Viewer Sheet, likewise.
            let count = sheet::ViewerSheet::ALL.len() as u32;
            if let Some(i) = sheet::ViewerSheet::ALL.iter().position(|&s| s == sheet::chosen()) {
                let _ = CheckMenuRadioItem(menu, id::SHEET as u32, id::SHEET as u32 + count - 1, id::SHEET as u32 + i as u32, MF_BYCOMMAND.0);
            }
        }
        library::update_menu(self, menu);
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
        // The open photograph's edits go to its sidecar first; Negs stays open
        // if they can't, as it does for the session. Quitting keeps the
        // stock the tray is trying, as any other command does.
        self.model.end_tray(true);
        if let Err(error) = self.model.library_save_now() {
            if !ending_session {
                self.model.error = Some(error);
                self.model.dirty |= dirty::ERROR;
                return false;
            }
        }
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
            self.model.prefs.window = Some([show, r.left, r.top, ((r.right - r.left) as f64 / s).round() as i32, ((r.bottom - r.top) as f64 / s).round() as i32]);
            self.model.prefs.window_pixels = Some([self.dpi as i32, r.right - r.left, r.bottom - r.top]);
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
        // ⌘ is Ctrl and ⌥ is Alt; ⌥⌘ shortcuts (Ctrl+Alt is AltGr on many
        // keyboards) keep Windows' own keys: F6, F7, F8.
        let file = popup("&File");
        add(file, id::OPEN, "&Open…\tCtrl+O");
        let recent = CreatePopupMenu().unwrap();
        let w = wide("Open &Recent");
        let _ = AppendMenuW(file, MF_POPUP, recent.0 as usize, pcwstr(&w));
        separator(file);
        add(file, id::SAVE_PREVIEW, "&Save Preview…\tCtrl+S");
        add(file, id::EXPORT, "&Export Photograph…\tCtrl+Shift+E");
        add(file, id::EXPORT_LUT, "Export &LUT…\tCtrl+Shift+L");
        separator(file);
        add(file, id::NEW_PRESET, "&New Preset\tCtrl+Shift+S");
        add(file, id::IMPORT_PRESETS, "&Import Presets…\tCtrl+Shift+O");
        add(file, id::PRESETS_FOLDER, "Show Presets &Folder");
        separator(file);
        add(file, id::SETTINGS, "Se&ttings…\tCtrl+,");
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
        let photo = popup("&Photo");
        add(photo, id::ORIGINAL, "Show &Original\tHold O");
        // Ratings, flags, copying edits, Reset Edits and the rest.
        library::add_menu(photo);
        let view = popup("&View");
        add(view, id::FILMSTRIP, "&Film Strip\tF6");
        add(view, id::ADJUSTMENTS, "&Adjustments\tF7");
        add(view, id::PRESETS, "&Presets\tF8");
        add(view, id::FULL_SCREEN, "F&ull Screen\tF11");
        add(view, id::IMAGE_ONLY, "&Image Only\tF");
        // Grid · Photo · Compare, as the Mac's title bar switch (ticket 46).
        add(view, id::GRID, "&Grid\tG");
        add(view, id::COMPARE, crate::compare::menu_title(false));
        // Every control in every module, as the Lab's Advanced controls switch did.
        add(view, id::ALL_CONTROLS, "Show A&ll Controls");
        separator(view);
        add(view, id::FIT, "Fi&t Photograph\tCtrl+0");
        add(view, id::ACTUAL, "Actual &Size\tCtrl+1");
        add(view, id::ZOOM_IN, "Zoom &In\tCtrl+=");
        add(view, id::ZOOM_OUT, "Zoom &Out\tCtrl+-");
        separator(view);
        add(view, id::ROTATE, "&Rotate Clockwise\tCtrl+R");
        add(view, id::FLIP, "Flip &Horizontal");
        add(view, id::CROP, "&Crop\tCtrl+K");
        separator(view);
        let themes = CreatePopupMenu().unwrap();
        for (i, name) in theme::ThemeName::ALL.into_iter().enumerate() {
            add(themes, id::THEME + i as u16, name.title());
        }
        let w = wide("T&heme");
        let _ = AppendMenuW(view, MF_POPUP, themes.0 as usize, pcwstr(&w));
        // The same sheets as Settings › Look › Viewer.
        let sheets = CreatePopupMenu().unwrap();
        for (i, s) in sheet::ViewerSheet::ALL.into_iter().enumerate() {
            add(sheets, id::SHEET + i as u16, s.title());
        }
        let w = wide("&Viewer Sheet");
        let _ = AppendMenuW(view, MF_POPUP, sheets.0 as usize, pcwstr(&w));
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
        // C compares, as on the Mac; Ctrl+K crops.
        (FVIRTKEY, 'C' as u16, id::COMPARE),
        (FVIRTKEY, 'F' as u16, id::IMAGE_ONLY),
        (ctrl, VK_OEM_COMMA.0, id::SETTINGS),
    ];
    let mut accels: Vec<ACCEL> = table.iter().map(|&(f, key, cmd)| ACCEL { fVirt: f, key, cmd }).collect();
    accels.extend(library::accelerators());
    unsafe { CreateAcceleratorTableW(&accels).unwrap_or_default() }
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
    if grid_bench::run_if_asked() {
        return;
    }
    if sampler::run_if_asked() {
        return;
    }
    // Direct2D, DirectWrite and Newsreader, on their own thread from the
    // start: the title bar's first paint sets the file name with them.
    let drawing_started = platform::qpc();
    d2d::prepare(move |shared| {
        let cost = shared.cost;
        d2d::warm(&[
            ("Negs DSC_0001.NEF", titlebar::NAME),
            ("Open a photograph", empty::HEADING),
            ("Explore film stocks, shape the print", empty::TAGLINE),
            ("or drop a RAW, TIFF, PNG or JPEG file here", empty::HINT),
            ("6016 × 4016 · developed in 1.2 s Original Fit 100%", status::WORDS),
        ]);
        let at = platform::qpc();
        post(move |app: &mut App| {
            app.report.line(&format!("drawing_started {drawing_started}"));
            app.report.line(&format!("drawing_ready {at}"));
            app.report.line(&format!("drawing_cost d2d_us {} dwrite_us {} fonts_us {}", cost.d2d.as_micros(), cost.dwrite.as_micros(), cost.fonts.as_micros()));
            // A photograph shown before now has no caption or label yet.
            canvas::drawing_ready(app.canvas);
        });
    });
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
        color_filter::register();
        presets::register();
        status::register();

        // View › Theme, unless NEGS_THEME forces one. A new user starts on
        // Plain or Plain dark, as Windows is, kept from then on.
        if model.prefs.app_theme.is_empty() {
            model.prefs.app_theme = theme::first_launch_theme().key().to_string();
            model.prefs.save();
        }
        if let Some(chosen) = theme::ThemeName::parse(&model.prefs.app_theme) {
            theme::set_chosen(chosen);
        }
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
        titlebar::set_fonts(fonts.ui);
        report.stamp("toolbar");
        let status = status::create(main);
        let canvas = create(w!("NegsCanvas"), "Photograph preview", WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let empty = create(w!("NegsEmpty"), "", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
        let strip = create(w!("NegsStrip"), "Film Strip", WS_CHILD | WS_TABSTOP, WINDOW_EX_STYLE(0), main, 0, HFONT::default());
        let panes = panes::create(main, fonts.ui);
        let inspector = inspector::Inspector::create(main, &fonts, dpi);
        canvas::init(canvas, dpi);

        let mut app = App {
            model,
            main,
            menubar,
            status,
            canvas,
            empty,
            strip,
            panes,
            inspector,
            presets_view: None,
            settings: None,
            fonts,
            dpi,
            menu,
            accelerators: accelerators(),
            report,
            splitter_drag: None,
            full_screen: None,
            strip_view: strip::Strip::default(),
            grid: None,
            thumbs: thumbs::Thumbs::default(),
            empty_view: empty::Empty::default(),
            taskbar: None,
            original_key: None,
            drop_target_active: false,
            view: Default::default(),
            quitting: false,
            preview_monitor: 0,
        };
        let _ = recent;
        empty::build(&mut app);
        sheet::set_chosen(sheet::from_prefs(&app.model.prefs));
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
    // At the DPI it was saved at, the very pixels; else the DIPs scaled.
    let (w, h) = match prefs.window_pixels {
        Some([saved, pw, ph]) if saved == dpi as i32 && pw > 0 && ph > 0 => (pw, ph),
        _ => ((w as f64 * s).round() as i32, (h as f64 * s).round() as i32),
    };
    let rect = RECT { left: x, top: y, right: x + w, bottom: y + h };
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
    // The Settings window's keys are its own.
    if let Some(handled) = settings::pre_translate(msg) {
        return handled;
    }
    if titlebar::keys(main, msg) {
        return true;
    }
    leave_field_on_click(main, msg);
    let mut accelerators = None;
    let handled = with_app(|app| {
        if matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MOUSEWHEEL) {
            app.report.stamp("first_input_received");
        }
        // The tray's keys come first (§5.2: after an alert and typing), and
        // a press outside it keeps its stock and closes it.
        if tray::key(app, msg) {
            return true;
        }
        if matches!(msg.message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) && app.model.tray().is_some() && !tray::keeps_open(app, msg) {
            app.model.end_tray(true);
        }
        // Rating keys never reach the library while typing (in the tray's Search too).
        if app.original_key(msg) || view_modes::key(app, msg, text_focus()) || grid::key(app, msg, text_focus()) || library::key(app, msg) {
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
        // Compare's keys: S, the arrows, Esc.
        if !text_focus() && compare::key(app, msg) {
            return true;
        }
        // ← and → open the neighbours wherever the focus is, as on the Mac.
        if strip::arrows_to_step(app, msg) {
            return true;
        }
        // A picker waiting for a click on the negative.
        if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_ESCAPE.0 as usize && !text_focus() && app.model.cancel_negative_pick() {
            return true;
        }
        // Escape's next use (§5.2): leaving Image Only.
        if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_ESCAPE.0 as usize && !text_focus() && view_modes::escape(app) {
            return true;
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

/// A press outside the text field being typed in ends the typing, as on the
/// Mac (`EditorWindow.sendEvent`), even where nothing takes the focus: the
/// inspector's empty space, labels, the title bar. The fields apply their
/// value when they lose the focus; that runs before the press is dispatched,
/// so the clicked control sees the typed value.
fn leave_field_on_click(main: HWND, msg: &MSG) {
    if !matches!(
        msg.message,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN
    ) {
        return;
    }
    unsafe {
        let focus = GetFocus();
        if focus.0.is_null() || focused_class().as_deref() != Some("Edit") || msg.hwnd == focus {
            return;
        }
        // A keyed-out field's own box: the window under it hands the press on.
        if draw::keyed::covers(focus, msg.pt) {
            return;
        }
        // Only the main window's fields, and only for presses inside it.
        if !IsChild(main, focus).as_bool() || !(msg.hwnd == main || IsChild(main, msg.hwnd).as_bool()) {
            return;
        }
        // An editable combo box's arrow belongs to its field.
        if let Ok(parent) = GetParent(focus) {
            if parent == msg.hwnd && draw::class_of(parent) == "ComboBox" {
                return;
            }
        }
        let _ = SetFocus(main);
    }
    // The commit was posted from WM_KILLFOCUS; apply it before the press.
    drain();
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
            // The fitted view's exact picture waits until sizing ends.
            WM_ENTERSIZEMOVE | WM_EXITSIZEMOVE => {
                canvas::set_live_resize(msg == WM_ENTERSIZEMOVE);
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
                if let Some(result) = view_switch::notify(lparam) {
                    return result;
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_ENTERMENULOOP => {
                // A menu keeps the stock being tried and closes the tray.
                if with_app(|app| app.model.end_tray(true)).is_none() {
                    post(|app: &mut App| app.model.end_tray(true));
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
                    with_app(|app| {
                        app.release_original();
                        view_modes::release(app);
                    });
                    titlebar::deactivated(hwnd);
                } else {
                    alert::reclaim_focus();
                }
                titlebar::invalidate(hwnd);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_ACTIVATEAPP if wparam.0 != 0 => {
                // Back from another app: the folder as it left it (`refresh`),
                // and Lightroom installed meanwhile or photos added there.
                post(|app: &mut App| {
                    app.model.library_refresh();
                    if app.model.lightroom_detected() {
                        app.model.refresh_lightroom(std::time::Duration::ZERO);
                    }
                });
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
            panes::set_font(self.panes, self.fonts.ui);
            SendMessageW(self.menubar, WM_SETFONT, WPARAM(self.fonts.ui.0 as usize), LPARAM(1));
            titlebar::set_metrics(self.main, self.menubar);
            titlebar::set_fonts(self.fonts.ui);
            let _ = InvalidateRect(self.status, None, false);
        }
        canvas::set_dpi(dpi);
        inspector::dpi_changed(self);
        presets::dpi_changed(self);
        empty::dpi_changed(self);
        self.model.dirty |= dirty::LAYOUT | dirty::STRUCTURE | dirty::CANVAS | dirty::STRIP;
    }

    /// Follows a change of the system's theme (Plain follows it), contrast
    /// or accent, or of View › Theme.
    fn retheme(&mut self) {
        if !theme::refresh() {
            return;
        }
        theme::apply_all();
        presets::retheme(self);
        settings::retheme(self);
        unsafe {
            let _ = RedrawWindow(self.main, None, None, RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME);
        }
        self.model.dirty |= dirty::LAYOUT | dirty::STRUCTURE | dirty::STRIP | dirty::CANVAS | dirty::CHROME;
    }

    /// View › Theme: shows `name` and remembers it in Settings.
    fn choose_theme(&mut self, name: theme::ThemeName) {
        theme::set_chosen(name);
        self.model.prefs.app_theme = name.key().to_string();
        self.model.prefs.save();
        self.retheme();
    }

    /// View › Viewer Sheet and Settings › Look › Viewer: shows `sheet`
    /// behind the photograph and remembers it.
    fn choose_sheet(&mut self, sheet: sheet::ViewerSheet) {
        sheet::set_chosen(sheet);
        self.model.prefs.viewer_sheet = sheet.key().into();
        self.model.prefs.save();
        canvas::sheet_changed(self.canvas);
        empty::sheet_changed(self);
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

}

pub fn dwm_flush() {
    unsafe {
        let _ = DwmFlush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window keeps room for both columns, or the canvas alone.
    #[test]
    fn minimum_width_follows_the_adjustments() {
        assert_eq!(minimum_width(true), 985.0);
        assert_eq!(minimum_width(false), 640.0);
    }

    /// A narrow window narrows the adjustments before the canvas, down to
    /// their own minimum.
    #[test]
    fn adjustments_give_way_to_the_canvas() {
        assert_eq!(inspector_width(340.0, 1400.0), 340.0);
        assert_eq!(inspector_width(440.0, 1000.0), 355.0);
        assert_eq!(inspector_width(440.0, 900.0), 340.0);
        assert_eq!(inspector_width(500.0, 2000.0), 440.0);
        // A width saved under the old 300 minimum shows at the new one.
        assert_eq!(inspector_width(300.0, 1400.0), 340.0);
    }
}
