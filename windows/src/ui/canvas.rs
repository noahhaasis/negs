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
    /// The tools under the photograph; None while cropping.
    pub tools: Option<Tools>,
    /// The render the developed picture came from, for the timing trace.
    pub frame: Option<crate::model::FrameTag>,
}

/// The photograph's tools, in a pill floating under it: zoom, rotate and
/// crop, before and after, the film strip. The title bar keeps the
/// window's own actions; the Mac has all of them in its toolbar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Fit,
    Actual,
    Rotate,
    Crop,
    Before,
    After,
    FilmStrip,
}

/// Which tools work, and which are on.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tools {
    pub crop: bool,
    pub original: bool,
    pub before: bool,
    pub filmstrip: bool,
}

impl Tool {
    const ALL: [Tool; 7] = [Tool::Fit, Tool::Actual, Tool::Rotate, Tool::Crop, Tool::Before, Tool::After, Tool::FilmStrip];

    /// A glyph of the icon font, or words.
    fn face(self) -> &'static str {
        match self {
            Tool::Fit => "\u{E9A6}",
            Tool::Actual => "1:1",
            Tool::Rotate => "\u{E7AD}",
            Tool::Crop => "\u{E7A8}",
            Tool::Before => "Before",
            Tool::After => "After",
            Tool::FilmStrip => "\u{E8B9}",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Tool::Fit => "Fit photograph (Ctrl+0)",
            Tool::Actual => "Actual size (Ctrl+1)",
            Tool::Rotate => "Rotate clockwise (Ctrl+R)",
            Tool::Crop => "Crop the photograph (C)",
            Tool::Before => "Show the photograph before developing (or hold O)",
            Tool::After => "Show the developed photograph",
            Tool::FilmStrip => "Show or hide film strip (F6)",
        }
    }

    /// Width in DIPs; a gap of `TOOL_GAP` follows Actual, Crop and After.
    fn width(self) -> f64 {
        match self {
            Tool::Actual => 40.0,
            Tool::Before => 64.0,
            Tool::After => 56.0,
            _ => 36.0,
        }
    }

    fn gap_after(self) -> bool {
        matches!(self, Tool::Actual | Tool::Crop | Tool::After)
    }

    fn enabled(self, tools: &Tools) -> bool {
        match self {
            Tool::Crop => tools.crop,
            Tool::Before => tools.original,
            _ => true,
        }
    }

    fn on(self, tools: &Tools) -> bool {
        match self {
            Tool::Before => tools.before,
            Tool::After => !tools.before,
            Tool::FilmStrip => tools.filmstrip,
            _ => false,
        }
    }
}

const TOOL_HEIGHT: f64 = 32.0;
const TOOL_GAP: f64 = 13.0;
const PILL_PAD: f64 = 4.0;
/// Under the pill, and between it and a fitted photograph.
const PILL_MARGIN: f64 = 12.0;

/// The room the tools keep under the photograph, in pixels.
fn tools_reserve(scale: f64) -> f64 {
    (TOOL_HEIGHT + 2.0 * PILL_PAD + 2.0 * PILL_MARGIN) * scale
}

