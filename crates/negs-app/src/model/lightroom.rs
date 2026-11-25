//! Open in Lightroom (ticket 48), the editor's part (the Mac's
//! `extension EditorModel` in `Lightroom.swift`): a batch export to 16-bit
//! TIFFs, handed to Lightroom in one launch when it ends; detection and the
//! cloud folder's cleanup, never on the launch path.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use super::{Model, SaveSettings, dirty};
use crate::batch::{Existing, Naming, Options};
use crate::model::ExportSize;
use crate::lightroom::{self, Apps, Cleanup, Kind};
use crate::model::{COLOR_SPACES, OutputFormat};
use crate::host;

/// After a send, Lightroom's copies are looked for this long after it,
/// as long as TIFFs wait: Add is clicked, then the copy takes a moment.
const CLEANUP_STEPS: [u64; 6] = [20, 60, 120, 300, 600, 1800];

/// The Lightroom side of the model.
#[derive(Default)]
pub struct State {
    /// What the last detection found (remembered in the settings, so the
    /// menus are right before it runs).
    pub apps: Apps,
    detected: Option<Instant>,
    /// "Sending 3 photographs to Lightroom…", "Starting Lightroom…": in
    /// the status line and the grid's foot while it lasts.
    pub words: Option<String>,
    /// Cloud TIFFs being renamed before they're handed over.
    handing_over: bool,
    /// The running cleanup schedule's number; another one replaces it.
    cleanup: u64,
}

impl State {
    pub fn remembered(prefs: &crate::session::Preferences) -> State {
        let apps = Apps { cloud: prefs.lightroom_cloud.clone(), classic: prefs.lightroom_classic.clone() };
        State { apps, ..State::default() }
    }
}

impl Model {
    /// The Lightroom items to offer, in menu order: those installed.
    pub fn lightroom_kinds(&self) -> Vec<Kind> {
        self.lightroom.apps.kinds()
    }

    /// Whether `targets` can go to Lightroom now: as a batch export could start.
    pub fn can_open_in_lightroom(&self, kind: Kind) -> bool {
        self.lightroom.apps.app(kind).is_some() && !self.export_targets().is_empty() && self.can_start_batch()
    }

    /// Develops what Export acts on to full-size 16-bit TIFFs in the export
    /// settings' colour space, with their EXIF, profile and rating, then
    /// opens them all in Lightroom at once.
    pub fn open_in_lightroom(&mut self, kind: Kind) {
        if !self.can_open_in_lightroom(kind) {
            return;
        }
        let folder = kind.folder();
        if let Err(e) = std::fs::create_dir_all(&folder) {
            self.error = Some(format!("Couldn’t create the folder for {}: {e}", kind.title()));
            self.mark(dirty::ERROR);
            return;
        }
        let targets = self.export_targets();
        let options = self.lightroom_options(kind);
        self.export_batch(targets, folder, options, None);
    }

    /// Full size, TIFF 16-bit in the export settings' colour space, the
    /// export engine last chosen, `IMG_0412-Negs.tif` numbered if taken.
    pub fn lightroom_options(&self, kind: Kind) -> Options {
        let chosen = COLOR_SPACES.iter().position(|s| *s == self.prefs.color_space).unwrap_or(0);
        Options {
            format: OutputFormat::Tiff.key().into(),
            engine: crate::engine::ExportEngine::from_key(&self.prefs.export_engine).key().into(),
            edge: ExportSize::FULL.edge.key().into(),
            pixels: ExportSize::FULL.pixels,
            bit_depth: 16,
            color_space: SaveSettings::effective_space(chosen, 16),
            jpeg_quality: self.prefs.jpeg_quality,
            naming: Naming::Custom,
            custom: lightroom::NAMING.into(),
            existing: Existing::Number,
            low_memory: self.prefs.low_memory_export,
            lightroom: Some(kind.key().into()),
        }
    }

