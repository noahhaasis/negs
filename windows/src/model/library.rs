//! The library glue: the open photograph's folder as a `negs_library::Library`
//! (its photographs, ratings, flags and per-photograph edits in `.negs`
//! sidecars), driven from the editor as `EditorModel`'s library half does
//! on the Mac. The library crate does the files; this decides when, and
//! keeps the editor in step: the open photograph's edits go to its sidecar a
//! second after the last change, on leaving it and at quit; the library's
//! undo steps share the editor's undo stack; another app's changes show when
//! the folder changes or the window is activated.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use negs_library::{selection, Config, EditGroup, Filter, Flag, Item, Library, Note, Outcome, Selection};

use super::library_map::{self as map, LibraryKey, Step};
use super::{Model, dirty};
use crate::session::{EditorSession, same_path};
use crate::ui;

/// The open folder's library, and what the editor keeps about it.
pub struct Shelf {
    pub library: Option<Library>,
    /// The strip's selection: what Paste Edits, Reset Edits and Duplicate act on.
    pub selection: Selection,
    watch: Option<ui::folder_watch::FolderWatch>,
    /// Counts libraries, so a watcher's late news for an old folder is dropped.
    generation: u64,
    /// The item `open_item` asked for, which `open_restoring` opens.
    opening: Option<Item>,
    /// A photograph opened before the catalog: the library opens it when the
    /// catalog arrives, as the Mac's `readoptSidecarEdits` does.
    pending: bool,
    /// The edits a session from before the library kept, for `pending`.
    legacy: Option<crate::state::EditState>,
    /// The open item whose edits the editor couldn't show: never synced.
    blocked: Option<Item>,
    /// The newest defaults table's film and paper, read once.
    table_stocks: std::cell::OnceCell<(String, String)>,
    save_due: Option<Instant>,
    save_timer: bool,
    /// The note in the status line ("★★★", "Edits Pasted"), and its serial.
    pub note: Option<(Note, u64)>,
    serial: u64,
    /// Off in the Windows test suite, whose photographs are scratch files.
    pub enabled: bool,
    /// The grid shows: ratings, flags and edits act on its selection.
    pub in_grid: bool,
}

impl Default for Shelf {
    fn default() -> Self {
        Self {
            library: None,
            selection: Selection::default(),
            watch: None,
            generation: 0,
            opening: None,
            pending: false,
            legacy: None,
            blocked: None,
            table_stocks: std::cell::OnceCell::new(),
            save_due: None,
            save_timer: false,
            note: None,
            serial: 0,
            enabled: !cfg!(test),
            in_grid: false,
        }
    }
}

/// The edits of a photograph left alone a second are saved.
const SAVE_DELAY: Duration = Duration::from_secs(1);

impl Model {
    // MARK: What the strip and menus show

    /// The library's open item: the photograph on screen, or its duplicate.
    pub fn library_item(&self) -> Option<Item> {
        self.shelf.library.as_ref()?.open_item().cloned()
    }

    /// What the strip lists: the filter's photographs, the open one always.
    pub fn shown_items(&self) -> Vec<Item> {
        let Some(library) = &self.shelf.library else { return vec![] };
        library.shown(library.open_item())
    }

    pub fn library_filter(&self) -> Filter {
        self.shelf.library.as_ref().map(|l| l.filter().clone()).unwrap_or_else(|| self.prefs.library.filter())
    }

    /// Rating and flag commands act on the open photograph only, as in the
    /// Mac's edit view: a forgotten selection never rates twenty photographs.
    pub fn rating_targets(&self) -> Vec<Item> {
        if self.shelf.in_grid {
            let targets = self.shelf.selection.ordered(&self.shown_items());
            if !targets.is_empty() {
                return targets;
            }
        }
        self.library_item().into_iter().collect()
    }

    /// Paste Edits, Paste from Previous, Reset Edits and Duplicate act on the
    /// strip's selection while it shows, else the open photograph.
    pub fn edit_targets(&self) -> Vec<Item> {
        let Some(open) = self.library_item() else { return vec![] };
        if !(self.prefs.filmstrip_visible || self.shelf.in_grid) || self.shelf.selection.selected().is_empty() {
            return vec![open];
        }
        let targets = self.shelf.selection.ordered(&self.shown_items());
        if targets.is_empty() { vec![open] } else { targets }
    }

