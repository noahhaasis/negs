//! Browser entry points for the spektrafilm engine.
//!
//! The page registers the data files a render reads (see
//! [`spektrafilm_core::data_fs`]), loads a photograph's bytes and renders it
//! to sRGB RGBA8. A render that read an unregistered file reports the paths
//! in [`Rendered::missing`]; the page fetches them and renders again.
#![cfg(target_arch = "wasm32")]

use spektrafilm_core::{data_fs, params::RuntimeParams, pipeline::Pipeline, preview, profile};
use spektrafilm_gpu::{
    ChainOutput, ChainResult, ComputeBackend,
    cpu_backend::CpuBackend,
    wgpu_backend::{BorderView, View, WgpuBackend},
};
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

/// Wakes rayon's workers after the page terminated them, so they end: Safari
/// doesn't end a worker that waits in `memory.atomic.wait`, as idle ones do,
/// and their shared memory stayed taken until the tab closed.
#[cfg(feature = "threads")]
#[wasm_bindgen(js_name = wakeThreadPool)]
pub fn wake_thread_pool() {
    rayon::spawn_broadcast(|_| {});
}

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

/// The enlarger M/Y filter shifts that print a mid-grey neutral with these
/// settings, as the native helper's `balance_print`
/// ([`spektrafilm_core::print_balance`]); image-independent, on the CPU.
#[wasm_bindgen(js_name = balancePrint)]
pub fn balance_print(film: &str, paper: &str, params_json: &str) -> Result<Balanced, JsError> {
    let started = web_time::Instant::now();
    let params = parse_params(params_json).map_err(js_error)?;
    let data = Path::new(DATA_DIR);
    data_fs::take_missing();
    let solved = (|| -> Result<(f64, f64), String> {
        spektrafilm_core::print_balance::solve_neutral_filter_shifts(
            profile::load_profile_by_name(data, film).map_err(|e| e.to_string())?,
            profile::load_profile_by_name(data, paper).map_err(|e| e.to_string())?,
            &params,
            data,
            spektrafilm_core::print_balance::MIDGRAY,
        )
    })();
    // A missing neutral-filter database falls back silently, so check even
    // when it solved.
    let missing = missing();
    if !missing.is_empty() {
        return Ok(Balanced { m_filter_shift: 0.0, y_filter_shift: 0.0, elapsed_ms: 0.0, missing });
    }
    let (m, y) = solved.map_err(|e| JsError::new(&e))?;
    Ok(Balanced {
        m_filter_shift: m,
        y_filter_shift: y,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        missing,
    })
}

#[wasm_bindgen(getter_with_clone)]
pub struct Balanced {
    #[wasm_bindgen(js_name = mFilterShift)]
    pub m_filter_shift: f64,
    #[wasm_bindgen(js_name = yFilterShift)]
    pub y_filter_shift: f64,
    #[wasm_bindgen(js_name = elapsedMs)]
    pub elapsed_ms: f64,
    /// Data files the solver read but nobody registered, as
    /// [`Rendered::missing`]: register them and solve again.
    pub missing: Vec<String>,
}