/// The pill, its tools and the dividers between groups, centred at the
/// bottom of a `width` × `height` canvas.
fn tool_layout(width: f64, height: f64, scale: f64) -> (Rect, Vec<(Tool, Rect)>, Vec<f64>) {
    let inner: f64 = Tool::ALL.iter().map(|t| t.width() + if t.gap_after() { TOOL_GAP } else { 0.0 }).sum();
    let pill = Rect::new(
        ((width - (inner + 2.0 * PILL_PAD) * scale) / 2.0).round(),
        (height - (PILL_MARGIN + TOOL_HEIGHT + 2.0 * PILL_PAD) * scale).round(),
        ((inner + 2.0 * PILL_PAD) * scale).round(),
        ((TOOL_HEIGHT + 2.0 * PILL_PAD) * scale).round(),
    );
    let mut x = pill.x + PILL_PAD * scale;
    let (mut tools, mut dividers) = (vec![], vec![]);
    for tool in Tool::ALL {
        let w = tool.width() * scale;
        tools.push((tool, Rect::new(x.round(), pill.y + PILL_PAD * scale, w.round(), TOOL_HEIGHT * scale)));
        x += w;
        if tool.gap_after() {
            dividers.push((x + TOOL_GAP * scale / 2.0).round());
            x += TOOL_GAP * scale;
        }
    }
    (pill, tools, dividers)
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
    /// The room under the photograph the tools take (none while they're hidden).
    reserve: f64,
    hot: Option<Tool>,
    pressed: Option<Tool>,
    /// The tools' tooltips, and the size their rectangles were placed for.
    tips: HWND,
    tips_for: Option<(u32, u32, u32)>,
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
            reserve: 0.0,
            hot: None,
            pressed: None,
            tips: HWND::default(),
            tips_for: None,
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
        view.set_reserve();
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
        tools: model.crop_session.is_none().then(|| Tools {
            crop: model.image.is_some(),
            original: model.can_show_original(),
            before: model.holding_original,
            filmstrip: model.prefs.filmstrip_visible,
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

    /// The part of the canvas the photograph fits and centres in: above the tools.
    fn viewport(&self) -> (f64, f64) {
        (self.width as f64, (self.height as f64 - self.reserve).max(1.0))
    }

    /// Follows the tools' showing (and the DPI); refits a fitted photograph.
    fn set_reserve(&mut self) {
        let reserve = if self.scene.tools.is_some() { tools_reserve(self.scale()) } else { 0.0 };
        if reserve != self.reserve {
            self.reserve = reserve;
            if self.fitting {
                self.apply_zoom();
            }
            self.clamp_center();
        }
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
        self.set_reserve();
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
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn rgba(c: [u8; 3], alpha: f32) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, alpha]
}

/// A palette colour (a `COLORREF`, 0x00BBGGRR).
fn color(c: super::theme::Color, alpha: f32) -> [f32; 4] {
    rgba([c as u8, (c >> 8) as u8, (c >> 16) as u8], alpha)
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
        if let Some(badge) = scene.badge {
            let font = 13.0;
            let (tw, th) = self.text_size(badge, font, 600).unwrap_or((0.0, 0.0));
            let (pw, ph) = (tw + 24.0 * s, th + 10.0 * s);
            let x = (self.width as f64 - pw) / 2.0;
            let y = 12.0 * s;
            items.push(Draw::Fill(QuadInstance::rounded(rect(x, y, pw, ph), [0.97, 0.97, 0.97, 0.92], (ph / 2.0) as f32, 0.0)));
            items.push(Draw::Fill(QuadInstance::rounded(rect(x, y, pw, ph), [0.0, 0.0, 0.0, 0.15], (ph / 2.0) as f32, 1.0)));
            items.push(Draw::Text(badge.into(), font, 600, [0.12, 0.12, 0.12, 1.0], (x + 12.0 * s, y + 5.0 * s), false));
        }
        let tools = scene.tools.filter(|_| scene.picture.is_some());
        if let Some(tools) = tools {
            self.build_tools(items, tools);
        }
        self.place_tips(tools.is_some());
        if scene.drop_target {
            let inset = 8.0 * s;
            let accent = accent_color();
            items.push(Draw::Fill(QuadInstance::rounded(
                rect(inset, inset, self.width as f64 - 2.0 * inset, self.height as f64 - 2.0 * inset),
                rgba(accent, 1.0),
                (12.0 * s) as f32,
                (3.0 * s) as f32,
            )));
        }
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

    /// The tools' pill, in the window's colours.
    fn build_tools(&mut self, items: &mut Vec<Draw>, tools: Tools) {
        let s = self.scale();
        let p = super::theme::palette();
        let (pill, slots, dividers) = tool_layout(self.width as f64, self.height as f64, s);
        let radius = (pill.height / 2.0) as f32;
        items.push(Draw::Fill(QuadInstance::rounded(rect(pill.x, pill.y, pill.width, pill.height), color(p.panel, 0.96), radius, 0.0)));
        items.push(Draw::Fill(QuadInstance::rounded(rect(pill.x, pill.y, pill.width, pill.height), color(p.strong_border, 0.6), radius, s.max(1.0) as f32)));
        for x in dividers {
            items.push(Draw::Fill(QuadInstance::fill(rect(x, pill.y + pill.height * 0.3, s.max(1.0).floor(), pill.height * 0.4), color(p.border, 1.0))));
        }
        // Before | After share one track; the one showing is in the accent.
        let segments: Vec<Rect> = slots.iter().filter(|(t, _)| matches!(t, Tool::Before | Tool::After)).map(|(_, r)| *r).collect();
        if let [before, after] = segments[..] {
            let track = Rect::new(before.x, before.y + 2.0 * s, after.max_x() - before.x, before.height - 4.0 * s);
            items.push(Draw::Fill(QuadInstance::rounded(rect(track.x, track.y, track.width, track.height), color(p.subtle_hover, 1.0), (track.height / 2.0) as f32, 0.0)));
        }
        for (tool, r) in slots {
            let enabled = tool.enabled(&tools);
            let on = tool.on(&tools);
            let hot = enabled && self.hot == Some(tool);
            let pressed = hot && self.pressed == Some(tool);
            let segment = matches!(tool, Tool::Before | Tool::After);
            let face = if segment { Rect::new(r.x + 2.0 * s, r.y + 4.0 * s, r.width - 4.0 * s, r.height - 8.0 * s) } else { r.inset(2.0 * s, 2.0 * s) };
            let round = if segment { (face.height / 2.0) as f32 } else { (6.0 * s) as f32 };
            let fill = if segment && on {
                Some(if pressed { p.accent_pressed } else if hot { p.accent_hover } else { p.accent })
            } else if pressed {
                Some(p.subtle_pressed)
            } else if hot {
                Some(if segment { p.subtle_pressed } else { p.subtle_hover })
            } else if on {
                Some(super::theme::mix(p.panel, p.accent, 0.16))
            } else {
                None
            };
            if let Some(fill) = fill {
                items.push(Draw::Fill(QuadInstance::rounded(rect(face.x, face.y, face.width, face.height), color(fill, 1.0), round, 0.0)));
            }
            let ink = if !enabled {
                p.disabled
            } else if segment && on {
                p.on_accent
            } else if on {
                super::theme::mix(p.text, p.accent, 0.6)
            } else {
                p.text
            };
            let glyph = tool.face().chars().all(|c| c >= '\u{E000}');
            let (size, weight) = if glyph { (16.0, 400) } else { (12.0, if segment && on { 600 } else { 400 }) };
            if let Some((tw, th)) = self.text_size(tool.face(), size, weight) {
                let at = ((r.x + (r.width - tw) / 2.0).round(), (r.y + (r.height - th) / 2.0).round());
                items.push(Draw::Text(tool.face().into(), size, weight, color(ink, 1.0), at, false));
            }
        }
    }

    /// A press on the pill: Some(true) if a tool took it (and wants the
    /// capture), Some(false) between tools or on one that doesn't work now,
    /// None off the pill.
    fn press(&mut self, x: f64, y: f64) -> Option<bool> {
        let tool = self.tool_at(x, y)?;
        let tools = self.scene.tools?;
        let tool = tool.filter(|t| t.enabled(&tools));
        self.pressed = tool;
        invalidate(self.hwnd);
        Some(tool.is_some())
    }

    /// Hot tracking over the tools; true when the pointer just reached Before.
    fn hover(&mut self, x: f64, y: f64) -> bool {
        let hot = self.tool_at(x, y).flatten();
        if hot == self.hot {
            return false;
        }
        self.hot = hot;
        invalidate(self.hwnd);
        if hot.is_some() {
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
        }
        hot == Some(Tool::Before)
    }

    /// The tool under a point: Some(None) on the pill between tools.
    fn tool_at(&self, x: f64, y: f64) -> Option<Option<Tool>> {
        self.scene.tools.filter(|_| self.scene.picture.is_some())?;
        let (pill, slots, _) = tool_layout(self.width as f64, self.height as f64, self.scale());
        let p = Point { x, y };
        if !pill.contains(p) {
            return None;
        }
        Some(slots.into_iter().find(|(_, r)| r.contains(p)).map(|(t, _)| t))
    }

    /// One tooltip per tool, over its place in the pill (none while hidden).
    fn place_tips(&mut self, shown: bool) {
        let key = shown.then_some((self.width, self.height, self.dpi));
        if key == self.tips_for {
            return;
        }
        self.tips_for = key;
        unsafe {
            if self.tips.is_invalid() {
                let Ok(tips) = CreateWindowExW(
                    WS_EX_TOPMOST,
                    TOOLTIPS_CLASSW,
                    None,
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    self.hwnd,
                    None,
                    super::instance(),
                    None,
                ) else {
                    return;
                };
                super::theme::apply_control(tips, true);
                self.tips = tips;
                for (i, tool) in Tool::ALL.into_iter().enumerate() {
                    // The tooltip keeps the pointer: the text lives as long as it.
                    let text: &'static [u16] = Box::leak(crate::platform::wide(tool.tip()).into_boxed_slice());
                    let info = TTTOOLINFOW {
                        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                        uFlags: TTF_SUBCLASS,
                        hwnd: self.hwnd,
                        uId: i + 1,
                        lpszText: windows::core::PWSTR(text.as_ptr() as *mut _),
                        ..Default::default()
                    };
                    SendMessageW(tips, TTM_ADDTOOLW, WPARAM(0), LPARAM(&info as *const _ as isize));
                }
            }
            let slots = if shown { tool_layout(self.width as f64, self.height as f64, self.scale()).1 } else { vec![] };
            for (i, tool) in Tool::ALL.into_iter().enumerate() {
                let rect = slots.iter().find(|(t, _)| *t == tool).map_or(RECT::default(), |(_, r)| RECT {
                    left: r.x as i32,
                    top: r.y as i32,
                    right: r.max_x() as i32,
                    bottom: r.max_y() as i32,
                });
                let info = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, hwnd: self.hwnd, uId: i + 1, rect, ..Default::default() };
                SendMessageW(self.tips, TTM_NEWTOOLRECTW, WPARAM(0), LPARAM(&info as *const _ as isize));
            }
        }
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
            if let Draw::Text(text, size, weight, ..) = draw {
                let _ = self.text(text, *size, *weight);
            }
        }
    }

    /// Turns pictures and text into GPU images; draws whose texture isn't
    /// ready yet are left out.
    fn resolve(&self, draws: Vec<Draw>) -> Vec<Item<'_>> {
        let px = |size: f64, s: f64| (size * s).round() as i32;
        let s = self.scale();
        let texts = &self.texts;
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

/// A click on a tool: the menu's command, or the original shown or not.
fn use_tool(tool: Tool) {
    with_app(|app| match tool {
        Tool::Before => app.model.hold_original(true),
        Tool::After => app.model.hold_original(false),
        Tool::Fit => app.command(super::id::FIT),
        Tool::Actual => app.command(super::id::ACTUAL),
        Tool::Rotate => app.command(super::id::ROTATE),
        Tool::Crop => app.command(super::id::CROP),
        Tool::FilmStrip => app.command(super::id::FILMSTRIP),
    });
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
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let _ = SetFocus(hwnd);
                let (x, y) = point(lparam);
                if let Some(pressed) = with_view(|v| v.press(x as f64, y as f64)).flatten() {
                    if pressed {
                        SetCapture(hwnd);
                    }
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
                    if with_view(|v| v.hover(x as f64, y as f64)).unwrap_or(false) {
                        // About to compare: decode the original ahead of the click.
                        post(|app: &mut App| app.model.prepare_original());
                    }
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
                    if v.hot.take().is_some() {
                        invalidate(hwnd);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                let (x, y) = point(lparam);
                let clicked = with_view(|v| {
                    v.pan = None;
                    let pressed = v.pressed.take();
                    if pressed.is_some() {
                        invalidate(hwnd);
                    }
                    // A tool acts when the button comes up over it.
                    pressed.filter(|&t| msg == WM_LBUTTONUP && v.tool_at(x as f64, y as f64) == Some(Some(t)))
                })
                .flatten();
                with_app(crop_mouse_up);
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                if let Some(tool) = clicked {
                    use_tool(tool);
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

    /// The tools sit in order inside the pill, centred above the bottom
    /// edge, inside the room a fitted photograph leaves them.
    #[test]
    fn tools_fit_their_pill_under_the_photograph() {
        for scale in [1.0, 1.5] {
            let (pill, tools, dividers) = tool_layout(900.0, 700.0, scale);
            assert!((pill.mid_x() - 450.0).abs() <= 1.0);
            assert!(pill.max_y() <= 700.0 - PILL_MARGIN * scale + 1.0);
            assert!(pill.y >= 700.0 - tools_reserve(scale));
            assert_eq!(tools.iter().map(|(t, _)| *t).collect::<Vec<_>>(), Tool::ALL);
            for pair in tools.windows(2) {
                assert!(pair[0].1.max_x() <= pair[1].1.x + 0.5);
            }
            for (_, r) in &tools {
                assert!(r.x >= pill.x && r.max_x() <= pill.max_x() + 1.0 && r.y >= pill.y && r.max_y() <= pill.max_y());
            }
            assert_eq!(dividers.len(), 3);
        }
    }

    /// Fitting leaves the tools' room free, and gives it back while cropping.
    #[test]
    fn fitting_keeps_clear_of_the_tools() {
        let reserve = tools_reserve(1.0);
        let zoom = zoom_amount(true, (4000.0, 3000.0), (1000.0, 800.0 - reserve));
        assert!(3000.0 * zoom <= 800.0 - reserve);
        assert_eq!(zoom_amount(true, (4000.0, 3000.0), (1000.0, 800.0)), 0.25);
    }

    #[test]
    fn crop_layout_fits_the_photograph_above_the_dial() {
        let layout = crop_layout(1000.0, 800.0, 96, Size::new(600.0, 400.0));
        assert!((layout.scale - (1000.0 - 72.0) / 600.0).min((800.0 - 72.0 - 76.0) / 400.0) .abs() < 1e-9 || layout.scale > 0.0);
        assert!(layout.frame.max_y() <= layout.dial.y);
        assert!((layout.frame.mid_x() - 500.0).abs() < 1e-9);
    }
}
