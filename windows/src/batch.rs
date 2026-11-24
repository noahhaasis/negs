//! Batch export (the Mac's `BatchExport.swift`): Export from the grid's
//! selection develops each photograph with its own edits, one after
//! another, into a folder. Its choices, the names it gives, the journal an
//! unfinished batch leaves for the next launch, and the words its status
//! line and popover say. The model runs it (`model/batch.rs`); the dialog
//! and the popover are `ui/dialogs.rs` and `ui/batch.rs`.
//!
//! The shared core's (core/src/caches/batch, through negs_core_rules): the
//! names, the matched exposure, the words and the journal's file run in
//! Odin (core/diff/tests/batch.rs). Options, Item, Journal and Run stay
//! here with their serde, as they hold this crate's EditState and model
//! types.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::ExportEngine;
use crate::model::{Edge, ExportSize, OutputFormat, SaveSettings};
use crate::state::{self, EditState};

use negs_core_rules::batch as core;
pub use negs_core_rules::batch::{Existing, Naming, Status, batch_name, exporting, lightroom_title, numbered, unique};

/// The batch dialog's choices, as the journal keeps them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    pub format: String,
    pub engine: String,
    pub edge: String,
    pub pixels: u32,
    pub bit_depth: u32,
    pub color_space: usize,
    pub jpeg_quality: u32,
    pub naming: Naming,
    pub custom: String,
    pub existing: Existing,
    pub low_memory: bool,
    /// Handed to Lightroom when the batch ends (ticket 48): its key.
    pub lightroom: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format: OutputFormat::Jpeg.key().into(),
            engine: ExportEngine::Reference.key().into(),
            edge: Edge::Full.key().into(),
            pixels: ExportSize::FULL.pixels,
            bit_depth: 8,
            color_space: 0,
            jpeg_quality: 95,
            naming: Naming::NameFilm,
            custom: "{name}_{film}".into(),
            existing: Existing::Number,
            low_memory: false,
            lightroom: None,
        }
    }
}

impl Options {
    pub fn output_format(&self) -> OutputFormat {
        OutputFormat::from_key(&self.format)
    }
    pub fn export_engine(&self) -> ExportEngine {
        ExportEngine::from_key(&self.engine)
    }
    pub fn size(&self) -> ExportSize {
        ExportSize { edge: Edge::from_key(&self.edge), pixels: self.pixels }
    }
    pub fn save(&self) -> SaveSettings {
        let s = core::save(&self.format, self.bit_depth, self.color_space, self.jpeg_quality);
        SaveSettings { bit_depth: s.bit_depth, color_space: s.color_space, jpeg_quality: s.jpeg_quality }
    }

    /// The file name for a photograph named `name` (without its extension,
    /// a duplicate's number included) developed with `film`. The extension
    /// is Windows' own for the format, as a single export's.
    pub fn file_name(&self, name: &str, film: &str) -> String {
        core::file_name(self.naming, &self.custom, name, film, self.output_format().key())
    }
}

/// One photograph of a batch: its edits as they were when Export was
/// chosen, and how it went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub photo: PathBuf,
    /// A duplicate's variant.
    pub variant: Option<String>,
    /// As the strip names it: `IMG_0412.CR3 (2)`.
    pub title: String,
    pub state: EditState,
    /// Its Match camera brightness is solved just before it exports.
    pub needs_match: bool,
    /// A match solved for it is stored with its edits (it had edits).
    pub stores_match: bool,
    /// `xmp:Rating`: 1–5, or −1 rejected; None writes none.
    pub rating: Option<i32>,
    /// The name it gets, before If a File Exists.
    pub file_name: String,
    /// The photograph's size before its crop, when known (the open one's):
    /// a 35mm border needs it. Otherwise read just before it exports.
    pub size: Option<(f64, f64)>,
    /// Its stored edits' look, which a match solved for them is stored with.
    pub look: Option<String>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub reason: Option<String>,
    /// Where it was written.
    #[serde(default)]
    pub written: Option<PathBuf>,
}

impl Item {
    pub fn library_item(&self) -> negs_library::Item {
        match &self.variant {
            Some(id) => negs_library::Item::duplicate(&self.photo, id.clone()),
            None => negs_library::Item::photo(&self.photo),
        }
    }
}

/// An unfinished batch, so the next launch can resume it. Written
/// atomically after each photograph; removed when the batch ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub schema: u32,
    pub destination: PathBuf,
    pub options: Options,
    pub items: Vec<Item>,
}

impl Journal {
    pub const SCHEMA: u32 = 1;

    pub fn new(destination: PathBuf, options: Options, items: Vec<Item>) -> Journal {
        Journal { schema: Self::SCHEMA, destination, options, items }
    }