/// A presentation border around an exported frame's sRGB RGBA8 bytes, as
/// the CLI adds one (`spektrafilm-io::border`): `style` (`solid`, `keyline`,
/// `film` or `35mm`) with `options` (a `BorderOptions` object), and with
/// `instagram` the 3:4 extension; `film` picks the 35mm edge print. The
/// bytes are bordered as encoded sRGB values (byte / 255) and rounded back.
/// Textures it reads but nobody registered come back in `missing`, as for
/// renders.
#[wasm_bindgen(js_name = addBorder)]
pub fn add_border(
    rgba: &[u8],
    width: u32,
    height: u32,
    style: &str,
    options_json: &str,
    instagram: bool,
    film: &str,
) -> Result<Bordered, JsError> {
    use spektrafilm_io::border;
    let started = web_time::Instant::now();
    let style: border::BorderStyle = style.parse().map_err(js_error)?;
    let options: border::BorderOptions = serde_json::from_str(options_json).map_err(|e| JsError::new(&e.to_string()))?;
    let encoding = Encoding::new("sRGB", true);
    let pixels = (width as usize) * (height as usize);
    if rgba.len() != pixels * 4 {
        return Err(JsError::new("the bytes aren't width × height RGBA"));
    }
    let data = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .map(|v| spektrafilm_math::precision::from_f32(v as f32 / 255.0))
        .collect();
    let image = ImageBuf::from_data(width, height, data);
    data_fs::take_missing();
    let bordered = border::add_border_with(&image, style, &options, film, encoding, &load_data_image);
    let missing = missing();
    if !missing.is_empty() {
        return Ok(Bordered { width: 0, height: 0, rgba: Vec::new(), elapsed_ms: 0.0, missing });
    }
    let mut bordered = bordered.map_err(js_error)?;
    drop(image);
    if instagram {
        bordered = border::extend_for_instagram(&bordered, style, &options, encoding);
    }
    let mut out = Vec::with_capacity(bordered.data.len() / 3 * 4);
    for p in bordered.data.chunks_exact(3) {
        for &v in p {
            out.push((spektrafilm_math::precision::to_f32(v).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out.push(255);
    }
    Ok(Bordered {
        width: bordered.width,
        height: bordered.height,
        rgba: out,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        missing,
    })
}

#[wasm_bindgen(getter_with_clone)]
pub struct Bordered {
    pub width: u32,
    pub height: u32,
    /// sRGB RGBA8, empty when `missing` isn't.
    pub rgba: Vec<u8>,
    #[wasm_bindgen(js_name = elapsedMs)]
    pub elapsed_ms: f64,
    pub missing: Vec<String>,
}

/// A registered image in the data directory, decoded.
fn load_data_image(path: &str) -> anyhow::Result<image::DynamicImage> {
    let bytes = data_fs::read(&Path::new(DATA_DIR).join(path)).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    Ok(image::load_from_memory(&bytes)?)
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
    /// Unclipped scan RGB, only for the diagnostic `renderRaw` entry point.
    /// Normal rendering never allocates or reads back this extra buffer.
    pub scan: Vec<f32>,
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
    /// The frame on the canvas, when the GPU made it from a scan it still
    /// may hold: everything it was rendered from but the Post settings
    /// ([`look_key`]), and its size. A change to Post alone then makes it
    /// again from that scan ([`WgpuBackend::repost`]).
    shown: Option<(String, (u32, u32))>,
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
            shown: None,
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

    /// Show the photograph on the canvas where the page puts it, from now
    /// on and at once, from the frame on it: a display pass, without the
    /// film chain. `width` × `height` is the canvas in device pixels, `rect`
    /// the finished photograph's left, top, width and height on it, and
    /// `background` the RGB around it, 0 to 1. `original` draws the
    /// original ([`Engine::original`]) in the developed frame's place, when
    /// there is one. `border` is a [`BorderView`]'s 36 numbers, as
    /// `web/border.js` lays it out, or empty for none. `transform` is the
    /// inverse normalized affine map and output extent from `web/crop.js`
    /// (12 numbers), or empty for identity. Resolves to the milliseconds until the GPU had drawn it, or -1 with no frame to draw
    /// yet.
    pub fn show(
        &self,
        width: u32,
        height: u32,
        rect: Vec<f32>,
        background: Vec<f32>,
        original: bool,
        border: Vec<f32>,
        transform: Vec<f32>,
    ) -> js_sys::Promise {
        let gpu = {
            let state = self.state.borrow();
            state.gpu.clone().filter(|_| state.canvas)
        };
        let (Some(gpu), &[x, y, w, h], &[r, g, b]) = (gpu, &rect[..], &background[..]) else {
            return js_sys::Promise::resolve(&JsValue::from(-1.0));
        };
        let started = web_time::Instant::now();
        let border = match border.len() {
            36 => {
                let v = |i: usize| [border[4 * i], border[4 * i + 1], border[4 * i + 2], border[4 * i + 3]];
                BorderView {
                    photo: v(0),
                    canvas: v(1),
                    frame: v(2),
                    line: v(3),
                    fill: v(4),
                    mat: v(5),
                    line_color: v(6),
                    texture: v(7),
                    texture_fit: v(8),
                }
            }
            _ => BorderView::default(),
        };
        // Inverse normalized affine rows, output/source extents, clipping
        // and whether leveling requires filtered sampling (web/crop.js).
        let transform = transform.try_into().unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0]);
        let view = View { width, height, rect: [x, y, w, h], background: [r, g, b], original, border, transform };
        let (drawn_tx, drawn_rx) = futures_channel::oneshot::channel();
        let drawing = gpu.show(
            view,
            Box::new(move || {
                let _ = drawn_tx.send(());
            }),
        );
        wasm_bindgen_futures::future_to_promise(async move {
            if !drawing {
                return Ok(JsValue::from(-1.0));
            }
            drawn_rx.await.map_err(|_| JsValue::from("GPU drawing was cancelled"))?;
            Ok(JsValue::from(started.elapsed().as_secs_f64() * 1000.0))
        })
    }

    /// Decode the registered PNG at `path` in the data directory
    /// (`borders/film.png`) and keep it as the border's texture on the
    /// canvas, for views whose border has one; an empty path clears it.
    /// Resolves to its `[width, height]`, or empty without a canvas.
    #[wasm_bindgen(js_name = borderTexture)]
    pub fn border_texture(&self, path: &str) -> Result<Vec<u32>, JsError> {
        let gpu = {
            let state = self.state.borrow();
            state.gpu.clone().filter(|_| state.canvas)
        };
        let Some(gpu) = gpu else {
            return Ok(Vec::new());
        };
        if path.is_empty() {
            gpu.set_border_texture(None);
            return Ok(Vec::new());
        }
        let image = load_data_image(path).map_err(js_error)?.to_rgba8();
        let (width, height) = image.dimensions();
        if !gpu.set_border_texture(Some((width, height, image.as_raw()))) {
            return Ok(Vec::new());
        }
        Ok(vec![width, height])
    }

    /// The frame on the canvas and where it's drawn, in the canvas's device
    /// pixels: `[width, height, left, top, drawn width, drawn height]`, or
    /// empty.
    #[wasm_bindgen(js_name = shownRect)]
    pub fn shown_rect(&self) -> Vec<f32> {
        let gpu = self.state.borrow().gpu.clone();
        gpu.and_then(|gpu| gpu.shown()).map_or(Vec::new(), |shown| shown.to_vec())
    }

    /// Limit the WebGPU engine's diffusion FFT grid to `bytes` (0: none),
    /// past which it makes the glow from a smaller copy of the frame
    /// ([`WgpuBackend::set_diffusion_grid_budget`]).
    #[wasm_bindgen(js_name = setDiffusionGridBudget)]
    pub fn set_diffusion_grid_budget(&self, bytes: f64) {
        if let Some(gpu) = &self.state.borrow().gpu {
            gpu.set_diffusion_grid_budget(bytes as u64);
        }
    }

    /// The largest frame the WebGPU engine renders and shows: `[longest
    /// side, pixels]`; empty for the CPU engine.
    pub fn limits(&self) -> Vec<f64> {
        let gpu = self.state.borrow().gpu.clone();
        gpu.map_or(Vec::new(), |gpu| {
            let (side, pixels) = gpu.image_limits();
            vec![side as f64, pixels as f64]
        })
    }

    /// Decode a PNG, JPEG or TIFF (with its embedded profile) or a camera
    /// RAW (with rawler, as shot), keeping it at most `max_dim` pixels on the
    /// longer side and `max_pixels` in all (0: no limit), resolving to its
    /// `[width, height]` as kept and as decoded. A device short of memory
    /// keeps a smaller working copy so; exports load it again in full.
    pub fn load(&self, bytes: &[u8], max_dim: u32, max_pixels: f64) -> Result<Vec<u32>, JsError> {
        // The last photograph goes first, so its memory holds the new one's
        // decode: wasm's memory never shrinks, so its peak is what stays.
        // A failed load leaves none.
        self.forget();
        let (image, colour, full) = decode_photo(bytes, max_dim, max_pixels)?;
        let size = vec![image.width, image.height, full.0, full.1];
        self.keep(image, colour);
        Ok(size)
    }

    /// Keep a photograph [`decode`] made elsewhere: its `width` × `height`
    /// RGB pixels and its [`Decoded::colour`]. Its decode's memory then
    /// stays with the module that made it, which can end.
    #[wasm_bindgen(js_name = loadDecoded)]
    pub fn load_decoded(&self, pixels: Vec<f32>, width: u32, height: u32, colour: &str) -> Result<(), JsError> {
        self.forget();
        if pixels.len() != width as usize * height as usize * 3 {
            return Err(JsError::new("decoded pixels don't match their size"));
        }
        let colour = serde_json::from_str(colour).map_err(|e| js_error(e.into()))?;
        self.keep(ImageBuf::from_data(width, height, pixels), colour);
        Ok(())
    }

    fn keep(&self, image: ImageBuf, colour: spektrafilm_io::InputColour) {
        let mut state = self.state.borrow_mut();
        state.source = Some(state.keyed(image));
        state.input_colour = Some(colour);
    }

    /// Drop the loaded photograph and everything made from it. Before a
    /// [`Engine::load`], whose bytes the call copies into the module's memory
    /// first: then they and the decode have the photograph's room too.
    pub fn forget(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(gpu) = &state.gpu {
            gpu.forget_photograph();
        }
        state.source = None;
        state.scaled.clear();
        state.metering = None;
        state.autoexposure = None;
        state.shown = None;
        state.input_colour = None;
    }

    /// The loaded photograph before any film, as the native helper's
    /// `original` shows it (the decoded source: rawler's develop for a RAW),
    /// at most `max_dim` pixels on the longer side, resampled as a settled
    /// frame of that size is. `params_json` says how to show it (the input
    /// interpretation, the output colour space). `present` keeps it on the
    /// canvas, if there is one, for views with `original` to draw
    /// ([`Engine::show`]); otherwise the [`Rendered`] carries its bytes.
    pub fn original(&self, params_json: &str, max_dim: u32, present: bool) -> Result<Rendered, JsError> {
        let started = web_time::Instant::now();
        let mut params = parse_params(params_json).map_err(js_error)?;
        let mut stages = Stages::new(started);
        let mut state = self.state.borrow_mut();
        let (source, key) = state.source_at(max_dim, false).ok_or_else(|| JsError::new("no image loaded"))?;
        stages.mark("resample");
        let colour = state.input_colour.clone().ok_or_else(|| JsError::new("no image loaded"))?;
        params.io.apply_input_colour(&colour);
        let (encoding, converted) = preview::original_encoding(&params, &colour, &source);
        let gpu = state.gpu.clone().filter(|_| present && state.canvas);
        // An sRGB original is the input the film chain has on the GPU, made
        // into bytes there as a presented frame's scan is; others are made
        // here.
        if let Some(gpu) = &gpu
            && converted.is_none()
            && encoding.space.name == "sRGB"
            && gpu.set_original_from_input(key, &source, encoding.encoded)
        {
            stages.mark("gpu");
            let (width, height) = (source.width, source.height);
            return Ok(Rendered::original(width, height, Vec::new(), true, &state, started, stages));
        }
        let (width, height, mut rgba) = preview::display_rgba(converted.as_ref().unwrap_or(&source), usize::MAX, encoding);
        let (width, height) = (width as u32, height as u32);
        stages.mark("display");
        let presented = gpu.is_some_and(|gpu| gpu.set_original(width, height, &rgba));
        if presented {
            rgba = Vec::new();
            stages.mark("upload");
        }
        Ok(Rendered::original(width, height, rgba, presented, &state, started, stages))
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
            render(&state, &film, &paper, &params_json, max_dim, interactive, present, false)
                .await
                .map(JsValue::from)
                .map_err(JsValue::from)
        })
    }

    /// Diagnostic counterpart of `render`: returns the unclipped scan before
    /// display quantisation, for the native golden harness's numeric gates.
    /// Uses the same pipeline and resident GPU chain, not a CPU substitute.
    #[wasm_bindgen(js_name = renderRaw)]
    pub fn render_raw(&self, film: String, paper: String, params_json: String, max_dim: u32) -> js_sys::Promise {
        let state = self.state.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            render(&state, &film, &paper, &params_json, max_dim, false, false, true)
                .await
                .map(JsValue::from)
                .map_err(JsValue::from)
        })
    }
}

