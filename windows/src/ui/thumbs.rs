//! The strip's and grid's thumbnails (`PhotoThumbnails`, `EditedThumbnails`,
//! `EditorModel.pumpThumbnails`): each photograph's developed thumbnail
//! when there is one, else its embedded preview, read off the UI thread.
//!
//! - **Reading**: a pool of `readers` threads takes the shown photographs
//!   first, then ±32 around the middle of the view; each looks for the
//!   developed thumbnail filed under the photograph's key, then the last one
//!   saved under an older engine (a stand-in, developed again), else reads
//!   the embedded preview (RAW) or the file. Explorer's thumbnails are never
//!   asked: their cache writes `Thumbs.db` into network folders.
//! - **Developing** (the pump): once the device and the engine's stamp are
//!   known and the edit view's preview is idle, `helpers` engine processes
//!   of their own develop the missing ones, those on screen first, then
//!   outward; in Low Power only those on screen. Each is the stored edits as
//!   opening shows them (a brightness match solved as opening would solve
//!   it), 512 px, cropped, turned and inside its border, saved as a JPEG
//!   under its key. The open photograph's is filed from its preview as
//!   it's left (`Model::file_thumbnail`), costing no develop.
//! - **Memory**: bounded caches of pixels per size, and the GPU bitmaps
//!   made from them, uploaded as they arrive, never while painting.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use negs_library::{Item, thumbnail};
use serde_json::{Value, json};

use super::surface::{self, Image};
use super::{App, post};
use crate::edited_store::{self, Store};
use crate::engine::{self, EngineClient};
use crate::model::Model;
use crate::sheet_rules::{self as rules, Lru, PumpItem, ThumbSize};
use crate::{json as js, state};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A thumbnail's pixels, opaque BGRA.
pub struct Pixels {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub id: u64,
    /// From under an older engine: shown, and developed again.
    pub stale: bool,
}

impl Pixels {
    fn new(bgra: Vec<u8>, width: u32, height: u32, stale: bool) -> Arc<Pixels> {
        Arc::new(Pixels { bgra, width, height, id: NEXT_ID.fetch_add(1, Ordering::Relaxed), stale })
    }

    pub fn ratio(&self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }
}

/// What a cell needs to find its thumbnail: the item, and the developed
/// one's key (`None` until the engine is known or without a look).
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    pub item: Item,
    pub key: Option<String>,
    pub loose: Option<String>,
}

/// The key and loose key of `item`'s developed thumbnail with its stored
/// edits, else its unedited look (`unedited`).
pub fn want(model: &Model, item: &Item, unedited: Option<&str>) -> Want {
    let none = Want { item: item.clone(), key: None, loose: None };
    let Some(library) = &model.shelf.library else { return none };
    let Some(file) = library.file(item) else { return none };
    let Some(look) = library.look(item).or(unedited) else { return none };
    Want { item: item.clone(), key: edited_store::thumbnails().key(file, item, look), loose: Store::loose_key(file, item, look) }
}

/// The unedited look, once a library is open.
pub fn unedited_look(model: &Model) -> Option<String> {
    model.shelf.library.as_ref().map(|l| thumbnail::unedited_look(&l.unedited_state()))
}

struct Job {
    path: PathBuf,
    size: ThumbSize,
    key: Option<String>,
    loose: Option<String>,
}

#[derive(Default)]
struct Queue {
    jobs: Mutex<Vec<Job>>,
    ready: Condvar,
}

enum Arrived {
    Edited { key: String, pixels: Arc<Pixels>, read: Option<(PathBuf, ThumbSize)> },
    Embedded { path: PathBuf, size: ThumbSize, pixels: Option<Arc<Pixels>> },
}

