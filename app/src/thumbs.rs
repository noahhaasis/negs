//! The strip's and grid's thumbnails (`windows/src/ui/thumbs.rs`,
//! `PhotoThumbnails` / `EditedThumbnails` / `pumpThumbnails` on the Mac):
//! each photograph's developed thumbnail when there is one, else its
//! embedded preview, read and colour-managed off the UI thread.
//!
//! - **Reading**: a pool of `rules::readers` threads takes the shown
//!   photographs first, then ±32 around the middle of the view; each looks
//!   for the developed thumbnail filed under the photograph's key, then the
//!   last one saved under an older engine (a stand-in, developed again),
//!   else reads the embedded preview (RAW) or the file
//!   (`negs_app::thumbnail::read`).
//! - **Colour**: every thumbnail arrives as sRGB; the worker converts it to
//!   the window's display (macOS Display P3 / the monitor's ICC profile,
//!   `color_display::active_display_profile`) and wraps the BGRA8 pixels in
//!   a GPUI `RenderImage`. The UI thread never decodes, resamples or
//!   colour-manages: it only takes ready images (GPUI uploads them to its
//!   atlas the first time they're painted).
//! - **Developing** (the pump): once the engine's stamp is known and the edit
//!   view's preview is idle, `rules::helpers` engine processes of their own
//!   develop the missing ones (on screen first, then outward), each the
//!   stored edits at 512 px, cropped, turned and bordered
//!   (`thumb_compose`), saved as a JPEG under its key. The open photograph's
//!   is filed from its preview as it's left (`Host::thumbnail_filed`).
//! - **Memory**: bounded `rules::Lru` caches per size. A `Thumb` owns its
//!   `RenderImage`; when the last reference goes (evicted, the folder
//!   closed, the display changed) the image is queued and released from
//!   GPUI's sprite atlas with `window.drop_image` before the next paint.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};

use gpui::{RenderImage, Window};
use negs_app::edited_store::{self, Store};
use negs_app::engine::{self, EngineClient};
use negs_app::host;
use negs_app::icc;
use negs_app::model::{Model, dirty};
use negs_app::sheet_rules::{self as rules, Lru, PumpItem, ThumbSize};
use negs_app::{json as js, state};
use negs_library::{Item, thumbnail};
use serde_json::{Value, json};

use crate::color_display::bgra_to_render_image;

// MARK: Images

/// A thumbnail ready to paint: display-managed BGRA8 in a GPUI image.
pub struct Thumb {
    image: Option<Arc<RenderImage>>,
    pub width: u32,
    pub height: u32,
    /// From under an older engine: shown, and developed again.
    pub stale: bool,
    /// Its image is another's (the canvas's): not released when it goes.
    borrowed: bool,
}

impl Thumb {
    pub fn image(&self) -> &Arc<RenderImage> {
        self.image.as_ref().expect("a thumb keeps its image until dropped")
    }

    pub fn ratio(&self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }

    pub fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

impl Thumb {
    /// An image made elsewhere (the canvas's), to fly as a thumbnail. Its
    /// owner releases it: this one doesn't queue it on drop.
    pub fn wrap(image: Arc<RenderImage>, width: u32, height: u32) -> Thumb {
        Thumb { image: Some(image), width, height, stale: false, borrowed: true }
    }
}

#[cfg(test)]
impl Thumb {
    /// A black thumbnail of `w` × `h`.
    pub fn blank_for_tests(w: u32, h: u32) -> Thumb {
        Thumb { image: Some(crate::color_display::bgra_to_render_image(vec![0; (w * h * 4) as usize], w, h)), width: w, height: h, stale: false, borrowed: false }
    }
}

impl Drop for Thumb {
    fn drop(&mut self) {
        if let Some(image) = self.image.take().filter(|_| !self.borrowed) {
            release(image);
        }
    }
}

/// Images whose last `Thumb` went, waiting for `flush_drops`.
fn drops() -> &'static Mutex<Vec<Arc<RenderImage>>> {
    static DROPS: OnceLock<Mutex<Vec<Arc<RenderImage>>>> = OnceLock::new();
    DROPS.get_or_init(Mutex::default)
}

/// Queues an image whose last owner went for `window.drop_image` (the mark
/// sprites share the queue).
pub fn release(image: Arc<RenderImage>) {
    drops().lock().unwrap_or_else(|e| e.into_inner()).push(image);
}

/// Releases every dropped thumbnail from GPUI's sprite atlas. Called at the
/// start of each paint of the strip or the grid.
pub fn flush_drops(window: &mut Window) -> usize {
    let images = std::mem::take(&mut *drops().lock().unwrap_or_else(|e| e.into_inner()));
    let count = images.len();
    for image in images {
        let _ = window.drop_image(image);
    }
    count
}

