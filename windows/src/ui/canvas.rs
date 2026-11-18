//! The photograph: wgpu renders straight into this window. Zoom and pan are
//! a transform on the uploaded texture, so they never re-render or copy
//! pixels. Crop, rotation and border are applied when drawing, as on the Mac
//! (`PreviewCanvas.swift`, `Border.swift`, `Crop.swift`).
//!
//! The view state lives here, apart from the app, so the canvas can paint
//! while the app is busy (a modal dialog, a re-entrant call).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::TextStyle;
use super::{App, post, with_app};
use crate::border::{BorderSettings, BorderStyle, FilmFormat};
use crate::crop::{self, CropSettings, Handle, Point, Rect, Size};
use crate::gpu::{self, Gpu, GpuImage, Item, PhotoUniform, QuadInstance};
use crate::icc;
use crate::picture::Picture;

mod software;

thread_local! {
    /// Settings › Custom viewer background, when on.
    static CUSTOM_BACKGROUND: Cell<Option<[u8; 3]>> = const { Cell::new(None) };
}

/// The colour around the photograph: Settings' custom colour, or the
/// theme's grey. High contrast keeps the system's colours.
fn background() -> [u8; 3] {
    let theme = super::theme::current();
    match CUSTOM_BACKGROUND.with(Cell::get) {
        Some(custom) if !theme.high_contrast => custom,
        _ => theme.palette.canvas,
    }
}

/// Sets the custom viewer background (None: the theme's) and repaints.
pub fn set_custom_background(hwnd: HWND, color: Option<[u8; 3]>) {
    if CUSTOM_BACKGROUND.with(|c| c.replace(color)) != color {
        invalidate(hwnd);
    }
}
/// 1:1 is one photograph pixel per screen pixel. Fitting shows the whole
/// photograph, but never enlarges a small one past 1:1 (the Mac's
/// `ImageScrollView.magnification`).
fn zoom_amount(fitting: bool, document: (f64, f64), viewport: (f64, f64)) -> f64 {
    if !fitting {
        return 1.0;
    }
    (viewport.0 / document.0).min(viewport.1 / document.1).min(1.0)
}

const MIN_ZOOM: f64 = 0.005;
const MAX_ZOOM: f64 = 16.0;
const CROP_MARGIN: f64 = 36.0;
const DIAL_SPACE: f64 = 76.0;
/// DIPs per degree on the straighten dial.
const DIAL_SPACING: f64 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grid {
    Thirds,
    Fine,
}

/// What crop mode shows: the whole photograph as shown (quarter turns
/// applied), turned by the angle, with the crop over it.
#[derive(Clone, Debug, PartialEq)]
pub struct CropScene {
    pub photo: Size,
    pub rotation: i32,
    pub angle: f64,
    pub shown: Rect,
    pub grid: Option<Grid>,
}

/// What the app wants on the canvas.
#[derive(Clone, Default)]
pub struct Scene {
    pub picture: Option<Picture>,
    /// The developed preview (not the placeholder or the original).
    pub developed: bool,
    /// The photograph's layout size in pixels, before cropping and rotation.
    pub size: Size,
    pub rotation: i32,
    pub crop: CropSettings,
    pub border: Option<BorderSettings>,
    pub film: String,
    pub badge: Option<&'static str>,
    pub zoom_command: u64,
    pub fit: bool,
    pub cropping: Option<CropScene>,
    pub drop_target: bool,
    /// The photograph couldn't be developed: its name and the reason, said
    /// where it would be, with Try Again (§4.14).
    pub failed: Option<(String, String)>,
    /// The render the developed picture came from, for the timing trace.
    pub frame: Option<crate::model::FrameTag>,
}


struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

struct Globals {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    cm: Option<icc::Transform>,
}

struct View {
    hwnd: HWND,
    dpi: u32,
    width: u32,
    height: u32,
    scene: Scene,
    gpu: Option<Arc<Gpu>>,
    surface: Option<Surface>,
    globals: Option<Globals>,
    monitor: isize,
    software_cm: Option<icc::Transform>,
    software_cache: software::Cache,
    /// Screen pixels per document pixel, and the document point at the
    /// middle of the view.
    zoom: f64,
    center: (f64, f64),
    fitting: bool,
    last_command: Option<u64>,
    document: (f64, f64),
    photo_display: (f64, f64),
    crop: CropSettings,
    pan: Option<(i32, i32, (f64, f64))>,
    crop_drag: Option<CropDrag>,
    texts: HashMap<(String, i32, i32), GpuImage>,
    first_image_presented: bool,
    /// Newsreader set with DirectWrite for the canvas's words, by text and style.
    words: HashMap<(String, String), GpuImage>,
    /// Try Again, where it was last drawn, and the pointer over it or pressing it.
    retry: Option<Rect>,
    retry_hot: bool,
    retry_pressed: bool,
    /// When the running line under the Developing tag started.
    running: Option<std::time::Instant>,
    /// The last developed frame drawn (`Scene::frame`).
    presented: Option<crate::model::FrameTag>,
}

#[derive(Clone, Copy)]
enum CropDrag {
    Handle(Handle, Rect, Point),
    Dial(f64, i32),
}

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

fn with_view<R>(f: impl FnOnce(&mut View) -> R) -> Option<R> {
    VIEW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut v| v.as_mut().map(f)))
}

