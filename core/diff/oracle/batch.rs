//! Batch export (the Mac's `BatchExport.swift`): Export from the grid's
//! selection develops each photograph with its own edits, one after
//! another, into a folder. Its choices, the names it gives, the journal an
//! unfinished batch leaves for the next launch, and the words its status
//! line and popover say. The model runs it (`model/batch.rs`); the dialog
//! and the popover are `ui/dialogs.rs` and `ui/batch.rs`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::ExportEngine;
use crate::model::{Edge, ExportSize, OutputFormat, SaveSettings};
use crate::state::{self, EditState};

/// A batch file's name: `{name}_{film}` as a single export suggests,
/// `{name}`, or the user's own text with those tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Naming {
    #[default]
    NameFilm,
    Name,
    Custom,
}

impl Naming {
    pub const ALL: [Naming; 3] = [Naming::NameFilm, Naming::Name, Naming::Custom];
    pub fn title(self) -> &'static str {
        match self {
            Naming::NameFilm => "Name_Film",
            Naming::Name => "Name",
            Naming::Custom => "Custom",
        }
    }
    /// As the Mac stores it (`batchNaming`).
    pub fn key(self) -> &'static str {
        match self {
            Naming::NameFilm => "nameFilm",
            Naming::Name => "name",
            Naming::Custom => "custom",
        }
    }
    pub fn from_key(key: &str) -> Naming {
        Self::ALL.into_iter().find(|n| n.key() == key).unwrap_or_default()
    }
}

/// What a batch does when a file of its name is already there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Existing {
    #[default]
    Number,
    Replace,
    Skip,
}

impl Existing {
    pub const ALL: [Existing; 3] = [Existing::Number, Existing::Replace, Existing::Skip];
    pub fn title(self) -> &'static str {
        match self {
            Existing::Number => "Add a Number",
            Existing::Replace => "Replace",
            Existing::Skip => "Skip",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Existing::Number => "number",
            Existing::Replace => "replace",
            Existing::Skip => "skip",
        }
    }
    pub fn from_key(key: &str) -> Existing {
        Self::ALL.into_iter().find(|n| n.key() == key).unwrap_or_default()
    }
}

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
        let bit_depth = if self.output_format() == OutputFormat::Jpeg { 8 } else { self.bit_depth };
        SaveSettings { bit_depth, color_space: SaveSettings::effective_space(self.color_space, bit_depth), jpeg_quality: self.jpeg_quality }
    }

    /// The file name for a photograph named `name` (without its extension,
    /// a duplicate's number included) developed with `film`. The extension
    /// is Windows' own for the format, as a single export's.
    pub fn file_name(&self, name: &str, film: &str) -> String {
        let template = match self.naming {
            Naming::NameFilm => "{name}_{film}",
            Naming::Name => "{name}",
            Naming::Custom if self.custom.is_empty() => "{name}",
            Naming::Custom => self.custom.as_str(),
        };
        let mut base = template.replace("{name}", name).replace("{film}", film);
        // A file name, never a path, and nothing Windows refuses in one.
        base = base.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
        let base = base.trim_end_matches(['.', ' ']).to_string();
        let base = if base.trim().is_empty() || base.starts_with('.') { format!("{name}{base}") } else { base };
        format!("{base}.{}", self.output_format().extension())
    }
}

/// How a photograph of a batch went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    #[default]
    Waiting,
    Done,
    Failed,
    Skipped,
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

/// The name `{name}` stands for: the file's without its extension, a
/// duplicate's number after it (`IMG_0412 (2)`).
pub fn batch_name(photo: &Path, title: &str) -> String {
    let file = photo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let suffix = title.strip_prefix(file.as_str()).unwrap_or("");
    let stem = photo.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{stem}{suffix}")
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

    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let staging = path.with_extension("json.tmp");
        std::fs::write(&staging, serde_json::to_vec(self)?)?;
        crate::platform::replace_file(&staging, path)
    }

    pub fn read(path: &Path) -> Option<Journal> {
        let journal: Journal = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (journal.schema == Self::SCHEMA).then_some(journal)
    }

    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

