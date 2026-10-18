// Minimal interactive preview for spektrafilm.
//
// Loads an image, exposes the most impactful runtime parameters as
// egui controls, and re-renders the full GPU-resident pipeline whenever
// a control changes. The output texture is uploaded to egui once per
// render; the panel scales it to fit.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use eframe::egui;
use rayon::prelude::*;
use spektrafilm_core::params::RuntimeParams;
use spektrafilm_core::pipeline::Pipeline;
use spektrafilm_core::profile;
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32, to_f32};

const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(120);
const IN_FLIGHT_REPAINT: Duration = Duration::from_millis(16);
const PREVIEW_TEXTURE_MAX_DIM: usize = 8192;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    if let Err(err) = embedded_bundle::setup() {
        eprintln!("[spektrafilm] embedded bundle setup failed: {err:#}");
    }
    let backend = spektrafilm_gpu::select_backend();
    let backend: Arc<dyn ComputeBackend> = Arc::from(backend);
    let initial_image = std::env::args().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]),
        renderer: gui_renderer(),
        // egui's default device descriptor hardcodes
        // `max_texture_dimension_2d: 8192`, so uploading the preview of an
        // image wider or taller than 8192 px (a single texture) panics.
        // Raise the cap to the adapter's real limit (16384 on Apple
        // Silicon Metal, higher on discrete GPUs), floored at egui's 8192
        // so we never regress. This is eframe's OWN render device — the
        // compute pipeline's wgpu device (WgpuBackend::new) is separate
        // and already lifts its storage-buffer limits.
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetupCreateNew {
                device_descriptor: Arc::new(|adapter: &eframe::wgpu::Adapter| {
                    let base_limits = if adapter.get_info().backend == eframe::wgpu::Backend::Gl {
                        eframe::wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        eframe::wgpu::Limits::default()
                    };
                    eframe::wgpu::DeviceDescriptor {
                        label: Some("spektrafilm egui wgpu device"),
                        required_features: eframe::wgpu::Features::default(),
                        required_limits: eframe::wgpu::Limits {
                            max_texture_dimension_2d: adapter
                                .limits()
                                .max_texture_dimension_2d
                                .max(8192),
                            ..base_limits
                        },
                        memory_hints: eframe::wgpu::MemoryHints::default(),
                    }
                }),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "spektrafilm",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(backend, initial_image)))),
    )
}

#[cfg(embed_bundle)]
mod embedded_bundle {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

    include!(env!("SPEKTRAFILM_EMBED_MANIFEST"));

    pub fn setup() -> Result<()> {
        let root = cache_root().join(BUNDLE_ID);
        let data_dir = root.join("data");
        let f64_cli = root.join(f64_cli_name());
        let marker = root.join(".ready");

        if !marker.is_file() || !data_dir.join("profiles").is_dir() || !f64_cli.is_file() {
            std::fs::create_dir_all(&data_dir)
                .with_context(|| format!("creating {}", data_dir.display()))?;
            for (rel, bytes) in DATA_FILES {
                write_file(&data_dir.join(rel), bytes)?;
            }
            write_file(&f64_cli, F64_EXE)?;
            write_file(&marker, BUNDLE_ID.as_bytes())?;
        }

        // This runs before any worker threads are spawned.
        unsafe {
            std::env::set_var("SPEKTRAFILM_DATA_DIR", &data_dir);
            std::env::set_var("SPEKTRAFILM_F64_CLI", &f64_cli);
        }
        Ok(())
    }

    fn cache_root() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("spektrafilm")
            .join("embedded")
    }

    fn f64_cli_name() -> &'static str {
        if cfg!(windows) {
            "spektrafilm-f64.exe"
        } else {
            "spektrafilm-f64"
        }
    }

    fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file =
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(not(embed_bundle))]
mod embedded_bundle {
    use anyhow::Result;

    pub fn setup() -> Result<()> {
        Ok(())
    }
}

fn gui_renderer() -> eframe::Renderer {
    match std::env::var("SPEKTRAFILM_GUI_RENDERER") {
        Ok(v) if v.eq_ignore_ascii_case("wgpu") => eframe::Renderer::Wgpu,
        Ok(v) if v.eq_ignore_ascii_case("glow") => eframe::Renderer::Glow,
        _ => {
            if cfg!(target_os = "macos") {
                // Metal-backed CAMetalLayer lets us tag the colorspace as sRGB.
                eframe::Renderer::Wgpu
            } else {
                // Keep the UI compositor away from wgpu/D3D12 by default on
                // Windows/Linux; CUDA/WGSL compute backends are selected separately.
                eframe::Renderer::Glow
            }
        }
    }
}

/// One entry in the film / paper combo box. `stock` is the filename
/// stem (the unique key the profile loader expects); `display` is the
/// human-readable label from the profile's `info.name`, falling back
/// to the stock id when missing.
#[derive(Debug, Clone)]
struct ProfileEntry {
    stock: String,
    display: String,
    is_bw: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
}

impl OutputFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG (lossless, 8-bit)",
            Self::Jpeg => "JPEG (smaller, 8-bit)",
            Self::Tiff => "TIFF (16-bit)",
        }
    }
}

struct App {
    backend: Arc<dyn ComputeBackend>,
    data_dir: PathBuf,
    /// Profiles where `info.support == "film"`.
    films: Vec<ProfileEntry>,
    /// Profiles where `info.support == "paper"` (or other print-stage
    /// supports).
    papers: Vec<ProfileEntry>,
    film_name: String,
    print_name: String,
    /// Development-time family (minutes) of the selected film / paper —
    /// non-trivial only for B&W stocks. Drives the dev-time pickers.
    film_dev_times: Vec<f64>,
    print_dev_times: Vec<f64>,
    params: RuntimeParams,
    /// GUI-only resolution cap; never serialized into export parameters.
    limit_preview_12mp: bool,
    /// Suggested format for Save and Export; a typed extension can override it.
    output_format: OutputFormat,
    image_path: Option<PathBuf>,
    /// Destination chosen in the most recent export dialog, independent
    /// of the folder used to open images.
    last_export_dir: Option<PathBuf>,
    image: Option<Arc<ImageBuf>>,
    /// WB of the currently developed input, which can lag behind a UI edit.
    loaded_raw_wb: spektrafilm_io::RawWhiteBalance,
    /// Invalidates in-flight results when opening an image or editing RAW WB.
    input_revision: u64,
    /// Last rendered pipeline output (post sRGB encode + clip). Retained
    /// so the Save button can write it without re-running the pipeline.
    output_image: Option<ImageBuf>,
    output_tex: Option<egui::TextureHandle>,
    pipeline_cache_key: Option<String>,
    pipeline_cache: Option<Pipeline>,
    last_render_ms: f32,
    last_pipeline_build_ms: f32,
    last_input_clone_ms: f32,
    last_scale_ms: f32,
    last_preview_ms: f32,
    last_worker_total_ms: f32,
    status: String,
    /// Set when any control change should trigger a re-render. The
    /// next `update()` tick dispatches the render onto a worker
    /// thread so the UI stays responsive while the pipeline (up to
    /// ~500 ms on 6 MP) runs in the background.
    dirty: bool,
    /// Marked when the user changes a param while a previous render
    /// is still in flight. After that render finishes we re-arm
    /// `dirty` so the latest state gets a fresh pass instead of
    /// dropping the user's mid-render edits on the floor.
    pending_dirty: bool,
    dirty_since: Option<Instant>,
    /// Current preview zoom multiplier on top of the fit-to-panel
    /// scale (1.0 = fit). Mouse wheel adjusts it around the cursor,
    /// drag pans, double-click resets.
    zoom: f32,
    /// Pan offset (screen pixels) added to the preview centre.
    pan: egui::Vec2,
    /// macOS-only: tag the wgpu CAMetalLayer's `colorspace` as sRGB on
    /// the first `update()` tick (it's not yet wired up at
    /// `App::new` time). Set to `true` once the call succeeds so we
    /// don't retry every frame.
    #[cfg(target_os = "macos")]
    metal_colorspace_tagged: bool,
    /// In-flight pipeline render. `Some` while the worker thread is
    /// running; main thread polls the receiver each frame and
    /// uploads the resulting texture once it lands. Decoupling the
    /// render from the UI thread is what keeps sliders responsive —
    /// the 250–500 ms pipeline used to block input handling.
    render_job: Option<RenderJob>,
    /// In-flight f64 export job. `Some` while the subprocess is
    /// running; the `update()` loop polls the receiver each frame and
    /// surfaces success/failure in `status` when the worker thread
    /// completes. Joined eagerly to release the thread.
    export_job: Option<ExportJob>,
}

/// One in-flight preview render. The worker owns a Pipeline + the
/// ImageBuf clone and, when it finishes, sends back the output buffer
/// plus the two timings the status bar shows.
struct RenderJob {
    rx: mpsc::Receiver<Result<RenderResult, String>>,
    handle: Option<JoinHandle<()>>,
    input_revision: u64,
    raw_wb: spektrafilm_io::RawWhiteBalance,
}

struct RenderResult {
    input: Arc<ImageBuf>,
    output: ImageBuf,
    preview: PreviewTextureData,
    input_clone_ms: f32,
    scale_ms: f32,
    pipeline_build_ms: f32,
    render_ms: f32,
    preview_ms: f32,
    worker_total_ms: f32,
}

struct PreviewTextureData {
    size: [usize; 2],
    rgba: Vec<u8>,
}

/// One in-flight f64 export. The worker thread owns the child
/// process and polls `cancel` in its wait loop. On completion the
/// worker sends `Ok(elapsed_seconds, output_filename)` or `Err(msg)`;
/// `Err("cancelled")` is also produced when the cancel flag is set.
/// The join handle is held so we can `join()` after consuming the
/// message and on `on_exit` to drain the worker before the process dies.
struct ExportJob {
    rx: mpsc::Receiver<Result<(f32, String), String>>,
    handle: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    started_at: Instant,
}

impl App {
    fn new(backend: Arc<dyn ComputeBackend>, initial_image: Option<PathBuf>) -> Self {
        let data_dir = pick_data_dir();
        let (films, papers) = scan_profiles(&data_dir);
        tracing::info!(data_dir = %data_dir.display(), films = films.len(), prints = papers.len(), "loaded profile catalog");
        let film_name = pick_default_stock(&films, "kodak_gold_200");
        // Load the default film once to derive two start-up choices:
        //   - the paired print paper (`target_print`): each profile is
        //     tuned against a specific paper, so a generic default gives a
        //     colour-shifted preview (Kodak Gold pairs with Portra Endura,
        //     not Fujifilm Crystal Archive).
        //   - whether to scan the film directly: positive/slide stocks have
        //     no print paper and must be scanned (see the film-change
        //     handler in `controls_panel`).
        let film_profile = profile::load_profile_by_name(&data_dir, &film_name).ok();
        let print_name = film_profile
            .as_ref()
            .and_then(|f| f.info.target_print.clone())
            .filter(|t| papers.iter().any(|p| &p.stock == t))
            .unwrap_or_else(|| pick_default_stock(&papers, "fujifilm_crystal_archive_typeii"));
        let mut params = RuntimeParams::default();
        params.io.scan_film = film_profile.as_ref().is_some_and(|f| f.is_positive());
        let film_dev_times = film_profile
            .as_ref()
            .map(|f| f.data.development_time.clone())
            .unwrap_or_default();
        let print_dev_times = profile_dev_times(&data_dir, &print_name);

        let mut app = Self {
            backend,
            data_dir,
            films,
            papers,
            film_name,
            print_name,
            film_dev_times,
            print_dev_times,
            params,
            limit_preview_12mp: false,
            output_format: OutputFormat::Png,
            image_path: None,
            last_export_dir: None,
            image: None,
            loaded_raw_wb: spektrafilm_io::RawWhiteBalance::default(),
            input_revision: 0,
            output_image: None,
            output_tex: None,
            pipeline_cache_key: None,
            pipeline_cache: None,
            last_render_ms: 0.0,
            last_pipeline_build_ms: 0.0,
            last_input_clone_ms: 0.0,
            last_scale_ms: 0.0,
            last_preview_ms: 0.0,
            last_worker_total_ms: 0.0,
            pending_dirty: false,
            dirty_since: None,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            render_job: None,
            status: String::from("Load an image to start."),
            dirty: false,
            #[cfg(target_os = "macos")]
            metal_colorspace_tagged: false,
            export_job: None,
        };
        if let Some(p) = initial_image {
            app.load_image_from_path(&p);
        }
        app
    }

