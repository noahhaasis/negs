//! The photograph: wgpu renders straight into this window. Zoom and pan are
//! a transform on the uploaded texture, so they never re-render or copy
//! pixels. Crop, rotation and border are applied when drawing, as on the Mac
//! (`PreviewCanvas.swift`, `Border.swift`, `Crop.swift`).
//!
//! The view state lives here, apart from the app, so the canvas can paint
//! while the app is busy (a modal dialog, a re-entrant call).

use std::cell::RefCell;
#[cfg(test)]
use std::cell::Cell;
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

mod print;
pub(super) use print::draw_lock;
mod software;

pub use print::PrintWords;
use print::{Glyph, Target};

/// The sheet the photograph lies on (Settings › Look › Viewer). High
/// contrast keeps the system's colours.
fn background() -> [u8; 3] {
    let theme = super::theme::current();
    if theme.high_contrast {
        return theme.palette.canvas;
    }
    let sheet = super::sheet::ink().sheet;
    [(sheet >> 16) as u8, (sheet >> 8) as u8, sheet as u8]
}

/// The viewer sheet changed: repaint in its colour and inks.
pub fn sheet_changed(hwnd: HWND) {
    invalidate(hwnd);
}

/// Direct2D is ready: the caption and label can be set now.
pub fn drawing_ready(hwnd: HWND) {
    invalidate(hwnd);
}

/// The window is being sized by its edge, or that ended: the fitted view
/// keeps to mipmaps while it moves, then makes its exact picture.
pub fn set_live_resize(on: bool) {
    with_view(|v| {
        if v.live_resize != on {
            v.live_resize = on;
            invalidate(v.hwnd);
        }
    });
}

impl View {
    /// The fitted photograph's exact-size picture over the photograph
    /// (ticket 38), if it's made; asks for it if not. `corner` is the
    /// crop's top left on screen, `clip` the crop in the turned frame of
    /// the `shown` size, in layout pixels.
    fn exact_draw(&mut self, picture: &Picture, scene: &Scene, corner: (f64, f64), clip: Rect, shown: Size) -> Option<Draw> {
        let z = self.zoom;
        let settled = scene.frame.is_none_or(|f| matches!(f.kind, crate::model::FrameKind::Settled | crate::model::FrameKind::Cached));
        let (tw, th) = crate::exact_fit::target(scene.size.width, scene.size.height, z);
        let eligible = self.fitting
            && settled
            && scene.crop.angle == 0.0
            && !self.live_resize
            && crate::exact_fit::worth_it(z, (picture.width, picture.height), (tw, th));
        if !eligible {
            self.exact_wanted = None;
            return None;
        }
        let key = (picture.id, tw, th);
        let made = match &self.gpu {
            Some(_) => self.exact.iter().any(|(k, _)| *k == key),
            None => self.software_cache.has_exact(key),
        };
        if !made {
            if self.exact_wanted.as_ref().is_none_or(|(p, w, h)| (p.id, *w, *h) != key) {
                self.exact_wanted = Some((picture.clone(), tw, th));
                // The same size again (a new develop): at once. A new size
                // (the inspector sliding, Image Only): once it holds still.
                let wait = if self.exact_size == (tw, th) { 1 } else { 150 };
                unsafe {
                    SetTimer(self.hwnd, TIMER_EXACT, wait, None);
                }
            }
            return None;
        }
        // In the exact picture's pixels: the crop on whole pixels, its
        // corner on a whole screen pixel, drawn 1:1.
        let turned = if scene.rotation.rem_euclid(2) == 1 { (th as f64, tw as f64) } else { (tw as f64, th as f64) };
        let (fx, fy) = (turned.0 / shown.width, turned.1 / shown.height);
        let c = Rect::new((clip.x * fx).round(), (clip.y * fy).round(), (clip.width * fx).round(), (clip.height * fy).round());
        let (x0, y0) = (corner.0.round(), corner.1.round());
        Some(Draw::Exact(
            key,
            PhotoUniform {
                origin_scale: [(x0 - c.x) as f32, (y0 - c.y) as f32, 1.0, 0.0],
                shown: [turned.0 as f32, turned.1 as f32, scene.rotation.rem_euclid(4) as f32, 0.0],
                clip: [c.x as f32, c.y as f32, c.width as f32, c.height as f32],
                quad: [x0 as f32, y0 as f32, c.width as f32, c.height as f32],
                extra: [1.0, picture.wide as u8 as f32, scene.flip as u8 as f32, 0.0],
            },
        ))
    }

