//! The lifecycle harness: drives the model's own render and brightness
//! lifecycle (`render` → `render_finished` → `reconcile_brightness_match` →
//! `brightness_solved` → `render` …) against the real helper, as the app runs
//! it, with no window.
//!
//! The model completes its requests by posting `FnOnce(&mut App)` tasks
//! (`ui::post`, and `ui::after` for timers). The harness wraps the model in a
//! headless `App` (null window handles, an inspector that is never built) and
//! runs those tasks unchanged, one at a time, as the message loop's `drain`
//! does. After each task it does what `App::refresh` does with the model:
//! takes the dirty marks, collects a reported error, and pumps the
//! thumbnails. It doesn't build or sync any window.
//!
//! - **Isolation.** The task queue is process-wide and other unit tests post
//!   to it, so each lifecycle test re-runs itself alone in a child process
//!   (`isolated`, the `lifecycle_test!` macro), with its own scratch
//!   `NEGS_PROFILE_DIR`. A static mutex runs one child at a time, whatever
//!   `--test-threads` says; they all use the GPU.
//! - **The helper.** `target\release\spektrafilm-native.exe` (or
//!   `SPEKTRAFILM_NATIVE_HELPER`), built from this checkout. Without it the
//!   tests print "skipped" and pass, like the other live tests, unless
//!   `NEGS_REQUIRE_HELPER=1`, which makes a missing helper a failure.
//! - **Timers.** `ui::after` tasks run when due by the clock, as in the app.
//!   `fire_timers` runs those due within a given time early ("run now"). A
//!   timer's task still sees the real clock: the 30 s output wait compares
//!   `waiting_output`'s `Instant`, which a test can move back.
//! - **Canvas scale.** `ui::canvas::set_fixed_scale` (physical screen pixels
//!   per photograph pixel) stands in for the canvas, which tests don't have.
//!   Without it a drag renders at export size, as with no canvas.
//! - **Idle** means: no posted task, no helper request in flight, no
//!   background thread that will post (`ui::testing::background`), and no
//!   render, solve, tone curve, thumbnail, Original or save running, for
//!   `QUIET` in a row.
//! - **Logs.** `model.render_log` lists every render request (kind, revision,
//!   generation, interactive scale); `commands()` every helper command in the
//!   order sent; `frames` every change of the shown frame (what the canvas
//!   would present next), sampled after each task, and `model.frame_log`
//!   every frame presented, also one replaced within a task; `errors` every
//!   error the model reported.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{FrameTag, Model, RenderRequest, dirty};
use crate::session::Preferences;
use crate::ui::{self, App};

/// How long the lifecycle must stay idle before `idle` counts.
const QUIET: Duration = Duration::from_millis(20);

/// One child at a time; also held by the child itself.
static SERIAL: Mutex<()> = Mutex::new(());

const CHILD: &str = "NEGS_HARNESS_CHILD";

/// A lifecycle test: the body runs in a child process of its own.
macro_rules! lifecycle_test {
    ($(#[$meta:meta])* fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            $crate::model::harness::isolated(concat!(module_path!(), "::", stringify!($name)), || $body);
        }
    };
}
pub(crate) use lifecycle_test;

/// Runs `body` in a child process that runs only this test, or runs it here
/// when this is that child. `path` is the test's `module_path!()` and name.
pub fn isolated(path: &str, body: impl FnOnce()) {
    // libtest names tests without the crate's name.
    let name = path.split_once("::").map_or(path, |(_, rest)| rest);
    let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if std::env::var(CHILD).is_ok_and(|child| child == name) {
        return body();
    }
    let profile = std::env::temp_dir().join(format!("negs-harness-profile-{}-{}", std::process::id(), name.replace("::", "-")));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).unwrap();
    let started = Instant::now();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, name)
        .env("NEGS_PROFILE_DIR", &profile)
        .env("NEGS_ALLOW_MULTIPLE", "1")
        .output()
        .expect("the test binary runs again");
    let _ = std::fs::remove_dir_all(&profile);
    let (stdout, stderr) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    eprintln!("--- {name} in a child process ({:.1} s) ---\n{stdout}{stderr}--- end of {name} ---", started.elapsed().as_secs_f64());
    assert!(output.status.success(), "{name} failed in its child process");
    // A filter that matched nothing would also succeed.
    assert!(stdout.contains("1 passed"), "{name} didn't run in its child process");
}

/// A model with the real helper, in a headless app.
pub struct Harness {
    pub app: App,
    /// The test photograph: 3072 × 2048 (6.3 MP), made once per machine.
    pub photo: PathBuf,
    /// Every shown frame, in order, as the canvas would present them.
    pub frames: Vec<FrameTag>,
    /// Every error the model reported (and `App::refresh` would show).
    pub errors: Vec<String>,
    /// The shown frame at the last refresh, which `clear_logs` keeps.
    last_frame: Option<FrameTag>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.app.model.shutdown();
        ui::canvas::set_fixed_scale(None);
    }
}

