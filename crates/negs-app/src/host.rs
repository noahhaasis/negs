//! UI-toolkit-agnostic boundary between `negs-app` and the active UI shell
//! (Win32 today, GPUI in `app/`, or the built-in headless queue in tests).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use crate::model::Model;

pub type ModelTask = Box<dyn FnOnce(&mut Model) + Send + 'static>;
pub type TimedModelTask = Box<dyn FnOnce(&mut Model) + 'static>;

/// Callbacks the shared core uses to reach the UI shell.
///
/// Every method other than `post` and `after` has a sensible headless default.
pub trait Host: Send + Sync + 'static {
    /// Schedules `task` to run on the UI thread with the active `Model`.
    fn post(&self, task: ModelTask);

    /// Schedules `task` to run on the UI thread after `delay`.
    fn after(&self, delay: Duration, task: TimedModelTask);

    /// Delivers a newly filed edited thumbnail (BGRA pixels) to the strip/grid cache.
    fn thumbnail_filed(&self, _key: String, _bgra: Vec<u8>, _width: u32, _height: u32) {}

    /// Notifies the UI that batch export progress or status changed.
    fn batch_refresh(&self) {}

    /// Notifies the UI that a single-photo export succeeded (e.g. taskbar flash on Windows).
    fn export_finished(&self) {}

    /// Records a newly opened photograph in the OS recent-documents list.
    fn note_recent_document(&self, _path: &Path) {}

    /// The locale's decimal separator for formatting user-facing numbers.
    fn decimal_separator(&self) -> char {
        '.'
    }

    /// Physical screen pixels per photograph pixel currently shown on the canvas.
    fn canvas_scale(&self) -> Option<f64> {
        None
    }

    /// Marks background work starting (tracked by tests to determine quiescence).
    fn background_enter(&self) {}

    /// Marks background work finishing.
    fn background_leave(&self) {}

    /// Whether the OS logged a display driver reset at or after `since`.
    fn display_reset_since(&self, since: SystemTime) -> bool {
        crate::platform::display_reset_since(since)
    }

    /// Whether the automated render benchmark is driving the app.
    fn bench_running(&self) -> bool {
        false
    }

    /// Optional native owner window handle (HWND as `isize` on Windows, `0` otherwise).
    fn owner_window_handle(&self) -> isize {
        0
    }
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();
static DEFAULT_HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// Registers the UI shell's `Host`. Should be called once at startup before
/// `Model::launch`.
pub fn register(host: Arc<dyn Host>) {
    let _ = HOST.set(host);
}

/// The registered `Host`, or the built-in headless test host when none was registered.
pub fn current() -> &'static Arc<dyn Host> {
    HOST.get().unwrap_or_else(|| DEFAULT_HOST.get_or_init(|| Arc::new(TestHost)))
}

pub fn post(task: impl FnOnce(&mut Model) + Send + 'static) {
    current().post(Box::new(task));
}

pub fn after(delay: Duration, task: impl FnOnce(&mut Model) + 'static) {
    current().after(delay, Box::new(task));
}

pub fn thumbnail_filed(key: String, bgra: Vec<u8>, width: u32, height: u32) {
    current().thumbnail_filed(key, bgra, width, height);
}

pub fn batch_refresh() {
    current().batch_refresh();
}

pub fn export_finished() {
    current().export_finished();
}

pub fn note_recent_document(path: &Path) {
    current().note_recent_document(path);
}

pub fn decimal_separator() -> char {
    current().decimal_separator()
}

pub fn canvas_scale() -> Option<f64> {
    if let Some(scale) = testing::fixed_scale() {
        return (scale > 0.0).then_some(scale);
    }
    current().canvas_scale()
}

pub fn display_reset_since(since: SystemTime) -> bool {
    current().display_reset_since(since)
}

pub fn bench_running() -> bool {
    current().bench_running()
}

pub fn owner_window_handle() -> isize {
    current().owner_window_handle()
}

/// Default headless `Host` backed by the in-process test queues.
pub struct TestHost;

impl Host for TestHost {
    fn post(&self, task: ModelTask) {
        testing::QUEUE.lock().unwrap().push_back(task);
    }

    fn after(&self, delay: Duration, task: TimedModelTask) {
        let due = Instant::now() + delay;
        testing::TIMED.with(|t| t.borrow_mut().push((due, task)));
    }
}

/// Headless task queues, timers, background work counter, and fixed canvas
/// scale override for unit and lifecycle tests.
pub mod testing {
    use super::*;

    pub(super) static QUEUE: Mutex<VecDeque<ModelTask>> = Mutex::new(VecDeque::new());
    static BACKGROUND: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        pub(super) static TIMED: RefCell<Vec<(Instant, TimedModelTask)>> = const { RefCell::new(Vec::new()) };
        static FIXED_SCALE: Cell<Option<f64>> = const { Cell::new(None) };
    }

    /// Runs the oldest posted task with `model`. Returns `false` when empty.
    pub fn run_posted(model: &mut Model) -> bool {
        let task = QUEUE.lock().unwrap().pop_front();
        match task {
            Some(task) => {
                task(model);
                true
            }
            None => false,
        }
    }

    pub fn posted() -> usize {
        QUEUE.lock().unwrap().len()
    }

    /// Runs the earliest timed task due by `horizon` (on this thread).
    pub fn run_timer(model: &mut Model, horizon: Instant) -> bool {
        let task = TIMED.with(|t| {
            let mut t = t.borrow_mut();
            let earliest = t
                .iter()
                .enumerate()
                .filter(|(_, (at, _))| *at <= horizon)
                .min_by_key(|(_, (at, _))| *at)
                .map(|(i, _)| i);
            earliest.map(|i| t.remove(i).1)
        });
        match task {
            Some(task) => {
                task(model);
                true
            }
            None => false,
        }
    }

    /// When the earliest timed task on this thread is due.
    pub fn next_timer() -> Option<Instant> {
        TIMED.with(|t| t.borrow().iter().map(|(at, _)| *at).min())
    }

    /// Guard for background work that will post a task back to the model.
    pub struct Background(());

    pub fn background() -> Background {
        BACKGROUND.fetch_add(1, Ordering::SeqCst);
        current().background_enter();
        Background(())
    }

    impl Drop for Background {
        fn drop(&mut self) {
            current().background_leave();
            BACKGROUND.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn background_running() -> usize {
        BACKGROUND.load(Ordering::SeqCst)
    }

    /// Overrides `canvas_scale()` on this thread. `Some(0.0)` reports no canvas;
    /// `None` delegates back to the active `Host`.
    pub fn set_fixed_scale(scale: Option<f64>) {
        FIXED_SCALE.with(|s| s.set(scale));
    }

    pub fn fixed_scale() -> Option<f64> {
        FIXED_SCALE.with(Cell::get)
    }

    /// Drops queued posted and timed tasks.
    pub fn clear() {
        QUEUE.lock().unwrap().clear();
        TIMED.with(|t| t.borrow_mut().clear());
    }
}