/// The size an image of `width` × `height` is kept at, within `max_dim` on
/// its longer side and `max_pixels` in all (0: no limit): the longer side the
/// page's `renderSide` caps a render at, so the largest render is this copy.
fn kept_size(width: u32, height: u32, max_dim: u32, max_pixels: f64) -> (u32, u32) {
    let (long, short) = (width.max(height) as f64, width.min(height) as f64);
    let mut side = if max_dim == 0 { long } else { long.min(max_dim as f64) };
    if max_pixels > 0.0 && side * side * short / long > max_pixels {
        side = (max_pixels * long / short).sqrt().floor();
    }
    let scale = (side / long).min(1.0);
    (
        ((width as f64 * scale).round() as u32).clamp(1, width),
        ((height as f64 * scale).round() as u32).clamp(1, height),
    )
}

/// A photograph decoded and kept as [`Engine::load`] keeps it, for a
/// short-lived module to make: wasm's memory never shrinks, so the decode's
/// peak then leaves with it instead of staying in the engine's.
#[wasm_bindgen]
pub struct Decoded {
    pixels: Vec<f32>,
    pub width: u32,
    pub height: u32,
    /// The size as decoded, upright.
    #[wasm_bindgen(js_name = fullWidth)]
    pub full_width: u32,
    #[wasm_bindgen(js_name = fullHeight)]
    pub full_height: u32,
    colour: String,
}