    /// Library commands wait for the library, and for a crop to be done.
    pub fn can_change_library(&self) -> bool {
        self.shelf.library.is_some() && self.library_item().is_some() && !self.cropping() && self.ready
    }

    // MARK: Opening

    /// Opens a photograph or one of its duplicates.
    pub fn open_item(&mut self, item: Item) {
        let photo = item.photo.clone();
        self.shelf.opening = Some(item);
        self.open(&photo);
    }

    /// `open_restoring`'s first step: the photograph's folder becomes the
    /// library and the photograph opens in it, saving the one before. Gives
    /// the file to open (a camera JPEG's RAW opens in its place) and the
    /// session to restore it with: the edits its sidecar keeps.
    pub(super) fn library_opening(&mut self, path: &Path, session: Option<EditorSession>) -> (PathBuf, Option<EditorSession>) {
        let wanted = self.shelf.opening.take().filter(|i| same_path(&i.photo, path));
        if !self.shelf.enabled {
            return (path.to_path_buf(), session);
        }
        if !self.has_catalog() || self.paths.is_none() {
            // Opened with the defaults; its edits come with the catalog. A
            // session from before the library is kept to hand on then.
            self.shelf.pending = true;
            self.shelf.legacy = session.as_ref().filter(|s| !s.library && same_path(&s.source, path)).map(|s| s.state.clone());
            return (path.to_path_buf(), session);
        }
        self.shelf.pending = false;
        self.shelf.legacy = None;
        let Some(folder) = path.parent().map(Path::to_path_buf) else { return (path.to_path_buf(), session) };
        if !self.library_for(&folder) {
            return (path.to_path_buf(), session);
        }
        // The photograph before is saved, saying if it can't be.
        self.library_save();
        let library = self.shelf.library.as_mut().expect("library");
        // The listing's spelling of it; a camera JPEG goes with its RAW.
        let mut photo = library.files().iter().find(|f| same_path(&f.path, path)).map(|f| f.path.clone()).unwrap_or_else(|| path.to_path_buf());
        if !library.files().iter().any(|f| f.path == photo) {
            if let Some(raw) = library.files().iter().find(|f| library.companion(&f.path).is_some_and(|c| same_path(c, path))) {
                photo = raw.path.clone();
            }
        }
        let item = match wanted {
            Some(item) if item.variant.is_some() => Item::duplicate(&photo, item.variant.unwrap_or_default()),
            _ => Item::photo(&photo),
        };
        let opened = library.open(&item);
        let limit = self.prefs.limits_preview();
        self.shelf.blocked = None;
        let mut state = match map::from_library(&opened.state, limit) {
            Ok(state) => state,
            Err(error) => {
                // A crop or border this Negs can't read (from a newer Mac):
                // shown with the defaults, and its edits never written over.
                self.shelf.blocked = Some(item.clone());
                self.error = Some(format!("“{}” has edits this version of Negs can’t show ({error}). They are left as they are.", item.file_name()));
                self.mark(dirty::ERROR | dirty::STRIP);
                return (photo, None);
            }
        };
        if let Some(problem) = opened.problem.clone() {
            self.error = Some(problem);
            self.mark(dirty::ERROR);
        }
        // A session from before the library kept the edits itself: they come
        // to a photograph whose sidecar has none, and are saved there.
        // Never over a sidecar the library couldn't read.
        let legacy = session.as_ref().filter(|s| !s.library && same_path(&s.source, path)).map(|s| s.state.clone());
        if let Some(mut legacy) = legacy.filter(|_| opened.problem.is_none()) {
            legacy.limit_preview = state.limit_preview;
            let library = self.shelf.library.as_mut().expect("library");
            if !library.open_edited() && legacy != state {
                let roll = library.state().and_then(|s| s.roll.clone());
                library.edit(map::to_library(&legacy, roll));
                state = legacy;
                self.library_edited();
            }
        }
        if !self.shelf.selection.contains(&item) {
            self.shelf.selection.select(&item);
        }
        self.mark(dirty::STRIP);
        let restored = EditorSession {
            schema: EditorSession::SCHEMA,
            source: photo.clone(),
            state,
            inspector_visible: session.as_ref().map_or(self.inspector_visible, |s| s.inspector_visible),
            fit_image: session.as_ref().map_or(true, |s| s.fit_image),
            library: true,
        };
        (photo, Some(restored))
    }

