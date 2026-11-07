//! The editor's state and behaviour, a port of `EditorModel.swift`: the edit,
//! undo, rendering one request at a time, opening, cropping, Show Original,
//! saving and exporting. It runs on the UI thread and owns no windows; it
//! marks what changed in `dirty`, and the UI redraws those parts.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};

use crate::border::{BorderColor, BorderSettings, BorderStyle};
use crate::crop::{CropSession, CropSettings, Size};
use crate::engine::{self, EngineClient, EnginePaths, ExportEngine, ExportHandle, ExportRequest};
use crate::histogram::{Histogram, Waveform};
use crate::json;
use crate::picture::Picture;
use crate::presets::{self, PresetFile, Status as PresetStatus};
use crate::previews::{self, Previews, Target};
use crate::session::{CatalogCache, EditorSession, Preferences};
use crate::rendered_preview::{self, Frame, RenderedPreviewCache};
use crate::state::{self, EditState, Stock};
use crate::tone_graph::{self, Controls, Curve, FilmHistogram};
use spektrafilm_io::image_size::PHOTO_LONG_SIDE_RANGE;
use crate::ui::{self, App};

pub mod dirty {
    pub const CANVAS: u32 = 1;
    pub const INSPECTOR: u32 = 1 << 1;
    pub const STRUCTURE: u32 = 1 << 2;
    pub const CHROME: u32 = 1 << 3;
    pub const STATUS: u32 = 1 << 4;
    pub const HISTOGRAM: u32 = 1 << 5;
    pub const STRIP: u32 = 1 << 6;
    pub const ERROR: u32 = 1 << 7;
    pub const ZOOM: u32 = 1 << 8;
    pub const CROP: u32 = 1 << 9;
    pub const LAYOUT: u32 = 1 << 10;
    pub const PRESETS: u32 = 1 << 11;
    /// A preview tile changed, or what they should show may have.
    pub const PREVIEWS: u32 = 1 << 12;
    /// The tone graph's curve or film histogram arrived.
    pub const TONE_GRAPH: u32 = 1 << 13;
    pub const ALL: u32 = u32::MAX;
}

pub const RAW_EXTENSIONS: &[&str] = &[
    "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx", "srw", "x3f", "iiq", "3fr", "ari",
    "bay", "crw", "dcr", "drf", "erf", "fff", "k25", "kdc", "mef", "mos", "mrw", "rwl",
];
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff"];

pub fn is_raw(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| RAW_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

pub fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str()) || IMAGE_EXTENSIONS.contains(&e.as_str()))
}

/// A reduction such as 68%; below 10% with one decimal.
pub fn percent(fraction: f64) -> String {
    format!("{:.*}%", if fraction < 0.1 { 1 } else { 0 }, fraction * 100.0)
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The image format a save or export writes; the engine picks the encoder
/// from the file extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 3] = [OutputFormat::Png, OutputFormat::Jpeg, OutputFormat::Tiff];
    pub fn key(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
        }
    }
    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|f| f.key() == key).unwrap_or(Self::Png)
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Png => "PNG (lossless)",
            Self::Jpeg => "JPEG (smaller)",
            Self::Tiff => "TIFF (lossless)",
        }
    }
    pub fn pattern(self) -> &'static str {
        match self {
            Self::Png => "*.png",
            Self::Jpeg => "*.jpg;*.jpeg",
            Self::Tiff => "*.tif;*.tiff",
        }
    }
}

/// The colour spaces a save offers, as colour names them.
pub const COLOR_SPACES: [&str; 4] = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"];
/// ProPhoto RGB bands at 8 bits, so it needs 16; Adobe RGB stands in.
pub const PROPHOTO: usize = 3;
pub const PROPHOTO_FALLBACK: usize = 2;
pub const QUALITY_RANGE: (u32, u32) = (1, 100);

/// How a save encodes the file: bits per channel, colour space and JPEG
/// quality. The colour space is the one the file is in; ProPhoto RGB at
/// 8 bits is already replaced by Adobe RGB (`effective_space`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveSettings {
    pub bit_depth: u32,
    pub color_space: usize,
    pub jpeg_quality: u32,
}

impl SaveSettings {
    /// The colour space a save with `bit_depth` uses when `chosen` was picked.
    pub fn effective_space(chosen: usize, bit_depth: u32) -> usize {
        let chosen = chosen.min(COLOR_SPACES.len() - 1);
        if chosen == PROPHOTO && bit_depth == 8 { PROPHOTO_FALLBACK } else { chosen }
    }
    pub fn space_name(self) -> &'static str {
        COLOR_SPACES[self.color_space.min(COLOR_SPACES.len() - 1)]
    }
    pub fn is_srgb(self) -> bool {
        self.color_space == 0
    }
    /// The `save_options` of a save command.
    pub fn json(self) -> Value {
        json!({"jpeg_quality": self.jpeg_quality, "bit_depth": self.bit_depth, "color_space": self.space_name()})
    }
    pub fn arguments(self) -> Vec<String> {
        vec![
            "--jpeg-quality".into(), self.jpeg_quality.to_string(),
            "--bit-depth".into(), self.bit_depth.to_string(),
            "--color-space".into(), self.space_name().into(),
        ]
    }
    /// For the save dialog's message; None for sRGB in an sRGB preview. The
    /// preview develops in `preview_space`; afterwards it follows this save.
    pub fn summary(self, export: bool, preview_space: &str) -> Option<String> {
        let space = self.space_name();
        let follows = if space == preview_space { String::new() } else { format!("; the preview then shows {space}") };
        if export {
            return (!self.is_srgb() || !follows.is_empty()).then(|| format!("Develops in {space} and embeds its profile{follows}"));
        }
        if space == preview_space {
            return (!self.is_srgb()).then(|| format!("Keeps the preview's {space} colours and embeds its profile"));
        }
        Some(format!("Converts the {preview_space} preview to {space}, clipping colours outside the smaller space{follows}"))
    }
}

/// The size a save or export is fitted to, like `Fit` in
/// `spektrafilm-io::resize`: full size, or a number of pixels on the long or
/// short edge of the finished file, border included. Never enlarged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSize {
    pub edge: Edge,
    pub pixels: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Full,
    Long,
    Short,
}

impl Edge {
    pub const ALL: [Edge; 3] = [Edge::Full, Edge::Long, Edge::Short];
    pub fn title(self) -> &'static str {
        match self {
            Edge::Full => "Full Size",
            Edge::Long => "Long Edge",
            Edge::Short => "Short Edge",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Edge::Full => "full",
            Edge::Long => "long",
            Edge::Short => "short",
        }
    }
    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|e| e.key() == key).unwrap_or(Edge::Full)
    }
}

pub const PIXEL_RANGE: (u32, u32) = (16, 65_535);

impl ExportSize {
    pub const FULL: ExportSize = ExportSize { edge: Edge::Full, pixels: 2048 };

    /// The `fit` of a save command; None at full size.
    pub fn json(self) -> Option<Value> {
        match self.edge {
            Edge::Full => None,
            Edge::Long => Some(json!({"long_edge": self.pixels})),
            Edge::Short => Some(json!({"short_edge": self.pixels})),
        }
    }
    pub fn arguments(self) -> Vec<String> {
        match self.edge {
            Edge::Full => vec![],
            Edge::Long => vec!["--long-edge".into(), self.pixels.to_string()],
            Edge::Short => vec!["--short-edge".into(), self.pixels.to_string()],
        }
    }
    /// For the save dialog's message; None at full size.
    pub fn summary(self) -> Option<String> {
        (self.edge != Edge::Full).then(|| format!("Fits {} px on the {} edge", self.pixels, if self.edge == Edge::Long { "long" } else { "short" }))
    }
}

/// One undo step: the state to restore and the action's name.
struct UndoStack {
    undo: Vec<(EditState, Cow<'static, str>)>,
    redo: Vec<(EditState, Cow<'static, str>)>,
}

/// What a finished render brings back to the UI thread.
pub struct Rendered {
    pub id: u64,
    pub revision: u64,
    pub generation: u64,
    pub interactive: bool,
    pub result: Value,
    pub picture: Picture,
    pub cached: Option<(String, Frame)>,
}

/// What kind of frame the canvas shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// A render of the photograph at its preview size (export size unless limited).
    Settled,
    /// A canvas-size render while a slider or handle moves.
    Drag,
    /// A canvas-size render of a discrete edit, ahead of its settled frame
    /// (ticket 28, `RenderStep::Quick`).
    Quick,
    /// The previous launch's frame from the rendered-preview cache.
    Cached,
}

impl FrameKind {
    pub fn name(self) -> &'static str {
        match self {
            FrameKind::Settled => "settled",
            FrameKind::Drag => "drag",
            FrameKind::Quick => "quick",
            FrameKind::Cached => "cached",
        }
    }
}

/// What a render develops (the Mac's `RenderStep`): the settled frame (export
/// size, or the 12 MP limit), or as many pixels as the canvas shows (its
/// screen pixels per photo pixel), while a slider moves (`Drag`) or ahead of
/// the settled frame of a discrete edit (`Quick`). Decided when the render
/// starts (`render_step`) and carried to `render_finished`: only a drag
/// frame re-renders on release, and a quick frame settles only through
/// `settle_quick_frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderStep {
    Settled,
    Drag(f64),
    Quick(f64),
}

impl RenderStep {
    pub fn kind(self) -> FrameKind {
        match self {
            RenderStep::Settled => FrameKind::Settled,
            RenderStep::Drag(_) => FrameKind::Drag,
            RenderStep::Quick(_) => FrameKind::Quick,
        }
    }
    pub fn interactive_scale(self) -> Option<f64> {
        match self {
            RenderStep::Settled => None,
            RenderStep::Drag(scale) | RenderStep::Quick(scale) => Some(scale),
        }
    }
}

/// A quick frame only pays when the settled render takes a while (about
/// 50 ms per megapixel on the GPU here, so ~230 ms at the minimum) and the
/// canvas shows at most half of its pixels.
pub const QUICK_FRAME_MIN_PIXELS: f64 = 4.5e6;
pub const QUICK_FRAME_MAX_PIXEL_FRACTION: f64 = 0.5;

/// The render step (the Mac's `renderStep`). `screen_scale` is the canvas's
/// physical screen pixels per photo pixel (`screen_pixels_per_photo_pixel`),
/// `frame_pixels` the photo frame (`image_size`), `has_frame` a developed
/// frame of this photograph on screen (not the restored launch frame, not
/// the placeholder), `quick_done` this edit's quick frame done (or a slider
/// let go), `reuses_output` the helper can answer from its kept output (a
/// post-only edit, which a quick frame would make a full render).
pub fn render_step(interacting: bool, screen_scale: Option<f64>, frame_pixels: f64, has_frame: bool, quick_done: bool, reuses_output: bool) -> RenderStep {
    let scale = screen_scale.filter(|&s| s > 0.0 && s < 1.0);
    if interacting {
        return scale.map_or(RenderStep::Settled, RenderStep::Drag);
    }
    match scale {
        Some(scale)
            if has_frame
                && !quick_done
                && !reuses_output
                && frame_pixels >= QUICK_FRAME_MIN_PIXELS
                && scale * scale <= QUICK_FRAME_MAX_PIXEL_FRACTION =>
        {
            RenderStep::Quick(scale)
        }
        _ => RenderStep::Settled,
    }
}

/// The parts of a render request the helper's reuse of its kept output
/// depends on (the Mac's `reuseKey`): all of it but the preview's path and
/// the post adjustments, which the helper applies to the kept output.
pub fn reuse_key(command: &Value) -> Value {
    let mut key = command.clone();
    if let Some(object) = key.as_object_mut() {
        object.remove("preview_path");
    }
    if key["params"].is_object() {
        key["params"]["post"] = Value::Null;
    }
    key
}

/// A frame that `render_finished` accepted but doesn't show yet: its
/// brightness match is pending, so it may be at the old exposure. Shown once
/// the match leaves the render as it is (`release_held`); any edit drops it.
/// Any kind of frame but a drag frame can be held.
pub struct Held {
    pub revision: u64,
    pub generation: u64,
    pub kind: FrameKind,
    pub rendered: Rendered,
}

/// What waits for a pending brightness match (`Model::settle_outputs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Save,
    Export,
}

impl Output {
    fn verb(self) -> &'static str {
        match self {
            Output::Save => "save",
            Output::Export => "export",
        }
    }
}

/// How long a waiting Save or Export gives a match with nothing running
/// before it gives up, and how often it looks again meanwhile.
const OUTPUT_PATIENCE: Duration = Duration::from_secs(30);

/// Which render the shown image came from: the helper's request id, the
/// edit and photograph it was asked for, and its kind. Only the timing trace
/// (`crate::trace`) and the tests read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTag {
    pub request: u64,
    pub revision: u64,
    pub generation: u64,
    pub kind: FrameKind,
}

/// A render request as the model sent it, for the lifecycle tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub kind: FrameKind,
    pub revision: u64,
    pub generation: u64,
    pub interactive_scale: Option<f64>,
}

pub struct Model {
    pub state: EditState,
    pub defaults: Value,
    /// The catalog's `nullable_paths`: where a saved null is a value.
    nullable_paths: Vec<String>,
    catalog: Value,
    pub stocks: Vec<Stock>,
    pub source: Option<PathBuf>,
    /// The developed preview. None until the first render of `source` completes.
    pub image: Option<Picture>,
    /// The unprocessed photograph, shown while the first render develops.
    pub placeholder: Option<Picture>,
    /// The undeveloped photograph, shown in place of `image` while held.
    pub original: Option<Picture>,
    pub histogram: Option<Histogram>,
    /// The RGB waveform of the same pixels, across the photograph as shown.
    pub waveform: Option<Waveform>,
    /// The tone graph's curve for the current film, paper and settings, or
    /// the last ones until the next arrives.
    pub tone_curve: Option<Curve>,
    /// The photograph's film exposure, from the last render.
    pub film_histogram: Option<FilmHistogram>,
    /// The developed preview's frame, uncropped, which the canvas lays out:
    /// the working frame, or less with the 12 MP limit.
    pub image_size: Size,
    /// The decoded source and the uncapped frame an export develops, from the
    /// last render; zero before one finishes.
    pub source_size: (u32, u32),
    pub working_size: Size,
    /// The settled preview's linear fraction of the working frame, below 1
    /// when limited or larger than the display proxy. Temporary drag frames
    /// leave this unchanged so the Image Size notice doesn't move the rows.
    pub preview_scale: f64,
    /// Why the last typed Longest side wasn't used.
    pub size_error: Option<String>,
    pub placeholder_size: Size,
    pub is_raw: bool,
    pub ready: bool,
    pub rendering: bool,
    pub preview_current: bool,
    /// The shown preview is out of date, and the update is slow enough to mention.
    pub shows_activity: bool,
    /// The latest edit couldn't be developed; the last good preview stays.
    pub render_failed: bool,
    pub saving: bool,
    /// A Balance Print solve is running.
    pub balancing_print: bool,
    pub exporting: bool,
    pub export_started: Instant,
    pub error: Option<String>,
    /// A message and its explanation, shown together (import failures).
    pub notice: Option<(String, String)>,
    pub status: String,
    pub inspector_visible: bool,
    pub fit_image: bool,
    pub zoom_command: u64,
    pub crop_session: Option<CropSession>,
    pub holding_original: bool,
    pub prefs: Preferences,
    pub dirty: u32,
    pub engine: Option<Arc<EngineClient>>,
    paths: Option<EnginePaths>,
    undo: UndoStack,
    export: Option<ExportHandle>,
    revision: u64,
    source_generation: u64,
    render_id: u64,
    /// The last render is at export size: saving it gives the same file as a
    /// Fast export, byte for byte.
    render_at_export_size: bool,
    render_on_gpu: bool,
    /// The output colour space of the last render, which a save in the same
    /// space keeps and a Fast export can reuse.
    rendered_space: String,
    /// How the helper read the photograph's colours (`input_colour`).
    pub input_colour: Value,
    /// The last render's RAW carries lens corrections the decoder can apply
    /// (`lens_correction_supported`), which shows the Lens corrections switch.
    pub lens_supported: bool,
    /// The shown frame was rendered at canvas size while a slider moved;
    /// letting go renders the photograph at export size.
    showing_interactive_frame: bool,
    showing_cached_preview: bool,
    preview_cache: RenderedPreviewCache,
    pending_render: bool,
    activity_timer: bool,
    interaction_start: Option<EditState>,
    closing: bool,
    explicit_open_requested: bool,
    /// The catalog arrived; the session restoration is still loading.
    awaiting_restoration: bool,
    restoration: Option<Result<Option<EditorSession>>>,
    catalog_arrived: bool,
    /// The decoded original, with the `original_key` it was decoded for.
    original_cache: Option<(Value, Picture)>,
    original_loading: bool,
    histogram_request: u64,
    tone_graph_shown: bool,
    /// A curve is being computed; `tone_curve_stale` asks again when it arrives.
    tone_curve_pending: bool,
    tone_curve_stale: bool,
    /// When the app started and when the first developed photograph went on
    /// screen, for the startup report.
    pub first_render_shown: bool,
    pub preset_library: presets::Library,
    /// The Presets folder as last listed, in the user's order.
    pub presets: Vec<PresetFile>,
    /// What the engine made of each listed file, by its identity.
    pub preset_status: HashMap<PresetFile, PresetStatus>,
    /// The preset whose name is being edited.
    pub renaming_preset: Option<PathBuf>,
    /// New Preset's file while it is first named: Escape removes it.
    unnamed_preset: Option<PathBuf>,
    /// The film, paper and preset thumbnails.
    pub previews: Previews,
    /// Lab › Film format › Custom picked while the value is still a format's,
    /// so the slider stays. Start fresh and Reset Adjustments clear it, as
    /// they do the edits.
    pub custom_film_format: bool,
    /// A brightness solve is with the helper.
    pub matching_brightness: bool,
    /// The solve in flight; an answer for another one is dropped. Shared with
    /// the thread that makes the reference, so a superseded one stops early.
    match_request: Arc<std::sync::atomic::AtomicU64>,
    /// A failed solve that trying again can't change: for the photograph
    /// (no inputs, such as a RAW without a camera preview) or for these match
    /// inputs, with the source generation and the note.
    brightness_failure: Option<(u64, Option<Value>, String)>,
    /// Save or Export waiting for a pending brightness match and its preview,
    /// so they write what the inspector shows: what it is, when it was asked
    /// for, and the action (see `settle_outputs`).
    waiting_output: Option<(Instant, Output, Box<dyn FnOnce(&mut Model)>)>,
    /// A settled frame of the current edit, held while its brightness match
    /// is pending: the canvas goes from the previous frame straight to the
    /// matched one (see `render_finished`).
    held: Option<Held>,
    /// The edit (revision, generation) whose quick frame is done: shown, held
    /// for the brightness match, or failed. Its next render settles. Set by a
    /// quick render of the current edit and by letting go of a slider;
    /// cleared once a settled render of the current edit ran, failed or not,
    /// so a failure doesn't retry (ticket 28).
    quick_step: Option<(u64, u64)>,
    /// The shown frame is the quick frame of this edit.
    quick_frame: Option<(u64, u64)>,
    /// The helper's kept output when it is a settled render: the epoch of the
    /// helper that made it (`EngineClient::helper_epoch`) and the request's
    /// `reuse_key`. A request with the same key is a post-only edit, which the
    /// helper answers from it in milliseconds, so it gets no quick frame.
    /// Cleared before any render is sent: the helper replaces its output
    /// with each render and keeps none after a failure.
    helper_output: Option<(u64, Value)>,
    /// The render `image` came from; None for no image.
    pub shown_frame: Option<FrameTag>,
    /// Every render request sent, in order (tests only).
    #[cfg(test)]
    pub render_log: Vec<RenderRequest>,
    /// Every frame presented, in order (tests only): each `present`, so a
    /// frame replaced within one task is still seen.
    #[cfg(test)]
    pub frame_log: Vec<FrameTag>,
}

/// What the adjustments column shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Adjustments,
    Presets,
}

/// Files being imported, one engine check at a time.
struct PresetImport {
    paths: VecDeque<PathBuf>,
    /// Dropped between rows: the index to insert the imported presets at.
    position: Option<usize>,
    added: Vec<PathBuf>,
    failures: Vec<String>,
}

impl Model {
    pub fn new(mut prefs: Preferences) -> Self {
        // Each launch starts with both editing panels available, even if they
        // were hidden in the previous session. F6/F7 still toggle them in use.
        prefs.filmstrip_visible = true;
        Self {
            state: EditState::default(),
            defaults: json!({}),
            nullable_paths: vec![],
            catalog: Value::Null,
            stocks: vec![],
            source: None,
            image: None,
            placeholder: None,
            original: None,
            histogram: None,
            waveform: None,
            tone_curve: None,
            film_histogram: None,
            image_size: Size::ZERO,
            source_size: (0, 0),
            working_size: Size::ZERO,
            preview_scale: 1.0,
            size_error: None,
            placeholder_size: Size::ZERO,
            is_raw: false,
            ready: false,
            rendering: false,
            preview_current: false,
            shows_activity: false,
            render_failed: false,
            saving: false,
            balancing_print: false,
            exporting: false,
            export_started: Instant::now(),
            error: None,
            notice: None,
            status: "Starting…".into(),
            inspector_visible: true,
            fit_image: true,
            zoom_command: 0,
            crop_session: None,
            holding_original: false,
            prefs,
            dirty: dirty::ALL,
            engine: None,
            paths: None,
            undo: UndoStack { undo: vec![], redo: vec![] },
            export: None,
            revision: 0,
            source_generation: 0,
            render_id: 0,
            render_at_export_size: false,
            render_on_gpu: false,
            rendered_space: String::new(),
            input_colour: Value::Null,
            lens_supported: false,
            showing_interactive_frame: false,
            showing_cached_preview: false,
            preview_cache: RenderedPreviewCache::default(),
            pending_render: false,
            activity_timer: false,
            interaction_start: None,
            closing: false,
            explicit_open_requested: false,
            awaiting_restoration: false,
            restoration: None,
            catalog_arrived: false,
            original_cache: None,
            original_loading: false,
            histogram_request: 0,
            tone_graph_shown: false,
            tone_curve_pending: false,
            tone_curve_stale: false,
            first_render_shown: false,
            preset_library: presets::Library::default(),
            presets: vec![],
            preset_status: HashMap::new(),
            renaming_preset: None,
            unnamed_preset: None,
            previews: Previews::default(),
            custom_film_format: false,
            matching_brightness: false,
            match_request: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            brightness_failure: None,
            waiting_output: None,
            held: None,
            quick_step: None,
            quick_frame: None,
            helper_output: None,
            shown_frame: None,
            #[cfg(test)]
            render_log: vec![],
            #[cfg(test)]
            frame_log: vec![],
        }
    }