/// A raw-handle surface must be destroyed while its HWND is still valid.
/// Do this explicitly, rather than at thread-local teardown after DestroyWindow.
pub fn shutdown() {
    VIEW.with(|cell| {
        let view = cell.borrow_mut().take();
        if let Some(view) = &view {
            if let Some(gpu) = &view.gpu {
                let _ = gpu.device.poll(wgpu::Maintain::Wait);
            }
        }
        drop(view);
    });
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsCanvas"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

pub fn init(hwnd: HWND, dpi: u32) {
    VIEW.with(|cell| {
        *cell.borrow_mut() = Some(View {
            hwnd,
            dpi,
            width: 0,
            height: 0,
            scene: Scene::default(),
            gpu: None,
            surface: None,
            globals: None,
            monitor: 0,
            software_cm: None,
            software_cache: software::Cache::default(),
            zoom: 1.0,
            center: (0.0, 0.0),
            fitting: true,
            last_command: None,
            document: (0.0, 0.0),
            photo_display: (0.0, 0.0),
            crop: CropSettings::default(),
            pan: None,
            crop_drag: None,
            texts: HashMap::new(),
            first_image_presented: false,
            words: HashMap::new(),
            retry: None,
            retry_hot: false,
            retry_pressed: false,
            running: None,
            presented: None,
        })
    });
}

/// Creates the device on a background thread, after the window's first frame.
pub fn start_gpu(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("gpu-init".into())
        .spawn(move || {
            let result = gpu::init(raw);
            post(move |app: &mut App| match result {
                Ok((gpu, surface)) => {
                    app.report.stamp("gpu_ready");
                    gpu_ready(gpu, surface);
                    // Pictures made before the GPU was ready upload now, off the UI thread.
                    let pictures: Vec<Picture> =
                        [&app.model.image, &app.model.placeholder].into_iter().flatten().cloned().collect();
                    std::thread::spawn(move || {
                        for picture in pictures {
                            picture.upload();
                        }
                        if let Some(borders) = crate::picture::borders() {
                            if let Some(film) = &borders.film {
                                film.upload();
                            }
                        }
                        post(|app: &mut App| app.model.dirty |= crate::model::dirty::CANVAS);
                    });
                }
                Err(error) => {
                    app.report.line(&format!("software_canvas {error:#}"));
                    with_view(|view| {
                        view.update_color_management();
                        invalidate(view.hwnd);
                    });
                    app.model.dirty |= crate::model::dirty::CANVAS;
                }
            });
        })
        .ok();
}

fn gpu_ready(gpu: Arc<Gpu>, surface: wgpu::Surface<'static>) {
    with_view(|view| {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: gpu.format,
            width: view.width.max(1),
            height: view.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        view.surface = Some(Surface { surface, config });
        view.gpu = Some(gpu);
        view.monitor = 0;
        view.update_color_management();
        invalidate(view.hwnd);
    });
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

pub fn resized(hwnd: HWND) {
    with_view(|view| {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
        }
        let (w, h) = (r.right.max(0) as u32, r.bottom.max(0) as u32);
        if (w, h) == (view.width, view.height) {
            return;
        }
        view.width = w;
        view.height = h;
        if let (Some(gpu), Some(surface)) = (&view.gpu, &mut view.surface) {
            surface.config.width = w.max(1);
            surface.config.height = h.max(1);
            surface.surface.configure(&gpu.device, &surface.config);
        }
        if view.fitting {
            view.apply_zoom();
        }
        view.clamp_center();
        invalidate(hwnd);
    });
}

pub fn set_dpi(dpi: u32) {
    with_view(|view| {
        view.dpi = dpi;
        view.texts.clear();
        view.words.clear();
        if view.fitting {
            view.apply_zoom();
        }
        view.clamp_center();
        invalidate(view.hwnd);
    });
}

/// The window moved or the displays changed: a different monitor may have a
/// different colour profile.
pub fn monitor_may_have_changed() {
    with_view(|view| view.update_color_management());
}

/// How many screen pixels a pixel of the photograph covers. A render this much
/// smaller than the photograph looks the same on screen. The canvas is
/// per-monitor DPI aware (`negs.manifest`), so these are physical pixels:
/// the same window at 150 % has 1.5 times the scale it has at 100 %.
pub fn screen_pixels_per_photo_pixel() -> Option<f64> {
    #[cfg(test)]
    if let Some(scale) = FIXED_SCALE.with(Cell::get) {
        return (scale > 0.0).then_some(scale);
    }
    with_view(|view| (view.zoom > 0.0 && view.document.0 > 0.0).then_some(view.zoom)).flatten()
}

#[cfg(test)]
thread_local! {
    static FIXED_SCALE: Cell<Option<f64>> = const { Cell::new(None) };
}

/// Tests: the scale `screen_pixels_per_photo_pixel` reports on this thread,
/// in place of a canvas (the Mac's `CanvasScale.fixed`). `Some(0.0)` reports
/// no canvas; None goes back to the canvas.
#[cfg(test)]
pub fn set_fixed_scale(scale: Option<f64>) {
    FIXED_SCALE.with(|s| s.set(scale));
}

/// The developed frame the canvas last drew, for the render benchmark.
pub fn presented_frame() -> Option<crate::model::FrameTag> {
    with_view(|view| view.presented).flatten()
}

pub fn zoom_by(factor: f64) {
    with_view(|view| {
        let (w, h) = view.viewport();
        view.zoom_about(factor, w / 2.0, h / 2.0);
    });
}

/// Builds the scene from the model and applies the zoom rules.
pub fn update_scene(app: &mut App) {
    let model = &app.model;
    let (picture, size, developed) = match (&model.original, &model.image, &model.placeholder) {
        (Some(original), Some(_), _) if model.crop_session.is_none() => (Some(original.clone()), model.image_size, false),
        (_, Some(image), _) => (Some(image.clone()), model.image_size, true),
        (_, None, Some(placeholder)) => (Some(placeholder.clone()), model.placeholder_size, false),
        _ => (None, Size::ZERO, false),
    };
    let cropped = model.state.crop.size_of(size);
    let border = model.state.border.shown(cropped.width as u32, cropped.height as u32);
    let cropping = model.crop_session.as_ref().map(|s| CropScene {
        photo: s.photo,
        rotation: s.rotation,
        angle: s.angle,
        shown: s.shown(),
        grid: if s.straightening { Some(Grid::Fine) } else if s.adjusting { Some(Grid::Thirds) } else { None },
    });
    let scene = Scene {
        picture,
        developed,
        size,
        rotation: model.rotation(),
        crop: model.state.crop,
        border,
        film: model.state.film.clone(),
        badge: model.badge(),
        zoom_command: model.zoom_command,
        fit: model.fit_image,
        failed: model.develop_error.clone().filter(|_| model.render_failed).map(|reason| {
            let name = model.source.as_deref().map(crate::model::file_name).unwrap_or_else(|| "The photograph".into());
            (name, reason)
        }),
        cropping,
        drop_target: app.drop_target_active,
        frame: if developed { model.shown_frame } else { None },
    };
    let first = scene.developed;
    let hwnd = with_view(|view| {
        let first = first && !view.first_image_presented;
        view.set_scene(scene);
        first.then_some(view.hwnd)
    })
    .flatten();
    // Paint the first developed frame now, not at the next WM_PAINT.
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = UpdateWindow(hwnd);
        }
    }
}

impl View {
    fn scale(&self) -> f64 {
        self.dpi as f64 / 96.0
    }

    /// The part of the canvas the photograph fits and centres in: all of it
    /// (its tools are words in the status line).
    fn viewport(&self) -> (f64, f64) {
        (self.width as f64, (self.height as f64).max(1.0))
    }

    fn display_size(scene: &Scene) -> (f64, f64) {
        let cropped = scene.crop.size_of(scene.size);
        if scene.rotation % 2 != 0 { (cropped.height, cropped.width) } else { (cropped.width, cropped.height) }
    }

    fn document_size(scene: &Scene) -> (f64, f64) {
        let display = Self::display_size(scene);
        match scene.border {
            Some(border) => {
                let (w, h) = border.document(display.0 as u32, display.1 as u32, &scene.film);
                (w as f64, h as f64)
            }
            None => display,
        }
    }

    /// The zoom rules of the Mac's PreviewCanvas: a zoom command applies; a
    /// higher-resolution render of the same photograph keeps the same region
    /// at the same apparent size; a border change keeps the photograph in
    /// place; a different crop refits.
    fn set_scene(&mut self, scene: Scene) {
        let display = Self::display_size(&scene);
        let document = Self::document_size(&scene);
        let old = self.document;
        let commanded = self.last_command != Some(scene.zoom_command);
        let recropped = self.crop != scene.crop && old.0 != 0.0;
        if recropped {
            self.fitting = true;
        }
        if old != document {
            if commanded || self.fitting || old.0 == 0.0 {
                self.document = document;
            } else if self.photo_display != display {
                let fraction = (self.center.0 / old.0, self.center.1 / old.1);
                self.zoom = (self.zoom * old.0 / document.0).clamp(MIN_ZOOM, MAX_ZOOM);
                self.document = document;
                self.center = (fraction.0 * document.0, fraction.1 * document.1);
            } else {
                // Only the border changed: keep the photograph where it is.
                self.document = document;
                self.center = (self.center.0 + (document.0 - old.0) / 2.0, self.center.1 + (document.1 - old.1) / 2.0);
            }
        }
        self.photo_display = display;
        self.crop = scene.crop;
        if commanded {
            self.last_command = Some(scene.zoom_command);
            self.fitting = scene.fit;
            self.apply_zoom();
        } else if (old != document || recropped) && self.fitting {
            self.apply_zoom();
        }
        self.scene = scene;
        self.clamp_center();
        invalidate(self.hwnd);
    }

    fn apply_zoom(&mut self) {
        let (dw, dh) = self.document;
        if dw <= 0.0 || self.width == 0 {
            return;
        }
        self.zoom = zoom_amount(self.fitting, (dw, dh), self.viewport()).clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = (dw / 2.0, dh / 2.0);
    }

