//! Browser entry points for the spektrafilm engine.
//!
//! The page registers the data files a render reads (see
//! [`spektrafilm_core::data_fs`]), loads a photograph's bytes and renders it
//! to sRGB RGBA8. A render that read an unregistered file reports the paths
//! in [`Rendered::missing`]; the page fetches them and renders again.
#![cfg(target_arch = "wasm32")]

use spektrafilm_core::{data_fs, params::RuntimeParams, pipeline::Pipeline, preview, profile};
use spektrafilm_gpu::{ChainOutput, ChainResult, ComputeBackend, cpu_backend::CpuBackend, wgpu_backend::WgpuBackend};
use spektrafilm_io::encoding::Encoding;
use spektrafilm_math::image::ImageBuf;
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};
use wasm_bindgen::prelude::*;

/// Start rayon's Web Workers: `await initThreadPool(navigator.hardwareConcurrency)`
/// before the first render, which must then run off the main thread.
#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

/// Where the engine looks for its data; the page registers files under it.
const DATA_DIR: &str = "/data";

#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panic");
        let location = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        report_panic(INSTANCE.load(Ordering::Relaxed), &format!("{message}{location}"));
    }));
}

/// The page's name for this engine, which its panic reports carry: with
/// threads, the memory and so this are shared with rayon's workers.
static INSTANCE: AtomicU32 = AtomicU32::new(0);

#[wasm_bindgen(js_name = setInstance)]
pub fn set_instance(instance: u32) {
    INSTANCE.store(instance, Ordering::Relaxed);
}

// A panic aborts its thread. On a rayon worker that leaves the engine's
// thread waiting forever, so tell the page directly, which then replaces the
// engine.
#[wasm_bindgen(inline_js = "export function report_panic(instance, message) {
    try { new BroadcastChannel('negs-engine-panic').postMessage({ instance, message }); } catch {}
}")]
extern "C" {
    fn report_panic(instance: u32, message: &str);
}

/// Make a data file readable, at its path relative to the data directory
/// (`profiles/kodak_portra_400.json`).
#[wasm_bindgen(js_name = registerData)]
pub fn register_data(path: &str, bytes: Vec<u8>) {
    data_fs::register(Path::new(DATA_DIR).join(path), bytes);
}

/// The stock catalog of these profiles, as the native helper's `catalog`
/// gives it (without its protocol fields), as JSON. Their profiles must be
/// registered; `web/make-catalog.mjs` runs it at build time.
#[wasm_bindgen]
pub fn catalog(stocks: Vec<String>) -> Result<String, JsError> {
    let catalog = spektrafilm_core::catalog::catalog(Path::new(DATA_DIR), stocks).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(catalog.to_string())
}

/// The module's memory, which only grows: its high-water mark. wasm32 caps
/// it at 4 GiB.
#[wasm_bindgen(js_name = memoryBytes)]
pub fn memory_bytes() -> f64 {
    (core::arch::wasm32::memory_size(0) * 65536) as f64
}

#[wasm_bindgen(getter_with_clone)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    /// sRGB RGBA8, empty when `missing` isn't or the frame was presented.
    pub rgba: Vec<u8>,
    /// Drawn on the engine's canvas (`Engine.setCanvas`) instead.
    pub presented: bool,
    /// Data files the render read but nobody registered, relative to the data
    /// directory. Register them and render again: the result may have used
    /// fallbacks in their place.
    pub missing: Vec<String>,
    pub backend: String,
    #[wasm_bindgen(js_name = elapsedMs)]
    pub elapsed_ms: f64,
    /// Where the time went, `stage ms` pairs.
    pub stages: String,
}

#[wasm_bindgen]
pub struct Engine {
    state: Rc<RefCell<State>>,
}

struct State {
    /// WebGPU, or the CPU engine.
    backend: Rc<dyn ComputeBackend>,
    /// The backend when it's WebGPU.
    gpu: Option<Rc<WgpuBackend>>,
    /// Whether the GPU has a canvas to present on.
    canvas: bool,
    source: Option<Keyed>,
    /// The source resampled for the last two render sizes: an interactive
    /// frame and a settled one, as a slider moves and stops.
    scaled: Vec<(Scaled, Keyed)>,
    /// The last image key handed out.
    keys: u64,
    /// The full source's auto-exposure metering frame, so every render size
    /// meters alike, as the desktop apps do; made on first use.
    metering: Option<ImageBuf>,
    /// The last metered auto-exposure EV, by everything metering reads: a
    /// slider that doesn't change those skips the spectral pass.
    autoexposure: Option<(String, f64)>,
    input_colour: Option<spektrafilm_io::InputColour>,
    /// The spectral pipeline of the last render, by `preview_pipeline_cache_key`.
    pipeline: Option<(String, Pipeline)>,
}