    /// Makes `folder` the library's, closing the last one. False if it can't
    /// be (the defaults tables are missing): the photograph opens without.
    fn library_for(&mut self, folder: &Path) -> bool {
        if self.shelf.library.as_ref().is_some_and(|l| l.folder() == folder) {
            return true;
        }
        self.library_close();
        let Some(paths) = self.paths.clone() else { return false };
        let config = Config {
            data: paths.data.clone(),
            app: env!("CARGO_PKG_VERSION").into(),
            bin: Box::new(super::recycle_bin::RecycleBin),
            // Settings › Show camera JPEGs separately, off; Save ratings and
            // edits next to photos, on. Neither is in Windows' Settings yet.
            pairing: true,
            sidecars: true,
            id_seed: None,
        };
        let mut library = match Library::new(folder, config) {
            Ok(library) => library,
            Err(error) => {
                self.error = Some(error);
                self.mark(dirty::ERROR);
                return false;
            }
        };
        library.set_filter(self.prefs.library.filter());
        library.set_defaults(self.library_defaults(&library));
        self.shelf.library = Some(library);
        self.shelf.selection = Selection::default();
        self.shelf.generation += 1;
        // The last folder's library steps went with it.
        self.undo.undo.retain(|(s, _)| !s.is_library());
        self.undo.redo.retain(|(s, _)| !s.is_library());
        let generation = self.shelf.generation;
        self.shelf.watch = ui::folder_watch::FolderWatch::start(folder, move || {
            ui::post(move |app: &mut ui::App| {
                if app.model.shelf.generation == generation {
                    app.model.library_refresh();
                }
            });
        })
        .ok();
        self.mark(dirty::STRIP);
        true
    }

    /// What the library measures edits from: the catalog's defaults, films
    /// and papers, and Settings › New photographs.
    fn library_defaults(&self, library: &Library) -> negs_library::EditDefaults {
        let library_prefs = &self.prefs.library;
        let base = negs_library::EditDefaults {
            table_film: library.defaults().table_film.clone(),
            table_paper: library.defaults().table_paper.clone(),
            ..library.defaults().clone()
        };
        map::edit_defaults(
            &base,
            &self.defaults,
            &self.nullable_paths,
            &self.stocks,
            (library_prefs.new_photo_film.as_deref(), library_prefs.new_photo_paper.as_deref()),
            self.prefs.match_new_photos,
        )
    }

    /// The catalog changed (it arrived, or the helper's differed from the
    /// cached one): the library measures from it, and the open photograph
    /// shows its saved edits under it, or opens in the library now.
    pub(super) fn library_catalog_changed(&mut self) {
        if !self.shelf.enabled || !self.has_catalog() {
            return;
        }
        if self.shelf.pending {
            self.shelf.pending = false;
            let Some(source) = self.source.clone() else { return };
            let Some(folder) = source.parent().map(Path::to_path_buf) else { return };
            if !self.library_for(&folder) {
                return;
            }
        }
        let Some(library) = &self.shelf.library else { return };
        let defaults = self.library_defaults(library);
        let library = self.shelf.library.as_mut().expect("library");
        library.set_defaults(defaults);
        let item = match (library.open_item().cloned(), &self.source) {
            (Some(item), _) => item,
            (None, Some(source)) => Item::photo(library.files().iter().find(|f| same_path(&f.path, source)).map_or(source.clone(), |f| f.path.clone())),
            (None, None) => return,
        };
        let opened = library.open(&item);
        if let Some(problem) = opened.problem.clone() {
            self.error = Some(problem);
            self.mark(dirty::ERROR);
        }
        // A session from before the library: its edits go to a sidecar without any.
        if let Some(legacy) = self.shelf.legacy.take().filter(|_| opened.problem.is_none()) {
            let library = self.shelf.library.as_mut().expect("library");
            if !library.open_edited() && map::from_library(&opened.state, legacy.limit_preview).ok() != Some(legacy.clone()) {
                let roll = library.state().and_then(|s| s.roll.clone());
                library.edit(map::to_library(&legacy, roll));
            }
        }
        if !self.shelf.selection.contains(&item) {
            self.shelf.selection.select(&item);
        }
        self.show_library_state();
        if self.shelf.library.as_ref().is_some_and(Library::unsaved) {
            self.library_edited();
        }
        self.mark(dirty::STRIP);
    }