    fn load_image_from_path(&mut self, path: &Path) {
        let t = Instant::now();
        match spektrafilm_io::load_image_with_wb(
            path,
            spektrafilm_io::Formats::Any,
            self.params.io.raw_white_balance,
        ) {
            Ok(img) => {
                // PNG and RAW come out of the loader as linear sRGB.
                // Tell the pipeline so it doesn't decode the gamma twice.
                if spektrafilm_io::input_is_linear_srgb(path) {
                    self.params.io.input_color_space = "sRGB".to_string();
                    self.params.io.input_cctf_decoding = false;
                }
                self.status = format!(
                    "Loaded {} × {} ({:.1} MP) in {:.0} ms",
                    img.width,
                    img.height,
                    (img.pixel_count() as f64 / 1e6),
                    t.elapsed().as_secs_f32() * 1000.0
                );
                self.image = Some(Arc::new(img));
                self.loaded_raw_wb = self.params.io.raw_white_balance;
                self.invalidate_input_preview();
                self.image_path = Some(path.to_path_buf());
                // A manual turn belongs to the previous photo.
                self.params.io.rotate_cw = 0;
                self.pan = egui::Vec2::ZERO;
            }
            Err(e) => {
                self.status = format!("Load error: {e:#}");
            }
        }
    }

    fn invalidate_input_preview(&mut self) {
        self.input_revision += 1;
        self.output_image = None;
        self.output_tex = None;
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        // Debounce from the latest edit, including edits made while a render
        // is in flight. Otherwise intermediate slider values can be rendered.
        self.dirty_since = Some(Instant::now());
    }

    fn raw_wb_pending(&self) -> bool {
        self.image_path
            .as_deref()
            .is_some_and(spektrafilm_io::is_raw)
            && self.loaded_raw_wb != self.params.io.raw_white_balance
    }

    fn reset_settings(&mut self) {
        let previous_wb = self.params.io.raw_white_balance;
        let mut params = RuntimeParams::default();
        // Match the defaults chosen when this image and film were loaded.
        if self
            .image_path
            .as_deref()
            .is_some_and(spektrafilm_io::input_is_linear_srgb)
        {
            params.io.input_color_space = "sRGB".into();
            params.io.input_cctf_decoding = false;
        }
        params.io.scan_film = profile::load_profile_by_name(&self.data_dir, &self.film_name)
            .is_ok_and(|film| film.is_positive());
        self.params = params;
        self.limit_preview_12mp = false;
        self.pan = egui::Vec2::ZERO;
        if previous_wb != self.params.io.raw_white_balance {
            self.invalidate_input_preview();
        } else {
            self.mark_dirty();
        }
    }

    fn preview_pipeline(
        &mut self,
        film_name: &str,
        print_name: &str,
        params: &RuntimeParams,
    ) -> Result<(Pipeline, f32), String> {
        let t = Instant::now();
        let key = preview_pipeline_cache_key(film_name, print_name, params);
        if self.pipeline_cache_key.as_deref() == Some(key.as_str())
            && let Some(pipeline) = self.pipeline_cache.as_ref()
        {
            return Ok((
                pipeline.clone().with_params(params.clone()),
                t.elapsed().as_secs_f32() * 1000.0,
            ));
        }

        let film = profile::load_profile_by_name(&self.data_dir, film_name)
            .map_err(|e| format!("film profile '{film_name}': {e}"))?;
        let effective_print_name = if params.io.scan_film {
            film_name
        } else {
            print_name
        };
        let print = profile::load_profile_by_name(&self.data_dir, effective_print_name)
            .map_err(|e| format!("print profile '{effective_print_name}': {e}"))?;
        let pipeline = Pipeline::new_with_spectral(film, print, params.clone(), &self.data_dir)
            .map_err(|e| format!("pipeline build: {e}"))?;
        self.pipeline_cache_key = Some(key);
        self.pipeline_cache = Some(pipeline.clone());
        Ok((pipeline, t.elapsed().as_secs_f32() * 1000.0))
    }

    fn preview_params(&self) -> RuntimeParams {
        let mut params = self.params.clone();
        // Serial grain is an export preference, including when preview falls
        // back to the CPU. Keep the user's stored choice for the export JSON.
        params.settings.serial_grain = false;
        params
    }