    fn mark(&mut self, flags: u32) {
        self.dirty |= flags;
    }

    pub fn has_catalog(&self) -> bool {
        !self.stocks.is_empty()
    }
    pub fn films(&self) -> Vec<&Stock> {
        self.stocks.iter().filter(|s| !s.paper).collect()
    }
    pub fn papers(&self) -> Vec<&Stock> {
        self.stocks.iter().filter(|s| s.paper).collect()
    }
    pub fn film_name(&self) -> String {
        self.stocks.iter().find(|s| !s.paper && s.id == self.state.film).map_or_else(|| self.state.film.clone(), |s| s.name.clone())
    }
    pub fn paper_name(&self) -> String {
        self.stocks.iter().find(|s| s.paper && s.id == self.state.paper).map_or_else(|| self.state.paper.clone(), |s| s.name.clone())
    }
    pub fn cropping(&self) -> bool {
        self.crop_session.is_some()
    }
    pub fn can_save(&self) -> bool {
        self.preview_current && self.image.is_some() && !self.rendering && !self.saving && !self.cropping()
    }
    pub fn can_export(&self) -> bool {
        self.source.is_some() && self.ready && !self.exporting && !self.saving && !self.cropping()
    }
    pub fn can_export_lut(&self) -> bool {
        self.has_catalog() && self.engine.is_some() && !self.saving && !self.exporting && !self.cropping()
    }
    pub fn can_balance_print(&self) -> bool {
        self.has_catalog() && self.engine.is_some() && !self.balancing_print && !self.cropping() && !json::boolean(self.value("io.scan_film"))
    }
    pub fn can_show_original(&self) -> bool {
        self.image.is_some() && !self.cropping()
    }
    pub fn interacting(&self) -> bool {
        self.interaction_start.is_some()
    }
    pub fn rotation(&self) -> i32 {
        self.state.rotation()
    }
    /// The shown photograph's size after cropping, before rotation; zero
    /// before one opens. Borders fit this size.
    pub fn photo_size(&self) -> Size {
        self.state.crop.size_of(if self.image.is_some() { self.image_size } else { self.placeholder_size })
    }
    pub fn size_mode(&self) -> state::SizeMode {
        state::size_mode(&self.state.params)
    }
    /// The frame and photograph the current edit develops, uncapped, with the
    /// engine's resolver; None until the source's size is known.
    pub fn resolved_size(&self) -> Option<Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError>> {
        (self.source_size.0 > 0).then(|| state::resolve_size(self.source_size, &self.state.params, &self.state.crop))
    }
    /// The exported photograph, cropped, before rotation, borders and export
    /// reductions; zero until known.
    pub fn photograph_size(&self) -> Size {
        match self.resolved_size() {
            Some(Ok(size)) => Size::new(size.photo.0 as f64, size.photo.1 as f64),
            _ => Size::ZERO,
        }
    }
    /// The crop being drawn, resolved like an export: with a target, a
    /// smaller crop needs a larger frame. None before the source's size is known.
    pub fn crop_draft_size(&self) -> Option<Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError>> {
        let session = self.crop_session.as_ref()?;
        (self.source_size.0 > 0).then(|| state::resolve_size(self.source_size, &session.start.params, &session.settings()))
    }
    /// The developed photograph as shown, uncropped, after rotation.
    fn shown_image_size(&self) -> Size {
        if self.rotation() % 2 != 0 { self.image_size.swapped() } else { self.image_size }
    }
    pub fn undo_title(&self) -> Option<&str> {
        self.undo.undo.last().map(|u| u.1.as_ref())
    }
    pub fn redo_title(&self) -> Option<&str> {
        self.undo.redo.last().map(|u| u.1.as_ref())
    }
    pub fn render_backend_gpu(&self) -> bool {
        self.render_on_gpu
    }

    // MARK: Starting

    /// Starts the helper and asks for its catalog at once, so both load while
    /// the window is built. The last launch's catalog, if the helper is
    /// unchanged, fills in the stocks so the inspector's first frame is complete.
    pub fn launch(&mut self) {
        let paths = match EnginePaths::locate() {
            Ok(paths) => paths,
            Err(error) => return self.report(&error),
        };
        let engine = match EngineClient::start(paths.clone()) {
            Ok(engine) => Arc::new(engine),
            Err(error) => return self.report(&error),
        };
        engine.request(json!({"command": "catalog"}), engine::timeout_for("catalog"), |result| {
            ui::post(move |app: &mut App| app.model.catalog_arrived(result));
        });
        if let Some(cached) = CatalogCache::load(&paths.helper, &paths.data) {
            self.apply_catalog(cached);
        }
        crate::picture::load_border_textures(&paths.data);
        self.engine = Some(engine);
        self.paths = Some(paths);
    }

    /// The session file loads concurrently with the catalog; apply it
    /// before the first render so no render with defaults is wasted.
    pub fn expect_restoration(&mut self) {
        self.awaiting_restoration = true;
    }

    pub fn restoration_loaded(&mut self, result: Result<Option<EditorSession>>) {
        self.awaiting_restoration = false;
        self.restoration = Some(result);
        if self.catalog_arrived {
            self.finish_start();
        }
    }

    fn catalog_arrived(&mut self, result: Result<(u64, Value)>) {
        match result {
            Ok((_, catalog)) => {
                if catalog["protocol"].as_f64() != Some(1.0) {
                    return self.report(&anyhow!("The image engine version doesn't match this app. Install Negs again."));
                }
                // An unchanged catalog keeps edits made while the helper started.
                if catalog != self.catalog {
                    if let Some(paths) = &self.paths {
                        CatalogCache::save(&catalog, &paths.helper, &paths.data);
                    }
                    self.apply_catalog(catalog);
                }
                self.catalog_arrived = true;
                if !self.awaiting_restoration {
                    self.finish_start();
                }
            }
            Err(error) => self.report(&error),
        }
    }

