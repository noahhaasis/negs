//! The render timing trace and the render benchmark. Both are off unless
//! their environment variables are set, and cost one atomic load per event
//! when off.
//!
//! `NEGS_RENDER_TRACE=<file>` appends one JSON object per line: `event`, the
//! QPC tick `qpc` (the first line, `start`, gives `qpc_frequency`) and the
//! event's fields. The events:
//!
//! - `edit`: an edit made the preview stale (`revision`, `generation`).
//! - `render_request`: the model asked for a render (`kind` settled or drag,
//!   `revision`, `generation`, `interactive_scale`).
//! - `helper_send`, `helper_reply`: any command written to the helper and its
//!   answer (`request` id, `command`, `ms`), in the engine's worker.
//! - `render_reply`: a render's answer, before the UI thread sees it
//!   (`elapsed_ms`, `cached_pipeline`, `pipeline_ms`, `backend`, size).
//! - `render_finished`: the UI thread took it: `shown`, `held` for its
//!   brightness match (`kind`), or dropped (neither).
//! - `held_shown`, `held_dropped`: a held frame was shown once its match was
//!   in, or dropped (`why`: an edit, another photograph, closing).
//! - `match_start`, `match_solved`: a camera brightness solve.
//! - `output_abandoned`: a waiting Save or Export gave up (nothing will
//!   finish the match).
//! - `present`: the canvas drew a frame it hadn't drawn before, after the
//!   Vulkan present or the software blit (`frame`: the render's request id,
//!   revision, generation and kind; `canvas` vulkan or software).
//! - `composited`: DWM finished composing after that present (`DwmFlush`).
//! - `bench_*`: the benchmark's steps, below.
//!
//! `NEGS_RENDER_BENCH=<film>,<film>[,…]` drives the app for the trace: once
//! the photograph passed on the command line is developed, presented and
//! the preview is idle (no render or brightness match running or due), it
//! waits `NEGS_RENDER_BENCH_SETTLE_MS` (default 1500), chooses the next film
//! in the list (as a click on its tile does), and repeats
//! `NEGS_RENDER_BENCH_STEPS` times (default 10). Then it closes the window.
//! A step that doesn't settle in `NEGS_RENDER_BENCH_TIMEOUT_S` (default 900)
//! ends the run with `bench_timeout`, and a render that fails (the preview
//! idle with no frame to show) ends it with `bench_failed`. While it runs,
//! error and notice dialogs are skipped and logged as `dialog_skipped`.
//! `windows/measure_render.ps1` runs it
//! and `windows/render_trace.py` summarises the trace.

use std::cell::RefCell;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::model::{FrameKind, FrameTag};
use crate::ui::{self, App};

static TRACE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

fn file() -> Option<&'static Mutex<std::fs::File>> {
    TRACE
        .get_or_init(|| {
            let path = std::env::var_os("NEGS_RENDER_TRACE")?;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
            let start = json!({"event": "start", "qpc": crate::platform::qpc(), "qpc_frequency": crate::platform::qpc_frequency(), "pid": std::process::id()});
            let _ = writeln!(file, "{start}");
            Some(Mutex::new(file))
        })
        .as_ref()
}

/// Whether `NEGS_RENDER_TRACE` is set.
pub fn enabled() -> bool {
    file().is_some()
}

/// Appends an event when tracing; `fields` (an object) runs only then.
pub fn event(name: &str, fields: impl FnOnce() -> Value) {
    let Some(file) = file() else { return };
    let qpc = crate::platform::qpc();
    let mut line = fields();
    if !line.is_object() {
        line = json!({"value": line});
    }
    line["event"] = json!(name);
    line["qpc"] = json!(qpc);
    let mut text = line.to_string();
    text.push('\n');
    if let Ok(mut file) = file.lock() {
        let _ = file.write_all(text.as_bytes());
    }
}

pub fn frame_json(frame: Option<FrameTag>) -> Value {
    match frame {
        Some(f) => json!({"request": f.request, "revision": f.revision, "generation": f.generation, "kind": f.kind.name()}),
        None => Value::Null,
    }
}