    /// The library replaced the open photograph's edits (Paste, Reset, Undo,
    /// another app's): the editor shows them, without its own history.
    fn show_library_state(&mut self) {
        let Some(edits) = self.shelf.library.as_ref().and_then(|l| l.state()).cloned() else { return };
        let Ok(next) = map::from_library(&edits, self.prefs.limits_preview()) else { return };
        self.cancel_brightness_match();
        self.brightness_failure = None;
        self.custom_film_format = false;
        self.replace_state(next);
        map::drop_editor_steps(&mut self.undo.undo);
        map::drop_editor_steps(&mut self.undo.redo);
        self.mark(dirty::CHROME | dirty::STRUCTURE | dirty::INSPECTOR);
    }

    // MARK: Saving the open photograph's edits

    /// The library has what the editor shows now, a crop being drawn included.
    pub(super) fn library_sync(&mut self) {
        let Some(library) = self.shelf.library.as_mut() else { return };
        let Some(open) = library.open_item() else { return };
        if !self.source.as_ref().is_some_and(|s| *s == open.photo) {
            return;
        }
        // Edits this editor can't show are never replaced by what it shows.
        if self.shelf.blocked.as_ref() == Some(open) {
            return;
        }
        // While the tray is open the stocks tried aren't edits yet: the
        // sidecar keeps the edit from before it until one is kept (`end_tray`).
        let mut state = self.previews.tray.as_ref().map_or_else(|| self.state.clone(), |t| t.before.clone());
        if let Some(session) = &self.crop_session {
            state.crop = session.settings();
        }
        let roll = library.state().and_then(|s| s.roll.clone());
        library.edit(map::to_library(&state, roll));
    }

    /// An edit: saved a second after the last one.
    pub(super) fn library_edited(&mut self) {
        if self.shelf.library.as_ref().and_then(Library::open_item).is_none() {
            return;
        }
        self.shelf.save_due = Some(Instant::now() + SAVE_DELAY);
        if !self.shelf.save_timer {
            self.shelf.save_timer = true;
            ui::after(SAVE_DELAY, |app: &mut ui::App| app.model.library_save_tick());
        }
    }

    fn library_save_tick(&mut self) {
        self.shelf.save_timer = false;
        let Some(due) = self.shelf.save_due else { return };
        let now = Instant::now();
        if due > now {
            self.shelf.save_timer = true;
            ui::after(due - now, |app: &mut ui::App| app.model.library_save_tick());
            return;
        }
        self.library_save();
    }

    /// At quit: writes the open photograph's edits, saying why it couldn't.
    pub fn library_save_now(&mut self) -> Result<(), String> {
        self.shelf.save_due = None;
        self.library_sync();
        match self.shelf.library.as_mut() {
            Some(library) => library.save().map_err(|e| format!("Negs stayed open: the edits couldn’t be saved. {e}")),
            None => Ok(()),
        }
    }

    /// Writes the open photograph's edits now, if they changed.
    pub fn library_save(&mut self) {
        self.shelf.save_due = None;
        self.library_sync();
        let Some(library) = self.shelf.library.as_mut() else { return };
        let was_edited = library.open_item().map(|i| library.summary(i).edited);
        if let Err(error) = library.save() {
            self.error = Some(error);
            self.mark(dirty::ERROR);
        }
        let library = self.shelf.library.as_ref().expect("library");
        if library.open_item().map(|i| library.summary(i).edited) != was_edited {
            self.mark(dirty::STRIP);
        }
    }

    /// Saves and closes the library before another folder's.
    pub fn library_close(&mut self) {
        self.library_close_saying(true);
    }

    /// At quit: a sidecar that can't be written is left unsaid.
    pub fn library_close_quietly(&mut self) {
        self.library_close_saying(false);
    }