    /// Spawn a render on a worker thread. The UI stays interactive
    /// during the 250–500 ms pipeline run (previously this blocked
    /// the main thread, dropping mid-drag slider events). If a job
    /// is already in flight, mark `pending_dirty` so the latest
    /// params get re-rendered as soon as the in-flight one returns.
    fn dispatch_render(&mut self, ctx: &egui::Context) {
        if self.render_job.is_some() {
            self.pending_dirty = true;
            return;
        }
        let t_clone = Instant::now();
        let Some(image) = self.image.as_ref().cloned() else {
            return;
        };
        let input_clone_ms = t_clone.elapsed().as_secs_f32() * 1000.0;
        let reload_path = self
            .raw_wb_pending()
            .then(|| self.image_path.clone())
            .flatten();
        let raw_wb = self.params.io.raw_white_balance;
        let input_revision = self.input_revision;
        let film_name = self.film_name.clone();
        let print_name = self.print_name.clone();
        let mut params = self.preview_params();
        let limit_preview_12mp = self.limit_preview_12mp;
        let (pipeline_template, pipeline_build_ms) =
            match self.preview_pipeline(&film_name, &print_name, &params) {
                Ok(p) => p,
                Err(msg) => {
                    eprintln!("[spektrafilm] render error: {msg}");
                    self.status = format!("Render error: {msg}");
                    return;
                }
            };
        let backend = self.backend.clone();
        let (tx, rx) = mpsc::channel();
        let ctx_for_worker = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("spektrafilm-render".into())
            .spawn(move || {
                // A panic inside the pipeline must still produce a channel
                // message — otherwise the receiver only sees a disconnect
                // and the user gets a uselessly vague status line.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let t_total = Instant::now();
                    // Re-develop from the RAW, never multiply the already
                    // balanced self.image. Use exactly the CLI's loading path.
                    let image = if let Some(path) = reload_path {
                        Arc::new(
                            spektrafilm_io::load_image_with_wb(
                                &path,
                                spektrafilm_io::Formats::Any,
                                raw_wb,
                            )
                            .map_err(|e| format!("RAW reload: {e:#}"))?,
                        )
                    } else {
                        image
                    };
                    let t_scale = Instant::now();
                    let dimensions = preview_dimensions(
                        image.width,
                        image.height,
                        params.io.upscale_factor,
                        limit_preview_12mp,
                    );
                    let scaled_image = (dimensions != (image.width, image.height))
                        .then(|| rescale_preview_input_fast(&image, dimensions));
                    let scale_ms = t_scale.elapsed().as_secs_f32() * 1000.0;
                    params.io.upscale_factor = 1.0;
                    let pipeline = pipeline_template.with_params(params);
                    let t = Instant::now();
                    let output = match scaled_image {
                        Some(image) => pipeline.process(image, backend.as_ref()),
                        None if pipeline.params.io.scan_film && pipeline.film.is_positive() => {
                            pipeline.process((*image).clone(), backend.as_ref())
                        }
                        None => pipeline
                            .process_resident_borrowed(image.as_ref(), backend.as_ref())
                            .unwrap_or_else(|| {
                                pipeline.process((*image).clone(), backend.as_ref())
                            }),
                    };
                    let render_ms = t.elapsed().as_secs_f32() * 1000.0;
                    let t_preview = Instant::now();
                    let preview = make_preview_texture_data(&output);
                    let preview_ms = t_preview.elapsed().as_secs_f32() * 1000.0;
                    let worker_total_ms = t_total.elapsed().as_secs_f32() * 1000.0;
                    Ok(RenderResult {
                        input: image,
                        output,
                        preview,
                        input_clone_ms,
                        scale_ms,
                        pipeline_build_ms,
                        render_ms,
                        preview_ms,
                        worker_total_ms,
                    })
                }))
                .unwrap_or_else(|panic| Err(panic_message(&panic)));
                let _ = tx.send(result);
                ctx_for_worker.request_repaint();
            })
            .expect("OS thread spawn");
        self.render_job = Some(RenderJob {
            rx,
            handle: Some(handle),
            input_revision,
            raw_wb,
        });
    }

    /// Central-panel preview with mouse-wheel zoom (centred on the
    /// cursor) and click-drag pan. Double-click resets to fit.
    /// `self.zoom = 1.0` means fit-to-panel; values >1 zoom in.
    fn draw_preview_with_zoom(&mut self, ui: &mut egui::Ui, tex: &egui::TextureHandle) {
        let avail = ui.available_size();
        let rotation = self.params.io.rotate_cw.rem_euclid(4);
        // Rotation is applied at display time (and at save/export time)
        // so turning the photo is instant — no pipeline re-render.
        let img_size = if rotation % 2 == 1 {
            let s = tex.size_vec2();
            egui::vec2(s.y, s.x)
        } else {
            tex.size_vec2()
        };
        let fit_scale = (avail.x / img_size.x).min(avail.y / img_size.y).min(1.0);
        let scale = fit_scale * self.zoom;
        let drawn_size = img_size * scale;

        let response = ui.allocate_response(avail, egui::Sense::click_and_drag());
        let rect = response.rect;

        // Two zoom sources, never combined:
        //   1. trackpad pinch (`zoom_delta` — already a multiplier
        //      around 1.0, smoothed by egui per frame)
        //   2. mouse wheel scroll (`smooth_scroll_delta.y` — egui's
        //      per-frame integrated scroll). Don't add `raw_scroll_delta`
        //      on top; `smooth` already accumulates the raw events.
        // Both are anchored on the cursor so the pixel under the
        // pointer stays put.
        let (pinch, scroll) = ui.input(|i| {
            if response.hovered() {
                (i.zoom_delta(), i.smooth_scroll_delta.y)
            } else {
                (1.0, 0.0)
            }
        });
        // Scroll → small log-zoom step per notch. 0.0015 keeps a single
        // trackpad swipe feeling like a smooth zoom rather than a leap.
        let scroll_factor = if scroll.abs() > 0.01 {
            (scroll * 0.0015).exp()
        } else {
            1.0
        };
        let factor = pinch * scroll_factor;
        if (factor - 1.0).abs() > 1e-4 {
            let cursor = ui.input(|i| i.pointer.hover_pos());
            let old_zoom = self.zoom;
            self.zoom = (self.zoom * factor).clamp(0.1, 32.0);
            if let Some(cur) = cursor {
                // Keep the image point under the cursor anchored.
                let centre = rect.center() + self.pan;
                let offset_from_centre = cur - centre;
                let scale_ratio = self.zoom / old_zoom;
                self.pan += offset_from_centre * (1.0 - scale_ratio);
            }
        }

        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.double_clicked() {
            self.zoom = 1.0;
            self.pan = egui::Vec2::ZERO;
        }

        let centre = rect.center() + self.pan;
        let draw_rect = egui::Rect::from_center_size(centre, drawn_size);
        // Clip to the central panel so panned-off pixels don't leak.
        let painter = ui.painter_at(rect);
        painter.add(rotated_image_mesh(tex.id(), draw_rect, rotation));

        // Rotate buttons + zoom indicator in the corner.
        let pct = (self.zoom * fit_scale * 100.0).round() as i32;
        let text = if (self.zoom - 1.0).abs() < 0.001 && self.pan.length_sq() < 1.0 {
            "fit".to_string()
        } else {
            format!("{pct} %  (double-click to reset)")
        };
        let bar_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(8.0, 6.0),
            egui::vec2(rect.width() - 16.0, 22.0),
        );
        let mut turn = 0;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("⟲").on_hover_text("Rotate left").clicked() {
                    turn = -1;
                }
                if ui.small_button("⟳").on_hover_text("Rotate right").clicked() {
                    turn = 1;
                }
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(12.0)
                        .color(egui::Color32::from_rgba_premultiplied(220, 220, 220, 200)),
                );
            });
        });
        if turn != 0 {
            self.rotate(turn);
        }
    }

    /// Turn the photo by `quarter_turns` × 90° clockwise (negative =
    /// counter-clockwise). Display-only until Save/Export, which both
    /// apply `params.io.rotate_cw` to the rendered output.
    fn rotate(&mut self, quarter_turns: i32) {
        self.params.io.rotate_cw = (self.params.io.rotate_cw + quarter_turns).rem_euclid(4);
        self.pan = egui::Vec2::ZERO;
    }

    /// Called once per `update()`. If the in-flight render finished,
    /// upload the texture and unblock the next pass. If more changes
    /// arrived during the render, re-arm `dirty`.
    fn poll_render_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.render_job.as_mut() else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.render_job = None;
                self.status = "Render error: worker thread vanished".into();
                return;
            }
        };
        if let Some(h) = job.handle.take() {
            let _ = h.join();
        }
        let input_revision = job.input_revision;
        let raw_wb = job.raw_wb;
        self.render_job = None;
        // A WB edit or a newly opened file must not be overwritten by an
        // older in-flight render, including its developed input buffer.
        if input_revision != self.input_revision {
            self.pending_dirty = false;
            self.dirty = true;
            return;
        }
        // A slider may have changed after this job captured its params.
        // Keep the last complete preview until a render of the latest state
        // is ready; displaying this result causes a visible intermediate flash.
        if self.dirty || self.pending_dirty {
            self.pending_dirty = false;
            self.dirty = true;
            return;
        }
        match result {
            Ok(r) => {
                self.image = Some(r.input);
                self.loaded_raw_wb = raw_wb;
                self.last_input_clone_ms = r.input_clone_ms;
                self.last_scale_ms = r.scale_ms;
                self.last_pipeline_build_ms = r.pipeline_build_ms;
                self.last_render_ms = r.render_ms;
                self.last_preview_ms = r.preview_ms;
                self.last_worker_total_ms = r.worker_total_ms;
                self.output_tex = Some(make_texture(ctx, r.preview));
                tracing::info!(
                    width = r.output.width,
                    height = r.output.height,
                    render_ms = r.render_ms,
                    "preview texture ready"
                );
                self.status = format!(
                    "Rendered {} × {} ({:.1} MP)",
                    r.output.width,
                    r.output.height,
                    r.output.pixel_count() as f64 / 1e6
                );
                self.output_image = Some(r.output);
            }
            Err(msg) => {
                if self.raw_wb_pending() {
                    // Keep the last successfully loaded image and settings
                    // paired. Export must never use a failed WB selection.
                    self.params.io.raw_white_balance = self.loaded_raw_wb;
                    self.dirty = true;
                }
                eprintln!("[spektrafilm] render error: {msg}");
                self.status = format!("Render error: {msg}");
            }
        }
    }

    /// Open a save dialog and write the most recent rendered output to
    /// disk. Suggested filename is the input stem + the chosen film
    /// stock + the selected output format.
    fn save_dialog(&mut self) {
        let Some(out) = self.output_image.clone() else {
            self.status = "Nothing to save yet — load an image first.".into();
            return;
        };
        let default_name = match &self.image_path {
            Some(p) => {
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("spektrafilm");
                format!(
                    "{stem}_{}_spektra.{}",
                    self.film_name,
                    self.output_format.extension()
                )
            }
            None => format!("spektrafilm.{}", self.output_format.extension()),
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "tif", "tiff", "jpg", "jpeg"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(self.output_format.extension());
        }
        let t = Instant::now();
        let out = out.rotated_cw(self.params.io.rotate_cw);
        // Carry the source's camera metadata (EXIF) over to the output.
        let metadata = self
            .image_path
            .as_deref()
            .and_then(spektrafilm_io::read_metadata);
        match spektrafilm_io::save_image(&out, &path, metadata.as_ref()) {
            Ok(()) => {
                self.status = format!(
                    "Saved {} in {:.0} ms",
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("(file)"),
                    t.elapsed().as_secs_f32() * 1000.0
                );
            }
            Err(e) => {
                self.status = format!("Save error: {e:#}");
            }
        }
    }

    /// Export the current frame by subprocessing the f64-built
    /// `spektrafilm` CLI. The GUI runs the pipeline at f32 on the GPU
    /// for fast iteration; export re-runs the pipeline at f64 (CPU)
    /// using the same params and writes a standard PNG/TIFF/JPEG.
    ///
    /// The f64 CLI is located via, in order: `$SPEKTRAFILM_F64_CLI`,
    /// then `spektrafilm-f64` on `PATH`, then `target/release/spektrafilm-f64`
    /// relative to `CARGO_MANIFEST_DIR`. Build it with
    /// `cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64`.
    fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(input_path) = self.image_path.clone() else {
            self.status = "Load an image before exporting.".into();
            return;
        };
        let cli_path = match locate_f64_cli() {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("Export: {e}");
                return;
            }
        };
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("spektrafilm");
        let default_name = format!(
            "{stem}_{}_spektra_f64.{}",
            self.film_name,
            self.output_format.extension()
        );
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.last_export_dir.as_deref().filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut out_path) = dialog
            .add_filter("Image", &["png", "tif", "tiff", "jpg", "jpeg"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        if out_path.extension().is_none() {
            out_path.set_extension(self.output_format.extension());
        }
        self.last_export_dir = out_path.parent().map(Path::to_path_buf);

        // Spawn the export on a worker thread so the egui event loop
        // keeps drawing. The cancel flag is shared with the worker so
        // the Cancel button (and `on_exit`) can kill the child cleanly
        // instead of letting it orphan after the GUI window closes.
        //
        // Enlarger + scanner LUTs are toggled on for the export only.
        // The GUI preview runs on wgpu (fast at full spectral
        // integration; LUT round-trip via CPU PCHIP would only slow
        // it down), but the f64 CPU export gets a 5-10× speedup from
        // the LUT path — matching Python's typical export config.
        let film = self.film_name.clone();
        let paper = self.print_name.clone();
        let mut params = self.params.clone();
        params.settings.use_enlarger_lut = true;
        params.settings.use_scanner_lut = true;
        let data_dir = self.data_dir.clone();
        let (tx, rx) = mpsc::channel();
        let ctx_for_worker = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = Arc::clone(&cancel);
        let started_at = Instant::now();
        let handle = std::thread::Builder::new()
            .name("spektrafilm-export".into())
            .spawn(move || {
                let res = run_f64_export(
                    &cli_path,
                    &input_path,
                    &out_path,
                    &film,
                    &paper,
                    &params,
                    &data_dir,
                    &cancel_for_worker,
                );
                let name = out_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("(file)")
                    .to_string();
                let msg = match res {
                    Ok(()) => Ok((started_at.elapsed().as_secs_f32(), name)),
                    Err(e) => Err(format!("{e:#}")),
                };
                let _ = tx.send(msg);
                ctx_for_worker.request_repaint();
            })
            .expect("OS thread spawn");
        self.status = "Exporting at f64 (CPU)…".into();
        self.export_job = Some(ExportJob {
            rx,
            handle: Some(handle),
            cancel,
            started_at,
        });
    }

    /// Request cancellation of the in-flight export. The worker thread
    /// observes the flag in its `try_wait` poll loop and SIGKILLs the
    /// child; `poll_export_job` then drains the resulting error message
    /// on the next `update()` tick.
    fn cancel_export(&mut self) {
        let Some(job) = self.export_job.as_ref() else {
            return;
        };
        if !job.cancel.swap(true, Ordering::SeqCst) {
            self.status = "Cancelling export…".into();
        }
    }

    /// Called once per `update()`. If the export is still in-flight,
    /// refreshes the status with elapsed time and requests a repaint a
    /// second from now (so the timer ticks without us busy-looping).
    /// If the job finished, drains the result into the status bar and
    /// joins the worker thread.
    fn poll_export_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.export_job.as_mut() else {
            return;
        };
        let msg = match job.rx.try_recv() {
            Ok(m) => m,
            Err(mpsc::TryRecvError::Empty) => {
                let secs = job.started_at.elapsed().as_secs();
                self.status = if job.cancel.load(Ordering::SeqCst) {
                    format!("Cancelling export… ({secs}s)")
                } else {
                    format!("Exporting at f64 (CPU)… ({secs}s)")
                };
                ctx.request_repaint_after(Duration::from_secs(1));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Export error: worker thread vanished".into();
                self.export_job = None;
                return;
            }
        };
        if let Some(h) = job.handle.take() {
            let _ = h.join();
        }
        self.export_job = None;
        self.status = match msg {
            Ok((secs, name)) => format!("Exported (f64 CPU) {name} in {secs:.1} s"),
            Err(e) if e.contains("cancelled") => "Export cancelled.".into(),
            Err(e) => format!("Export error: {e}"),
        };
    }

    fn controls_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("spektrafilm");
        ui.add_space(6.0);

        // ── File ────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            if ui.button("Open…").clicked() {
                let mut dialog = rfd::FileDialog::new();
                // Re-open the picker where the previously loaded file
                // lives, with that file preselected where the platform
                // supports it (macOS/Windows).
                if let Some(prev) = &self.image_path {
                    if let Some(dir) = prev.parent().filter(|d| d.is_dir()) {
                        dialog = dialog.set_directory(dir);
                        if let Some(name) = prev.file_name().and_then(|n| n.to_str()) {
                            dialog = dialog.set_file_name(name);
                        }
                    }
                }
                if let Some(path) = dialog
                    .add_filter(
                        "Image",
                        &[
                            "png", "tif", "tiff", // standard
                            "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf",
                            "rw2", "pef", "srw", "x3f", "iiq", "3fr", "crw", "rwl", "mrw", "mef",
                            "kdc",
                        ],
                    )
                    .pick_file()
                {
                    self.load_image_from_path(&path);
                }
            }
            egui::ComboBox::from_id_salt("output_format")
                .selected_text(self.output_format.label())
                .show_ui(ui, |ui| {
                    for format in [OutputFormat::Png, OutputFormat::Jpeg, OutputFormat::Tiff] {
                        ui.selectable_value(&mut self.output_format, format, format.label());
                    }
                })
                .response
                .on_hover_text("Sets the suggested format for Save and Export. You can also change the filename extension. JPEG uses quality 95; TIFF is uncompressed.");
            let save_enabled = self.output_image.is_some();
            if ui
                .add_enabled(save_enabled, egui::Button::new("Save…"))
                .on_hover_text("Save the rendered preview at its displayed resolution. Use Export for the export resolution, independent of the 12 MP preview limit.")
                .on_disabled_hover_text("Render an image first")
                .clicked()
            {
                self.save_dialog();
            }
            let export_busy = self.export_job.is_some();
            if export_busy {
                if ui
                    .button("Cancel")
                    .on_hover_text("Stop the in-flight f64 CPU export and kill the child process.")
                    .clicked()
                {
                    self.cancel_export();
                }
            } else {
                let export_enabled = self.image_path.is_some() && !self.raw_wb_pending();
                if ui
                    .add_enabled(export_enabled, egui::Button::new("Export…"))
                    .on_hover_text(
                        "Re-run the pipeline at f64 precision (CPU, via the spektrafilm-f64 \
                         CLI subprocess) and write a PNG/TIFF/JPEG. The live preview stays \
                         at f32 GPU for speed; export trades time for precision.",
                    )
                    .on_disabled_hover_text(
                        "Load an image and wait for RAW white balance to finish",
                    )
                    .clicked()
                {
                    self.export_dialog(ctx);
                }
            }
        });
        if let Some(p) = &self.image_path {
            ui.label(
                egui::RichText::new(p.file_name().and_then(|s| s.to_str()).unwrap_or("")).small(),
            );
        }
        ui.add_space(4.0);

        egui::CollapsingHeader::new("RAW white balance")
            .default_open(true)
            .show(ui, |ui| {
                let is_raw = self.image_path.as_deref().is_some_and(spektrafilm_io::is_raw);
                let before = self.params.io.raw_white_balance;
                ui.add_enabled_ui(is_raw, |ui| {
                    use spektrafilm_io::RawWhiteBalanceMode;
                    let wb = &mut self.params.io.raw_white_balance;
                    egui::ComboBox::from_label("Mode")
                        .selected_text(wb.mode.label())
                        .show_ui(ui, |ui| {
                            for mode in RawWhiteBalanceMode::ALL {
                                ui.selectable_value(&mut wb.mode, mode, mode.label());
                            }
                        });
                    ui.add_enabled_ui(wb.mode == RawWhiteBalanceMode::Custom, |ui| {
                        ui.add(egui::Slider::new(&mut wb.temperature, 1667.0..=25000.0)
                            .logarithmic(true).text("Temperature (K)"));
                        ui.add(egui::Slider::new(&mut wb.tint, 0.01..=4.0).text("Tint"))
                            .on_hover_text("Green multiplier in linear ACES: 1 is neutral; higher is greener, lower is more magenta.");
                    });
                });
                if !is_raw {
                    ui.small("RAW files only; RGB inputs already have white balance applied.");
                } else {
                    ui.small("Re-develops the input RAW for preview and export.");
                    if self.raw_wb_pending() {
                        ui.small("Updating RAW white balance…");
                    }
                }
                if before != self.params.io.raw_white_balance {
                    self.invalidate_input_preview();
                }
            });

        // ── Profiles ────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Profiles")
            .default_open(true)
            .show(ui, |ui| {
                let film_changed =
                    profile_combo(ui, "film", "Film stock", &self.films, &mut self.film_name);
                if film_changed {
                    self.film_dev_times = Vec::new();
                    self.params.film_render.development_time = None;
                    if let Ok(film) = profile::load_profile_by_name(&self.data_dir, &self.film_name)
                    {
                        // Slide/positive stocks have no print paper — they
                        // are scanned directly. Negatives print onto their
                        // paired `target_print`. Auto-follow the film type
                        // so switching stocks doesn't leave a wrong (or, for
                        // a paperless slide, a failed) render.
                        self.params.io.scan_film = film.is_positive();
                        self.film_dev_times = film.data.development_time.clone();
                        if let Some(target) = film.info.target_print.as_deref()
                            && self.papers.iter().any(|p| p.stock == target)
                            && self.print_name != target
                        {
                            self.print_name = target.to_string();
                            self.print_dev_times =
                                profile_dev_times(&self.data_dir, &self.print_name);
                            self.params.print_render.development_time = None;
                        }
                    }
                    self.mark_dirty();
                }
                // B&W stocks are profiled at several development times —
                // pick one (longer = more contrast). Hidden for the
                // single-time / colour case.
                if self.film_dev_times.len() > 1
                    && dev_time_combo(
                        ui,
                        "film_dev_time",
                        "Development time",
                        &self.film_dev_times.clone(),
                        &mut self.params.film_render.development_time,
                    )
                {
                    self.mark_dirty();
                }
                // Slide films are scanned directly, so the print paper is
                // unused — disable the picker and say why rather than
                // letting it silently affect nothing.
                if self.params.io.scan_film {
                    ui.label(
                        egui::RichText::new("Direct film scan — printing is skipped.")
                            .italics()
                            .small(),
                    );
                }
                let paper_changed = ui
                    .add_enabled_ui(!self.params.io.scan_film, |ui| {
                        profile_combo(
                            ui,
                            "paper",
                            "Print paper / film",
                            &self.papers,
                            &mut self.print_name,
                        )
                    })
                    .inner;
                if paper_changed {
                    self.print_dev_times = profile_dev_times(&self.data_dir, &self.print_name);
                    self.params.print_render.development_time = None;
                    self.mark_dirty();
                }
                if !self.params.io.scan_film
                    && self.print_dev_times.len() > 1
                    && dev_time_combo(
                        ui,
                        "print_dev_time",
                        "Print development time",
                        &self.print_dev_times.clone(),
                        &mut self.params.print_render.development_time,
                    )
                {
                    self.mark_dirty();
                }
            });

        // ── Exposure ────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Exposure")
            .default_open(true)
            .show(ui, |ui| {
                let mut changed = false;
                changed |= ui
                    .checkbox(&mut self.params.camera.auto_exposure, "Auto exposure")
                    .changed();
                if self.params.camera.auto_exposure {
                    let method = &mut self.params.camera.auto_exposure_method;
                    egui::ComboBox::from_label("Metering")
                        .selected_text(method.clone())
                        .show_ui(ui, |ui| {
                            for opt in [
                                "average",
                                "median",
                                "center_weighted",
                                "partial",
                                "matrix",
                                "multi_zone",
                                "highlight_weighted",
                            ] {
                                changed |=
                                    ui.selectable_value(method, opt.to_string(), opt).changed();
                            }
                        });
                }
                changed |= ui
                    .add(
                        egui::Slider::new(
                            &mut self.params.camera.exposure_compensation_ev,
                            -5.0..=5.0,
                        )
                        .text("EV compensation")
                        .step_by(0.1),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.params.camera.film_format_mm, 4.0..=120.0)
                            .text("Film format (mm)")
                            .logarithmic(true),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.params.camera.lens_blur_um, 0.0..=100.0)
                            .text("Lens blur (µm)"),
                    )
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Halation ────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Halation")
            .default_open(true)
            .show(ui, |ui| {
                let h = &mut self.params.film_render.halation;
                let mut changed = false;
                changed |= ui.checkbox(&mut h.active, "Active").changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut h.halation_amount, 0.0..=3.0)
                            .text("Halation amount"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut h.halation_spatial_scale, 0.1..=5.0)
                            .text("Halation scale"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut h.scatter_amount, 0.0..=3.0).text("Scatter amount"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut h.scatter_spatial_scale, 0.1..=5.0)
                            .text("Scatter scale"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut h.halation_n_bounces, 1..=5).text("Bounces"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut h.halation_bounce_decay, 0.0..=1.0)
                            .text("Bounce decay"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut h.halation_renormalize, "Renormalize")
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── DIR couplers ────────────────────────────────────────────────
        egui::CollapsingHeader::new("DIR couplers")
            .default_open(true)
            .show(ui, |ui| {
                let d = &mut self.params.film_render.dir_couplers;
                let mut changed = false;
                changed |= ui.checkbox(&mut d.active, "Active").changed();
                changed |= ui
                    .add(egui::Slider::new(&mut d.amount, 0.0..=2.0).text("Amount"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut d.diffusion_size_um, 0.0..=100.0)
                            .text("Diffusion size (µm)"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut d.diffusion_tail_um, 0.0..=400.0)
                            .text("Diffusion tail (µm)"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut d.diffusion_tail_weight, 0.0..=1.0)
                            .text("Tail weight"),
                    )
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Diffusion filter (lens) ─────────────────────────────────────
        egui::CollapsingHeader::new("Diffusion filter (lens)")
            .default_open(true)
            .show(ui, |ui| {
                let df = &mut self.params.camera.diffusion_filter;
                let mut changed = false;
                changed |= ui.checkbox(&mut df.active, "Active").changed();
                egui::ComboBox::from_label("Family")
                    .selected_text(df.filter_family.clone())
                    .show_ui(ui, |ui| {
                        for fam in ["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"] {
                            changed |= ui
                                .selectable_value(&mut df.filter_family, fam.to_string(), fam)
                                .changed();
                        }
                    });
                changed |= ui
                    .add(egui::Slider::new(&mut df.strength, 0.0..=2.0).text("Strength"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut df.spatial_scale, 0.1..=3.0).text("Spatial scale"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut df.halo_warmth, -1.5..=1.5).text("Halo warmth"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut df.core_intensity, 0.0..=2.0).text("Core intensity"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut df.halo_intensity, 0.0..=2.0).text("Halo intensity"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut df.bloom_intensity, 0.0..=2.0)
                            .text("Bloom intensity"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut df.halo_size, 0.1..=3.0).text("Halo size"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut df.bloom_size, 0.1..=3.0).text("Bloom size"))
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Grain ───────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Grain")
            .default_open(true)
            .show(ui, |ui| {
                let g = &mut self.params.film_render.grain;
                let mut changed = false;
                changed |= ui.checkbox(&mut g.active, "Active").changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut g.agx_particle_area_um2, 0.05..=1.0)
                            .text("Particle area (µm²)"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut g.blur, 0.0..=3.0).text("Post-blur σ"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut g.n_sub_layers, 1..=4).text("Sub-layers"))
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Glare ───────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Glare")
            .default_open(false)
            .show(ui, |ui| {
                let g = &mut self.params.print_render.glare;
                let mut changed = false;
                changed |= ui.checkbox(&mut g.active, "Active").changed();
                changed |= ui
                    .add(egui::Slider::new(&mut g.percent, 0.0..=0.2).text("Percent"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut g.roughness, 0.0..=2.0).text("Roughness"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut g.blur, 0.0..=5.0).text("Blur σ (px)"))
                    .changed();
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Print curves (s023 morph) ───────────────────────────────────
        egui::CollapsingHeader::new("Print curves")
            .default_open(false)
            .show(ui, |ui| {
                let m = &mut self.params.print_render.density_curves_morph;
                let mut changed = false;
                changed |= ui
                    .checkbox(&mut m.active, "Morph density curves")
                    .on_hover_text(
                        "Rebuild the print density curves from the profile's parametric \
                         model with coupled-gamma morphing. Off = use the stored curves.",
                    )
                    .changed();
                if m.active {
                    changed |= ui
                        .add(egui::Slider::new(&mut m.gamma_factor, 0.5..=2.0).text("Gamma"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut m.gamma_factor_fast, 0.5..=2.0)
                                .text("Gamma fast"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut m.gamma_factor_slow, 0.5..=2.0)
                                .text("Gamma slow"),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut m.gamma_factor_red, 0.5..=2.0).text("Gamma R"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut m.gamma_factor_green, 0.5..=2.0).text("Gamma G"),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut m.gamma_factor_blue, 0.5..=2.0).text("Gamma B"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut m.developer_exhaustion, 0.0..=1.0)
                                .text("Developer exhaustion"),
                        )
                        .changed();
                }
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Scanner ─────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Scanner")
            .default_open(false)
            .show(ui, |ui| {
                let s = &mut self.params.scanner;
                let mut changed = false;
                changed |= ui
                    .add(egui::Slider::new(&mut s.lens_blur, 0.0..=5.0).text("Lens blur σ (px)"))
                    .changed();
                let [mut sigma, mut amount] = s.unsharp_mask;
                changed |= ui
                    .add(egui::Slider::new(&mut sigma, 0.0..=3.0).text("Unsharp σ (px)"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut amount, 0.0..=2.0).text("Unsharp amount"))
                    .changed();
                if changed {
                    s.unsharp_mask = [sigma, amount];
                }
                changed |= ui
                    .checkbox(&mut s.white_correction, "White correction")
                    .changed();
                if s.white_correction {
                    changed |= ui
                        .add(egui::Slider::new(&mut s.white_level, 0.5..=1.0).text("White level"))
                        .changed();
                }
                changed |= ui
                    .checkbox(&mut s.black_correction, "Black correction")
                    .changed();
                if s.black_correction {
                    changed |= ui
                        .add(egui::Slider::new(&mut s.black_level, 0.0..=0.5).text("Black level"))
                        .changed();
                }
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Color management ────────────────────────────────────────────
        egui::CollapsingHeader::new("Color management")
            .default_open(false)
            .show(ui, |ui| {
                let algo = &mut self.params.io.output_gamut_compress.algorithm;
                let mut changed = false;
                // Only the ported modes are offered; cam16ucs is upstream's
                // default, oklch the lighter perceptual option. Enabling either
                // drops the preview off the GPU-resident fast path.
                egui::ComboBox::from_label("Gamut compression")
                    .selected_text(algo.clone())
                    .show_ui(ui, |ui| {
                        for opt in ["off", "oklch", "oklrab", "cam16ucs", "aces_rgc"] {
                            changed |= ui.selectable_value(algo, opt.to_string(), opt).changed();
                        }
                    });
                // Input gamut compression — baked into the tc_lut at build time,
                // so changing it rebuilds the LUT on the next pass. "xy" is the
                // ACES-RGC-style radial compression toward the spectral locus.
                let in_algo = &mut self.params.io.input_gamut_compress.algorithm;
                egui::ComboBox::from_label("Input gamut compression")
                    .selected_text(in_algo.clone())
                    .show_ui(ui, |ui| {
                        for opt in ["off", "xy"] {
                            changed |= ui.selectable_value(in_algo, opt.to_string(), opt).changed();
                        }
                    });
                // CAT16 (vs CAT02) for the input chromatic adaptation feeding
                // Hanatos — better blue/violet behavior; matches upstream >=0.3.3.
                changed |= ui
                    .checkbox(
                        &mut self.params.settings.use_cat16,
                        "CAT16 input adaptation",
                    )
                    .changed();
                // RGB → film raw spectral upsampler. hanatos2025 is the default
                // spectral LUT; arctic2026alpha02 is the new memory-color
                // reflectance LUT; mallett2019 is a faster matrix basis.
                let method = &mut self.params.settings.rgb_to_raw_method;
                egui::ComboBox::from_label("RGB→raw upsampling")
                    .selected_text(method.clone())
                    .show_ui(ui, |ui| {
                        for opt in ["hanatos2025", "arctic2026alpha02", "mallett2019"] {
                            changed |= ui.selectable_value(method, opt.to_string(), opt).changed();
                        }
                    });
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Enlarger ────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Enlarger")
            .default_open(false)
            .show(ui, |ui| {
                let e = &mut self.params.enlarger;
                let mut changed = false;
                changed |= ui
                    .add(
                        egui::Slider::new(&mut e.print_exposure, 0.1..=5.0)
                            .text("Print exposure")
                            .logarithmic(true),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut e.m_filter_shift, -50.0..=50.0)
                            .text("Magenta filter shift"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut e.y_filter_shift, -50.0..=50.0)
                            .text("Yellow filter shift"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut e.preflash_exposure, 0.0..=0.5)
                            .text("Preflash exposure"),
                    )
                    .on_hover_text(
                        "Uniform low pre-exposure of the print through the film base. \
                         Lifts shadow density and lowers print contrast. 0 = off.",
                    )
                    .changed();
                if e.preflash_exposure > 0.0 {
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut e.preflash_m_filter_shift, -50.0..=50.0)
                                .text("Preflash magenta shift"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut e.preflash_y_filter_shift, -50.0..=50.0)
                                .text("Preflash yellow shift"),
                        )
                        .changed();
                }
                if changed {
                    self.mark_dirty();
                }
            });

        // ── Output ──────────────────────────────────────────────────────
        egui::CollapsingHeader::new("Output")
            .default_open(false)
            .show(ui, |ui| {
                let io = &mut self.params.io;
                let mut changed = false;
                let mut scan_film = io.scan_film;
                changed |= ui
                    .checkbox(&mut scan_film, "Scan film (skip printing)")
                    .on_hover_text(
                        "Scan the developed film directly instead of printing onto paper. \
                         Auto-enabled for positive/slide stocks (no print paper); toggle \
                         manually to scan a negative as-is.",
                    )
                    .changed();
                if changed {
                    io.scan_film = scan_film;
                }
                changed |= ui
                    .add(
                        egui::Slider::new(&mut io.upscale_factor, 0.1..=1.0)
                            .text("Working resolution"),
                    )
                    .on_hover_text(
                        "Resize the working image before processing (Python upscale_factor). \
                         Lower = faster preview + export at reduced resolution; 1.0 = full \
                         resolution. The diffusion-filter cost scales with this.",
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut io.output_cctf_encoding, "Output sRGB encoded")
                    .changed();
                if changed {
                    self.mark_dirty();
                }
                if ui
                    .checkbox(&mut self.limit_preview_12mp, "Limit preview to 12 MP")
                    .on_hover_text(
                        "Reduce preview memory and rendering time for large images. \
                         Export keeps the Working resolution setting; Save writes this \
                         smaller preview. Fine grain and detail can look different. \
                         Disable to inspect at the full working resolution.",
                    )
                    .changed()
                {
                    self.mark_dirty();
                }
                // This preference affects the next export, not the preview.
                ui.checkbox(
                    &mut self.params.settings.serial_grain,
                    "Serial grain for export (less RAM)",
                )
                .on_hover_text(
                    "Process grain channels one at a time during export. Uses less memory \
                     when grain is enabled, but exports take longer. Image quality and \
                     resolution are unchanged.",
                );
            });

        ui.add_space(8.0);
        if ui
            .button("Reset settings")
            .on_hover_text("Restore all defaults except the selected film stock and print paper.")
            .clicked()
        {
            self.reset_settings();
        }

        // ── Metrics ─────────────────────────────────────────────────────
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.monospace(format!(
            "render:        {:>6.1} ms   {}",
            self.last_render_ms,
            fps_label(self.last_render_ms)
        ));
        ui.monospace(format!(
            "pipeline build:{:>6.1} ms",
            self.last_pipeline_build_ms
        ));
        ui.monospace(format!(
            "clone/scale:   {:>6.1} / {:>5.1} ms",
            self.last_input_clone_ms, self.last_scale_ms
        ));
        ui.monospace(format!("preview pack:  {:>6.1} ms", self.last_preview_ms));
        ui.monospace(format!(
            "worker total:  {:>6.1} ms",
            self.last_worker_total_ms
        ));
        ui.monospace(format!("backend: {}", self.backend.name()));
        ui.label(egui::RichText::new(&self.status).small());
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // First-paint hook: tag the wgpu Metal layer's colorspace as sRGB.
        // Has to happen here (not in `App::new`) because the CAMetalLayer
        // isn't wired up at construction time.
        #[cfg(target_os = "macos")]
        if !self.metal_colorspace_tagged {
            match tag_metal_layer_srgb(frame) {
                Ok(()) => {
                    eprintln!("[spektrafilm] CAMetalLayer.colorspace = sRGB — tagged OK");
                    self.metal_colorspace_tagged = true;
                }
                Err(e) => {
                    eprintln!("[spektrafilm] colorspace tag attempt: {e}");
                }
            }
        }
        let _ = frame;

        if self.dirty && self.image.is_some() {
            let now = Instant::now();
            let dirty_since = *self.dirty_since.get_or_insert(now);
            if self.render_job.is_some() {
                self.pending_dirty = true;
                self.dirty = false;
            } else {
                let elapsed = now.saturating_duration_since(dirty_since);
                if elapsed >= PREVIEW_DEBOUNCE {
                    self.dispatch_render(ctx);
                    self.dirty = false;
                    self.dirty_since = None;
                } else {
                    ctx.request_repaint_after(PREVIEW_DEBOUNCE - elapsed);
                }
            }
        }
        if self.render_job.is_some() {
            ctx.request_repaint_after(IN_FLIGHT_REPAINT);
        }
        self.poll_export_job(ctx);
        egui::SidePanel::right("controls")
            .resizable(false)
            .exact_width(340.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls_panel(ui, ctx));
            });
        // Poll after controls so an edit made on this frame can invalidate a
        // just-finished render before its texture is shown in the center panel.
        self.poll_render_job(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            // Clone the texture handle (cheap — internally `Arc`) so we
            // can take `&mut self` to mutate `zoom` / `pan` from the
            // closure without a borrow conflict on `self.output_tex`.
            let tex = self.output_tex.clone();
            if let Some(tex) = tex {
                self.draw_preview_with_zoom(ui, &tex);
            } else if self.image.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Drop or open an image to start.").size(18.0));
                });
            }
            if self.image.is_some()
                && (self.dirty || self.pending_dirty || self.render_job.is_some())
            {
                let center = ui.max_rect().center();
                ui.painter()
                    .circle_filled(center, 18.0, egui::Color32::from_black_alpha(160));
                egui::Spinner::new().color(egui::Color32::WHITE).paint_at(
                    ui,
                    egui::Rect::from_center_size(center, egui::vec2(22.0, 22.0)),
                );
            }
        });

        // Accept drag-and-dropped image files.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_image_from_path(&path);
        }
    }

    /// Called once when the window is closing. If an export is still
    /// in-flight, set the cancel flag and join the worker thread so
    /// the child process is SIGKILLed before the GUI exits. Without
    /// this the f64 CLI orphans into its own process group and keeps
    /// hammering the CPU long after the window is gone.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let Some(job) = self.export_job.take() else {
            return;
        };
        job.cancel.store(true, Ordering::SeqCst);
        if let Some(h) = job.handle {
            let _ = h.join();
        }
    }
}