pub struct Thumbs {
    embedded: [Lru<PathBuf, Arc<Pixels>>; 3],
    edited: Lru<String, Arc<Pixels>>,
    images: Lru<u64, Image>,
    /// Each photograph's shape as its embedded preview has it.
    ratios: HashMap<PathBuf, f64>,
    /// Each photograph's shape from its header, read for the whole folder
    /// before its thumbnails (`PhotoAspects`), so rows and frames don't move
    /// as they arrive; and those asked for.
    header_ratios: HashMap<PathBuf, f64>,
    headers_asked: HashSet<PathBuf>,
    reading: HashSet<(PathBuf, ThumbSize)>,
    unreadable: HashSet<PathBuf>,
    queue: Option<Arc<Queue>>,
    /// Keys found on disk, being developed, or that failed to develop.
    on_disk: HashSet<String>,
    developing: HashSet<String>,
    failed: HashSet<String>,
    helpers: Vec<Arc<EngineClient>>,
    busy: usize,
    generation: Arc<AtomicU64>,
    /// Counts arrivals: a view painted at an older one paints again.
    pub revision: u64,
    /// No new developing while the grid flies (`grid::start_flight`).
    pub held: bool,
}

impl Default for Thumbs {
    fn default() -> Self {
        let lru = |s: ThumbSize| {
            let (bytes, count) = s.cache_limits();
            Lru::new(bytes, count)
        };
        Self {
            embedded: [lru(ThumbSize::Strip), lru(ThumbSize::Medium), lru(ThumbSize::Large)],
            edited: Lru::new(160 << 20, 600),
            images: Lru::new(384 << 20, 1400),
            ratios: HashMap::new(),
            header_ratios: HashMap::new(),
            headers_asked: HashSet::new(),
            reading: HashSet::new(),
            unreadable: HashSet::new(),
            queue: None,
            on_disk: HashSet::new(),
            developing: HashSet::new(),
            failed: HashSet::new(),
            helpers: vec![],
            busy: 0,
            generation: Arc::default(),
            revision: 0,
            held: false,
        }
    }
}

fn slot(size: ThumbSize) -> usize {
    size as usize
}

impl Thumbs {
    /// The bitmap to paint for `want` at `size`: its developed thumbnail,
    /// else its embedded preview at this size or another. Uploads one made
    /// on a lost device again (rare); otherwise allocates nothing.
    pub fn shown(&mut self, want: &Want, size: ThumbSize) -> Option<&Image> {
        let pixels = self.pixels(want, size)?.clone();
        let fresh = self.images.peek(&pixels.id).is_some_and(Image::current);
        if !fresh {
            let image = surface::upload(&pixels.bgra, pixels.width, pixels.height)?;
            let bytes = image.bytes();
            self.images.insert(pixels.id, image, bytes);
        }
        self.images.get(&pixels.id)
    }

    fn pixels(&self, want: &Want, size: ThumbSize) -> Option<&Arc<Pixels>> {
        if let Some(p) = want.key.as_ref().and_then(|k| self.edited.peek(k)) {
            return Some(p);
        }
        let path = &want.item.photo;
        std::iter::once(size).chain(ThumbSize::ALL).find_map(|s| self.embedded[slot(s)].peek(path))
    }

    /// The photograph's shape as shown: its developed thumbnail's (cropped
    /// and turned), else its embedded preview's, else its header's; `None`
    /// before any is read. A developed thumbnail within rounding of the
    /// shape known keeps it (`kept_ratio`): rows don't move as they arrive.
    pub fn ratio(&self, want: &Want) -> Option<f64> {
        let path = &want.item.photo;
        let known = self.ratios.get(path).or_else(|| self.header_ratios.get(path)).copied();
        match want.key.as_ref().and_then(|k| self.edited.peek(k)) {
            Some(p) => Some(rules::kept_ratio(known, p.ratio())),
            None => known,
        }
    }