    /// In the settings folder (a test profile's under tests).
    pub fn path() -> PathBuf {
        crate::session::settings_dir().join("active-export.json")
    }

    /// Photographs still to export.
    pub fn remaining(&self) -> usize {
        self.items.iter().filter(|i| i.status == Status::Waiting).count()
    }

    /// Staged as `active-export.json.tmp` and moved over the file.
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_vec(self)?;
        core::journal_write(path, &json).map_err(anyhow::Error::msg)
    }

    pub fn read(path: &Path) -> Option<Journal> {
        serde_json::from_slice(&core::journal_read(path)?).ok()
    }

    pub fn remove(path: &Path) {
        core::journal_remove(path)
    }
}

/// A match solved for `state`'s edits applied as opening applies it
/// (`Model::apply_brightness_match`): the total moves with the match,
/// keeping the user's exposure above it; a change under 0.05 EV keeps the
/// values and only marks them current. None without the match on.
pub fn matched(state: &EditState, ev: f64, limited: bool, inputs: Value) -> Option<EditState> {
    let mut matched = state.brightness_match.clone()?;
    let mut next = state.clone();
    if let Some(total) = core::matched_exposure(next.value(state::PREP_EXPOSURE), matched.ev, ev) {
        matched.ev = Some(ev);
        crate::json::set(&mut next.params, state::PREP_EXPOSURE, total);
    }
    matched.inputs = Some(inputs);
    matched.highlight_limited = limited;
    next.brightness_match = Some(matched);
    Some(next)
}

/// A batch as the status line and its popover show it: running, or
/// finished with failures until they've been seen.
#[derive(Debug)]
pub struct Run {
    pub journal: Journal,
    /// The photograph exporting now.
    pub current: Option<usize>,
    pub running: bool,
    pub cancelled: bool,
}

impl Run {
    fn view(&self) -> core::RunView {
        core::RunView::new(self.journal.items.iter().map(|i| i.status), self.running, self.cancelled)
    }
    pub fn total(&self) -> usize {
        self.journal.items.len()
    }
    /// Exported, failed or skipped.
    pub fn finished(&self) -> usize {
        self.view().counts().finished
    }
    pub fn exported(&self) -> usize {
        self.view().counts().exported
    }
    pub fn failures(&self) -> Vec<&Item> {
        self.journal.items.iter().filter(|i| i.status == Status::Failed).collect()
    }
    pub fn skipped(&self) -> usize {
        self.view().counts().skipped
    }
    pub fn fraction(&self) -> f64 {
        self.view().counts().fraction
    }
    /// The popover's rule: the share exported, then the share that failed
    /// or was skipped. A batch that all failed draws no yellow.
    pub fn rule_parts(&self) -> (f64, f64) {
        self.view().counts().rule_parts
    }
    /// Ended with something to look at: failures, or a cancel.
    pub fn needs_attention(&self) -> bool {
        self.view().counts().needs_attention
    }

    /// The popover's title.
    pub fn title(&self) -> String {
        self.view().title()
    }

    /// The status line's and the grid foot's words while it runs.
    pub fn progress_words(&self) -> String {
        exporting(self.finished(), self.total(), self.journal.options.lightroom.as_deref().map(lightroom_title))
    }

    /// "Exported 10 of 12 · cancelled", then "· 2 failed" in bold.
    pub fn outcome_words(&self) -> (String, Option<String>) {
        self.view().outcome_words()
    }

    /// The status once it ends.
    pub fn finished_words(&self) -> String {
        self.view().finished_words(&self.journal.destination)
    }
}