    fn clamp_center(&mut self) {
        let (dw, dh) = self.document;
        let viewport = self.viewport();
        let (vw, vh) = (viewport.0 / self.zoom, viewport.1 / self.zoom);
        self.center.0 = if dw <= vw { dw / 2.0 } else { self.center.0.clamp(vw / 2.0, dw - vw / 2.0) };
        self.center.1 = if dh <= vh { dh / 2.0 } else { self.center.1.clamp(vh / 2.0, dh - vh / 2.0) };
    }

    fn zoom_about(&mut self, factor: f64, x: f64, y: f64) {
        if self.document.0 <= 0.0 {
            return;
        }
        let (hx, hy) = (self.viewport().0 / 2.0, self.viewport().1 / 2.0);
        let doc = ((x - hx) / self.zoom + self.center.0, (y - hy) / self.zoom + self.center.1);
        self.fitting = false;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = (doc.0 - (x - hx) / self.zoom, doc.1 - (y - hy) / self.zoom);
        self.clamp_center();
        invalidate(self.hwnd);
    }

    fn scroll(&mut self, dx: f64, dy: f64) {
        self.center = (self.center.0 + dx / self.zoom, self.center.1 + dy / self.zoom);
        self.clamp_center();
        invalidate(self.hwnd);
    }

    /// The document's top-left corner on screen.
    fn document_origin(&self) -> (f64, f64) {
        let (w, h) = self.viewport();
        (w / 2.0 - self.center.0 * self.zoom, h / 2.0 - self.center.1 * self.zoom)
    }

    fn update_color_management(&mut self) {
        if self.gpu.is_none() {
            let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST).0 as isize };
            if monitor != self.monitor {
                self.monitor = monitor;
                self.software_cm = icc::for_window(self.hwnd);
                invalidate(self.hwnd);
            }
            return;
        }
        let gpu = self.gpu.clone().unwrap();
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST).0 as isize };
        if monitor == self.monitor && self.globals.is_some() {
            return;
        }
        self.monitor = monitor;
        let cm = icc::for_window(self.hwnd);
        let lut = match &cm {
            Some(t) => gpu.lut_texture(&t.lut),
            None => gpu.lut_texture(&[[0.0; 3], [1.0; 3]]),
        };
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<gpu::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.globals_bind_group(&buffer, &lut);
        self.globals = Some(Globals { buffer, bind_group, cm });
        invalidate(self.hwnd);
    }

    fn text_size(&mut self, text: &str, size: f64, weight: i32) -> Option<(f64, f64)> {
        if self.gpu.is_some() {
            return self.text(text, size, weight).map(|(_, w, h)| (w, h));
        }
        let mask = crate::text::render(text, (size * self.scale()).round() as i32, weight)?;
        Some((mask.width as f64, mask.height as f64))
    }

    fn text(&mut self, text: &str, size_dip: f64, weight: i32) -> Option<(&GpuImage, f64, f64)> {
        let gpu = self.gpu.clone()?;
        let px = (size_dip * self.scale()).round() as i32;
        let key = (text.to_string(), px, weight);
        if !self.texts.contains_key(&key) {
            let mask = crate::text::render(text, px, weight)?;
            let image = gpu.upload_mask(&mask.coverage, mask.width, mask.height);
            self.texts.insert(key.clone(), image);
        }
        let image = self.texts.get(&key)?;
        let size = image.texture.size();
        Some((image, size.width as f64, size.height as f64))
    }

    fn paint(&mut self) {
        let Some(gpu) = self.gpu.clone() else {
            self.paint_gdi();
            return;
        };
        if self.surface.is_none() || self.globals.is_none() || self.width == 0 || self.height == 0 {
            self.paint_gdi();
            return;
        }
        let surface = self.surface.as_ref().unwrap();
        let frame = match surface.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                surface.surface.configure(&gpu.device, &surface.config);
                match surface.surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(_) => return,
                }
            }
            Err(_) => return,
        };
        let target = frame.texture.create_view(&Default::default());
        let globals = self.globals.as_ref().unwrap();
        let mut uniform = gpu::Globals { viewport: [self.width as f32, self.height as f32, 0.0, 0.0], ..Default::default() };
        if let Some(cm) = &globals.cm {
            uniform.cm = cm.matrix.map(|row| [row[0], row[1], row[2], 0.0]);
            uniform.flags[0] = 1.0;
        }
        gpu.queue.write_buffer(&globals.buffer, 0, bytemuck::bytes_of(&uniform));
        let mut items = vec![];
        self.build(&mut items);
        self.prepare_texts(&items);
        let items = self.resolve(items);
        let photo_drawn = items.iter().any(|item| matches!(item, Item::Photo(..)));
        let globals = self.globals.as_ref().unwrap();
        let clear = background().map(|c| c as f32 / 255.0);
        gpu::draw(&gpu, &target, &globals.bind_group, clear, &items);
        frame.present();
        if photo_drawn {
            self.note_presented("vulkan");
        }
        if photo_drawn && self.scene.developed && !self.first_image_presented {
            self.first_image_presented = true;
            std::thread::spawn(|| {
                super::dwm_flush();
                let at = crate::platform::qpc();
                post(move |app: &mut App| app.report.line(&format!("first_image_on_screen {at}")));
            });
        }
    }

    /// Records a developed frame drawn for the first time, for the render
    /// trace and benchmark, with the time DWM composed it.
    fn note_presented(&mut self, canvas: &'static str) {
        if self.scene.frame.is_none() || self.scene.frame == self.presented {
            return;
        }
        self.presented = self.scene.frame;
        if crate::trace::enabled() {
            let frame = crate::trace::frame_json(self.presented);
            crate::trace::event("present", || serde_json::json!({"frame": frame, "canvas": canvas}));
            std::thread::spawn(move || {
                super::dwm_flush();
                crate::trace::event("composited", || serde_json::json!({"frame": frame, "canvas": canvas}));
            });
        }
    }

    /// Software presentation while Vulkan is unavailable or still starting.
    fn paint_gdi(&mut self) {
        self.update_color_management();
        let mut draws = Vec::new();
        self.build(&mut draws);
        let pixels = software::render(self.width, self.height, self.dpi, &draws, self.software_cm.as_ref(), &mut self.software_cache);
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(self.hwnd, &mut ps);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.width as i32,
                    biHeight: -(self.height as i32),
                    biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                    ..Default::default()
                }, ..Default::default()
            };
            if self.width > 0 && self.height > 0 {
                SetDIBitsToDevice(dc, 0, 0, self.width, self.height, 0, 0, 0, self.height,
                    pixels.as_ptr().cast(), &info, DIB_RGB_COLORS);
                let _ = GdiFlush();
            }
            let _ = EndPaint(self.hwnd, &ps);
        }
        if self.width > 0 && self.height > 0 && self.scene.picture.is_some() {
            self.note_presented("software");
        }
        if self.width > 0 && self.height > 0 && self.scene.developed
            && self.scene.picture.is_some() && !self.first_image_presented {
            self.first_image_presented = true;
            std::thread::spawn(|| {
                super::dwm_flush();
                let at = crate::platform::qpc();
                post(move |app: &mut App| app.report.line(&format!("first_image_on_screen {at}")));
            });
        }
    }
}

/// A draw before its textures are resolved.
enum Draw {
    Photo(Picture, PhotoUniform),
    Fill(QuadInstance),
    Texture(Picture, QuadInstance),
    Text(String, f64, i32, [f32; 4], (f64, f64), bool),
    /// Words set with DirectWrite (Newsreader): the text, its style, the
    /// width it wraps and centres in (DIPs; 0 for one line), colour, place.
    Words(String, TextStyle, f32, [f32; 4], (f64, f64)),
}