/// Extract the human-readable payload from a caught panic. Panics carry
/// either a `&str` (literal messages) or a `String` (formatted ones);
/// anything else gets a generic label.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        format!("panic: {s}")
    } else if let Some(s) = panic.downcast_ref::<String>() {
        format!("panic: {s}")
    } else {
        "panic (no message)".to_string()
    }
}

/// Find the data directory. Probes, in order:
///   1. `$SPEKTRAFILM_DATA_DIR` — explicit override.
///   2. `<exe>/../Resources/data` — macOS `.app` bundle layout
///      (`Foo.app/Contents/MacOS/<exe>` → `Contents/Resources/data`).
///   3. `<exe>/data` — data shipped next to the binary.
///   4. `./data` — current working directory.
///   5. `<CARGO_MANIFEST_DIR>/../../data` — workspace root, for
///      `cargo run -p spektrafilm-gui`.
///
/// Finder launches apps with `cwd=/`, so the exe-relative probes (2–3)
/// matter for a double-clicked `.app`. Falls back to `./data` so the
/// downstream loader surfaces a clear "profiles not found" error.
fn pick_data_dir() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("SPEKTRAFILM_DATA_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("..").join("Resources").join("data"));
        candidates.push(dir.join("data"));
    }
    candidates.push(PathBuf::from("data"));
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest).join("..").join("..").join("data"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// Scan `<data_dir>/profiles/*.json`, parse each profile's `info`, and
/// bucket the results by `info.support`. Films go into the first vec,
/// papers (and any other print-stage supports) into the second.
/// Each entry carries the filename stem (the unique loader key) plus a
/// human-readable display label.
fn scan_profiles(data_dir: &Path) -> (Vec<ProfileEntry>, Vec<ProfileEntry>) {
    let mut films = Vec::new();
    let mut papers = Vec::new();
    let dir = data_dir.join("profiles");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (films, papers);
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_lossy = name.to_string_lossy().to_string();
        let Some(stem) = name_lossy.strip_suffix(".json") else {
            continue;
        };
        let stock = stem.to_string();
        // Cheap probe: just deserialize the file's `info` field. We
        // could skip the rest of the profile but `Profile` already does
        // the right thing — and we pay this once at startup.
        let (display, is_paper, is_bw) = match profile::load_profile_by_name(data_dir, &stock) {
            Ok(p) => {
                let display = p.info.name.clone().unwrap_or_else(|| stock.clone());
                let is_paper = p.info.support == "paper" || p.info.stage == "printing";
                (display, is_paper, p.is_bw())
            }
            Err(err) => {
                tracing::warn!(%stock, %err, "could not load profile for catalog");
                (stock.clone(), false, false)
            }
        };
        let entry = ProfileEntry {
            stock,
            display,
            is_bw,
        };
        if is_paper {
            papers.push(entry);
        } else {
            films.push(entry);
        }
    }
    films.sort_by(|a, b| a.display.cmp(&b.display));
    papers.sort_by(|a, b| a.display.cmp(&b.display));
    (films, papers)
}