    /// Reads the shapes of `wants` not known yet from their headers, nearest
    /// `centre` first, on a thread, a batch at a time (`PhotoAspects.read`):
    /// about a millisecond each. The batch nearest `centre` is read at
    /// launch, without waiting for the device, so the frames first painted
    /// have their shapes (not 3:2 and then again); the rest once the device
    /// is made: reading a whole large folder's at launch slowed it.
    pub fn read_headers(&mut self, wants: &[Want], centre: usize) {
        let early = !surface::ready();
        if early && !self.headers_asked.is_empty() {
            return;
        }
        let missing: Vec<(usize, PathBuf)> = wants
            .iter()
            .enumerate()
            .filter(|(_, w)| !self.ratios.contains_key(&w.item.photo) && !self.headers_asked.contains(&w.item.photo))
            .map(|(i, w)| (i, w.item.photo.clone()))
            .collect();
        if missing.is_empty() {
            return;
        }
        let mut order = rules::outward(missing, centre);
        if early {
            order.truncate(rules::HEADER_BATCH);
        }
        self.headers_asked.extend(order.iter().cloned());
        let _ = std::thread::Builder::new().name("negs-headers".into()).spawn(move || {
            crate::thumbnail::com_thread();
            unsafe {
                use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
            }
            for batch in order.chunks(rules::HEADER_BATCH) {
                let started = std::time::Instant::now();
                let read: Vec<(PathBuf, f64)> = batch.iter().filter_map(|p| crate::thumbnail::header_ratio(p).map(|r| (p.clone(), r))).collect();
                crate::trace::event("headers_read", || json!({"count": batch.len(), "known": read.len(), "ms": started.elapsed().as_secs_f64() * 1000.0}));
                post(move |app: &mut App| {
                    app.thumbs.header_ratios.extend(read);
                    app.model.dirty |= crate::model::dirty::STRIP;
                });
            }
        });
    }

    /// Asks for `wants` at `size`, most wanted first (replacing what was
    /// asked before at any size but what's already being read).
    pub fn read(&mut self, wants: &[&Want], size: ThumbSize) {
        // Nothing to show them with yet: launch keeps the cores.
        if !surface::ready() {
            return;
        }
        let queue = self.queue.get_or_insert_with(start_readers).clone();
        let mut queued = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut kept = Vec::with_capacity(queued.len());
        for job in queued.drain(..) {
            if job.size == size {
                self.reading.remove(&(job.path, job.size));
            } else {
                kept.push(job);
            }
        }
        *queued = kept;
        for want in wants {
            let path = &want.item.photo;
            let has_edited = want.key.as_ref().is_some_and(|k| self.edited.contains(k));
            // The strip's (432) serves the grid's smaller size (384) as well.
            let covered = ThumbSize::ALL.iter().any(|s| s.pixels() >= size.pixels() && self.embedded[slot(*s)].contains(path));
            if has_edited || covered || self.unreadable.contains(path) {
                continue;
            }
            if !self.reading.insert((path.clone(), size)) {
                continue;
            }
            queued.push(Job { path: path.clone(), size, key: want.key.clone(), loose: want.loose.clone() });
        }
        queue.ready.notify_all();
    }

    fn arrived(&mut self, arrived: Arrived) {
        self.revision += 1;
        match arrived {
            Arrived::Edited { key, pixels, read } => {
                if let Some(read) = read {
                    self.reading.remove(&read);
                }
                self.on_disk.insert(key.clone());
                self.upload(&pixels);
                let bytes = pixels.bgra.len();
                self.edited.insert(key, pixels, bytes);
            }
            Arrived::Embedded { path, size, pixels } => {
                self.reading.remove(&(path.clone(), size));
                match pixels {
                    Some(pixels) => {
                        // The header's shape, from the preview at full size,
                        // unless the thumbnail's differs by more than its
                        // rounding: frames don't move a pixel as they arrive.
                        let ratio = rules::kept_ratio(self.header_ratios.get(&path).copied(), pixels.ratio());
                        self.ratios.insert(path.clone(), ratio);
                        self.upload(&pixels);
                        let bytes = pixels.bgra.len();
                        self.embedded[slot(size)].insert(path, pixels, bytes);
                    }
                    None => {
                        self.unreadable.insert(path);
                    }
                }
            }
        }
    }

    /// The open photograph's thumbnail, filed from its preview as it was
    /// left: shown at once.
    pub fn filed(&mut self, key: String, bgra: Vec<u8>, width: u32, height: u32) {
        self.failed.remove(&key);
        self.arrived(Arrived::Edited { key, pixels: Pixels::new(bgra, width, height, false), read: None });
    }