/// The canvas's words in the design's type (§4.14).
const TAG: TextStyle = TextStyle::serif(17.0, 400).italic();
const FAILURE: TextStyle = TextStyle::serif(24.0, 600).line_height(1.3);
const REASON: TextStyle = TextStyle::serif(16.0, 400).italic().line_height(1.45);
const BUTTON_WORDS: TextStyle = TextStyle::serif(13.5, 500);
const DROP: TextStyle = TextStyle::serif(22.0, 400).italic();
/// Redraws the tag's running line.
const TIMER_RUNNING: usize = 7;

fn words_key(text: &str, style: &TextStyle, width: f32, dpi: u32) -> (String, String) {
    (text.to_string(), format!("{style:?}|{width}|{dpi}|{}", super::theme::tokens().plain))
}

/// A theme colour (0xRRGGBB) for the GPU.
fn hex(c: u32, alpha: f32) -> [f32; 4] {
    rgba([(c >> 16) as u8, (c >> 8) as u8, c as u8], alpha)
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn rgba(c: [u8; 3], alpha: f32) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, alpha]
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> [f32; 4] {
    [x as f32, y as f32, w as f32, h as f32]
}

/// The crop rectangle in the turned frame of the photograph as shown, from
/// the exporter's pixel rectangle in the unrotated photograph.
fn shown_pixels(crop: &CropSettings, size: Size, rotation: i32) -> (Rect, Size) {
    let mut r = crop.pixels(size);
    let mut s = size;
    for _ in 0..rotation.rem_euclid(4) {
        r = Rect::new(s.height - r.y - r.height, r.x, r.height, r.width);
        s = s.swapped();
    }
    (r, s)
}

/// Crop mode's layout: the photograph's frame, its scale and the dial.
pub struct CropLayout {
    pub frame: Rect,
    pub scale: f64,
    pub dial: Rect,
}

fn crop_layout(width: f64, height: f64, dpi: u32, photo: Size) -> CropLayout {
    let s = dpi as f64 / 96.0;
    let (margin, dial_space) = (CROP_MARGIN * s, DIAL_SPACE * s);
    let available = Size::new((width - 2.0 * margin).max(1.0), (height - 2.0 * margin - dial_space).max(1.0));
    let scale = if photo.width > 0.0 && photo.height > 0.0 { (available.width / photo.width).min(available.height / photo.height) } else { 1.0 };
    let frame = Rect::new(
        (width - photo.width * scale) / 2.0,
        margin + (available.height - photo.height * scale) / 2.0,
        photo.width * scale,
        photo.height * scale,
    );
    let dial_width = (380.0 * s).min((width - 2.0 * margin).max(120.0 * s));
    let dial_height = 58.0 * s;
    let dial = Rect::new((width - dial_width) / 2.0, height - 20.0 * s - dial_height, dial_width, dial_height);
    CropLayout { frame, scale, dial }
}

impl View {
    fn build(&mut self, items: &mut Vec<Draw>) {
        let scene = self.scene.clone();
        let s = self.scale();
        if let Some(picture) = scene.picture.clone() {
            if let Some(cropping) = &scene.cropping {
                self.build_crop(items, picture, cropping);
            } else {
                self.build_photo(items, picture, &scene);
            }
        }
        self.retry = None;
        if scene.cropping.is_none() {
            if let Some((name, reason)) = &scene.failed {
                self.build_failure(items, name, reason);
            } else if scene.drop_target && scene.picture.is_some() {
                self.build_dropping(items);
            }
        }
        match scene.badge.filter(|_| scene.failed.is_none() && !scene.drop_target) {
            Some(badge) => self.build_tag(items, badge),
            None => self.running = None,
        }
        if scene.drop_target && scene.picture.is_none() {
            // Nothing shown yet: a line round the canvas.
            let inset = 12.0 * s;
            let t = super::theme::tokens();
            items.push(Draw::Fill(QuadInstance::rounded(
                rect(inset, inset, self.width as f64 - 2.0 * inset, self.height as f64 - 2.0 * inset),
                hex(t.hex.accent, 1.0),
                (4.0 * s) as f32,
                (1.5 * s) as f32,
            )));
        }
        super::status::set_fitting(self.fitting);
    }

    /// The photograph's rectangle on screen, as drawn (its border included),
    /// clipped to the canvas.
    fn shown_rect(&self) -> Rect {
        let (ox, oy) = self.document_origin();
        let (dw, dh) = self.document;
        let r = Rect::new(ox, oy, dw * self.zoom, dh * self.zoom);
        let (x, y) = (r.x.max(0.0), r.y.max(0.0));
        Rect::new(x, y, (r.max_x().min(self.width as f64) - x).max(0.0), (r.max_y().min(self.height as f64) - y).max(0.0))
    }

    /// Developing, Updating or Loading Original (`DevelopTag`): at the top
    /// centre of the photograph, 16 DIP below its visible top edge, on the
    /// theme's `bg` at 0.78 with a faint `paper` edge, the word in italic
    /// `paper` over a short line where a yellow mark runs every 1.1 s.
    fn build_tag(&mut self, items: &mut Vec<Draw>, title: &str) {
        let s = self.scale();
        let t = super::theme::tokens();
        let Some((tw, th)) = self.words_size(title, &TAG) else { return };
        let line = 64.0 * s;
        let (pad_x, pad_top, pad_bottom, gap) = (16.0 * s, 7.0 * s, 9.0 * s, 5.0 * s);
        let (w, h) = ((tw.max(line) + 2.0 * pad_x).round(), (pad_top + th + gap + s.max(1.0) + pad_bottom).round());
        let photo = self.shown_rect();
        let x = ((photo.mid_x() - w / 2.0).round()).max(0.0);
        let y = (photo.y + 16.0 * s).round();
        let strength = t.shadow_strength() as f32;
        for i in 1..=4 {
            let spread = i as f64 * 2.0 * s;
            items.push(Draw::Fill(QuadInstance::rounded(rect(x - spread, y + 4.0 * s - spread, w + 2.0 * spread, h + 2.0 * spread), [0.0, 0.0, 0.0, 0.1 * strength / i as f32], (3.0 * s + spread) as f32, 0.0)));
        }
        items.push(Draw::Fill(QuadInstance::rounded(rect(x, y, w, h), hex(t.hex.bg, 0.78), (3.0 * s) as f32, 0.0)));
        items.push(Draw::Fill(QuadInstance::rounded(rect(x, y, w, h), hex(t.hex.paper, 0.08), (3.0 * s) as f32, s.max(1.0) as f32)));
        items.push(Draw::Words(title.into(), TAG, 0.0, hex(t.hex.paper, 1.0), ((x + (w - tw) / 2.0).round(), y + pad_top - s)));
        let (lx, ly) = ((x + (w - line) / 2.0).round(), (y + pad_top + th + gap).round());
        let thin = s.max(1.0).round();
        items.push(Draw::Fill(QuadInstance::fill(rect(lx, ly, line, thin), hex(t.hex.track, 1.0))));
        // The mark runs only where the canvas redraws by itself (the GPU);
        // still, a third of the way along, as with Reduce Motion.
        let animate = self.gpu.is_some() && super::motion::enabled();
        let phase = if animate {
            let started = *self.running.get_or_insert_with(std::time::Instant::now);
            (started.elapsed().as_secs_f64() / 1.1).fract()
        } else {
            1.0 / 3.0 / 1.3 + 0.3 / 1.3
        };
        // From -0.3 to 1 of the line's width, clipped to it.
        let start = lx + line * (phase * 1.3 - 0.3);
        let (a, b) = (start.max(lx), (start + line * 0.3).min(lx + line));
        if b > a {
            items.push(Draw::Fill(QuadInstance::fill(rect(a, ly, b - a, thin), hex(t.hex.accent, 1.0))));
        }
        if animate {
            unsafe {
                SetTimer(self.hwnd, TIMER_RUNNING, 33, None);
            }
        }
    }