/// Combo box that picks one of `entries` by its `stock` id (the
/// underlying file stem) while showing `display` (the human-readable
/// name) as the label. Falls back to showing the raw stock id if no
/// entry with the current `selected_stock` exists.
/// Development-time family of a profile (empty for colour stocks or on
/// load failure — the picker hides itself for ≤1 entries either way).
fn profile_dev_times(data_dir: &Path, stock: &str) -> Vec<f64> {
    profile::load_profile_by_name(data_dir, stock)
        .map(|p| p.data.development_time)
        .unwrap_or_default()
}

/// Development-time picker for a B&W development-time family. `selection`
/// of `None` means the profile's default (the floor-middle entry, matching
/// upstream's `select_development_time`). Returns true when changed.
fn dev_time_combo(
    ui: &mut egui::Ui,
    salt: &str,
    label: &str,
    times: &[f64],
    selection: &mut Option<f64>,
) -> bool {
    // Same resolution as the render path, so the combo always highlights
    // exactly the entry the pipeline will use.
    let current_idx = profile::development_time_index(times, *selection);
    let mut changed = false;
    ui.label(label);
    egui::ComboBox::from_id_salt(salt)
        .selected_text(format!("{} min", times[current_idx]))
        .width(ui.available_width().min(280.0))
        .show_ui(ui, |ui| {
            for (i, t) in times.iter().enumerate() {
                if ui
                    .selectable_label(i == current_idx, format!("{t} min"))
                    .clicked()
                    && i != current_idx
                {
                    *selection = Some(*t);
                    changed = true;
                }
            }
        });
    changed
}

