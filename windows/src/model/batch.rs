//! Running a batch export (the Mac's `exportBatch` and `runBatch`): each
//! photograph of the grid's selection with its edits as they were when
//! Export was chosen, one after another, on a worker thread. A photograph
//! whose Match camera brightness isn't solved for its edits is solved just
//! before it exports, in the batch's own helper, as opening it would solve
//! it (the match cache too), and the match is stored with its edits. The
//! journal keeps an unfinished batch for the next launch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::{App, Model, Output, dirty, is_raw, library_map as map};
use crate::batch::{self, Existing, Item, Journal, Options, Run, Status};
use crate::engine::{self, EngineClient, EnginePaths, ExportHandle, ExportRequest};
use crate::state::{self, EditState};
use crate::{json as js, ui};

/// What the UI thread holds of a running batch's worker.
#[derive(Default)]
pub struct Control {
    cancelled: AtomicBool,
    closing: AtomicBool,
    export: Mutex<Option<ExportHandle>>,
}

impl Control {
    fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst) || self.closing.load(Ordering::SeqCst)
    }
    fn stop(&self, closing: bool) {
        if closing {
            self.closing.store(true, Ordering::SeqCst);
        } else {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = &*self.export.lock().unwrap() {
            handle.cancel();
        }
    }
}

/// What a batch photograph's match is solved with, from the open editor.
#[derive(Clone)]
struct Setup {
    paths: EnginePaths,
    scratch: PathBuf,
    /// The defaults' Prep and print exposure: the solve's own exposure.
    prep: Value,
    print_exposure: Value,
    /// The preview's colour space, which the solve request carries as the
    /// open photograph's does, so the match cache serves both.
    preview_space: String,
    stamp: Option<String>,
}

impl Model {
    /// What Export acts on: the grid's selection, else the open photograph.
    pub fn export_targets(&self) -> Vec<negs_library::Item> {
        self.rating_targets()
    }

    /// Export is a single export: the edit view's photograph, or the open
    /// one alone selected in the grid.
    pub fn exports_single(&self) -> bool {
        !self.shelf.in_grid || self.library_item().is_some_and(|open| self.export_targets() == [open])
    }

    pub fn batch_running(&self) -> bool {
        self.batch.as_ref().is_some_and(|b| b.running)
    }

    /// One exporter at a time: not during a single export either.
    pub fn can_start_batch(&self) -> bool {
        !self.closing && !self.batch_running() && !self.exporting && !self.saving && self.ready && self.has_catalog() && self.engine.is_some() && !self.cropping()
    }

    /// Whether File › Export can start now, single or batch.
    pub fn can_export_any(&self) -> bool {
        if self.batch_running() {
            return false;
        }
        if self.exports_single() { self.can_export() } else { self.can_start_batch() && !self.export_targets().is_empty() }
    }

    /// "Export Photograph…", or "Export 12 Photographs…" for a grid selection.
    pub fn export_title(&self) -> String {
        let count = if self.exports_single() { 1 } else { self.export_targets().len() };
        if count > 1 { format!("Export {count} Photographs…") } else { "Export Photograph…".into() }
    }