    /// A photograph that couldn't be developed (`DevelopFailure`): what's
    /// shown fades to the canvas, and over it "NAME couldn’t be developed",
    /// the reason, and Try Again.
    fn build_failure(&mut self, items: &mut Vec<Draw>, name: &str, reason: &str) {
        let s = self.scale();
        let t = super::theme::tokens();
        let photo = self.shown_rect();
        if photo.width > 0.0 {
            items.push(Draw::Fill(QuadInstance::fill(rect(photo.x, photo.y, photo.width, photo.height), hex(t.hex.bg, 0.88))));
        }
        let room = ((self.width as f64 - 120.0 * s) / s).max(120.0) as f32;
        let heading = format!("{name} couldn’t be developed");
        let (Some((hw, hh)), Some((rw, rh))) = (self.words_size_in(&heading, &FAILURE, room), self.words_size_in(reason, &REASON, room)) else { return };
        let button = self.words_size("Try Again", &BUTTON_WORDS).unwrap_or((60.0 * s, 18.0 * s));
        let (bw, bh) = ((button.0 + 24.0 * s).max(76.0 * s).round(), (23.0 * s).round());
        let total = hh + 10.0 * s + rh + 22.0 * s + bh;
        let mut y = ((self.height as f64 - total) / 2.0).round();
        let mid = self.width as f64 / 2.0;
        items.push(Draw::Words(heading, FAILURE, room, hex(t.hex.paper, 1.0), ((mid - hw / 2.0).round(), y)));
        y += hh + 10.0 * s;
        items.push(Draw::Words(reason.into(), REASON, room, hex(t.hex.label, 1.0), ((mid - rw / 2.0).round(), y)));
        y += rh + 22.0 * s;
        let r = Rect::new((mid - bw / 2.0).round(), y, bw, bh);
        let (hot, pressed) = (self.retry_hot, self.retry_pressed && self.retry_hot);
        if hot || pressed {
            items.push(Draw::Fill(QuadInstance::rounded(rect(r.x, r.y, r.width, r.height), hex(t.hex.paper, if pressed { 0.09 } else { 0.04 }), (bh / 2.0) as f32, 0.0)));
        }
        let ring = if hot { t.hex.label } else { t.hex.dim };
        items.push(Draw::Fill(QuadInstance::rounded(rect(r.x, r.y, r.width, r.height), hex(ring, 1.0), (bh / 2.0) as f32, s.max(1.0) as f32)));
        items.push(Draw::Words("Try Again".into(), BUTTON_WORDS, 0.0, hex(t.hex.paper, 1.0), ((r.mid_x() - button.0 / 2.0).round(), (r.mid_y() - button.1 / 2.0).round())));
        self.retry = Some(r);
    }

    /// A photograph dragged over the canvas: the photograph gives way to
    /// "drop to open it" and a yellow line round its place (§4.14).
    fn build_dropping(&mut self, items: &mut Vec<Draw>) {
        let s = self.scale();
        let t = super::theme::tokens();
        let photo = self.shown_rect();
        let o = 12.0 * s;
        items.push(Draw::Fill(QuadInstance::rounded(rect(photo.x - o, photo.y - o, photo.width + 2.0 * o, photo.height + 2.0 * o), hex(t.hex.accent, 1.0), (4.0 * s) as f32, (1.5 * s) as f32)));
        if let Some((w, h)) = self.words_size("drop to open it", &DROP) {
            items.push(Draw::Words("drop to open it".into(), DROP, 0.0, hex(t.hex.paper, 1.0), ((self.width as f64 - w) / 2.0, (self.height as f64 - h) / 2.0)));
        }
    }

    /// A press on Try Again: true if it took it (and wants the capture).
    fn press_retry(&mut self, x: f64, y: f64) -> bool {
        let on = self.retry.is_some_and(|r| r.contains(Point { x, y }));
        if on {
            self.retry_pressed = true;
            self.retry_hot = true;
            invalidate(self.hwnd);
        }
        on
    }