fn profile_combo(
    ui: &mut egui::Ui,
    salt: &str,
    label: &str,
    entries: &[ProfileEntry],
    selected_stock: &mut String,
) -> bool {
    ui.label(label);
    let display = entries
        .iter()
        .find(|e| &e.stock == selected_stock)
        .map(|e| e.display.clone())
        .unwrap_or_else(|| selected_stock.clone());
    let prev = selected_stock.clone();
    egui::ComboBox::from_id_salt(salt)
        .selected_text(&display)
        .width(ui.available_width().min(280.0))
        .show_ui(ui, |ui| {
            for (is_bw, heading) in [(true, "Black & white"), (false, "Colour")] {
                if entries.iter().any(|entry| entry.is_bw == is_bw) {
                    ui.strong(heading);
                    for entry in entries.iter().filter(|entry| entry.is_bw == is_bw) {
                        ui.selectable_value(selected_stock, entry.stock.clone(), &entry.display);
                    }
                }
            }
        });
    prev != *selected_stock
}

fn fps_label(ms: f32) -> &'static str {
    if ms < 16.0 {
        "60 fps"
    } else if ms < 33.0 {
        "30 fps"
    } else if ms < 67.0 {
        "15 fps"
    } else if ms < 200.0 {
        "5 fps"
    } else if ms < 400.0 {
        "2 fps"
    } else {
        ""
    }
}

fn preview_pipeline_cache_key(film_name: &str, print_name: &str, params: &RuntimeParams) -> String {
    serde_json::json!({
        "film": film_name,
        "print": print_name,
        "film_dev": params.film_render.development_time,
        "print_dev": params.print_render.development_time,
        "scan_film": params.io.scan_film,
        "input_color_space": params.io.input_color_space,
        "input_cctf_decoding": params.io.input_cctf_decoding,
        "input_gamut": params.io.input_gamut_compress,
        "output_color_space": params.io.output_color_space,
        "output_gamut": params.io.output_gamut_compress,
        "settings": {
            "rgb_to_raw_method": params.settings.rgb_to_raw_method,
            "apply_hanatos2025_adaptation_window": params.settings.apply_hanatos2025_adaptation_window,
            "apply_hanatos2025_adaptation_surface": params.settings.apply_hanatos2025_adaptation_surface,
            "spectral_gaussian_blur": params.settings.spectral_gaussian_blur,
            "lut_resolution": params.settings.lut_resolution,
            "neutral_print_filters_from_database": params.settings.neutral_print_filters_from_database,
            "use_cat16": params.settings.use_cat16,
        },
        "enlarger": {
            "illuminant": params.enlarger.illuminant,
            "c_filter_neutral": params.enlarger.c_filter_neutral,
            "m_filter_neutral": params.enlarger.m_filter_neutral,
            "y_filter_neutral": params.enlarger.y_filter_neutral,
            "m_filter_shift": params.enlarger.m_filter_shift,
            "y_filter_shift": params.enlarger.y_filter_shift,
            "preflash_exposure": params.enlarger.preflash_exposure,
            "preflash_m_filter_shift": params.enlarger.preflash_m_filter_shift,
            "preflash_y_filter_shift": params.enlarger.preflash_y_filter_shift,
            "normalize_print_exposure": params.enlarger.normalize_print_exposure,
            "print_exposure_compensation": params.enlarger.print_exposure_compensation,
        },
        "exposure_compensation_ev": if params.enlarger.print_exposure_compensation {
            params.camera.exposure_compensation_ev
        } else {
            0.0
        },
    })
    .to_string()
}

const PREVIEW_PIXEL_LIMIT: u64 = 12_000_000;

fn preview_dimensions(width: u32, height: u32, factor: f32, limited: bool) -> (u32, u32) {
    // Preserve the existing working-resolution behavior, then impose the
    // preview-only cap. A smaller working image must never be enlarged.
    let (width, height) = if factor <= 0.0 || (factor - 1.0).abs() < 1e-6 {
        (width, height)
    } else {
        (
            ((width as f32 * factor).round() as u32).max(1),
            ((height as f32 * factor).round() as u32).max(1),
        )
    };
    let pixels = u64::from(width) * u64::from(height);
    if !limited || pixels <= PREVIEW_PIXEL_LIMIT {
        return (width, height);
    }
    let scale = (PREVIEW_PIXEL_LIMIT as f64 / pixels as f64).sqrt();
    // Floor so rounding cannot exceed the cap. Bound each axis for the
    // degenerate case of a one-pixel-high or one-pixel-wide panorama.
    (
        ((width as f64 * scale).floor() as u32).clamp(1, PREVIEW_PIXEL_LIMIT as u32),
        ((height as f64 * scale).floor() as u32).clamp(1, PREVIEW_PIXEL_LIMIT as u32),
    )
}

fn rescale_preview_input_fast(image: &ImageBuf, (new_w, new_h): (u32, u32)) -> ImageBuf {
    // The normal f32 GUI can resize directly from the retained source,
    // avoiding another full-resolution RGB allocation just to downsample.
    #[cfg(not(feature = "precision-f64"))]
    let f32buf = image.data.as_slice();
    #[cfg(feature = "precision-f64")]
    let f32buf: Vec<f32> = image.data.par_iter().map(|&v| to_f32(v)).collect();
    let src = image::ImageBuffer::<image::Rgb<f32>, _>::from_raw(image.width, image.height, f32buf)
        .expect("ImageBuf dims match its data length");
    let dst = image::imageops::resize(&src, new_w, new_h, image::imageops::FilterType::CatmullRom);
    let data: Vec<Scalar> = dst.into_raw().into_par_iter().map(from_f32).collect();
    ImageBuf::from_data(new_w, new_h, data)
}

fn pick_default_stock(entries: &[ProfileEntry], preferred: &str) -> String {
    if entries.iter().any(|e| e.stock == preferred) {
        preferred.to_string()
    } else {
        entries
            .first()
            .map(|e| e.stock.clone())
            .unwrap_or_else(|| preferred.to_string())
    }
}