#[wasm_bindgen]
impl Decoded {
    /// The RGB pixels, row-major; the first call takes them.
    #[wasm_bindgen(js_name = takePixels)]
    pub fn take_pixels(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.pixels)
    }

    /// What the loader read it as, for [`Engine::load_decoded`].
    #[wasm_bindgen(getter)]
    pub fn colour(&self) -> String {
        self.colour.clone()
    }
}

/// Decode a photograph as [`Engine::load`] does, without an engine.
#[wasm_bindgen(js_name = decode)]
pub fn decode_kept(bytes: &[u8], max_dim: u32, max_pixels: f64) -> Result<Decoded, JsError> {
    let (image, colour, (full_width, full_height)) = decode_photo(bytes, max_dim, max_pixels)?;
    let colour = serde_json::to_string(&colour).map_err(|e| js_error(e.into()))?;
    let (width, height) = (image.width, image.height);
    Ok(Decoded { pixels: image.data, width, height, full_width, full_height, colour })
}

/// The kept image, how to read it and its size as decoded, upright.
fn decode_photo(
    bytes: &[u8],
    max_dim: u32,
    max_pixels: f64,
) -> Result<(ImageBuf, spektrafilm_io::InputColour, (u32, u32)), JsError> {
    // A RAW turns upright after it shrinks: a smaller copy to turn.
    let mut orientation = 1;
    let (image, colour) = match decode(bytes) {
        Ok((image, info, format)) => {
            let interpretation = spektrafilm_io::InputInterpretation::default();
            let png = format == image::ImageFormat::Png;
            spektrafilm_io::colour::convert(image, &info, &interpretation, png).map_err(js_error)?
        }
        Err(not_an_image) => {
            // A camera RAW the decoder knows but can't read says so;
            // anything else may have been a damaged image.
            let (image, exif) = spektrafilm_io::load_raw_bytes_unoriented(bytes, Default::default()).map_err(|raw| {
                let raw = format!("{raw:#}");
                match raw.split_once("DecoderFailed(\"") {
                    Some((_, reason)) => JsError::new(reason.split('"').next().unwrap_or(&raw)),
                    None => js_error(not_an_image.context(raw)),
                }
            })?;
            orientation = exif;
            (image, spektrafilm_io::InputColour::raw("rawler"))
        }
    };
    let (width, height) = (image.width, image.height);
    let target = kept_size(width, height, max_dim, max_pixels);
    let image = if target == (width, height) {
        image
    } else {
        // The full image goes as its copy comes.
        let small = preview::downsample_area(&image, target);
        drop(image);
        small
    };
    let full = if (5..=8).contains(&orientation) { (height, width) } else { (width, height) };
    Ok((image.oriented(orientation), colour, full))
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

    /// The size of the source fitted within `max_dim`.
    fn size_at(&self, max_dim: u32) -> Option<(u32, u32)> {
        let source = self.source.as_ref()?;
        let (width, height) = (source.0.width, source.0.height);
        let scale = (max_dim as f64 / width.max(height) as f64).min(1.0);
        Some((
            ((width as f64 * scale).round() as u32).clamp(1, width),
            ((height as f64 * scale).round() as u32).clamp(1, height),
        ))
    }

    /// The source fitted within `max_dim`, resampled once per size.
    fn source_at(&mut self, max_dim: u32, interactive: bool) -> Option<Keyed> {
        let source = self.source.clone()?;
        let (width, height) = (source.0.width, source.0.height);
        let size = self.size_at(max_dim)?;
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
    raw: bool,
) -> Result<Rendered, JsError> {
    let started = web_time::Instant::now();
    let mut params = parse_params(params_json).map_err(js_error)?;
    let mut stages = Stages::new(started);
    if present && let Some(rendered) = repost(state, film, paper, &params, max_dim, interactive, started).await? {
        return Ok(rendered);
    }
    let (pipeline, (source, source_key), backend, gpu) = {
        let mut state = state.borrow_mut();
        if !state.backend.is_gpu() {
            let (width, height) = state.size_at(max_dim).ok_or_else(|| JsError::new("no image loaded"))?;
            // The exact CPU convolution keeps three complex FFT planes
            // alive, plus the PSF, RGB images, and decoded source. Leave
            // room for those in wasm32's 4 GiB address space. The iPad
            // reproduction requested a 2.7 GiB Vec (itself > isize::MAX).
            // Reject before resampling or constructing the large PSF.
            const MAX_FFT_BUFFER_BYTES: u64 = 512 * 1024 * 1024;
            if preview::cpu_diffusion_fft_buffer_bytes(&params, width, height) > MAX_FFT_BUFFER_BYTES {
                return Err(JsError::new("BROWSER_RENDER_LIMIT: Diffusion at this resolution needs too much memory in this browser. Reduce its spatial scale or use the desktop app for full-resolution export."));
            }
        }
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
            stages.mark("pipeline");
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
                _ => {
                    let ev = pipeline.frame_autoexposure_ev(&source.0);
                    stages.mark("meter");
                    ev
                }
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
    let post = || pipeline.display_post().map(std::sync::Arc::new);
    let output = match (!raw && pipeline.display_on_gpu(backend.as_ref()), gpu.is_some()) {
        (true, true) => ChainOutput::Present { encoded, post: post() },
        (true, false) => ChainOutput::Rgba8 { encoded, post: post() },
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
    let mut scan = Vec::new();
    let (width, height, mut rgba, mut presented) = match result {
        ChainResult::Scan(output) => {
            if raw {
                scan = output.data.clone();
            }
            let encoding = Encoding::new(&params.io.output_color_space, encoded);
            let (width, height, rgba) = preview::display_rgba(&output, usize::MAX, encoding);
            (width as u32, height as u32, rgba, false)
        }
        ChainResult::Rgba8 { width, height, rgba } => (width, height, rgba, false),
        ChainResult::Presented { width, height } => (width, height, Vec::new(), true),
    };
    {
        let mut state = state.borrow_mut();
        if presented {
            state.shown = Some((look_key(&state, film, paper, &params), (width, height)));
        } else if gpu.is_some() {
            state.shown = None;
        }
    }
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
        scan,
        presented,
        missing,
        backend: used_backend,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        stages: stages.text,
    })
}

/// A Post edit to the frame on the canvas: made again from its scan, with no
/// film chain, when everything else it was rendered from is the same. An
/// interactive frame takes the frame whatever its size, a settled one only
/// at its own size. None when it has to render.
async fn repost(
    state: &RefCell<State>,
    film: &str,
    paper: &str,
    params: &RuntimeParams,
    max_dim: u32,
    interactive: bool,
    started: web_time::Instant,
) -> Result<Option<Rendered>, JsError> {
    let (gpu, post) = {
        let state = state.borrow();
        let Some(gpu) = state.gpu.clone().filter(|_| state.canvas) else {
            return Ok(None);
        };
        let Some((key, size)) = &state.shown else {
            return Ok(None);
        };
        let mut params = params.clone();
        if let Some(colour) = &state.input_colour {
            params.io.apply_input_colour(colour);
        }
        if *key != look_key(&state, film, paper, &params) || !(interactive || state.size_at(max_dim) == Some(*size)) {
            return Ok(None);
        }
        let post = (!params.post.is_identity()).then(|| {
            spektrafilm_core::post::PostProcess::new(&params.post, params.io.output_cctf_encoding, &params.io.output_color_space)
                .display()
        });
        (gpu, post)
    };
    let (drawn_tx, drawn_rx) = futures_channel::oneshot::channel();
    let on_drawn = Box::new(move || {
        let _ = drawn_tx.send(());
    });
    let Some((width, height)) = gpu.repost(post.as_ref(), on_drawn) else {
        state.borrow_mut().shown = None;
        return Ok(None);
    };
    let mut stages = Stages::new(started);
    stages.mark("submit");
    drawn_rx.await.map_err(|_| JsError::new("GPU drawing was cancelled"))?;
    stages.mark("post");
    Ok(Some(Rendered {
        width,
        height,
        rgba: Vec::new(),
        scan: Vec::new(),
        presented: true,
        missing: Vec::new(),
        backend: gpu.name().to_owned(),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        stages: stages.text,
    }))
}

/// What a frame is rendered from but its Post settings: the photograph, the
/// stocks and every other parameter.
fn look_key(state: &State, film: &str, paper: &str, params: &RuntimeParams) -> String {
    let mut params = params.clone();
    params.post = Default::default();
    let photo = state.source.as_ref().map_or(0, |(_, key)| *key);
    serde_json::json!([photo, film, paper, params]).to_string()
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
        Rendered { width: 0, height: 0, rgba: Vec::new(), scan: Vec::new(), presented: false, missing, backend, elapsed_ms: 0.0, stages }
    }

    /// An original's ([`Engine::original`]).
    fn original(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        presented: bool,
        state: &State,
        started: web_time::Instant,
        stages: Stages,
    ) -> Self {
        Rendered {
            width,
            height,
            rgba,
            scan: Vec::new(),
            presented,
            missing: Vec::new(),
            backend: state.backend.name().to_owned(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            stages: stages.text,
        }
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