    /// The batch has ended: what it wrote goes to Lightroom in one launch.
    pub(super) fn lightroom_batch_finished(&mut self, kind: Kind, written: Vec<PathBuf>, failed: usize, cancelled: bool) {
        let photographs = if written.len() == 1 { "1 photograph".to_string() } else { format!("{} photographs", written.len()) };
        let failures = if failed > 0 { format!("; {failed} failed") } else { String::new() };
        if written.is_empty() || cancelled {
            self.lightroom.words = None;
            self.status = if cancelled { format!("Sending to {} cancelled", kind.title()) } else { format!("Nothing sent to {}{failures}", kind.title()) };
            if kind == Kind::Cloud {
                self.schedule_lightroom_cleanup();
            }
            return self.mark(dirty::STATUS | dirty::STRIP);
        }
        let Some(app) = self.lightroom.apps.app(kind).map(PathBuf::from) else {
            self.status = format!("{} isn’t installed any more", kind.title());
            return self.mark(dirty::STATUS);
        };
        self.status = format!("Sending {photographs} to {}…", kind.title());
        self.lightroom.words = Some(self.status.clone());
        self.lightroom.handing_over = kind == Kind::Cloud;
        self.mark(dirty::STATUS | dirty::STRIP);
        let sent = format!("Sent {photographs} to {}{failures}", kind.title());
        #[cfg(test)]
        let background = host::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            // Named so Lightroom's library can't already have them, which
            // is what tells cleanup that Lightroom took its copy.
            let files: Vec<PathBuf> = if kind == Kind::Cloud {
                let libraries = lightroom::all_libraries();
                written.iter().map(|f| lightroom::unique_in_library(f, &libraries)).collect()
            } else {
                written
            };
            host::post(|model: &mut Model| model.lightroom.handing_over = false);
            let starting = move || host::post(move |model: &mut Model| model.lightroom_words(Some(format!("Starting {}…", kind.title()))));
            let opened = hand_over(&files, &app, starting);
            host::post(move |model: &mut Model| model.lightroom_handed_over(kind, opened, sent));
        });
    }

    fn lightroom_handed_over(&mut self, kind: Kind, opened: Result<(), String>, sent: String) {
        match opened {
            Ok(()) => {
                self.status = sent.clone();
                self.lightroom_words(Some(sent.clone()));
                host::after(Duration::from_secs(4), move |model: &mut Model| {
                    if model.lightroom.words.as_deref() == Some(&sent) {
                        model.lightroom_words(None);
                    }
                });
            }
            Err(e) => {
                self.lightroom_words(None);
                self.status = String::new();
                self.error = Some(format!("Couldn’t open {}: {e}", kind.title()));
                self.mark(dirty::ERROR);
            }
        }
        if kind == Kind::Cloud {
            self.schedule_lightroom_cleanup();
        }
    }

    fn lightroom_words(&mut self, words: Option<String>) {
        self.lightroom.words = words;
        self.mark(dirty::STATUS | dirty::STRIP);
    }

    // MARK: Detection and cleanup, never on the launch path

    /// Cloud TIFFs are being written or renamed. A photograph sent again
    /// keeps the name and size of Lightroom's earlier copy until it's
    /// renamed, so cleanup would take it for copied: it waits.
    pub fn lightroom_sending(&self) -> bool {
        self.lightroom.handing_over
            || self.batch.as_ref().is_some_and(|b| b.running && b.journal.options.lightroom.as_deref() == Some(Kind::Cloud.key()))
    }

    /// After the first window: which Lightroom is installed, and the cloud
    /// TIFFs Lightroom has copied since. Again when Negs becomes active
    /// (Lightroom installed meanwhile, or photos added there), at most
    /// every 10 s.
    pub fn refresh_lightroom(&mut self, delay: Duration) {
        if self.lightroom.detected.is_some_and(|last| last.elapsed() < Duration::from_secs(10)) {
            return;
        }
        self.lightroom.detected = Some(Instant::now());
        let sending = self.lightroom_sending();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let apps = lightroom::detect();
            let cleanup = if sending { Cleanup::default() } else { lightroom::clean_up(&Kind::Cloud.folder(), None, SystemTime::now()) };
            host::post(move |model: &mut Model| model.lightroom_found(apps, cleanup));
        });
    }

    /// Whether Negs was active when it last looked: `refresh_lightroom`
    /// again on activation only once it has looked after launch.
    pub fn lightroom_detected(&self) -> bool {
        self.lightroom.detected.is_some()
    }

    fn lightroom_found(&mut self, apps: Apps, cleanup: Cleanup) {
        crate::trace::event("lightroom_detected", || {
            serde_json::json!({"cloud": apps.cloud.is_some(), "classic": apps.classic.is_some(), "copied": cleanup.copied, "expired": cleanup.expired, "waiting": cleanup.waiting})
        });
        if apps != self.lightroom.apps {
            self.lightroom.apps = apps.clone();
            self.mark(dirty::CHROME);
        }
        if self.prefs.lightroom_cloud != apps.cloud || self.prefs.lightroom_classic != apps.classic {
            self.prefs.lightroom_cloud = apps.cloud;
            self.prefs.lightroom_classic = apps.classic;
            self.prefs.save();
        }
        if cleanup.waiting > 0 && self.lightroom.cleanup == 0 {
            self.schedule_lightroom_cleanup();
        }
    }

    /// Looks for Lightroom's copies a few times after a send, as long as
    /// TIFFs wait.
    pub fn schedule_lightroom_cleanup(&mut self) {
        self.lightroom.cleanup += 1;
        let schedule = self.lightroom.cleanup;
        cleanup_step(schedule, 0);
    }
}

/// The `step`th look of cleanup schedule `schedule`, unless a later one replaced it.
fn cleanup_step(schedule: u64, step: usize) {
    let Some(&seconds) = CLEANUP_STEPS.get(step) else {
        return host::post(move |model: &mut Model| {
            if model.lightroom.cleanup == schedule {
                model.lightroom.cleanup = 0;
            }
        });
    };
    host::after(Duration::from_secs(seconds), move |model: &mut Model| {
        if model.lightroom.cleanup != schedule || model.closing {
            return;
        }
        if model.lightroom_sending() {
            return cleanup_step(schedule, step + 1);
        }
        std::thread::spawn(move || {
            let cleanup = lightroom::clean_up(&Kind::Cloud.folder(), None, SystemTime::now());
            host::post(move |model: &mut Model| {
                if model.lightroom.cleanup != schedule {
                    return;
                }
                if cleanup.waiting == 0 {
                    model.lightroom.cleanup = 0;
                } else {
                    cleanup_step(schedule, step + 1);
                }
            });
        });
    });
}

/// Lightroom's launch, or under test what it was given (`HANDED`).
#[cfg(not(test))]
fn hand_over(files: &[PathBuf], app: &std::path::Path, starting: impl FnOnce()) -> Result<(), String> {
    lightroom::open(files, app, starting).map_err(|e| e.to_string())
}

/// Under test nothing is launched: the files are kept for the test to read.
#[cfg(test)]
pub static HANDED: std::sync::Mutex<Vec<(PathBuf, Vec<PathBuf>)>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn hand_over(files: &[PathBuf], app: &std::path::Path, _starting: impl FnOnce()) -> Result<(), String> {
    HANDED.lock().unwrap().push((app.to_path_buf(), files.to_vec()));
    Ok(())
}