/// Convert the pipeline's post-sRGB-encoded RGB ImageBuf into an RGBA preview.
/// The full output remains available for saving, but the interactive texture is
/// capped so a large RAW does not force a full-size upload into egui every pass.
fn make_preview_texture_data(out: &ImageBuf) -> PreviewTextureData {
    let src_w = out.width as usize;
    let src_h = out.height as usize;
    let max_src_dim = src_w.max(src_h).max(1);
    let scale_num = PREVIEW_TEXTURE_MAX_DIM.min(max_src_dim);
    let w = ((src_w * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let h = ((src_h * scale_num + max_src_dim / 2) / max_src_dim).max(1);
    let mut rgba = vec![0u8; w * h * 4];
    rgba.par_chunks_exact_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let gy = if h == src_h {
                y as f32
            } else {
                (y as f32 + 0.5) * (src_h as f32 / h as f32) - 0.5
            };
            let y0 = gy.floor().clamp(0.0, (src_h - 1) as f32) as usize;
            let y1 = (y0 + 1).min(src_h - 1);
            let ty = (gy - y0 as f32).clamp(0.0, 1.0);

            for x in 0..w {
                let gx = if w == src_w {
                    x as f32
                } else {
                    (x as f32 + 0.5) * (src_w as f32 / w as f32) - 0.5
                };
                let x0 = gx.floor().clamp(0.0, (src_w - 1) as f32) as usize;
                let x1 = (x0 + 1).min(src_w - 1);
                let tx = (gx - x0 as f32).clamp(0.0, 1.0);
                let p00 = (y0 * src_w + x0) * 3;
                let p10 = (y0 * src_w + x1) * 3;
                let p01 = (y1 * src_w + x0) * 3;
                let p11 = (y1 * src_w + x1) * 3;
                let dst = x * 4;

                for c in 0..3 {
                    let v00 = to_f32(out.data[p00 + c]);
                    let v10 = to_f32(out.data[p10 + c]);
                    let v01 = to_f32(out.data[p01 + c]);
                    let v11 = to_f32(out.data[p11 + c]);
                    let top = v00 + (v10 - v00) * tx;
                    let bot = v01 + (v11 - v01) * tx;
                    row[dst + c] = ((top + (bot - top) * ty).clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                row[dst + 3] = 255;
            }
        });
    PreviewTextureData { size: [w, h], rgba }
}

/// Textured quad filling `rect`, with the texture rotated by
/// `quarter_turns_cw` × 90° clockwise. `rect` must already have the
/// rotated aspect ratio. Rotating via UVs keeps the turn instant.
fn rotated_image_mesh(
    tex: egui::TextureId,
    rect: egui::Rect,
    quarter_turns_cw: i32,
) -> egui::Shape {
    // Texture corners in screen order TL, TR, BR, BL.
    let uvs = [
        egui::pos2(0.0, 0.0),
        egui::pos2(1.0, 0.0),
        egui::pos2(1.0, 1.0),
        egui::pos2(0.0, 1.0),
    ];
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let k = quarter_turns_cw.rem_euclid(4) as usize;
    let mut mesh = egui::Mesh::with_texture(tex);
    for (i, &pos) in corners.iter().enumerate() {
        // After a CW turn, the screen's top-left shows the texture's
        // bottom-left, i.e. the UV one step "behind" in corner order.
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uvs[(i + 4 - k) % 4],
            color: egui::Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

/// Upload a prepared preview texture. The pipeline emits values already
/// clamped to [0, 1] and sRGB-encoded when `output_cctf_encoding` is on.
fn make_texture(ctx: &egui::Context, preview: PreviewTextureData) -> egui::TextureHandle {
    let color_image = egui::ColorImage::from_rgba_unmultiplied(preview.size, &preview.rgba);
    ctx.load_texture(
        "spektrafilm-output",
        color_image,
        egui::TextureOptions::LINEAR,
    )
}

/// Locate the f64-built `spektrafilm` CLI binary. Search order:
///   1. `$SPEKTRAFILM_F64_CLI` — explicit override, full path.
///   2. `spektrafilm-f64` next to the running GUI executable (release
///      builds: both binaries live in `target/release/`).
///   3. `spektrafilm-f64` on `$PATH`.
///   4. `target/release/spektrafilm-f64` relative to `CARGO_MANIFEST_DIR`
///      (handy when running via `cargo run`).
///
/// Returns a path that exists and is executable, or an explanatory
/// error pointing to the build command.
fn locate_f64_cli() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("SPEKTRAFILM_F64_CLI") {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "SPEKTRAFILM_F64_CLI points at {p} but no such file"
        ));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in f64_cli_names() {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    for name in f64_cli_names() {
        if let Some(p) = which_on_path(name) {
            return Ok(p);
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        for name in f64_cli_names() {
            let p = PathBuf::from(&manifest)
                .join("..")
                .join("..")
                .join("target")
                .join("release")
                .join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    Err("no f64 CLI found. Build it with \
         `cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64`, \
         and either put it on PATH or set $SPEKTRAFILM_F64_CLI."
        .into())
}

fn f64_cli_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["spektrafilm-f64.exe", "spektrafilm-f64"]
    } else {
        &["spektrafilm-f64"]
    }
}

/// Minimal PATH lookup so we don't pull in the `which` crate for one call.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// RAII guard so the params tempfile is unlinked on every code path
/// (early `?` return, panic during `Command::output`, normal exit).
/// Without this, a failed `serde_json::to_writer` or a panic mid-spawn
/// would leak user-state JSON into the OS temp dir.
struct TempPath(PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// JSON readers in older companions ignore unknown fields. Explicit flags
/// ensure a stale CLI rejects RAW WB rather than silently exporting As shot.
fn append_raw_wb_args(
    cmd: &mut std::process::Command,
    input: &Path,
    wb: spektrafilm_io::RawWhiteBalance,
) {
    if spektrafilm_io::is_raw(input) {
        cmd.arg("--raw-wb").arg(wb.mode.as_str());
        if wb.mode == spektrafilm_io::RawWhiteBalanceMode::Custom {
            cmd.arg("--raw-temperature")
                .arg(wb.temperature.to_string())
                .arg("--raw-tint")
                .arg(wb.tint.to_string());
        }
    }
}

/// Run the f64 CLI as a child process. Writes the current
/// `RuntimeParams` to a temp JSON file, invokes
/// `spektrafilm-f64 process …`, polls for completion while watching
/// `cancel`, and surfaces the CLI's stderr verbatim on failure. If
/// `cancel` is set the child is SIGKILLed and `Err("cancelled")` is
/// returned — this prevents the orphan-process / system-freeze bug
/// where closing the GUI mid-export left the CPU pipeline running.
fn run_f64_export(
    cli_path: &Path,
    input: &Path,
    output: &Path,
    film: &str,
    paper: &str,
    params: &spektrafilm_core::params::RuntimeParams,
    data_dir: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp = TempPath(std::env::temp_dir().join(format!(
        "spektrafilm-export-{}-{nanos}.json",
        std::process::id()
    )));
    {
        let f = std::fs::File::create(&temp.0)
            .with_context(|| format!("creating params tempfile {}", temp.0.display()))?;
        serde_json::to_writer(std::io::BufWriter::new(f), params)
            .context("serializing params to JSON")?;
    }

    let mut cmd = std::process::Command::new(cli_path);
    // Force the CPU backend: the wgpu shaders run f32 even in a
    // precision-f64 build (WGSL has no f64). Letting the child default
    // to GPU would silently demote the export back to f32 math.
    cmd.env("SPEKTRAFILM_BACKEND", "cpu")
        .arg("process")
        .arg(input)
        .arg("-o")
        .arg(output)
        .arg("--film")
        .arg(film)
        .arg("--paper")
        .arg(paper)
        .arg("--params")
        .arg(&temp.0)
        .arg("--data-dir")
        .arg(data_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if params.io.scan_film {
        cmd.arg("--scan-film");
    }
    append_raw_wb_args(&mut cmd, input, params.io.raw_white_balance);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", cli_path.display()))?;

    // Poll loop. 100 ms is responsive to a Cancel click without
    // burning CPU while the export grinds through CPU f64 math.
    let status = loop {
        if let Some(s) = child.try_wait().context("waiting for f64 CLI")? {
            break s;
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("cancelled");
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // The child has exited; piped buffers are drained safely.
    let mut stderr_buf = String::new();
    if let Some(mut s) = child.stderr.take() {
        use std::io::Read;
        let _ = s.read_to_string(&mut stderr_buf);
    }

    if !status.success() {
        let trimmed = stderr_buf.trim();
        if trimmed.contains("unexpected argument") && trimmed.contains("--raw-wb") {
            anyhow::bail!(
                "Export companion {} is too old to apply RAW white balance. Rebuild it with \
                 `cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64` \
                 and update that executable.\n{trimmed}",
                cli_path.display()
            );
        }
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "(signal)".into());
        anyhow::bail!(
            "f64 CLI exited {code} — {}",
            if trimmed.is_empty() {
                "(no stderr)"
            } else {
                trimmed
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod raw_wb_tests {
    use super::*;
    use spektrafilm_io::{RawWhiteBalance, RawWhiteBalanceMode};

    fn app() -> App {
        let mut app = App::new(Arc::new(spektrafilm_gpu::cpu_backend::CpuBackend), None);
        app.image_path = Some(PathBuf::from("test.NEF"));
        app.image = Some(Arc::new(ImageBuf::from_data(1, 1, vec![from_f32(0.1); 3])));
        app
    }

    fn completed_job(app: &mut App, revision: u64, result: Result<RenderResult, String>) {
        let (tx, rx) = mpsc::channel();
        assert!(tx.send(result).is_ok());
        // A dispatched render has consumed the dirty state.
        app.dirty = false;
        app.dirty_since = None;
        app.render_job = Some(RenderJob {
            rx,
            handle: None,
            input_revision: revision,
            raw_wb: app.params.io.raw_white_balance,
        });
    }

    fn result() -> RenderResult {
        let output = ImageBuf::from_data(1, 1, vec![from_f32(0.25); 3]);
        RenderResult {
            input: Arc::new(output.clone()),
            preview: make_preview_texture_data(&output),
            output,
            input_clone_ms: 0.0,
            scale_ms: 0.0,
            pipeline_build_ms: 0.0,
            render_ms: 0.0,
            preview_ms: 0.0,
            worker_total_ms: 0.0,
        }
    }

    #[test]
    fn wb_edit_discards_old_worker_input_and_output() {
        let mut app = app();
        completed_job(&mut app, 0, Ok(result()));
        app.params.io.raw_white_balance.mode = RawWhiteBalanceMode::Daylight;
        app.invalidate_input_preview();
        assert!(app.raw_wb_pending());
        app.poll_render_job(&egui::Context::default());
        assert!(app.output_image.is_none());
        assert!(app.output_tex.is_none());
        assert_eq!(app.image.as_ref().unwrap().data, vec![from_f32(0.1); 3]);
        assert_eq!(app.loaded_raw_wb, RawWhiteBalance::default());
        assert!(app.dirty);
    }

    #[test]
    fn exposure_edit_discards_intermediate_render_and_restarts_debounce() {
        let mut app = app();
        app.output_image = Some(ImageBuf::from_data(1, 1, vec![from_f32(0.1); 3]));
        completed_job(&mut app, 0, Ok(result()));
        app.params.enlarger.print_exposure = 0.8;
        app.mark_dirty();
        let first_edit = app.dirty_since.unwrap();
        app.dirty_since = Some(first_edit - PREVIEW_DEBOUNCE);
        app.params.enlarger.print_exposure = 0.75;
        app.mark_dirty();
        assert!(app.dirty_since.unwrap() > first_edit - PREVIEW_DEBOUNCE);

        app.poll_render_job(&egui::Context::default());

        assert_eq!(
            app.output_image.as_ref().unwrap().data,
            vec![from_f32(0.1); 3]
        );
        assert!(app.dirty);
        assert!(app.render_job.is_none());
    }

    #[test]
    fn queued_edit_discards_completed_render_without_replacing_preview() {
        let mut app = app();
        app.output_image = Some(ImageBuf::from_data(1, 1, vec![from_f32(0.1); 3]));
        completed_job(&mut app, 0, Ok(result()));
        app.pending_dirty = true;

        app.poll_render_job(&egui::Context::default());

        assert_eq!(
            app.output_image.as_ref().unwrap().data,
            vec![from_f32(0.1); 3]
        );
        assert!(app.dirty);
        assert!(!app.pending_dirty);
    }

    #[test]
    fn preview_limit_edit_discards_in_flight_result_and_preserves_export_params() {
        let mut app = app();
        assert!(!app.limit_preview_12mp);
        let export_params = serde_json::to_value(&app.params).unwrap();
        let source = app.image.as_ref().unwrap().clone();
        completed_job(&mut app, 0, Ok(result()));
        app.limit_preview_12mp = true;
        app.mark_dirty();
        app.poll_render_job(&egui::Context::default());
        assert!(app.output_image.is_none());
        assert!(app.dirty);
        assert!(Arc::ptr_eq(app.image.as_ref().unwrap(), &source));
        assert_eq!(serde_json::to_value(&app.params).unwrap(), export_params);
        app.reset_settings();
        assert!(!app.limit_preview_12mp);
    }

    #[test]
    fn successful_reload_updates_input_and_export_settings_together() {
        let mut app = app();
        app.params.io.raw_white_balance = RawWhiteBalance {
            mode: RawWhiteBalanceMode::Custom,
            temperature: 3200.0,
            tint: 0.85,
        };
        app.params.io.rotate_cw = 1;
        app.params.scanner.white_correction = true;
        app.invalidate_input_preview();
        completed_job(&mut app, 1, Ok(result()));
        app.poll_render_job(&egui::Context::default());
        assert!(!app.raw_wb_pending());
        assert_eq!(app.image.as_ref().unwrap().data, vec![from_f32(0.25); 3]);
        // The same RuntimeParams serialization is used by run_f64_export.
        let export: RuntimeParams =
            serde_json::from_str(&serde_json::to_string(&app.params).unwrap()).unwrap();
        assert_eq!(export.io.raw_white_balance, app.loaded_raw_wb);
        assert_eq!(export.io.rotate_cw, 1);
        assert!(export.scanner.white_correction);
        assert!(app.output_image.is_some());
    }

    #[test]
    fn failed_reload_restores_settings_for_retained_input() {
        let mut app = app();
        app.params.io.raw_white_balance.mode = RawWhiteBalanceMode::Daylight;
        app.invalidate_input_preview();
        completed_job(&mut app, 1, Err("missing matrix".into()));
        app.poll_render_job(&egui::Context::default());
        assert!(!app.raw_wb_pending());
        assert_eq!(app.params.io.raw_white_balance, RawWhiteBalance::default());
        assert_eq!(app.image.as_ref().unwrap().data, vec![from_f32(0.1); 3]);
        assert!(app.status.contains("missing matrix"));
        assert!(app.dirty); // regenerate the retained image's preview
    }

    #[test]
    fn export_command_carries_current_wb_with_custom_values_only_when_active() {
        for mode in RawWhiteBalanceMode::ALL {
            let wb = RawWhiteBalance {
                mode,
                temperature: 4321.5,
                tint: 0.875,
            };
            let mut cmd = std::process::Command::new("spektrafilm-f64");
            append_raw_wb_args(&mut cmd, Path::new("photo.NEF"), wb);
            let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
            let serialized = serde_json::to_value(wb).unwrap();
            assert_eq!(args[0], "--raw-wb");
            assert_eq!(args[1], serialized["mode"].as_str().unwrap());
            if mode == RawWhiteBalanceMode::Custom {
                assert_eq!(
                    args[2..],
                    ["--raw-temperature", "4321.5", "--raw-tint", "0.875"]
                );
            } else {
                assert_eq!(args.len(), 2);
            }
            let mut non_raw = std::process::Command::new("spektrafilm-f64");
            append_raw_wb_args(&mut non_raw, Path::new("photo.png"), wb);
            assert_eq!(non_raw.get_args().count(), 0);
        }
    }

    #[test]
    #[cfg(unix)]
    fn legacy_export_companion_cannot_silently_ignore_raw_wb() {
        use std::os::unix::fs::PermissionsExt;
        let legacy_cli = TempPath(std::env::temp_dir().join(format!(
            "spektrafilm-legacy-wb-test-{}.sh",
            std::process::id()
        )));
        // Emulate the old CLI: JSON's new fields would be silently ignored,
        // whereas unknown CLI flags fail during argument parsing.
        std::fs::write(
            &legacy_cli.0,
            br#"#!/bin/sh
for arg do
    if [ "$arg" = "--raw-wb" ]; then
        printf '%s\n' "error: unexpected argument '--raw-wb' found" >&2
        exit 2
    fi
done
exit 0
"#,
        )
        .unwrap();
        std::fs::set_permissions(&legacy_cli.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut params = RuntimeParams::default();
        params.io.raw_white_balance = RawWhiteBalance {
            mode: RawWhiteBalanceMode::Custom,
            temperature: 7200.0,
            tint: 1.1,
        };
        let error = run_f64_export(
            &legacy_cli.0,
            Path::new("photo.NEF"),
            Path::new("unused.png"),
            "kodak_gold_200",
            "kodak_portra_endura",
            &params,
            Path::new("data"),
            &AtomicBool::new(false),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("too old to apply RAW white balance"),
            "{error}"
        );
        assert!(error.contains(legacy_cli.0.to_str().unwrap()));
        assert!(error.contains("--bin spektrafilm-f64"));
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use spektrafilm_io::RawWhiteBalanceMode;

    #[test]
    fn gui_catalog_includes_bw_camera_and_print_films_in_the_correct_pickers() {
        let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let (films, prints) = scan_profiles(&data_dir);
        let bw_films: Vec<_> = films
            .iter()
            .filter(|p| p.is_bw)
            .map(|p| p.stock.as_str())
            .collect();
        let bw_prints: Vec<_> = prints
            .iter()
            .filter(|p| p.is_bw)
            .map(|p| p.stock.as_str())
            .collect();
        assert_eq!(bw_films, ["kodak_doublex", "kodak_trix"]);
        assert_eq!(bw_prints, ["kodak_2302"]);
        assert_eq!(films.len() + prints.len(), 31);
        assert_eq!(
            films
                .iter()
                .find(|p| p.stock == "kodak_doublex")
                .unwrap()
                .display,
            "Kodak Double-X Negative Film 5222"
        );
        assert_eq!(
            prints
                .iter()
                .find(|p| p.stock == "kodak_2302")
                .unwrap()
                .display,
            "Kodak Print Film 2302"
        );
    }

    #[test]
    fn reset_restores_all_params_but_keeps_selected_profiles_and_slide_defaults() {
        let mut app = App::new(Arc::new(spektrafilm_gpu::cpu_backend::CpuBackend), None);
        app.film_name = "kodak_ektachrome_100".into();
        app.print_name = "kodak_2302".into();
        app.image_path = Some(PathBuf::from("photo.png"));
        app.params.camera.exposure_compensation_ev = 2.0;
        app.params.film_render.development_time = Some(10.0);
        app.params.print_render.development_time = Some(5.0);
        app.params.enlarger.print_exposure = 3.0;
        app.params.scanner.white_correction = true;
        app.params.settings.use_cat16 = false;
        app.params.settings.serial_grain = true;
        app.params.io.output_cctf_encoding = false;
        app.params.io.scan_film = false;
        app.params.io.rotate_cw = 2;

        app.reset_settings();

        let mut expected = RuntimeParams::default();
        expected.io.input_color_space = "sRGB".into();
        expected.io.scan_film = true;
        assert_eq!(
            serde_json::to_value(&app.params).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(app.film_name, "kodak_ektachrome_100");
        assert_eq!(app.print_name, "kodak_2302");
        assert!(app.dirty);
    }

    #[test]
    fn reset_raw_white_balance_invalidates_previously_developed_input() {
        let mut app = App::new(Arc::new(spektrafilm_gpu::cpu_backend::CpuBackend), None);
        app.image_path = Some(PathBuf::from("photo.NEF"));
        app.image = Some(Arc::new(ImageBuf::from_data(1, 1, vec![from_f32(0.1); 3])));
        app.output_image = Some(ImageBuf::from_data(1, 1, vec![from_f32(0.25); 3]));
        app.loaded_raw_wb.mode = RawWhiteBalanceMode::Daylight;
        app.params.io.raw_white_balance.mode = RawWhiteBalanceMode::Daylight;

        app.reset_settings();

        assert_eq!(app.params.io.raw_white_balance, Default::default());
        assert!(!app.params.io.scan_film);
        assert!(app.raw_wb_pending());
        assert_eq!(app.input_revision, 1);
        assert!(app.output_image.is_none());
        assert!(app.dirty);
    }
}

/// macOS only: walk from the eframe `RawWindowHandle` down to the
/// `CAMetalLayer` and tag its colorspace as sRGB. Without this the
/// metal layer's `colorspace` is null and macOS treats the framebuffer
/// pixels as raw display primaries — sRGB content rendered into a
/// wide-gamut panel comes out oversaturated. Setting the layer's
/// colorspace makes the OS gamut-map exactly like it does for a
/// regular sRGB-tagged PNG opened in Preview, so what you see in the
/// GUI matches what you export.
#[cfg(target_os = "macos")]
fn tag_metal_layer_srgb<H: raw_window_handle::HasWindowHandle>(h: &H) -> Result<(), String> {
    use core_graphics::color_space::{CGColorSpace, kCGColorSpaceSRGB};
    use foreign_types::ForeignType;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use raw_window_handle::RawWindowHandle;

    let handle = h
        .window_handle()
        .map_err(|e| format!("no window handle: {e}"))?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let ns_view: *mut AnyObject = appkit.ns_view.as_ptr().cast();
    if ns_view.is_null() {
        return Err("ns_view is null".into());
    }

    // `kCGColorSpaceSRGB` is an extern static CFStringRef, accessing it
    // is unsafe in the 2024 edition's stricter model.
    let srgb_cs = unsafe { CGColorSpace::create_with_name(kCGColorSpaceSRGB) }
        .ok_or_else(|| "CGColorSpaceCreateWithName(sRGB) returned null".to_string())?;
    // `foreign_types::ForeignType::as_ptr` returns the opaque
    // `CGColorSpaceRef` (i.e. `*mut sys::CGColorSpace`). `setColorspace:`
    // is declared as taking a `CGColorSpaceRef`, whose ObjC type encoding
    // is `^{CGColorSpace=}`. A bare `*mut c_void` encodes as `^v`, which
    // objc2's debug-build argument-encoding verification rejects (the
    // release build skips that check, which is why only debug crashed).
    // Cast through a zero-field opaque struct whose `Encode` matches the
    // expected `^{CGColorSpace=}` so both builds pass the same pointer.
    #[repr(C)]
    struct CGColorSpaceOpaque {
        _private: [u8; 0],
    }
    // A pointer to this type encodes as `^{CGColorSpace=}` — what
    // `setColorspace:` expects. `RefEncode` (not `Encode`) is the trait
    // objc2 consults for a `*const T` argument.
    unsafe impl objc2::RefEncode for CGColorSpaceOpaque {
        const ENCODING_REF: objc2::Encoding =
            objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGColorSpace", &[]));
    }
    let cs_ref = srgb_cs.as_ptr() as *const CGColorSpaceOpaque;

    // Only `CAMetalLayer` has a `colorspace` property; calling
    // `setColorspace:` on a plain `CALayer` is an unrecognized selector
    // (crash), so we always gate on `respondsToSelector:`.
    let selector = objc2::sel!(setColorspace:);
    unsafe {
        let root_layer: *mut AnyObject = msg_send![ns_view, layer];
        if root_layer.is_null() {
            return Err("ns_view.layer is null".into());
        }
        // Find the CAMetalLayer. wgpu-hal 24 (the raw-window-metal
        // approach) does NOT replace the view's layer: when the root
        // layer isn't already a CAMetalLayer — the default for an
        // eframe/winit NSView — it installs the metal layer as a
        // *sublayer*. So the root layer is a plain CALayer with no
        // `colorspace`; the layer we must tag is the metal sublayer.
        // (Older eframe set the view's own layer to the CAMetalLayer, so
        // check the root first and fall back to scanning sublayers.)
        let metal_layer = if msg_send![root_layer, respondsToSelector: selector] {
            root_layer
        } else {
            let sublayers: *mut AnyObject = msg_send![root_layer, sublayers];
            let mut found: *mut AnyObject = std::ptr::null_mut();
            if !sublayers.is_null() {
                let count: usize = msg_send![sublayers, count];
                for i in 0..count {
                    let sub: *mut AnyObject = msg_send![sublayers, objectAtIndex: i];
                    if !sub.is_null() && msg_send![sub, respondsToSelector: selector] {
                        found = sub;
                        break;
                    }
                }
            }
            found
        };
        if metal_layer.is_null() {
            // The metal sublayer may not exist yet on the first frame;
            // the caller retries until this succeeds.
            return Err("no CAMetalLayer on the view or its sublayers yet".into());
        }
        // CAMetalLayer.colorspace is `retain`-strong, so ObjC takes its
        // own reference; we can let `srgb_cs` drop after the call.
        let _: () = msg_send![metal_layer, setColorspace: cs_ref];
        tracing::info!("tagged CAMetalLayer.colorspace = sRGB");
    }
    Ok(())
}

#[cfg(test)]
mod preview_resolution_tests {
    use super::*;

    #[test]
    fn cap_preserves_aspect_ratio_and_never_exceeds_twelve_megapixels() {
        for (w, h) in [(9520, 6336), (6048, 4024), (6336, 9520), (8000, 2000)] {
            let (pw, ph) = preview_dimensions(w, h, 1.0, true);
            let pixels = u64::from(pw) * u64::from(ph);
            assert!(pixels <= PREVIEW_PIXEL_LIMIT);
            assert!(pixels > PREVIEW_PIXEL_LIMIT - 20_000);
            assert!((f64::from(pw) / f64::from(ph) - f64::from(w) / f64::from(h)).abs() < 0.002);
            assert_eq!(preview_dimensions(w, h, 1.0, false), (w, h));
        }
        for dimensions in [(1, u32::MAX), (u32::MAX, 1), (u32::MAX, u32::MAX)] {
            let (w, h) = preview_dimensions(dimensions.0, dimensions.1, 1.0, true);
            assert!(w > 0 && h > 0);
            assert!(u64::from(w) * u64::from(h) <= PREVIEW_PIXEL_LIMIT);
        }
    }

    #[test]
    fn cap_respects_smaller_inputs_and_working_resolution() {
        for dimensions in [(1, 1), (3000, 2000), (4000, 3000)] {
            assert_eq!(
                preview_dimensions(dimensions.0, dimensions.1, 1.0, true),
                dimensions
            );
        }
        assert_eq!(preview_dimensions(9520, 6336, 0.25, true), (2380, 1584));
        assert_eq!(preview_dimensions(9520, 6336, 0.5, false), (4760, 3168));
        let (w, h) = preview_dimensions(9520, 6336, 0.5, true);
        assert!(u64::from(w) * u64::from(h) <= PREVIEW_PIXEL_LIMIT);
    }

    #[test]
    fn resampling_leaves_retained_source_intact() {
        let source = ImageBuf::from_data(12, 8, vec![from_f32(0.25); 12 * 8 * 3]);
        let scaled = rescale_preview_input_fast(&source, (6, 4));
        assert_eq!((scaled.width, scaled.height), (6, 4));
        assert_eq!(scaled.data.len(), 6 * 4 * 3);
        assert!(scaled.data.iter().all(|&v| (to_f32(v) - 0.25).abs() < 1e-6));
        assert_eq!((source.width, source.height), (12, 8));
        assert_eq!(source.data, vec![from_f32(0.25); 12 * 8 * 3]);
    }
}

#[cfg(test)]
mod serial_grain_tests {
    use super::*;

    #[test]
    fn serial_grain_is_opt_in_and_does_not_change_preview_params() {
        let old: RuntimeParams = serde_json::from_str(r#"{"settings":{}}"#).unwrap();
        assert!(!old.settings.serial_grain);
        let mut app = App::new(Arc::new(spektrafilm_gpu::cpu_backend::CpuBackend), None);
        assert!(!app.params.settings.serial_grain);
        let before = serde_json::to_value(app.preview_params()).unwrap();
        app.params.settings.serial_grain = true;
        assert_eq!(serde_json::to_value(app.preview_params()).unwrap(), before);
        assert!(app.params.settings.serial_grain);
        assert_eq!(app.params.io.upscale_factor, 1.0);
    }

    #[test]
    #[cfg(unix)]
    fn export_child_receives_serial_grain_choice_in_params() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir();
        let cli = TempPath(dir.join(format!(
            "spektrafilm-export-params-{}.sh",
            std::process::id()
        )));
        let captured = TempPath(dir.join(format!(
            "spektrafilm-export-params-{}.json",
            std::process::id()
        )));
        // Capture exactly the JSON the GUI sends to its subprocess.
        std::fs::write(
            &cli.0,
            br#"#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        --params) params="$2"; shift ;;
        -o) output="$2"; shift ;;
    esac
    shift
done
cp "$params" "$output"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&cli.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        for serial in [false, true] {
            let mut params = RuntimeParams::default();
            params.settings.serial_grain = serial;
            run_f64_export(
                &cli.0,
                Path::new("photo.tiff"),
                &captured.0,
                "kodak_gold_200",
                "kodak_portra_endura",
                &params,
                Path::new("data"),
                &AtomicBool::new(false),
            )
            .unwrap();
            let received: RuntimeParams =
                serde_json::from_reader(std::fs::File::open(&captured.0).unwrap()).unwrap();
            assert_eq!(received.settings.serial_grain, serial);
            assert_eq!(received.io.upscale_factor, 1.0);
        }
    }
}