    fn upload(&mut self, pixels: &Pixels) {
        let started = std::time::Instant::now();
        let made = surface::upload(&pixels.bgra, pixels.width, pixels.height);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if ms > 2.0 {
            crate::trace::event("upload_slow", || json!({"ms": ms, "width": pixels.width, "height": pixels.height}));
        }
        if let Some(image) = made {
            let bytes = image.bytes();
            self.images.insert(pixels.id, image, bytes);
        }
    }

    /// The device was lost: bitmaps are made again as they're painted.
    pub fn device_lost(&mut self) {
        self.images.clear();
    }

    // MARK: Developing

    /// Develops what's missing, as `pump_order` orders it. `on_screen` and
    /// `open` index `wants`.
    pub fn pump(&mut self, model: &Model, wants: &[Want], on_screen: std::ops::Range<usize>, open: Option<usize>) {
        let Some(paths) = model.engine_paths() else { return };
        if !surface::ready() || model.shelf.library.is_none() {
            return;
        }
        if edited_store::thumbnails().engine().is_none() {
            return edited_store::read_stamp(paths.clone(), |app: &mut App| app.thumbnails_known());
        }
        if !model.render_idle() || self.held {
            return;
        }
        let low_power = low_power();
        let target = rules::helpers(cores(), memory(), low_power);
        crate::trace::event("thumbnail_pump", || json!({"busy": self.busy, "helpers": target, "low_power": low_power, "keys": wants.iter().filter(|w| w.key.is_some()).count()}));
        if self.busy >= target {
            return;
        }
        let items: Vec<PumpItem> = wants
            .iter()
            .enumerate()
            .map(|(i, w)| PumpItem {
                look: w.key.is_some(),
                on_screen: on_screen.contains(&i),
                open: open == Some(i),
                done: w.key.as_ref().is_some_and(|k| {
                    self.edited.peek(k).is_some_and(|p| !p.stale) || self.on_disk.contains(k) || self.developing.contains(k) || self.failed.contains(k)
                }),
            })
            .collect();
        for index in rules::pump_order(&items, low_power) {
            if self.busy >= target {
                break;
            }
            let want = &wants[index];
            let Some(key) = want.key.clone() else { continue };
            let Some(job) = develop_job(model, want, key.clone()) else {
                self.failed.insert(key);
                continue;
            };
            while self.helpers.len() < target {
                match EngineClient::start_background(paths.clone()) {
                    Ok(engine) => self.helpers.push(Arc::new(engine)),
                    Err(_) => return,
                }
            }
            let engine = self.helpers[self.busy % self.helpers.len()].clone();
            self.busy += 1;
            self.developing.insert(key);
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1_000_000;
            std::thread::spawn(move || run_develop(engine, job, generation));
        }
        // Fewer helpers in Low Power: the spare one goes once it's idle.
        if self.busy == 0 && self.helpers.len() > target {
            self.helpers.truncate(target);
        }
    }

    fn developed(&mut self, key: String, pixels: Option<Arc<Pixels>>, found: bool) {
        self.busy = self.busy.saturating_sub(1);
        self.developing.remove(&key);
        match pixels {
            Some(pixels) => self.arrived(Arrived::Edited { key, pixels, read: None }),
            None if found => {
                self.on_disk.insert(key);
            }
            None => {
                self.failed.insert(key);
            }
        }
    }
}

impl App {
    /// The engine's stamp is known: cells get their keys.
    pub fn thumbnails_known(&mut self) {
        self.model.dirty |= crate::model::dirty::STRIP;
    }
}

/// The developing request for `want`, made on the UI thread from its
/// sidecar as it is now.
struct DevelopJob {
    key: String,
    loose: Option<String>,
    source: PathBuf,
    command: Value,
    /// A brightness match to solve first: the prep exposure above it.
    solve: Option<f64>,
    raw: bool,
    /// Turned and bordered as the edit view shows it.
    finish: crate::thumb_compose::Finish,
}