    fn library_close_saying(&mut self, say: bool) {
        self.library_sync();
        self.shelf.save_due = None;
        self.shelf.watch = None;
        if let Some(mut library) = self.shelf.library.take() {
            if let Err(error) = library.close() {
                if say && !self.closing {
                    self.error = Some(error);
                    self.mark(dirty::ERROR);
                }
            }
        }
    }

    // MARK: Other apps' changes

    /// The folder changed, or the window was activated: the library lists it
    /// again and reads what other apps (another Mac over a share) wrote.
    pub fn library_refresh(&mut self) {
        if self.shelf.library.is_none() || self.closing {
            return;
        }
        self.library_sync();
        let interacting = self.interacting() || self.cropping();
        let library = self.shelf.library.as_mut().expect("library");
        let (_, outcome) = library.refresh(interacting);
        self.library_outcome(outcome, None);
        self.prune_selection();
        self.mark(dirty::STRIP);
    }

    fn prune_selection(&mut self) {
        let shown: HashSet<Item> = self.shown_items().into_iter().collect();
        self.shelf.selection.prune(&shown);
    }

    /// Shows what a library action did: its note, an error, the open
    /// photograph's new edits, or another photograph where it went.
    /// `next` is where the edit view goes if the open one is gone.
    fn library_outcome(&mut self, outcome: Outcome, next: Option<Item>) {
        if let Some(note) = outcome.note {
            self.show_note(note);
        }
        if let Some(error) = outcome.error {
            self.error = Some(error);
            self.mark(dirty::ERROR);
        }
        if outcome.open_changed {
            self.show_library_state();
        }
        if !outcome.select.is_empty() {
            self.shelf.selection.select_all_of(&outcome.select);
        }
        if let Some(open) = outcome.open {
            self.open_item(open);
        } else if outcome.open_gone {
            match next {
                Some(next) => self.open_item(next),
                None => self.close_photograph(),
            }
        }
        self.mark(dirty::STRIP | dirty::CHROME);
    }

    fn show_note(&mut self, note: Note) {
        self.shelf.serial += 1;
        let serial = self.shelf.serial;
        let shown = map::note_duration(&note);
        self.shelf.note = Some((note, serial));
        self.mark(dirty::STATUS);
        ui::after(shown, move |app: &mut ui::App| {
            if app.model.shelf.note.as_ref().is_some_and(|(_, s)| *s == serial) {
                app.model.shelf.note = None;
                app.model.mark(dirty::STATUS);
            }
        });
    }

    /// The open photograph went (deleted, or its duplicate) and nothing is
    /// left to show: the empty window, with the library kept for Undo.
    fn close_photograph(&mut self) {
        self.cancel_brightness_match();
        self.drop_held("close");
        self.source = None;
        self.image = None;
        self.placeholder = None;
        self.original = None;
        self.original_cache = None;
        self.original_loading = false;
        self.holding_original = false;
        self.shown_frame = None;
        self.histogram = None;
        self.waveform = None;
        self.crop_session = None;
        self.interaction_start = None;
        self.render_failed = false;
        self.develop_error = None;
        self.source_generation += 1;
        map::drop_editor_steps(&mut self.undo.undo);
        map::drop_editor_steps(&mut self.undo.redo);
        self.status.clear();
        self.mark(dirty::ALL & !dirty::ERROR);
    }

    // MARK: Library actions, each one undo step

    /// Runs `action` on the library and keeps its undo step in the editor's
    /// stack, with the photograph open before and after it.
    fn library_action(&mut self, action: impl FnOnce(&mut Library) -> Outcome, next: Option<Item>) -> bool {
        if !self.can_change_library() {
            return false;
        }
        self.library_save();
        let before = self.library_item();
        let library = self.shelf.library.as_mut().expect("library");
        let (depth, _) = library.undo_depth();
        let outcome = action(library);
        let stepped = library.undo_depth().0 > depth;
        let name = library.undo_name().unwrap_or("Change").to_string();
        if stepped {
            self.undo.undo.push((Step::Library { before: before.clone(), after: before }, name.into()));
            self.undo.redo.clear();
        }
        self.library_outcome(outcome, next);
        self.update_library_step(stepped);
        stepped
    }