/// How many dropped images are waiting (tests and the bench).
pub fn pending_drops() -> usize {
    drops().lock().unwrap_or_else(|e| e.into_inner()).len()
}

// MARK: The display

struct Display {
    epoch: u64,
    transform: Option<Arc<icc::Transform>>,
}

fn display_lock() -> &'static RwLock<Display> {
    static DISPLAY: OnceLock<RwLock<Display>> = OnceLock::new();
    DISPLAY.get_or_init(|| RwLock::new(Display { epoch: 1, transform: None }))
}

/// The display the workers convert to, and its epoch.
fn display() -> (u64, Option<Arc<icc::Transform>>) {
    let d = display_lock().read().unwrap_or_else(|e| e.into_inner());
    (d.epoch, d.transform.clone())
}

fn display_epoch() -> u64 {
    display_lock().read().unwrap_or_else(|e| e.into_inner()).epoch
}

/// sRGB → display conversion tables: 8-bit decode, and linear → sRGB
/// encode finely enough that interpolating it stays within a code value.
struct Tables {
    decode: [f32; 256],
    encode: Vec<f32>,
}

const ENCODE_STEPS: usize = 16384;

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables {
        decode: std::array::from_fn(|i| crate::color_display::srgb_decode(i as f32 / 255.0)),
        encode: (0..=ENCODE_STEPS).map(|i| crate::color_display::srgb_encode(i as f32 / ENCODE_STEPS as f32)).collect(),
    })
}

#[inline]
fn encode(t: &Tables, linear: f32) -> f32 {
    let p = linear.clamp(0.0, 1.0) * ENCODE_STEPS as f32;
    let i = (p as usize).min(ENCODE_STEPS - 1);
    let f = p - i as f32;
    t.encode[i] + (t.encode[i + 1] - t.encode[i]) * f
}

/// Converts opaque sRGB BGRA8 in place to the display's encoding, as the
/// canvas presents the photograph (`color_display::color_manage_linear_to_bgra`).
/// Without a transform (an sRGB monitor) the pixels are already right.
pub fn manage(bgra: &mut [u8], cm: Option<&icc::Transform>) {
    let Some(cm) = cm else { return };
    let t = tables();
    let last = cm.lut.len() - 1;
    let lastf = last as f32;
    for p in bgra.chunks_exact_mut(4) {
        let lin = [t.decode[p[2] as usize], t.decode[p[1] as usize], t.decode[p[0] as usize]];
        let mut out = [0u8; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let m = cm.matrix[c][0] * lin[0] + cm.matrix[c][1] * lin[1] + cm.matrix[c][2] * lin[2];
            let q = encode(t, m) * lastf;
            let i = (q as usize).min(last);
            let j = (i + 1).min(last);
            let v = cm.lut[i][c] + (cm.lut[j][c] - cm.lut[i][c]) * (q - i as f32);
            *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        p[0] = out[2];
        p[1] = out[1];
        p[2] = out[0];
        p[3] = 255;
    }
}

/// sRGB BGRA8 → a display-managed `Thumb` for the display of `epoch`.
fn make_thumb(mut bgra: Vec<u8>, width: u32, height: u32, stale: bool, cm: Option<&icc::Transform>) -> Option<Arc<Thumb>> {
    if width == 0 || height == 0 || bgra.len() != width as usize * height as usize * 4 {
        return None;
    }
    manage(&mut bgra, cm);
    let image = bgra_to_render_image(bgra, width, height);
    Some(Arc::new(Thumb { image: Some(image), width, height, stale, borrowed: false }))
}

// MARK: Arrivals

enum Arrived {
    Edited { key: String, thumb: Arc<Thumb>, read: Option<(PathBuf, ThumbSize)>, epoch: u64 },
    Embedded { path: PathBuf, size: ThumbSize, thumb: Option<Arc<Thumb>>, epoch: u64 },
    Headers { ratios: Vec<(PathBuf, f64)> },
    Developed { key: String, thumb: Option<Arc<Thumb>>, found: bool, epoch: u64 },
    /// A developed thumbnail looked for on disk (its embedded preview
    /// already shown) and not there: the pump develops it.
    Missed { path: PathBuf, size: ThumbSize },
}

fn arrivals() -> &'static Mutex<Vec<Arrived>> {
    static ARRIVALS: OnceLock<Mutex<Vec<Arrived>>> = OnceLock::new();
    ARRIVALS.get_or_init(Mutex::default)
}