struct Bench {
    films: Vec<String>,
    steps: usize,
    settle: Duration,
    timeout: Duration,
    step: usize,
    idle_since: Option<Instant>,
    step_started: Instant,
    done: bool,
}

thread_local! {
    static BENCH: RefCell<Option<Bench>> = const { RefCell::new(None) };
}

const TICK: Duration = Duration::from_millis(20);

/// The benchmark is driving the app. Its error and notice dialogs would stop
/// it (the tick doesn't run under a modal), so they go to the trace instead.
pub fn bench_running() -> bool {
    BENCH.with(|b| b.borrow().as_ref().is_some_and(|bench| !bench.done))
}

/// Starts the benchmark when `NEGS_RENDER_BENCH` is set.
pub fn start_bench() {
    let Some(films) = std::env::var("NEGS_RENDER_BENCH").ok() else { return };
    let films: Vec<String> = films.split(',').map(str::trim).filter(|f| !f.is_empty()).map(str::to_owned).collect();
    if films.is_empty() {
        return;
    }
    let number = |name: &str, default: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    let bench = Bench {
        steps: number("NEGS_RENDER_BENCH_STEPS", 10) as usize,
        settle: Duration::from_millis(number("NEGS_RENDER_BENCH_SETTLE_MS", 1500)),
        timeout: Duration::from_secs(number("NEGS_RENDER_BENCH_TIMEOUT_S", 900)),
        films,
        step: 0,
        idle_since: None,
        step_started: Instant::now(),
        done: false,
    };
    event("bench_start", || json!({"films": bench.films, "steps": bench.steps, "settle_ms": bench.settle.as_millis() as u64}));
    BENCH.with(|b| *b.borrow_mut() = Some(bench));
    ui::after(TICK, tick);
}

/// Settled, idle and on screen: the step is over.
fn settled(app: &App) -> bool {
    let model = &app.model;
    model.ready
        && model.source.is_some()
        && model.render_idle()
        && model.shown_frame.is_some_and(|f| f.kind == FrameKind::Settled)
        && ui::canvas::presented_frame() == model.shown_frame
}

fn tick(app: &mut App) {
    let next = BENCH.with(|b| {
        let mut b = b.borrow_mut();
        let bench = b.as_mut()?;
        if bench.done {
            return None;
        }
        let now = Instant::now();
        // Nothing will settle: the photograph or the film can't be developed.
        if app.model.render_failed && app.model.render_idle() {
            bench.done = true;
            // The error is in the last `render_reply` (and `dialog_skipped`).
            event("bench_failed", || json!({"step": bench.step, "film": app.model.state.film}));
            return Some(None);
        }
        if !settled(app) {
            bench.idle_since = None;
            if now.duration_since(bench.step_started) > bench.timeout {
                bench.done = true;
                event("bench_timeout", || json!({"step": bench.step}));
                return Some(None);
            }
            return Some(Some(String::new()));
        }
        let since = *bench.idle_since.get_or_insert(now);
        if now.duration_since(since) < bench.settle {
            return Some(Some(String::new()));
        }
        let frame = app.model.shown_frame;
        event("bench_idle", || json!({"step": bench.step, "frame": frame_json(frame), "film": app.model.state.film, "status": app.model.status, "match_note": app.model.brightness_match_note()}));
        if bench.step >= bench.steps {
            bench.done = true;
            event("bench_done", || json!({"steps": bench.steps}));
            return Some(None);
        }
        let current = bench.films.iter().position(|f| *f == app.model.state.film);
        let film = bench.films[current.map_or(0, |i| (i + 1) % bench.films.len())].clone();
        bench.step += 1;
        bench.idle_since = None;
        bench.step_started = now;
        event("bench_edit", || json!({"step": bench.step, "film": film}));
        Some(Some(film))
    });
    match next {
        // Waiting, or an edit to make.
        Some(Some(film)) => {
            if !film.is_empty() {
                app.model.choose_film(&film);
            }
            ui::after(TICK, tick);
        }
        // Finished: close the window as the user would.
        Some(None) => unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
            let _ = PostMessageW(ui::main_hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        None => {}
    }
}