/// The dialog's message: what the batch does, what the engine does, then
/// the size and colour.
pub fn message(count: usize, engine: ExportEngine, size: ExportSize, save: SaveSettings, preview_space: &str) -> String {
    let save = core::Save { bit_depth: save.bit_depth, color_space: save.color_space, jpeg_quality: save.jpeg_quality };
    core::message(count, engine.key(), size.edge.key(), size.pixels, save, preview_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let mut options = Options::default();
        assert_eq!(options.file_name("IMG_0412 (2)", "kodak_gold_200"), "IMG_0412 (2)_kodak_gold_200.jpg");
        options.naming = Naming::Name;
        options.format = OutputFormat::Tiff.key().into();
        assert_eq!(options.file_name("IMG_0412", "x"), "IMG_0412.tif");
        options.naming = Naming::Custom;
        options.custom = "trip/{film}-{name}".into();
        assert_eq!(options.file_name("A", "f"), "trip-f-A.tif");
        options.custom = "a:b\\c*{name}?".into();
        assert_eq!(options.file_name("A", "f"), "a-b-c-A-.tif");
        options.custom = ".{film}".into();
        assert_eq!(options.file_name("A", "f"), "A.f.tif");
        options.custom = String::new();
        assert_eq!(options.file_name("A", "f"), "A.tif");
        assert_eq!(unique(&["a.jpg", "b.jpg", "A.jpg", "a.jpg"].map(String::from)), ["a.jpg", "b.jpg", "A 2.jpg", "a 3.jpg"]);
        assert_eq!(numbered("a.jpg", |n| ["a.jpg", "a 2.jpg"].contains(&n)), "a 3.jpg");
        let photo = Path::new(if cfg!(windows) { r"C:\p\IMG.CR3" } else { "/p/IMG.CR3" });
        assert_eq!(batch_name(photo, "IMG.CR3 (2)"), "IMG (2)");
        assert_eq!(batch_name(photo, "IMG.CR3"), "IMG");
        assert_eq!(Naming::from_key("custom"), Naming::Custom);
        assert_eq!(Existing::from_key("skip"), Existing::Skip);
        assert_eq!(Existing::from_key("?"), Existing::Number);
    }

    fn item(name: &str, status: Status) -> Item {
        Item {
            photo: PathBuf::from(name),
            variant: None,
            title: name.into(),
            state: EditState::default(),
            needs_match: false,
            stores_match: false,
            rating: None,
            file_name: name.into(),
            size: None,
            look: None,
            status,
            reason: None,
            written: None,
        }
    }

    fn run(statuses: &[Status]) -> Run {
        let items = statuses.iter().enumerate().map(|(i, s)| item(&format!("IMG_{i}.CR3"), *s)).collect();
        Run { journal: Journal::new(PathBuf::from(if cfg!(windows) { r"C:\out\Trip" } else { "/out/Trip" }), Options::default(), items), current: None, running: false, cancelled: false }
    }

    /// The Mac's `testThePopoversRuleIsYellowOnlyForWhatExported`, and the words.
    #[test]
    fn the_rule_is_yellow_only_for_what_exported() {
        let mixed = run(&[Status::Done, Status::Failed, Status::Skipped, Status::Waiting]);
        let (exported, other) = mixed.rule_parts();
        assert!((exported - 0.25).abs() < 1e-9 && (other - 0.5).abs() < 1e-9);
        let failed = run(&[Status::Failed, Status::Failed]);
        assert_eq!(failed.fraction(), 1.0);
        assert_eq!(failed.rule_parts(), (0.0, 1.0));
        assert_eq!(run(&[]).rule_parts().0, 0.0);
        assert!(failed.needs_attention());
        assert_eq!(failed.title(), "Exported 0 of 2");
        assert_eq!(failed.outcome_words(), ("Exported 0 of 2".into(), Some("2 failed".into())));
        assert_eq!(mixed.finished_words(), "Exported 1 photograph to “Trip”; 1 failed; 1 skipped");
        let mut running = run(&[Status::Done, Status::Waiting, Status::Waiting]);
        running.running = true;
        assert_eq!(running.title(), "Exporting 2 of 3");
        assert_eq!(running.progress_words(), "Exporting 2 of 3 photographs");
        assert_eq!(exporting(0, 2, Some("Lightroom")), "Sending 1 of 2 photographs to Lightroom");
        assert_eq!(exporting(0, 1, None), "Exporting 1 of 1 photograph");
        let mut one = run(&[Status::Done]);
        assert_eq!(one.title(), "Exported 1 photograph");
        one.cancelled = true;
        assert_eq!(one.title(), "Cancelled after 1 of 1");
        assert_eq!(one.finished_words(), "Export cancelled. Exported 1 photograph to “Trip”");
    }

    /// The Mac's `testBatchJournalRoundTrip`.
    #[test]
    fn the_journal_round_trips() {
        let dir = std::env::temp_dir().join(format!("negs-batch-journal-{}", std::process::id()));
        let path = dir.join("j").join("active-export.json");
        let mut state = EditState::default();
        state.params = serde_json::json!({"prep": {"lift": 5}});
        state.brightness_match = Some(Default::default());
        let mut it = item("IMG.CR3", Status::Waiting);
        it.variant = Some("v2".into());
        it.title = "IMG.CR3 (2)".into();
        it.state = state;
        it.needs_match = true;
        it.rating = Some(-1);
        it.size = Some((6000.0, 4000.0));
        it.look = Some("abc".into());
        let journal = Journal::new(dir.clone(), Options::default(), vec![it]);
        journal.write(&path).unwrap();
        assert_eq!(Journal::read(&path), Some(journal.clone()));
        assert_eq!(journal.remaining(), 1);
        Journal::remove(&path);
        assert_eq!(Journal::read(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