/// Hands a worker's result to the UI thread and wakes it.
fn deliver(arrived: Arrived) {
    arrivals().lock().unwrap_or_else(|e| e.into_inner()).push(arrived);
    host::post(|model| model.dirty |= dirty::STRIP);
}

/// `Host::thumbnail_filed`: the open photograph's thumbnail, filed from its
/// preview as it was left; converted here, on the filing thread.
pub fn filed_from_host(key: String, bgra: Vec<u8>, width: u32, height: u32) {
    let (epoch, cm) = display();
    if let Some(thumb) = make_thumb(bgra, width, height, false, cm.as_deref()) {
        deliver(Arrived::Edited { key, thumb, read: None, epoch });
    }
}

// MARK: Wants

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

// MARK: The cache

struct Job {
    path: PathBuf,
    size: ThumbSize,
    /// Only the developed thumbnail on disk: the embedded one's shown.
    edited_only: bool,
    key: Option<String>,
    loose: Option<String>,
}

#[derive(Default)]
struct Queue {
    jobs: Mutex<Vec<Job>>,
    ready: Condvar,
}

/// Counters for the bench and the progress log.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub read: u64,
    pub developed: u64,
    pub filed: u64,
    pub dropped_stale: u64,
}

pub struct Thumbs {
    embedded: [Lru<PathBuf, Arc<Thumb>>; 3],
    edited: Lru<String, Arc<Thumb>>,
    /// Each photograph's shape as its embedded preview has it.
    ratios: HashMap<PathBuf, f64>,
    /// Each photograph's shape from its header, read before its thumbnail
    /// so rows and frames don't move as they arrive; and those asked for.
    header_ratios: HashMap<PathBuf, f64>,
    headers_asked: HashSet<PathBuf>,
    reading: HashSet<(PathBuf, ThumbSize)>,
    /// Developed thumbnails' keys looked for on disk already. Without it a
    /// photograph whose embedded preview was read before its key was known
    /// (the store learns the engine's stamp after launch) kept showing the
    /// camera's preview.
    looked_up: HashSet<String>,
    unreadable: HashSet<PathBuf>,
    queue: Option<Arc<Queue>>,
    /// Keys found on disk, being developed, or that failed to develop.
    on_disk: HashSet<String>,
    developing: HashSet<String>,
    failed: HashSet<String>,
    helpers: Vec<Arc<EngineClient>>,
    busy: usize,
    generation: Arc<AtomicU64>,
    /// The folder whose thumbnails are cached; another clears them.
    folder: Option<PathBuf>,
    /// Counts arrivals: a view painted at an older one paints again.
    pub revision: u64,
    /// No new developing while the sheet flies (`grid::start_flight`).
    pub held: bool,
    pub stats: Stats,
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
            ratios: HashMap::new(),
            header_ratios: HashMap::new(),
            headers_asked: HashSet::new(),
            reading: HashSet::new(),
            looked_up: HashSet::new(),
            unreadable: HashSet::new(),
            queue: None,
            on_disk: HashSet::new(),
            developing: HashSet::new(),
            failed: HashSet::new(),
            helpers: vec![],
            busy: 0,
            generation: Arc::default(),
            folder: None,
            revision: 0,
            held: false,
            stats: Stats::default(),
        }
    }
}

fn slot(size: ThumbSize) -> usize {
    size as usize
}

impl Thumbs {
    /// The thumbnail to paint for `want` at `size`: its developed one, else
    /// its embedded preview at this size or another.
    pub fn shown(&self, want: &Want, size: ThumbSize) -> Option<Arc<Thumb>> {
        if let Some(t) = want.key.as_ref().and_then(|k| self.edited.peek(k)) {
            return Some(t.clone());
        }
        let path = &want.item.photo;
        std::iter::once(size).chain(ThumbSize::ALL).find_map(|s| self.embedded[slot(s)].peek(path)).cloned()
    }

    /// The photograph's shape as shown: its developed thumbnail's (cropped
    /// and turned), else its embedded preview's, else its header's; `None`
    /// before any is read. A developed thumbnail within rounding of the
    /// shape known keeps it (`kept_ratio`): rows don't move as they arrive.
    pub fn ratio(&self, want: &Want) -> Option<f64> {
        let path = &want.item.photo;
        let known = self.ratios.get(path).or_else(|| self.header_ratios.get(path)).copied();
        match want.key.as_ref().and_then(|k| self.edited.peek(k)) {
            Some(t) => Some(rules::kept_ratio(known, t.ratio())),
            None => known,
        }
    }