    /// The last library step ends where the edit view is now.
    fn update_library_step(&mut self, stepped: bool) {
        if !stepped {
            return;
        }
        let now = self.library_item();
        if let Some((Step::Library { after, .. }, _)) = self.undo.undo.iter_mut().rev().find(|(s, _)| s.is_library()) {
            *after = now;
        }
        self.mark(dirty::CHROME);
    }

    /// A rating or flag key, or its menu item.
    pub fn library_key(&mut self, key: LibraryKey) {
        let targets = self.rating_targets();
        if targets.is_empty() || !self.can_change_library() {
            return;
        }
        let order = self.shown_items();
        let stepped = self.library_action(
            |library| match key {
                LibraryKey::Rate(stars) => library.set_rating(&targets, stars),
                LibraryKey::Raise => library.change_rating(&targets, 1),
                LibraryKey::Lower => library.change_rating(&targets, -1),
                LibraryKey::Pick => library.set_flag(&targets, Some(Flag::Pick)),
                LibraryKey::Reject => library.set_flag(&targets, Some(Flag::Reject)),
                LibraryKey::Unflag => library.set_flag(&targets, None),
            },
            None,
        );
        self.move_on(&targets, &order);
        self.update_library_step(stepped);
    }

    /// After rating or flagging: a photograph the filter now leaves out goes
    /// from the strip, and the next one shown opens (or the one before at
    /// the end). Advance After Rating opens the next one anyway (`moveOn`).
    fn move_on(&mut self, targets: &[Item], order: &[Item]) {
        let Some(library) = &self.shelf.library else { return };
        let filter = library.filter().clone();
        let dropped: HashSet<Item> = if filter.active() { targets.iter().filter(|i| !filter.includes(&library.summary(i))).cloned().collect() } else { HashSet::new() };
        if let Some(current) = self.library_item() {
            if dropped.contains(&current) {
                // With none left the photograph stays, as the strip's only one.
                if let Some(next) = selection::next_after(&current, order, &dropped) {
                    self.open_item(next);
                }
                return;
            }
        }
        self.prune_selection();
        if self.prefs.library.advance_after_rating {
            self.step_photo(1);
        }
    }

    /// Opens the photograph `by` places along the strip.
    pub fn step_photo(&mut self, by: isize) {
        if self.cropping() {
            return;
        }
        let order = self.shown_items();
        let Some(current) = self.library_item() else { return };
        let Some(index) = order.iter().position(|i| *i == current) else { return };
        let next = index as isize + by;
        if next >= 0 && (next as usize) < order.len() {
            let item = order[next as usize].clone();
            self.shelf.selection.select(&item);
            self.open_item(item);
        }
    }

    /// Copy Edits, or Copy Edits… with the groups chosen (remembered).
    pub fn copy_edits(&mut self, groups: Option<BTreeSet<EditGroup>>) {
        let Some(item) = self.library_item().filter(|_| self.can_change_library()) else { return };
        if let Some(groups) = &groups {
            self.prefs.library.set_copy_groups(groups);
            self.prefs.save();
        }
        self.library_save();
        let library = self.shelf.library.as_mut().expect("library");
        let outcome = library.copy_edits(&item, groups.unwrap_or_else(EditGroup::standard));
        self.library_outcome(outcome, None);
    }

    pub fn can_paste_edits(&self) -> bool {
        self.can_change_library() && self.shelf.library.as_ref().is_some_and(|l| l.copied().is_some())
    }

    pub fn can_paste_from_previous(&self) -> bool {
        self.can_change_library() && self.shelf.library.as_ref().is_some_and(|l| l.previous_edits().is_some())
    }

    pub fn paste_edits(&mut self) {
        let targets = self.edit_targets();
        self.library_action(|library| library.paste_edits(&targets), None);
    }

    pub fn paste_from_previous(&mut self) {
        let targets = self.edit_targets();
        self.library_action(|library| library.paste_from_previous(&targets), None);
    }

    /// Photo › Reset Edits: as if never edited, crop and rotation included.
    pub fn reset_photo_edits(&mut self) {
        let targets = self.edit_targets();
        self.library_action(|library| library.reset_edits(&targets), None);
    }

