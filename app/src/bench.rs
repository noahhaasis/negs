//! `NEGS_FRAME_BENCH=<out.json>`: the app drives itself through the
//! library's motion and writes how its frames went, then quits. Open a
//! folder (the photograph or folder argument) of 60 or more photographs:
//!
//! 1. waits for the library and the strip's thumbnails to settle;
//! 2. marks a few photographs (on a scratch copy only!) so the strip and
//!    grid draw stars, picks, rejects and a selection;
//! 3. **strip**: flings the strip back and forth for 4 s;
//! 4. **flight**: opens and folds the grid five times (`SheetFlight`);
//! 5. **grid**: flings the open grid down and up for 6 s (four flings each
//!    way from 8000 DIP/s, as `windows/src/ui/grid_bench.rs`);
//!
//! each with every frame asked for, and writes per phase the frame
//! intervals' and the render passes' median, p95, p99 and max (ms) and the
//! frames over 1.5 refresh intervals of 60 Hz.
//!
//! `NEGS_BENCH_TOUR=print` instead walks the photograph view for its
//! pictures (no timings): the print on its sheet in three themes, a black
//! border on a black sheet (the light table), cropping and straightening,
//! Image Only and upside down.
//!
//! `NEGS_BENCH_SHOTS=<dir>`: at named moments (the strip, the grid with
//! its marks and selection, filtered, halfway through an opening and a
//! fold held still, other themes) it writes `<dir>/<name>.ready` and holds
//! until `<name>.done` appears (or 6 s), for a script outside to capture
//! the window.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Context, Window};
use negs_app::model::{LibraryKey, dirty};
use negs_app::theme::ThemeName;
use negs_library::{Filter, FlagFilter, Item};

use crate::NegsApp;
use crate::grid;