fn develop_job(model: &Model, want: &Want, key: String) -> Option<DevelopJob> {
    let library = model.shelf.library.as_ref()?;
    let (stored, solved) = library.thumbnail_state(&want.item).unwrap_or_else(|| (library.unedited_state(), false));
    let mut state = crate::model::library_map::from_library(&stored, false).ok()?;
    js::set(&mut state.params, "io.output_color_space", Value::String("sRGB".into()));
    let solve = (state.brightness_match.is_some() && !solved).then(|| {
        let ev = state.brightness_match.as_ref().and_then(|m| m.ev).unwrap_or(0.0);
        js::number(state.value(state::PREP_EXPOSURE)) - ev
    });
    let source = want.item.photo.clone();
    let mut command = json!({
        "command": "thumbnail", "input": source, "film": state.film, "paper": state.paper,
        "params": state.params, "long_side": rules::EDITED_LONG_SIDE, "reduced_decode": true,
    });
    if state.crop.cropped() {
        let r = state.crop.rect;
        command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": state.crop.angle});
    }
    let finish = crate::thumb_compose::Finish::of(&state, model.engine_paths().map(|p| p.data.clone()));
    Some(DevelopJob { key, loose: want.loose.clone(), source: source.clone(), command, solve, raw: crate::model::is_raw(&source), finish })
}

fn run_develop(engine: Arc<EngineClient>, mut job: DevelopJob, generation: u64) {
    let store = edited_store::thumbnails();
    let key = job.key.clone();
    let finish = move |pixels: Option<Arc<Pixels>>, found: bool| {
        post(move |app: &mut App| {
            app.thumbs.developed(key, pixels, found);
            app.model.dirty |= crate::model::dirty::STRIP;
        });
    };
    if store.contains(&job.key) {
        return finish(None, true);
    }
    job.command["source_generation"] = json!(generation);
    // Every request names it, the decode and the solve's too.
    let buffer = engine.directory().join(format!("thumbnail-{generation}.rgba"));
    job.command["preview_path"] = json!(buffer);
    let ask = |command: Value| -> anyhow::Result<Value> {
        let (tx, rx) = std::sync::mpsc::channel();
        let timeout = engine::timeout_for("thumbnail");
        engine.request(command, timeout, move |r| {
            let _ = tx.send(r.map(|(_, v)| v));
        });
        rx.recv()?
    };
    let developed = (|| -> anyhow::Result<Arc<Pixels>> {
        if let Some(offset) = job.solve {
            // As opening solves it: the decoded source, the camera's
            // preview (or the file) as the reference, then the exposure.
            // The camera preview is read while the helper decodes.
            let mut decode = job.command.clone();
            decode["decode_only"] = json!(true);
            let (decoded, preview) = std::thread::scope(|scope| {
                let preview = job.raw.then(|| scope.spawn(|| crate::brightness::read_camera_preview(&job.source)));
                let decoded = ask(decode);
                (decoded, preview.map(|thread| thread.join().unwrap_or_else(|_| Err(anyhow::anyhow!("The camera preview couldn’t be read.")))))
            });
            let decoded = decoded?;
            let size = (js::number(&decoded["source_width"]) as u32, js::number(&decoded["source_height"]) as u32);
            let png = crate::brightness::reference(&job.source, job.raw, preview, size);
            if let Ok(png) = png {
                let reference = engine.directory().join(format!("thumbnail-reference-{generation}.png"));
                std::fs::write(&reference, png)?;
                let mut place = job.command.clone();
                place["command"] = json!("place_exposure");
                place["reference_path"] = json!(reference);
                let params = &mut place["params"];
                js::set(params, state::PREP_EXPOSURE, json!(0.0));
                let placed = ask(place);
                let _ = std::fs::remove_file(&reference);
                if let Some(ev) = placed.ok().and_then(|r| r["exposure_ev"].as_f64()).filter(|ev| ev.is_finite()) {
                    let (low, high) = state::PREP_EXPOSURE_RANGE;
                    js::set(&mut job.command["params"], state::PREP_EXPOSURE, json!((ev + offset).clamp(low, high)));
                }
            }
        }
        let result = ask(job.command.clone());
        let rgba = std::fs::read(&buffer);
        let _ = std::fs::remove_file(&buffer);
        let result = result?;
        let (w, h) = (js::number(&result["width"]) as u32, js::number(&result["height"]) as u32);
        let rgba = rgba?;
        let image = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow::anyhow!("short buffer"))?;
        let image = crate::thumb_compose::finish(image, &job.finish);
        let (w, h) = image.dimensions();
        let rgba = image.into_raw();
        let jpeg = edited_store::jpeg(&rgba, w, h, rules::EDITED_LONG_SIDE).ok_or_else(|| anyhow::anyhow!("encode"))?;
        let _ = store.save(&jpeg, &job.key, job.loose.as_deref());
        let (bgra, w, h) = edited_store::decode(&jpeg).ok_or_else(|| anyhow::anyhow!("decode"))?;
        Ok(Pixels::new(bgra, w, h, false))
    })();
    crate::trace::event("thumbnail_developed", || match &developed {
        Ok(p) => json!({"key": job.key, "width": p.width, "height": p.height}),
        Err(error) => json!({"key": job.key, "error": format!("{error:#}")}),
    });
    match developed {
        Ok(pixels) => finish(Some(pixels), false),
        Err(_) => finish(None, false),
    }
}