#[wasm_bindgen]
impl Engine {
    /// The CPU engine, single-threaded unless the module was built with
    /// threads.
    #[wasm_bindgen(js_name = newCpu)]
    pub fn new_cpu() -> Engine {
        Self::with_backend(Rc::new(CpuBackend), None)
    }

    /// The WebGPU engine. Fails without WebGPU.
    #[wasm_bindgen(js_name = newGpu)]
    pub async fn new_gpu() -> Result<Engine, JsError> {
        let gpu = Rc::new(WgpuBackend::new_async().await.ok_or_else(|| JsError::new("no WebGPU adapter"))?);
        Ok(Self::with_backend(gpu.clone(), Some(gpu)))
    }

    fn with_backend(backend: Rc<dyn ComputeBackend>, gpu: Option<Rc<WgpuBackend>>) -> Engine {
        let state = State {
            backend,
            gpu,
            canvas: false,
            source: None,
            scaled: Vec::new(),
            keys: 0,
            metering: None,
            autoexposure: None,
            input_colour: None,
            pipeline: None,
        };
        Engine { state: Rc::new(RefCell::new(state)) }
    }

    /// Present renders asked to on `canvas` (the WebGPU engine only), which
    /// then reads nothing back: an `OffscreenCanvas` from the page's
    /// `transferControlToOffscreen`.
    #[wasm_bindgen(js_name = setCanvas)]
    pub fn set_canvas(&self, canvas: web_sys::OffscreenCanvas) -> Result<(), JsError> {
        let mut state = self.state.borrow_mut();
        let gpu = state.gpu.clone().ok_or_else(|| JsError::new("only the WebGPU engine presents"))?;
        gpu.set_canvas(wgpu::SurfaceTarget::OffscreenCanvas(canvas)).map_err(|e| JsError::new(&e))?;
        state.canvas = true;
        Ok(())
    }

    pub fn backend(&self) -> String {
        self.state.borrow().backend.name().to_owned()
    }

    /// Decode a PNG, JPEG or TIFF (with its embedded profile) or a camera
    /// RAW (with rawler, as shot), at most `max_dim` pixels on the longer
    /// side (0: full size), resolving to its `[width, height]`.
    pub fn load(&self, bytes: &[u8], max_dim: u32) -> Result<Vec<u32>, JsError> {
        let (image, colour) = match decode(bytes) {
            Ok((image, info, format)) => {
                let interpretation = spektrafilm_io::InputInterpretation::default();
                let png = format == image::ImageFormat::Png;
                spektrafilm_io::colour::convert(image, &info, &interpretation, png).map_err(js_error)?
            }
            Err(not_an_image) => {
                // A camera RAW the decoder knows but can't read says so;
                // anything else may have been a damaged image.
                let image = spektrafilm_io::load_raw_bytes(bytes, Default::default()).map_err(|raw| {
                    let raw = format!("{raw:#}");
                    match raw.split_once("DecoderFailed(\"") {
                        Some((_, reason)) => JsError::new(reason.split('"').next().unwrap_or(&raw)),
                        None => js_error(not_an_image.context(raw)),
                    }
                })?;
                (image, spektrafilm_io::InputColour::raw("rawler"))
            }
        };
        let long_side = image.width.max(image.height);
        let scale = if max_dim == 0 { 1.0 } else { (max_dim as f64 / long_side as f64).min(1.0) };
        let target = (
            ((image.width as f64 * scale).round() as u32).max(1),
            ((image.height as f64 * scale).round() as u32).max(1),
        );
        let image = if target == (image.width, image.height) {
            image
        } else {
            preview::downsample_area(&image, target)
        };
        let size = vec![image.width, image.height];
        let mut state = self.state.borrow_mut();
        state.source = None;
        state.scaled.clear();
        state.metering = None;
        state.autoexposure = None;
        state.source = Some(state.keyed(image));
        state.input_colour = Some(colour);
        Ok(size)
    }

