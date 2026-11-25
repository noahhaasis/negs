//! The open folder as a library: its photographs, their ratings, flags and
//! edits, the open photograph, and undo. What `PhotoLibrary` and the
//! library half of `EditorModel` do on macOS, without a UI.
//!
//! Every call works on the files at once: a sidecar is read, patched and
//! written (atomically) before the call returns, so what the library shows
//! is what is on disk. Calls are small (a few kilobytes per photograph);
//! a front end that minds a slow network share runs them off its UI
//! thread. The open photograph's edits are the exception: `edit` keeps
//! them in memory and `save` writes them, which the front end calls a
//! second after the last edit, on leaving the photograph and at quit.

use crate::delete::{Bin, DeletePlan};
use crate::edits::{decode_edits, encode_edits, EditDefaults, EditGroup, EditState, SavedEdits};
use crate::filter::Filter;
use crate::folder::{self, PhotoFile};
use crate::item::{file_name, flag_text, items_of, rating_text, Item, Summary};
use crate::json::Json;
use crate::sidecar::{self, removing_variant, Flag, Patch, Sidecar, Tables, LATEST_DEFAULTS, SCHEMA};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How a library is set up.
pub struct Config {
    /// The engine's data folder, which holds `sidecar-defaults/`.
    pub data: PathBuf,
    /// The app's version, written into each sidecar as `app`.
    pub app: String,
    /// Where deleted photographs go.
    pub bin: Box<dyn Bin + Send>,
    /// Settings › Show camera JPEGs separately, off: a RAW's camera JPEG
    /// goes with it.
    pub pairing: bool,
    /// Settings › Save ratings and edits next to photos.
    pub sidecars: bool,
    /// Seeds the ids of new variants; random when `None`.
    pub id_seed: Option<u64>,
}

/// What the app says briefly after an action ("Edits Pasted"), and why it
/// wasn't saved, if it wasn't (`LibraryNote`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub detail: Option<String>,
    /// How many photographs it acted on.
    pub count: usize,
}

/// What an action did, beyond the files and the library's summaries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub note: Option<Note>,
    /// An alert: a sidecar that couldn't be written, a file that couldn't
    /// be deleted or put back.
    pub error: Option<String>,
    /// The open photograph's edits were replaced (`Library::state`): show
    /// them, and drop the editor's own undo history for it.
    pub open_changed: bool,
    /// The open photograph is gone (deleted, or a duplicate undone): open
    /// another, such as `open`, or show the grid.
    pub open_gone: bool,
    /// Open this one: the photograph that was open came back (Undo of a
    /// delete), or a duplicate's photograph after its duplicate went.
    pub open: Option<Item>,
    /// What to select: new duplicates, or what an Undo put back.
    pub select: Vec<Item>,
}

/// What opening a photograph found.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub state: EditState,
    /// Why its sidecar isn't used, said once: it opens with the defaults
    /// and the file is left alone. Or the film or paper this version
    /// lacks.
    pub problem: Option<String>,
}

/// What Copy Edits took.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedEdits {
    /// The user's edits (`user_edits`), without a solved brightness match.
    pub state: EditState,
    pub groups: BTreeSet<EditGroup>,
    /// The photograph had edits of its own. One without them pastes none:
    /// what it lands on is left without edits in `groups`.
    pub edited: bool,
}

/// The open photograph's sidecar as it stands (`PhotoSidecarContext`).
#[derive(Clone, Debug)]
struct Open {
    item: Item,
    variant: String,
    /// `variant` was made up when it opened: the file's first variant is
    /// written, should another app have made one since.
    variant_is_new: bool,
    defaults_version: i64,
    /// The user's edits as it opened. One without saved edits gets them
    /// only once they change.
    opened: EditState,
    edited_at_open: bool,
    /// The stored edits last read or sent; the same edits aren't written again.
    written: Option<Json>,
    /// Why it isn't written: unreadable, from a newer Negs, unwritable.
    blocked: Option<String>,
    /// The film and paper it names that this version lacks, with what is
    /// shown instead; written back while that stays chosen.
    missing_film: Option<(String, String)>,
    missing_paper: Option<(String, String)>,
    /// What it shows now.
    state: EditState,
}

/// What opening a photograph needs from its sidecar (`SidecarRestore`).
#[derive(Clone, Debug)]
struct Restore {
    exists: bool,
    variant: String,
    new_variant: bool,
    defaults_version: i64,
    rating: u8,
    flag: Option<Flag>,
    edits: Option<SavedEdits>,
    problem: Option<String>,
}

/// A photograph's stored edits as a batch exports them
/// (`Library::export_edits`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExportEdits {
    pub state: EditState,
    /// The match stored with them was solved for them.
    pub match_solved: bool,
    /// What this version lacks of them: "the film “x”", "the paper “y”".
    pub missing: Vec<String>,
    /// Their look (`thumbnail::look`), which a match solved for them is
    /// stored with.
    pub look: String,
}

/// A sidecar as it was before and after an undoable action.
#[derive(Clone, Debug, PartialEq)]
struct FileChange {
    photo: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
enum StepKind {
    /// Ratings and flags: each item's before and after. Undo puts back the
    /// fields the step changed, merged with the file as it is then, unless
    /// the file is still as the step left it.
    Summaries(Vec<(Item, Summary, Summary)>),
    /// Paste, Reset, Duplicate: the files as they were.
    Sidecars,
    /// Files moved to the bin, and duplicates removed from sidecars.
    Delete { plan: DeletePlan, moves: Vec<(PathBuf, PathBuf)>, reopen: Option<Item> },
    /// Redo of a delete: the same plan again.
    Redelete { plan: DeletePlan },
}

#[derive(Clone, Debug)]
struct Step {
    action: String,
    kind: StepKind,
    files: Vec<FileChange>,
}

/// Keeps each sidecar's bytes before an action first touches it.
#[derive(Default)]
struct Recorder {
    before: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl Recorder {
    fn touch(&mut self, photo: &Path) {
        if !self.before.iter().any(|(p, _)| p == photo) {
            self.before.push((photo.to_path_buf(), sidecar::read_bytes(photo).ok().flatten()));
        }
    }

    fn photos(&self) -> Vec<PathBuf> {
        self.before.iter().map(|(p, _)| p.clone()).collect()
    }

    fn finish(self) -> Vec<FileChange> {
        self.before
            .into_iter()
            .filter_map(|(photo, before)| {
                let after = sidecar::read_bytes(&photo).ok().flatten();
                (before != after).then_some(FileChange { photo, before, after })
            })
            .collect()
    }
}

/// Small, unpredictable variant ids: 8 lowercase hex digits, as macOS makes.
struct Ids(u64);

impl Ids {
    fn new(seed: Option<u64>) -> Ids {
        let seed = seed.unwrap_or_else(|| {
            use std::hash::{BuildHasher, Hasher};
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
            hasher.write_u32(std::process::id());
            hasher.finish()
        });
        Ids(seed)
    }