/// Seconds a drain may take before the test fails (`NEGS_HARNESS_TIMEOUT_S`,
/// default 600: a 6 MP render on the CPU fallback is well inside it).
fn timeout() -> Duration {
    Duration::from_secs(std::env::var("NEGS_HARNESS_TIMEOUT_S").ok().and_then(|s| s.parse().ok()).unwrap_or(600))
}

impl Harness {
    /// Launches the model as the app does (helper, catalog, then ready) with
    /// `prefs`, and waits until it is ready. None, after printing "skipped",
    /// when the release helper isn't built, unless `NEGS_REQUIRE_HELPER=1`.
    pub fn start(prefs: Preferences) -> Option<Harness> {
        assert!(std::env::var_os(CHILD).is_some(), "lifecycle tests run through lifecycle_test!, in a process of their own");
        if crate::engine::EnginePaths::locate().is_err() {
            assert!(std::env::var_os("NEGS_REQUIRE_HELPER").is_none(), "NEGS_REQUIRE_HELPER is set and the release helper isn't built");
            eprintln!("lifecycle harness skipped: build the release helper first");
            return None;
        }
        ui::testing::clear();
        let photo = test_photo();
        let mut model = Model::new(prefs);
        model.launch();
        assert!(model.engine.is_some(), "the helper didn't start: {:?}", model.error);
        let mut harness = Harness { app: App::headless(model), photo, frames: vec![], errors: vec![], last_frame: None };
        harness.drain_until("the catalog", |m| m.ready);
        assert!(harness.app.model.has_catalog(), "no catalog: {:?}", harness.errors);
        Some(harness)
    }

    pub fn model(&mut self) -> &mut Model {
        &mut self.app.model
    }

    /// Does what a click or key does: runs `edit` with the model, then what
    /// `App::refresh` does with it.
    pub fn edit<R>(&mut self, edit: impl FnOnce(&mut Model) -> R) -> R {
        let result = edit(&mut self.app.model);
        self.refreshed();
        result
    }

    /// Opens `path` and waits until its preview (and any match) is idle.
    pub fn open(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.edit(|m| m.open(&path));
        self.idle();
    }

    /// Runs one posted task, or one `ui::after` task that is due. False when
    /// neither was waiting.
    pub fn step(&mut self) -> bool {
        let ran = ui::testing::run_posted(&mut self.app) || ui::testing::run_timer(&mut self.app, Instant::now());
        if ran {
            self.refreshed();
        }
        ran
    }

    /// Runs the `ui::after` tasks due within `within` from now, as if that
    /// much time had passed (their tasks still see the real clock).
    pub fn fire_timers(&mut self, within: Duration) -> usize {
        let horizon = Instant::now() + within;
        let mut ran = 0;
        while ui::testing::run_timer(&mut self.app, horizon) {
            self.refreshed();
            ran += 1;
        }
        ran
    }

    /// What `App::refresh` does with the model, without windows: takes the
    /// dirty marks and a reported error, and pumps the thumbnails, until
    /// nothing is marked. Records a newly shown frame.
    fn refreshed(&mut self) {
        let model = &mut self.app.model;
        if model.shown_frame != self.last_frame {
            self.last_frame = model.shown_frame;
            self.frames.extend(model.shown_frame);
        }
        while model.dirty != 0 {
            let flags = std::mem::take(&mut model.dirty);
            if flags & dirty::ERROR != 0
                && let Some(error) = model.error.take()
            {
                self.errors.push(error);
            }
            let previews = dirty::PREVIEWS | dirty::INSPECTOR | dirty::STRUCTURE | dirty::CHROME | dirty::LAYOUT | dirty::PRESETS | dirty::CANVAS;
            if flags & previews != 0 && model.prefs.previews {
                model.pump_previews();
            }
        }
    }

    /// Nothing running or about to post (see the module's notes).
    pub fn quiescent(&self) -> bool {
        let m = &self.app.model;
        ui::testing::posted() == 0
            && ui::testing::background_running() == 0
            && m.engine.as_ref().is_none_or(|e| e.inflight.load(Ordering::SeqCst) == 0)
            && !m.rendering
            && !m.pending_render
            && !m.matching_brightness
            && !m.tone_curve_pending
            && !m.previews.busy
            && !m.original_loading
            && !m.saving
            && !m.balancing_print
            && !m.exporting
    }