    /// Lets every cached image go (each is released from the atlas at the
    /// next paint), and forgets what's being read.
    pub fn clear(&mut self) {
        for lru in &mut self.embedded {
            lru.clear();
        }
        self.edited.clear();
        self.reading.clear();
        self.looked_up.clear();
        if let Some(queue) = &self.queue {
            queue.jobs.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        self.revision += 1;
    }

    /// The window moved to a display with another profile (or the profile
    /// changed): what's cached was converted for the old one, so it goes and
    /// is read again for the new one. Results still under way are dropped.
    pub fn set_display(&mut self, transform: Option<Arc<icc::Transform>>) {
        {
            let mut d = display_lock().write().unwrap_or_else(|e| e.into_inner());
            d.epoch += 1;
            d.transform = transform;
        }
        self.clear();
    }

    /// A folder opened: the last one's thumbnails go.
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        if self.folder != folder {
            self.folder = folder;
            self.clear();
            self.ratios.clear();
            self.header_ratios.clear();
            self.headers_asked.clear();
            self.unreadable.clear();
        }
    }

    /// Takes what the workers finished. True if anything arrived.
    pub fn drain(&mut self) -> bool {
        let arrived = std::mem::take(&mut *arrivals().lock().unwrap_or_else(|e| e.into_inner()));
        let any = !arrived.is_empty();
        for a in arrived {
            self.arrived(a);
        }
        any
    }

    fn arrived(&mut self, arrived: Arrived) {
        let now = display_epoch();
        self.revision += 1;
        match arrived {
            Arrived::Headers { ratios } => self.header_ratios.extend(ratios),
            Arrived::Edited { key, thumb, read, epoch } => {
                let filed = read.is_none();
                if let Some(read) = read {
                    self.reading.remove(&read);
                }
                self.on_disk.insert(key.clone());
                if filed {
                    self.stats.filed += 1;
                    self.failed.remove(&key);
                }
                if epoch != now {
                    self.stats.dropped_stale += 1;
                    return;
                }
                self.stats.read += 1;
                let bytes = thumb.bytes();
                self.edited.insert(key, thumb, bytes);
            }
            Arrived::Missed { path, size } => {
                self.reading.remove(&(path, size));
            }
            Arrived::Embedded { path, size, thumb, epoch } => {
                self.reading.remove(&(path.clone(), size));
                match thumb {
                    Some(_) if epoch != now => self.stats.dropped_stale += 1,
                    Some(thumb) => {
                        // The header's shape, unless the thumbnail's differs by
                        // more than its rounding: frames don't move as they arrive.
                        let ratio = rules::kept_ratio(self.header_ratios.get(&path).copied(), thumb.ratio());
                        self.ratios.insert(path.clone(), ratio);
                        self.stats.read += 1;
                        let bytes = thumb.bytes();
                        self.embedded[slot(size)].insert(path, thumb, bytes);
                    }
                    None => {
                        self.unreadable.insert(path);
                    }
                }
            }
            Arrived::Developed { key, thumb, found, epoch } => {
                self.busy = self.busy.saturating_sub(1);
                self.developing.remove(&key);
                match thumb {
                    Some(thumb) => {
                        self.stats.developed += 1;
                        self.on_disk.insert(key.clone());
                        if epoch == now {
                            let bytes = thumb.bytes();
                            self.edited.insert(key, thumb, bytes);
                        } else {
                            self.stats.dropped_stale += 1;
                        }
                    }
                    None if found => {
                        self.on_disk.insert(key);
                    }
                    None => {
                        self.failed.insert(key);
                    }
                }
            }
        }
    }

    /// Reads the shapes of `wants` not known yet from their headers, nearest
    /// `centre` first, on a thread, a batch at a time (`PhotoAspects.read`).
    pub fn read_headers(&mut self, wants: &[Want], centre: usize) {
        let missing: Vec<(usize, PathBuf)> = wants
            .iter()
            .enumerate()
            .filter(|(_, w)| !self.ratios.contains_key(&w.item.photo) && !self.headers_asked.contains(&w.item.photo))
            .map(|(i, w)| (i, w.item.photo.clone()))
            .collect();
        if missing.is_empty() {
            return;
        }
        let order = rules::outward(missing, centre);
        self.headers_asked.extend(order.iter().cloned());
        let _ = std::thread::Builder::new().name("negs-headers".into()).spawn(move || {
            negs_app::thumbnail::com_thread();
            for batch in order.chunks(rules::HEADER_BATCH) {
                let ratios: Vec<(PathBuf, f64)> =
                    batch.iter().filter_map(|p| negs_app::thumbnail::header_ratio(p).map(|r| (p.clone(), r))).collect();
                deliver(Arrived::Headers { ratios });
            }
        });
    }

