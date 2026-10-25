//! The editor's state and behaviour, a port of `EditorModel.swift`: the edit,
//! undo, rendering one request at a time, opening, cropping, Show Original,
//! saving and exporting. It runs on the UI thread and owns no windows; it
//! marks what changed in `dirty`, and the UI redraws those parts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};

use crate::border::{BorderColor, BorderSettings, BorderStyle};
use crate::crop::{CropSession, CropSettings, Size};
use crate::engine::{self, EngineClient, EnginePaths, ExportEngine, ExportHandle, ExportRequest};
use crate::histogram::Histogram;
use crate::json;
use crate::picture::Picture;
use crate::session::{CatalogCache, EditorSession, Preferences};
use crate::rendered_preview::{self, Frame, RenderedPreviewCache};
use crate::state::{self, EditState, Stock};
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
            Self::Png => "PNG (lossless, 8-bit)",
            Self::Jpeg => "JPEG (smaller, 8-bit)",
            Self::Tiff => "TIFF (16-bit)",
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
    undo: Vec<(EditState, &'static str)>,
    redo: Vec<(EditState, &'static str)>,
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

pub struct Model {
    pub state: EditState,
    pub defaults: Value,
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
    /// The developed preview's frame, uncropped, which the canvas lays out:
    /// the working frame, or less with the 12 MP limit.
    pub image_size: Size,
    /// The decoded source and the uncapped frame an export develops, from the
    /// last render; zero before one finishes.
    pub source_size: (u32, u32),
    pub working_size: Size,
    /// The shown texture's linear fraction of the working frame, below 1 when
    /// the preview is limited or larger than the display proxy.
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
    pub exporting: bool,
    pub export_started: Instant,
    pub error: Option<String>,
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
    original_cache: Option<Picture>,
    original_loading: bool,
    histogram_request: u64,
    /// When the app started and when the first developed photograph went on
    /// screen, for the startup report.
    pub first_render_shown: bool,
}

impl Model {
    pub fn new(mut prefs: Preferences) -> Self {
        // Each launch starts with both editing panels available, even if they
        // were hidden in the previous session. F6/F7 still toggle them in use.
        prefs.filmstrip_visible = true;
        Self {
            state: EditState::default(),
            defaults: json!({}),
            catalog: Value::Null,
            stocks: vec![],
            source: None,
            image: None,
            placeholder: None,
            original: None,
            histogram: None,
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
            exporting: false,
            export_started: Instant::now(),
            error: None,
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
            first_render_shown: false,
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
    pub fn undo_title(&self) -> Option<&'static str> {
        self.undo.undo.last().map(|u| u.1)
    }
    pub fn redo_title(&self) -> Option<&'static str> {
        self.undo.redo.last().map(|u| u.1)
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
    }

    // MARK: Parameters

    pub fn value(&self, path: &str) -> &Value {
        self.state.value(path)
    }
    pub fn default_value(&self, path: &str) -> &Value {
        json::get(&self.defaults, path)
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
        json::set(&mut next.params, path, value);
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
        if self.showing_interactive_frame && self.ready && self.source.is_some() && !self.closing {
            self.schedule_render();
        }
        if !self.preview_current {
            self.schedule_activity();
        }
        self.mark(dirty::CHROME);
    }

    pub fn choose_film(&mut self, stock: &str) {
        let mut next = self.state.clone();
        next.film = stock.into();
        json::set(&mut next.params, "film_render.development_time", Value::Null);
        self.pair_film(&mut next);
        self.apply(next, "Change Film");
    }

    pub fn choose_paper(&mut self, stock: &str) {
        let mut next = self.state.clone();
        next.paper = stock.into();
        json::set(&mut next.params, "print_render.development_time", Value::Null);
        self.apply(next, "Change Paper");
    }

    fn pair_film(&self, state: &mut EditState) {
        let Some(film) = self.stocks.iter().find(|s| !s.paper && s.id == state.film) else { return };
        json::set(&mut state.params, "io.scan_film", Value::Bool(film.positive));
        if self.stocks.iter().any(|s| s.paper && s.id == film.target) && state.paper != film.target {
            state.paper = film.target.clone();
            json::set(&mut state.params, "print_render.development_time", Value::Null);
        }
    }

    /// Original develops the source's own pixels; Longest side resizes the
    /// source so the cropped photograph has that many pixels on its longest side.
    pub fn choose_image_size(&mut self, mode: state::SizeMode) {
        self.size_error = None;
        match mode {
            state::SizeMode::Original => {
                let mut next = self.state.clone();
                json::set(&mut next.params, state::SIZE_FACTOR, json::num(1.0));
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
        json::set(&mut next.params, state::SIZE_FACTOR, json::num(1.0));
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

    pub fn limit_preview(&mut self, enabled: bool) {
        let mut next = self.state.clone();
        next.limit_preview = enabled;
        self.apply(next, "Change Preview Resolution");
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
    /// paper, and rotation. An older edit's working-resolution factor resets.
    pub fn reset(&mut self) {
        if self.cropping() {
            return;
        }
        let mut next = self.state.clone();
        next.params = self.defaults.clone();
        next.limit_preview = false;
        json::set(&mut next.params, "io.rotate_cw", self.state.value("io.rotate_cw").clone());
        json::set(&mut next.params, state::SIZE_TARGET, self.state.value(state::SIZE_TARGET).clone());
        let positive = self.stocks.iter().find(|s| !s.paper && s.id == next.film).is_some_and(|s| s.positive);
        json::set(&mut next.params, "io.scan_film", Value::Bool(positive));
        self.apply(next, "Reset Adjustments");
    }

    fn apply(&mut self, next: EditState, action: &'static str) {
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
        if crop_changed {
            self.update_histogram();
        }
        self.mark(dirty::INSPECTOR | dirty::CANVAS | dirty::CHROME | if structure { dirty::STRUCTURE } else { 0 });
        if render {
            self.changed();
        }
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

    fn register_undo(&mut self, previous: EditState, action: &'static str) {
        self.undo.undo.push((previous, action));
        self.undo.redo.clear();
        self.mark(dirty::CHROME);
    }

    pub fn perform_undo(&mut self) {
        if self.cropping() {
            return;
        }
        let Some((previous, action)) = self.undo.undo.pop() else { return };
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
        Some(EditorSession { schema: 1, source, state: saved, inspector_visible: self.inspector_visible, fit_image: self.fit_image })
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
        self.placeholder = None;
        self.source_size = (0, 0);
        self.working_size = Size::ZERO;
        self.preview_scale = 1.0;
        self.size_error = None;
        self.preview_current = false;
        self.showing_cached_preview = false;
        self.render_failed = false;
        self.update_histogram();
        self.interaction_start = None;
        self.crop_session = None;
        self.original = None;
        self.original_cache = None;
        self.original_loading = false;
        self.holding_original = false;
        self.source_generation += 1;
        let generation = self.source_generation;
        let restoring = session.is_some();
        let fit = session.as_ref().map_or(true, |s| s.fit_image);
        if let Some(session) = session {
            let mut restored = session.state;
            restored.params = json::restoring(&self.defaults, &restored.params);
            self.state = restored;
        } else {
            json::set(&mut self.state.params, "io.rotate_cw", json::num(0.0));
            self.state.crop = CropSettings::default();
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
        let state = self.state.clone();
        let fallback = move || {
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
                    let raw = frame.raw;
                    let picture = Picture::new(frame.pixels, frame.width, frame.height);
                    ui::post(move |app: &mut App| {
                        let model = &mut app.model;
                        if generation != model.source_generation || model.image.is_some() || model.closing { return; }
                        if model.state.rendered() != expected { std::thread::spawn(fallback); return; }
                        model.image = Some(picture); model.image_size = size; model.is_raw = raw;
                        model.source_size = source_size; model.working_size = working; model.preview_scale = scale;
                        model.showing_cached_preview = true; model.placeholder = None; model.hide_activity();
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
        if let Some(cached) = &self.original_cache {
            self.original = Some(cached.clone());
            return;
        }
        self.load_original();
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
                    model.original_cache = Some(picture.clone());
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
                "params": self.state.params, "limit_preview": self.state.limit_preview,
                "preview_path": buffer,
            });
            engine.request(command, engine::timeout_for("original"), move |result| {
                let picture = result.and_then(|(_, r)| {
                    Picture::read(&buffer, json::number(&r["width"]) as u32, json::number(&r["height"]) as u32)
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

    // MARK: Rendering

    fn changed(&mut self) {
        self.changed_with_cache(true);
    }

    fn changed_with_cache(&mut self, invalidate: bool) {
        if invalidate {
            self.preview_cache.invalidate();
            self.showing_cached_preview = false;
        }
        self.revision += 1;
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
        // While a slider moves, render only as many pixels as the canvas
        // shows; the frame on release is the export-size one.
        let interactive_scale = if self.interacting() {
            crate::ui::canvas::screen_pixels_per_photo_pixel().filter(|&s| s < 1.0)
        } else {
            None
        };
        self.rendering = true;
        self.mark(dirty::CHROME);
        let buffer = engine.directory().join("preview.rgba");
        let mut command = json!({
            "command": "render", "input": source, "source_generation": generation,
            "film": self.state.film, "paper": self.state.paper, "params": self.state.params,
            "limit_preview": self.state.limit_preview, "preview_path": buffer,
        });
        if let Some(scale) = interactive_scale {
            command["interactive_scale"] = json!(scale);
        }
        // The target describes the cropped photograph; the helper sizes the frame from the crop.
        if self.size_mode() == state::SizeMode::LongestSide && self.state.crop.cropped() {
            let r = self.state.crop.rect;
            command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": self.state.crop.angle});
        }
        let started = Instant::now();
        let state = self.state.clone();
        let paths = self.paths.clone();
        std::thread::spawn(move || {
            let cache_key = if interactive_scale.is_none() { paths.and_then(|paths| rendered_preview::key(&source, &state, &paths)) } else { None };
            engine.request(command, engine::timeout_for("render"), move |result| {
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
                        raw: json::boolean(&result["raw"]), pixels: pixels.clone(),
                    }));
                    let picture = Picture::new(pixels, w, h);
                    // A long GPU render may have outlived a driver reset.
                    if engine::elapsed_ms(started) > 1500.0
                        && ExportEngine::ran_on_gpu(json::string(&result["backend"]))
                        && crate::platform_events::display_reset_since(std::time::SystemTime::now() - started.elapsed())
                    {
                        return Err(anyhow!("The graphics driver reset while developing the photograph, so this preview was discarded. Edit again to retry."));
                    }
                    Ok(Rendered { id, revision, generation, interactive: interactive_scale.is_some(), result, picture, cached })
                });
                ui::post(move |app: &mut App| app.model.render_finished(rendered, revision, generation, interactive_scale.is_some()));
            });
        });
    }

    fn render_finished(&mut self, rendered: Result<Rendered>, revision: u64, generation: u64, interactive: bool) {
        self.rendering = false;
        self.mark(dirty::CHROME);
        match rendered {
            Ok(rendered) if self.should_show(revision, generation) => {
                let result = &rendered.result;
                self.image_size = Size::new(json::number(&result["photo_width"]), json::number(&result["photo_height"]));
                self.source_size = (json::number(&result["source_width"]) as u32, json::number(&result["source_height"]) as u32);
                self.working_size = Size::new(json::number(&result["working_width"]), json::number(&result["working_height"]));
                self.preview_scale = if self.working_size.width > 0.0 { (json::number(&result["width"]) / self.working_size.width).min(1.0) } else { 1.0 };
                self.showing_interactive_frame = rendered.interactive;
                self.image = Some(rendered.picture);
                self.placeholder = None;
                self.update_histogram();
                let (shown, rotation) = (self.shown_image_size(), self.rotation());
                if let Some(session) = &mut self.crop_session {
                    session.update(shown, rotation);
                }
                self.is_raw = json::boolean(&result["raw"]);
                if self.revision == revision && !rendered.interactive {
                    self.render_id = rendered.id;
                    self.preview_current = true;
                    self.hide_activity();
                    self.render_at_export_size = json::boolean(&result["export_resolution"]);
                    self.render_on_gpu = ExportEngine::ran_on_gpu(json::string(&result["backend"]));
                    self.showing_cached_preview = false;
                    if let Some((key, frame)) = rendered.cached { self.preview_cache.save(key, frame); }
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
            }
            Ok(_) => {}
            Err(error) => {
                if revision == self.revision && generation == self.source_generation && !self.closing {
                    self.render_failed = true;
                    self.hide_activity();
                    self.report(&error);
                }
            }
        }
        if (self.revision != revision || (interactive && !self.interacting())) && !self.closing {
            self.schedule_render();
        }
    }

    /// While a slider moves, show each finished frame. Otherwise only the
    /// latest edit, so a discrete change never flashes an older state.
    fn should_show(&self, revision: u64, generation: u64) -> bool {
        !self.closing && generation == self.source_generation && (self.revision == revision || self.interacting())
    }

    /// Counts the preview's tones off the UI thread. A newer preview or
    /// crop supersedes a count still running.
    fn update_histogram(&mut self) {
        self.histogram_request += 1;
        let Some(image) = self.image.clone() else {
            self.histogram = None;
            self.mark(dirty::HISTOGRAM);
            return;
        };
        let (request, crop) = (self.histogram_request, self.state.crop);
        std::thread::spawn(move || {
            let counted = image.histogram(&crop);
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                if request == model.histogram_request && model.histogram.as_ref() != Some(&counted) {
                    model.histogram = Some(counted);
                    model.mark(dirty::HISTOGRAM);
                }
            });
        });
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
    fn save_command(&self, output: &Path, size: ExportSize) -> Value {
        let mut command = json!({"command": "save", "render_id": self.render_id, "output": output, "rotate_cw": self.rotation()});
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

    pub fn save_preview(&mut self, destination: PathBuf, size: ExportSize) {
        if !self.can_save() || !self.check_destination(&destination) {
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        self.saving = true;
        self.mark(dirty::CHROME);
        let command = self.save_command(&destination, size);
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
    /// meanwhile), the exporter runs.
    pub fn export(&mut self, destination: PathBuf, engine_choice: ExportEngine, size: ExportSize) {
        if !self.check_destination(&destination) || self.exporting || self.saving {
            return;
        }
        if engine_choice == ExportEngine::Fast && self.can_save() && self.render_at_export_size {
            if let Some(engine) = self.engine.clone() {
                self.saving = true;
                self.mark(dirty::CHROME);
                let staging = engine::staging_path(&destination);
                let command = self.save_command(&staging, size);
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
                            Err(_) => model.run_export(destination, engine_choice, size),
                        }
                    });
                });
                return;
            }
        }
        self.run_export(destination, engine_choice, size);
    }

    fn run_export(&mut self, destination: PathBuf, engine_choice: ExportEngine, size: ExportSize) {
        let (Some(source), Some(engine), Some(paths)) = (self.source.clone(), self.engine.clone(), self.paths.clone()) else { return };
        let executable = match engine_choice.executable(&paths) {
            Ok(executable) => executable,
            Err(error) => return self.report(&error),
        };
        let photo = self.photo_size();
        let request = ExportRequest {
            executable,
            engine: engine_choice,
            data: paths.data.clone(),
            scratch: engine.directory().to_path_buf(),
            source,
            destination: destination.clone(),
            film: self.state.film.clone(),
            paper: self.state.paper.clone(),
            params: self.state.params.clone(),
            scan_film: json::boolean(self.state.value("io.scan_film")),
            crop: self.state.crop.cropped().then_some(self.state.crop),
            border: self.state.border.exported(photo.width as u32, photo.height as u32),
            size_arguments: size.arguments(),
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