    /// Develop the loaded photograph on `film`, printed on `paper` (ignored
    /// when the params scan the film), resolving to a [`Rendered`] at most
    /// `max_dim` pixels on the longer side. `params_json` is a RuntimeParams
    /// object; empty means the defaults. An `interactive` frame, drawn while
    /// a slider moves, reduces the source with a cheaper filter, as the
    /// desktop apps do. `present` draws it on the canvas, if there is one.
    pub fn render(
        &self,
        film: String,
        paper: String,
        params_json: String,
        max_dim: u32,
        interactive: bool,
        present: bool,
    ) -> js_sys::Promise {
        let state = self.state.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            render(&state, &film, &paper, &params_json, max_dim, interactive, present)
                .await
                .map(JsValue::from)
                .map_err(JsValue::from)
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Scaled {
    size: (u32, u32),
    interactive: bool,
}

/// An image and the key that names its pixels to the GPU, which keeps the
/// last ones it rendered uploaded.
type Keyed = (Rc<ImageBuf>, u64);

impl State {
    fn keyed(&mut self, image: ImageBuf) -> Keyed {
        self.keys += 1;
        (Rc::new(image), self.keys)
    }

    /// The source fitted within `max_dim`, resampled once per size.
    fn source_at(&mut self, max_dim: u32, interactive: bool) -> Option<Keyed> {
        let source = self.source.clone()?;
        let (width, height) = (source.0.width, source.0.height);
        let scale = (max_dim as f64 / width.max(height) as f64).min(1.0);
        let size = (
            ((width as f64 * scale).round() as u32).clamp(1, width),
            ((height as f64 * scale).round() as u32).clamp(1, height),
        );
        if size == (width, height) {
            return Some(source);
        }
        let key = Scaled { size, interactive };
        if let Some((_, image)) = self.scaled.iter().find(|(k, _)| *k == key) {
            return Some(image.clone());
        }
        let image = if interactive {
            preview::downsample_area(&source.0, size)
        } else {
            preview::rescale_preview_input_fast(&source.0, size)
        };
        let image = self.keyed(image);
        // One frame of each kind: replace the old one of this kind.
        self.scaled.retain(|(k, _)| k.interactive != interactive);
        self.scaled.push((key, image.clone()));
        Some(image)
    }
}

async fn render(
    state: &RefCell<State>,
    film: &str,
    paper: &str,
    params_json: &str,
    max_dim: u32,
    interactive: bool,
    present: bool,
) -> Result<Rendered, JsError> {
    let started = web_time::Instant::now();
    let mut params = parse_params(params_json).map_err(js_error)?;
    let mut stages = Stages::new(started);
    let (pipeline, (source, source_key), backend, gpu) = {
        let mut state = state.borrow_mut();
        let source = state.source_at(max_dim, interactive).ok_or_else(|| JsError::new("no image loaded"))?;
        stages.mark("resample");
        if let Some(colour) = &state.input_colour {
            params.io.apply_input_colour(colour);
        }
        data_fs::take_missing();
        let data = Path::new(DATA_DIR);
        let key = preview::preview_pipeline_cache_key(film, paper, &params);
        if state.pipeline.as_ref().is_none_or(|(old, _)| old != &key) {
            state.pipeline = None;
            let built = (|| -> Result<Pipeline, String> {
                let film_profile = profile::load_profile_by_name(data, film).map_err(|e| e.to_string())?;
                let print = if params.io.scan_film { film } else { paper };
                let print_profile = profile::load_profile_by_name(data, print).map_err(|e| e.to_string())?;
                Pipeline::new_with_spectral(film_profile, print_profile, params.clone(), data)
            })();
            let missing = missing();
            if !missing.is_empty() {
                return Ok(Rendered::missing(missing));
            }
            state.pipeline = Some((key, built.map_err(|e| JsError::new(&e))?));
        }
        if params.camera.auto_exposure && state.metering.is_none() {
            let full = &state.source.as_ref().unwrap().0;
            state.metering = Some(spektrafilm_core::stages::filming::metering_preview(full));
        }
        let mut pipeline = state.pipeline.as_ref().unwrap().1.clone().with_params(params.clone());
        if let Some(metering) = state.metering.clone().filter(|_| params.camera.auto_exposure) {
            pipeline = pipeline.with_metering_preview(metering);
            let io = &params.io;
            let metered = serde_json::json!([
                key_of(&state.pipeline),
                params.camera.auto_exposure_method,
                io.input_color_space,
                io.input_cctf_decoding,
                params.settings,
            ])
            .to_string();
            let ev = match &state.autoexposure {
                Some((key, ev)) if *key == metered => *ev,
                _ => pipeline.frame_autoexposure_ev(&source.0),
            };
            state.autoexposure = Some((metered, ev));
            pipeline = pipeline.with_autoexposure_ev(ev);
        }
        let gpu = state.gpu.clone().filter(|_| present && state.canvas);
        (pipeline, source, state.backend.clone(), gpu)
    };
    stages.mark("prepare");

    // The GPU's readback resolves between tasks, so await it; whatever the
    // resident chain doesn't cover renders on the CPU, which never waits.
    let (mapped_tx, mapped_rx) = futures_channel::oneshot::channel();
    let on_mapped = Box::new(move || {
        let _ = mapped_tx.send(());
    });
    // The GPU makes the display bytes when they'd be the same, and presents
    // them when there's a canvas.
    let encoded = params.io.output_cctf_encoding;
    let output = match (pipeline.display_on_gpu(backend.as_ref()), gpu.is_some()) {
        (true, true) => ChainOutput::Present { encoded },
        (true, false) => ChainOutput::Rgba8 { encoded },
        (false, _) => ChainOutput::Scan,
    };
    let submitted = if backend.is_gpu() {
        pipeline.submit_resident(&source, Some(source_key), backend.as_ref(), output, on_mapped)
    } else {
        None
    };
    let (result, used_backend) = match submitted {
        Some(pending) => {
            stages.mark("submit");
            mapped_rx.await.map_err(|_| JsError::new("GPU readback was cancelled"))?;
            stages.mark("gpu");
            let result = pipeline.finish_submitted(pending, backend.as_ref());
            stages.mark("finish");
            (result, backend.name().to_owned())
        }
        None => {
            let (output, used) = pipeline
                .try_process_with_backend((*source).clone(), &CpuBackend)
                .map_err(|e| JsError::new(&e.to_string()))?;
            stages.mark("cpu");
            (ChainResult::Scan(output), used.to_owned())
        }
    };
    let missing = missing();
    if !missing.is_empty() {
        return Ok(Rendered::missing(missing));
    }
    let (width, height, mut rgba, mut presented) = match result {
        ChainResult::Scan(output) => {
            let encoding = Encoding::new(&params.io.output_color_space, encoded);
            let (width, height, rgba) = preview::display_rgba(&output, usize::MAX, encoding);
            (width as u32, height as u32, rgba, false)
        }
        ChainResult::Rgba8 { width, height, rgba } => (width, height, rgba, false),
        ChainResult::Presented { width, height } => (width, height, Vec::new(), true),
    };
    if let Some(gpu) = gpu.filter(|_| !presented) {
        presented = gpu.present_rgba(width, height, &rgba);
        if presented {
            rgba = Vec::new();
        }
    }
    stages.mark("display");
    Ok(Rendered {
        width,
        height,
        rgba,
        presented,
        missing,
        backend: used_backend,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        stages: stages.text,
    })
}

fn key_of(pipeline: &Option<(String, Pipeline)>) -> &str {
    pipeline.as_ref().map_or("", |(key, _)| key)
}

struct Stages {
    last: web_time::Instant,
    text: String,
}

impl Stages {
    fn new(started: web_time::Instant) -> Self {
        Stages { last: started, text: String::new() }
    }

    fn mark(&mut self, stage: &str) {
        let now = web_time::Instant::now();
        let ms = (now - self.last).as_secs_f64() * 1000.0;
        self.text += &format!("{}{stage} {ms:.0}", if self.text.is_empty() { "" } else { " · " });
        self.last = now;
    }
}

impl Rendered {
    fn missing(missing: Vec<String>) -> Self {
        let (backend, stages) = (String::new(), String::new());
        Rendered { width: 0, height: 0, rgba: Vec::new(), presented: false, missing, backend, elapsed_ms: 0.0, stages }
    }
}

fn js_error(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

fn missing() -> Vec<String> {
    data_fs::take_missing()
        .into_iter()
        .map(|path| path.strip_prefix(DATA_DIR).unwrap_or(&path).to_string_lossy().into_owned())
        .collect()
}

fn parse_params(json: &str) -> anyhow::Result<RuntimeParams> {
    if json.trim().is_empty() {
        return Ok(RuntimeParams::default());
    }
    let mut value: serde_json::Value = serde_json::from_str(json)?;
    spektrafilm_core::params::infer_input_interpretation(&mut value);
    Ok(serde_json::from_value(value)?)
}

/// `spektrafilm_io`'s file decoder, from memory: orientation applied, the
/// embedded profile kept.
fn decode(
    bytes: &[u8],
) -> anyhow::Result<(image::DynamicImage, spektrafilm_io::colour::FileColour, image::ImageFormat)> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.no_limits();
    let format = reader.format().ok_or_else(|| anyhow::anyhow!("unknown image format"))?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut info = spektrafilm_io::colour::FileColour {
        icc: decoder.icc_profile().ok().flatten(),
        cmyk: matches!(
            decoder.original_color_type(),
            image::ExtendedColorType::Cmyk8 | image::ExtendedColorType::Cmyk16
        ),
        ..Default::default()
    };
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    info.float = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    info.gray = !img.color().has_color();
    Ok((img, info, format))
}