    /// Asks for `wants` at `size`, most wanted first (replacing what was
    /// asked before at this size but what's already being read).
    pub fn read(&mut self, wants: &[&Want], size: ThumbSize) {
        let queue = self.queue.get_or_insert_with(start_readers).clone();
        let mut queued = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut kept = Vec::with_capacity(queued.len());
        for job in queued.drain(..) {
            if job.size == size {
                // Not taken yet: asked for again below, in today's order.
                if let Some(key) = &job.key {
                    self.looked_up.remove(key);
                }
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
            if has_edited || self.unreadable.contains(path) {
                continue;
            }
            // Covered by an embedded preview: only its developed one, once.
            let edited_only = covered;
            if edited_only {
                let Some(key) = &want.key else { continue };
                if self.looked_up.contains(key) || self.developing.contains(key) || self.failed.contains(key) {
                    continue;
                }
            }
            if !self.reading.insert((path.clone(), size)) {
                continue;
            }
            if let Some(key) = &want.key {
                self.looked_up.insert(key.clone());
            }
            queued.push(Job { path: path.clone(), size, edited_only, key: want.key.clone(), loose: want.loose.clone() });
        }
        queue.ready.notify_all();
    }

    /// Jobs not yet taken by a reader, and those being read.
    pub fn outstanding(&self) -> usize {
        self.reading.len() + self.busy
    }

    // MARK: Developing

    /// Develops what's missing, as `pump_order` orders it. `on_screen` and
    /// `open` index `wants`.
    pub fn pump(&mut self, model: &Model, wants: &[Want], on_screen: std::ops::Range<usize>, open: Option<usize>) {
        let Some(paths) = model.engine_paths() else { return };
        if model.shelf.library.is_none() {
            return;
        }
        if edited_store::thumbnails().engine().is_none() {
            return edited_store::read_stamp(paths.clone(), |model| model.dirty |= dirty::STRIP);
        }
        if !model.render_idle() || self.held {
            return;
        }
        let low_power = machine::low_power();
        let target = rules::helpers(machine::cores(), machine::memory(), low_power);
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
                    self.edited.peek(k).is_some_and(|t| !t.stale)
                        || self.on_disk.contains(k)
                        || self.developing.contains(k)
                        || self.failed.contains(k)
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
            let _ = std::thread::Builder::new().name("negs-develop".into()).spawn(move || run_develop(engine, job, generation));
        }
        // Fewer helpers in Low Power: the spare one goes once it's idle.
        if self.busy == 0 && self.helpers.len() > target {
            self.helpers.truncate(target);
        }
    }