    /// Each target's edits as they are now, and the name it gets.
    pub fn batch_items(&self, targets: &[negs_library::Item], options: &Options) -> Vec<Item> {
        let Some(library) = &self.shelf.library else { return vec![] };
        let open = self.library_item().filter(|_| self.source.is_some());
        let limit = self.prefs.limits_preview();
        let previous = library.state().cloned().unwrap_or_else(negs_library::EditState::empty);
        let mut items: Vec<Item> = targets
            .iter()
            .map(|target| {
                let raw = is_raw(&target.photo);
                let title = library.name(target);
                let mut item = Item {
                    photo: target.photo.clone(),
                    variant: target.variant.clone(),
                    title: title.clone(),
                    state: self.state.clone(),
                    needs_match: false,
                    stores_match: false,
                    rating: negs_library::xmp::export_rating(&library.summary(target)),
                    file_name: String::new(),
                    size: None,
                    look: None,
                    status: Status::Waiting,
                    reason: None,
                    written: None,
                };
                let matchable =
                    |s: &EditState| s.brightness_match.is_some() && (raw || js::string(s.value("io.input_interpretation.mode")) == "auto");
                if open.as_ref() == Some(target) {
                    // As on screen, its match settled.
                    let size = if self.image.is_some() { self.image_size } else { self.placeholder_size };
                    item.size = (!size.is_zero()).then_some((size.width, size.height));
                } else if let Some(stored) = library.export_edits(target) {
                    match map::from_library(&stored.state, limit) {
                        Ok(state) => item.state = state,
                        Err(error) => {
                            item.status = Status::Failed;
                            item.reason = Some(format!("Edited with settings this version of Negs can’t read ({error})."));
                        }
                    }
                    item.needs_match = !stored.match_solved && matchable(&item.state);
                    item.stores_match = true;
                    item.look = Some(stored.look);
                    if !stored.missing.is_empty() {
                        item.status = Status::Failed;
                        item.reason = Some(format!("Edited with {}, which this version of Negs doesn’t have.", stored.missing.join(" and ")));
                    }
                } else {
                    // Never edited: as opening it now would show it.
                    let fresh = library.defaults().new_photo_state(&previous);
                    if let Ok(state) = map::from_library(&fresh, limit) {
                        item.state = state;
                    }
                    item.needs_match = matchable(&item.state);
                }
                item.file_name = options.file_name(&batch::batch_name(&target.photo, &title), &item.state.film);
                item
            })
            .collect();
        let names = batch::unique(&items.iter().map(|i| i.file_name.clone()).collect::<Vec<_>>());
        for (item, name) in items.iter_mut().zip(names) {
            item.file_name = name;
        }
        items
    }

    /// Starts exporting `targets` to `folder` with their edits as they are
    /// now, once the open photograph's match has settled if it is one of
    /// them. `named` names a batch of one.
    pub fn export_batch(&mut self, targets: Vec<negs_library::Item>, folder: PathBuf, options: Options, named: Option<String>) {
        let open_is_target = self.library_item().is_some_and(|open| targets.contains(&open));
        let start = move |model: &mut Model| {
            model.start_export_batch(&targets, folder, options, named);
        };
        if open_is_target { self.after_brightness(Output::Export, start) } else { start(self) }
    }

    fn start_export_batch(&mut self, targets: &[negs_library::Item], folder: PathBuf, options: Options, named: Option<String>) -> bool {
        if !self.can_start_batch() || targets.is_empty() {
            return false;
        }
        let mut items = self.batch_items(targets, &options);
        if let (Some(named), [item]) = (named, items.as_mut_slice()) {
            item.file_name = named;
        }
        // Replacing a photograph's own file is refused, as for a single export.
        if options.existing == Existing::Replace && items.iter().any(|i| crate::platform::same_file(&folder.join(&i.file_name), &i.photo)) {
            self.error = Some("Choose a different folder or file name to preserve the original photographs.".into());
            self.mark(dirty::ERROR);
            return false;
        }
        self.start_batch(Journal::new(folder, options, items))
    }

    fn start_batch(&mut self, journal: Journal) -> bool {
        if !self.can_start_batch() {
            return false;
        }
        self.batch = Some(Run { journal, current: None, running: false, cancelled: false });
        self.launch_batch();
        true
    }