    /// Hot tracking over Try Again.
    fn hover_retry(&mut self, x: f64, y: f64) {
        let hot = self.retry.is_some_and(|r| r.contains(Point { x, y }));
        if hot != self.retry_hot {
            self.retry_hot = hot;
            invalidate(self.hwnd);
            if hot {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut track);
                }
            }
        }
    }

    /// The size of `text` set in `style` on one line, in pixels.
    fn words_size(&mut self, text: &str, style: &TextStyle) -> Option<(f64, f64)> {
        self.words_size_in(text, style, 0.0)
    }

    /// The size of `text` set in `style`, wrapped and centred in `width`
    /// DIPs (0: one line), in pixels.
    fn words_size_in(&mut self, text: &str, style: &TextStyle, width: f32) -> Option<(f64, f64)> {
        let mask = self.words_mask(text, style, width)?;
        Some((mask.0 as f64, mask.1 as f64))
    }

    /// Sets words with DirectWrite, uploading them on the GPU; the software
    /// canvas sets them again when drawing.
    fn words_mask(&mut self, text: &str, style: &TextStyle, width: f32) -> Option<(u32, u32)> {
        let key = words_key(text, style, width, self.dpi);
        if let Some(image) = self.words.get(&key) {
            let size = image.texture.size();
            return Some((size.width, size.height));
        }
        let plain = super::theme::tokens().plain;
        let mask = super::d2d::mask(text, style, width, super::d2d::Align::Center, self.scale() as f32, plain)?;
        let size = (mask.width, mask.height);
        if let Some(gpu) = self.gpu.clone() {
            self.words.insert(key, gpu.upload_mask(&mask.coverage, mask.width, mask.height));
        }
        Some(size)
    }

    fn build_photo(&mut self, items: &mut Vec<Draw>, picture: Picture, scene: &Scene) {
        let z = self.zoom;
        let (ox, oy) = self.document_origin();
        let (dw, dh) = self.document;
        let (pw, ph) = self.photo_display;
        let screen = |x: f64, y: f64, w: f64, h: f64| rect(ox + x * z, oy + y * z, w * z, h * z);
        // The photograph's rectangle in the document.
        let (px, py) = ((dw - pw) / 2.0, (dh - ph) / 2.0);
        if let Some(border) = scene.border {
            let (w, h) = (pw as u32, ph as u32);
            let background = match border.style {
                BorderStyle::Solid | BorderStyle::Keyline => border.color.rgba(),
                _ if border.instagram => WHITE,
                _ => BLACK,
            };
            items.push(Draw::Fill(QuadInstance::fill(screen(0.0, 0.0, dw, dh), background)));
            match border.style {
                BorderStyle::Solid => {}
                BorderStyle::Keyline => {
                    let k = border.keyline(w, h) as f64;
                    items.push(Draw::Fill(QuadInstance::fill(screen(px - k, py - k, pw + 2.0 * k, ph + 2.0 * k), border.line_color.rgba())));
                }
                BorderStyle::Film | BorderStyle::Film35 => {
                    let (cw, ch) = border.canvas(w, h, &scene.film);
                    let (cw, ch) = (cw as f64, ch as f64);
                    items.push(Draw::Fill(QuadInstance::fill(screen((dw - cw) / 2.0, (dh - ch) / 2.0, cw, ch), BLACK)));
                }
            }
        }
        // The photograph through its crop.
        let (clip, shown) = shown_pixels(&scene.crop, scene.size, scene.rotation);
        let origin = (ox + px * z - clip.x * z, oy + py * z - clip.y * z);
        let uniform = PhotoUniform {
            origin_scale: [origin.0 as f32, origin.1 as f32, z as f32, scene.crop.angle.to_radians() as f32],
            shown: [shown.width as f32, shown.height as f32, scene.rotation.rem_euclid(4) as f32, 0.0],
            clip: [clip.x as f32, clip.y as f32, clip.width as f32, clip.height as f32],
            quad: screen(px, py, pw, ph),
            extra: [1.0, picture.wide as u8 as f32, 0.0, 0.0],
        };
        items.push(Draw::Photo(picture, uniform));
        // Film frame and 35mm strip over the photograph's edge.
        let Some(border) = scene.border else { return };
        let (w, h) = (pw as u32, ph as u32);
        match border.style {
            BorderStyle::Film => {
                let Some(texture) = crate::picture::borders().and_then(|b| b.film.clone()) else { return };
                let (inset, pad) = spektrafilm_io::border::film_geometry(w, h);
                let (inset, pad) = (inset as f64, pad as f64);
                let portrait = ph > pw;
                let canvas = (pw + 2.0 * inset, ph + 2.0 * inset);
                let landscape = if portrait { (canvas.1, canvas.0) } else { canvas };
                let (tw, th) = (texture.width as f64, texture.height as f64);
                let scale = landscape.0.min(landscape.1) / tw.min(th);
                let (u, v) = (pad / scale / tw, pad / scale / th);
                let xs = [0.0, pad, landscape.0 - pad, landscape.0];
                let ys = [0.0, pad, landscape.1 - pad, landscape.1];
                let us = [0.0, u, 1.0 - u, 1.0];
                let vs = [0.0, v, 1.0 - v, 1.0];
                let (cx, cy) = ((dw - canvas.0) / 2.0, (dh - canvas.1) / 2.0);
                for i in 0..3 {
                    for j in 0..3 {
                        let (x, y, qw, qh) = (xs[i], ys[j], xs[i + 1] - xs[i], ys[j + 1] - ys[j]);
                        // Portrait photographs turn the landscape frame anticlockwise.
                        let (dx, dy, ddw, ddh) = if portrait { (y, landscape.0 - x - qw, qh, qw) } else { (x, y, qw, qh) };
                        let quad = QuadInstance::image(screen(cx + dx, cy + dy, ddw, ddh), [us[i] as f32, vs[j] as f32, us[i + 1] as f32, vs[j + 1] as f32], portrait);
                        items.push(Draw::Texture(texture.clone(), quad));
                    }
                }
            }
            BorderStyle::Film35 => {
                let Some(format) = FilmFormat::of(w, h) else { return };
                let Some(texture) = crate::picture::strip(&scene.film, format) else { return };
                let (cw, ch) = border.canvas(w, h, &scene.film);
                let (cw, ch) = (cw as f64, ch as f64);
                let turned = crate::border::strip_turned(&scene.film, w, h);
                items.push(Draw::Texture(texture, QuadInstance::image(screen((dw - cw) / 2.0, (dh - ch) / 2.0, cw, ch), [0.0, 0.0, 1.0, 1.0], turned)));
            }
            _ => {}
        }
    }

    fn build_crop(&mut self, items: &mut Vec<Draw>, picture: Picture, cropping: &CropScene) {
        let s = self.scale();
        let layout = crop_layout(self.width as f64, self.height as f64, self.dpi, cropping.photo);
        let (frame, scale) = (layout.frame, layout.scale);
        let shown = cropping.shown;
        let crop_rect = Rect::new(frame.x + shown.x * scale, frame.y + shown.y * scale, shown.width * scale, shown.height * scale);
        let bounds = crop::bounds(cropping.photo, cropping.angle);
        let uniform = PhotoUniform {
            origin_scale: [frame.x as f32, frame.y as f32, scale as f32, cropping.angle.to_radians() as f32],
            shown: [cropping.photo.width as f32, cropping.photo.height as f32, cropping.rotation.rem_euclid(4) as f32, 1.0],
            clip: [shown.x as f32, shown.y as f32, shown.width as f32, shown.height as f32],
            quad: rect(frame.x + bounds.x * scale - 1.0, frame.y + bounds.y * scale - 1.0, bounds.width * scale + 2.0, bounds.height * scale + 2.0),
            extra: [0.45, picture.wide as u8 as f32, 0.0, 0.0],
        };
        items.push(Draw::Photo(picture, uniform));
        let c = crop_rect;
        if let Some(grid) = cropping.grid {
            let lines = if grid == Grid::Thirds { 3 } else { 9 };
            let alpha = if grid == Grid::Thirds { 0.6 } else { 0.4 };
            let thin = (0.5 * s).max(1.0);
            for i in 1..lines {
                let x = c.x + c.width * i as f64 / lines as f64;
                let y = c.y + c.height * i as f64 / lines as f64;
                items.push(Draw::Fill(QuadInstance::fill(rect(x - thin / 2.0, c.y, thin, c.height), [1.0, 1.0, 1.0, alpha])));
                items.push(Draw::Fill(QuadInstance::fill(rect(c.x, y - thin / 2.0, c.width, thin), [1.0, 1.0, 1.0, alpha])));
            }
        }
        // Outline, then corner brackets just outside the crop, with a soft shadow.
        let line = (1.0 * s).max(1.0);
        items.push(Draw::Fill(QuadInstance::rounded(rect(c.x - line, c.y - line, c.width + 2.0 * line, c.height + 2.0 * line), [1.0, 1.0, 1.0, 0.9], 0.0, line as f32)));
        let length = (22.0 * s).min(c.width / 2.0).min(c.height / 2.0);
        let width = 3.0 * s;
        let outer = c.inset(-width / 2.0, -width / 2.0);
        let mut brackets = vec![];
        for (x, y, dx, dy) in [(outer.x, outer.y, 1.0, 1.0), (outer.max_x(), outer.y, -1.0, 1.0), (outer.x, outer.max_y(), 1.0, -1.0), (outer.max_x(), outer.max_y(), -1.0, -1.0)] {
            let horizontal = Rect::new(if dx > 0.0 { x - width / 2.0 } else { x - length }, y - width / 2.0, length + width / 2.0, width);
            let vertical = Rect::new(x - width / 2.0, if dy > 0.0 { y - width / 2.0 } else { y - length }, width, length + width / 2.0);
            brackets.push(horizontal);
            brackets.push(vertical);
        }
        for b in &brackets {
            items.push(Draw::Fill(QuadInstance::rounded(rect(b.x - s, b.y - s, b.width + 2.0 * s, b.height + 2.0 * s), [0.0, 0.0, 0.0, 0.25], (1.5 * s) as f32, 0.0)));
        }
        for b in &brackets {
            items.push(Draw::Fill(QuadInstance::fill(rect(b.x, b.y, b.width, b.height), WHITE)));
        }
        self.build_dial(items, &layout, cropping.angle);
    }

    /// A ruler to turn the photograph by degrees, like the iPhone's.
    fn build_dial(&mut self, items: &mut Vec<Draw>, layout: &CropLayout, angle: f64) {
        let s = self.scale();
        let d = layout.dial;
        items.push(Draw::Fill(QuadInstance::rounded(rect(d.x, d.y, d.width, d.height), [0.97, 0.97, 0.97, 0.92], (12.0 * s) as f32, 0.0)));
        items.push(Draw::Fill(QuadInstance::rounded(rect(d.x, d.y, d.width, d.height), [0.0, 0.0, 0.0, 0.15], (12.0 * s) as f32, 1.0)));
        let label = if angle == 0.0 { "0°".to_string() } else { format!("{angle:.1}°") };
        let color = if angle == 0.0 { [0.45, 0.45, 0.45, 1.0] } else { [0.1, 0.1, 0.1, 1.0] };
        if let Some((tw, _)) = self.text_size(&label, 13.0, 600) {
            items.push(Draw::Text(label, 13.0, 600, color, (d.x + (d.width - tw) / 2.0, d.y + 6.0 * s), false));
        }
        let ruler = Rect::new(d.x + 14.0 * s, d.y + 28.0 * s, d.width - 28.0 * s, 22.0 * s);
        let mid = ruler.x + ruler.width / 2.0;
        let half = ruler.width / 2.0;
        for degree in -45..=45 {
            let x = mid + (degree as f64 - angle) * DIAL_SPACING * s;
            if x < ruler.x || x > ruler.max_x() {
                continue;
            }
            let major = degree % 5 == 0;
            let height = if degree == 0 { ruler.height } else if major { ruler.height * 0.7 } else { ruler.height * 0.4 };
            let fade = 1.0 - ((x - mid).abs() / half).powi(2);
            let alpha = (if major { 0.75 } else { 0.4 }) * fade;
            let width = if degree == 0 { 1.5 * s } else { 1.0 * s.max(1.0) };
            items.push(Draw::Fill(QuadInstance::fill(rect(x - width / 2.0, ruler.y + (ruler.height - height) / 2.0, width, height), [0.0, 0.0, 0.0, alpha as f32])));
        }
        items.push(Draw::Fill(QuadInstance::rounded(rect(mid - s, ruler.y, 2.0 * s, ruler.height), rgba(accent_color(), 1.0), s as f32, 0.0)));
    }

    /// Rasterises the texts a frame needs before it borrows the cache.
    fn prepare_texts(&mut self, draws: &[Draw]) {
        for draw in draws {
            match draw {
                Draw::Text(text, size, weight, ..) => {
                    let _ = self.text(text, *size, *weight);
                }
                Draw::Words(text, style, width, ..) => {
                    let _ = self.words_mask(text, style, *width);
                }
                _ => {}
            }
        }
    }

    /// Turns pictures and text into GPU images; draws whose texture isn't
    /// ready yet are left out.
    fn resolve(&self, draws: Vec<Draw>) -> Vec<Item<'_>> {
        let px = |size: f64, s: f64| (size * s).round() as i32;
        let s = self.scale();
        let texts = &self.texts;
        let words = &self.words;
        let mut items = vec![];
        for draw in draws {
            match draw {
                Draw::Photo(picture, uniform) => {
                    if let Some(image) = picture_texture(&picture) {
                        items.push(Item::Photo(image, uniform));
                    }
                }
                Draw::Fill(quad) => items.push(Item::Quad(None, quad)),
                Draw::Texture(picture, quad) => {
                    if let Some(image) = picture_texture(&picture) {
                        items.push(Item::Quad(Some(image), quad));
                    }
                }
                Draw::Text(text, size, weight, color, (x, y), _) => {
                    if let Some(image) = texts.get(&(text, px(size, s), weight)) {
                        let size = image.texture.size();
                        items.push(Item::Quad(Some(image), QuadInstance::text(rect(x.round(), y.round(), size.width as f64, size.height as f64), color)));
                    }
                }
                Draw::Words(text, style, width, color, (x, y)) => {
                    if let Some(image) = words.get(&words_key(&text, &style, width, self.dpi)) {
                        let size = image.texture.size();
                        items.push(Item::Quad(Some(image), QuadInstance::text(rect(x.round(), y.round(), size.width as f64, size.height as f64), color)));
                    }
                }
            }
        }
        items
    }
}