    /// Runs tasks until `done` holds and everything is quiet, or fails
    /// after the timeout with what was still running.
    pub fn drain_until(&mut self, what: &str, done: impl Fn(&Model) -> bool) {
        let deadline = Instant::now() + timeout();
        let mut quiet_since: Option<Instant> = None;
        loop {
            if self.step() {
                quiet_since = None;
                continue;
            }
            if done(&self.app.model) && self.quiescent() {
                if quiet_since.get_or_insert_with(Instant::now).elapsed() >= QUIET {
                    return;
                }
            } else {
                quiet_since = None;
            }
            if Instant::now() > deadline {
                panic!("{what}: not idle after {:?}\n{}", timeout(), self.describe());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Runs tasks until the lifecycle is idle.
    pub fn idle(&mut self) {
        self.drain_until("idle", |_| true);
    }

    /// Runs tasks until `done` holds, without waiting for quiet: for
    /// stopping in the middle of the lifecycle.
    pub fn run_until(&mut self, what: &str, done: impl Fn(&Model) -> bool) {
        let deadline = Instant::now() + timeout();
        while !done(&self.app.model) {
            if !self.step() {
                assert!(Instant::now() < deadline, "{what}: not reached after {:?}\n{}", timeout(), self.describe());
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    pub fn renders(&self) -> &[RenderRequest] {
        &self.app.model.render_log
    }

    /// Every helper command so far, in the order sent, with its `id`.
    pub fn commands(&self) -> Vec<Value> {
        self.app.model.engine.as_ref().map(|e| e.commands.lock().unwrap().clone()).unwrap_or_default()
    }

    /// The commands' names, in order.
    pub fn command_names(&self) -> Vec<String> {
        self.commands().iter().map(|c| c["command"].as_str().unwrap_or_default().to_owned()).collect()
    }

    /// Forgets the logs, so a test can look at what one step did.
    pub fn clear_logs(&mut self) {
        self.app.model.render_log.clear();
        self.app.model.frame_log.clear();
        self.frames.clear();
        self.errors.clear();
        if let Some(engine) = &self.app.model.engine {
            engine.commands.lock().unwrap().clear();
        }
    }

    fn describe(&self) -> String {
        let m = &self.app.model;
        format!(
            "posted {} background {} in flight {:?} rendering {} pending {} matching {} match pending {} preview current {} \
             tone curve {} thumbnail {} revision {} shown {:?}\nrenders {:?}\ncommands {:?}\nerrors {:?}",
            ui::testing::posted(),
            ui::testing::background_running(),
            m.engine.as_ref().map(|e| e.inflight.load(Ordering::SeqCst)),
            m.rendering,
            m.pending_render,
            m.matching_brightness,
            m.brightness_match_pending(),
            m.preview_current,
            m.tone_curve_pending,
            m.previews.busy,
            m.revision,
            m.shown_frame,
            m.render_log,
            self.command_names(),
            self.errors,
        )
    }
}

/// Preferences for a lifecycle test: no thumbnails, the match as given.
pub fn prefs(match_new_photos: bool) -> Preferences {
    Preferences { previews: false, match_new_photos, ..Preferences::default() }
}

/// The deterministic test photograph, 3072 × 2048 (6.3 MP, over the 4.5 MP
/// a quick frame needs), written once to the temporary folder: soft
/// gradients and a few patches, so the brightness match has tones to read.
pub fn test_photo() -> PathBuf {
    const SIZE: (u32, u32) = (3072, 2048);
    let path = std::env::temp_dir().join(format!("negs-harness-photo-{}x{}-v1.png", SIZE.0, SIZE.1));
    if image::image_dimensions(&path).is_ok_and(|size| size == SIZE) {
        return path;
    }
    let (w, h) = SIZE;
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f64 / w as f64, y as f64 / h as f64);
            let patch = ((x / 384) + (y / 384)) % 3;
            let base = 0.12 + 0.6 * u * (1.0 - 0.4 * v);
            let (r, g, b) = match patch {
                0 => (base * 1.1, base * 0.95, base * 0.8),
                1 => (base * 0.85, base, base * 1.05),
                _ => (base, base * 0.9, base * 0.95),
            };
            let shade = |c: f64| ((c + 0.05 * (u * 37.0).sin() * (v * 23.0).cos()).clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.extend_from_slice(&[shade(r), shade(g), shade(b)]);
        }
    }
    let staging = path.with_extension(format!("{}.tmp", std::process::id()));
    let file = std::io::BufWriter::new(std::fs::File::create(&staging).unwrap());
    let encoder = image::codecs::png::PngEncoder::new_with_quality(file, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Sub);
    image::ImageEncoder::write_image(encoder, &pixels, w, h, image::ExtendedColorType::Rgb8).unwrap();
    // Another process may have written it meanwhile: either copy is the same.
    if std::fs::rename(&staging, &path).is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    path
}