/// `name.ext`, then `name 2.ext`, `name 3.ext`… until `taken` says no.
pub fn numbered(file_name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(file_name) {
        return file_name.to_string();
    }
    let path = Path::new(file_name);
    let base = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = 2;
    loop {
        let name = format!("{base} {n}.{ext}");
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

/// The batch's own names made unique among themselves, in order, ignoring
/// case as Windows does (a RAW+JPEG pair would share one).
pub fn unique(names: &[String]) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    names
        .iter()
        .map(|name| {
            let unique = numbered(name, |n| used.contains(&n.to_lowercase()));
            used.insert(unique.to_lowercase());
            unique
        })
        .collect()
}

/// A match solved for `state`'s edits applied as opening applies it
/// (`Model::apply_brightness_match`): the total moves with the match,
/// keeping the user's exposure above it; a change under 0.05 EV keeps the
/// values and only marks them current. None without the match on.
pub fn matched(state: &EditState, ev: f64, limited: bool, inputs: Value) -> Option<EditState> {
    let mut matched = state.brightness_match.clone()?;
    let mut next = state.clone();
    let offset = crate::json::number(next.value(state::PREP_EXPOSURE)) - matched.ev.unwrap_or(0.0);
    if matched.ev.is_none_or(|old| (ev - old).abs() >= 0.05) {
        matched.ev = Some(ev);
        let (low, high) = state::PREP_EXPOSURE_RANGE;
        let total = if offset.abs() < 1e-9 { ev } else { ev + offset };
        crate::json::set(&mut next.params, state::PREP_EXPOSURE, crate::json::num(total.clamp(low, high)));
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
    pub fn total(&self) -> usize {
        self.journal.items.len()
    }
    /// Exported, failed or skipped.
    pub fn finished(&self) -> usize {
        self.total() - self.journal.remaining()
    }
    fn count(&self, status: Status) -> usize {
        self.journal.items.iter().filter(|i| i.status == status).count()
    }
    pub fn exported(&self) -> usize {
        self.count(Status::Done)
    }
    pub fn failures(&self) -> Vec<&Item> {
        self.journal.items.iter().filter(|i| i.status == Status::Failed).collect()
    }
    pub fn skipped(&self) -> usize {
        self.count(Status::Skipped)
    }
    pub fn fraction(&self) -> f64 {
        if self.total() == 0 { 1.0 } else { self.finished() as f64 / self.total() as f64 }
    }
    /// The popover's rule: the share exported, then the share that failed
    /// or was skipped. A batch that all failed draws no yellow.
    pub fn rule_parts(&self) -> (f64, f64) {
        if self.total() == 0 {
            return (0.0, 0.0);
        }
        let exported = self.exported() as f64 / self.total() as f64;
        (exported, (self.fraction() - exported).max(0.0))
    }
    /// Ended with something to look at: failures, or a cancel.
    pub fn needs_attention(&self) -> bool {
        !self.running && (self.count(Status::Failed) > 0 || self.cancelled)
    }

    /// The popover's title.
    pub fn title(&self) -> String {
        let (exported, total) = (self.exported(), self.total());
        if self.running {
            return format!("Exporting {} of {total}", (self.finished() + 1).min(total));
        }
        if self.cancelled {
            return format!("Cancelled after {exported} of {total}");
        }
        if total == 1 && exported == 1 { "Exported 1 photograph".into() } else { format!("Exported {exported} of {total}") }
    }

    /// The status line's and the grid foot's words while it runs.
    pub fn progress_words(&self) -> String {
        exporting(self.finished(), self.total(), self.journal.options.lightroom.as_deref().map(lightroom_title))
    }

    /// "Exported 10 of 12 · cancelled", then "· 2 failed" in bold.
    pub fn outcome_words(&self) -> (String, Option<String>) {
        let failed = self.count(Status::Failed);
        (
            format!("Exported {} of {}{}", self.exported(), self.total(), if self.cancelled { " · cancelled" } else { "" }),
            (failed > 0).then(|| format!("{failed} failed")),
        )
    }

    /// The status once it ends.
    pub fn finished_words(&self) -> String {
        let (exported, failed, skipped) = (self.exported(), self.count(Status::Failed), self.skipped());
        let folder = self.journal.destination.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let mut text = if exported == 1 { "Exported 1 photograph".to_string() } else { format!("Exported {exported} photographs") };
        text += &format!(" to “{folder}”");
        if failed > 0 {
            text += &format!("; {failed} failed");
        }
        if skipped > 0 {
            text += &format!("; {skipped} skipped");
        }
        if self.cancelled { format!("Export cancelled. {text}") } else { text }
    }
}

/// Lightroom's name by the key a batch keeps (`Options::lightroom`).
pub fn lightroom_title(key: &str) -> &'static str {
    if key == "classic" { "Lightroom Classic" } else { "Lightroom" }
}

/// "Exporting 5 of 12 photographs", or "Sending 1 of 2 photographs to Lightroom".
pub fn exporting(finished: usize, total: usize, lightroom: Option<&str>) -> String {
    let verb = if lightroom.is_none() { "Exporting" } else { "Sending" };
    let what = if total == 1 { "photograph" } else { "photographs" };
    let to = lightroom.map(|l| format!(" to {l}")).unwrap_or_default();
    format!("{verb} {} of {total} {what}{to}", (finished + 1).min(total))
}

/// The dialog's message: what the batch does, what the engine does, then
/// the size and colour.
pub fn message(count: usize, engine: ExportEngine, size: ExportSize, save: SaveSettings, preview_space: &str) -> String {
    let what = if count == 1 {
        "With its own edits, crop and border.".to_string()
    } else {
        format!("Each of the {count} photographs with its own edits, crop and border, one after another.")
    };
    let lead = match engine {
        ExportEngine::Fast => "Fast looks the same as Reference, in a fraction of the time.",
        ExportEngine::Reference => "Reference is the most precise; about half a minute for each large photo.",
    };
    let size = size.summary().map(|s| format!(" {s}.")).unwrap_or_default();
    let space = save.summary(true, preview_space).map(|s| format!(" {s}.")).unwrap_or_default();
    format!("{what} {lead}{size}{space}")
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
        assert_eq!(batch_name(Path::new(r"C:\p\IMG.CR3"), "IMG.CR3 (2)"), "IMG (2)");
        assert_eq!(batch_name(Path::new(r"C:\p\IMG.CR3"), "IMG.CR3"), "IMG");
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
        Run { journal: Journal::new(PathBuf::from(r"C:\out\Trip"), Options::default(), items), current: None, running: false, cancelled: false }
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