    fn finish_start(&mut self) {
        if let Some(restoration) = self.restoration.take() {
            if !self.explicit_open_requested {
                match restoration.and_then(|s| match s {
                    Some(session) => self.restore(session),
                    None => Ok(()),
                }) {
                    Ok(()) => {}
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        self.ready = true;
        self.check_presets();
        if self.source.is_none() && self.status == "Starting…" {
            self.status.clear();
        }
        self.mark(dirty::STATUS | dirty::CHROME | dirty::INSPECTOR);
        if self.source.is_some() {
            self.changed_with_cache(false);
        }
    }

    fn apply_catalog(&mut self, catalog: Value) {
        self.stocks = catalog["profiles"].as_array().map(|a| a.iter().map(Stock::new).collect()).unwrap_or_default();
        self.defaults = catalog["defaults"].clone();
        // The engine leaves lens corrections off for params from before them,
        // so their crops keep their place; the app corrects every new edit.
        json::set(&mut self.defaults, state::LENS_CORRECTION, Value::Bool(true));
        self.nullable_paths = catalog["nullable_paths"]
            .as_array()
            .map(|paths| paths.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        self.catalog = catalog;
        // Keep anything already applied to the photograph, such as its rotation.
        let rotation = self.state.value("io.rotate_cw").clone();
        let mut next = self.state.clone();
        next.params = self.defaults.clone();
        if !rotation.is_null() {
            json::set(&mut next.params, "io.rotate_cw", rotation);
        }
        if !self.stocks.iter().any(|s| !s.paper && s.id == next.film) {
            if let Some(first) = self.stocks.iter().find(|s| !s.paper) {
                next.film = first.id.clone();
            }
        }
        if !self.stocks.iter().any(|s| s.paper && s.id == next.paper) {
            if let Some(first) = self.stocks.iter().find(|s| s.paper) {
                next.paper = first.id.clone();
            }
        }
        self.pair_film(&mut next);
        self.state = next;
        self.mark(dirty::STRUCTURE | dirty::INSPECTOR | dirty::CHROME | dirty::STATUS);
        self.schedule_tone_curve();
    }

    // MARK: Parameters

    pub fn value(&self, path: &str) -> &Value {
        let value = self.state.value(path);
        match state::grain_lane(path) {
            Some((field, index)) if value.is_null() => self.grain_preset(field, index),
            _ => value,
        }
    }
    /// A grain preset lane defaults to the film's preset, not the engine's
    /// null.
    pub fn default_value(&self, path: &str) -> &Value {
        match state::grain_lane(path) {
            Some((field, index)) => self.grain_preset(field, index),
            None => json::get(&self.defaults, path),
        }
    }
    fn film_stock(&self) -> Option<&Stock> {
        self.stocks.iter().find(|s| !s.paper && s.id == self.state.film)
    }
    pub fn film_is_bw(&self) -> bool {
        self.film_stock().is_some_and(|s| s.bw)
    }
    fn grain_preset(&self, field: &str, index: usize) -> &Value {
        static NULL: Value = Value::Null;
        self.film_stock().map_or(&NULL, |s| json::get(&s.grain, &format!("{field}.{index}")))
    }
    pub fn is_modified(&self, path: &str) -> bool {
        let (current, original) = (self.value(path), self.default_value(path));
        if let (Some(a), Some(b)) = (current.as_f64(), original.as_f64()) {
            // Rust defaults arrive as widened f32 values; slider edits are decimal.
            return (a - b).abs() > 1e-6 * b.abs().max(1.0);
        }
        !json::same(current, original)
    }
    pub fn set(&mut self, path: &str, value: Value) {
        let mut next = self.state.clone();
        if let Some((field, index)) = state::grain_lane(path) {
            // The whole array materializes from the preset; back at the
            // preset in every lane it returns to null, so a later film
            // change applies that film's preset.
            let array = format!("film_render.grain.{field}");
            let preset: Vec<f64> = self
                .film_stock()
                .and_then(|s| s.grain[field].as_array())
                .map(|a| a.iter().map(json::number).collect())
                .unwrap_or_default();
            let mut lanes: Vec<f64> = match self.state.value(&array).as_array() {
                Some(a) if !a.is_empty() => a.iter().map(json::number).collect(),
                _ => preset.clone(),
            };
            if index < lanes.len() {
                lanes[index] = json::number(&value);
            }
            let at_preset = lanes.len() == preset.len()
                && lanes.iter().zip(&preset).all(|(a, b)| (a - b).abs() <= 1e-6 * b.abs().max(1.0));
            let new = if at_preset { Value::Null } else { Value::Array(lanes.into_iter().map(json::num).collect()) };
            json::set(&mut next.params, &array, new);
        } else {
            json::set(&mut next.params, path, value);
        }
        self.apply(next, "Change Adjustment");
    }
    pub fn reset_value(&mut self, path: &str) {
        let value = self.default_value(path).clone();
        self.set(path, value);
    }

    pub fn begin_interaction(&mut self) {
        if self.interaction_start.is_none() {
            self.interaction_start = Some(self.state.clone());
        }
        self.hide_activity();
    }

    pub fn end_interaction(&mut self, action: &'static str) {
        let Some(start) = self.interaction_start.take() else { return };
        if start != self.state {
            self.register_undo(start, action);
        }
        // The drag frames were canvas-size already: letting go settles,
        // without a quick frame.
        self.quick_step = Some((self.revision, self.source_generation));
        if self.showing_interactive_frame && self.ready && self.source.is_some() && !self.closing {
            self.schedule_render();
        }
        if !self.preview_current {
            self.schedule_activity();
        }
        // A press without a change kept a held frame's solve from starting,
        // and a quick frame from settling.
        self.release_held();
        self.settle_quick_frame();
        self.settle_outputs();
        self.mark(dirty::CHROME);
    }

    pub fn choose_film(&mut self, stock: &str) {
        let next = state::choosing(&self.state, stock, false, self.prefs.keep_paper, &self.stocks);
        self.apply(next, "Change Film");
    }

    pub fn choose_paper(&mut self, stock: &str) {
        let next = state::choosing(&self.state, stock, true, self.prefs.keep_paper, &self.stocks);
        self.apply(next, "Change Paper");
    }

    /// When the catalog loads, the film brings its own paper whatever the setting.
    fn pair_film(&self, state: &mut EditState) {
        state::pair_film(state, &self.stocks, true);
    }

    /// Original develops the source's own pixels; Longest side resizes the
    /// source so the cropped photograph has that many pixels on its longest side.
    pub fn choose_image_size(&mut self, mode: state::SizeMode) {
        self.size_error = None;
        match mode {
            state::SizeMode::Original => {
                let mut next = self.state.clone();
                json::set(&mut next.params, state::SIZE_TARGET, Value::Null);
                self.apply(next, "Change Image Size");
            }
            state::SizeMode::LongestSide if self.size_mode() != state::SizeMode::LongestSide => {
                let pixels = self.prefs.image_size_long_side.filter(|p| PHOTO_LONG_SIDE_RANGE.contains(p)).unwrap_or_else(|| self.native_long_side());
                let _ = self.set_longest_side(pixels);
            }
            _ => {}
        }
        self.mark(dirty::INSPECTOR);
    }

    /// The crop's longest side in the source's own pixels, within the field's range.
    fn native_long_side(&self) -> u32 {
        let source = if self.source_size.0 > 0 { Size::new(self.source_size.0 as f64, self.source_size.1 as f64) } else { self.placeholder_size };
        let size = self.state.crop.size_of(source);
        let side = size.width.max(size.height) as u32;
        if side == 0 { 4000 } else { side.clamp(*PHOTO_LONG_SIDE_RANGE.start(), *PHOTO_LONG_SIDE_RANGE.end()) }
    }

    /// Sets a Longest side target, or explains why it can't be used and
    /// changes nothing.
    pub fn set_longest_side(&mut self, pixels: u32) -> std::result::Result<(), String> {
        let mut next = self.state.clone();
        json::set(&mut next.params, state::SIZE_TARGET, json::num(pixels as f64));
        let checked = if !PHOTO_LONG_SIDE_RANGE.contains(&pixels) {
            Err(format!("Enter a whole number from {} to {} px.", PHOTO_LONG_SIDE_RANGE.start(), PHOTO_LONG_SIDE_RANGE.end()))
        } else if self.source_size.0 > 0 {
            state::resolve_size(self.source_size, &next.params, &next.crop).map(|_| ()).map_err(|e| e.to_string())
        } else {
            Ok(())
        };
        self.size_error = checked.clone().err();
        self.mark(dirty::INSPECTOR);
        checked?;
        self.prefs.image_size_long_side = Some(pixels);
        self.prefs.save();
        self.apply(next, "Change Image Size");
        Ok(())
    }

    /// Settings › Limit preview to 12 MP. A preference, outside Undo and
    /// Reset; the state carries a copy so the render and its cache follow it.
    pub fn set_limit_preview(&mut self, enabled: bool) {
        if self.prefs.limit_preview == Some(enabled) {
            return;
        }
        self.prefs.limit_preview = Some(enabled);
        self.prefs.save();
        self.replace_state(self.state.clone());
    }

    /// Settings › Use less memory for export: only exports read it.
    pub fn set_low_memory_export(&mut self, enabled: bool) {
        if self.prefs.low_memory_export != enabled {
            self.prefs.low_memory_export = enabled;
            self.prefs.save();
        }
    }

    /// Settings › New photographs: Start fresh, or keep the previous edits.
    pub fn set_fresh_edits(&mut self, fresh: bool) {
        if self.prefs.fresh_edits != fresh {
            self.prefs.fresh_edits = fresh;
            self.prefs.save();
        }
    }

    /// Settings › Match new photographs to camera brightness.
    pub fn set_match_new_photos(&mut self, on: bool) {
        if self.prefs.match_new_photos != on {
            self.prefs.match_new_photos = on;
            self.prefs.save();
        }
    }

    /// The state with the app's preferences that travel in it.
    fn with_preferences(&self, mut state: EditState) -> EditState {
        state.limit_preview = self.prefs.limits_preview();
        state
    }

    /// A new match for a new photograph or Reset, when Settings asks for one.
    fn new_match(&self) -> Option<state::BrightnessMatch> {
        self.prefs.match_new_photos.then(state::BrightnessMatch::default)
    }

    /// Lab › Film format: a stills format writes its long edge; Custom keeps
    /// the value and shows the slider.
    pub fn choose_film_format(&mut self, format: Option<usize>) {
        match format.and_then(|i| state::FILM_FORMATS.get(i)) {
            Some((_, mm)) => {
                self.custom_film_format = false;
                self.set(state::FILM_FORMAT.path(), json::num(*mm));
            }
            None => self.custom_film_format = true,
        }
        self.mark(dirty::INSPECTOR | dirty::STRUCTURE);
    }

    /// Whether the Lab shows the long-edge slider.
    pub fn film_format_custom(&self) -> bool {
        self.custom_film_format || state::film_format(json::number(self.value(state::FILM_FORMAT.path()))).is_none()
    }

    /// Stars a film or paper, or unstars it; favourites lead the grid.
    pub fn toggle_favorite(&mut self, id: &str, paper: bool) {
        self.prefs.toggle_favorite(id, paper);
        self.prefs.save();
        self.mark(dirty::STRUCTURE | dirty::INSPECTOR | dirty::PREVIEWS);
    }

    /// The starred stocks of one kind that the catalog has, in the order starred.
    pub fn favorite_stocks(&self, paper: bool) -> Vec<&Stock> {
        self.prefs.favorites(paper).iter().filter_map(|id| self.stocks.iter().find(|s| s.paper == paper && &s.id == id)).collect()
    }

    pub fn set_border(&mut self, change: impl FnOnce(&mut BorderSettings), action: &'static str) {
        let mut next = self.state.clone();
        change(&mut next.border);
        self.apply(next, action);
    }

    /// A border width in percent of the long edge, stepped like the
    /// adjustment sliders. Back at the default it stores the default exactly,
    /// so exports match ones made before widths could change.
    pub fn set_border_percent(&mut self, field: BorderWidth, percent: f64) {
        let (low, high, step) = field.range();
        let fraction = crate::state::snap(percent, low, high, step) / 100.0;
        let initial = field.get(&BorderSettings::default());
        let value = if (fraction - initial).abs() < 1e-9 { initial } else { fraction };
        self.set_border(|b| field.set(b, value), "Change Border");
    }

    pub fn set_border_color(&mut self, line: bool, color: BorderColor) {
        self.set_border(|b| if line { b.line_color = color } else { b.color = color }, "Change Border Color");
    }

    /// Keeps the border, crop, and Image Size's Longest side, like the film,
    /// paper, and rotation.
    pub fn reset(&mut self) {
        if self.cropping() {
            return;
        }
        let mut next = self.state.clone();
        next.params = self.defaults.clone();
        next.brightness_match = self.new_match();
        json::set(&mut next.params, "io.rotate_cw", self.state.value("io.rotate_cw").clone());
        json::set(&mut next.params, state::SIZE_TARGET, self.state.value(state::SIZE_TARGET).clone());
        let positive = self.stocks.iter().find(|s| !s.paper && s.id == next.film).is_some_and(|s| s.positive);
        json::set(&mut next.params, "io.scan_film", Value::Bool(positive));
        self.custom_film_format = false;
        self.mark(dirty::STRUCTURE);
        self.apply(next, "Reset Adjustments");
    }

    fn apply(&mut self, next: EditState, action: impl Into<Cow<'static, str>>) {
        if next == self.state {
            return;
        }
        if !self.interacting() {
            let previous = self.state.clone();
            self.register_undo(previous, action);
        }
        self.replace_state(next);
    }

    /// A border, crop, or rotation change only redraws the canvas; anything
    /// else re-renders.
    fn replace_state(&mut self, next: EditState) {
        let next = self.with_preferences(next);
        let render = next.rendered() != self.state.rendered();
        let structure = self.structure_key(&next) != self.structure_key(&self.state);
        let crop_changed = next.crop != self.state.crop;
        let rotation_changed = next.rotation() != self.state.rotation();
        self.state = next;
        if rotation_changed {
            let (shown, rotation) = (self.shown_image_size(), self.rotation());
            if let Some(session) = &mut self.crop_session {
                session.update(shown, rotation);
            }
        }
        if crop_changed || rotation_changed {
            self.update_histogram();
        }
        self.mark(dirty::INSPECTOR | dirty::CANVAS | dirty::CHROME | if structure { dirty::STRUCTURE } else { 0 });
        if render {
            self.changed();
        }
        self.reconcile_brightness_match();
        self.release_held();
        self.settle_outputs();
    }

    /// What decides which inspector rows exist.
    fn structure_key(&self, state: &EditState) -> (String, String, bool, bool, String, BorderStyle, bool) {
        (
            state.film.clone(),
            state.paper.clone(),
            json::boolean(state.value("io.scan_film")),
            json::boolean(state.value("camera.auto_exposure")),
            json::string(state.value("io.raw_white_balance.mode")).to_string(),
            state.border.style,
            state.border.enabled,
        )
    }

    fn register_undo(&mut self, previous: EditState, action: impl Into<Cow<'static, str>>) {
        self.undo.undo.push((previous, action.into()));
        self.undo.redo.clear();
        self.mark(dirty::CHROME);
    }

    pub fn perform_undo(&mut self) {
        if self.cropping() {
            return;
        }
        let Some((previous, action)) = self.undo.undo.pop() else { return };
        let previous = self.with_preferences(previous);
        if previous != self.state {
            self.undo.redo.push((self.state.clone(), action));
            self.replace_state(previous);
        }
        self.mark(dirty::CHROME | dirty::ZOOM);
        self.zoom_after_undo();
    }

    pub fn perform_redo(&mut self) {
        if self.cropping() {
            return;
        }
        let Some((next, action)) = self.undo.redo.pop() else { return };
        let next = self.with_preferences(next);
        if next != self.state {
            self.undo.undo.push((self.state.clone(), action));
            self.replace_state(next);
        }
        self.mark(dirty::CHROME);
        self.zoom_after_undo();
    }

    /// A different crop is a new picture: the canvas refits it.
    fn zoom_after_undo(&mut self) {
        self.mark(dirty::CANVAS);
    }

    pub fn rotate(&mut self) {
        let turned = json::num(((self.rotation() + 1) % 4) as f64);
        if self.cropping() {
            // Turns with the crop: one undo step on Done, undone by Cancel.
            let mut next = self.state.clone();
            json::set(&mut next.params, "io.rotate_cw", turned);
            self.replace_state(next);
        } else {
            self.set("io.rotate_cw", turned);
            self.zoom(true);
        }
    }

    pub fn zoom(&mut self, fit: bool) {
        self.fit_image = fit;
        self.zoom_command += 1;
        self.mark(dirty::ZOOM | dirty::CANVAS);
    }

    pub fn session(&self) -> Option<EditorSession> {
        let source = self.source.clone()?;
        if !self.ready {
            return None;
        }
        let mut saved = self.state.clone();
        // Include a crop being drawn, without forcing the user to press Done.
        if let Some(session) = &self.crop_session {
            saved.crop = session.settings();
        }
        Some(EditorSession { schema: EditorSession::SCHEMA, source, state: saved, inspector_visible: self.inspector_visible, fit_image: self.fit_image })
    }

    fn restore(&mut self, session: EditorSession) -> Result<()> {
        session.validate()?;
        let known = |id: &str, paper: bool| self.stocks.iter().any(|s| s.paper == paper && s.id == id);
        if !known(&session.state.film, false) || !known(&session.state.paper, true) {
            return Err(anyhow!("The previous session uses an unavailable film or paper. Open a photograph to continue."));
        }
        let source = session.source.clone();
        self.open_restoring(&source, Some(session));
        Ok(())
    }

    // MARK: Presets

    pub fn pane(&self) -> Pane {
        if self.prefs.inspector_pane == "Presets" { Pane::Presets } else { Pane::Adjustments }
    }

    /// The toolbar's Adjustments and Presets buttons: show that panel, or
    /// hide the column if it already shows it.
    pub fn toggle_pane(&mut self, pane: Pane) {
        if self.inspector_visible && self.pane() == pane {
            self.inspector_visible = false;
            self.mark(dirty::LAYOUT | dirty::CHROME);
        } else {
            self.show_pane(pane);
        }
    }

    /// The title bar's panel button: hide the column, or show it with the
    /// panel it last showed.
    pub fn toggle_inspector(&mut self) {
        if self.inspector_visible {
            self.inspector_visible = false;
            self.mark(dirty::LAYOUT | dirty::CHROME);
        } else {
            self.show_pane(self.pane());
        }
    }

    /// Shows a panel, remembered across launches. Opening Presets lists the
    /// folder again, so files copied in by hand appear.
    pub fn show_pane(&mut self, pane: Pane) {
        let opening = !self.inspector_visible || self.pane() != pane;
        self.inspector_visible = true;
        if self.pane() != pane {
            self.prefs.inspector_pane = if pane == Pane::Presets { "Presets" } else { "Adjustments" }.into();
            self.prefs.save();
        }
        if opening && pane == Pane::Presets {
            self.refresh_presets();
        }
        self.mark(dirty::LAYOUT | dirty::CHROME | dirty::PRESETS);
    }

    pub fn can_change_presets(&self) -> bool {
        self.has_catalog() && !self.cropping() && self.engine.is_some()
    }

    /// Lists the folder again and asks the engine about files it hasn't seen.
    pub fn refresh_presets(&mut self) {
        match self.preset_library.list() {
            Ok(presets) => self.presets = presets,
            Err(error) => {
                self.presets.clear();
                self.report(&error);
            }
        }
        let presets = &self.presets;
        self.preset_status.retain(|file, _| presets.contains(file));
        self.previews.retain_presets(&self.presets.iter().map(|p| p.path.clone()).collect::<Vec<_>>());
        self.mark(dirty::PRESETS | dirty::PREVIEWS);
        self.check_presets();
    }

    /// Checks unchecked presets once the photograph is on screen, so a
    /// remembered Presets panel never delays the first render.
    fn check_presets(&mut self) {
        let Some(engine) = self.engine.clone() else { return };
        if !self.ready || (self.source.is_some() && self.image.is_none()) {
            return;
        }
        let unchecked: Vec<PresetFile> = self.presets.iter().filter(|p| !self.preset_status.contains_key(p)).cloned().collect();
        if unchecked.is_empty() {
            return;
        }
        for preset in unchecked {
            let text = match self.preset_library.read(&preset).and_then(|data| presets::text(&data)) {
                Ok(text) => text,
                Err(error) => {
                    self.preset_status.insert(preset, PresetStatus::Invalid(format!("{error:#}")));
                    continue;
                }
            };
            self.preset_status.insert(preset.clone(), PresetStatus::Checking);
            let command = json!({"command": "preset_import", "text": text});
            engine.request(command, engine::timeout_for("preset_import"), move |result| {
                ui::post(move |app: &mut App| app.model.preset_checked(preset, text, result));
            });
        }
        self.mark(dirty::PRESETS);
    }

    fn preset_checked(&mut self, preset: PresetFile, text: String, result: Result<(u64, Value)>) {
        if !self.presets.contains(&preset) {
            return;
        }
        let status = match result {
            Ok((_, result)) => PresetStatus::Valid {
                film: json::string(&result["film"]).into(),
                paper: json::string(&result["paper"]).into(),
                scan: json::boolean(&result["params"]["io"]["scan_film"]),
                text,
                white_balance: result["params"]["io"]["raw_white_balance"].clone(),
            },
            Err(error) => PresetStatus::Invalid(format!("{error:#}")),
        };
        self.preset_status.insert(preset, status);
        self.mark(dirty::PRESETS);
    }

    /// The film, paper and Lab settings of a preset, as one undo step. The
    /// photograph's own settings stay: Image Size, rotation, crop, border,
    /// output encoding, the preview limit and the Post tab.
    pub fn apply_preset(&mut self, preset: &PresetFile) {
        if !self.can_change_presets() {
            return;
        }
        match self.preset_library.read(preset).and_then(|data| presets::text(&data)) {
            Ok(text) => self.request_preset(preset.name(), text, 0),
            Err(error) => {
                self.report(&error);
                self.refresh_presets();
            }
        }
    }

    /// The engine keeps the photograph's settings from the params it's
    /// given; ask again if they changed while it answered.
    fn request_preset(&mut self, name: String, text: String, attempt: u32) {
        let Some(engine) = self.engine.clone() else { return };
        let snapshot = self.state.clone();
        let command = json!({"command": "preset_import", "text": text, "params": snapshot.params});
        engine.request(command, engine::timeout_for("preset_import"), move |result| {
            ui::post(move |app: &mut App| app.model.preset_imported(name, text, snapshot, attempt, result));
        });
    }

    fn preset_imported(&mut self, name: String, text: String, snapshot: EditState, attempt: u32, result: Result<(u64, Value)>) {
        let result = match result {
            Ok((_, result)) => result,
            Err(error) => {
                self.report(&error);
                return self.refresh_presets();
            }
        };
        if !self.can_change_presets() {
            return;
        }
        if self.state != snapshot {
            if attempt < 2 {
                self.request_preset(name, text, attempt + 1);
            }
            return;
        }
        let (film, paper) = (json::string(&result["film"]).to_owned(), json::string(&result["paper"]).to_owned());
        let known = |id: &str, paper: bool| self.stocks.iter().any(|s| s.paper == paper && s.id == id);
        if !known(&film, false) || !known(&paper, true) {
            return self.report(&anyhow!("This version of Negs doesn’t have the preset’s film or paper."));
        }
        // Not `choose_film`: its stock-preset resets would discard the preset's values.
        let mut next = self.state.clone();
        next.film = film;
        next.paper = paper;
        next.params = result["params"].clone();
        self.apply(next, format!("Apply Preset “{name}”"));
        self.status = format!("Applied preset “{name}”");
        self.mark(dirty::STATUS);
    }

    /// Saves the film, paper and Lab settings as a new preset, named after
    /// the film, at the end of the list, and starts renaming it.
    pub fn new_preset(&mut self) {
        if !self.can_change_presets() {
            return;
        }
        self.show_pane(Pane::Presets);
        let Some(engine) = self.engine.clone() else { return };
        let name = presets::name::from_file_name(&self.film_name());
        let command = json!({"command": "preset_export", "film": self.state.film, "paper": self.state.paper, "params": self.state.params});
        engine.request(command, engine::timeout_for("preset_export"), move |result| {
            ui::post(move |app: &mut App| app.model.preset_exported(name, result));
        });
    }

    fn preset_exported(&mut self, name: String, result: Result<(u64, Value)>) {
        let added = result.and_then(|(_, result)| {
            let text = result["text"].as_str().ok_or_else(|| anyhow!("The image engine didn’t return the preset."))?;
            self.preset_library.add(text.as_bytes(), &name)
        });
        match added {
            Ok(file) => {
                self.refresh_presets();
                self.unnamed_preset = Some(file.path.clone());
                self.renaming_preset = Some(file.path);
            }
            Err(error) => self.report(&error),
        }
    }

    /// Copies each file that the engine reads into the Presets folder, under
    /// its own name, and lists the ones that it refuses. They go to the end,
    /// or before the preset at `position` (a drop between rows).
    pub fn import_presets(&mut self, paths: Vec<PathBuf>, position: Option<usize>) {
        if !self.can_change_presets() || paths.is_empty() {
            return;
        }
        self.show_pane(Pane::Presets);
        self.import_next(PresetImport { paths: paths.into(), position, added: vec![], failures: vec![] });
    }

    fn import_next(&mut self, mut job: PresetImport) {
        while let Some(path) = job.paths.pop_front() {
            let read = presets::read_file(&path).and_then(|data| Ok((presets::text(&data)?, data)));
            match (read, self.engine.clone()) {
                (Ok(_), None) => job.failures.push(format!("{}: {}", file_name(&path), engine::STOPPED)),
                (Ok((text, data)), Some(engine)) => {
                    engine.request(json!({"command": "preset_import", "text": text}), engine::timeout_for("preset_import"), move |result| {
                        ui::post(move |app: &mut App| app.model.import_checked(job, path, data, result));
                    });
                    return;
                }
                (Err(error), _) => job.failures.push(format!("{}: {error:#}", file_name(&path))),
            }
        }
        self.finish_import(job);
    }

    fn import_checked(&mut self, mut job: PresetImport, path: PathBuf, data: Vec<u8>, result: Result<(u64, Value)>) {
        let name = presets::name::from_file_name(&path.file_stem().unwrap_or_default().to_string_lossy());
        match result.and_then(|_| self.preset_library.add(&data, &name)) {
            Ok(file) => job.added.push(file.path),
            Err(error) => job.failures.push(format!("{}: {error:#}", file_name(&path))),
        }
        self.import_next(job);
    }

    fn finish_import(&mut self, job: PresetImport) {
        let imported = job.added.len();
        if let (Some(position), true) = (job.position, imported > 0) {
            if let Ok(listed) = self.preset_library.list() {
                let added = |p: &PresetFile| job.added.contains(&p.path);
                let moved: Vec<PresetFile> = listed.iter().filter(|p| added(p)).cloned().collect();
                let mut order: Vec<PresetFile> = listed.iter().filter(|p| !added(p)).cloned().collect();
                let before = listed.iter().take(position).filter(|p| !added(p)).count().min(order.len());
                order.splice(before..before, moved);
                if let Err(error) = self.preset_library.set_order(&order) {
                    self.report(&error);
                }
            }
        }
        self.refresh_presets();
        if imported > 0 {
            self.status = if imported == 1 { "Imported 1 preset".into() } else { format!("Imported {imported} presets") };
            self.mark(dirty::STATUS);
        }
        if !job.failures.is_empty() {
            let lead = if imported == 0 { "No presets were imported." } else { "Some files weren’t imported." };
            self.notice = Some((lead.into(), job.failures.join("\n\n")));
            self.mark(dirty::ERROR);
        }
    }

    /// The presets in a new order, as dragged; remembered at once.
    pub fn move_presets(&mut self, order: Vec<PresetFile>) {
        if order == self.presets {
            return;
        }
        self.presets = order;
        if let Err(error) = self.preset_library.set_order(&self.presets) {
            self.report(&error);
        }
        self.refresh_presets();
    }

    /// Escape while naming: keeps the old name, or removes the preset New
    /// Preset just made.
    pub fn cancel_rename(&mut self, preset: &PresetFile) {
        self.renaming_preset = None;
        self.mark(dirty::PRESETS);
        if self.unnamed_preset.as_ref() != Some(&preset.path) {
            return;
        }
        self.unnamed_preset = None;
        if let Err(error) = self.preset_library.delete(preset) {
            self.report(&error);
        }
        self.refresh_presets();
    }

    pub fn rename_preset(&mut self, preset: &PresetFile, name: &str) {
        self.renaming_preset = None;
        self.mark(dirty::PRESETS);
        if self.unnamed_preset.as_ref() == Some(&preset.path) {
            self.unnamed_preset = None;
        }
        if name == preset.name() {
            return;
        }
        match self.preset_library.rename(preset, name) {
            Ok(renamed) => self.previews.move_preset(&preset.path, &renamed.path),
            Err(error) => self.report(&error),
        }
        self.refresh_presets();
    }

    pub fn delete_preset(&mut self, preset: &PresetFile) {
        if let Err(error) = self.preset_library.delete(preset) {
            self.report(&error);
        }
        self.refresh_presets();
    }

    /// Writes a copy wherever the user picked, byte for byte.
    pub fn export_preset(&mut self, preset: &PresetFile, destination: &Path) {
        let copied = self.preset_library.read(preset).and_then(|data| crate::session::write_atomic(destination, &data));
        if let Err(error) = copied {
            self.report(&error);
            self.refresh_presets();
        }
    }

    // MARK: Cropping

    /// Shows the whole developed photograph, without the border, to crop it.
    pub fn begin_crop(&mut self) {
        if self.cropping() || self.image.is_none() || self.image_size.is_zero() {
            return;
        }
        self.hold_original(false);
        self.crop_session = Some(CropSession::new(self.state.clone(), self.shown_image_size(), self.rotation()));
        self.mark(dirty::CROP | dirty::CANVAS | dirty::CHROME | dirty::LAYOUT | dirty::STRIP);
    }

    /// Done applies the crop, and any rotation while cropping, as one undo
    /// step. Cancel restores the rotation and keeps the previous crop.
    pub fn end_crop(&mut self, apply: bool) {
        // A crop too small for the Longest side stays a draft, with its error shown.
        if apply && matches!(self.crop_draft_size(), Some(Err(spektrafilm_io::image_size::SizeError::TooLarge { .. }))) {
            self.mark(dirty::CROP);
            return;
        }
        let Some(session) = self.crop_session.take() else { return };
        let rotation = session.start.value("io.rotate_cw").clone();
        if apply {
            let mut previous = self.state.clone();
            previous.crop = session.start.crop;
            json::set(&mut previous.params, "io.rotate_cw", rotation);
            let mut next = self.state.clone();
            next.crop = session.settings();
            if next != previous {
                self.register_undo(previous, "Crop");
            }
            self.replace_state(next);
        } else if !json::same(self.state.value("io.rotate_cw"), &rotation) {
            let mut next = self.state.clone();
            json::set(&mut next.params, "io.rotate_cw", rotation);
            self.replace_state(next);
        }
        // Cropping kept a held frame's solve from starting.
        self.release_held();
        self.settle_outputs();
        self.mark(dirty::CROP | dirty::CANVAS | dirty::CHROME | dirty::LAYOUT | dirty::STRIP);
        self.zoom(true);
    }

    pub fn crop_changed(&mut self) {
        self.mark(dirty::CANVAS | dirty::CROP);
    }

    // MARK: Opening

    pub fn open(&mut self, path: &Path) {
        self.explicit_open_requested = true;
        self.open_restoring(path, None);
    }

    fn open_restoring(&mut self, path: &Path, session: Option<EditorSession>) {
        if std::fs::File::open(path).is_err() {
            self.prefs.recent_files.retain(|p| !crate::session::same_path(p, path));
            self.prefs.save();
            self.error = Some(format!("“{}” can’t be opened. It may have been moved or deleted.", file_name(path)));
            self.mark(dirty::ERROR | dirty::CHROME);
            return;
        }
        self.source = Some(path.to_path_buf());
        self.image = None;
        self.shown_frame = None;
        self.placeholder = None;
        self.source_size = (0, 0);
        self.working_size = Size::ZERO;
        self.preview_scale = 1.0;
        self.size_error = None;
        self.preview_current = false;
        self.showing_cached_preview = false;
        self.render_failed = false;
        self.update_histogram();
        if self.film_histogram.take().is_some() {
            self.mark(dirty::TONE_GRAPH);
        }
        self.interaction_start = None;
        self.crop_session = None;
        self.original = None;
        self.original_cache = None;
        self.original_loading = false;
        self.holding_original = false;
        self.cancel_brightness_match();
        self.drop_held("open");
        self.brightness_failure = None;
        self.source_generation += 1;
        let generation = self.source_generation;
        let restoring = session.is_some();
        let fit = session.as_ref().map_or(true, |s| s.fit_image);
        if self.prefs.fresh_edits {
            self.custom_film_format = false;
        }
        if let Some(session) = session {
            let mut restored = session.state;
            restored.params = state::restored_params(&self.defaults, &restored.params, &self.nullable_paths);
            // Sessions from before Image Size may carry the old working-resolution factor.
            let factor = json::get(&self.defaults, state::SIZE_FACTOR).clone();
            if !factor.is_null() {
                json::set(&mut restored.params, state::SIZE_FACTOR, factor);
            }
            // The inspector no longer edits Post tone or colour; an older
            // session's Post edits would otherwise stay applied with no
            // control to undo them.
            let post = json::get(&self.defaults, "post").clone();
            if !post.is_null() {
                json::set(&mut restored.params, "post", post);
            }
            // Sessions from before it was a preference carried it per photograph.
            if self.prefs.limit_preview.is_none() && restored.limit_preview {
                self.prefs.limit_preview = Some(true);
                self.prefs.save();
            }
            self.state = self.with_preferences(restored);
        } else {
            // The exposure set above the previous photograph's match carries
            // with Keep the previous edits; the match is solved anew.
            let previous_offset = json::number(self.state.value(state::PREP_EXPOSURE)) - self.state.brightness_match.as_ref().and_then(|m| m.ev).unwrap_or(0.0);
            if self.prefs.fresh_edits {
                // Film/paper and output preferences belong to the workflow;
                // the previous photograph's Prep, Lab and Post edits do not.
                let retained: Vec<(&str, Value)> = [state::SIZE_TARGET, "io.output_color_space", "io.output_cctf_encoding"]
                    .into_iter()
                    .map(|path| (path, self.state.value(path).clone()))
                    .collect();
                self.state.params = self.defaults.clone();
                for (path, value) in retained {
                    if !value.is_null() {
                        json::set(&mut self.state.params, path, value);
                    }
                }
                let positive = self.stocks.iter().find(|s| !s.paper && s.id == self.state.film).is_some_and(|s| s.positive);
                json::set(&mut self.state.params, "io.scan_film", Value::Bool(positive));
                self.state.border = BorderSettings::default();
            }
            json::set(&mut self.state.params, "io.rotate_cw", json::num(0.0));
            // How one file's colours are read never carries over to the next.
            let interpretation = json::get(&self.defaults, "io.input_interpretation").clone();
            if !interpretation.is_null() {
                json::set(&mut self.state.params, "io.input_interpretation", interpretation);
            }
            // Lens corrections are per photograph: on for a new one.
            json::set(&mut self.state.params, state::LENS_CORRECTION, json::get(&self.defaults, state::LENS_CORRECTION).clone());
            self.state.crop = CropSettings::default();
            self.state.brightness_match = self.new_match();
            if !self.prefs.fresh_edits {
                let (low, high) = state::PREP_EXPOSURE_RANGE;
                json::set(&mut self.state.params, state::PREP_EXPOSURE, json::num(previous_offset.clamp(low, high)));
            }
            self.state = self.with_preferences(self.state.clone());
        }
        self.is_raw = is_raw(path);
        self.undo.undo.clear();
        self.undo.redo.clear();
        self.zoom(fit);
        self.prefs.note_recent(path);
        self.prefs.save();
        crate::ui::note_recent_document(path);
        self.status = if self.ready { String::new() } else { "Starting…".into() };
        self.mark(dirty::ALL & !dirty::ERROR);
        // Check the exact developed frame before decoding the raw placeholder.
        let path = path.to_path_buf();
        let state = self.render_state();
        self.input_colour = Value::Null;
        self.lens_supported = false;
        #[cfg(test)]
        let background = ui::testing::background();
        let fallback = move || {
            #[cfg(test)]
            let _background = background;
            if let Some((picture, size)) = crate::thumbnail::placeholder(&path) {
                ui::post(move |app: &mut App| app.model.show_placeholder(picture, size, generation));
            }
        };
        if restoring && let Some(paths) = self.paths.clone() {
            let expected = state.rendered();
            self.preview_cache.load(self.source.clone().unwrap(), state, paths, move |frame| {
                if let Some(frame) = frame {
                    let size = Size::new(frame.photo_width, frame.photo_height);
                    let source_size = (frame.source_width as u32, frame.source_height as u32);
                    let working = Size::new(frame.working_width, frame.working_height);
                    let scale = (frame.width as f64 / frame.working_width).min(1.0);
                    let (raw, lens_supported) = (frame.raw, frame.lens_correction_supported);
                    let picture = Picture::new(frame.pixels, frame.width, frame.height).in_display_space(&frame.display_space);
                    ui::post(move |app: &mut App| {
                        let model = &mut app.model;
                        if generation != model.source_generation || model.image.is_some() || model.closing { return; }
                        if model.render_state().rendered() != expected { std::thread::spawn(fallback); return; }
                        model.image = Some(picture); model.image_size = size; model.is_raw = raw; model.lens_supported = lens_supported;
                        model.source_size = source_size; model.working_size = working; model.preview_scale = scale;
                        model.showing_cached_preview = true; model.placeholder = None; model.hide_activity();
                        model.shown_frame = Some(FrameTag { request: 0, revision: model.revision, generation, kind: FrameKind::Cached });
                        #[cfg(test)]
                        model.frame_log.extend(model.shown_frame);
                        model.update_histogram();
                        model.mark(dirty::CANVAS | dirty::STATUS | dirty::INSPECTOR);
                    });
                } else { fallback(); }
            });
        } else {
            std::thread::spawn(fallback);
        }
        self.changed_with_cache(!restoring);
    }

    fn show_placeholder(&mut self, picture: Picture, size: Size, generation: u64) {
        if generation != self.source_generation || self.image.is_some() {
            return;
        }
        self.placeholder_size = size;
        self.placeholder = Some(picture);
        self.mark(dirty::CANVAS | dirty::INSPECTOR);
    }

    // MARK: Original

    /// Decodes the original ahead of a press, such as when the pointer
    /// reaches the button.
    pub fn prepare_original(&mut self) {
        self.drop_stale_original();
        if self.can_show_original() && self.original_cache.is_none() {
            self.load_original();
        }
    }

    /// Shows the original in place of the developed photograph while held.
    /// The canvas keeps its crop, rotation, and border.
    pub fn hold_original(&mut self, held: bool) {
        if !held || !self.can_show_original() {
            if self.holding_original || self.original.is_some() {
                self.holding_original = false;
                self.original = None;
                self.mark(dirty::CANVAS | dirty::STATUS);
            }
            return;
        }
        if self.holding_original {
            return;
        }
        self.holding_original = true;
        self.mark(dirty::CANVAS | dirty::STATUS);
        self.drop_stale_original();
        if let Some((_, cached)) = &self.original_cache {
            self.original = Some(cached.clone());
            return;
        }
        self.load_original();
    }

    /// What the original's decode depends on: the RAW development (white
    /// balance, lens corrections), how other files' colours are read, the
    /// preview limit and the output space.
    fn original_key(&self) -> Value {
        let params = self.render_state().params;
        let io = &params["io"];
        json!([io["raw_white_balance"], io["lens_correction"], io["input_interpretation"], io["input_color_space"],
            io["input_cctf_decoding"], io["output_color_space"], io["output_cctf_encoding"], self.state.limit_preview])
    }

    fn drop_stale_original(&mut self) {
        if self.original_cache.as_ref().is_some_and(|(key, _)| *key != self.original_key()) && !self.original_loading {
            self.original_cache = None;
        }
    }

    /// Decodes at the developed preview's resolution, so it is as sharp at
    /// actual size.
    fn load_original(&mut self) {
        if self.original_loading {
            return;
        }
        let (Some(source), Some(image)) = (self.source.clone(), &self.image) else { return };
        // Never above the source's own resolution: Image Size can enlarge the preview.
        let source_long = self.source_size.0.max(self.source_size.1);
        let max_pixels = if source_long > 0 { image.width.max(image.height).min(source_long) } else { image.width.max(image.height) };
        let generation = self.source_generation;
        let key = self.original_key();
        self.original_loading = true;
        let name = file_name(&source);
        let finish = move |app: &mut App, picture: Result<Picture>| {
            let model = &mut app.model;
            if generation != model.source_generation {
                return;
            }
            model.original_loading = false;
            match picture {
                Ok(picture) => {
                    model.original_cache = Some((key, picture.clone()));
                    if model.holding_original {
                        model.original = Some(picture);
                        model.mark(dirty::CANVAS | dirty::STATUS);
                    }
                }
                Err(_) => {
                    if model.holding_original {
                        model.holding_original = false;
                        model.status = format!("The original of {name} couldn’t be shown");
                        model.mark(dirty::CANVAS | dirty::STATUS);
                    }
                }
            }
        };
        if self.is_raw {
            // RAW: the helper decodes it with no film, framed like the engine.
            let Some(engine) = self.engine.clone() else { return };
            let buffer = engine.directory().join("original.rgba");
            let command = json!({
                "command": "original", "input": source.clone(), "source_generation": generation,
                "params": self.render_state().params, "limit_preview": self.state.limit_preview,
                "preview_path": buffer,
            });
            engine.request(command, engine::timeout_for("original"), move |result| {
                let picture = result.and_then(|(_, r)| {
                    Picture::read(&buffer, json::number(&r["width"]) as u32, json::number(&r["height"]) as u32)
                        .map(|p| p.in_display_space(json::string(&r["display_color_space"])))
                });
                ui::post(move |app: &mut App| finish(app, picture));
            });
        } else {
            let path = source.clone();
            std::thread::spawn(move || {
                let picture = crate::thumbnail::original(&path, max_pixels);
                ui::post(move |app: &mut App| finish(app, picture));
            });
        }
    }

    // MARK: Previews

    /// Settings › Film, paper and preset previews. Off shows the menus and
    /// the plain preset list, and frees the thumbnails here and in the engine.
    pub fn set_previews(&mut self, on: bool) {
        if self.prefs.previews == on {
            return;
        }
        self.prefs.previews = on;
        self.prefs.save();
        // The grid goes, and its search with it, as the Mac's foldout does.
        if !on {
            self.previews.film_query.clear();
        }
        if !on && self.previews.clear() {
            if let Some(engine) = &self.engine {
                engine.request(json!({"command": "release_thumbnails"}), engine::timeout_for("release_thumbnails"), |_| {});
            }
        }
        self.mark(dirty::STRUCTURE | dirty::PRESETS | dirty::PREVIEWS);
    }

    /// Settings › Keep paper when changing film. The film tiles show what a
    /// click would develop, so they follow it: their keys include the paper.
    pub fn set_keep_paper(&mut self, on: bool) {
        if self.prefs.keep_paper != on {
            self.prefs.keep_paper = on;
            self.prefs.save();
            self.mark(dirty::PREVIEWS);
        }
    }

    /// Opens or closes the Film or Paper foldout.
    pub fn toggle_foldout(&mut self, paper: bool) {
        let open = if paper { &mut self.previews.paper_open } else { &mut self.previews.film_open };
        *open = !*open;
        self.mark(dirty::STRUCTURE | dirty::PREVIEWS);
    }

    /// Whether the Film or Paper grid is open and on screen.
    pub fn grid_shown(&self, paper: bool) -> bool {
        self.prefs.previews
            && self.previews.open(paper)
            && self.inspector_visible
            && self.pane() == Pane::Adjustments
            && !self.cropping()
            && !(paper && json::boolean(self.value("io.scan_film")))
    }

    /// Whether any thumbnails are on screen.
    pub fn previews_on_screen(&self) -> bool {
        self.grid_shown(false) || self.grid_shown(true) || self.preset_thumbnails_shown()
    }

    /// Whether the Presets list, with its thumbnails, is on screen.
    fn preset_thumbnails_shown(&self) -> bool {
        self.prefs.previews && self.inspector_visible && self.pane() == Pane::Presets && !self.cropping()
    }

    /// The main preview has nothing to do: thumbnails may use the engine.
    fn previews_may_render(&self) -> bool {
        self.ready
            && !self.closing
            && !self.rendering
            && !self.pending_render
            && !self.interacting()
            && self.image.is_some()
            && (self.preview_current || self.render_failed)
    }

    /// The thumbnails on screen, in the order they're made, with their keys
    /// and requests: the open grids top to bottom, then the presets once the
    /// engine has checked them all.
    fn wanted_previews(&self) -> Vec<(Target, previews::Key, Value)> {
        let Some(source) = &self.source else { return vec![] };
        let generation = self.source_generation;
        let state = self.render_state();
        let mut wanted = vec![];
        for paper in [false, true] {
            if !self.grid_shown(paper) {
                continue;
            }
            let (films, papers) = (self.films(), self.papers());
            // While the Film grid searches, only its matches, as listed.
            let query = if paper { "" } else { self.previews.film_query.as_str() };
            let searching = !query.trim().is_empty();
            let groups = if paper { state::paper_groups(&papers) } else { state::matching(query, state::film_groups(&films, &papers)) };
            // The favourites first, so their thumbnails come first; a search hides them.
            let favorites = if searching { vec![] } else { self.favorite_stocks(paper) };
            let rest = groups.iter().flat_map(|g| g.stocks.iter().copied()).filter(|s| !favorites.iter().any(|f| f.id == s.id));
            for stock in favorites.iter().copied().chain(rest) {
                let command = previews::stock_command(&state, stock, &self.stocks, self.prefs.keep_paper, source, generation);
                wanted.push((Target::Stock { id: stock.id.clone(), paper }, previews::Key::new(generation, &command), command));
            }
        }
        let checking = self.presets.iter().any(|p| matches!(self.preset_status.get(p), None | Some(PresetStatus::Checking)));
        if self.preset_thumbnails_shown() && !checking {
            let kept: Option<Vec<String>> = self.catalog["preset_kept"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect());
            let valid: Vec<(&PresetFile, &str, &Value)> = self
                .presets
                .iter()
                .filter_map(|p| match self.preset_status.get(p) {
                    Some(PresetStatus::Valid { text, white_balance, .. }) => Some((p, text.as_str(), white_balance)),
                    _ => None,
                })
                .collect();
            let photo = state.value("io.raw_white_balance");
            for (preset, text, _) in previews::render_order(valid, self.is_raw, |p| Some(p.2.clone()), photo) {
                let command = previews::preset_command(text, &state, source, generation);
                wanted.push((Target::Preset(preset.path.clone()), previews::preset_key(&command, kept.as_deref(), generation), command));
            }
        }
        wanted
    }

    /// Dims or blanks out-of-date thumbnails, and asks the engine for the
    /// next one on screen: one request at a time, and only while the main
    /// preview has nothing to do. True when a tile's look changed.
    pub fn pump_previews(&mut self) -> bool {
        if !self.prefs.previews {
            return false;
        }
        let mut changed = false;
        let mut next = None;
        for (target, key, command) in self.wanted_previews() {
            let (needs, look) = self.previews.thumb_mut(&target).mark(&key);
            changed |= look;
            if needs && next.is_none() {
                next = Some((target, key, command));
            }
        }
        if self.previews.busy || !self.previews_may_render() {
            return changed;
        }
        let (Some((target, key, mut command)), Some(engine)) = (next, self.engine.clone()) else { return changed };
        self.previews.busy = true;
        self.previews.requests += 1;
        let buffer = engine.directory().join(format!("thumbnail-{}.rgba", self.previews.requests));
        command["preview_path"] = json!(buffer);
        let (color, rotation) = (self.previews.color.clone(), self.rotation());
        engine.request(command, engine::timeout_for("thumbnail"), move |result| {
            let bitmap = result.and_then(|(_, r)| {
                let rgba = std::fs::read(&buffer)?;
                let wide = crate::picture::is_wide(json::string(&r["display_color_space"]));
                previews::Bitmap::from_rgba(&rgba, json::number(&r["width"]) as u32, json::number(&r["height"]) as u32, wide, color.as_deref(), rotation)
            });
            let _ = std::fs::remove_file(&buffer);
            ui::post(move |app: &mut App| app.model.thumbnail_finished(target, key, bitmap));
        });
        changed
    }

    fn thumbnail_finished(&mut self, target: Target, key: previews::Key, bitmap: Result<previews::Bitmap>) {
        self.previews.busy = false;
        if self.closing || !self.prefs.previews {
            return;
        }
        // The photograph changed meanwhile: drop it, the next pump asks again.
        if key.generation == self.source_generation {
            let bitmap = match bitmap {
                Ok(bitmap) => Some(Arc::new(bitmap)),
                // Leave the placeholder; a later edit retries. Don't spin on a failure.
                Err(error) => {
                    eprintln!("thumbnail {target:?}: {error:#}");
                    None
                }
            };
            self.previews.thumb_mut(&target).show(bitmap, key);
        }
        self.mark(dirty::PREVIEWS);
    }

    /// The canvas moved to a monitor with another profile: thumbnails are
    /// made again through it.
    pub fn set_preview_color(&mut self, color: Option<Arc<crate::icc::Transform>>) {
        let same = match (&self.previews.color, &color) {
            (Some(a), Some(b)) => a.profile == b.profile,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.previews.color = color;
            self.previews.clear();
            self.mark(dirty::PREVIEWS);
        }
    }

    // MARK: Rendering

    /// The colour space the preview develops in: the one Save and Export
    /// last used, so editing shows the export's gamut.
    pub fn preview_space(&self) -> &str {
        COLOR_SPACES.iter().find(|s| **s == self.prefs.color_space).copied().unwrap_or(COLOR_SPACES[0])
    }

    /// The edit as the helper renders it: in the preview space. Sessions
    /// keep the edit itself; the space is the app's.
    pub fn render_state(&self) -> EditState {
        let mut state = self.state.clone();
        json::set(&mut state.params, "io.output_color_space", Value::String(self.preview_space().into()));
        state
    }

    /// Develops the preview in `space` from now on, and remembers it for
    /// the next save.
    pub fn choose_preview_space(&mut self, space: &str) {
        if space == self.preview_space() || !COLOR_SPACES.contains(&space) {
            return;
        }
        self.prefs.color_space = space.into();
        self.prefs.save();
        self.changed();
    }

    /// After a save or export picked another colour space, the preview
    /// develops in it. Requests run in order, so the save goes first.
    pub fn follow_preview_space(&mut self) {
        if self.source.is_some() && !self.rendered_space.is_empty() && self.rendered_space != self.preview_space() {
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.changed_with_cache(true);
    }

    fn changed_with_cache(&mut self, invalidate: bool) {
        // Every edit, undo, redo and new photograph comes here: a held frame
        // is of an older state, and the render scheduled below replaces it.
        self.drop_held("edit");
        self.schedule_tone_curve();
        if invalidate {
            self.preview_cache.invalidate();
            self.showing_cached_preview = false;
        }
        self.revision += 1;
        crate::trace::event("edit", || json!({"revision": self.revision, "generation": self.source_generation, "interacting": self.interacting()}));
        self.preview_current = false;
        if self.render_failed {
            self.render_failed = false;
        }
        self.mark(dirty::CHROME | dirty::STATUS);
        if !self.ready || self.source.is_none() || self.closing {
            return;
        }
        self.schedule_render();
        self.schedule_activity();
    }

    /// Renders run one at a time; edits made meanwhile coalesce into the next.
    fn schedule_render(&mut self) {
        if self.pending_render {
            return;
        }
        self.pending_render = true;
        ui::post(|app: &mut App| {
            app.model.pending_render = false;
            app.model.render();
        });
    }

    fn render(&mut self) {
        if self.rendering || self.closing {
            return;
        }
        let (Some(source), Some(engine)) = (self.source.clone(), self.engine.clone()) else { return };
        let (revision, generation) = (self.revision, self.source_generation);
        // A quick frame's settled render waits for a solve that started
        // after it was scheduled (matching turned on while the quick frame
        // shows, which is no edit at exposure 0): the old exposure never
        // renders at export size. The solve's end settles it
        // (`settle_quick_frame`). The Mac does the same.
        if self.quick_step == Some((revision, generation))
            && !self.preview_current
            && !self.interacting()
            && (self.matching_brightness || self.held.is_some())
        {
            return;
        }
        let buffer = engine.directory().join("preview.rgba");
        let state = self.render_state();
        let mut command = json!({
            "command": "render", "input": source, "source_generation": generation,
            "film": state.film, "paper": state.paper, "params": state.params,
            "limit_preview": self.state.limit_preview, "preview_path": buffer,
        });
        // The target describes the cropped photograph; the helper sizes the frame from the crop.
        if self.size_mode() == state::SizeMode::LongestSide && self.state.crop.cropped() {
            let r = self.state.crop.rect;
            command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": self.state.crop.angle});
        }
        // While a slider moves, render only as many pixels as the canvas
        // shows; the frame on release is the export-size one. A discrete
        // edit shows such a frame first, unless the helper can answer
        // quickly anyway: a small photograph, or post adjustments alone.
        let key = reuse_key(&command);
        let epoch = engine.helper_epoch();
        let reuses_output = self.helper_output.as_ref().is_some_and(|(kept, output)| *kept == epoch && json::same(output, &key));
        let frame = (revision, generation);
        let step = render_step(
            self.interacting(),
            crate::ui::canvas::screen_pixels_per_photo_pixel(),
            self.image_size.width * self.image_size.height,
            self.has_developed_frame(),
            self.quick_step == Some(frame),
            reuses_output,
        );
        let (kind, interactive_scale) = (step.kind(), step.interactive_scale());
        if let Some(scale) = interactive_scale {
            command["interactive_scale"] = json!(scale);
        }
        self.rendering = true;
        self.mark(dirty::CHROME);
        // The helper replaces its kept output with this render (and keeps
        // none if it fails): forget it now, on this thread, before the
        // request can reach the helper.
        self.helper_output = None;
        #[cfg(test)]
        self.render_log.push(RenderRequest { kind, revision, generation, interactive_scale });
        crate::trace::event("render_request", || json!({"kind": kind.name(), "revision": revision, "generation": generation, "interactive_scale": interactive_scale, "reuses_output": reuses_output}));
        let started = Instant::now();
        let paths = self.paths.clone();
        std::thread::spawn(move || {
            let cache_key = if interactive_scale.is_none() { paths.and_then(|paths| rendered_preview::key(&source, &state, &paths)) } else { None };
            let client = engine.clone();
            engine.request(command, engine::timeout_for("render"), move |result| {
                // The worker sends nothing between this request and its
                // reply, so this is the epoch of the helper that kept it.
                let kept = (kind == FrameKind::Settled && result.is_ok()).then(|| (client.helper_epoch(), key));
                crate::trace::event("render_reply", || match &result {
                    Ok((id, r)) => json!({"kind": kind.name(), "revision": revision, "generation": generation, "request": id,
                        "elapsed_ms": r["elapsed_ms"], "cached_pipeline": r["cached_pipeline"], "pipeline_ms": r["pipeline_ms"],
                        "backend": r["backend"], "width": r["width"], "height": r["height"], "round_trip_ms": engine::elapsed_ms(started)}),
                    Err(error) => json!({"kind": kind.name(), "revision": revision, "generation": generation, "error": format!("{error:#}")}),
                });
                // Read the buffer off the UI thread, before the next render can
                // overwrite it.
                let rendered = result.and_then(|(id, result)| {
                    let (w, h) = (json::number(&result["width"]) as u32, json::number(&result["height"]) as u32);
                    ensure!(w > 0 && h > 0 && w <= crate::picture::MAX_PREVIEW && h <= crate::picture::MAX_PREVIEW, "Invalid preview dimensions");
                    let pixels = std::fs::read(&buffer)?;
                    ensure!(pixels.len() == w as usize * h as usize * 4, "Invalid preview buffer");
                    let cached = cache_key.map(|key| (key, Frame {
                        width: w, height: h, photo_width: json::number(&result["photo_width"]), photo_height: json::number(&result["photo_height"]),
                        source_width: json::number(&result["source_width"]), source_height: json::number(&result["source_height"]),
                        working_width: json::number(&result["working_width"]), working_height: json::number(&result["working_height"]),
                        raw: json::boolean(&result["raw"]), display_space: json::string(&result["display_color_space"]).to_string(),
                        lens_correction_supported: json::boolean(&result["lens_correction_supported"]),
                        pixels: pixels.clone(),
                    }));
                    let picture = Picture::new(pixels, w, h).in_display_space(json::string(&result["display_color_space"]));
                    // A long GPU render may have outlived a driver reset.
                    if engine::elapsed_ms(started) > 1500.0
                        && ExportEngine::ran_on_gpu(json::string(&result["backend"]))
                        && crate::platform_events::display_reset_since(std::time::SystemTime::now() - started.elapsed())
                    {
                        return Err(anyhow!("The graphics driver reset while developing the photograph, so this preview was discarded. Edit again to retry."));
                    }
                    Ok(Rendered { id, revision, generation, interactive: interactive_scale.is_some(), result, picture, cached })
                });
                ui::post(move |app: &mut App| {
                    // Renders run one at a time: nothing replaced it since.
                    if kept.is_some() {
                        app.model.helper_output = kept;
                    }
                    app.model.render_finished(rendered, revision, generation, kind);
                });
            });
        });
    }

    /// A render is back. Acceptance: a frame of the current photograph and
    /// edit (or any frame while a slider moves) is taken, and the match reads
    /// its `source_size` and `is_raw` at once. A settled frame whose
    /// brightness match is pending would show the old exposure for as long as
    /// the solve and the next render take: it is held instead, with the
    /// previous frame and the activity badge left up, and the solve starts
    /// from it (the helper holds its decode). `release_held` shows it if the
    /// match leaves the render as it is; an edit drops it. Anything else is
    /// presented now.
    ///
    /// A quick frame (ticket 28) is accepted whenever its photograph is still
    /// open, so a quick frame of an earlier edit shows while edits keep coming
    /// (Ctrl+Z held, fast clicks); the current edit renders next. It is held
    /// like a settled frame, and the match can start from it. It settles only
    /// through `settle_quick_frame`. A failed quick frame is silent: the
    /// settled render that follows reports the error once.
    fn render_finished(&mut self, rendered: Result<Rendered>, revision: u64, generation: u64, kind: FrameKind) {
        self.rendering = false;
        self.mark(dirty::CHROME);
        // The quick frame is done: the next render settles. Once that ran,
        // failed or not, nothing waits to settle (no retries).
        if (revision, generation) == (self.revision, self.source_generation) {
            match kind {
                FrameKind::Quick => self.quick_step = Some((revision, generation)),
                FrameKind::Settled => self.quick_step = None,
                _ => {}
            }
        }
        let showable = match kind {
            FrameKind::Quick => !self.closing && generation == self.source_generation,
            _ => self.should_show(revision, generation),
        };
        let accepted = rendered.is_ok() && showable;
        match rendered {
            Ok(rendered) if accepted => {
                let result = &rendered.result;
                self.source_size = (json::number(&result["source_width"]) as u32, json::number(&result["source_height"]) as u32);
                self.is_raw = json::boolean(&result["raw"]);
                let hold = kind != FrameKind::Drag
                    && self.revision == revision
                    && !self.interacting()
                    && !self.cropping()
                    && self.brightness_match_pending();
                crate::trace::event("render_finished", || json!({
                    "revision": revision, "generation": generation, "kind": kind.name(), "ok": true,
                    "shown": !hold, "held": hold, "current_revision": self.revision,
                }));
                if hold {
                    // Replaces a frame held for the same edit (a slider
                    // pressed and let go without a change renders it again).
                    self.held = Some(Held { revision, generation, kind, rendered });
                    self.mark(dirty::STATUS);
                } else {
                    self.drop_held("superseded");
                    self.present(rendered, kind);
                }
            }
            rendered => {
                crate::trace::event("render_finished", || json!({
                    "revision": revision, "generation": generation, "kind": kind.name(), "ok": rendered.is_ok(),
                    "shown": false, "held": false, "current_revision": self.revision,
                }));
                if let Err(error) = rendered
                    && kind != FrameKind::Quick
                    && revision == self.revision
                    && generation == self.source_generation
                    && !self.closing
                {
                    self.render_failed = true;
                    self.hide_activity();
                    self.report(&error);
                }
            }
        }
        if (self.revision != revision || (kind == FrameKind::Drag && !self.interacting())) && !self.closing {
            self.schedule_render();
        }
        // Starts the solve from a frame just held (a quick one too: the
        // helper then holds the decode); shows a held frame whose match is no
        // longer pending.
        self.release_held();
        self.settle_quick_frame();
        self.settle_outputs();
    }

    /// After a quick frame of the current edit, renders it at its settled
    /// size once nothing holds it: no render running or due, no brightness
    /// solve running and no frame held for one (a match that changes the
    /// exposure is a new edit, with its own quick frame), no slider held.
    /// Called wherever one of these can clear: a render's or a solve's end,
    /// a held frame shown, and the end of an interaction. Does nothing until
    /// all hold. No delay: a 400 ms debounce was tried on the Mac and dropped.
    fn settle_quick_frame(&mut self) {
        if self.quick_step != Some((self.revision, self.source_generation))
            || self.preview_current
            || self.render_failed
            || self.interacting()
            || self.rendering
            || self.pending_render
            || self.matching_brightness
            || self.held.is_some()
            || !self.ready
            || self.source.is_none()
            || self.closing
        {
            return;
        }
        self.schedule_render();
    }

    /// A frame this helper developed of the open photograph is on screen: not
    /// the placeholder (`placeholder`), not the restored launch frame from the
    /// preview cache. A quick frame replaces only such a frame.
    fn has_developed_frame(&self) -> bool {
        self.image.is_some() && !self.showing_cached_preview
    }

    /// The shown frame is the current edit's quick frame: the helper holds
    /// its decode.
    fn shows_quick_frame(&self) -> bool {
        self.quick_frame == Some((self.revision, self.source_generation))
    }

    /// Shows an accepted frame: everything `render_finished` did before the
    /// held frame, in the same order.
    fn present(&mut self, rendered: Rendered, kind: FrameKind) {
        let (revision, generation) = (rendered.revision, rendered.generation);
        self.shown_frame = Some(FrameTag { request: rendered.id, revision, generation, kind });
        #[cfg(test)]
        self.frame_log.extend(self.shown_frame);
        self.quick_frame = (kind == FrameKind::Quick).then_some((revision, generation));
        let result = &rendered.result;
        self.image_size = Size::new(json::number(&result["photo_width"]), json::number(&result["photo_height"]));
        self.working_size = Size::new(json::number(&result["working_width"]), json::number(&result["working_height"]));
        if !rendered.interactive {
            self.preview_scale = if self.working_size.width > 0.0 { (json::number(&result["width"]) / self.working_size.width).min(1.0) } else { 1.0 };
        }
        self.showing_interactive_frame = rendered.interactive;
        self.image = Some(rendered.picture);
        self.placeholder = None;
        self.update_histogram();
        let film = FilmHistogram::parse(&result["film_histogram"]);
        if film != self.film_histogram {
            self.film_histogram = film;
            self.mark(dirty::TONE_GRAPH);
        }
        let (shown, rotation) = (self.shown_image_size(), self.rotation());
        if let Some(session) = &mut self.crop_session {
            session.update(shown, rotation);
        }
        self.lens_supported = json::boolean(&result["lens_correction_supported"]);
        self.rendered_space = json::string(&result["output_color_space"]).to_string();
        if self.input_colour != result["input_colour"] {
            self.input_colour = result["input_colour"].clone();
        }
        if self.revision == revision && !rendered.interactive {
            self.render_id = rendered.id;
            self.preview_current = true;
            self.hide_activity();
            self.render_at_export_size = json::boolean(&result["export_resolution"]);
            self.render_on_gpu = ExportEngine::ran_on_gpu(json::string(&result["backend"]));
            self.showing_cached_preview = false;
            if let Some((key, frame)) = rendered.cached {
                self.preview_cache.save(key, frame);
            }
            self.reconcile_brightness_match();
        }
        let seconds = json::number(&result["elapsed_ms"]) / 1000.0;
        let time = format!("developed in {seconds:.*} s", if seconds < 10.0 { 2 } else { 1 });
        let backend = if ExportEngine::ran_on_gpu(json::string(&result["backend"])) { "" } else { " on the CPU" };
        self.status = if rendered.interactive {
            format!(
                "Preview · {} × {} · {time}{backend}",
                json::number(&result["output_width"]) as u64,
                json::number(&result["output_height"]) as u64
            )
        } else {
            // The photograph as exported, and whether the shown texture
            // has fewer pixels: inspect grain in an export when it does.
            let photograph = self.state.crop.size_of(self.working_size);
            let shown = if self.preview_scale < 1.0 { format!(" · shown at {}", percent(self.preview_scale)) } else { String::new() };
            format!("{} × {}{shown} · {time}{backend}", photograph.width as u64, photograph.height as u64)
        };
        self.mark(dirty::CANVAS | dirty::STATUS | dirty::INSPECTOR | dirty::CROP);
        // Between a quick frame and its settled render the helper has nothing
        // else to do (ticket 28): the presets are checked after the latter.
        if kind != FrameKind::Quick {
            self.check_presets();
        }
    }

    /// Resolves a held frame wherever its match can stop being pending
    /// without an edit: the solve answered (unchanged, or failed), matching
    /// was turned off, or what kept the solve from starting (a slider held,
    /// cropping, a render running) let go. Starts the solve if it can, then
    /// shows the frame once nothing about the match is pending or running.
    /// A changed exposure is an edit, which dropped the frame already.
    fn release_held(&mut self) {
        let Some(held) = &self.held else { return };
        if held.revision != self.revision || held.generation != self.source_generation || self.closing {
            return self.drop_held("stale");
        }
        self.reconcile_brightness_match();
        if self.matching_brightness || self.brightness_match_pending() {
            return;
        }
        let Some(held) = self.held.take() else { return };
        crate::trace::event("held_shown", || json!({"revision": held.revision, "generation": held.generation, "kind": held.kind.name()}));
        self.present(held.rendered, held.kind);
        // Thumbnails wait for the current preview (`App::refresh` pumps and
        // redraws them), and so do Save and Export. A quick frame settles now.
        self.mark(dirty::PREVIEWS);
        self.settle_quick_frame();
        self.settle_outputs();
    }

    /// Forgets a held frame without showing it.
    fn drop_held(&mut self, why: &'static str) {
        if let Some(held) = self.held.take() {
            crate::trace::event("held_dropped", || json!({"revision": held.revision, "generation": held.generation, "why": why}));
        }
    }

    /// The frame waiting for its brightness match, tagged as `shown_frame`
    /// would tag it (tests).
    #[cfg(test)]
    pub fn held_frame(&self) -> Option<FrameTag> {
        self.held.as_ref().map(|h| FrameTag { request: h.rendered.id, revision: h.revision, generation: h.generation, kind: h.kind })
    }

    /// While a slider moves, show each finished frame. Otherwise only the
    /// latest edit, so a discrete change never flashes an older state.
    fn should_show(&self, revision: u64, generation: u64) -> bool {
        !self.closing && generation == self.source_generation && (self.revision == revision || self.interacting())
    }

    /// Counts the preview's tones and waveform off the UI thread. A newer
    /// preview, crop or turn supersedes a count still running.
    fn update_histogram(&mut self) {
        self.histogram_request += 1;
        let Some(image) = self.image.clone() else {
            self.histogram = None;
            self.waveform = None;
            self.mark(dirty::HISTOGRAM);
            return;
        };
        let (request, crop, rotation) = (self.histogram_request, self.state.crop, self.rotation());
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            let scopes = image.scopes(&crop, rotation);
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                if request == model.histogram_request
                    && (model.histogram.as_ref() != Some(&scopes.histogram) || model.waveform.as_ref() != Some(&scopes.waveform))
                {
                    model.histogram = Some(scopes.histogram);
                    model.waveform = Some(scopes.waveform);
                    model.mark(dirty::HISTOGRAM);
                }
            });
        });
    }

    // MARK: Tone graph

    /// The inspector shows the tone graph, or stopped showing it.
    pub fn show_tone_graph(&mut self, shown: bool) {
        if self.tone_graph_shown != shown {
            self.tone_graph_shown = shown;
            self.schedule_tone_curve();
        }
    }

    /// What the tone graph's next curve is for.
    fn tone_request(&self) -> Value {
        let state = self.render_state();
        tone_graph::request(&state.film, &state.paper, &state.params)
    }

    pub fn tone_controls(&self) -> Controls {
        Controls::read(|path| self.value(path))
    }

    /// The shown curve is for the current settings, apart from the controls
    /// its prediction follows; otherwise it stands in, dimmed, until the next.
    pub fn tone_curve_current(&self) -> bool {
        self.tone_curve.as_ref().is_some_and(|curve| curve.matches(&self.tone_request()))
    }

    /// Asks the helper for the current settings' curve, one request at a
    /// time; edits made meanwhile ask again once it answers.
    fn schedule_tone_curve(&mut self) {
        if !self.tone_graph_shown || !self.has_catalog() || self.closing || !json::boolean(&self.catalog["capabilities"]["tone_curve"]) {
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        if self.tone_curve_pending {
            self.tone_curve_stale = true;
            return;
        }
        let request = self.tone_request();
        if self.tone_curve.as_ref().is_some_and(|curve| json::same(&curve.request, &request)) {
            return;
        }
        let controls = self.tone_controls();
        self.tone_curve_pending = true;
        self.tone_curve_stale = false;
        let mut command = request.clone();
        command["command"] = json!("tone_curve");
        command["count"] = json!(tone_graph::POINTS);
        engine.request(command, engine::timeout_for("tone_curve"), move |result| {
            ui::post(move |app: &mut App| app.model.tone_curve_arrived(request, controls, result));
        });
    }

    fn tone_curve_arrived(&mut self, request: Value, controls: Controls, result: Result<(u64, Value)>) {
        self.tone_curve_pending = false;
        // A curve that can't be computed leaves the last one; the render reports errors.
        if let Ok((_, result)) = result
            && let Some(curve) = Curve::parse(&result, request, controls)
            && self.tone_curve.as_ref() != Some(&curve)
        {
            self.tone_curve = Some(curve);
            self.mark(dirty::TONE_GRAPH);
        }
        if self.tone_curve_stale {
            self.schedule_tone_curve();
        }
    }

    /// Only mention an update once it has visibly taken a while.
    fn schedule_activity(&mut self) {
        if self.activity_timer || self.shows_activity {
            return;
        }
        self.activity_timer = true;
        ui::after(Duration::from_millis(400), |app: &mut App| {
            let model = &mut app.model;
            if !model.activity_timer {
                return;
            }
            model.activity_timer = false;
            if !model.preview_current && !model.showing_cached_preview && !model.interacting() && !model.render_failed && model.source.is_some() {
                model.shows_activity = true;
                model.mark(dirty::CANVAS);
            }
        });
    }

    fn hide_activity(&mut self) {
        self.activity_timer = false;
        if self.shows_activity {
            self.shows_activity = false;
            self.mark(dirty::CANVAS);
        }
    }

    /// The badge at the top of the canvas.
    pub fn badge(&self) -> Option<&'static str> {
        if self.original.is_some() {
            return Some("Original");
        }
        if self.holding_original {
            return Some("Loading Original");
        }
        if self.source.is_none() || self.render_failed {
            return None;
        }
        if self.image.is_none() {
            return Some("Developing");
        }
        self.shows_activity.then_some("Updating")
    }

    // MARK: Saving

    /// The default file name: `<photo name>_<film id>.<ext>`.
    pub fn default_name(&self, format: OutputFormat) -> String {
        let stem = self.source.as_ref().and_then(|s| s.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{stem}_{}.{}", self.state.film, format.extension())
    }

    /// The message under the save dialog's options: what the engine does,
    /// then what the save adds.
    pub fn save_lead(engine: Option<ExportEngine>) -> &'static str {
        match engine {
            Some(ExportEngine::Reference) => "The most precise result. Can take about half a minute for large photos.",
            Some(ExportEngine::Fast) => "Looks the same as Reference, in a fraction of the time.",
            None => "Saves the preview as shown.",
        }
    }

    pub fn save_extras(&self) -> String {
        let mut extras = String::new();
        let photograph = self.photograph_size();
        if !photograph.is_zero() {
            extras += &format!(" The photograph is {} × {} px.", photograph.width as u64, photograph.height as u64);
        }
        if self.state.crop.cropped() {
            extras += " Crops the photograph.";
        }
        let photo = self.photo_size();
        if let Some(border) = self.state.border.exported(photo.width as u32, photo.height as u32) {
            extras += &format!(
                " Adds the {} border{}.",
                crate::border::title(border.style).to_lowercase(),
                if border.instagram { ", extended to 3:4 for Instagram" } else { "" }
            );
        }
        extras
    }

    /// Saves the current preview with the crop, rotation, border, and size.
    fn save_command(&self, output: &Path, size: ExportSize, save: SaveSettings) -> Value {
        let mut command = json!({"command": "save", "render_id": self.render_id, "output": output, "rotate_cw": self.rotation(),
            "save_options": save.json()});
        if self.state.crop.cropped() {
            let r = self.state.crop.rect;
            command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": self.state.crop.angle});
        }
        let photo = self.photo_size();
        if let Some(border) = self.state.border.exported(photo.width as u32, photo.height as u32) {
            command["border"] = json!(crate::border::raw_value(border.style));
            command["border_options"] = border.options();
            command["instagram"] = json!(border.instagram);
        }
        if let Some(fit) = size.json() {
            command["fit"] = fit;
        }
        command
    }

    /// Refuses to overwrite the photograph itself.
    fn check_destination(&mut self, destination: &Path) -> bool {
        if self.source.as_ref().is_some_and(|s| crate::platform::same_file(destination, s)) {
            self.error = Some("Choose a different filename to preserve the original photograph.".into());
            self.mark(dirty::ERROR);
            return false;
        }
        true
    }

    // MARK: Camera brightness

    /// Prep › Match camera brightness for this photograph.
    pub fn matches_brightness(&self) -> bool {
        self.state.brightness_match.is_some()
    }

    /// Why matching can't run for this photograph, or None.
    pub fn brightness_match_unavailable(&self) -> Option<String> {
        if let Some((generation, None, message)) = &self.brightness_failure
            && *generation == self.source_generation
        {
            return Some(message.clone());
        }
        if !self.is_raw && json::string(self.value("io.input_interpretation.mode")) != "auto" {
            return Some("Matching reads the file’s own colour tags. Set Input Colour to Automatic to use it.".into());
        }
        None
    }

    fn brightness_match_failed_here(&self) -> bool {
        matches!(&self.brightness_failure, Some((generation, Some(inputs), _))
            if *generation == self.source_generation && json::same(inputs, &state::match_inputs(&self.state)))
    }

    /// On, possible, and not failed for the current settings: the meter then
    /// only sets where the highlight limit starts.
    pub fn brightness_match_active(&self) -> bool {
        self.matches_brightness() && self.brightness_match_unavailable().is_none() && !self.brightness_match_failed_here()
    }

    /// On and not yet solved for the current settings. Save and Export wait.
    pub fn brightness_match_pending(&self) -> bool {
        let Some(m) = &self.state.brightness_match else { return false };
        if !self.brightness_match_active() {
            return false;
        }
        m.ev.is_none() || !m.inputs.as_ref().is_some_and(|inputs| json::same(inputs, &state::match_inputs(&self.state)))
    }

    /// The note under the Exposure slider.
    pub fn brightness_match_note(&self) -> Option<String> {
        if !self.matches_brightness() {
            return None;
        }
        if let Some(unavailable) = self.brightness_match_unavailable() {
            return Some(unavailable);
        }
        if self.brightness_match_failed_here()
            && let Some((_, _, message)) = &self.brightness_failure
        {
            return Some(message.clone());
        }
        if self.brightness_match_pending() || self.matching_brightness {
            return Some("Matching…".into());
        }
        let m = self.state.brightness_match.as_ref()?;
        let ev = m.ev?;
        let value = format!("{}{:.2}", if ev >= 0.0 { "+" } else { "−" }, ev.abs());
        let reference = if self.is_raw { "the camera" } else { "the original" };
        Some(if m.highlight_limited {
            format!("Starts at {value} EV to match {reference}, held back to protect highlights.")
        } else {
            format!("Starts at {value} EV to match {reference}.")
        })
    }

    /// Turns matching on (from the match, Exposure back to 0) or off (the
    /// brightness stays; Exposure then shows all of it). One undo step.
    pub fn set_matches_brightness(&mut self, on: bool) {
        if on == self.matches_brightness() {
            return;
        }
        let mut next = self.state.clone();
        if on {
            next.brightness_match = Some(state::BrightnessMatch::default());
            json::set(&mut next.params, state::PREP_EXPOSURE, json::num(0.0));
        } else {
            next.brightness_match = None;
        }
        self.apply(next, if on { "Match Camera Brightness" } else { "Stop Matching Camera Brightness" });
    }

    fn matched_ev(&self) -> f64 {
        if self.brightness_match_active() { self.state.brightness_match.as_ref().and_then(|m| m.ev).unwrap_or(0.0) } else { 0.0 }
    }

    /// The Exposure slider's range: the engine's total stays within ±3 EV.
    pub fn exposure_offset_range(&self) -> (f64, f64) {
        let matched = self.matched_ev();
        (state::PREP_EXPOSURE_RANGE.0 - matched, state::PREP_EXPOSURE_RANGE.1 - matched)
    }

    /// The Exposure slider: the user's exposure above the match.
    pub fn exposure_offset(&self) -> f64 {
        json::number(self.value(state::PREP_EXPOSURE)) - self.matched_ev()
    }

    pub fn set_exposure_offset(&mut self, value: f64) {
        let (low, high) = self.exposure_offset_range();
        let offset = ((value / state::PREP_EXPOSURE_STEP).round() * state::PREP_EXPOSURE_STEP).clamp(low, high);
        let matched = self.matched_ev();
        let total = if offset.abs() < 1e-9 { matched } else { matched + offset };
        let (min, max) = state::PREP_EXPOSURE_RANGE;
        self.set(state::PREP_EXPOSURE, json::num(total.clamp(min, max)));
    }

    pub fn exposure_offset_modified(&self) -> bool {
        (json::number(self.value(state::PREP_EXPOSURE)) - self.matched_ev()).abs() > 1e-6
    }

    pub fn reset_exposure_offset(&mut self) {
        let matched = self.matched_ev();
        self.set(state::PREP_EXPOSURE, json::num(matched));
    }

    /// Whether Prep shows Auto exposure and Metering: while matched the meter
    /// only sets where the highlight limit starts, so they show when matching
    /// is off or unavailable, or when changed.
    pub fn shows_meter(&self) -> bool {
        !self.brightness_match_active() || self.is_modified("camera.auto_exposure") || self.is_modified("camera.auto_exposure_method")
    }

    fn cancel_brightness_match(&mut self) {
        self.match_request.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.matching_brightness {
            self.matching_brightness = false;
            self.mark(dirty::INSPECTOR);
        }
    }

    /// Solves the match when it's on and stale, once the preview of the
    /// current settings is shown or held, its quick frame included (the
    /// helper then holds their decode), so before their settled render. One
    /// solve at a time; its answer looks again, so edits made meanwhile
    /// coalesce. A result is applied only if the inputs it was
    /// solved for are still current, without an undo step of its own: Undo
    /// restores a state together with the match solved for it, and a
    /// restored state whose match is stale is solved again.
    fn reconcile_brightness_match(&mut self) {
        // Cheap checks first: this runs on every edit, drag frames included.
        if self.matching_brightness
            || self.interacting()
            || !(self.preview_current || self.held.is_some() || self.shows_quick_frame())
            || self.rendering
            || !self.ready
            || self.cropping()
            || self.closing
            || self.state.brightness_match.is_none()
            || !self.brightness_match_pending()
        {
            return;
        }
        let (Some(engine), Some(source)) = (self.engine.clone(), self.source.clone()) else { return };
        let inputs = state::match_inputs(&self.state);
        let id = self.match_request.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let current = self.match_request.clone();
        let (generation, size, raw) = (self.source_generation, self.source_size, self.is_raw);
        // The solve's own exposure: Prep and the print exposure are the user's.
        let mut request = self.render_state();
        json::set(&mut request.params, "prep", json::get(&self.defaults, "prep").clone());
        json::set(&mut request.params, "enlarger.print_exposure", json::get(&self.defaults, "enlarger.print_exposure").clone());
        let reference = engine.directory().join(format!("camera-brightness-{id}.png"));
        crate::trace::event("match_start", || json!({"match": id, "revision": self.revision, "generation": generation}));
        self.matching_brightness = true;
        self.mark(dirty::INSPECTOR);
        std::thread::spawn(move || {
            crate::thumbnail::com_thread();
            let made = if raw { crate::brightness::camera_reference(&source, size) } else { crate::brightness::file_reference(&source, size) };
            let superseded = || current.load(std::sync::atomic::Ordering::Relaxed) != id;
            let written = made.and_then(|png| Ok(std::fs::write(&reference, png)?));
            if let Err(error) = written {
                return ui::post(move |app: &mut App| app.model.brightness_solved(id, generation, inputs, Err(error)));
            }
            if superseded() {
                let _ = std::fs::remove_file(&reference);
                return ui::post(move |app: &mut App| app.model.brightness_solved(id, generation, inputs, Err(anyhow!("superseded"))));
            }
            let command = json!({
                "command": "place_exposure", "input": source, "source_generation": generation,
                "film": request.film, "paper": request.paper, "params": request.params, "reference_path": reference,
            });
            engine.request(command, engine::timeout_for("place_exposure"), move |result| {
                let _ = std::fs::remove_file(&reference);
                ui::post(move |app: &mut App| app.model.brightness_solved(id, generation, inputs, result.map(|(_, r)| r)));
            });
        });
    }

    fn brightness_solved(&mut self, id: u64, generation: u64, inputs: Value, result: Result<Value>) {
        crate::trace::event("match_solved", || match &result {
            Ok(r) => json!({"match": id, "generation": generation, "exposure_ev": r["exposure_ev"], "elapsed_ms": r["elapsed_ms"]}),
            Err(error) => json!({"match": id, "generation": generation, "error": format!("{error:#}")}),
        });
        // Opening another photograph or shutting down superseded this one.
        if id != self.match_request.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        self.matching_brightness = false;
        self.mark(dirty::INSPECTOR | dirty::STRUCTURE);
        if self.closing || generation != self.source_generation {
            return;
        }
        let solved = result.and_then(|r| match r["exposure_ev"].as_f64() {
            Some(ev) if ev.is_finite() && (state::PREP_EXPOSURE_RANGE.0..=state::PREP_EXPOSURE_RANGE.1).contains(&ev) => {
                Ok((ev, json::boolean(&r["highlight_limited"])))
            }
            _ => Err(anyhow!("No reliable brightness match with these settings.")),
        });
        match solved {
            Ok((ev, limited)) => self.apply_brightness_match(ev, limited, inputs),
            Err(error) => {
                if json::same(&state::match_inputs(&self.state), &inputs) {
                    let (message, photograph) = crate::brightness::failure(&error);
                    self.brightness_failure = Some((generation, (!photograph).then_some(inputs), message));
                }
            }
        }
        self.reconcile_brightness_match();
        // An unchanged answer or a failure leaves the held frame current:
        // shown, unless another solve started. A changed exposure was an
        // edit, which dropped it and renders the matched one. A quick frame
        // then settles (at the matched exposure).
        self.release_held();
        self.settle_quick_frame();
        self.settle_outputs();
    }

    /// Nothing for the preview to do: no render running or scheduled, no
    /// solve running or due, and the shown preview is the current edit's (or
    /// it failed). The render benchmark (`crate::trace`) waits for this.
    pub fn render_idle(&self) -> bool {
        !self.rendering
            && !self.pending_render
            && !self.matching_brightness
            && !self.brightness_match_pending()
            && (self.preview_current || self.render_failed)
    }

    /// Moves the total with the match, keeping the user's exposure above it.
    /// A change under 0.05 EV keeps the values and only marks them current.
    fn apply_brightness_match(&mut self, ev: f64, limited: bool, inputs: Value) {
        let Some(mut matched) = self.state.brightness_match.clone() else { return };
        if !json::same(&state::match_inputs(&self.state), &inputs) {
            return;
        }
        let mut next = self.state.clone();
        let offset = json::number(next.value(state::PREP_EXPOSURE)) - matched.ev.unwrap_or(0.0);
        if matched.ev.is_none_or(|old| (ev - old).abs() >= 0.05) {
            matched.ev = Some(ev);
            let (low, high) = state::PREP_EXPOSURE_RANGE;
            let total = if offset.abs() < 1e-9 { ev } else { ev + offset };
            json::set(&mut next.params, state::PREP_EXPOSURE, json::num(total.clamp(low, high)));
        }
        matched.inputs = Some(inputs);
        matched.highlight_limited = limited;
        next.brightness_match = Some(matched);
        self.replace_state(next);
    }

    /// Save and Export wait for a pending match and its preview, and for the
    /// settled frame after a quick one.
    fn outputs_wait(&self) -> bool {
        self.brightness_match_pending()
            || self.matching_brightness
            || (!self.preview_current && !self.render_failed && self.brightness_match_active())
            || self.quick_frame_unsettled()
    }

    /// Something running or due will end the wait: a render, a solve, a held
    /// frame (its solve, or what keeps it from starting), a slider held, or
    /// a quick frame whose settled render `settle_quick_frame` starts.
    fn outputs_progressing(&self) -> bool {
        self.rendering || self.pending_render || self.matching_brightness || self.held.is_some() || self.interacting() || self.quick_frame_unsettled()
    }

    /// The current edit's quick frame is done and its settled frame isn't
    /// (ticket 28).
    fn quick_frame_unsettled(&self) -> bool {
        self.quick_step == Some((self.revision, self.source_generation)) && !self.preview_current && !self.render_failed
    }

    fn after_brightness(&mut self, output: Output, action: impl FnOnce(&mut Model) + 'static) {
        if !self.outputs_wait() {
            return action(self);
        }
        self.waiting_output = Some((Instant::now(), output, Box::new(action)));
        self.status = if self.quick_frame_unsettled() && !self.brightness_match_pending() && !self.matching_brightness {
            "Developing the preview…".into()
        } else {
            "Matching camera brightness…".into()
        };
        self.mark(dirty::STATUS | dirty::CHROME);
        self.check_outputs_later();
        // A failed render won't finish the wait.
        self.settle_outputs();
    }

    /// Looks again every `OUTPUT_PATIENCE` while Save or Export waits.
    fn check_outputs_later(&mut self) {
        ui::after(OUTPUT_PATIENCE, |app: &mut App| {
            let model = &mut app.model;
            model.settle_outputs();
            if model.waiting_output.is_some() {
                model.check_outputs_later();
            }
        });
    }

    /// Runs a waiting Save or Export once nothing is pending, so it writes the
    /// matched frame. It never runs on another frame: while a render or solve
    /// is running or due it keeps waiting (a large RAW on the CPU takes about
    /// a minute); once nothing is, after `OUTPUT_PATIENCE`, or at once when
    /// the render failed, it gives up and says so.
    fn settle_outputs(&mut self) {
        let Some((started, output, _)) = &self.waiting_output else { return };
        let (started, output) = (*started, *output);
        if self.closing {
            self.waiting_output = None;
            return;
        }
        if self.outputs_wait() {
            let stuck = !self.outputs_progressing() && (self.render_failed || started.elapsed() >= OUTPUT_PATIENCE);
            if stuck {
                self.waiting_output = None;
                crate::trace::event("output_abandoned", || json!({"output": output.verb(), "revision": self.revision, "render_failed": self.render_failed}));
                let verb = output.verb();
                self.report(&if self.render_failed {
                    anyhow!("Couldn’t {verb}: the preview didn’t develop, so the camera brightness couldn’t be matched.")
                } else {
                    anyhow!("Couldn’t {verb}: the preview with the camera brightness match isn’t ready. Try again once it shows.")
                });
                self.mark(dirty::CHROME);
            }
            return;
        }
        let Some((_, _, action)) = self.waiting_output.take() else { return };
        action(self);
    }

    /// Solves the enlarger's magenta and yellow shifts so a mid-grey prints
    /// neutral with the current settings (upstream's print balance), and
    /// sets them as one undoable edit. Doesn't depend on the photograph.
    pub fn balance_print(&mut self) {
        if !self.can_balance_print() {
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        let command = json!({"command": "balance_print", "film": self.state.film,
            "paper": self.state.paper, "params": self.state.params});
        self.balancing_print = true;
        self.status = "Balancing print…".into();
        self.mark(dirty::INSPECTOR | dirty::STATUS);
        engine.request(command, engine::timeout_for("balance_print"), move |result| {
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                model.balancing_print = false;
                match result {
                    Ok((_, reply)) => {
                        let (m, y) = (json::number(&reply["m_filter_shift"]), json::number(&reply["y_filter_shift"]));
                        let mut next = model.state.clone();
                        json::set(&mut next.params, "enlarger.m_filter_shift", json!(m));
                        json::set(&mut next.params, "enlarger.y_filter_shift", json!(y));
                        model.apply(next, "Balance Print");
                        model.status = format!("Print balanced: magenta {m:.2}, yellow {y:.2}");
                    }
                    Err(error) => model.report(&error),
                }
                model.mark(dirty::INSPECTOR | dirty::STATUS);
            });
        });
    }