/// The picture's texture, borrowed for one frame. Pictures live in the
/// scene until the next frame is built, which outlives the draw.
fn picture_texture<'a>(picture: &Picture) -> Option<&'a GpuImage> {
    // SAFETY: the scene holds a clone of every picture it draws, and the
    // scene is only replaced between frames, so the texture outlives `draw`.
    picture.texture().map(|t| unsafe { &*(t as *const GpuImage) })
}

/// The Windows accent colour, for the drop outline and the dial's needle.
pub fn accent_color() -> [u8; 3] {
    super::theme::accent_rgb()
}

fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32)
}

/// Crop mode: the handle or the dial under the pointer.
fn crop_target(app: &App, x: f64, y: f64) -> Option<CropTarget> {
    let session = app.model.crop_session.as_ref()?;
    let (w, h, dpi) = with_view(|v| (v.width as f64, v.height as f64, v.dpi))?;
    let layout = crop_layout(w, h, dpi, session.photo);
    let shown = session.shown();
    let frame = Rect::new(layout.frame.x + shown.x * layout.scale, layout.frame.y + shown.y * layout.scale, shown.width * layout.scale, shown.height * layout.scale);
    let p = Point { x, y };
    if layout.dial.contains(p) {
        return Some(CropTarget::Dial);
    }
    crop::handle_at(p, frame, dpi as f64 / 96.0).map(|h| CropTarget::Handle(h, layout.scale, dpi))
}

enum CropTarget {
    Handle(Handle, f64, u32),
    Dial,
}

fn cursor_for(target: Option<&CropTarget>) -> PCWSTRCursor {
    match target {
        Some(CropTarget::Dial) => IDC_SIZEWE,
        Some(CropTarget::Handle(handle, ..)) => match (handle.x, handle.y) {
            (0, 0) => IDC_SIZEALL,
            (-1, -1) | (1, 1) => IDC_SIZENWSE,
            (1, -1) | (-1, 1) => IDC_SIZENESW,
            (_, 0) => IDC_SIZEWE,
            _ => IDC_SIZENS,
        },
        None => IDC_ARROW,
    }
}

type PCWSTRCursor = windows::core::PCWSTR;

fn set_cursor(cursor: PCWSTRCursor) {
    unsafe {
        let _ = SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
    }
}

fn crop_mouse_down(app: &mut App, x: f64, y: f64, double: bool) -> bool {
    let Some(target) = crop_target(app, x, y) else { return false };
    let Some(session) = app.model.crop_session.as_mut() else { return false };
    match target {
        CropTarget::Dial => {
            if double {
                session.begin_straightening();
                session.straighten(0.0);
                session.end_straightening();
            } else {
                session.begin_straightening();
                let angle = session.angle;
                with_view(|v| v.crop_drag = Some(CropDrag::Dial(angle, x as i32)));
            }
        }
        CropTarget::Handle(handle, _, _) => {
            let start = session.shown();
            session.adjusting = true;
            with_view(|v| v.crop_drag = Some(CropDrag::Handle(handle, start, Point { x, y })));
        }
    }
    app.model.crop_changed();
    true
}

fn crop_mouse_move(app: &mut App, x: f64, y: f64) {
    let Some(drag) = with_view(|v| v.crop_drag).flatten() else {
        let target = crop_target(app, x, y);
        set_cursor(cursor_for(target.as_ref()));
        return;
    };
    let Some((w, h, dpi)) = with_view(|v| (v.width as f64, v.height as f64, v.dpi)) else { return };
    let Some(session) = app.model.crop_session.as_mut() else { return };
    let s = dpi as f64 / 96.0;
    match drag {
        CropDrag::Dial(start, x0) => {
            let mut next = start - (x - x0 as f64) / (DIAL_SPACING * s);
            // Catch at zero.
            if next.abs() < 0.4 {
                next = 0.0;
            }
            session.straighten(next);
            set_cursor(IDC_SIZEWE);
        }
        CropDrag::Handle(handle, start, p0) => {
            let layout = crop_layout(w, h, dpi, session.photo);
            let offset = ((x - p0.x) / layout.scale, (y - p0.y) / layout.scale);
            let (photo, angle, bounds) = (session.photo, session.angle, session.bounds());
            if handle == Handle::MOVE {
                // One axis at a time, so the crop slides along the turned photograph's edges.
                let across = crop::constrain(start, crop::move_rect(start, offset.0, 0.0, bounds), angle, photo);
                let shown = crop::constrain(across, crop::move_rect(across, 0.0, offset.1, bounds), angle, photo);
                session.set_shown(shown);
            } else {
                // Follow the edge being dragged, not the pointer's distance from it.
                let edge = Point { x: if handle.x < 0 { start.x } else { start.max_x() }, y: if handle.y < 0 { start.y } else { start.max_y() } };
                let resized = crop::resize(start, handle, Point { x: edge.x + offset.0, y: edge.y + offset.1 }, session.ratio(), bounds, 40.0 * s / layout.scale);
                session.set_shown(crop::constrain(start, resized, angle, photo));
            }
            set_cursor(cursor_for(Some(&CropTarget::Handle(handle, 0.0, dpi))));
        }
    }
    app.model.crop_changed();
}


