//! GPUI implementation of `negs_app::host::Host`.
//!
//! Bridges background worker callbacks (`post`) and UI-thread delayed timers
//! (`after`) into GPUI's foreground executor, exposes live physical canvas
//! scale to `Model`, and tracks the native window handle (for Windows ICC).
//!
//! Deliberately does NOT register a `negs_app::picture::register_uploader`,
//! so `Picture::with_pixels` always retains CPU RGBA8 buffers for
//! `color_display.rs`'s fused exact-fit Lanczos + ICC color management pass.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use negs_app::host::{Host, ModelTask, TimedModelTask};
use negs_app::model::{Model, dirty};

thread_local! {
    /// UI-thread delayed tasks scheduled via `Host::after` (`TimedModelTask` is `!Send`).
    static TIMED_TASKS: RefCell<Vec<(Instant, TimedModelTask)>> = const { RefCell::new(Vec::new()) };
}

/// Channels owned by the GPUI root view to drive `Host` callbacks on the foreground thread.
pub struct HostChannels {
    pub tasks: async_channel::Receiver<ModelTask>,
    pub timers: async_channel::Receiver<Instant>,
}

/// Shared GPUI `Host` registered once at startup before `Model::launch()`.
pub struct GpuiHost {
    task_tx: async_channel::Sender<ModelTask>,
    timer_tx: async_channel::Sender<Instant>,
    /// Physical screen pixels per photo pixel, stored as `f64::to_bits` (`0` = unknown).
    canvas_scale_bits: AtomicU64,
    /// Native owner window handle (`HWND` on Windows, `0` elsewhere).
    owner_hwnd: AtomicIsize,
}

impl GpuiHost {
    pub fn new() -> (Arc<Self>, HostChannels) {
        let (task_tx, tasks) = async_channel::unbounded();
        let (timer_tx, timers) = async_channel::unbounded();
        let host = Arc::new(Self {
            task_tx,
            timer_tx,
            canvas_scale_bits: AtomicU64::new(0),
            owner_hwnd: AtomicIsize::new(0),
        });
        (host, HostChannels { tasks, timers })
    }

    /// Updates the live canvas scale (`viewport_physical / photo_dims * zoom`)
    /// so `Model::render` picks exact interactive/settled sizing.
    pub fn set_canvas_scale(&self, scale: Option<f64>) {
        let bits = match scale {
            Some(s) if s.is_finite() && s > 0.0 => s.to_bits(),
            _ => 0,
        };
        self.canvas_scale_bits.store(bits, Ordering::Relaxed);
    }

    /// Records the OS window handle (used by `negs_app::icc` on Windows).
    pub fn set_owner_hwnd(&self, hwnd: isize) {
        self.owner_hwnd.store(hwnd, Ordering::Relaxed);
    }

    /// Runs every `Host::after` task whose deadline has elapsed on the UI thread.
    /// Returns the number of tasks executed.
    pub fn run_due_timers(model: &mut Model) -> usize {
        let now = Instant::now();
        let due: Vec<TimedModelTask> = TIMED_TASKS.with(|cell| {
            let mut tasks = cell.borrow_mut();
            let mut ready = Vec::new();
            let mut i = 0;
            while i < tasks.len() {
                if tasks[i].0 <= now {
                    ready.push(tasks.remove(i).1);
                } else {
                    i += 1;
                }
            }
            ready
        });
        let count = due.len();
        for task in due {
            task(model);
        }
        count
    }
}

impl Host for GpuiHost {
    fn post(&self, task: ModelTask) {
        let _ = self.task_tx.send_blocking(task);
    }

    fn after(&self, delay: Duration, task: TimedModelTask) {
        let due = Instant::now() + delay;
        TIMED_TASKS.with(|cell| cell.borrow_mut().push((due, task)));
        let _ = self.timer_tx.try_send(due);
    }

    fn thumbnail_filed(&self, key: String, bgra: Vec<u8>, width: u32, height: u32) {
        // Colour-managed here, on the filing thread, then shown in the strip
        // and grid at once (`thumbs::filed_from_host` wakes the UI).
        crate::thumbs::filed_from_host(key, bgra, width, height);
    }

    fn batch_refresh(&self) {
        self.post(Box::new(|model| {
            model.dirty |= dirty::STATUS | dirty::CHROME;
        }));
    }

    fn export_finished(&self) {
        self.post(Box::new(|model| {
            model.dirty |= dirty::STATUS | dirty::CHROME;
        }));
    }

    fn note_recent_document(&self, _path: &Path) {
        // OS recent-document registration is a Phase 6/7 polish item.
    }

    fn decimal_separator(&self) -> char {
        '.'
    }

    fn canvas_scale(&self) -> Option<f64> {
        let bits = self.canvas_scale_bits.load(Ordering::Relaxed);
        if bits == 0 {
            None
        } else {
            let scale = f64::from_bits(bits);
            (scale.is_finite() && scale > 0.0).then_some(scale)
        }
    }

    fn owner_window_handle(&self) -> isize {
        self.owner_hwnd.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_scale_round_trips() {
        let (host, _channels) = GpuiHost::new();
        assert_eq!(host.canvas_scale(), None);
        host.set_canvas_scale(Some(0.42));
        assert!((host.canvas_scale().unwrap() - 0.42).abs() < 1e-12);
        host.set_canvas_scale(Some(0.0));
        assert_eq!(host.canvas_scale(), None);
        host.set_canvas_scale(None);
        assert_eq!(host.canvas_scale(), None);
    }
}