    fn launch_batch(&mut self) {
        let (Some(run), Some(engine), Some(paths)) = (self.batch.as_mut(), self.engine.clone(), self.paths.clone()) else { return };
        run.running = true;
        run.cancelled = false;
        let journal = run.journal.clone();
        let _ = journal.write(&self.batch_journal);
        let control = Arc::new(Control::default());
        self.batch_control = Some(control.clone());
        let setup = Setup {
            paths,
            scratch: engine.directory().to_path_buf(),
            prep: js::get(&self.defaults, "prep").clone(),
            print_exposure: js::get(&self.defaults, "enlarger.print_exposure").clone(),
            preview_space: self.preview_space().to_string(),
            stamp: self.match_engine_stamp(),
        };
        self.status = String::new();
        self.mark(dirty::STATUS | dirty::CHROME | dirty::STRIP);
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            run_batch(journal, setup, control);
        });
    }

    /// One photograph began (`index`), or ended (`item`).
    fn batch_progress(&mut self, index: usize, item: Option<Item>) {
        let path = self.batch_journal.clone();
        let Some(run) = self.batch.as_mut() else { return };
        match item {
            None => run.current = Some(index),
            Some(item) => {
                // A match solved for stored edits is stored with them.
                if item.stores_match && item.needs_match && item.status == Status::Done {
                    store_match(self, &item);
                }
                let Some(run) = self.batch.as_mut() else { return };
                if let Some(slot) = run.journal.items.get_mut(index) {
                    *slot = item;
                }
                let _ = run.journal.write(&path);
            }
        }
        self.mark(dirty::STATUS | dirty::STRIP);
        crate::ui::batch::refresh();
    }

    /// The worker is done: everything exported, cancelled, or quitting.
    fn batch_ended(&mut self) {
        self.batch_control = None;
        if self.closing {
            // The journal stays for the next launch.
            return;
        }
        let Some(run) = self.batch.as_mut() else { return };
        run.running = false;
        run.current = None;
        Journal::remove(&self.batch_journal);
        let run = self.batch.as_ref().unwrap();
        self.status = run.finished_words();
        crate::trace::event("batch_finished", || json!({"exported": run.exported(), "failed": run.failures().len(), "skipped": run.skipped(), "cancelled": run.cancelled}));
        let lightroom = run.journal.options.lightroom.as_deref().map(crate::lightroom::Kind::from_key);
        let written: Vec<PathBuf> = run.journal.items.iter().filter(|i| i.status == Status::Done).filter_map(|i| i.written.clone()).collect();
        let (failed, cancelled) = (run.failures().len(), run.cancelled);
        if !run.needs_attention() && !self.batch_popover_shown {
            self.batch = None;
        }
        self.mark(dirty::STATUS | dirty::CHROME | dirty::STRIP);
        crate::ui::batch::refresh();
        if let Some(kind) = lightroom {
            self.lightroom_batch_finished(kind, written, failed, cancelled);
        }
    }

    pub fn cancel_batch(&mut self) {
        if let (Some(run), Some(control)) = (self.batch.as_mut(), &self.batch_control) {
            if run.running {
                run.cancelled = true;
                control.stop(false);
                self.mark(dirty::STATUS);
            }
        }
    }

    /// At quit: the exporter stops, and the journal stays.
    pub(super) fn stop_batch_for_quit(&mut self) {
        if let Some(control) = &self.batch_control {
            control.stop(true);
        }
    }

    /// The failed photographs again.
    pub fn retry_batch(&mut self) {
        if self.closing || !self.can_start_batch() {
            return;
        }
        let Some(run) = self.batch.as_mut().filter(|r| !r.running) else { return };
        for item in &mut run.journal.items {
            if item.status == Status::Failed {
                item.status = Status::Waiting;
                item.reason = None;
            }
        }
        if run.journal.remaining() > 0 {
            self.launch_batch();
        }
    }

    /// Forgets a finished batch, once its popover is closed.
    pub fn clear_batch(&mut self) {
        if !self.batch_running() {
            self.batch = None;
            self.mark(dirty::STATUS | dirty::STRIP);
        }
    }

    /// The batch's files, or its folder.
    pub fn batch_written(&self) -> (PathBuf, Vec<PathBuf>) {
        let Some(run) = &self.batch else { return (PathBuf::new(), vec![]) };
        let written = run.journal.items.iter().filter_map(|i| i.written.clone()).filter(|p| p.exists()).collect();
        (run.journal.destination.clone(), written)
    }

    /// A batch the last session didn't finish: its journal, unless there's
    /// nothing left (then it goes).
    pub fn unfinished_batch(&self) -> Option<Journal> {
        if self.batch.is_some() || self.closing {
            return None;
        }
        let journal = Journal::read(&self.batch_journal)?;
        if journal.remaining() == 0 {
            Journal::remove(&self.batch_journal);
            return None;
        }
        Some(journal)
    }

    /// Resumes it (Resume), or forgets it (Discard). False if it couldn't start yet.
    pub fn resume_batch(&mut self, journal: Journal, resume: bool) -> bool {
        if !resume {
            Journal::remove(&self.batch_journal);
            return true;
        }
        if self.start_batch(journal) {
            return true;
        }
        self.error = Some("The export couldn’t resume yet. Try again once Negs has started.".into());
        self.mark(dirty::ERROR);
        false
    }
}

/// Stores a batch photograph's solved match with its edits, if they're
/// still the ones it was solved for (`store_solved_match`).
fn store_match(model: &mut Model, item: &Item) {
    let (Some(library), Some(look)) = (model.shelf.library.as_mut(), item.look.as_deref()) else { return };
    let target = item.library_item();
    let roll = library.export_edits(&target).and_then(|stored| stored.state.roll);
    if library.store_solved_match(&target, look, &map::to_library(&item.state, roll)) {
        model.mark(dirty::STRIP);
    }
}