    /// Makes the wanted exact picture: on the GPU at once, on the software
    /// canvas on a thread. Then the canvas draws it.
    fn make_exact(&mut self) {
        let Some((picture, tw, th)) = self.exact_wanted.take() else { return };
        let key = (picture.id, tw, th);
        self.exact_size = (tw, th);
        match self.gpu.clone() {
            Some(gpu) => {
                let Some(source) = picture.texture() else { return };
                let started = std::time::Instant::now();
                let image = gpu.lanczos(source, picture.width, picture.height, tw, th);
                crate::trace::event("exact_fit", || serde_json::json!({"from": [picture.width, picture.height], "to": [tw, th], "ms": started.elapsed().as_secs_f64() * 1000.0}));
                if let Some(dump) = std::env::var_os("NEGS_EXACT_FIT_DUMP") {
                    let dump = std::path::PathBuf::from(dump);
                    let _ = std::fs::write(dump.join(format!("source-{}x{}.raw", picture.width, picture.height)), gpu.read_back(&source.texture));
                    let _ = std::fs::write(dump.join(format!("exact-{tw}x{th}.raw", )), gpu.read_back(&image.texture));
                }
                // The last two: Original and the develop both stay sharp.
                if self.exact.len() >= 2 {
                    self.exact.remove(0);
                }
                self.exact.push((key, image));
                invalidate(self.hwnd);
            }
            None => {
                let hwnd = self.hwnd.0 as isize;
                std::thread::Builder::new()
                    .name("exact-fit".into())
                    .spawn(move || {
                        let Some(pixels) = picture.with_pixels(|rgba| crate::exact_fit::resample(rgba, picture.width, picture.height, tw, th)) else { return };
                        post(move |_: &mut App| {
                            with_view(|v| {
                                v.software_cache.add_exact(key, pixels, picture.id);
                                invalidate(HWND(hwnd as *mut _));
                            });
                        });
                    })
                    .ok();
            }
        }
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
    /// Mirrored left to right before the quarter turns (`io.flip`).
    pub flip: bool,
    /// Degrees clockwise as shown.
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
    /// Mirrored left to right after the crop, before the turns (`io.flip`).
    pub flip: bool,
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
    /// The caption's and the label's words.
    pub words: PrintWords,
    /// A developed photograph is shown: the trim marks can crop it.
    pub croppable: bool,
    /// View › Image Only: no trim marks.
    pub image_only: bool,
    /// T held: the photograph turned upside down (display only).
    pub upside_down: bool,
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
    /// What clicks can reach (names, lock, trim marks, the dial's arrows),
    /// where they were last drawn, in pixels.
    targets: Vec<(Target, Rect)>,
    /// The label's tooltips as last set: where the film, the paper and the
    /// lock are, and whether the lock is closed.
    label_tips: Option<([RECT; 3], bool)>,
    /// The target under the pointer, and the pointer is over the print.
    hot: Option<Target>,
    over_print: bool,
    /// A press on a target: what, and where it began.
    pressed: Option<(Target, (f64, f64))>,
    /// The print's rectangle on screen as last laid out, cut to its band.
    print: Option<Rect>,
    /// The marks set as masks, by glyph, pixel scale and Plain.
    glyphs: HashMap<(Glyph, u32, bool), GpuImage>,
    /// The strip's leader over the canvas's bottom right corner, raised or
    /// not: drawn here too, so its slanted edge is smooth.
    leader: Option<bool>,
    /// The software canvas's last frame without the Developing tag, so the
    /// tag's line can move without drawing the photograph again.
    software_base: Option<(u32, u32, Vec<u8>)>,
    /// Where the tag was last drawn, in pixels, and where its draws begin.
    tag_rect: Option<Rect>,
    tag_start: Option<usize>,
    /// The fitted view's exact-size pictures (ticket 38), the last two
    /// made, by picture and size; the one wanted next; the size last made.
    exact: Vec<(ExactKey, GpuImage)>,
    exact_wanted: Option<(Picture, u32, u32)>,
    exact_size: (u32, u32),
    /// The window is being sized by its edge: the fit changes every frame.
    live_resize: bool,
}

/// An exact picture's picture id and size.
type ExactKey = (u64, u32, u32);
/// Makes a wanted exact picture once the canvas holds still.
const TIMER_EXACT: usize = 8;

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
            first_image_presented: false,
            words: HashMap::new(),
            retry: None,
            retry_hot: false,
            retry_pressed: false,
            running: None,
            presented: None,
            targets: Vec::new(),
            label_tips: None,
            hot: None,
            over_print: false,
            pressed: None,
            print: None,
            glyphs: HashMap::new(),
            leader: None,
            software_base: None,
            tag_rect: None,
            tag_start: None,
            exact: Vec::new(),
            exact_wanted: None,
            exact_size: (0, 0),
            live_resize: false,
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
                    app.prepare_surfaces();
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
                    app.prepare_surfaces();
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

/// The strip's leader stands over the canvas, raised under the pointer, or
/// is hidden (None).
pub fn set_leader(leader: Option<bool>) {
    let hwnd = with_view(|v| {
        let changed = v.leader != leader;
        v.leader = leader;
        changed.then_some(v.hwnd)
    })
    .flatten();
    if let Some(hwnd) = hwnd {
        invalidate(hwnd);
    }
}

pub fn zoom_by(factor: f64) {
    with_view(|view| {
        let (w, h) = view.viewport();
        view.zoom_about(factor, w / 2.0, h / 2.0);
    });
}

/// Whether a press at (`x`, `y`), canvas pixels, is on a stock's name or
/// the paper lock under the print: those leave an open tray open.
pub fn opens_tray_at(x: i32, y: i32) -> bool {
    with_view(|v| matches!(v.print_hit(x as f64, y as f64).0, Some(Target::Film | Target::Paper | Target::Lock))).unwrap_or(false)
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
    // Picking the base shows the whole frame, without its border.
    let whole = model.negative_pick() == Some(crate::negative::Pick::Base);
    let shown_crop = model.shown_crop();
    let cropped = shown_crop.size_of(size);
    // A stock just tried in the tray: its tile until its develop is in
    // (`EditorModel.trialImage`). The tile is cropped already, so it's shown
    // uncropped at the cropped photograph's size, turned as the photograph is;
    // while zoomed into a cropped photograph that would move the view, so not then.
    let fitting = with_view(|v| v.fitting).unwrap_or(true);
    let trial = model.shown_trial().filter(|_| fitting || !shown_crop.cropped()).map(|trial| {
        let tile = (trial.picture.width, trial.picture.height);
        let (w, h) = crate::tray_rules::stand_in_size(tile, (cropped.width, cropped.height));
        (trial.picture.clone(), Size::new(w, h))
    });
    let (picture, size, developed, crop) = match trial {
        Some((picture, size)) => (Some(picture), size, false, CropSettings::default()),
        None => (picture, size, developed, shown_crop),
    };
    // As a photograph opens, its developed thumbnail until anything else
    // (`standIn`): cropped, turned and bordered already, so shown as it is,
    // at its own shape with a RAW's 2048 px on the long side.
    let stand_in = model.stand_in.as_ref().filter(|_| picture.is_none());
    let (picture, size, crop) = match stand_in {
        Some(stand_in) => {
            let (w, h) = crate::tray_rules::stand_in_size((stand_in.width, stand_in.height), (0.0, 0.0));
            (Some(stand_in.clone()), Size::new(w, h), CropSettings::default())
        }
        None => (picture, size, crop),
    };
    let border = if whole || stand_in.is_some() { None } else { model.state.border.shown(cropped.width as u32, cropped.height as u32) };
    let cropping = model.crop_session.as_ref().map(|s| CropScene {
        photo: s.photo,
        rotation: s.rotation,
        flip: s.flip,
        angle: s.angle,
        shown: s.shown(),
        grid: if s.straightening { Some(Grid::Fine) } else if s.adjusting { Some(Grid::Thirds) } else { None },
    });
    let scene = Scene {
        picture,
        developed,
        size,
        rotation: if stand_in.is_some() { 0 } else { model.rotation() },
        flip: stand_in.is_none() && model.state.flip(),
        crop,
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
        words: print_words(app),
        croppable: model.image.is_some() && !model.render_failed,
        image_only: app.view.image_only,
        upside_down: app.view.upside_down(),
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

thread_local! {
    /// The open photograph's camera facts, read off the UI thread: the
    /// file, and its facts once read.
    static FACTS: RefCell<Option<(std::path::PathBuf, Option<crate::photo_facts::PhotoFacts>)>> = const { RefCell::new(None) };
}

/// The caption's and the label's words for the open photograph. The
/// camera's facts are read on a thread the first time.
fn print_words(app: &App) -> PrintWords {
    let model = &app.model;
    let Some(source) = model.source.clone() else { return PrintWords::default() };
    let facts = FACTS.with(|f| {
        let mut f = f.borrow_mut();
        match &*f {
            Some((path, facts)) if *path == source => facts.clone(),
            _ => {
                *f = Some((source.clone(), None));
                let path = source.clone();
                std::thread::Builder::new()
                    .name("photo-facts".into())
                    .spawn(move || {
                        let facts = crate::photo_facts::PhotoFacts::read(&path).unwrap_or_default();
                        post(move |app: &mut App| {
                            FACTS.with(|f| {
                                if let Some((shown, slot)) = &mut *f.borrow_mut()
                                    && *shown == path
                                {
                                    *slot = Some(facts);
                                }
                            });
                            app.model.dirty |= crate::model::dirty::CANVAS;
                        });
                    })
                    .ok();
                None
            }
        }
    });
    let decimal = super::inspector::decimal_separator();
    PrintWords {
        facts: facts.map(|f| f.items(decimal)).unwrap_or_default(),
        file: crate::model::file_name(&source),
        film: model.film_name(),
        paper: model.paper_name(),
        scanning: crate::json::boolean(model.value("io.scan_film")),
        original: model.holding_original && model.original.is_some(),
        keep_paper: model.prefs.keep_paper,
        tray: model.tray_shown().filter(|_| !model.tray().is_some_and(|t| t.read_as)),
        negative: model.is_negative(),
        read_as_open: model.tray_shown() == Some(false) && model.tray().is_some_and(|t| t.read_as),
        prompt: model.negative_pick().map(|p| p.prompt()),
    }
}

impl View {
    fn scale(&self) -> f64 {
        self.dpi as f64 / 96.0
    }

    /// The band the photograph lies in, between the caption's band above
    /// and the label's below (§2.3); zoomed in, it scrolls under them.
    fn band(&self) -> Rect {
        let s = self.scale();
        let top = (print::CAPTION_BAND * s).round();
        let bottom = (print::LABEL_BAND * s).round();
        Rect::new(0.0, top, self.width as f64, (self.height as f64 - top - bottom).max(1.0))
    }

    /// The band's size: what the photograph centres and scrolls in.
    fn viewport(&self) -> (f64, f64) {
        let band = self.band();
        (band.width.max(1.0), band.height)
    }

    /// What a fitted photograph fills: the band less the side margins.
    fn fit_viewport(&self) -> (f64, f64) {
        let band = self.band();
        ((band.width - 2.0 * (print::MARGIN * self.scale()).round()).max(1.0), band.height)
    }

    /// Zoomed in or out from fitting, not a zoom that happens to match the fit.
    fn zoomed(&self) -> bool {
        if self.fitting || self.document.0 <= 0.0 {
            return false;
        }
        let fit = zoom_amount(true, self.document, self.fit_viewport()).clamp(MIN_ZOOM, MAX_ZOOM);
        (self.zoom / fit - 1.0).abs() > 0.005
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
        self.zoom = zoom_amount(self.fitting, (dw, dh), self.fit_viewport()).clamp(MIN_ZOOM, MAX_ZOOM);
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
        let band = self.band();
        let (hx, hy) = (band.mid_x(), band.mid_y());
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
        let band = self.band();
        (band.mid_x() - self.center.0 * self.zoom, band.mid_y() - self.center.1 * self.zoom)
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
        gpu::draw(&gpu, &target, (self.width, self.height), &globals.bind_group, clear, &items);
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
        let mut ps = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(self.hwnd, &mut ps) };
        let mut draws = Vec::new();
        self.build(&mut draws);
        let size = (self.width, self.height);
        let r = ps.rcPaint;
        // Only the tag's running line moved: draw it over the last frame
        // without it, rather than the photograph again.
        let only_tag = self.tag_rect.is_some_and(|t| {
            r.left as f64 >= t.x.floor() && r.top as f64 >= t.y.floor() && r.right as f64 <= t.max_x().ceil() && r.bottom as f64 <= t.max_y().ceil()
        });
        let base = match (self.tag_start, self.software_base.take()) {
            (Some(_), Some((w, h, base))) if only_tag && (w, h) == size => Some(base),
            (Some(start), _) => Some(software::render(size.0, size.1, self.dpi, &draws[..start], self.software_cm.as_ref(), &mut self.software_cache)),
            _ => None,
        };
        let pixels = match (base, self.tag_start) {
            (Some(base), Some(start)) => {
                let pixels = software::render_onto(base.clone(), size.0, size.1, self.dpi, &draws[start..], self.software_cm.as_ref(), &mut self.software_cache);
                self.software_base = Some((size.0, size.1, base));
                pixels
            }
            _ => software::render(size.0, size.1, self.dpi, &draws, self.software_cm.as_ref(), &mut self.software_cache),
        };
        unsafe {
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
    /// Words set with DirectWrite (Newsreader): the text, its style, the
    /// width it wraps and centres in (DIPs; 0 for one line), colour, place.
    Words(String, TextStyle, f32, [f32; 4], (f64, f64)),
    /// A mark drawn with Direct2D (the caret, the lock, the turn arrows,
    /// a strike), its colour, its top left in pixels.
    Glyph(Glyph, [f32; 4], (f64, f64)),
    /// The fitted photograph's exact-size picture, drawn 1:1.
    Exact(ExactKey, PhotoUniform),
    /// What follows draws only inside this rectangle (pixels); None: anywhere.
    Clip(Option<Rect>),
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
/// the exporter's pixel rectangle in the unrotated photograph: mirrored if
/// `flip`, then turned.
fn shown_pixels(crop: &CropSettings, size: Size, rotation: i32, flip: bool) -> (Rect, Size) {
    let mut r = crop.pixels(size);
    if flip {
        r = Rect::new(size.width - r.x - r.width, r.y, r.width, r.height);
    }
    let mut s = size;
    for _ in 0..rotation.rem_euclid(4) {
        r = Rect::new(s.height - r.y - r.height, r.x, r.height, r.width);
        s = s.swapped();
    }
    (r, s)
}

/// Crop mode's layout: the photograph's frame, its scale and the dial's
/// tick row.
pub struct CropLayout {
    pub frame: Rect,
    pub scale: f64,
    pub dial: Rect,
}

fn crop_layout(width: f64, height: f64, dpi: u32, photo: Size) -> CropLayout {
    let s = dpi as f64 / 96.0;
    let (frame, scale) = print::crop_place(width, height, s, photo);
    CropLayout { frame, scale, dial: print::dial_rect(frame, s) }
}

/// Turns draws half a turn about `c` (pixels): the photograph upside down
/// while T is held. Rectangles move to their mirror places, textures read
/// backwards, and the photograph takes two more quarter turns.
fn turn_half(draws: &mut [Draw], c: (f64, f64)) {
    let turn_rect = |r: [f32; 4]| [(2.0 * c.0) as f32 - r[0] - r[2], (2.0 * c.1) as f32 - r[1] - r[3], r[2], r[3]];
    for draw in draws {
        match draw {
            Draw::Photo(_, u) | Draw::Exact(_, u) => {
                let (shown_w, shown_h, z) = (u.shown[0], u.shown[1], u.origin_scale[2]);
                u.origin_scale[0] = (2.0 * c.0) as f32 - u.origin_scale[0] - shown_w * z;
                u.origin_scale[1] = (2.0 * c.1) as f32 - u.origin_scale[1] - shown_h * z;
                u.shown[2] = ((u.shown[2] as i32 + 2) % 4) as f32;
                u.clip = [shown_w - u.clip[0] - u.clip[2], shown_h - u.clip[1] - u.clip[3], u.clip[2], u.clip[3]];
                u.quad = turn_rect(u.quad);
            }
            Draw::Fill(q) => q.rect = turn_rect(q.rect),
            Draw::Texture(_, q) => {
                q.rect = turn_rect(q.rect);
                q.uv = [q.uv[2], q.uv[3], q.uv[0], q.uv[1]];
            }
            _ => {}
        }
    }
}

impl View {
    /// The print (the photograph with its border) on screen, cut to its
    /// band, on whole pixels; None before there's a document.
    /// Where (x, y) falls on the photograph as shown, as fractions of it
    /// from its top left (outside it, beyond 0…1).
    fn shown_fraction(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let (dw, dh) = self.document;
        let (pw, ph) = self.photo_display;
        if pw <= 0.0 || ph <= 0.0 {
            return None;
        }
        let (ox, oy) = self.document_origin();
        let (left, top) = (ox + (dw - pw) / 2.0 * self.zoom, oy + (dh - ph) / 2.0 * self.zoom);
        Some([(x - left) / (pw * self.zoom), (y - top) / (ph * self.zoom)])
    }

    fn print_rect(&self) -> Option<Rect> {
        let (dw, dh) = self.document;
        if dw <= 0.0 || self.width == 0 {
            return None;
        }
        let (ox, oy) = self.document_origin();
        let shown = Rect::new(ox, oy, dw * self.zoom, dh * self.zoom).intersection(&self.band());
        let (x, y) = (shown.x.floor(), shown.y.floor());
        let r = Rect::new(x, y, shown.max_x().ceil() - x, shown.max_y().ceil() - y);
        (r.width > 0.0 && r.height > 0.0).then_some(r)
    }

    /// The label's tooltips follow its names and the paper lock
    /// (`ControlHelp.film`, `paper`, `paperLock`, `paperKept`): over each as
    /// last drawn, nowhere while it isn't.
    fn follow_label_tips(&mut self) {
        let place = |target| {
            self.target_rect(target).map_or(RECT::default(), |r| RECT {
                left: r.x.floor() as i32,
                top: r.y.floor() as i32,
                right: r.max_x().ceil() as i32,
                bottom: r.max_y().ceil() as i32,
            })
        };
        let rects = [place(Target::Film), place(Target::Paper), place(Target::Lock)];
        let kept = self.scene.words.keep_paper;
        if self.label_tips == Some((rects, kept)) || self.hwnd.is_invalid() {
            return;
        }
        self.label_tips = Some((rects, kept));
        let lock = if kept { crate::help::PAPER_KEPT } else { crate::help::PAPER_LOCK };
        for (part, (rect, text)) in rects.into_iter().zip([crate::help::FILM, crate::help::PAPER, lock]).enumerate() {
            super::dialogs::area_tooltip(self.hwnd, part + 1, rect, text);
        }
    }

    fn build(&mut self, items: &mut Vec<Draw>) {
        let scene = self.scene.clone();
        let s = self.scale();
        let ink = super::sheet::ink();
        self.targets.clear();
        self.print = None;
        self.tag_start = None;
        self.tag_rect = None;
        if let Some(picture) = scene.picture.clone() {
            if let Some(cropping) = &scene.cropping {
                self.build_crop(items, picture, cropping, &ink);
            } else if !scene.drop_target {
                // While a file is dragged in, the photograph, caption and
                // label step aside for a line round the print's place.
                let print = self.print_rect();
                self.print = print;
                let zoomed = self.zoomed();
                let failed = scene.failed.is_some();
                // A light under the print when its edge sinks into the sheet;
                // fitted only: zoomed in, the print has no edges on screen.
                if let Some(print) = print.filter(|_| !zoomed && !failed && !super::theme::current().high_contrast) {
                    let edge = super::sheet::light_table::edge(scene.border.as_ref());
                    if let Some(light) = super::sheet::light_table::light(ink.sheet, edge) {
                        use super::sheet::light_table::{BLUR, CORNER_RADIUS, SPREAD};
                        let (spread, blur) = (SPREAD * s, BLUR * s);
                        let reach = spread + 3.0 * blur;
                        items.push(Draw::Fill(QuadInstance::blurred(
                            rect(print.x - reach, print.y - reach, print.width + 2.0 * reach, print.height + 2.0 * reach),
                            hex(light, 1.0),
                            (CORNER_RADIUS * s) as f32,
                            blur as f32,
                        )));
                    }
                }
                let start = items.len();
                items.push(Draw::Clip(Some(self.band())));
                self.build_photo(items, picture, &scene);
                items.push(Draw::Clip(None));
                if scene.upside_down {
                    // About the middle of the part on screen, on a half pixel,
                    // so the turn maps the pixel grid onto itself.
                    if let Some(p) = print {
                        let c = ((2.0 * p.mid_x()).round() / 2.0, (2.0 * p.mid_y()).round() / 2.0);
                        turn_half(&mut items[start..], c);
                    }
                }
                if let Some(print) = print {
                    let portrait = !zoomed && print.height > print.width;
                    self.build_caption(items, print, portrait, &ink);
                    self.build_label(items, print, &ink);
                    if !zoomed && !scene.image_only && scene.croppable && !failed {
                        self.build_trim_marks(items, print, &ink);
                    }
                }
            }
        }
        self.follow_label_tips();
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
        if let Some(raised) = self.leader {
            // Its right and bottom edges on the canvas's, under the leader's window.
            let t = super::theme::tokens();
            let (w, h) = (crate::sheet_rules::LEADER_WIDTH, crate::sheet_rules::LEADER_RAISED);
            let at = (self.width as f64 - (w * s).round(), self.height as f64 - (h * s).round());
            items.push(Draw::Glyph(Glyph::Tongue(raised), hex(t.film.film, 1.0), at));
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
        let photo = self.print_rect().unwrap_or_else(|| self.shown_rect());
        let x = ((photo.mid_x() - w / 2.0).round()).max(0.0);
        let y = (photo.y + 16.0 * s).round();
        // With its shadow, for the software canvas's running line.
        self.tag_rect = Some(Rect::new(x - 10.0 * s, y - 10.0 * s, w + 20.0 * s, h + 24.0 * s));
        self.tag_start = Some(items.len());
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
        // The mark runs unless motion is reduced: then it stays a third of
        // the way along. The software canvas redraws only the tag for it.
        let animate = super::motion::enabled();
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
        let ink = super::sheet::ink();
        let photo = self.print_rect().unwrap_or_else(|| self.shown_rect());
        let o = 12.0 * s;
        items.push(Draw::Fill(QuadInstance::rounded(rect(photo.x - o, photo.y - o, photo.width + 2.0 * o, photo.height + 2.0 * o), hex(ink.accent, 1.0), (4.0 * s) as f32, (1.5 * s) as f32)));
        if let Some((w, h)) = self.words_size("drop to open it", &DROP) {
            items.push(Draw::Words("drop to open it".into(), DROP, 0.0, hex(ink.paper, 1.0), ((self.width as f64 - w) / 2.0, (self.height as f64 - h) / 2.0)));
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
        let (clip, shown) = shown_pixels(&scene.crop, scene.size, scene.rotation, scene.flip);
        let origin = (ox + px * z - clip.x * z, oy + py * z - clip.y * z);
        // Straightened in the unflipped photograph: the other way round as shown.
        let angle = if scene.flip { -scene.crop.angle } else { scene.crop.angle };
        let uniform = PhotoUniform {
            origin_scale: [origin.0 as f32, origin.1 as f32, z as f32, angle.to_radians() as f32],
            shown: [shown.width as f32, shown.height as f32, scene.rotation.rem_euclid(4) as f32, 0.0],
            clip: [clip.x as f32, clip.y as f32, clip.width as f32, clip.height as f32],
            quad: screen(px, py, pw, ph),
            extra: [1.0, picture.wide as u8 as f32, scene.flip as u8 as f32, 0.0],
        };
        let exact = self.exact_draw(&picture, scene, (ox + px * z, oy + py * z), clip, shown);
        items.push(Draw::Photo(picture, uniform));
        // Over it, so a sliver its rounding leaves shows the photograph, not the sheet.
        items.extend(exact);
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

    /// Cropping (§4.10): the photograph in the print's place, darkened
    /// outside the crop with `rgba(8,7,6,.6)`; the crop a 1 DIP `paper` line
    /// with the rule of thirds in faint `paper` (ninths while straightening)
    /// and 3 DIP `paper` corners just outside it. The caption stays; the
    /// straightening dial takes the label's place.
    fn build_crop(&mut self, items: &mut Vec<Draw>, picture: Picture, cropping: &CropScene, ink: &super::sheet::SheetInk) {
        let s = self.scale();
        let t = super::theme::tokens();
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
            extra: [super::theme::CROP_SHADE_OPACITY, picture.wide as u8 as f32, cropping.flip as u8 as f32, 0.0],
        };
        items.push(Draw::Photo(picture, uniform));
        let c = crop_rect;
        let paper = t.hex.paper;
        let lines = if cropping.grid == Some(Grid::Fine) { 9 } else { 3 };
        let thin = s.max(1.0).round();
        for i in 1..lines {
            let x = (c.x + c.width * i as f64 / lines as f64).round();
            let y = (c.y + c.height * i as f64 / lines as f64).round();
            items.push(Draw::Fill(QuadInstance::fill(rect(x, c.y, thin, c.height), hex(paper, 0.3))));
            items.push(Draw::Fill(QuadInstance::fill(rect(c.x, y, c.width, thin), hex(paper, 0.3))));
        }
        items.push(Draw::Fill(QuadInstance::rounded(rect(c.x - thin, c.y - thin, c.width + 2.0 * thin, c.height + 2.0 * thin), hex(paper, 0.9), 0.0, thin as f32)));
        // The corners, 20 DIPs along each edge, just outside the line.
        let length = (20.0 * s).min(c.width / 2.0).min(c.height / 2.0);
        let width = 3.0 * s;
        let outer = c.inset(-width / 2.0, -width / 2.0);
        let mut corners = vec![];
        for (x, y, dx, dy) in [(outer.x, outer.y, 1.0, 1.0), (outer.max_x(), outer.y, -1.0, 1.0), (outer.x, outer.max_y(), 1.0, -1.0), (outer.max_x(), outer.max_y(), -1.0, -1.0)] {
            corners.push(Rect::new(if dx > 0.0 { x - width / 2.0 } else { x - length }, y - width / 2.0, length + width / 2.0, width));
            corners.push(Rect::new(x - width / 2.0, if dy > 0.0 { y - width / 2.0 } else { y - length }, width, length + width / 2.0));
        }
        // Uncropped, the corners lie on the sheet: on a light one, edged in its ink.
        if !ink.light {
            for b in &corners {
                items.push(Draw::Fill(QuadInstance::fill(rect(b.x - s, b.y - s, b.width + 2.0 * s, b.height + 2.0 * s), hex(ink.paper, 1.0))));
            }
        }
        for b in &corners {
            items.push(Draw::Fill(QuadInstance::fill(rect(b.x, b.y, b.width, b.height), hex(paper, 1.0))));
        }
        let portrait = frame.height > frame.width;
        self.build_caption(items, frame, portrait, ink);
        self.build_straighten_dial(items, frame, cropping.angle, ink);
    }

    /// Rasterises the texts a frame needs before it borrows the cache.
    fn prepare_texts(&mut self, draws: &[Draw]) {
        for draw in draws {
            match draw {
                Draw::Words(text, style, width, ..) => {
                    let _ = self.words_mask(text, style, *width);
                }
                Draw::Glyph(glyph, ..) => self.glyph_mask(*glyph),
                _ => {}
            }
        }
        // Words of photographs long gone.
        if self.words.len() > 400 {
            self.words.clear();
        }
    }

    /// Sets a mark as a mask on the GPU, once per scale.
    fn glyph_mask(&mut self, glyph: Glyph) {
        let plain = super::theme::tokens().plain;
        let key = (glyph, self.dpi, plain);
        let Some(gpu) = self.gpu.clone() else { return };
        if self.glyphs.contains_key(&key) {
            return;
        }
        let (w, h) = glyph.size();
        if let Some(mask) = super::d2d::shape_mask(w as f32, h as f32, self.scale() as f32, plain, |p| glyph.draw(p)) {
            self.glyphs.insert(key, gpu.upload_mask(&mask.coverage, mask.width, mask.height));
        }
    }

    /// Turns pictures and text into GPU images; draws whose texture isn't
    /// ready yet are left out.
    fn resolve(&self, draws: Vec<Draw>) -> Vec<Item<'_>> {
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
                Draw::Words(text, style, width, color, (x, y)) => {
                    if let Some(image) = words.get(&words_key(&text, &style, width, self.dpi)) {
                        let size = image.texture.size();
                        items.push(Item::Quad(Some(image), QuadInstance::text(rect(x.round(), y.round(), size.width as f64, size.height as f64), color)));
                    }
                }
                Draw::Exact(key, uniform) => {
                    if let Some((_, image)) = self.exact.iter().find(|(k, _)| *k == key) {
                        items.push(Item::Photo(image, uniform));
                    }
                }
                Draw::Glyph(glyph, color, (x, y)) => {
                    if let Some(image) = self.glyphs.get(&(glyph, self.dpi, super::theme::tokens().plain)) {
                        let size = image.texture.size();
                        items.push(Item::Quad(Some(image), QuadInstance::text(rect(x.round(), y.round(), size.width as f64, size.height as f64), color)));
                    }
                }
                Draw::Clip(r) => items.push(Item::Clip(r.map(|r| {
                    let (x, y) = (r.x.max(0.0).floor(), r.y.max(0.0).floor());
                    [x as u32, y as u32, (r.max_x().ceil() - x).max(0.0) as u32, (r.max_y().ceil() - y).max(0.0) as u32]
                }))),
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
    crop::handle_at(p, frame, dpi as f64 / 96.0).map(CropTarget::Handle)
}

enum CropTarget {
    Handle(Handle),
    Dial,
}

fn cursor_for(target: Option<&CropTarget>) -> PCWSTRCursor {
    match target {
        Some(CropTarget::Dial) => IDC_SIZEWE,
        Some(CropTarget::Handle(handle)) => match (handle.x, handle.y) {
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
        CropTarget::Handle(handle) => {
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
            let mut next = start - (x - x0 as f64) / print::dial_spacing(s);
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
            set_cursor(cursor_for(Some(&CropTarget::Handle(handle))));
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
            WM_TIMER if wparam.0 == TIMER_EXACT => {
                let _ = KillTimer(hwnd, TIMER_EXACT);
                with_view(|v| v.make_exact());
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_RUNNING => {
                let _ = KillTimer(hwnd, TIMER_RUNNING);
                // The next frame sets it again while the tag shows. The
                // software canvas redraws the tag alone.
                let tag = with_view(|v| v.running.is_some().then(|| if v.gpu.is_some() { None } else { v.tag_rect })).flatten();
                match tag {
                    Some(Some(t)) => {
                        let r = RECT { left: t.x.floor() as i32, top: t.y.floor() as i32, right: t.max_x().ceil() as i32, bottom: t.max_y().ceil() as i32 };
                        let _ = InvalidateRect(hwnd, Some(&r), false);
                    }
                    Some(None) => invalidate(hwnd),
                    None => {}
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
                // Prep's pickers: the click reads the negative there.
                if with_app(|app| app.model.negative_pick().is_some()).unwrap_or(false) {
                    let shown = with_view(|v| v.shown_fraction(x as f64, y as f64)).flatten();
                    post(move |app: &mut App| match shown {
                        Some(shown) => app.model.picked_shown_point(shown),
                        None => {
                            app.model.cancel_negative_pick();
                        }
                    });
                    return LRESULT(0);
                }
                // The label's names, the lock, a trim mark or a turn arrow:
                // they act when the button comes up over them.
                if let Some(target) = with_view(|v| v.print_hit(x as f64, y as f64).0).flatten() {
                    with_view(|v| v.pressed = Some((target, (x as f64, y as f64))));
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
                // A trim mark dragged inward crops.
                let pressed = with_view(|v| v.pressed).flatten();
                if let Some((Target::Mark(handle), start)) = pressed {
                    let s = with_view(|v| v.scale()).unwrap_or(1.0);
                    if print::inward(handle, start, x as f64, y as f64) > print::MARK_DRAG * s {
                        with_view(|v| v.pressed = None);
                        let _ = ReleaseCapture();
                        if with_app(|app| app.model.begin_crop()).is_none() {
                            post(|app: &mut App| app.model.begin_crop());
                        }
                    }
                    return LRESULT(0);
                }
                if pressed.is_some() {
                    return LRESULT(0);
                }
                let panned = with_view(|v| {
                    let Some((x0, y0, center)) = v.pan else { return false };
                    v.center = (center.0 - (x - x0) as f64 / v.zoom, center.1 - (y - y0) as f64 / v.zoom);
                    v.clamp_center();
                    invalidate(hwnd);
                    true
                })
                .unwrap_or(false);
                if !panned {
                    with_view(|v| {
                        v.hover_retry(x as f64, y as f64);
                        if v.crop_drag.is_none() {
                            v.hover_print(Some((x as f64, y as f64)));
                        }
                    });
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
                    v.hover_print(None);
                });
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                let (x, y) = point(lparam);
                if let Some((target, _)) = with_view(|v| v.pressed.take()).flatten() {
                    if msg == WM_LBUTTONUP {
                        let _ = ReleaseCapture();
                        let at = with_view(|v| (v.print_hit(x as f64, y as f64).0 == Some(target)).then(|| v.target_rect(target))).flatten();
                        if let Some(rect) = at {
                            let canvas = hwnd.0 as isize;
                            post(move |app: &mut App| print::act(app, HWND(canvas as *mut _), target, rect));
                        }
                    }
                    return LRESULT(0);
                }
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
                    let over_target = with_view(|v| v.hot.is_some() && v.crop_drag.is_none()).unwrap_or(false);
                    if over_target {
                        set_cursor(IDC_HAND);
                    } else if !cropping {
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
        let (r, s) = shown_pixels(&crop, size, 0, false);
        assert_eq!((r, s), (Rect::new(100.0, 160.0, 500.0, 200.0), size));
        let (r, s) = shown_pixels(&crop, size, 1, false);
        assert_eq!((r, s), (Rect::new(440.0, 100.0, 200.0, 500.0), Size::new(800.0, 1000.0)));
        let (r, _) = shown_pixels(&crop, size, 4, false);
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