    /// Stops the developing helpers (at quit).
    pub fn shutdown(&mut self) {
        for helper in self.helpers.drain(..) {
            helper.stop();
        }
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
    finish: negs_app::thumb_compose::Finish,
}

fn develop_job(model: &Model, want: &Want, key: String) -> Option<DevelopJob> {
    let library = model.shelf.library.as_ref()?;
    let (stored, solved) = library.thumbnail_state(&want.item).unwrap_or_else(|| (library.unedited_state(), false));
    let mut state = negs_app::model::library_map::from_library(&stored, false).ok()?;
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
    let finish = negs_app::thumb_compose::Finish::of(&state, model.engine_paths().map(|p| p.data.clone()));
    Some(DevelopJob { key, loose: want.loose.clone(), source: source.clone(), command, solve, raw: negs_app::model::is_raw(&source), finish })
}

fn run_develop(engine: Arc<EngineClient>, mut job: DevelopJob, generation: u64) {
    let store = edited_store::thumbnails();
    let key = job.key.clone();
    let (epoch, cm) = display();
    let finish = move |thumb: Option<Arc<Thumb>>, found: bool| deliver(Arrived::Developed { key, thumb, found, epoch });
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
    let developed = (|| -> anyhow::Result<Arc<Thumb>> {
        if let Some(offset) = job.solve {
            // As opening solves it: the decoded source, the camera's preview
            // (or the file) as the reference, then the exposure.
            let mut decode = job.command.clone();
            decode["decode_only"] = json!(true);
            let (decoded, preview) = std::thread::scope(|scope| {
                let preview = job.raw.then(|| scope.spawn(|| negs_app::brightness::read_camera_preview(&job.source)));
                let decoded = ask(decode);
                (decoded, preview.map(|t| t.join().unwrap_or_else(|_| Err(anyhow::anyhow!("The camera preview couldn’t be read.")))))
            });
            let decoded = decoded?;
            let size = (js::number(&decoded["source_width"]) as u32, js::number(&decoded["source_height"]) as u32);
            if let Ok(png) = negs_app::brightness::reference(&job.source, job.raw, preview, size) {
                let reference = engine.directory().join(format!("thumbnail-reference-{generation}.png"));
                std::fs::write(&reference, png)?;
                let mut place = job.command.clone();
                place["command"] = json!("place_exposure");
                place["reference_path"] = json!(reference);
                js::set(&mut place["params"], state::PREP_EXPOSURE, json!(0.0));
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
        let image = image::RgbaImage::from_raw(w, h, rgba?).ok_or_else(|| anyhow::anyhow!("short buffer"))?;
        let image = negs_app::thumb_compose::finish(image, &job.finish);
        let (w, h) = image.dimensions();
        let jpeg = edited_store::jpeg(image.as_raw(), w, h, rules::EDITED_LONG_SIDE).ok_or_else(|| anyhow::anyhow!("encode"))?;
        let _ = store.save(&jpeg, &job.key, job.loose.as_deref());
        let (bgra, w, h) = edited_store::decode(&jpeg).ok_or_else(|| anyhow::anyhow!("decode"))?;
        make_thumb(bgra, w, h, false, cm.as_deref()).ok_or_else(|| anyhow::anyhow!("empty"))
    })();
    negs_app::trace::event("thumbnail_developed", || match &developed {
        Ok(t) => json!({"key": job.key, "width": t.width, "height": t.height}),
        Err(error) => json!({"key": job.key, "error": format!("{error:#}")}),
    });
    match developed {
        Ok(thumb) => finish(Some(thumb), false),
        Err(_) => finish(None, false),
    }
}

// MARK: Reading

fn start_readers() -> Arc<Queue> {
    let queue = Arc::new(Queue::default());
    for n in 0..rules::readers(machine::cores()) {
        let queue = queue.clone();
        let _ = std::thread::Builder::new().name(format!("negs-thumbs-{n}")).spawn(move || {
            negs_app::thumbnail::com_thread();
            loop {
                let job = {
                    let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                    while jobs.is_empty() {
                        jobs = queue.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
                    }
                    jobs.remove(0)
                };
                deliver(read_one(job));
            }
        });
    }
    queue
}

fn read_one(job: Job) -> Arrived {
    let (epoch, cm) = display();
    let store = edited_store::thumbnails();
    if let Some(key) = &job.key {
        let stand_in = || job.loose.as_deref().and_then(|l| store.previous(l)).map(|k| (k, true));
        if let Some((found, stale)) = store.contains(key).then(|| (key.clone(), false)).or_else(stand_in) {
            if let Some((bgra, w, h)) = std::fs::read(store.path(&found)).ok().and_then(|f| edited_store::decode(&f)) {
                if !stale {
                    store.touch(&found);
                }
                if let Some(thumb) = make_thumb(bgra, w, h, stale, cm.as_deref()) {
                    return Arrived::Edited { key: key.clone(), thumb, read: Some((job.path, job.size)), epoch };
                }
            }
        }
    }
    if job.edited_only {
        return Arrived::Missed { path: job.path, size: job.size };
    }
    let thumb = negs_app::thumbnail::read(&job.path, job.size.pixels()).and_then(|(bgra, w, h)| make_thumb(bgra, w, h, false, cm.as_deref()));
    Arrived::Embedded { path: job.path, size: job.size, thumb, epoch }
}

// MARK: The machine

pub mod machine {
    pub fn cores() -> usize {
        std::thread::available_parallelism().map_or(4, |n| n.get())
    }

    #[cfg(target_os = "macos")]
    pub fn memory() -> u64 {
        use std::ffi::{c_char, c_int, c_void};
        unsafe extern "C" {
            fn sysctlbyname(name: *const c_char, oldp: *mut c_void, oldlenp: *mut usize, newp: *mut c_void, newlen: usize) -> c_int;
        }
        let mut bytes: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let ok = unsafe { sysctlbyname(c"hw.memsize".as_ptr(), (&mut bytes as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
        if ok && bytes > 0 { bytes } else { 8 << 30 }
    }

    #[cfg(windows)]
    #[allow(non_snake_case)]
    #[repr(C)]
    struct MemoryStatusEx {
        dwLength: u32,
        dwMemoryLoad: u32,
        ullTotalPhys: u64,
        ullAvailPhys: u64,
        ullTotalPageFile: u64,
        ullAvailPageFile: u64,
        ullTotalVirtual: u64,
        ullAvailVirtual: u64,
        ullAvailExtendedVirtual: u64,
    }

    #[cfg(windows)]
    #[allow(non_snake_case)]
    #[repr(C)]
    #[derive(Default)]
    struct SystemPowerStatus {
        ACLineStatus: u8,
        BatteryFlag: u8,
        BatteryLifePercent: u8,
        SystemStatusFlag: u8,
        BatteryLifeTime: u32,
        BatteryFullLifeTime: u32,
    }

    #[cfg(windows)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    }

    #[cfg(windows)]
    pub fn memory() -> u64 {
        let mut status = MemoryStatusEx {
            dwLength: std::mem::size_of::<MemoryStatusEx>() as u32,
            dwMemoryLoad: 0,
            ullTotalPhys: 0,
            ullAvailPhys: 0,
            ullTotalPageFile: 0,
            ullAvailPageFile: 0,
            ullTotalVirtual: 0,
            ullAvailVirtual: 0,
            ullAvailExtendedVirtual: 0,
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 { status.ullTotalPhys } else { 8 << 30 }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn memory() -> u64 {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
            .map_or(8 << 30, |kb| kb * 1024)
    }

    /// Windows' energy saver (battery saver) is on: Low Power, as the Mac's.
    #[cfg(windows)]
    pub fn low_power() -> bool {
        let mut status = SystemPowerStatus::default();
        unsafe { GetSystemPowerStatus(&mut status) != 0 && status.SystemStatusFlag == 1 }
    }

    /// macOS Low Power Mode needs `NSProcessInfo` (Phase 7b); Linux has no
    /// common signal. Never Low Power for now.
    #[cfg(not(windows))]
    pub fn low_power() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Tests that read or bump the global display epoch run one at a time.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn display_p3_transform() -> icc::Transform {
        let srgb_curve = icc::Curve::Parametric(3, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]);
        let profile = icc::MatrixProfile {
            colorants: [[0.5151, 0.2920, 0.1571], [0.2412, 0.6922, 0.0666], [-0.0011, 0.0419, 0.7841]],
            curves: [srgb_curve.clone(), srgb_curve.clone(), srgb_curve],
        };
        profile.transform(Path::new("Display P3")).expect("Display P3 transform")
    }

    #[test]
    fn manage_matches_canvas_colour_path_within_a_code_value() {
        let cm = display_p3_transform();
        let mut worst = 0i32;
        let mut seed = 0x2545_f491_u32;
        for _ in 0..4000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let rgb = [(seed & 255) as u8, ((seed >> 8) & 255) as u8, ((seed >> 16) & 255) as u8];
            let mut px = vec![rgb[2], rgb[1], rgb[0], 255];
            manage(&mut px, Some(&cm));
            let want = crate::color_display::color_manage_srgb_to_bgra(rgb, Some(&cm));
            for c in 0..4 {
                worst = worst.max((px[c] as i32 - want[c] as i32).abs());
            }
        }
        assert!(worst <= 1, "fast thumbnail path differs from the canvas path by {worst}");
    }

    #[test]
    fn manage_without_profile_leaves_srgb_untouched_and_p3_desaturates_red() {
        let mut px = vec![0, 0, 255, 255, 10, 200, 30, 255];
        let before = px.clone();
        manage(&mut px, None);
        assert_eq!(px, before);
        manage(&mut px, Some(&display_p3_transform()));
        assert!(px[2] < 255 && px[1] > 0, "sRGB red sits inside P3: R drops, G rises ({:?})", &px[..4]);
    }

    #[test]
    fn dropping_the_last_thumb_queues_its_image_for_the_atlas() {
        let _serial = serial();
        let before = pending_drops();
        let thumb = make_thumb(vec![128; 4 * 4 * 4], 4, 4, false, None).expect("thumb");
        let held = thumb.clone();
        drop(thumb);
        assert_eq!(pending_drops(), before, "a clone still holds it");
        drop(held);
        assert!(pending_drops() > before, "the image waits for window.drop_image");
        assert!(make_thumb(vec![0; 7], 4, 4, false, None).is_none(), "a short buffer makes nothing");
    }

    #[test]
    fn cache_eviction_and_clear_release_images() {
        let _serial = serial();
        let mut thumbs = Thumbs::default();
        // Strip cache: 120 entries. Insert 130 tiny thumbs; 10 must be dropped.
        let before = pending_drops();
        for i in 0..130 {
            let path = PathBuf::from(format!("/scratch/{i}.jpg"));
            thumbs.arrived(Arrived::Embedded {
                path,
                size: ThumbSize::Strip,
                thumb: make_thumb(vec![9; 16], 2, 2, false, None),
                epoch: display_epoch(),
            });
        }
        assert!(pending_drops() >= before + 10, "LRU eviction releases images");
        let want = |i: usize| Want { item: Item::photo(Path::new(&format!("/scratch/{i}.jpg"))), key: None, loose: None };
        assert!(thumbs.shown(&want(129), ThumbSize::Strip).is_some());
        assert!(thumbs.shown(&want(0), ThumbSize::Strip).is_none(), "the oldest went");
        // A thumbnail for the strip also serves the grid's medium size.
        assert!(thumbs.shown(&want(129), ThumbSize::Medium).is_some());
        let mid = pending_drops();
        thumbs.set_folder(Some(PathBuf::from("/elsewhere")));
        assert!(pending_drops() >= mid + 120, "closing the folder releases the rest");
        assert!(thumbs.shown(&want(129), ThumbSize::Strip).is_none());
    }

    #[test]
    fn results_for_an_old_display_are_dropped() {
        let _serial = serial();
        let mut thumbs = Thumbs::default();
        let old = display_epoch();
        thumbs.set_display(None);
        let path = PathBuf::from("/scratch/old-display.jpg");
        thumbs.reading.insert((path.clone(), ThumbSize::Strip));
        thumbs.arrived(Arrived::Embedded { path: path.clone(), size: ThumbSize::Strip, thumb: make_thumb(vec![1; 16], 2, 2, false, None), epoch: old });
        let want = Want { item: Item::photo(&path), key: None, loose: None };
        assert!(thumbs.shown(&want, ThumbSize::Strip).is_none(), "converted for the old display");
        assert!(!thumbs.reading.contains(&(path, ThumbSize::Strip)), "and no longer being read");
        assert_eq!(thumbs.stats.dropped_stale, 1);
    }

    #[test]
    fn a_key_learnt_after_the_embedded_preview_still_finds_the_developed_one() {
        let _guard = serial();
        let mut thumbs = Thumbs::default();
        let path = PathBuf::from("/nonexistent/negs-test/DSC_0001.NEF");
        thumbs.arrived(Arrived::Embedded { path: path.clone(), size: ThumbSize::Strip, thumb: make_thumb(vec![9; 16], 2, 2, false, None), epoch: display_epoch() });
        let unkeyed = Want { item: Item::photo(&path), key: None, loose: None };
        thumbs.read(&[&unkeyed], ThumbSize::Strip);
        assert_eq!(thumbs.outstanding(), 0, "covered, nothing more to read");
        // The store learnt the engine: the photograph has a key now.
        let keyed = Want { item: Item::photo(&path), key: Some("k1".into()), loose: None };
        thumbs.read(&[&keyed], ThumbSize::Strip);
        assert_eq!(thumbs.outstanding(), 1, "its developed thumbnail is looked for");
        thumbs.read(&[&keyed], ThumbSize::Strip);
        assert_eq!(thumbs.outstanding(), 1, "once");
        // The reader finds nothing on disk and says so.
        let started = std::time::Instant::now();
        while thumbs.outstanding() > 0 && started.elapsed() < std::time::Duration::from_secs(5) {
            thumbs.drain();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(thumbs.outstanding(), 0);
        thumbs.read(&[&keyed], ThumbSize::Strip);
        assert_eq!(thumbs.outstanding(), 0, "not on disk: left to the pump, not asked again");
        thumbs.shutdown();
    }

    #[test]
    fn developed_thumbnail_wins_and_keeps_the_known_shape() {
        let _serial = serial();
        let mut thumbs = Thumbs::default();
        let path = PathBuf::from("/scratch/dev.jpg");
        let epoch = display_epoch();
        thumbs.arrived(Arrived::Embedded { path: path.clone(), size: ThumbSize::Strip, thumb: make_thumb(vec![1; 300 * 200 * 4], 300, 200, false, None), epoch });
        let want = Want { item: Item::photo(&path), key: Some("k".into()), loose: None };
        assert!((thumbs.ratio(&want).unwrap() - 1.5).abs() < 1e-9);
        // 512×341 rounds to 1.501…: the strip keeps 1.5, frames don't move.
        thumbs.arrived(Arrived::Developed { key: "k".into(), thumb: make_thumb(vec![2; 512 * 341 * 4], 512, 341, false, None), found: false, epoch });
        assert_eq!(thumbs.shown(&want, ThumbSize::Strip).unwrap().width, 512);
        assert!((thumbs.ratio(&want).unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(thumbs.busy, 0);
    }

    #[test]
    fn machine_reports_something_sane() {
        assert!(machine::cores() >= 1);
        assert!(machine::memory() >= 1 << 30);
    }
}