/// The worker: what's waiting, one photograph at a time.
fn run_batch(journal: Journal, setup: Setup, control: Arc<Control>) {
    let mut helper: Option<EngineClient> = None;
    let generation = AtomicU64::new(0);
    for (index, item) in journal.items.iter().enumerate() {
        if control.stopped() {
            break;
        }
        if item.status != Status::Waiting {
            continue;
        }
        ui::post(move |app: &mut App| app.model.batch_progress(index, None));
        let done = export_item(item.clone(), &journal, &setup, &control, &mut helper, &generation);
        // Cancelled, or quitting: it stays waiting.
        if done.status == Status::Waiting {
            break;
        }
        ui::post(move |app: &mut App| app.model.batch_progress(index, Some(done)));
    }
    if let Some(helper) = helper {
        helper.stop();
    }
    ui::post(|app: &mut App| app.model.batch_ended());
}

/// One photograph: its match solved if it needs one, then the exporter.
/// Still waiting if it was cancelled.
fn export_item(mut item: Item, journal: &Journal, setup: &Setup, control: &Control, helper: &mut Option<EngineClient>, generation: &AtomicU64) -> Item {
    let failed = |mut item: Item, reason: String| {
        item.status = Status::Failed;
        item.reason = Some(reason);
        item
    };
    let folder = &journal.destination;
    let options = &journal.options;
    if !folder.is_dir() {
        let name = folder.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        return failed(item, format!("The folder “{name}” is missing."));
    }
    if !item.photo.exists() {
        return failed(item, "The photograph is missing.".into());
    }
    let mut name = item.file_name.clone();
    let taken = |name: &str| folder.join(name).exists();
    if taken(&name) {
        match options.existing {
            Existing::Skip => {
                item.status = Status::Skipped;
                item.reason = Some(format!("{name} already exists."));
                return item;
            }
            Existing::Number => name = batch::numbered(&name, taken),
            Existing::Replace => {}
        }
    }
    let destination = folder.join(&name);
    if crate::platform::same_file(&destination, &item.photo) {
        return failed(item, "It would replace the photograph’s own file.".into());
    }
    let mut state = item.state.clone();
    let mut size = item.size;
    let border = state.border;
    let needs_size = size.is_none() && border.enabled && border.export && border.style == crate::border::BorderStyle::Film35;
    if item.needs_match || needs_size {
        if helper.is_none() {
            match EngineClient::start(setup.paths.clone()) {
                Ok(started) => *helper = Some(started),
                Err(error) if needs_size => return failed(item, format!("{error:#}")),
                Err(_) => {}
            }
        }
        if let Some(engine) = helper.as_ref() {
            let id = generation.fetch_add(1, Ordering::SeqCst) + 1;
            match solve(engine, &item, setup, id) {
                Ok((solved, decoded)) => {
                    state = solved;
                    size = Some((decoded.0 as f64, decoded.1 as f64));
                }
                // Unmatched, as the photograph would open; a 35mm border needs the size.
                Err(error) if needs_size => return failed(item, format!("{error:#}")),
                Err(_) => {}
            }
        }
        if control.stopped() {
            return item;
        }
    }
    let executable = match options.export_engine().executable(&setup.paths) {
        Ok(executable) => executable,
        Err(error) => return failed(item, format!("{error:#}")),
    };
    let photo = state.crop.size_of(size.map_or(crate::crop::Size::ZERO, |(w, h)| crate::crop::Size { width: w, height: h }));
    let mut params = state.params.clone();
    js::set(&mut params, "settings.serial_grain", Value::Bool(options.low_memory));
    let request = ExportRequest {
        executable,
        engine: options.export_engine(),
        data: setup.paths.data.clone(),
        scratch: setup.scratch.clone(),
        source: item.photo.clone(),
        destination: destination.clone(),
        film: state.film.clone(),
        paper: state.paper.clone(),
        params,
        scan_film: js::boolean(state.value("io.scan_film")),
        crop: state.crop.cropped().then_some(state.crop),
        border: state.border.exported(photo.width as u32, photo.height as u32),
        size_arguments: options.size().arguments(),
        save_arguments: options.save().arguments(),
    };
    let handle = ExportHandle::default();
    *control.export.lock().unwrap() = Some(handle.clone());
    if control.stopped() {
        handle.cancel();
    }
    let outcome = engine::run_export(&request, &handle).and_then(|done| match item.rating {
        Some(rating) => negs_library::xmp::embed_in_file(&destination, rating).map(|_| done).map_err(anyhow::Error::from),
        None => Ok(done),
    });
    *control.export.lock().unwrap() = None;
    match outcome {
        Ok(_) => {
            item.state = state;
            item.status = Status::Done;
            item.written = Some(destination);
            item.reason = None;
            item
        }
        Err(error) if error.is::<engine::Cancelled>() || control.stopped() => item,
        Err(error) => failed(item, format!("{error:#}")),
    }
}