    /// Duplicate: the edit view opens the first copy (the library selects them).
    pub fn duplicate_photos(&mut self) {
        let targets = self.edit_targets();
        if self.library_action(|library| library.duplicate(&targets), None) {
            let made = self.shelf.selection.ordered(&self.shown_items());
            if let Some(first) = made.into_iter().find(|i| i.variant.is_some() && !targets.contains(i)) {
                self.open_item(first);
                self.update_library_step(true);
            }
        }
    }

    /// What Delete acts on: the open photograph (or duplicate), as in the
    /// Mac's edit view.
    pub fn delete_targets(&self) -> Vec<Item> {
        self.rating_targets()
    }

    pub fn rejected(&self) -> Vec<Item> {
        self.shelf.library.as_ref().map(Library::rejected).unwrap_or_default()
    }

    /// The question before deleting `items`, and whether it must be asked
    /// (always for a volume without a Recycle Bin).
    pub fn delete_question(&self, items: &[Item]) -> Option<(negs_library::DeletePlan, negs_library::Question, bool)> {
        let library = self.shelf.library.as_ref()?;
        let plan = library.delete_plan(items);
        if plan.is_empty() {
            return None;
        }
        let question = library.delete_question(&plan);
        let permanent = library.delete_is_permanent(&plan);
        Some((plan, question, permanent))
    }

    /// Deletes `plan` as one undo step; the edit view goes to the next photograph.
    pub fn delete_photos(&mut self, plan: negs_library::DeletePlan) {
        let order = self.shown_items();
        let mut gone: HashSet<Item> = plan.items.iter().cloned().collect();
        if let Some(library) = &self.shelf.library {
            for file in &plan.files {
                gone.insert(Item::photo(file));
                for id in library.duplicates(file) {
                    gone.insert(Item::duplicate(file, id.as_str()));
                }
            }
        }
        // The next one shown, else the next in the folder (the filter hides the rest).
        let everything = self.shelf.library.as_ref().map(Library::items).unwrap_or_default();
        let next = self.library_item().and_then(|current| selection::next_after(&current, &order, &gone).or_else(|| selection::next_after(&current, &everything, &gone)));
        self.library_action(|library| library.delete(&plan), next);
        self.prune_selection();
    }

    // MARK: Undo

    /// Undoes or redoes the library step on top of the stack. Undone at once,
    /// the files are as they were, and the edit view goes back to the
    /// photograph it was on.
    pub(super) fn library_undo(&mut self, before: Option<Item>, after: Option<Item>, action: std::borrow::Cow<'static, str>, undoing: bool) {
        self.library_save();
        let Some(library) = self.shelf.library.as_mut() else { return };
        let outcome = if undoing { library.undo() } else { library.redo() };
        // Both stacks hold the library's steps in the same order; one the
        // library no longer has goes.
        let Some(outcome) = outcome else { return };
        let step = Step::Library { before: before.clone(), after: after.clone() };
        if undoing { self.undo.redo.push((step, action)) } else { self.undo.undo.push((step, action)) }
        self.library_outcome(outcome, None);
        let (from, to) = if undoing { (after, before) } else { (before, after) };
        let current = self.library_item();
        if current == from && to != current {
            if let Some(to) = to.filter(|i| self.shown_items().contains(i) || i.photo.exists()) {
                self.open_item(to);
            }
        }
        self.prune_selection();
        self.mark(dirty::CHROME | dirty::STRIP);
    }

    // MARK: Filter and settings

    pub fn set_library_filter(&mut self, filter: Filter) {
        self.prefs.library.set_filter(&filter);
        self.prefs.save();
        if let Some(library) = self.shelf.library.as_mut() {
            library.set_filter(filter);
        }
        self.prune_selection();
        self.mark(dirty::STRIP | dirty::CHROME);
    }

    pub fn toggle_advance_after_rating(&mut self) {
        self.prefs.library.advance_after_rating = !self.prefs.library.advance_after_rating;
        self.prefs.save();
    }

    // MARK: Settings › New photographs