    fn next(&mut self) -> String {
        // SplitMix64.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        format!("{:08x}", (z ^ (z >> 31)) as u32)
    }
}

/// A sidecar's size and date as listed: a different one is another
/// app's write (or the library's own, which it records as it reads back).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<std::time::SystemTime>,
    size: Option<u64>,
}

impl Stamp {
    fn of(file: &PhotoFile) -> Stamp {
        Stamp { modified: file.modified, size: file.size }
    }
}

/// Whether sidecars can be written in `folder` (a locked card, a
/// read-only share can't).
pub fn can_write(folder: &Path) -> bool {
    std::fs::metadata(folder).map(|m| !m.permissions().readonly()).unwrap_or(false)
}

pub struct Library {
    folder: PathBuf,
    tables: Tables,
    defaults: EditDefaults,
    app: String,
    bin: Box<dyn Bin + Send>,
    pairing: bool,
    sidecars: bool,
    filter: Filter,
    files: Vec<PhotoFile>,
    /// Where each photograph is in `files`: the strip and grid look up
    /// every cell's file on each sync, which searching made quadratic.
    file_index: HashMap<PathBuf, usize>,
    companions: HashMap<PathBuf, PathBuf>,
    /// Each photograph's sidecar as last listed; `None`, it had none.
    sidecar_files: HashMap<PathBuf, Option<Stamp>>,
    duplicates: HashMap<PathBuf, Vec<String>>,
    summaries: HashMap<Item, Summary>,
    looks: HashMap<Item, String>,
    read_only: bool,
    failed: Option<String>,
    open: Option<Open>,
    copied: Option<CopiedEdits>,
    previous: Option<EditState>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Folders whose sidecars couldn't be written, said once each.
    unwritable: HashSet<PathBuf>,
    ids: Ids,
}

impl Library {
    /// The library of `folder`, listed and its sidecars read.
    pub fn new(folder: impl Into<PathBuf>, config: Config) -> Result<Library, String> {
        let tables = Tables::new(config.data);
        let defaults = EditDefaults::for_table(&*tables.latest()?);
        let mut library = Library {
            folder: folder.into(),
            tables,
            defaults,
            app: config.app,
            bin: config.bin,
            pairing: config.pairing,
            sidecars: config.sidecars,
            filter: Filter::default(),
            files: Vec::new(),
            file_index: HashMap::new(),
            companions: HashMap::new(),
            sidecar_files: HashMap::new(),
            duplicates: HashMap::new(),
            summaries: HashMap::new(),
            looks: HashMap::new(),
            read_only: false,
            failed: None,
            open: None,
            copied: None,
            previous: None,
            undo: Vec::new(),
            redo: Vec::new(),
            unwritable: HashSet::new(),
            ids: Ids::new(config.id_seed),
        };
        library.refresh(false);
        Ok(library)
    }

    // MARK: What it shows

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The photographs, in Finder's order.
    pub fn files(&self) -> &[PhotoFile] {
        &self.files
    }

    /// The photographs, each followed by its duplicates.
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::with_capacity(self.files.len());
        for file in &self.files {
            items.push(Item::photo(&file.path));
            for id in self.duplicates.get(&file.path).into_iter().flatten() {
                items.push(Item::duplicate(&file.path, id));
            }
        }
        items
    }

    /// The listing of `item`'s photograph, for its thumbnail's key.
    pub fn file(&self, item: &Item) -> Option<&PhotoFile> {
        self.file_index.get(&item.photo).map(|&i| &self.files[i])
    }

    /// `IMG_0412.CR3`, or `IMG_0412.CR3 (2)` for its first duplicate.
    pub fn name(&self, item: &Item) -> String {
        let name = item.file_name();
        match item.variant.as_ref().and_then(|id| self.duplicates.get(&item.photo)?.iter().position(|d| d == id)) {
            Some(index) => format!("{name} ({})", index + 2),
            None => name,
        }
    }

    /// A photograph's duplicates, by variant id.
    pub fn duplicates(&self, photo: &Path) -> &[String] {
        self.duplicates.get(photo).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn summary(&self, item: &Item) -> Summary {
        self.summaries.get(item).cloned().unwrap_or_default()
    }

    /// The look of an item's stored edits, which names its edited
    /// thumbnail; `None` without edits (it develops as a new photograph).
    pub fn look(&self, item: &Item) -> Option<&str> {
        self.looks.get(item).map(String::as_str)
    }

    /// What `item`'s developed thumbnail is made from, read from its sidecar
    /// now, as opening it would show it (`developThumbnail`): its stored
    /// edits restored, a film or paper this version lacks replaced as
    /// opening replaces it, and whether the brightness match stored with
    /// them was solved for them. `None` without stored edits (it develops
    /// as `unedited_state`), or when its sidecar can't be read. Opens and
    /// writes nothing.
    pub fn thumbnail_state(&self, item: &Item) -> Option<(EditState, bool)> {
        if !self.sidecars {
            return None;
        }
        let found = sidecar::read(&item.photo).ok()??;
        let chosen = item.variant.as_deref().and_then(|id| found.variant(id)).or(found.variants.first())?;
        let table = self.tables.get(found.defaults_version).ok()?;
        let saved = decode_edits(chosen.edits.as_ref()?, &table).ok()?;
        let mut state = self.defaults.restored(&saved.state);
        let d = &self.defaults;
        if !d.films.is_empty() && !d.films.contains_key(&state.film) {
            let shown = if d.films.contains_key(&d.table_film) { d.table_film.clone() } else { d.films.keys().next().cloned().unwrap_or_default() };
            state.params.set_at("io.scan_film", Json::Bool(d.films.get(&shown).copied().unwrap_or(false)));
            state.film = shown;
        }
        if !d.papers.is_empty() && !d.papers.contains(&state.paper) {
            state.paper = if d.papers.contains(&d.table_paper) { d.table_paper.clone() } else { d.papers.iter().next().cloned().unwrap_or_default() };
        }
        Some((state, saved.match_solved))
    }

    /// What a batch export develops for `item` (`batchItems`): its stored
    /// edits restored, as they are in its sidecar now. A film or paper this
    /// version lacks isn't replaced but named in `missing` (the batch fails
    /// it). `None` without stored edits, or with a sidecar that can't be
    /// read: it exports as a new photograph would open.
    pub fn export_edits(&self, item: &Item) -> Option<ExportEdits> {
        if !self.sidecars {
            return None;
        }
        let found = sidecar::read(&item.photo).ok()??;
        let chosen = item.variant.as_deref().and_then(|id| found.variant(id)).or(found.variants.first())?;
        let edits = chosen.edits.as_ref()?;
        let table = self.tables.get(found.defaults_version).ok()?;
        let saved = decode_edits(edits, &table).ok()?;
        let state = self.defaults.restored(&saved.state);
        let d = &self.defaults;
        let mut missing = vec![];
        if !d.films.is_empty() && !d.films.contains_key(&state.film) {
            missing.push(format!("the film “{}”", state.film));
        }
        if !d.papers.is_empty() && !d.papers.contains(&state.paper) {
            missing.push(format!("the paper “{}”", state.paper));
        }
        let look = crate::thumbnail::look(edits, found.defaults_version);
        Some(ExportEdits { state, match_solved: saved.match_solved, missing, look })
    }

    /// Stores a brightness match a batch solved for `item`'s stored edits,
    /// as opening it would have (`storeSolvedMatch`): only while they are
    /// still the ones it was solved for (their look is `look`), and never
    /// for the open photograph, whose own state is saved. No undo step.
    /// True if the sidecar was written.
    pub fn store_solved_match(&mut self, item: &Item, look: &str, state: &EditState) -> bool {
        if !self.sidecars || self.open.as_ref().is_some_and(|o| o.item == *item) || self.read_only_photo(&item.photo) {
            return false;
        }
        let Ok(Some(found)) = sidecar::read(&item.photo) else { return false };
        let Some(variant) = item.variant.as_deref().and_then(|id| found.variant(id)).or(found.variants.first()) else { return false };
        if variant.edits.as_ref().map(|e| crate::thumbnail::look(e, found.defaults_version)).as_deref() != Some(look) {
            return false;
        }
        if !state.brightness_match.as_ref().is_some_and(|m| m.ev.is_some() && m.solved) {
            return false;
        }
        let Ok(table) = self.tables.get(found.defaults_version) else { return false };
        let patch = Patch {
            primary: item.variant.is_none(),
            edits: Some(Some(encode_edits(state, true, &table))),
            known: table.known(Some(&self.defaults.params)),
            ..Patch::new(variant.id.clone(), found.defaults_version, &self.app)
        };
        if self.write(&item.photo, &patch).is_err() {
            return false;
        }
        self.reload(&item.photo);
        true
    }

    /// How a photograph without stored edits develops for its thumbnail:
    /// as a new photograph opens, after nothing (`plainState`).
    pub fn unedited_state(&self) -> EditState {
        self.defaults.new_photo_state(&EditState::empty())
    }

    /// A RAW's camera JPEG, which goes with it.
    pub fn companion(&self, photo: &Path) -> Option<&Path> {
        self.companions.get(photo).map(PathBuf::as_path)
    }

    /// Ratings and edits can't be saved in the folder.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Why the folder couldn't be listed, if it couldn't.
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }

    /// What the strip and grid show: what the filter lets through, and
    /// always `current`, so the open photograph never vanishes under the
    /// edit view.
    pub fn shown(&self, current: Option<&Item>) -> Vec<Item> {
        let items = self.items();
        if !self.filter.active() {
            return items;
        }
        items.into_iter().filter(|i| Some(i) == current || self.filter.includes(&self.summary(i))).collect()
    }

    /// The folder's rejected photographs and duplicates, filtered out or
    /// not: what Delete Rejected Photographs deletes.
    pub fn rejected(&self) -> Vec<Item> {
        self.items().into_iter().filter(|i| self.summary(i).flag == Some(Flag::Reject)).collect()
    }

    pub fn defaults(&self) -> &EditDefaults {
        &self.defaults
    }

    pub fn copied(&self) -> Option<&CopiedEdits> {
        self.copied.as_ref()
    }

    /// The edits of the last photograph left with edits, for Paste from
    /// Previous.
    pub fn previous_edits(&self) -> Option<&EditState> {
        self.previous.as_ref()
    }

    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|s| s.action.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|s| s.action.as_str())
    }

    /// How many library steps Undo and Redo hold: a front end that keeps
    /// one undo stack with its editor's steps adds a library step when an
    /// action made one.
    pub fn undo_depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    // MARK: Settings

    /// The engine's defaults and Settings › New photographs. A photograph
    /// open without edits of its own follows a new film or paper, without
    /// an undo step (`followNewPhotoStocks`): `open_changed` then.
    pub fn set_defaults(&mut self, defaults: EditDefaults) -> Outcome {
        let stocks_changed = (defaults.new_photo_film.as_str(), defaults.new_photo_paper.as_str())
            != (self.defaults.new_photo_film.as_str(), self.defaults.new_photo_paper.as_str());
        self.defaults = defaults;
        let mut outcome = Outcome::default();
        if !stocks_changed {
            return outcome;
        }
        let defaults = self.defaults.clone();
        let with_new_stocks = |edits: &EditState| {
            let mut next = edits.clone();
            next.film = defaults.new_photo_film.clone();
            next.paper = defaults.new_photo_paper.clone();
            next.params.set_at("io.scan_film", Json::Bool(defaults.films.get(&next.film).copied().unwrap_or(false)));
            next
        };
        let Some(open) = self.open.as_mut() else { return outcome };
        if open.edited_at_open || open.blocked.is_some() {
            return outcome;
        }
        if open.state.user_edits() != open.opened {
            open.opened = with_new_stocks(&open.opened);
            return outcome;
        }
        let next = with_new_stocks(&open.state);
        if next != open.state {
            open.state = next;
            open.opened = open.state.user_edits();
            outcome.open_changed = true;
        }
        outcome
    }

    /// Settings › Show camera JPEGs separately (off pairs them).
    pub fn set_pairing(&mut self, pairing: bool) {
        if pairing != self.pairing {
            self.pairing = pairing;
            self.refresh(false);
        }
    }

    /// Settings › Save ratings and edits next to photos.
    pub fn set_sidecars(&mut self, on: bool) {
        if on != self.sidecars {
            self.sidecars = on;
            self.summaries.clear();
            self.looks.clear();
            self.duplicates.clear();
            self.sidecar_files.clear();
            self.refresh(false);
        }
    }

    // MARK: The listing

    /// Lists the folder again and reads the sidecars that are new, changed
    /// or gone since: when the folder watcher says something changed, and
    /// when the window becomes active (network shares lose events).
    /// Returns the photographs whose sidecars another app changed. If the
    /// open photograph's is one of them, its edits are taken from the file
    /// when nothing of the user's is unsaved and they're not `interacting`
    /// (dragging a slider, cropping): `open_changed` then.
    pub fn refresh(&mut self, interacting: bool) -> (Vec<PathBuf>, Outcome) {
        let mut outcome = Outcome::default();
        let listing = match folder::listing(&self.folder, self.pairing) {
            Ok(listing) => listing,
            Err(e) => {
                self.failed = Some(e.to_string());
                return (Vec::new(), outcome);
            }
        };
        self.failed = None;
        self.files = listing.photos;
        self.file_index = self.files.iter().enumerate().map(|(i, f)| (f.path.clone(), i)).collect();
        self.companions = listing.companions;
        self.read_only = !can_write(&self.folder);
        let present: HashSet<PathBuf> = self.files.iter().map(|f| f.path.clone()).collect();
        self.sidecar_files.retain(|p, _| present.contains(p));
        self.duplicates.retain(|p, _| present.contains(p));
        self.summaries.retain(|i, _| present.contains(&i.photo));
        self.looks.retain(|i, _| present.contains(&i.photo));
        if !self.sidecars {
            return (Vec::new(), outcome);
        }
        let mut changed: Vec<PathBuf> = Vec::new();
        for file in self.files.clone() {
            let found = listing.sidecars.get(&file.name).map(Stamp::of);
            let known = self.sidecar_files.get(&file.path);
            if known != Some(&found) {
                if known.is_some() {
                    changed.push(file.path.clone());
                }
                self.reload(&file.path);
            }
        }
        if let Some(open) = &self.open {
            if changed.contains(&open.item.photo) {
                outcome = self.sidecar_changed_on_disk(interacting);
            }
        }
        (changed, outcome)
    }

    /// Reads `photo`'s sidecar again: its items' summaries, looks and
    /// duplicates.
    fn reload(&mut self, photo: &Path) {
        if !self.sidecars {
            return;
        }
        // What this read saw, so the next listing tells another app's
        // changes from the library's own.
        let stamp = std::fs::metadata(sidecar::sidecar_path(photo)).ok().map(|m| Stamp::of(&PhotoFile::new(sidecar::sidecar_path(photo), Some(&m))));
        self.sidecar_files.insert(photo.to_path_buf(), stamp);
        let read: Vec<(Item, Summary, Option<String>)> = match sidecar::read(photo) {
            Ok(found) => items_of(photo, found.as_ref()),
            Err(problem) => vec![(Item::photo(photo), Summary { problem: Some(problem.message(&file_name(photo))), ..Summary::default() }, None)],
        };
        let ids: Vec<String> = read.iter().filter_map(|(i, _, _)| i.variant.clone()).collect();
        for gone in self.duplicates.get(photo).cloned().unwrap_or_default() {
            if !ids.contains(&gone) {
                let item = Item::duplicate(photo, gone);
                self.summaries.remove(&item);
                self.looks.remove(&item);
            }
        }
        if ids.is_empty() {
            self.duplicates.remove(photo);
        } else {
            self.duplicates.insert(photo.to_path_buf(), ids);
        }
        for (item, summary, look) in read {
            self.summaries.insert(item.clone(), summary);
            match look {
                Some(look) => self.looks.insert(item, look),
                None => self.looks.remove(&item),
            };
        }
    }

    // MARK: The open photograph

    /// The open photograph, if one is.
    pub fn open_item(&self) -> Option<&Item> {
        self.open.as_ref().map(|o| &o.item)
    }

    /// What the open photograph shows.
    pub fn state(&self) -> Option<&EditState> {
        self.open.as_ref().map(|o| &o.state)
    }

    /// The open photograph has edits its sidecar keeps: saved before, or
    /// made since it opened.
    pub fn open_edited(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.edited_at_open || o.state.user_edits() != o.opened)
    }

    /// Opens `item`: the photograph open before is saved first, and
    /// remembered for Paste from Previous if it has edits. One without
    /// saved edits opens with the defaults, following the one before it
    /// in Image Size and output colour.
    pub fn open(&mut self, item: &Item) -> Opened {
        let mut previous = EditState::empty();
        if self.open.is_some() {
            let _ = self.save();
            let open = self.open.as_ref().expect("still open");
            if open.written.is_some() {
                self.previous = Some(open.state.user_edits());
            }
            previous = open.state.clone();
        }
        let restore = self.restore(&item.photo, item.variant.as_deref());
        self.adopt(item, restore, &previous)
    }

    /// Saves the open photograph and closes it, as at quit.
    pub fn close(&mut self) -> Result<(), String> {
        let saved = self.save();
        if let Some(open) = self.open.take() {
            if open.written.is_some() {
                self.previous = Some(open.state.user_edits());
            }
        }
        saved
    }

    /// The user changed the open photograph: kept in memory until `save`.
    pub fn edit(&mut self, state: EditState) {
        if let Some(open) = &mut self.open {
            open.state = state;
        }
    }

    /// Whether `save` would write: the open photograph has edits its
    /// sidecar doesn't have yet.
    pub fn unsaved(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.blocked.is_none() && self.edits_to_send(o).is_some())
    }

    /// Writes the open photograph's edits if they changed: a second after
    /// the last edit, on leaving it, at quit. Back where it started, edits
    /// this visit saved are removed again (`flushSidecar`). An error says
    /// why it couldn't, once per folder.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.sidecars {
            return Ok(());
        }
        let Some(open) = &self.open else { return Ok(()) };
        if open.blocked.is_some() {
            return Ok(());
        }
        let Some(edits) = self.edits_to_send(open) else { return Ok(()) };
        let table = self.tables.get(open.defaults_version)?;
        let patch = Patch {
            primary: open.variant_is_new || open.item.variant.is_none(),
            edits: Some(edits.clone()),
            known: table.known(Some(&self.defaults.params)),
            ..Patch::new(&open.variant, open.defaults_version, &self.app)
        };
        let photo = open.item.photo.clone();
        if let Err(reason) = self.write(&photo, &patch) {
            let mut outcome = Outcome::default();
            self.failed_write(&photo, reason, &mut outcome);
            return Err(outcome.error.unwrap_or_default());
        }
        self.reload(&photo);
        let open = self.open.as_mut().expect("open");
        open.written = edits;
        Ok(())
    }

    /// The edits the open photograph's sidecar would be sent now: once the
    /// user has edited it, or always if it had edits (`None` inside removes
    /// them). `None` when nothing changed (`sidecarEditsToSend`).
    fn edits_to_send(&self, open: &Open) -> Option<Option<Json>> {
        let edited = open.state.user_edits() != open.opened;
        let edits = if edited || open.edited_at_open { Some(self.stored_edits(&open.state, open)?) } else { None };
        (edits != open.written).then_some(edits)
    }

    /// `state` as the open photograph's sidecar stores it.
    fn stored_edits(&self, state: &EditState, open: &Open) -> Option<Json> {
        let table = self.tables.get(open.defaults_version).ok()?;
        let mut saved = state.clone();
        if let Some((name, shown)) = &open.missing_film {
            if saved.film == *shown {
                saved.film = name.clone();
            }
        }
        if let Some((name, shown)) = &open.missing_paper {
            if saved.paper == *shown {
                saved.paper = name.clone();
            }
        }
        let solved = state.brightness_match.as_ref().is_some_and(|m| m.ev.is_some() && m.solved);
        Some(encode_edits(&saved, solved, &table))
    }

    /// Reads `photo`'s sidecar for opening `variant` (or its first).
    fn restore(&mut self, photo: &Path, variant: Option<&str>) -> Restore {
        let mut restore = Restore {
            exists: false,
            variant: self.ids.next(),
            new_variant: true,
            defaults_version: LATEST_DEFAULTS,
            rating: 0,
            flag: None,
            edits: None,
            problem: None,
        };
        if !self.sidecars {
            return restore;
        }
        let found = match sidecar::read(photo) {
            Ok(None) => return restore,
            Ok(Some(found)) => found,
            Err(problem) => {
                restore.exists = true;
                restore.problem = Some(problem.message(&file_name(photo)));
                return restore;
            }
        };
        restore.exists = true;
        restore.defaults_version = found.defaults_version;
        let Some(chosen) = variant.and_then(|id| found.variant(id)).or(found.variants.first()) else { return restore };
        restore.variant = chosen.id.clone();
        restore.new_variant = false;
        restore.rating = chosen.rating;
        restore.flag = chosen.flag;
        if let Some(edits) = &chosen.edits {
            let decoded = self.tables.get(found.defaults_version).map_err(crate::sidecar::Problem::Unreadable).and_then(|t| decode_edits(edits, &t));
            match decoded {
                Ok(saved) => restore.edits = Some(saved),
                Err(problem) => {
                    restore.problem = Some(problem.message(&file_name(photo)));
                    restore.edits = None;
                }
            }
        }
        restore
    }

    /// Sets the open photograph from what its sidecar held (`adoptEdits`).
    fn adopt(&mut self, item: &Item, restore: Restore, previous: &EditState) -> Opened {
        let mut problem = restore.problem.clone();
        let mut missing_film = None;
        let mut missing_paper = None;
        let state = match (&restore.edits, &restore.problem) {
            (Some(saved), None) => {
                let mut restored = self.defaults.restored(&saved.state);
                let d = &self.defaults;
                // A film or paper this version lacks falls back to the table's.
                if !d.films.is_empty() && !d.films.contains_key(&restored.film) {
                    let shown = if d.films.contains_key(&d.table_film) { d.table_film.clone() } else { d.films.keys().next().cloned().unwrap_or_default() };
                    missing_film = Some((restored.film.clone(), shown.clone()));
                    restored.params.set_at("io.scan_film", Json::Bool(d.films.get(&shown).copied().unwrap_or(false)));
                    restored.film = shown;
                }
                if !d.papers.is_empty() && !d.papers.contains(&restored.paper) {
                    let shown = if d.papers.contains(&d.table_paper) { d.table_paper.clone() } else { d.papers.iter().next().cloned().unwrap_or_default() };
                    missing_paper = Some((restored.paper.clone(), shown.clone()));
                    restored.paper = shown;
                }
                if missing_film.is_some() || missing_paper.is_some() {
                    let names: Vec<String> = [missing_film.as_ref().map(|(s, _)| format!("the film “{s}”")), missing_paper.as_ref().map(|(s, _)| format!("the paper “{s}”"))]
                        .into_iter()
                        .flatten()
                        .collect();
                    let shown: Vec<String> = [missing_film.as_ref().map(|(_, s)| s.clone()), missing_paper.as_ref().map(|(_, s)| s.clone())].into_iter().flatten().collect();
                    problem = Some(format!(
                        "{} was edited with {}, which this version of Negs doesn’t have. It shows {} instead, and keeps the saved choice unless you pick another.",
                        item.file_name(),
                        names.join(" and "),
                        shown.join(" and ")
                    ));
                }
                restored
            }
            _ => self.defaults.new_photo_state(previous),
        };
        self.summaries.entry(item.clone()).or_insert(Summary {
            rating: restore.rating,
            flag: restore.flag,
            edited: restore.edits.is_some(),
            problem: restore.problem.clone(),
        });
        let mut open = Open {
            item: item.clone(),
            variant: restore.variant,
            variant_is_new: restore.new_variant,
            defaults_version: restore.defaults_version,
            opened: state.user_edits(),
            edited_at_open: restore.edits.is_some(),
            written: None,
            blocked: restore.problem,
            missing_film,
            missing_paper,
            state: state.clone(),
        };
        if open.edited_at_open && open.blocked.is_none() {
            open.written = self.stored_edits(&state, &open);
        }
        self.open = Some(open);
        Opened { state, problem }
    }

    /// Another app changed the open photograph's sidecar. Newer edits
    /// replace these only if they're all saved (`sidecarChangedOnDisk`).
    fn sidecar_changed_on_disk(&mut self, interacting: bool) -> Outcome {
        let mut outcome = Outcome::default();
        let Some(open) = self.open.clone() else { return outcome };
        if open.blocked.is_some() {
            return outcome;
        }
        let restore = self.restore(&open.item.photo, open.item.variant.as_deref());
        let current = self.open.as_mut().expect("open");
        if let Some(problem) = &restore.problem {
            // Never written over now.
            current.blocked = Some(problem.clone());
            return outcome;
        }
        if !restore.new_variant && current.variant_is_new {
            current.variant = restore.variant.clone();
            current.variant_is_new = false;
        }
        let ours = open.written.as_ref().and_then(|w| self.tables.get(open.defaults_version).ok().and_then(|t| decode_edits(w, &t).ok()));
        let open = self.open.clone().expect("open");
        if (restore.edits == ours && restore.defaults_version == open.defaults_version) || interacting || self.edits_to_send(&open).is_some() {
            return outcome;
        }
        if open.item.variant.is_some() && restore.new_variant {
            // Its duplicate is gone: its photograph opens instead.
            self.open = None;
            outcome.open_gone = true;
            outcome.open = Some(Item::photo(&open.item.photo));
            return outcome;
        }
        let previous = open.state.clone();
        self.adopt(&open.item, restore, &previous);
        outcome.open_changed = true;
        outcome
    }

    // MARK: Writing

    /// Applies `patch` to `photo`'s sidecar as it is on disk now and writes
    /// it, or removes it once nothing is left. Nothing is written when
    /// nothing changes.
    fn write(&self, photo: &Path, patch: &Patch) -> Result<(), String> {
        let raw = sidecar::read_raw(photo).map_err(|p| p.message(&file_name(photo)))?;
        let next = patch.applied(raw.as_ref()).map_err(|p| p.message(&file_name(photo)))?;
        if next == raw && raw.is_some() == sidecar::sidecar_path(photo).exists() {
            return Ok(());
        }
        sidecar::write_raw(photo, next.as_ref()).map_err(|e| e.to_string())
    }

    /// A write failed: the library shows what the file keeps, the open
    /// photograph's sidecar isn't written again, and the folder's problem
    /// is said once (`sidecarFailed`).
    fn failed_write(&mut self, photo: &Path, reason: String, outcome: &mut Outcome) {
        if let Some(open) = &mut self.open {
            if open.item.photo == photo {
                open.blocked = Some(reason.clone());
            }
        }
        self.reload(photo);
        let folder = photo.parent().unwrap_or(Path::new("")).to_path_buf();
        if self.unwritable.insert(folder.clone()) && outcome.error.is_none() {
            outcome.error = Some(format!("Edits to photographs in “{}” can’t be saved beside them: {reason}", file_name(&folder)));
        }
    }

    fn push(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
    }

    // MARK: Ratings and flags

    /// Rates `items`, 0 (none) to 5 stars.
    pub fn set_rating(&mut self, items: &[Item], stars: i64) -> Outcome {
        let stars = stars.clamp(0, 5) as u8;
        self.change_summaries("Rating", None, items, |s| s.rating = stars, |s| rating_text(s.rating))
    }

    /// Raises or lowers the rating of `items` (] and [).
    pub fn change_rating(&mut self, items: &[Item], by: i64) -> Outcome {
        let many = if by > 0 { "Rating Raised" } else { "Rating Lowered" };
        self.change_summaries("Rating", Some(many), items, |s| s.rating = (s.rating as i64 + by).clamp(0, 5) as u8, |s| rating_text(s.rating))
    }

    pub fn set_flag(&mut self, items: &[Item], flag: Option<Flag>) -> Outcome {
        self.change_summaries("Flag", None, items, |s| s.flag = flag, |s| flag_text(s.flag).to_string())
    }

    /// `many` is the note for several photographs, when the note would
    /// differ between them (`changeLibrary`).
    fn change_summaries(
        &mut self,
        action: &str,
        many: Option<&str>,
        items: &[Item],
        change: impl Fn(&mut Summary),
        note: impl Fn(&Summary) -> String,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        if items.is_empty() {
            return outcome;
        }
        let mut recorder = Recorder::default();
        let mut befores: Vec<(Item, Summary, Summary)> = Vec::new();
        let mut unsaved: Option<String> = None;
        let mut last = Summary::default();
        for item in items {
            let before = self.summary(item);
            let mut after = before.clone();
            change(&mut after);
            last = after.clone();
            if after == before {
                continue;
            }
            befores.push((item.clone(), before.clone(), after.clone()));
            self.summaries.insert(item.clone(), after.clone());
            recorder.touch(&item.photo);
            if let Err(reason) = self.save_summary(&before, &after, item, &mut outcome) {
                unsaved.get_or_insert(reason);
            }
        }
        for photo in recorder.photos() {
            if self.sidecars {
                self.reload_keeping_unsaved(&photo);
            }
        }
        let text = if items.len() == 1 { note(&last) } else { many.map(str::to_string).unwrap_or_else(|| note(&last)) };
        outcome.note = Some(Note { text, detail: unsaved, count: items.len() });
        if !befores.is_empty() {
            let files = recorder.finish();
            self.push(Step { action: action.into(), kind: StepKind::Summaries(befores), files });
        }
        outcome
    }

    /// Reads `photo`'s sidecar again, keeping summaries the app holds but
    /// couldn't save (a read-only folder keeps ratings for the session).
    fn reload_keeping_unsaved(&mut self, photo: &Path) {
        if self.read_only_photo(photo) {
            return;
        }
        self.reload(photo);
    }

    fn read_only_photo(&self, photo: &Path) -> bool {
        !can_write(photo.parent().unwrap_or(Path::new(".")))
    }

    /// Writes a rating or flag change to `item`'s sidecar; why it can't, if
    /// it can't. The open photograph's goes to its variant; another's to its
    /// file's first variant, or a new one if it has no file
    /// (`saveLibraryChange`).
    fn save_summary(&mut self, before: &Summary, after: &Summary, item: &Item, outcome: &mut Outcome) -> Result<(), String> {
        if !self.sidecars {
            return Err("Not saved: Settings › Save ratings and edits is off".into());
        }
        let rating = (after.rating != before.rating).then_some(after.rating as i64);
        let flag = (after.flag != before.flag).then_some(after.flag);
        let photo = item.photo.clone();
        let patch = if let Some(open) = self.open.as_ref().filter(|o| o.item == *item) {
            if open.blocked.is_some() {
                return Err("Not saved: its .negs file can’t be changed".into());
            }
            if self.read_only_photo(&photo) {
                return Err("Not saved: the folder is read-only".into());
            }
            Patch { primary: open.variant_is_new || open.item.variant.is_none(), rating, flag, ..Patch::new(&open.variant, open.defaults_version, &self.app) }
        } else {
            if before.problem.is_some() {
                return Err("Not saved: its .negs file can’t be changed".into());
            }
            if self.read_only_photo(&photo) {
                return Err("Not saved: the folder is read-only".into());
            }
            let variant = item.variant.clone().unwrap_or_else(|| self.ids.next());
            Patch { primary: item.variant.is_none(), rating, flag, ..Patch::new(variant, LATEST_DEFAULTS, &self.app) }
        };
        if let Err(reason) = self.write(&photo, &patch) {
            self.failed_write(&photo, reason, outcome);
        }
        Ok(())
    }

    // MARK: Edits per photograph

    /// `item`'s edits as `user_edits` gives them, and whether it has any of
    /// its own; `None` if its sidecar can't be read.
    fn user_edits_of(&mut self, item: &Item) -> Option<(EditState, bool)> {
        if let Some(open) = self.open.as_ref().filter(|o| o.item == *item) {
            return Some((open.state.user_edits(), self.open_edited()));
        }
        if !self.sidecars {
            return Some((self.defaults.plain_state().user_edits(), false));
        }
        let restore = self.restore(&item.photo, item.variant.as_deref());
        if restore.problem.is_some() {
            return None;
        }
        match restore.edits {
            None => Some((self.defaults.plain_state().user_edits(), false)),
            Some(saved) => Some((self.defaults.restored(&saved.state).user_edits(), true)),
        }
    }

    /// Copy Edits: `item`'s edits in `groups` (`EditGroup::standard` for a
    /// plain Copy Edits).
    pub fn copy_edits(&mut self, item: &Item, groups: BTreeSet<EditGroup>) -> Outcome {
        let note = match self.user_edits_of(item) {
            None => Note { text: "Edits Not Copied".into(), detail: Some("Its .negs file can’t be read".into()), count: 1 },
            Some((state, edited)) => {
                self.copied = Some(CopiedEdits { state, groups, edited });
                Note { text: "Edits Copied".into(), detail: None, count: 1 }
            }
        };
        Outcome { note: Some(note), ..Outcome::default() }
    }

    /// Paste Edits onto `items`, as one undo step.
    pub fn paste_edits(&mut self, items: &[Item]) -> Outcome {
        let Some(copied) = self.copied.clone() else { return Outcome::default() };
        let from_unedited = (!copied.edited).then(|| copied.groups.clone());
        self.change_edits(items, "Paste Edits", "Edits Pasted", false, from_unedited, |_, target| EditGroup::merge(&copied.groups, &copied.state, target))
    }

    /// Applies the last photograph left with edits, no copy step.
    pub fn paste_from_previous(&mut self, items: &[Item]) -> Outcome {
        let Some(previous) = self.previous.clone() else { return Outcome::default() };
        self.change_edits(items, "Paste from Previous", "Edits Pasted", false, None, |_, target| EditGroup::merge(&EditGroup::standard(), &previous, target))
    }

    /// Removes the edits of `items`, crop and rotation included: they are
    /// as if never edited. Ratings and flags stay.
    pub fn reset_edits(&mut self, items: &[Item]) -> Outcome {
        self.change_edits(items, "Reset Edits", "Edits Reset", true, None, |_, target| target.clone())
    }

    /// Changes each of `items`' edits by `transform` as one undo step
    /// (`changeEdits`): the open one in place, the others in their
    /// sidecars. Use for whole roll prints them all as frames of one roll.
    pub fn change_each_edit(&mut self, items: &[Item], action: &str, note: &str, transform: impl Fn(&Item, &EditState) -> EditState) -> Outcome {
        self.change_edits(items, action, note, false, None, transform)
    }

    /// `item`'s edits as it opens with them (`openingState`); None if its
    /// sidecar can't be read.
    pub fn opening_state(&mut self, item: &Item) -> Option<EditState> {
        if let Some(open) = self.open.as_ref().filter(|o| o.item == *item) {
            return Some(open.state.clone());
        }
        if !self.sidecars {
            return Some(self.defaults.plain_state());
        }
        let restore = self.restore(&item.photo, item.variant.as_deref());
        if restore.problem.is_some() {
            return None;
        }
        Some(match restore.edits {
            None => self.defaults.plain_state(),
            Some(saved) => self.defaults.restored(&saved.state),
        })
    }

    /// A new id for a variant or a roll, as the library makes them.
    pub fn new_id(&mut self) -> String {
        self.ids.next()
    }

    /// Changes the edits of `items` as one undo step: the open one in
    /// place, the others in their sidecars. `from_unedited`: the groups
    /// pasted from a photograph without edits, which leave one without
    /// edits as it is, and one whose edits were all in them without any
    /// (`changeEdits`).
    fn change_edits(
        &mut self,
        items: &[Item],
        action: &str,
        note: &str,
        reset: bool,
        from_unedited: Option<BTreeSet<EditGroup>>,
        transform: impl Fn(&Item, &EditState) -> EditState,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        if items.is_empty() {
            return outcome;
        }
        let open_item = self.open.as_ref().map(|o| o.item.clone());
        if !self.sidecars {
            // Nothing is saved per photograph: only the open one changes.
            if let Some(open) = self.open.as_mut().filter(|o| items.contains(&o.item)) {
                let next = if reset { self.defaults.new_photo_state(&open.state) } else { transform(&open.item, &open.state) };
                open.state = next;
                outcome.open_changed = true;
            }
            let unsaved = items.iter().any(|i| Some(i) != open_item.as_ref()).then(|| "Not saved: Settings › Save ratings and edits is off".to_string());
            outcome.note = Some(Note { text: note.into(), detail: unsaved, count: items.len() });
            return outcome;
        }
        // The open photograph's own edits are in its sidecar before it's kept.
        let _ = self.save();
        let mut recorder = Recorder::default();
        let mut unsaved: Option<String> = None;
        let cant = "Not saved: its .negs file can’t be changed";
        for item in items {
            let photo = item.photo.clone();
            if self.read_only_photo(&photo) {
                unsaved.get_or_insert("Not saved: the folder is read-only".into());
                continue;
            }
            let raw = match sidecar::read_raw(&photo) {
                Ok(raw) => raw,
                Err(_) => {
                    unsaved.get_or_insert(cant.into());
                    continue;
                }
            };
            if Some(item) == open_item.as_ref() {
                let open = self.open.as_ref().expect("open");
                if open.blocked.is_some() {
                    unsaved.get_or_insert(cant.into());
                    continue;
                }
                // Nothing pasted onto nothing.
                if from_unedited.is_some() && !self.open_edited() {
                    continue;
                }
                let pasted = transform(item, &open.state);
                let cleared = from_unedited.as_ref().is_some_and(|g| self.defaults.holds_no_edits(&pasted, g));
                recorder.touch(&photo);
                outcome.open_changed = true;
                if reset || cleared {
                    // Without edits it shows what a new photograph does now.
                    let next = self.defaults.new_photo_state(&open.state);
                    let patch = Patch { primary: open.variant_is_new || open.item.variant.is_none(), edits: Some(None), ..Patch::new(&open.variant, open.defaults_version, &self.app) };
                    let open = self.open.as_mut().expect("open");
                    open.state = next;
                    open.written = None;
                    open.edited_at_open = false;
                    open.opened = open.state.user_edits();
                    if let Err(reason) = self.write(&photo, &patch) {
                        self.failed_write(&photo, reason, &mut outcome);
                    }
                } else {
                    self.open.as_mut().expect("open").state = pasted;
                    if let Err(error) = self.save() {
                        outcome.error.get_or_insert(error);
                    }
                }
                continue;
            }
            let found = match raw.map(Sidecar::from_raw).transpose() {
                Ok(found) => found,
                Err(_) => {
                    unsaved.get_or_insert(cant.into());
                    continue;
                }
            };
            let variant = match &item.variant {
                Some(id) => found.as_ref().and_then(|s| s.variant(id)),
                None => found.as_ref().and_then(|s| s.variants.first()),
            }
            .cloned();
            // A duplicate removed meanwhile.
            if item.variant.is_some() && variant.is_none() {
                continue;
            }
            // Nothing pasted onto nothing.
            if from_unedited.is_some() && variant.as_ref().is_none_or(|v| v.edits.is_none()) {
                continue;
            }
            let version = found.as_ref().map(|s| s.defaults_version).unwrap_or(LATEST_DEFAULTS);
            let Ok(table) = self.tables.get(version) else {
                unsaved.get_or_insert(cant.into());
                continue;
            };
            let mut edits = None;
            if !reset {
                let saved = match variant.as_ref().and_then(|v| v.edits.as_ref()).map(|e| decode_edits(e, &table)).transpose() {
                    Ok(saved) => saved,
                    Err(_) => {
                        unsaved.get_or_insert(cant.into());
                        continue;
                    }
                };
                let base = match saved {
                    Some(saved) => self.defaults.restored(&saved.state),
                    None => self.defaults.plain_state(),
                };
                let pasted = transform(item, &base);
                // Its match, if on, is solved again when it opens.
                if !from_unedited.as_ref().is_some_and(|g| self.defaults.holds_no_edits(&pasted, g)) {
                    edits = Some(encode_edits(&pasted, false, &table));
                }
            }
            recorder.touch(&photo);
            let id = variant.as_ref().map(|v| v.id.clone()).unwrap_or_else(|| self.ids.next());
            let patch = Patch {
                primary: item.variant.is_none(),
                edits: Some(edits),
                known: table.known(Some(&self.defaults.params)),
                ..Patch::new(id, version, &self.app)
            };
            if let Err(reason) = self.write(&photo, &patch) {
                self.failed_write(&photo, reason, &mut outcome);
            }
        }
        self.finish(recorder, action, StepKind::Sidecars);
        outcome.note = Some(Note { text: note.into(), detail: unsaved, count: items.len() });
        outcome
    }

    /// After changing several sidecars: the library shows them, and one
    /// undo step puts them back (`finishSidecarChange`).
    fn finish(&mut self, recorder: Recorder, action: &str, kind: StepKind) {
        for photo in recorder.photos() {
            self.reload(&photo);
        }
        let files = recorder.finish();
        if !files.is_empty() {
            self.push(Step { action: action.into(), kind, files });
        }
    }

    /// Duplicate: a virtual copy of each item, a new variant in its sidecar
    /// with the same edits, rating and flag, listed after the others. The
    /// grid selects the copies (`select`); the edit view opens the first.
    pub fn duplicate(&mut self, items: &[Item]) -> Outcome {
        let mut outcome = Outcome::default();
        if items.is_empty() {
            return outcome;
        }
        if !self.sidecars {
            outcome.note = Some(Note {
                text: "Not Duplicated".into(),
                detail: Some("Duplicates are kept in .negs files, which Settings › Save ratings and edits turns off".into()),
                count: 1,
            });
            return outcome;
        }
        let _ = self.save();
        let mut recorder = Recorder::default();
        let mut made: Vec<Item> = Vec::new();
        let mut unsaved: Option<String> = None;
        for item in items {
            let photo = item.photo.clone();
            if self.read_only_photo(&photo) {
                unsaved.get_or_insert("Not saved: the folder is read-only".into());
                continue;
            }
            let Ok(raw) = sidecar::read_raw(&photo) else {
                unsaved.get_or_insert("Not saved: its .negs file can’t be changed".into());
                continue;
            };
            recorder.touch(&photo);
            let mut top = raw.as_ref().and_then(Json::as_object).cloned().unwrap_or_else(|| {
                BTreeMap::from([("negs".to_string(), Json::from(SCHEMA)), ("defaultsVersion".to_string(), Json::from(LATEST_DEFAULTS))])
            });
            let mut variants = top.get("variants").and_then(Json::as_array).cloned().unwrap_or_default();
            if variants.is_empty() {
                // The photograph's own variant first, with the id the open one already uses.
                let id = match self.open.as_ref().filter(|o| o.item == *item) {
                    Some(open) => open.variant.clone(),
                    None => self.ids.next(),
                };
                variants.push(Json::from_pairs([("id", Json::string(id))]));
            }
            let original = item.variant.as_ref().and_then(|id| variants.iter().position(|v| v.get("id").as_str() == Some(id))).unwrap_or(0);
            let taken: HashSet<String> = variants.iter().filter_map(|v| v.get("id").as_str().map(str::to_string)).collect();
            let mut id = self.ids.next();
            while taken.contains(&id) {
                id = self.ids.next();
            }
            let mut copy = BTreeMap::from([("id".to_string(), Json::string(&id))]);
            // Edits, rating and flag, as Lightroom's virtual copies; from here on its own.
            for key in ["edits", "rating", "flag"] {
                if let Some(value) = variants[original].as_object().and_then(|o| o.get(key)) {
                    copy.insert(key.into(), value.clone());
                }
            }
            variants.push(Json::Object(copy));
            top.insert("variants".into(), Json::Array(variants));
            top.insert("app".into(), Json::string(&self.app));
            let patch = Patch { replacement: Some(Some(Json::Object(top))), ..Patch::new(&id, LATEST_DEFAULTS, &self.app) };
            match self.write(&photo, &patch) {
                Ok(()) => made.push(Item::duplicate(&photo, id)),
                Err(reason) => self.failed_write(&photo, reason, &mut outcome),
            }
        }
        let action = if made.len() > 1 { "Duplicate Photographs" } else { "Duplicate" };
        self.finish(recorder, action, StepKind::Sidecars);
        outcome.note = Some(Note { text: if made.is_empty() { "Not Duplicated" } else { "Duplicated" }.into(), detail: unsaved, count: made.len().max(1) });
        outcome.select = made;
        outcome
    }

    // MARK: Deleting

    /// What deleting `items` does.
    pub fn delete_plan(&self, items: &[Item]) -> DeletePlan {
        DeletePlan::new(items, |p| self.duplicates(p).to_vec(), |p| self.companion(p).map(Path::to_path_buf))
    }

    /// The alert before deleting `plan` (always ask when it is permanent).
    pub fn delete_question(&self, plan: &DeletePlan) -> crate::delete::Question {
        plan.question(|i| self.name(i), self.delete_is_permanent(plan), self.bin.name())
    }

    /// Whether a delete of `plan` would be for good: its folder's volume
    /// has no bin. Then always ask first.
    pub fn delete_is_permanent(&self, plan: &DeletePlan) -> bool {
        !plan.files.is_empty() && !self.bin.available(&self.folder)
    }

    /// Moves `plan`'s photographs to the bin with their sidecars (and their
    /// camera JPEGs), and removes the rest from theirs, as one undo step. A
    /// read-only folder deletes nothing.
    pub fn delete(&mut self, plan: &DeletePlan) -> Outcome {
        let mut outcome = Outcome::default();
        if plan.is_empty() {
            return outcome;
        }
        if self.read_only_photo(&self.folder.join("x")) {
            outcome.error = Some(format!("Nothing was deleted: the folder “{}” is read-only.", file_name(&self.folder)));
            return outcome;
        }
        let permanent = self.delete_is_permanent(plan);
        let (step, outcome) = self.delete_now(plan, permanent);
        if let Some(step) = step {
            self.push(step);
        }
        outcome
    }

    fn delete_now(&mut self, plan: &DeletePlan, permanent: bool) -> (Option<Step>, Outcome) {
        let mut outcome = Outcome::default();
        // The open photograph's last edits go to its sidecar first, and so
        // to the bin with it, or into what Undo puts back.
        let _ = self.save();
        let mut gone: HashSet<Item> = plan.items.iter().cloned().collect();
        for file in &plan.files {
            for id in self.duplicates(file).to_vec() {
                gone.insert(Item::duplicate(file, id));
            }
        }
        // The open one goes, or is renamed as its photograph's place is
        // taken: it opens again afterwards, and Undo opens it as it was.
        let open = self.open.as_ref().map(|o| o.item.clone());
        let renamed = open.as_ref().map(|o| plan.after(o)).filter(|after| Some(after) != open.as_ref());
        let reopen = open.filter(|o| gone.contains(o) || renamed.is_some());
        if reopen.is_some() {
            self.open = None;
            outcome.open_gone = true;
            outcome.open = renamed;
        }
        // What is chosen of a photograph that stays leaves its sidecar: a
        // duplicate, or the photograph's own variant, the first.
        let mut recorder = Recorder::default();
        if self.sidecars {
            for item in &plan.duplicates {
                let result = sidecar::read_raw(&item.photo).map_err(|p| p.message(&item.file_name())).and_then(|raw| match raw {
                    None => Ok(None),
                    Some(raw) => {
                        let first = raw.get("variants").as_array().and_then(|v| v.first()).and_then(|v| v.get("id").as_str()).map(str::to_string);
                        let Some(id) = item.variant.clone().or(first) else { return Ok(None) };
                        removing_variant(&id, &raw).map(Some).map_err(|p| p.message(&item.file_name()))
                    }
                });
                match result {
                    Ok(None) => {}
                    Ok(Some(removed)) => {
                        recorder.touch(&item.photo);
                        if let Err(e) = sidecar::write_raw(&item.photo, removed.as_ref()) {
                            outcome.error.get_or_insert(format!("“{}” couldn’t be removed: {e}", self.name(item)));
                        }
                    }
                    Err(_) => {
                        outcome.error.get_or_insert(format!("“{}” couldn’t be removed: its .negs file can’t be changed.", self.name(item)));
                    }
                }
            }
            for photo in recorder.photos() {
                self.reload(&photo);
            }
        }
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        for file in &plan.files {
            for path in plan.files_of(file) {
                let result = if permanent { self.bin.remove(&path) } else { self.bin.trash(&path).map(|landed| moves.push((path.clone(), landed))) };
                if let Err(e) = result {
                    outcome.error.get_or_insert(format!("“{}” couldn’t be deleted. {e}", file_name(file)));
                    break;
                }
            }
        }
        let files = recorder.finish();
        self.refresh(false);
        let kept = if permanent { plan.without_files() } else { plan.clone() };
        let step = (!permanent || !files.is_empty()).then(|| Step {
            action: plan.action_name(self.bin.name()),
            kind: StepKind::Delete { plan: kept, moves, reopen },
            files,
        });
        (step, outcome)
    }

    // MARK: Undo

    /// Undoes the last library action. Undone at once, it leaves every
    /// file byte for byte as it was. Ratings changed elsewhere meanwhile are
    /// merged with; edits are put back as they were.
    pub fn undo(&mut self) -> Option<Outcome> {
        let step = self.undo.pop()?;
        let (inverse, outcome) = self.apply_inverse(step);
        self.redo.push(inverse);
        Some(outcome)
    }

    pub fn redo(&mut self) -> Option<Outcome> {
        let step = self.redo.pop()?;
        let (inverse, outcome) = self.apply_inverse(step);
        self.undo.push(inverse);
        Some(outcome)
    }

    /// Puts `step` back and returns the step that does it again.
    fn apply_inverse(&mut self, step: Step) -> (Step, Outcome) {
        let mut outcome = Outcome::default();
        let _ = self.save();
        match step.kind {
            StepKind::Summaries(targets) => {
                let mut recorder = Recorder::default();
                let mut restored: HashSet<PathBuf> = HashSet::new();
                // A file still as the step left it goes back byte for byte.
                for change in &step.files {
                    if sidecar::read_bytes(&change.photo).ok().flatten() == change.after {
                        recorder.touch(&change.photo);
                        if sidecar::write_bytes(&change.photo, change.before.as_deref()).is_ok() {
                            restored.insert(change.photo.clone());
                        }
                    }
                }
                let items: HashSet<Item> = self.items().into_iter().collect();
                let mut befores: Vec<(Item, Summary, Summary)> = Vec::new();
                for (item, was, became) in &targets {
                    if !items.contains(item) {
                        continue;
                    }
                    let before = self.summary(item);
                    let mut after = before.clone();
                    if was.rating != became.rating {
                        after.rating = was.rating;
                    }
                    if was.flag != became.flag {
                        after.flag = was.flag;
                    }
                    befores.push((item.clone(), before.clone(), after.clone()));
                    if restored.contains(&item.photo) || after == before {
                        continue;
                    }
                    // Changed elsewhere meanwhile: merged with the file as it is.
                    self.summaries.insert(item.clone(), after.clone());
                    recorder.touch(&item.photo);
                    let _ = self.save_summary(&before, &after, item, &mut outcome);
                }
                for photo in recorder.photos() {
                    self.reload_keeping_unsaved(&photo);
                }
                let files = recorder.finish();
                (Step { action: step.action, kind: StepKind::Summaries(befores), files }, outcome)
            }
            StepKind::Sidecars => {
                let files = self.put_back(&step.files, &mut outcome);
                (Step { action: step.action, kind: StepKind::Sidecars, files }, outcome)
            }
            StepKind::Delete { plan, moves, reopen } => {
                for (original, trashed) in moves.iter().rev() {
                    if let Err(e) = self.bin.put_back(trashed, original) {
                        outcome.error.get_or_insert(format!("“{}” couldn’t be put back. {e}", file_name(original)));
                    }
                }
                self.put_back(&step.files, &mut outcome);
                self.refresh(false);
                let restored: Vec<Item> = plan.items.iter().filter(|i| i.photo.exists()).cloned().collect();
                if let Some(reopen) = reopen.filter(|r| r.photo.exists()) {
                    outcome.open = Some(reopen);
                }
                outcome.select = restored;
                (Step { action: step.action, kind: StepKind::Redelete { plan }, files: Vec::new() }, outcome)
            }
            StepKind::Redelete { plan } => {
                let permanent = self.delete_is_permanent(&plan);
                let (again, delete_outcome) = self.delete_now(&plan, permanent);
                let again = again.unwrap_or(Step { action: step.action, kind: StepKind::Sidecars, files: Vec::new() });
                (again, delete_outcome)
            }
        }
    }

    /// Writes each file back as it was before `changes` (`restoreSidecars`):
    /// the open photograph shows its edits as restored, or its photograph
    /// opens if it was a duplicate that's gone. A file another app has
    /// since made unreadable to this version is left alone. Returns the
    /// changes that would do them again.
    fn put_back(&mut self, changes: &[FileChange], outcome: &mut Outcome) -> Vec<FileChange> {
        let mut recorder = Recorder::default();
        for change in changes {
            let current = sidecar::read_bytes(&change.photo).ok().flatten();
            if let Some(bytes) = &current {
                if current != change.after && Sidecar::from_bytes(bytes).is_err() {
                    outcome.error.get_or_insert(format!("The .negs file of {} changed and is left as it is.", file_name(&change.photo)));
                    continue;
                }
            }
            recorder.touch(&change.photo);
            if let Err(e) = sidecar::write_bytes(&change.photo, change.before.as_deref()) {
                outcome.error.get_or_insert(format!("Edits to “{}” couldn’t be put back: {e}", file_name(&change.photo)));
            }
            self.reload(&change.photo);
        }
        if let Some(open) = self.open.clone() {
            if recorder.photos().contains(&open.item.photo) {
                let duplicate_gone = open.item.variant.as_ref().is_some_and(|id| !self.duplicates(&open.item.photo).contains(id));
                if duplicate_gone {
                    self.open = None;
                    outcome.open_gone = true;
                    outcome.open = Some(Item::photo(&open.item.photo));
                } else {
                    let restore = self.restore(&open.item.photo, open.item.variant.as_deref());
                    self.adopt(&open.item, restore, &open.state);
                    outcome.open_changed = true;
                }
            }
        }
        recorder.finish()
    }
}