/// Solves a batch photograph's match in the batch's helper, as opening it
/// would (`reconcile_brightness_match`: the same request, reference and
/// match cache), and gives its size, which the decode reads. The state
/// comes back matched, or as it was if its match isn't needed.
fn solve(engine: &EngineClient, item: &Item, setup: &Setup, generation: u64) -> Result<(EditState, (u32, u32))> {
    let ask = |command: Value| -> Result<Value> {
        let name = command["command"].as_str().unwrap_or_default().to_string();
        let (sender, receiver) = std::sync::mpsc::channel();
        engine.request(command, engine::timeout_for(&name), move |result| {
            let _ = sender.send(result.map(|(_, v)| v));
        });
        receiver.recv()?
    };
    let source = &item.photo;
    let mut request = item.state.clone();
    js::set(&mut request.params, "io.output_color_space", Value::String(setup.preview_space.clone()));
    js::set(&mut request.params, "prep", setup.prep.clone());
    js::set(&mut request.params, "enlarger.print_exposure", setup.print_exposure.clone());
    let buffer = engine.directory().join("batch-decode.rgba");
    let decode = json!({
        "command": "thumbnail", "input": source, "source_generation": generation,
        "film": request.film, "paper": request.paper, "params": request.params,
        "long_side": 16, "preview_path": buffer, "reduced_decode": false, "decode_only": true,
    });
    let raw_named = is_raw(source);
    let (decoded, preview) = std::thread::scope(|scope| {
        let preview = (raw_named && item.needs_match).then(|| scope.spawn(|| crate::brightness::read_camera_preview(source)));
        let decoded = ask(decode);
        (decoded, preview.map(|thread| thread.join().unwrap_or_else(|_| Err(anyhow!("The camera preview couldn’t be read.")))))
    });
    let _ = std::fs::remove_file(&buffer);
    let decoded = decoded?;
    let size = (js::number(&decoded["source_width"]) as u32, js::number(&decoded["source_height"]) as u32);
    if !item.needs_match {
        return Ok((item.state.clone(), size));
    }
    let key = setup.stamp.as_ref().and_then(|stamp| crate::match_cache::key(source, &request, stamp));
    let kept = key.as_ref().and_then(|key| crate::match_cache::shared().solved(key));
    let (ev, limited) = match kept {
        Some(kept) => kept,
        None => {
            let png = crate::brightness::reference(source, js::boolean(&decoded["raw"]), preview, size)?;
            let reference = engine.directory().join(format!("batch-brightness-{generation}.png"));
            std::fs::write(&reference, png)?;
            let placed = ask(json!({
                "command": "place_exposure", "input": source, "source_generation": generation,
                "film": request.film, "paper": request.paper, "params": request.params, "reference_path": reference,
            }));
            let _ = std::fs::remove_file(&reference);
            let placed = placed?;
            let ev = placed["exposure_ev"].as_f64().filter(|ev| ev.is_finite() && (state::PREP_EXPOSURE_RANGE.0..=state::PREP_EXPOSURE_RANGE.1).contains(ev));
            let ev = ev.ok_or_else(|| anyhow!("No reliable brightness match with these settings."))?;
            let limited = js::boolean(&placed["highlight_limited"]);
            if let Some(key) = key.filter(|key| setup.stamp.as_ref().and_then(|stamp| crate::match_cache::key(source, &request, stamp)).as_ref() == Some(key)) {
                crate::match_cache::shared().store(key, ev, limited);
            }
            (ev, limited)
        }
    };
    let matched = batch::matched(&item.state, ev, limited, state::match_inputs(&item.state)).unwrap_or_else(|| item.state.clone());
    Ok((matched, size))
}