// MARK: Reading

fn start_readers() -> Arc<Queue> {
    let queue = Arc::new(Queue::default());
    for n in 0..rules::readers(cores()) {
        let queue = queue.clone();
        let _ = std::thread::Builder::new().name(format!("negs-thumbs-{n}")).spawn(move || {
            crate::thumbnail::com_thread();
            // Under the photograph's own develop, which runs at normal priority.
            unsafe {
                use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
            }
            loop {
                let job = {
                    let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                    while jobs.is_empty() {
                        jobs = queue.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
                    }
                    jobs.remove(0)
                };
                let started = std::time::Instant::now();
                let name = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = job.size;
                let arrived = read_one(job);
                crate::trace::event("thumbnail_read", || {
                    let (kind, w, h) = match &arrived {
                        Arrived::Edited { pixels, .. } => (if pixels.stale { "stand_in" } else { "edited" }, pixels.width, pixels.height),
                        Arrived::Embedded { pixels: Some(p), .. } => ("embedded", p.width, p.height),
                        Arrived::Embedded { pixels: None, .. } => ("failed", 0, 0),
                    };
                    json!({"name": name, "size": size as usize, "kind": kind, "width": w, "height": h, "ms": started.elapsed().as_secs_f64() * 1000.0})
                });
                post(move |app: &mut App| {
                    app.thumbs.arrived(arrived);
                    app.model.dirty |= crate::model::dirty::STRIP;
                });
            }
        });
    }
    queue
}

fn read_one(job: Job) -> Arrived {
    let store = edited_store::thumbnails();
    if let Some(key) = &job.key {
        let stand_in = || job.loose.as_deref().and_then(|l| store.previous(l)).map(|k| (k, true));
        if let Some((found, stale)) = store.contains(key).then(|| (key.clone(), false)).or_else(stand_in) {
            if let Some((bgra, w, h)) = std::fs::read(store.path(&found)).ok().and_then(|f| edited_store::decode(&f)) {
                if !stale {
                    store.touch(&found);
                }
                return Arrived::Edited { key: key.clone(), pixels: Pixels::new(bgra, w, h, stale), read: Some((job.path, job.size)) };
            }
        }
    }
    let pixels = crate::thumbnail::read(&job.path, job.size.pixels()).map(|(bgra, w, h)| Pixels::new(bgra, w, h, false));
    Arrived::Embedded { path: job.path, size: job.size, pixels }
}

// MARK: The machine

fn cores() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

fn memory() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() { status.ullTotalPhys } else { 8 << 30 }
}

/// Windows' energy saver (battery saver) is on: Low Power, as the Mac's.
pub fn low_power() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.SystemStatusFlag == 1
}