    pub fn export_lut(&mut self, destination: PathBuf) {
        if !self.can_export_lut() || !self.check_destination(&destination) {
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        let staging = engine::staging_path(&destination);
        let command = json!({"command": "export_lut", "film": self.state.film,
            "paper": self.state.paper, "params": self.state.params, "size": 33, "output": staging});
        self.saving = true;
        self.status = "Exporting LUT…".into();
        self.mark(dirty::CHROME | dirty::STATUS);
        engine.request(command, engine::timeout_for("export_lut"), move |result| {
            let result = result.and_then(|_| crate::platform::replace_file(&staging, &destination));
            let _ = std::fs::remove_file(&staging);
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                model.saving = false;
                match result {
                    Ok(()) => model.status = format!("Exported {}", file_name(&destination)),
                    Err(error) => model.report(&error),
                }
                model.mark(dirty::STATUS | dirty::CHROME);
            });
        });
    }

    /// Saves once a pending brightness match and its preview are done; the
    /// preview then develops in the save's colour space.
    pub fn save_preview(&mut self, destination: PathBuf, size: ExportSize, save: SaveSettings) {
        self.after_brightness(Output::Save, move |model| {
            model.save_preview_now(destination, size, save);
            model.follow_preview_space();
        });
    }

    fn save_preview_now(&mut self, destination: PathBuf, size: ExportSize, save: SaveSettings) {
        if !self.can_save() || !self.check_destination(&destination) {
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        self.saving = true;
        self.mark(dirty::CHROME);
        let command = self.save_command(&destination, size, save);
        engine.request(command, engine::timeout_for("save"), move |result| {
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                model.saving = false;
                match result {
                    Ok(_) => model.status = format!("Saved {}", file_name(&destination)),
                    Err(error) => model.report(&error),
                }
                model.mark(dirty::STATUS | dirty::CHROME);
            });
        });
    }

    /// Exports at full resolution. A current preview at export size is what
    /// a Fast export would render, so it is saved instead, through a hidden
    /// sibling renamed into place; if that save fails (an edit arrived
    /// meanwhile), the exporter runs. An export in another colour space than
    /// the preview's always develops again in it.
    pub fn export(&mut self, destination: PathBuf, engine_choice: ExportEngine, size: ExportSize, save: SaveSettings) {
        self.after_brightness(Output::Export, move |model| {
            model.export_now(destination, engine_choice, size, save);
            model.follow_preview_space();
        });
    }

    fn export_now(&mut self, destination: PathBuf, engine_choice: ExportEngine, size: ExportSize, save: SaveSettings) {
        if !self.check_destination(&destination) || self.exporting || self.saving {
            return;
        }
        if engine_choice == ExportEngine::Fast && save.space_name() == self.rendered_space && self.can_save() && self.render_at_export_size {
            if let Some(engine) = self.engine.clone() {
                self.saving = true;
                self.mark(dirty::CHROME);
                let staging = engine::staging_path(&destination);
                let command = self.save_command(&staging, size, save);
                engine.request(command, engine::timeout_for("save"), move |result| {
                    let moved = result.map(|_| {
                        crate::platform::replace_file(&staging, &destination)
                    });
                    let _ = std::fs::remove_file(&staging);
                    ui::post(move |app: &mut App| {
                        let model = &mut app.model;
                        model.saving = false;
                        model.mark(dirty::CHROME | dirty::STATUS);
                        match moved {
                            Ok(Ok(())) => {
                                model.status = if model.render_on_gpu {
                                    format!("Exported {}", file_name(&destination))
                                } else {
                                    format!("Exported {} on the CPU (32-bit): no GPU was available for Fast export", file_name(&destination))
                                };
                                app.export_finished();
                            }
                            Ok(Err(error)) => model.report(&error),
                            Err(_) => model.run_export(destination, engine_choice, size, save),
                        }
                    });
                });
                return;
            }
        }
        self.run_export(destination, engine_choice, size, save);
    }

    fn run_export(&mut self, destination: PathBuf, engine_choice: ExportEngine, size: ExportSize, save: SaveSettings) {
        let (Some(source), Some(engine), Some(paths)) = (self.source.clone(), self.engine.clone(), self.paths.clone()) else { return };
        let executable = match engine_choice.executable(&paths) {
            Ok(executable) => executable,
            Err(error) => return self.report(&error),
        };
        let photo = self.photo_size();
        let mut params = self.state.params.clone();
        json::set(&mut params, "settings.serial_grain", Value::Bool(self.prefs.low_memory_export));
        let request = ExportRequest {
            executable,
            engine: engine_choice,
            data: paths.data.clone(),
            scratch: engine.directory().to_path_buf(),
            source,
            destination: destination.clone(),
            film: self.state.film.clone(),
            paper: self.state.paper.clone(),
            params,
            scan_film: json::boolean(self.state.value("io.scan_film")),
            crop: self.state.crop.cropped().then_some(self.state.crop),
            border: self.state.border.exported(photo.width as u32, photo.height as u32),
            size_arguments: size.arguments(),
            save_arguments: save.arguments(),
        };
        let handle = ExportHandle::default();
        self.export = Some(handle.clone());
        self.exporting = true;
        self.export_started = Instant::now();
        self.mark(dirty::CHROME | dirty::STATUS);
        std::thread::spawn(move || {
            let outcome = engine::run_export(&request, &handle);
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                model.exporting = false;
                model.export = None;
                model.mark(dirty::CHROME | dirty::STATUS);
                match outcome {
                    Ok(outcome) => {
                        model.status = if outcome.fell_back_to_cpu {
                            format!("Exported {} on the CPU (32-bit): no GPU was available for Fast export", file_name(&destination))
                        } else {
                            format!("Exported {}", file_name(&destination))
                        };
                        app.export_finished();
                    }
                    Err(error) if error.is::<engine::Cancelled>() => model.status = "Export cancelled".into(),
                    Err(error) => model.report(&error),
                }
            });
        });
    }

    pub fn cancel_export(&mut self) {
        if let Some(handle) = &self.export {
            handle.cancel();
        }
    }

    pub fn shutdown(&mut self) {
        self.closing = true;
        self.cancel_brightness_match();
        self.drop_held("close");
        self.waiting_output = None;
        self.cancel_export();
        if let Some(engine) = &self.engine {
            engine.stop();
        }
        self.preview_cache.flush();
    }

    pub fn report(&mut self, error: &anyhow::Error) {
        self.error = Some(format!("{error:#}"));
        self.status = "Could not complete the operation".into();
        self.mark(dirty::ERROR | dirty::STATUS);
    }
}

