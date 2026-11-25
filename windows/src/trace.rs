//! The render timing trace (re-exported from `negs-app`) and the Win32
//! render benchmark runner (`NEGS_RENDER_BENCH`).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::model::FrameKind;
use crate::ui::{self, App};

pub use negs_app::trace::{enabled, event, frame_json};

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
        if app.model.render_failed && app.model.render_idle() {
            bench.done = true;
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
        Some(Some(film)) => {
            if !film.is_empty() {
                app.model.choose_film(&film);
            }
            ui::after(TICK, tick);
        }
        Some(None) => unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
            let _ = PostMessageW(ui::main_hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        None => {}
    }
}