    /// The film and paper a photograph without edits opens with.
    pub fn new_photo_stocks(&self) -> (String, String) {
        if let Some(library) = &self.shelf.library {
            let d = library.defaults();
            return (d.new_photo_film.clone(), d.new_photo_paper.clone());
        }
        // The newest defaults table's, read once.
        if self.shelf.table_stocks.get().is_none() {
            let table = self.paths.as_ref().and_then(|p| negs_library::DefaultsTable::load(&p.data, negs_library::sidecar::LATEST_DEFAULTS).ok());
            if let Some(table) = table {
                let d = negs_library::EditDefaults::for_table(&table);
                let _ = self.shelf.table_stocks.set((d.table_film, d.table_paper));
            }
        }
        let tabled = self.shelf.table_stocks.get().map_or(("", ""), |(f, p)| (f.as_str(), p.as_str()));
        let chosen = &self.prefs.library;
        map::new_photo_stocks((chosen.new_photo_film.as_deref(), chosen.new_photo_paper.as_deref()), tabled, &self.stocks)
    }

    /// Settings' Film list: the films listed (Settings › Fotufilm films), in
    /// the film menu's groups, the chosen one always among them.
    pub fn new_photo_film_choices(&self) -> Vec<(String, String)> {
        let (film, _) = self.new_photo_stocks();
        let shown = self.fotufilm_films();
        let films: Vec<&crate::state::Stock> = self.stocks.iter().filter(|s| !s.paper && (shown.lists(s) || s.id == film)).collect();
        let papers = self.papers();
        crate::state::film_groups(&films, &papers).into_iter().flat_map(|g| g.stocks).map(|s| (s.id.clone(), s.name.clone())).collect()
    }

    /// Settings' Paper list, in the paper menu's groups.
    pub fn new_photo_paper_choices(&self) -> Vec<(String, String)> {
        crate::state::paper_groups(&self.papers()).into_iter().flat_map(|g| g.stocks).map(|s| (s.id.clone(), s.name.clone())).collect()
    }

    /// Whether the new photographs' film is a slide, scanned without a paper.
    pub fn new_photo_film_is_slide(&self) -> bool {
        let (film, _) = self.new_photo_stocks();
        self.stocks.iter().any(|s| !s.paper && s.id == film && s.positive)
    }

    /// Chooses the film new photographs open with; its own paper comes with
    /// it unless the paper lock is closed, as when choosing a photograph's
    /// film (`choose_film`).
    pub fn set_new_photo_film(&mut self, film: &str) {
        let (old_film, old_paper) = self.new_photo_stocks();
        let choice = crate::state::EditState { film: old_film, paper: old_paper, ..Default::default() };
        let chosen = crate::state::choosing(&choice, film, false, self.prefs.keep_paper, &self.stocks);
        self.set_new_photo_stocks(chosen.film, chosen.paper);
    }

    pub fn set_new_photo_paper(&mut self, paper: &str) {
        let (film, _) = self.new_photo_stocks();
        self.set_new_photo_stocks(film, paper.to_string());
    }

    /// Keeps the choice under the Mac's keys. The library measures from it,
    /// and an open photograph without edits of its own follows it, without
    /// an undo step and without writing anything (`followNewPhotoStocks`).
    fn set_new_photo_stocks(&mut self, film: String, paper: String) {
        if self.new_photo_stocks() == (film.clone(), paper.clone()) && self.prefs.library.new_photo_film.is_some() {
            return;
        }
        self.prefs.library.new_photo_film = Some(film);
        self.prefs.library.new_photo_paper = Some(paper);
        self.prefs.save();
        if self.shelf.library.is_none() {
            return;
        }
        self.library_sync();
        let library = self.shelf.library.as_ref().expect("library");
        let defaults = self.library_defaults(library);
        let outcome = self.shelf.library.as_mut().expect("library").set_defaults(defaults);
        if outcome.open_changed {
            self.show_library_state();
        }
        self.mark(dirty::STRIP | dirty::INSPECTOR);
    }

    /// The rating an export carries (`xmp:Rating`), from the open photograph.
    pub fn export_rating(&self) -> Option<i32> {
        let library = self.shelf.library.as_ref()?;
        negs_library::xmp::export_rating(&library.summary(library.open_item()?))
    }
}