/// The three border width settings, in percent of the long edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderWidth {
    Mat,
    Line,
    Film,
}

impl BorderWidth {
    pub fn range(self) -> (f64, f64, f64) {
        match self {
            BorderWidth::Mat | BorderWidth::Film => (0.0, 15.0, 0.1),
            BorderWidth::Line => (0.05, 2.0, 0.05),
        }
    }
    pub fn get(self, b: &BorderSettings) -> f64 {
        match self {
            BorderWidth::Mat => b.width,
            BorderWidth::Line => b.line,
            BorderWidth::Film => b.film_width,
        }
    }
    pub fn set(self, b: &mut BorderSettings, value: f64) {
        match self {
            BorderWidth::Mat => b.width = value,
            BorderWidth::Line => b.line = value,
            BorderWidth::Film => b.film_width = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prophoto_needs_sixteen_bits() {
        assert_eq!(SaveSettings::effective_space(PROPHOTO, 16), PROPHOTO);
        assert_eq!(SaveSettings::effective_space(PROPHOTO, 8), PROPHOTO_FALLBACK);
        assert_eq!(SaveSettings::effective_space(1, 8), 1);
        assert_eq!(SaveSettings::effective_space(99, 16), PROPHOTO);
    }

    #[test]
    fn save_settings_reach_the_engine() {
        let save = SaveSettings { bit_depth: 16, color_space: 1, jpeg_quality: 80 };
        assert_eq!(save.json(), json!({"jpeg_quality": 80, "bit_depth": 16, "color_space": "Display P3"}));
        assert_eq!(save.arguments(), ["--jpeg-quality", "80", "--bit-depth", "16", "--color-space", "Display P3"]);
        assert!(!save.is_srgb());
        assert_eq!(SaveSettings { color_space: 0, ..save }.summary(true, "sRGB"), None);
        // A save in the preview's space keeps its values; another converts it.
        assert_eq!(save.summary(false, "Display P3").as_deref(), Some("Keeps the preview's Display P3 colours and embeds its profile"));
        assert_eq!(
            save.summary(false, "sRGB").as_deref(),
            Some("Converts the sRGB preview to Display P3, clipping colours outside the smaller space; the preview then shows Display P3")
        );
        assert_eq!(
            SaveSettings { color_space: 0, ..save }.summary(true, "Display P3").as_deref(),
            Some("Develops in sRGB and embeds its profile; the preview then shows sRGB")
        );
    }

    /// A finished render of `width` pixels across a 2400 × 1600 working frame.
    fn rendered(model: &Model, width: u32, interactive: bool) -> Rendered {
        let height = width * 2 / 3;
        let result = json!({
            "width": width, "height": height, "photo_width": 2400, "photo_height": 1600,
            "source_width": 96, "source_height": 64, "working_width": 2400, "working_height": 1600,
            "output_width": width, "output_height": height, "elapsed_ms": 10, "backend": "wgpu",
        });
        let picture = Picture::new(vec![0; width as usize * height as usize * 4], width, height);
        Rendered { id: 1, revision: model.revision, generation: model.source_generation, interactive, result, picture, cached: None }
    }

    /// Settings › Keep paper when changing film, through the film menu.
    #[test]
    fn keep_paper_leaves_the_paper_alone() {
        let stock = |id: &str, paper: bool, target: &str| Stock {
            id: id.into(), name: id.into(), paper, positive: false, bw: false, target: target.into(), times: vec![], grain: Value::Null,
        };
        for keep in [false, true] {
            let mut model = Model::new(Preferences { keep_paper: keep, ..Preferences::default() });
            model.stocks = vec![stock("portra", false, "endura"), stock("vision", false, "print_film"), stock("endura", true, ""), stock("crystal", true, ""), stock("print_film", true, "")];
            model.state = EditState { film: "portra".into(), paper: "crystal".into(), params: json!({"print_render": {"development_time": 2.0}}), ..Default::default() };
            model.choose_film("vision");
            assert_eq!(model.state.film, "vision");
            assert_eq!(model.state.paper, if keep { "crystal" } else { "print_film" });
            assert_eq!(model.state.value("print_render.development_time").is_null(), !keep);
            assert_eq!(model.undo_title(), Some("Change Film"));
            // Loading a catalog still pairs the film with its paper.
            let mut loaded = model.state.clone();
            model.pair_film(&mut loaded);
            assert_eq!(loaded.paper, "print_film");
        }
    }

    /// The Film grid's search: thumbnails only for its matches, in the order
    /// the grid lists them, without the favourites; cleared, every film
    /// again with the favourites first. Searching never edits.
    #[test]
    fn a_film_search_asks_only_for_its_matching_thumbnails() {
        let prefs = Preferences { previews: true, inspector_pane: "Adjustments".into(), favorite_films: vec!["gold".into()], ..Preferences::default() };
        let mut model = catalog_model(prefs);
        model.source = Some(PathBuf::from("photo.png"));
        model.previews.film_open = true;
        model.previews.paper_open = true;
        let wanted = |model: &Model, paper: bool| -> Vec<String> {
            model.wanted_previews().into_iter().filter_map(|(target, ..)| match target { Target::Stock { id, paper: p } if p == paper => Some(id), _ => None }).collect()
        };
        assert_eq!(wanted(&model, false), ["gold", "portra"]);
        let (edit, undo) = (model.state.clone(), model.undo_title().map(str::to_owned));
        model.previews.film_query = "PORTRA".into();
        assert_eq!(wanted(&model, false), ["portra"]);
        // A favourite that matches is asked for once, under its group.
        model.previews.film_query = "gold color negative".into();
        assert_eq!(wanted(&model, false), ["gold"]);
        // Both, in catalog order: the favourite no longer leads.
        model.previews.film_query = "color".into();
        assert_eq!(wanted(&model, false), ["portra", "gold"]);
        model.previews.film_query = "nothing like it".into();
        assert!(wanted(&model, false).is_empty());
        // Papers have no search.
        assert_eq!(wanted(&model, true), ["endura", "crystal"]);
        // Only spaces, or cleared: everything, favourites first.
        for query in ["   ", ""] {
            model.previews.film_query = query.into();
            assert_eq!(wanted(&model, false), ["gold", "portra"]);
        }
        assert_eq!((model.state.clone(), model.undo_title().map(str::to_owned)), (edit, undo));
    }

    /// Lens corrections: on for every new edit and after Reset, though the
    /// engine's default is off; the switch shows only for a supported RAW.
    #[test]
    fn lens_corrections_are_on_for_new_edits_and_after_reset() {
        let mut model = Model::new(Preferences::default());
        model.apply_catalog(json!({"profiles": [], "defaults": {"io": {"lens_correction": {"enabled": false}, "rotate_cw": 0}}}));
        assert_eq!(model.value(state::LENS_CORRECTION), &json!(true));
        model.set(state::LENS_CORRECTION, json!(false));
        assert_eq!(model.value(state::LENS_CORRECTION), &json!(false));
        model.reset();
        assert_eq!(model.value(state::LENS_CORRECTION), &json!(true));
        let (revision, generation) = (model.revision, model.source_generation);
        let mut frame = rendered(&model, 1200, false);
        frame.result["raw"] = json!(true);
        frame.result["lens_correction_supported"] = json!(true);
        model.render_finished(Ok(frame), revision, generation, FrameKind::Settled);
        // Reset started a new match (Settings matches new photographs), so
        // the frame is held: the match reads `is_raw` at once, the switch
        // follows the frame once it shows.
        assert!(model.is_raw && !model.lens_supported && model.held_frame().is_some());
        model.set_matches_brightness(false);
        assert!(model.held_frame().is_none() && model.preview_current);
        assert!(model.is_raw && model.lens_supported);
    }

    /// The Image Size notice describes the settled preview, so reduced drag
    /// frames don't insert or reword it and move the sliders below it.
    #[test]
    fn drag_frames_keep_the_preview_scale() {
        for settled in [2400, 1200] {
            let mut model = Model::new(Preferences::default());
            let (revision, generation) = (model.revision, model.source_generation);
            model.render_finished(Ok(rendered(&model, settled, false)), revision, generation, FrameKind::Settled);
            let scale = model.preview_scale;
            assert_eq!(scale, settled as f64 / 2400.0);

            model.begin_interaction();
            model.render_finished(Ok(rendered(&model, 600, true)), revision, generation, FrameKind::Drag);
            assert!(model.showing_interactive_frame);
            assert_eq!(model.preview_scale, scale, "A drag frame must not change the Image Size notice");

            model.end_interaction("Adjust");
            model.render_finished(Ok(rendered(&model, settled, false)), revision, generation, FrameKind::Settled);
            assert_eq!(model.preview_scale, scale);
        }
    }

    /// The tone graph's histogram is the shown photograph's: each render
    /// replaces it, and opening another photograph clears it at once.
    #[test]
    fn the_film_histogram_follows_the_shown_photograph() {
        let mut model = Model::new(Preferences::default());
        let (revision, generation) = (model.revision, model.source_generation);
        let mut frame = rendered(&model, 1200, false);
        frame.result["film_histogram"] = json!({"stops": [-8, 8], "ev": 0, "counts": [[1, 2], [3, 4], [5, 6]]});
        model.render_finished(Ok(frame), revision, generation, FrameKind::Settled);
        assert_eq!(model.film_histogram.as_ref().map(|h| h.bins(None)), Some(vec![3.0, 4.0]));
        // A render for the previous photograph is dropped with its histogram.
        let file = std::env::temp_dir().join(format!("negs-film-histogram-{}.png", std::process::id()));
        std::fs::write(&file, b"not a photograph").unwrap();
        model.dirty = 0;
        model.open(&file);
        let _ = std::fs::remove_file(&file);
        assert!(model.film_histogram.is_none());
        assert_ne!(model.dirty & dirty::TONE_GRAPH, 0);
        let mut old = rendered(&model, 1200, false);
        old.result["film_histogram"] = json!({"stops": [-8, 8], "ev": 0, "counts": [[9, 9], [9, 9], [9, 9]]});
        model.render_finished(Ok(old), model.revision, generation, FrameKind::Settled);
        assert!(model.film_histogram.is_none());
    }

    /// A catalog with two films and two papers, and defaults for what the
    /// new rules read.
    fn catalog_model(prefs: Preferences) -> Model {
        let mut model = Model::new(prefs);
        let stock = |id: &str, paper: bool, target: &str| json!({"id": id, "name": id, "paper": paper, "positive": false, "bw": false, "target_print": target});
        model.apply_catalog(json!({
            "profiles": [stock("portra", false, "endura"), stock("gold", false, "endura"), stock("endura", true, ""), stock("crystal", true, "")],
            "defaults": {
                "io": {"lens_correction": {"enabled": false}, "rotate_cw": 0, "photo_long_side_px": null, "scan_film": false,
                    "output_cctf_encoding": true, "input_interpretation": {"mode": "auto"}, "raw_white_balance": {"mode": "as_shot"}},
                "prep": {"exposure_ev": 0.0, "shadows": 0.0, "highlights": 0.0},
                "post": {"contrast": 0.0, "saturation": 0.0},
                "camera": {"exposure_compensation_ev": 0.0, "film_format_mm": 35.0, "auto_exposure": true, "auto_exposure_method": "center_weighted"},
                "enlarger": {"print_exposure": 1.0},
            },
        }));
        model.ready = true;
        model
    }

    fn photo(name: &str) -> PathBuf {
        let file = std::env::temp_dir().join(format!("negs-{name}-{}.png", std::process::id()));
        std::fs::write(&file, b"not a photograph").unwrap();
        file
    }

    /// Edits a photograph the way a user would before opening the next.
    fn edit(model: &mut Model) {
        model.choose_film("gold");
        model.set("prep.shadows", json!(30));
        model.set("camera.exposure_compensation_ev", json!(1.5));
        model.set(state::SIZE_TARGET, json!(3000));
        model.set("io.output_cctf_encoding", json!(false));
        model.set("io.rotate_cw", json!(1));
        model.set_border(|b| b.enabled = true, "Change Border");
        model.state.crop.angle = 2.0;
    }

    /// Settings › New photographs: Start fresh (the default) keeps the film,
    /// paper, Image Size and output colour and resets the rest; Keep the
    /// previous edits carries them, apart from crop, rotation and lens
    /// corrections, and the exposure above the match (`LifecycleTests`).
    #[test]
    fn new_photographs_start_fresh_or_keep_the_previous_edits() {
        let file = photo("fresh");
        let mut model = catalog_model(Preferences::default());
        edit(&mut model);
        model.custom_film_format = true;
        model.open(&file);
        let s = &model.state;
        assert_eq!((s.film.as_str(), s.paper.as_str()), ("gold", "endura"));
        assert_eq!(json::number(s.value("prep.shadows")), 0.0);
        assert_eq!(json::number(s.value("camera.exposure_compensation_ev")), 0.0);
        assert_eq!(json::number(s.value(state::SIZE_TARGET)), 3000.0);
        assert_eq!(s.value("io.output_cctf_encoding"), &json!(false));
        assert_eq!((s.rotation(), s.crop, s.border.enabled), (0, CropSettings::default(), false));
        assert_eq!(s.value(state::LENS_CORRECTION), &json!(true));
        assert_eq!(s.brightness_match, Some(state::BrightnessMatch::default()), "Settings › Match new photographs is on");
        assert!(!model.custom_film_format);

        let mut prefs = Preferences::default();
        prefs.fresh_edits = false;
        prefs.match_new_photos = false;
        let mut model = catalog_model(prefs);
        edit(&mut model);
        model.state.brightness_match = Some(state::BrightnessMatch { ev: Some(0.4), ..Default::default() });
        model.set(state::PREP_EXPOSURE, json!(0.9));
        model.open(&file);
        let s = &model.state;
        assert_eq!(json::number(s.value("prep.shadows")), 30.0);
        assert_eq!(json::number(s.value("camera.exposure_compensation_ev")), 1.5);
        assert!(s.border.enabled);
        assert_eq!((s.rotation(), s.crop), (0, CropSettings::default()));
        assert_eq!(s.brightness_match, None);
        assert!((json::number(s.value(state::PREP_EXPOSURE)) - 0.5).abs() < 1e-9, "Only the exposure above the match carries");
        let _ = std::fs::remove_file(file);
    }

    /// Restoring: an older session's Post edits go (no control can undo
    /// them now), and its per-photograph preview limit becomes the setting.
    #[test]
    fn restored_sessions_drop_post_edits_and_lend_the_preview_limit() {
        let file = photo("restore");
        let mut model = catalog_model(Preferences::default());
        let mut state = model.state.clone();
        json::set(&mut state.params, "post.contrast", json!(20));
        json::set(&mut state.params, "prep.shadows", json!(15));
        state.limit_preview = true;
        state.brightness_match = Some(state::BrightnessMatch { ev: Some(0.3), inputs: Some(json!({"a": 1})), highlight_limited: true });
        let session = EditorSession { schema: EditorSession::SCHEMA, source: file.clone(), state, inspector_visible: true, fit_image: true };
        model.open_restoring(&file, Some(session.clone()));
        assert_eq!(json::number(model.state.value("post.contrast")), 0.0);
        assert_eq!(json::number(model.state.value("prep.shadows")), 15.0);
        assert_eq!((model.prefs.limit_preview, model.state.limit_preview), (Some(true), true));
        assert_eq!(model.state.brightness_match, session.state.brightness_match, "The match restores with the session");
        // Once a preference, a session no longer changes it.
        model.prefs.limit_preview = Some(false);
        model.open_restoring(&file, Some(session));
        assert_eq!((model.prefs.limit_preview, model.state.limit_preview), (Some(false), false));
        let _ = std::fs::remove_file(file);
    }

    /// Limit preview to 12 MP is a preference: Undo and Reset leave it.
    #[test]
    fn the_preview_limit_stays_through_undo_and_reset() {
        let mut model = catalog_model(Preferences::default());
        model.set("prep.shadows", json!(10));
        model.prefs.limit_preview = Some(true);
        model.replace_state(model.state.clone());
        assert!(model.state.limit_preview);
        model.perform_undo();
        assert!(model.state.limit_preview && json::number(model.value("prep.shadows")) == 0.0);
        model.reset();
        assert!(model.state.limit_preview);
        assert_eq!(model.undo_title(), Some("Reset Adjustments"));
    }

    /// Lab › Film format: a format writes its long edge; Custom keeps the
    /// value and the slider until a format, Reset or Start fresh.
    #[test]
    fn film_formats_write_the_long_edge() {
        let mut model = catalog_model(Preferences::default());
        assert!(!model.film_format_custom(), "The engine's 35 is 135");
        model.choose_film_format(Some(4));
        assert_eq!(json::number(model.value("camera.film_format_mm")), 70.0);
        model.choose_film_format(None);
        assert!(model.film_format_custom());
        assert_eq!(json::number(model.value("camera.film_format_mm")), 70.0, "Custom keeps the value");
        model.choose_film_format(Some(0));
        assert!(!model.film_format_custom());
        model.set("camera.film_format_mm", json!(45));
        assert!(model.film_format_custom(), "A value no format has reads as Custom");
        model.reset();
        assert!(!model.film_format_custom());
    }

    /// A solve as the helper answers it.
    fn solve(model: &mut Model, inputs: Value, ev: f64) {
        let id = model.match_request.load(std::sync::atomic::Ordering::Relaxed);
        let generation = model.source_generation;
        model.matching_brightness = true;
        model.brightness_solved(id, generation, inputs, Ok(json!({"exposure_ev": ev, "highlight_limited": false})));
    }

    fn matched(model: &Model) -> f64 {
        json::number(model.value(state::PREP_EXPOSURE))
    }

    /// The match moves the total; Exposure is the offset above it and
    /// survives a new match; Undo restores the film with its match; answers
    /// for other inputs or superseded requests are dropped (`ExposurePlacementTests`).
    #[test]
    fn brightness_matches_apply_for_their_own_inputs() {
        let mut model = catalog_model(Preferences::default());
        model.is_raw = true;
        model.set_matches_brightness(true);
        assert!(model.brightness_match_pending());
        assert_eq!(model.brightness_match_note().as_deref(), Some("Matching…"));
        let first = state::match_inputs(&model.state);
        solve(&mut model, first.clone(), 0.8);
        assert_eq!((matched(&model), model.exposure_offset()), (0.8, 0.0));
        assert!(!model.brightness_match_pending() && !model.exposure_offset_modified());
        assert_eq!(model.brightness_match_note().as_deref(), Some("Starts at +0.80 EV to match the camera."));
        let (low, high) = model.exposure_offset_range();
        assert!((low + 3.8).abs() < 1e-9 && (high - 2.2).abs() < 1e-9, "The total stays within ±3 EV");
        // Prep and the print exposure are the user's: they never re-solve.
        model.set("prep.shadows", json!(20));
        model.set("enlarger.print_exposure", json!(1.2));
        assert!(!model.brightness_match_pending());
        model.set_exposure_offset(0.5);
        assert!((matched(&model) - 1.3).abs() < 1e-9 && model.exposure_offset_modified());
        let before = model.state.clone();
        // Another paper prints brighter: the match is stale until solved again.
        model.choose_paper("crystal");
        assert!(model.brightness_match_pending());
        solve(&mut model, first.clone(), 0.1);
        assert!(model.brightness_match_pending(), "An answer for the old paper is dropped");
        let inputs = state::match_inputs(&model.state);
        solve(&mut model, inputs, 1.4);
        assert!((matched(&model) - 1.9).abs() < 1e-9 && (model.exposure_offset() - 0.5).abs() < 1e-9);
        // Under 0.05 EV only marks the match current.
        model.choose_paper("endura");
        let inputs = state::match_inputs(&model.state);
        solve(&mut model, inputs, 1.42);
        assert!((matched(&model) - 1.9).abs() < 1e-9 && !model.brightness_match_pending());
        model.perform_undo();
        model.perform_undo();
        assert_eq!(model.state, before, "Undo restores the paper with its match");
        assert!(!model.brightness_match_pending());
        // A request superseded by opening another photograph is dropped.
        let id = model.match_request.load(std::sync::atomic::Ordering::Relaxed);
        model.cancel_brightness_match();
        model.choose_paper("crystal");
        let (inputs, generation) = (state::match_inputs(&model.state), model.source_generation);
        model.brightness_solved(id, generation, inputs, Ok(json!({"exposure_ev": 2.0})));
        assert!(model.brightness_match_pending());
    }

    #[test]
    fn turning_matching_off_keeps_the_brightness() {
        let mut model = catalog_model(Preferences::default());
        model.is_raw = true;
        model.set_matches_brightness(true);
        let inputs = state::match_inputs(&model.state);
        solve(&mut model, inputs, 0.6);
        model.set_matches_brightness(false);
        assert_eq!(model.undo_title(), Some("Stop Matching Camera Brightness"));
        assert!(!model.brightness_match_active() && model.shows_meter());
        assert_eq!((matched(&model), model.exposure_offset()), (0.6, 0.6), "Off, Exposure shows all of it");
        model.choose_paper("crystal");
        assert!(!model.brightness_match_pending(), "Off, nothing re-solves");
        model.set_matches_brightness(true);
        assert_eq!(matched(&model), 0.0);
        assert!(!model.shows_meter(), "While matched the meter hides unless changed");
        model.set("camera.auto_exposure_method", json!("matrix"));
        assert!(model.shows_meter());
    }

    /// A RAW without a usable preview can't be matched, for the photograph;
    /// a failed solve holds until the next edit; the engine's reasons are
    /// worded for the note.
    #[test]
    fn failed_matches_explain_themselves() {
        let mut model = catalog_model(Preferences::default());
        model.is_raw = true;
        model.set_matches_brightness(true);
        let (id, generation) = (model.match_request.load(std::sync::atomic::Ordering::Relaxed), model.source_generation);
        let inputs = state::match_inputs(&model.state);
        model.brightness_solved(id, generation, inputs.clone(), Err(crate::brightness::Unusable("This RAW has no camera preview to match.".into()).into()));
        assert_eq!(model.brightness_match_unavailable().as_deref(), Some("This RAW has no camera preview to match."));
        assert!(!model.brightness_match_active() && !model.brightness_match_pending() && model.shows_meter());
        model.set("prep.shadows", json!(5));
        assert!(model.brightness_match_unavailable().is_some(), "Another try can't find a preview either");

        let mut model = catalog_model(Preferences::default());
        model.is_raw = true;
        model.set_matches_brightness(true);
        let inputs = state::match_inputs(&model.state);
        let generation = model.source_generation;
        model.brightness_solved(model.match_request.load(std::sync::atomic::Ordering::Relaxed), generation, inputs, Err(anyhow!("Camera brightness requires exposure outside the supported ±3 EV range")));
        assert_eq!(model.brightness_match_note().as_deref(), Some("Matching would take more than ±3 EV."));
        assert!(!model.brightness_match_pending());
        model.choose_paper("crystal");
        assert!(model.brightness_match_pending(), "The next edit tries again");
        // Files: matching reads their own tags.
        model.is_raw = false;
        model.set("io.input_interpretation.mode", json!("assume"));
        assert!(model.brightness_match_unavailable().unwrap().contains("Set Input Colour to Automatic"));
    }

    /// Save and Export wait for a pending match, then run.
    #[test]
    fn outputs_wait_for_a_pending_match() {
        let mut model = catalog_model(Preferences::default());
        model.is_raw = true;
        model.set_matches_brightness(true);
        let ran = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = ran.clone();
        model.after_brightness(Output::Save, move |_| counter.set(counter.get() + 1));
        assert_eq!(ran.get(), 0);
        assert_eq!(model.status, "Matching camera brightness…");
        model.preview_current = true;
        let inputs = state::match_inputs(&model.state);
        solve(&mut model, inputs, 0.2);
        // The solved exposure develops again; the output waits for that preview.
        model.preview_current = false;
        model.settle_outputs();
        assert_eq!(ran.get(), 0);
        model.preview_current = true;
        model.settle_outputs();
        assert_eq!(ran.get(), 1);
        model.after_brightness(Output::Save, move |_| ran.set(ran.get() + 10));
    }

    /// Favourites lead the film tiles' thumbnails, in the order starred.
    #[test]
    fn favourites_come_first() {
        let mut model = catalog_model(Preferences::default());
        model.prefs.toggle_favorite("gold", false);
        model.prefs.toggle_favorite("crystal", true);
        model.prefs.toggle_favorite("missing", false);
        assert_eq!(model.favorite_stocks(false).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["gold"]);
        assert_eq!(model.favorite_stocks(true).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["crystal"]);
    }

    /// `render_step`'s inputs, as the Mac's `QuickFrameStepTests` sets them:
    /// a 24 MP photograph on a canvas at a quarter of its pixels per side.
    #[derive(Clone, Copy)]
    struct Step {
        interacting: bool,
        scale: Option<f64>,
        pixels: f64,
        has_frame: bool,
        done: bool,
        reuses: bool,
    }

    impl Default for Step {
        fn default() -> Self {
            Step { interacting: false, scale: Some(0.25), pixels: 24e6, has_frame: true, done: false, reuses: false }
        }
    }

    impl Step {
        fn step(self) -> RenderStep {
            render_step(self.interacting, self.scale, self.pixels, self.has_frame, self.done, self.reuses)
        }
    }

    /// The Mac's `testDiscreteEditsOfLargePhotosRenderAQuickFrameFirst`.
    #[test]
    fn discrete_edits_of_large_photos_render_a_quick_frame_first() {
        let s = Step::default();
        assert_eq!(s.step(), RenderStep::Quick(0.25));
        assert_eq!(Step { done: true, ..s }.step(), RenderStep::Settled, "its quick frame is done: settle");
        assert_eq!(Step { has_frame: false, ..s }.step(), RenderStep::Settled, "a new photograph has no frame to replace yet");
        assert_eq!(Step { reuses: true, ..s }.step(), RenderStep::Settled, "post-only edits reuse the helper's output");
        assert_eq!(Step { pixels: 4e6, ..s }.step(), RenderStep::Settled, "small photographs render fast enough");
        assert_eq!(Step { scale: Some(0.8), ..s }.step(), RenderStep::Settled, "the canvas shows most pixels anyway");
        assert_eq!(Step { scale: Some(1.0), ..s }.step(), RenderStep::Settled, "zoomed in");
        assert_eq!(Step { scale: None, ..s }.step(), RenderStep::Settled, "no canvas");
        // The limits themselves: 4.5 MP and half the pixels are in.
        assert_eq!(Step { pixels: QUICK_FRAME_MIN_PIXELS, ..s }.step(), RenderStep::Quick(0.25));
        assert_eq!(Step { pixels: QUICK_FRAME_MIN_PIXELS - 1.0, ..s }.step(), RenderStep::Settled);
        let half = QUICK_FRAME_MAX_PIXEL_FRACTION.sqrt();
        assert_eq!(Step { scale: Some(half - 1e-9), ..s }.step(), RenderStep::Quick(half - 1e-9));
        assert_eq!(Step { scale: Some(half + 1e-9), ..s }.step(), RenderStep::Settled);
        assert_eq!(Step { scale: Some(0.0), ..s }.step(), RenderStep::Settled, "no canvas (`set_fixed_scale(Some(0.0))`)");
    }

    /// The Mac's `testDragsAreUnchanged`.
    #[test]
    fn drags_are_unchanged() {
        let s = Step { interacting: true, ..Step::default() };
        assert_eq!(s.step(), RenderStep::Drag(0.25));
        assert_eq!(Step { pixels: 1e6, has_frame: false, done: true, reuses: true, ..s }.step(), RenderStep::Drag(0.25));
        assert_eq!(Step { scale: Some(1.0), ..s }.step(), RenderStep::Settled);
        assert_eq!(Step { scale: None, ..s }.step(), RenderStep::Settled);
        assert_eq!(RenderStep::Drag(0.25).kind(), FrameKind::Drag);
        assert_eq!(RenderStep::Quick(0.25).kind(), FrameKind::Quick);
        assert_eq!((RenderStep::Quick(0.3).interactive_scale(), RenderStep::Settled.interactive_scale()), (Some(0.3), None));
    }

    /// The canvas scale is in physical pixels (`view.zoom`, PerMonitorV2):
    /// the same window in DIPs shows 1.5 times the pixels at 150 %. A
    /// 1300 DIP wide view of the 3072 px test photograph gets a quick frame
    /// at 100 % and 150 %; a 2000 DIP one only at 100 %.
    #[test]
    fn the_quick_frame_limit_counts_physical_pixels() {
        let photo = (3072.0, 2048.0);
        let step = |view_dips: f64, dpi_scale: f64| {
            let screen_scale = view_dips * dpi_scale / photo.0;
            Step { scale: Some(screen_scale), pixels: photo.0 * photo.1, ..Step::default() }.step()
        };
        assert!(matches!(step(1300.0, 1.0), RenderStep::Quick(s) if (s - 0.4232).abs() < 1e-3));
        assert!(matches!(step(1300.0, 1.5), RenderStep::Quick(s) if (s - 0.6348).abs() < 1e-3));
        assert!(matches!(step(2000.0, 1.0), RenderStep::Quick(_)));
        assert_eq!(step(2000.0, 1.5), RenderStep::Settled, "0.98 of the pixels per side");
        // The measurements' window: 1600 × 1000 DIP at 125 % on the 60 MP DNG.
        assert!(matches!(Step { scale: Some(1300.0 * 1.25 / 9504.0), pixels: 9504.0 * 6320.0, ..Step::default() }.step(), RenderStep::Quick(_)));
    }

    /// The Mac's `testReuseKeyIgnoresOnlyPostAndThePreviewPath`, plus: the
    /// comparison is by value (`json::same`), and the crop is in the key.
    #[test]
    fn the_reuse_key_ignores_only_post_and_the_preview_path() {
        let command = json!({
            "command": "render", "film": "a", "preview_path": "C:\\x",
            "params": {"post": {"contrast": 0}, "camera": {"exposure_compensation_ev": 0}},
        });
        let mut post = command.clone();
        post["params"]["post"]["contrast"] = json!(20);
        post["preview_path"] = json!("C:\\y");
        assert!(json::same(&reuse_key(&command), &reuse_key(&post)));
        assert!(reuse_key(&post).get("preview_path").is_none());
        assert_eq!(reuse_key(&post)["params"]["post"], Value::Null);
        let mut camera = command.clone();
        camera["params"]["camera"]["exposure_compensation_ev"] = json!(1);
        assert!(!json::same(&reuse_key(&command), &reuse_key(&camera)));
        let mut scaled = command.clone();
        scaled["interactive_scale"] = json!(0.25);
        assert!(!json::same(&reuse_key(&command), &reuse_key(&scaled)));
        // 0 and 0.0 are the same value.
        let mut float = command.clone();
        float["params"]["camera"]["exposure_compensation_ev"] = json!(0.0);
        assert_ne!(reuse_key(&command), reuse_key(&float));
        assert!(json::same(&reuse_key(&command), &reuse_key(&float)));
        // A crop (Longest side) is part of the request, so of the key: the
        // helper itself keys on the resolved working size instead.
        let mut cropped = command.clone();
        cropped["crop"] = json!({"x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5, "angle": 0});
        assert!(!json::same(&reuse_key(&command), &reuse_key(&cropped)));
    }

    /// A quick frame is a preview of the edit, not the edit's frame: Save,
    /// the export-size flag and the render id stay as they were; the status
    /// says "Preview", with the backend. (The preview cache: lifecycle test.)
    #[test]
    fn a_quick_frame_is_not_the_current_preview() {
        let mut model = Model::new(Preferences::default());
        model.render_id = 7;
        model.render_at_export_size = true;
        let mut quick = rendered(&model, 600, true);
        quick.result["backend"] = json!("cpu");
        model.present(quick, FrameKind::Quick);
        assert!(!model.preview_current && !model.can_save() && model.showing_interactive_frame);
        assert_eq!((model.render_id, model.render_at_export_size), (7, true));
        assert_eq!(model.status, "Preview · 600 × 400 · developed in 0.01 s on the CPU");
        assert!(model.shows_quick_frame());
        assert_eq!(model.shown_frame.map(|f| f.kind), Some(FrameKind::Quick));
        // A settled frame of the same edit replaces it.
        model.present(rendered(&model, 2400, false), FrameKind::Settled);
        assert!(model.preview_current && !model.shows_quick_frame() && model.render_id == 1);
    }
}

#[cfg(test)]
pub mod harness;
#[cfg(test)]
mod lifecycle_tests;

/// Presets through the real helper (`target\release`, built from this
/// checkout); skipped without it. `NEGS_PRESETS_TEST_DIR` (or
/// `%USERPROFILE%\negs-presets-test`) adds every file there.
#[cfg(test)]
mod preset_tests {
    use super::*;
    use std::sync::mpsc;

    struct Live {
        model: Model,
        engine: Arc<EngineClient>,
        folder: PathBuf,
    }

    impl Drop for Live {
        fn drop(&mut self) {
            self.engine.stop();
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    fn ask(engine: &EngineClient, command: Value) -> Result<Value> {
        let (sender, receiver) = mpsc::channel();
        engine.request(command, Duration::from_secs(300), move |result| {
            let _ = sender.send(result);
        });
        receiver.recv()?.map(|(_, value)| value)
    }

    /// A model with the helper's catalog and an empty Presets folder. Its
    /// own requests go nowhere (no window); tests ask the helper themselves.
    fn live(test: &str) -> Option<Live> {
        let Ok(paths) = EnginePaths::locate() else {
            assert!(std::env::var_os("NEGS_REQUIRE_HELPER").is_none(), "NEGS_REQUIRE_HELPER is set and the release helper isn't built");
            eprintln!("skipped: build the release helper first");
            return None;
        };
        let engine = Arc::new(EngineClient::start(paths).ok()?);
        let catalog = ask(&engine, json!({"command": "catalog"})).unwrap();
        assert_eq!(catalog["capabilities"]["presets"], json!(true), "the helper predates presets");
        let mut model = Model::new(Preferences::default());
        model.apply_catalog(catalog);
        model.ready = true;
        model.engine = Some(engine.clone());
        let folder = std::env::temp_dir().join(format!("negs-presets-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        model.preset_library = presets::Library::new(folder.clone());
        Some(Live { model, engine, folder })
    }

    impl Live {
        /// What the panel does on a click, with the helper's answer.
        fn apply(&mut self, name: &str, text: &str) -> Result<()> {
            let snapshot = self.model.state.clone();
            let result = ask(&self.engine, json!({"command": "preset_import", "text": text, "params": snapshot.params}))?;
            self.model.preset_imported(name.into(), text.into(), snapshot, 0, Ok((0, result)));
            match self.model.error.take() {
                Some(error) => Err(anyhow!(error)),
                None => Ok(()),
            }
        }

        fn export(&self) -> String {
            let state = &self.model.state;
            let result = ask(&self.engine, json!({"command": "preset_export", "film": state.film, "paper": state.paper, "params": state.params})).unwrap();
            result["text"].as_str().unwrap().to_owned()
        }
    }

    const DOUBLE_X: &str = r#"{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"},
        "film_chemistry": {"development_time": 8.0}, "couplers": {"langmuir_donor_k_rgb": [Infinity, 1.0, 1.0]},
        "camera": {"exposure_compensation_ev": -0.5}, "load_raw": {"white_balance": "daylight"}}"#;

    #[test]
    fn applying_is_one_undo_step_and_keeps_the_photographs_settings() {
        let Some(mut live) = live("apply") else { return };
        let model = &mut live.model;
        // The photograph's own settings, which a preset never changes.
        model.set(state::SIZE_TARGET, json!(3000));
        model.set("io.rotate_cw", json!(1));
        model.set("post.contrast", json!(20));
        model.set("settings.use_cat16", json!(false));
        model.set(state::LENS_CORRECTION, json!(false));
        model.set("camera.exposure_compensation_ev", json!(1.5));
        model.prefs.limit_preview = Some(true);
        model.replace_state(model.state.clone());
        let before = model.state.clone();
        let steps = model.undo.undo.len();

        live.apply("Double-X", DOUBLE_X).unwrap();
        let model = &mut live.model;
        assert_eq!((model.state.film.as_str(), model.state.paper.as_str()), ("kodak_doublex", "kodak_2302"));
        assert_eq!(model.value("camera.exposure_compensation_ev"), &json!(-0.5));
        // Snapped to a time Double-X lists, so the Development list shows it.
        assert_eq!(json::number(model.value("film_render.development_time")), 9.0);
        assert_eq!(model.value("film_render.dir_couplers.langmuir_donor_k_rgb.0"), &Value::Null);
        assert_eq!(model.value("io.raw_white_balance.mode"), &json!("daylight"));
        for path in [state::SIZE_TARGET, "io.rotate_cw", "post.contrast", "settings.use_cat16", state::LENS_CORRECTION] {
            assert!(json::same(model.value(path), before.value(path)), "{path}");
        }
        assert!(model.state.limit_preview);
        assert_eq!(model.undo.undo.len(), steps + 1);
        assert_eq!(model.undo_title(), Some("Apply Preset “Double-X”"));
        assert_eq!(model.status, "Applied preset “Double-X”");
        model.perform_undo();
        assert_eq!(model.state, before);
        model.perform_redo();
        assert_eq!(model.state.film, "kodak_doublex");
        assert_eq!(model.value("camera.exposure_compensation_ev"), &json!(-0.5));
    }

    #[test]
    fn a_film_change_after_a_preset_brings_back_the_films_own_presets() {
        let Some(mut live) = live("films") else { return };
        let stock_paths: Vec<String> = state::DIR_HALATION_STOCK_PATHS
            .iter()
            .map(|p| p.to_string())
            .chain(state::GRAIN_STOCK_FIELDS.iter().map(|f| format!("film_render.grain.{f}")))
            .collect();
        let explicit = r#"{"simulation": {"film_stock": "kodak_portra_400", "print_paper": "kodak_portra_endura"},
            "couplers": {"gamma_samelayer_rgb": [0.5, 0.5, 0.25]}, "halation": {"strength": [0.1, 0.02, 0.0]}}"#;
        // Film A, a preset with explicit DIR and halation values, film B, film A.
        live.model.choose_film("kodak_portra_400");
        live.apply("Explicit", explicit).unwrap();
        let applied = live.model.state.clone();
        assert!(!applied.value("film_render.dir_couplers.gamma_samelayer_rgb").is_null());
        live.model.choose_film("kodak_gold_200");
        live.model.choose_film("kodak_portra_400");
        for path in &stock_paths {
            assert!(live.model.state.value(path).is_null(), "{path} kept the preset's value");
        }
        // Undo goes back through B to the preset's values.
        live.model.perform_undo();
        live.model.perform_undo();
        assert_eq!(live.model.state, applied);
    }

    #[test]
    fn a_session_saved_after_applying_restores_the_same_params() {
        let Some(mut live) = live("session") else { return };
        live.apply("Double-X", DOUBLE_X).unwrap();
        let photo = live.folder.join("photo.png");
        std::fs::create_dir_all(&live.folder).unwrap();
        std::fs::write(&photo, b"x").unwrap();
        let session = EditorSession { schema: EditorSession::SCHEMA, source: photo, state: live.model.state.clone(), inspector_visible: true, fit_image: true };
        let store = crate::session::SessionStore { file: live.folder.join("session.json") };
        store.save(&session).unwrap();
        let loaded = store.load().unwrap().unwrap();
        let restored = json::restoring(&live.model.defaults, &loaded.state.params, &live.model.nullable_paths);
        assert!(json::same(&restored, &live.model.state.params));
        assert_eq!(loaded.state.film, "kodak_doublex");
    }

    #[test]
    fn new_preset_saves_the_settings_and_escape_removes_it() {
        let Some(mut live) = live("new") else { return };
        live.apply("Double-X", DOUBLE_X).unwrap();
        live.model.preset_library.add(DOUBLE_X.as_bytes(), "Double-X").unwrap();
        let text = live.export();
        let name = presets::name::from_file_name(&live.model.film_name());
        live.model.preset_exported(name.clone(), Ok((0, json!({"text": text}))));
        let saved = live.model.presets.last().cloned().unwrap();
        assert_eq!(saved.name(), name);
        assert_eq!(live.model.renaming_preset.as_ref(), Some(&saved.path));
        // It reads back as the same stocks.
        let check = ask(&live.engine, json!({"command": "preset_import", "text": presets::text(&std::fs::read(&saved.path).unwrap()).unwrap()})).unwrap();
        assert_eq!((check["film"].as_str(), check["paper"].as_str()), (Some("kodak_doublex"), Some("kodak_2302")));
        // Escape while naming it removes it again.
        let model = &mut live.model;
        model.cancel_rename(&saved);
        assert_eq!(model.presets.iter().map(PresetFile::name).collect::<Vec<_>>(), ["Double-X"]);
        assert!(!saved.path.exists());

        // Enter keeps it, under the typed name; Escape on it later only stops renaming.
        model.preset_exported(name, Ok((0, json!({"text": text}))));
        let kept = model.presets.last().cloned().unwrap();
        model.rename_preset(&kept, "Mine");
        let mine = model.presets.iter().find(|p| p.name() == "Mine").cloned().unwrap();
        model.renaming_preset = Some(mine.path.clone());
        model.cancel_rename(&mine);
        assert!(model.renaming_preset.is_none() && mine.path.exists());
        // Dragging the last row to the top is remembered.
        let order = vec![model.presets[1].clone(), model.presets[0].clone()];
        model.move_presets(order);
        assert_eq!(model.presets.iter().map(PresetFile::name).collect::<Vec<_>>(), ["Mine", "Double-X"]);
        let reread = presets::Library::new(live.folder.clone()).list().unwrap();
        assert_eq!(reread.iter().map(PresetFile::name).collect::<Vec<_>>(), ["Mine", "Double-X"]);
    }

    #[test]
    fn files_the_engine_refuses_are_listed_with_the_reason() {
        let Some(mut live) = live("refused") else { return };
        let exotic = live.model.preset_library.add(br#"{"output_gamut_compress": {"algorithm": "jzazbz"}}"#, "Exotic").unwrap();
        live.model.refresh_presets();
        let text = presets::text(&std::fs::read(&exotic.path).unwrap()).unwrap();
        let result = ask(&live.engine, json!({"command": "preset_import", "text": text}));
        live.model.preset_checked(exotic.clone(), text, result.map(|v| (0, v)));
        match live.model.preset_status.get(&exotic) {
            Some(PresetStatus::Invalid(reason)) => assert!(reason.contains("jzazbz"), "{reason}"),
            other => panic!("accepted: {other:?}"),
        }
        // Importing refuses a file the same way and copies nothing.
        let outside = live.folder.with_extension("refused.json");
        std::fs::write(&outside, br#"{"simulation": {"film_stock": "kodak_unknown"}}"#).unwrap();
        let text = presets::text(&std::fs::read(&outside).unwrap()).unwrap();
        let result = ask(&live.engine, json!({"command": "preset_import", "text": text}));
        let job = PresetImport { paths: VecDeque::new(), position: None, added: vec![], failures: vec![] };
        live.model.engine = None;
        live.model.import_checked(job, outside.clone(), text.into_bytes(), result.map(|v| (0, v)));
        let _ = std::fs::remove_file(&outside);
        let (message, reasons) = live.model.notice.take().unwrap_or_default();
        assert_eq!(message, "No presets were imported.");
        assert!(reasons.contains(".refused.json: ") && reasons.contains("kodak_unknown"), "{reasons}");
        assert_eq!(live.model.presets.len(), 1);
    }

    #[test]
    fn every_test_preset_imports_and_applies_and_exports_again() {
        let Some(mut live) = live("all") else { return };
        let mut files: Vec<PathBuf> = vec![];
        let shared = std::env::var_os("NEGS_PRESETS_TEST_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("negs-presets-test")));
        if let Some(Ok(entries)) = shared.map(std::fs::read_dir) {
            files.extend(entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")));
        }
        files.push(live.engine.paths.data.join("presets").join("spektrafilm-gui-default-28bf883.json"));
        eprintln!("{} preset files", files.len());
        // The photograph's own settings survive every one.
        live.model.set("io.rotate_cw", json!(3));
        live.model.set("post.saturation", json!(-40));
        for file in &files {
            let text = presets::text(&std::fs::read(file).unwrap()).unwrap();
            let name = file.file_stem().unwrap().to_string_lossy().into_owned();
            live.apply(&name, &text).unwrap_or_else(|e| panic!("{}: {e:#}", file.display()));
            assert_eq!(json::number(live.model.value("io.rotate_cw")), 3.0, "{name}");
            assert_eq!(json::number(live.model.value("post.saturation")), -40.0, "{name}");
            // Saved again by New Preset, it applies to the same settings.
            let (film, paper, params) = (live.model.state.film.clone(), live.model.state.paper.clone(), live.model.state.params.clone());
            let again = live.export();
            live.model.set("camera.exposure_compensation_ev", json!(2.75));
            live.apply("again", &again).unwrap();
            assert_eq!((&live.model.state.film, &live.model.state.paper), (&film, &paper), "{name}");
            let ev = |p: &Value| json::number(json::get(p, "camera.exposure_compensation_ev"));
            assert_eq!(ev(&live.model.state.params), ev(&params), "{name}");
        }
    }

    /// Matches a photograph the way `reconcile_brightness_match` does, through
    /// the helper: develop it, make the reference, solve, and hand the answer
    /// to the model.
    fn match_through_the_helper(live: &mut Live, path: &Path, raw: bool) -> Value {
        let model = &mut live.model;
        model.is_raw = raw;
        model.set_matches_brightness(false);
        model.set_matches_brightness(true);
        let preview = live.folder.join("preview.rgba");
        std::fs::create_dir_all(&live.folder).unwrap();
        let state = model.render_state();
        let rendered = ask(&live.engine, json!({"command": "render", "input": path, "source_generation": model.source_generation,
            "film": state.film, "paper": state.paper, "params": state.params, "limit_preview": true, "preview_path": preview})).unwrap();
        let size = (json::number(&rendered["source_width"]) as u32, json::number(&rendered["source_height"]) as u32);
        let png = if raw { crate::brightness::camera_reference(path, size) } else { crate::brightness::file_reference(path, size) }.unwrap();
        let reference = image::load_from_memory(&png).unwrap();
        assert!(reference.width().max(reference.height()) <= 512);
        assert!(crate::brightness::same_shape(reference.width() as f64 / reference.height() as f64, size.0 as f64 / size.1 as f64));
        let file = live.folder.join("reference.png");
        std::fs::write(&file, png).unwrap();
        let mut params = state.params.clone();
        json::set(&mut params, "prep", json::get(&model.defaults, "prep").clone());
        let inputs = state::match_inputs(&model.state);
        let reply = ask(&live.engine, json!({"command": "place_exposure", "input": path, "source_generation": model.source_generation,
            "film": state.film, "paper": state.paper, "params": params, "reference_path": file}));
        let id = model.match_request.load(std::sync::atomic::Ordering::Relaxed);
        let generation = model.source_generation;
        model.matching_brightness = true;
        model.brightness_solved(id, generation, inputs, reply);
        assert!(!model.brightness_match_pending(), "{:?}", model.brightness_match_note());
        let ev = model.state.brightness_match.as_ref().and_then(|m| m.ev).unwrap();
        assert!((json::number(model.value(state::PREP_EXPOSURE)) - ev).abs() < 1e-9 && model.exposure_offset().abs() < 1e-9);
        json!(ev)
    }

    /// Optional: a real RAW (`NEGS_TEST_RAW`) is matched to its camera's
    /// preview; docs/card.jpg to itself.
    #[test]
    fn photographs_are_matched_through_the_helper() {
        let Some(mut live) = live("brightness") else { return };
        let card = live.engine.paths.data.parent().unwrap().join("docs").join("card.jpg");
        let ev = match_through_the_helper(&mut live, &card, false);
        eprintln!("card.jpg matched at {ev} EV: {:?}", live.model.brightness_match_note());
        assert!(live.model.brightness_match_note().unwrap().contains("match the original"));
        let Some(raw) = std::env::var_os("NEGS_TEST_RAW").map(PathBuf::from) else {
            eprintln!("skipped the RAW: set NEGS_TEST_RAW");
            return;
        };
        let ev = match_through_the_helper(&mut live, &raw, true);
        eprintln!("{} matched at {ev} EV: {:?}", raw.display(), live.model.brightness_match_note());
        assert!(live.model.brightness_match_note().unwrap().contains("match the camera"));
    }

    /// The tone graph through the real helper: one curve request at a time,
    /// edits meanwhile ask again on its answer, and the prediction from the
    /// last curve matches the helper's own for a new print exposure.
    #[test]
    fn tone_curves_are_asked_one_at_a_time_and_predicted_between() {
        let Some(mut live) = live("tone") else { return };
        let model = &mut live.model;
        assert_eq!(model.catalog["capabilities"]["tone_curve"], json!(true), "the helper predates the tone graph");
        // Hidden: nothing asked.
        model.set(tone_graph::PRINT_EXPOSURE, json!(1.0));
        assert!(!model.tone_curve_pending);
        model.show_tone_graph(true);
        assert!(model.tone_curve_pending);
        let (first, controls) = (model.tone_request(), model.tone_controls());
        // Edits while it's computed don't ask again yet.
        model.set(tone_graph::PRINT_EXPOSURE, json!(2.0));
        assert!(model.tone_curve_stale);

        let mut command = first.clone();
        command["command"] = json!("tone_curve");
        command["count"] = json!(tone_graph::POINTS);
        let reply = ask(&live.engine, command).unwrap();
        let model = &mut live.model;
        model.tone_curve_arrived(first.clone(), controls, Ok((0, reply)));
        let curve = model.tone_curve.clone().unwrap();
        assert!(json::same(&curve.request, &first));
        // It stands in for print exposure 2 while that curve is asked for.
        assert!(model.tone_curve_current());
        assert!(model.tone_curve_pending && !model.tone_curve_stale);
        // A failed answer keeps the last curve.
        model.tone_curve_arrived(model.tone_request(), model.tone_controls(), Err(anyhow!("helper stopped")));
        assert_eq!(model.tone_curve.as_ref(), Some(&curve));
        assert!(!model.tone_curve_pending);

        let display = curve.predicted(model.tone_controls());
        let mut command = model.tone_request();
        command["command"] = json!("tone_curve");
        command["count"] = json!(tone_graph::POINTS);
        let exact = Curve::parse(&ask(&live.engine, command).unwrap(), Value::Null, live.model.tone_controls()).unwrap();
        // Over the stops the graph shows, where the shifted paper exposure is
        // one the last curve reached: beyond it (the deepest shadows, where
        // the paper's exposure flattens) the prediction holds the last value.
        let paper = curve.paper.as_ref().unwrap();
        let shift = 2f64.log10();
        let reached = |c: usize, i: usize| (paper[c][paper[c].len() - 1]..=paper[c][0]).contains(&(paper[c][i] + shift));
        let shown: Vec<(usize, usize)> = (0..3)
            .flat_map(|c| (0..exact.stops.len()).map(move |i| (c, i)))
            .filter(|&(c, i)| (tone_graph::SHOWN.0..=tone_graph::SHOWN.1).contains(&exact.stops[i]) && reached(c, i))
            .collect();
        assert!(shown.len() > 300);
        let worst = shown.iter().map(|&(c, i)| (display.rgb[c][i] - exact.rgb[c][i]).abs()).fold(0.0, f64::max);
        assert!(worst < 0.002, "predicted vs helper: {worst}");
        let (_, middle, _) = exact.predicted(exact.controls).handles(None).unwrap();
        assert!((display.handles(None).unwrap().1 - middle).abs() < 0.05);

        // Another paper: the old curve no longer stands in.
        let model = &mut live.model;
        let other = model.papers().into_iter().map(|s| s.id.clone()).find(|id| *id != model.state.paper).unwrap();
        model.choose_paper(&other);
        assert!(!model.tone_curve_current());
    }
}