#[derive(Clone, Debug)]
enum Step {
    WaitLoaded,
    /// Until nothing is read or developed for a second (at most `secs`).
    WaitThumbs(f64),
    Mark,
    Shot(&'static str),
    StripFling(f64),
    /// The strip back on the open photograph (where the fling left it is
    /// nowhere near the grid's cells, so nothing would fly).
    CentreStrip,
    OpenGrid(Option<&'static str>),
    CloseGrid(Option<&'static str>),
    WaitFlight,
    /// Holds a flight at `fraction` of its run.
    Freeze(f64),
    Unfreeze,
    Hold(u64),
    Select,
    Filtered(bool),
    Theme(ThemeName),
    GridFling(f64),
    /// Until the open photograph is developed and nothing's updating.
    WaitDeveloped,
    /// The viewer sheet, by its stored key ("" for the theme's own).
    Sheet(&'static str),
    /// A black border, or none.
    BlackBorder(bool),
    /// Begin cropping, or end it (applying or not).
    Crop(Option<bool>),
    Straighten(f64),
    ImageOnly(bool),
    UpsideDown(bool),
    /// Compares the first two photographs (or the open one with its
    /// original), side by side or as a slider; None leaves.
    Compare(Option<(bool, negs_app::compare::Layout)>),
    ZoomActual(bool),
    Write,
}

pub struct Bench {
    out: PathBuf,
    shots: Option<PathBuf>,
    steps: Vec<Step>,
    at: usize,
    step_started: Instant,
    step_frames: u32,
    /// The phase frames are being measured for, and the last frame's time.
    phase: Option<&'static str>,
    last_frame: Option<Instant>,
    frame_started: Option<Instant>,
    intervals: BTreeMap<&'static str, Vec<f64>>,
    builds: BTreeMap<&'static str, Vec<f64>>,
    quiet_since: Option<Instant>,
    started: Instant,
    notes: Vec<String>,
    theme_before: Option<ThemeName>,
    /// The strip's scroll as a fling began.
    origin: f64,
}

impl Bench {
    pub fn from_env() -> Option<Bench> {
        let out = PathBuf::from(std::env::var_os("NEGS_FRAME_BENCH")?);
        let shots = std::env::var_os("NEGS_BENCH_SHOTS").map(PathBuf::from);
        if let Some(dir) = &shots {
            let _ = std::fs::create_dir_all(dir);
        }
        use Step::*;
        if std::env::var("NEGS_BENCH_TOUR").is_ok_and(|t| t == "print") {
            let mut steps = vec![WaitLoaded, WaitDeveloped, Hold(600), Shot("print-darkroom")];
            steps.extend([Theme(ThemeName::LightTable), Hold(400), Shot("print-light-table"), Theme(ThemeName::PlainDark), Hold(400), Shot("print-plain-dark")]);
            steps.extend([Theme(ThemeName::Darkroom), Sheet("black"), BlackBorder(true), WaitDeveloped, Hold(600), Shot("print-black-border-on-black")]);
            steps.extend([BlackBorder(false), Sheet(""), WaitDeveloped, Crop(None), Hold(800), Shot("crop"), Straighten(4.5), Hold(800), Shot("crop-straighten"), Crop(Some(false)), WaitDeveloped]);
            steps.extend([ImageOnly(true), Hold(500), Shot("image-only"), ImageOnly(false), UpsideDown(true), Hold(500), Shot("upside-down"), UpsideDown(false), Hold(200)]);
            use negs_app::compare::Layout;
            steps.extend([Compare(Some((false, Layout::SideBySide))), Hold(2500), Shot("compare-side-by-side"), Compare(None), Compare(Some((false, Layout::Slider))), Hold(1500), Shot("compare-slider")]);
            steps.extend([ZoomActual(true), Hold(1500), Shot("compare-slider-100"), ZoomActual(false), Compare(None), Compare(Some((true, Layout::SideBySide))), Hold(3000), Shot("compare-original"), Compare(None), Hold(300), Write]);
            if shots.is_none() {
                steps.retain(|s| !matches!(s, Shot(_)));
            }
            return Some(Bench::with_steps(out, shots, steps));
        }
        let mut steps = vec![WaitLoaded, WaitThumbs(150.0), Mark, Hold(400), Shot("strip"), StripFling(4.0), CentreStrip, Hold(300)];
        // A first opening to warm the grid's thumbnails, then settle.
        steps.extend([OpenGrid(None), WaitFlight, WaitThumbs(150.0), CloseGrid(None), WaitFlight, Hold(500)]);
        for _ in 0..5 {
            steps.extend([OpenGrid(Some("flight_open")), WaitFlight, Hold(250), CloseGrid(Some("flight_fold")), WaitFlight, Hold(250)]);
        }
        steps.extend([OpenGrid(None), WaitFlight, Select, Hold(400), Shot("grid"), GridFling(6.0), Hold(300)]);
        steps.extend([Filtered(true), Hold(400), Shot("grid-filtered"), Filtered(false), Hold(200)]);
        steps.extend([Theme(ThemeName::ContactSheet), Hold(400), Shot("grid-contact-sheet"), Theme(ThemeName::LightTable), Hold(400), Shot("grid-light-table")]);
        steps.extend([Theme(ThemeName::Darkroom), Hold(300), CloseGrid(None), Freeze(0.45), Hold(300), Shot("fold-midflight"), Unfreeze, WaitFlight, Hold(300)]);
        steps.extend([OpenGrid(None), Freeze(0.4), Hold(300), Shot("open-midflight"), Unfreeze, WaitFlight, Hold(200), CloseGrid(None), WaitFlight, Hold(300), Write]);
        if shots.is_none() {
            steps.retain(|s| !matches!(s, Shot(_)));
        }
        Some(Bench::with_steps(out, shots, steps))
    }

    fn with_steps(out: PathBuf, shots: Option<PathBuf>, steps: Vec<Step>) -> Bench {
        let now = Instant::now();
        Bench {
            out,
            shots,
            steps,
            at: 0,
            step_started: now,
            step_frames: 0,
            phase: None,
            last_frame: None,
            frame_started: None,
            intervals: BTreeMap::new(),
            builds: BTreeMap::new(),
            quiet_since: None,
            started: now,
            notes: vec![],
            theme_before: None,
            origin: 0.0,
        }
    }

    /// A render pass starts: its interval from the last one, if measured.
    pub fn frame_start(&mut self) {
        let now = Instant::now();
        if let (Some(phase), Some(last)) = (self.phase, self.last_frame) {
            self.intervals.entry(phase).or_default().push((now - last).as_secs_f64() * 1000.0);
        }
        self.last_frame = self.phase.map(|_| now);
        self.frame_started = Some(now);
    }

    /// The render pass is built.
    pub fn frame_end(&mut self) {
        if let (Some(phase), Some(start)) = (self.phase, self.frame_started.take()) {
            self.builds.entry(phase).or_default().push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn measure(&mut self, phase: Option<&'static str>) {
        self.phase = phase;
        self.last_frame = phase.map(|_| Instant::now());
    }

    fn next(&mut self) {
        self.at += 1;
        self.step_started = Instant::now();
        self.step_frames = 0;
        self.quiet_since = None;
    }

    /// Runs the current step; called at the start of every render.
    pub fn tick(&mut self, app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) {
        window.request_animation_frame();
        self.step_frames += 1;
        let in_step = self.step_started.elapsed();
        let Some(step) = self.steps.get(self.at).cloned() else { return };
        if self.step_frames % 120 == 1 {
            eprintln!(
                "bench: step {} {:?} frame {} library {} source {} first_render {} failed {} strip {} outstanding {} in_grid {}",
                self.at, step, self.step_frames, app.model.shelf.library.is_some(), app.model.source.is_some(), app.model.first_render_shown,
                app.model.render_failed, app.strip.cells.len(), app.thumbs.outstanding(), app.model.shelf.in_grid
            );
        }
        if self.started.elapsed() > Duration::from_secs(600) {
            self.notes.push(format!("timed out at step {} ({step:?})", self.at));
            return self.write(app, cx);
        }
        match step {
            Step::WaitLoaded => {
                if app.model.shelf.library.is_some() && app.model.source.is_some() && (app.model.first_render_shown || app.model.render_failed) && !app.strip.cells.is_empty() {
                    self.notes.push(format!("loaded in {:.1} s, {} photographs", self.started.elapsed().as_secs_f64(), app.strip.cells.len()));
                    self.next();
                }
            }
            Step::WaitThumbs(max) => {
                let quiet = app.thumbs.outstanding() == 0;
                match (quiet, self.quiet_since) {
                    (true, None) => self.quiet_since = Some(Instant::now()),
                    (false, _) => self.quiet_since = None,
                    _ => {}
                }
                let settled = self.quiet_since.is_some_and(|t| t.elapsed() > Duration::from_secs(1));
                if settled || in_step.as_secs_f64() > max {
                    self.notes.push(format!("thumbnails {} after {:.1} s", if settled { "settled" } else { "still coming" }, in_step.as_secs_f64()));
                    self.next();
                }
            }
            Step::Mark => {
                mark(app);
                app.after_model_update(cx);
                self.next();
            }
            Step::Select => {
                let items: Vec<Item> = app.model.shown_items();
                let open = app.model.library_item();
                let selection = &mut app.model.shelf.selection;
                if let Some(open) = open {
                    selection.select(&open);
                }
                for i in [8usize, 9, 10] {
                    if let Some(item) = items.get(i) {
                        selection.toggle(item, Some(&items));
                    }
                }
                app.model.dirty |= dirty::STRIP | dirty::CHROME;
                app.after_model_update(cx);
                self.next();
            }
            Step::Shot(name) => {
                let Some(dir) = self.shots.clone() else { return self.next() };
                // A few frames on screen first.
                if self.step_frames == 4 {
                    let _ = std::fs::write(dir.join(format!("{name}.ready")), b"");
                }
                if dir.join(format!("{name}.done")).exists() || in_step > Duration::from_secs(6) {
                    if in_step > Duration::from_secs(6) {
                        self.notes.push(format!("shot {name} not taken"));
                    }
                    self.next();
                }
            }
            Step::StripFling(secs) => {
                if self.step_frames == 1 {
                    self.origin = app.strip.scroll;
                    self.measure(Some("strip"));
                }
                let t = in_step.as_secs_f64();
                // Back and forth over the row at 2400 DIP/s.
                let span = (app.strip.layout.width - app.strip.visible_width()).max(1.0);
                let travel = (2400.0 * t) % (2.0 * span);
                let x = if travel < span { travel } else { 2.0 * span - travel };
                app.strip.set_scroll(x);
                crate::strip::request_now(app);
                if t >= secs {
                    self.measure(None);
                    self.next();
                }
            }
            Step::CentreStrip => {
                crate::strip::centre_open(app);
                crate::strip::request_now(app);
                self.next();
            }
            Step::OpenGrid(phase) => {
                grid::open(app);
                app.after_model_update(cx);
                self.measure(phase);
                self.next();
            }
            Step::CloseGrid(phase) => {
                grid::close(app, true);
                app.after_model_update(cx);
                self.measure(phase);
                self.next();
            }
            Step::WaitFlight => {
                if app.grid.flight.is_none() {
                    self.measure(None);
                    self.next();
                }
            }
            Step::Freeze(fraction) => {
                if let Some(f) = app.grid.flight.as_mut() {
                    f.freeze(Some(fraction * grid::DURATION));
                }
                self.next();
            }
            Step::Unfreeze => {
                if let Some(f) = app.grid.flight.as_mut() {
                    f.freeze(None);
                }
                self.next();
            }
            Step::Hold(ms) => {
                if in_step >= Duration::from_millis(ms) {
                    self.next();
                }
            }
            Step::Filtered(on) => {
                let mut filter = Filter::default();
                if on {
                    filter.toggle_flag(FlagFilter::Pick);
                    filter.toggle_flag(FlagFilter::Unflagged);
                }
                app.model.set_library_filter(filter);
                app.after_model_update(cx);
                self.next();
            }
            Step::Theme(name) => {
                self.theme_before.get_or_insert(app.theme.name);
                app.theme = crate::theme_gpui::GpuiTheme::from_name(name);
                self.next();
            }
            Step::GridFling(secs) => {
                if self.step_frames == 1 {
                    self.measure(Some("grid"));
                }
                let t = in_step.as_secs_f64();
                let max = (app.grid.layout.height - app.grid.viewport()).max(0.0);
                app.grid.set_scroll(flung(t, max));
                grid::request_now(app);
                if t >= secs {
                    self.measure(None);
                    self.next();
                }
            }
            Step::WaitDeveloped => {
                if (app.model.image.is_some() && app.model.badge().is_none() && in_step > Duration::from_millis(300)) || app.model.render_failed || in_step > Duration::from_secs(60) {
                    self.next();
                }
            }
            Step::Sheet(key) => {
                app.model.prefs.viewer_sheet = key.to_string();
                app.model.dirty |= dirty::CANVAS;
                self.next();
            }
            Step::BlackBorder(on) => {
                app.model.set_border(
                    |b| {
                        b.enabled = on;
                        b.style = negs_app::border::BorderStyle::Solid;
                        b.color = negs_app::border::BorderColor::BLACK;
                    },
                    "Border",
                );
                app.after_model_update(cx);
                self.next();
            }
            Step::Crop(end) => {
                match end {
                    None => app.model.begin_crop(),
                    Some(apply) => app.model.end_crop(apply),
                }
                app.after_model_update(cx);
                self.next();
            }
            Step::Straighten(angle) => {
                if let Some(s) = app.model.crop_session.as_mut() {
                    s.begin_straightening();
                    s.straighten(angle);
                    s.end_straightening();
                }
                app.model.crop_changed();
                self.next();
            }
            Step::ImageOnly(on) => {
                app.canvas.image_only = on;
                self.next();
            }
            Step::UpsideDown(on) => {
                app.canvas.upside_down = on;
                self.next();
            }
            Step::Compare(how) => {
                match how {
                    None => crate::compare_view::leave(app),
                    Some((true, layout)) => app.model.compare_with_original(layout),
                    Some((false, layout)) => {
                        let items = app.model.shown_items();
                        if let (Some(a), Some(b)) = (items.first(), items.get(1)) {
                            app.model.compare(a.clone(), b.clone(), negs_app::compare::Origin::Photo);
                            app.model.set_compare_layout(layout);
                        }
                    }
                }
                app.after_model_update(cx);
                self.next();
            }
            Step::ZoomActual(on) => {
                app.model.zoom(!on);
                app.after_model_update(cx);
                self.next();
            }
            Step::Write => self.write(app, cx),
        }
    }

    fn write(&mut self, app: &mut NegsApp, cx: &mut Context<NegsApp>) {
        if let Some(theme) = self.theme_before.take() {
            app.theme = crate::theme_gpui::GpuiTheme::from_name(theme);
        }
        let refresh = 1000.0 / 60.0;
        let mut phases = serde_json::Map::new();
        for (phase, values) in &self.intervals {
            let over = values.iter().filter(|v| **v > refresh * 1.5).count();
            let mut v = values.clone();
            let mut b = self.builds.get(phase).cloned().unwrap_or_default();
            phases.insert(
                phase.to_string(),
                serde_json::json!({
                    "frames": v.len(),
                    "interval_ms": summary(&mut v),
                    "render_ms": summary(&mut b),
                    "over_25ms": over,
                    "fps": if values.is_empty() { 0.0 } else { 1000.0 * values.len() as f64 / values.iter().sum::<f64>() },
                }),
            );
        }
        let report = serde_json::json!({
            "os": std::env::consts::OS,
            "photographs": app.strip.cells.len(),
            "grid_viewport_and_strip_width": [app.grid.viewport(), app.strip.visible_width()],
            "scale": f64::from(app.canvas.scale_factor),
            "thumbs": {"read": app.thumbs.stats.read, "developed": app.thumbs.stats.developed, "filed": app.thumbs.stats.filed, "dropped_stale": app.thumbs.stats.dropped_stale},
            "mark_sprites_made": app.sprites.made,
            "notes": self.notes,
            "phases": phases,
        });
        let _ = std::fs::write(&self.out, serde_json::to_string_pretty(&report).unwrap_or_default());
        self.at = self.steps.len();
        app.shutdown_cleanly();
        cx.quit();
    }
}

fn summary(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values.get(((values.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
    serde_json::json!({"median": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": at(1.0)})
}

/// Where the grid is scrolled `t` seconds in: four flings down, four back
/// up, each from 8000 DIP/s easing off as a touchpad's inertia does.
fn flung(t: f64, max: f64) -> f64 {
    const FLING: f64 = 0.75;
    let tau = 0.35;
    let distance = |dt: f64| 8000.0 * tau * (1.0 - (-dt / tau).exp());
    let mut y: f64 = 0.0;
    let mut start = 0.0;
    for fling in 0..8 {
        let dt = (t - start).clamp(0.0, FLING);
        let d = distance(dt);
        let next = if fling < 4 { (y + d).min(max) } else { (y - d).max(0.0) };
        if t < start + FLING {
            return next;
        }
        y = if fling < 4 { (y + distance(FLING)).min(max) } else { (y - distance(FLING)).max(0.0) };
        start += FLING;
    }
    y
}

/// Stars, picks, rejects on a few photographs of the (scratch!) folder.
fn mark(app: &mut NegsApp) {
    let items = app.model.shown_items();
    let in_grid = app.model.shelf.in_grid;
    app.model.shelf.in_grid = true;
    let marks: [(usize, &[LibraryKey]); 7] = [
        (0, &[LibraryKey::Rate(3)]),
        (1, &[LibraryKey::Pick]),
        (2, &[LibraryKey::Reject]),
        (3, &[LibraryKey::Rate(5), LibraryKey::Pick]),
        (5, &[LibraryKey::Rate(2)]),
        (6, &[LibraryKey::Rate(4), LibraryKey::Pick]),
        (7, &[LibraryKey::Reject]),
    ];
    let advance = app.model.prefs.library.advance_after_rating;
    app.model.prefs.library.advance_after_rating = false;
    for (index, keys) in marks {
        let Some(item) = items.get(index) else { continue };
        app.model.shelf.selection.select(item);
        for key in keys {
            app.model.library_key(*key);
        }
    }
    app.model.prefs.library.advance_after_rating = advance;
    app.model.shelf.in_grid = in_grid;
    if let Some(open) = app.model.library_item() {
        app.model.shelf.selection.select(&open);
    }
    app.model.dirty |= dirty::STRIP | dirty::CHROME;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flings_go_down_then_back_up_within_the_sheet() {
        let max = 5000.0;
        assert_eq!(flung(0.0, max), 0.0);
        let mut last = 0.0;
        for i in 1..=30 {
            let y = flung(i as f64 * 0.1, max);
            assert!((0.0..=max).contains(&y));
            if i as f64 * 0.1 < 3.0 {
                assert!(y >= last - 1e-9, "down: {y} after {last}");
            }
            last = y;
        }
        assert_eq!(flung(1.6, max), max, "reaches the end");
        assert!(flung(6.0, max) < 1.0, "and comes back up");
    }
}