fn crop_mouse_up(app: &mut App) {
    let Some(drag) = with_view(|v| v.crop_drag.take()).flatten() else { return };
    if let Some(session) = app.model.crop_session.as_mut() {
        match drag {
            CropDrag::Dial(..) => session.end_straightening(),
            CropDrag::Handle(..) => session.adjusting = false,
        }
    }
    app.model.crop_changed();
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                // Busy view (a nested call): leave the region invalid for later.
                if with_view(|v| v.paint()).is_some() {
                    let _ = ValidateRect(hwnd, None);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                resized(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_RUNNING => {
                let _ = KillTimer(hwnd, TIMER_RUNNING);
                // The next frame sets it again while the tag shows.
                if with_view(|v| v.running.is_some()).unwrap_or(false) {
                    invalidate(hwnd);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let _ = SetFocus(hwnd);
                let (x, y) = point(lparam);
                if with_view(|v| v.press_retry(x as f64, y as f64)).unwrap_or(false) {
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                let double = msg == WM_LBUTTONDBLCLK;
                let cropping = with_app(|app| app.model.cropping()).unwrap_or(false);
                if cropping {
                    if with_app(|app| crop_mouse_down(app, x as f64, y as f64, double)).unwrap_or(false) {
                        SetCapture(hwnd);
                    }
                } else if double {
                    with_view(|v| {
                        v.fitting = true;
                        v.apply_zoom();
                        invalidate(hwnd);
                    });
                } else {
                    with_view(|v| v.pan = Some((x, y, v.center)));
                    SetCapture(hwnd);
                    set_cursor(IDC_SIZEALL);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = point(lparam);
                let panned = with_view(|v| {
                    let Some((x0, y0, center)) = v.pan else { return false };
                    v.center = (center.0 - (x - x0) as f64 / v.zoom, center.1 - (y - y0) as f64 / v.zoom);
                    v.clamp_center();
                    invalidate(hwnd);
                    true
                })
                .unwrap_or(false);
                if !panned {
                    with_view(|v| v.hover_retry(x as f64, y as f64));
                    with_app(|app| {
                        if app.model.cropping() {
                            crop_mouse_move(app, x as f64, y as f64);
                        }
                    });
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_view(|v| {
                    if std::mem::take(&mut v.retry_hot) {
                        invalidate(hwnd);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                let (x, y) = point(lparam);
                let clicked = with_view(|v| {
                    v.pan = None;
                    let pressed = std::mem::take(&mut v.retry_pressed);
                    if pressed {
                        invalidate(hwnd);
                    }
                    // Try Again acts when the button comes up over it.
                    pressed && msg == WM_LBUTTONUP && v.retry.is_some_and(|r| r.contains(Point { x: x as f64, y: y as f64 }))
                })
                .unwrap_or(false);
                with_app(crop_mouse_up);
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                if clicked {
                    if with_app(|app| app.model.retry_develop()).is_none() {
                        post(|app: &mut App| app.model.retry_develop());
                    }
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if (lparam.0 & 0xffff) as u32 == HTCLIENT {
                    let cropping = with_app(|app| app.model.cropping()).unwrap_or(false);
                    if !cropping {
                        let panning = with_view(|v| v.pan.is_some()).unwrap_or(false);
                        set_cursor(if panning { IDC_SIZEALL } else { IDC_ARROW });
                    }
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f64;
                let keys = (wparam.0 & 0xffff) as u32;
                let mut p = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                let cropping = with_app(|app| app.model.cropping()).unwrap_or(false);
                if cropping {
                    return LRESULT(0);
                }
                with_view(|v| {
                    if msg == WM_MOUSEWHEEL && keys & 0x0008 != 0 {
                        // Ctrl+wheel (and a touchpad pinch) zooms about the pointer.
                        v.zoom_about((delta / 120.0 * 0.2).exp(), p.x as f64, p.y as f64);
                    } else {
                        let step = delta / 120.0 * 60.0 * v.scale();
                        if msg == WM_MOUSEHWHEEL || keys & 0x0004 != 0 {
                            v.scroll(if msg == WM_MOUSEHWHEEL { step } else { -step }, 0.0);
                        } else {
                            v.scroll(0.0, -step);
                        }
                    }
                });
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_KEYDOWN => {
                let step = 40.0;
                with_view(|v| {
                    let s = step * v.scale();
                    match VIRTUAL_KEY(wparam.0 as u16) {
                        VK_LEFT => v.scroll(-s, 0.0),
                        VK_RIGHT => v.scroll(s, 0.0),
                        VK_UP => v.scroll(0.0, -s),
                        VK_DOWN => v.scroll(0.0, s),
                        _ => {}
                    }
                });
                LRESULT(0)
            }
            WM_SETFOCUS | WM_KILLFOCUS => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fit shrinks a large photograph to the view but leaves a small one at 1:1.
    #[test]
    fn fit_never_enlarges_past_one_to_one() {
        assert_eq!(zoom_amount(true, (4000.0, 3000.0), (1000.0, 1000.0)), 0.25);
        assert_eq!(zoom_amount(true, (400.0, 300.0), (1000.0, 1000.0)), 1.0);
        assert_eq!(zoom_amount(true, (1000.0, 500.0), (1000.0, 1000.0)), 1.0);
        assert_eq!(zoom_amount(false, (4000.0, 3000.0), (1000.0, 1000.0)), 1.0);
    }

    /// The canvas cuts the exporter's pixels, then turns them.
    #[test]
    fn shown_crop_turns_the_exporters_pixels() {
        let crop = CropSettings { rect: Rect::new(0.1, 0.2, 0.5, 0.25), ..Default::default() };
        let size = Size::new(1000.0, 800.0);
        let (r, s) = shown_pixels(&crop, size, 0);
        assert_eq!((r, s), (Rect::new(100.0, 160.0, 500.0, 200.0), size));
        let (r, s) = shown_pixels(&crop, size, 1);
        assert_eq!((r, s), (Rect::new(440.0, 100.0, 200.0, 500.0), Size::new(800.0, 1000.0)));
        let (r, _) = shown_pixels(&crop, size, 4);
        assert_eq!(r, Rect::new(100.0, 160.0, 500.0, 200.0));
    }


    #[test]
    fn crop_layout_fits_the_photograph_above_the_dial() {
        let layout = crop_layout(1000.0, 800.0, 96, Size::new(600.0, 400.0));
        assert!((layout.scale - (1000.0 - 72.0) / 600.0).min((800.0 - 72.0 - 76.0) / 400.0) .abs() < 1e-9 || layout.scale > 0.0);
        assert!(layout.frame.max_y() <= layout.dial.y);
        assert!((layout.frame.mid_x() - 500.0).abs() < 1e-9);
    }
}
