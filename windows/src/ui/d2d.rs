//! Direct2D and DirectWrite painting into the GDI back buffers (`draw.rs`),
//! so the controls stay real windows while their drawing moves over one at
//! a time: smooth shapes, the pencil marks (`negs_marks`) and Newsreader,
//! the macOS app's serif, with its optical size and weight axes.
//!
//! - The factories and fonts are made once per process, off the first
//!   frame: `prepare` starts them on a thread of their own after the
//!   inspector is shown, and the first paint that needs them waits for that
//!   (or makes them itself).
//! - Newsreader is read from the executable (`include_bytes!`) through an
//!   in-memory font loader; nothing is installed. On Windows 10 1803 and
//!   later (`IDWriteFactory6`) each format sets `wght` and `opsz`, the
//!   optical size following the point size as the mockup's
//!   `font-optical-sizing: auto` does. On 1703–1709 the font's named
//!   instances stand in (opsz 18); before that, Segoe UI.
//! - Painting allocates nothing once warm: formats, text layouts and
//!   geometry are cached per thread; a lost render target is made again.
//! - The DC render target draws in software: no Direct3D device to make
//!   (10–30 ms) and nothing to lose with the GPU.
//!
//! Coordinates are DIPs (the macOS app's points): the target's DPI is the
//! window's, so a 15 pt title is 15 DIPs, 22.5 pixels at 150 %.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{BOOL, D2DERR_RECREATE_TARGET, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC;
use windows::core::{Interface, PCWSTR, w};

use negs_marks::{Path, Radii, Seg};

use super::theme::{self, Tokens};

static NEWSREADER: &[u8] = include_bytes!("../../../macos/Fonts/Newsreader-Negs.ttf");
static NEWSREADER_ITALIC: &[u8] = include_bytes!("../../../macos/Fonts/Newsreader-Negs-Italic.ttf");

// MARK: Factories and fonts, once per process

/// What every thread shares: the factories and the fonts, all free-threaded
/// (a multithreaded Direct2D factory and the shared DirectWrite one).
pub struct Shared {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    fonts: Fonts,
    /// What making them cost.
    pub cost: Cost,
}

// SAFETY: the Direct2D factory is made multithreaded and the DirectWrite
// factory shared; both, and the font collections made from them, may be
// used from any thread.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[derive(Clone, Copy, Debug, Default)]
pub struct Cost {
    pub d2d: Duration,
    pub dwrite: Duration,
    /// Reading Newsreader and finding the system font.
    pub fonts: Duration,
    pub thread: bool,
}

/// How Newsreader is set on this Windows.
pub enum Serif {
    /// 1803 and later: the variable font, axes set per format.
    Variable { f6: IDWriteFactory6, collection: IDWriteFontCollection, family: Vec<u16> },
    /// 1703–1709: the font's named instances (400–700 at opsz 18).
    Instances { collection: IDWriteFontCollection, family: Vec<u16> },
    /// Before 1703: the system font instead.
    Missing,
}

struct Fonts {
    serif: Serif,
    /// The system font: Segoe UI Variable (Windows 11) or Segoe UI.
    ui: Vec<u16>,
    /// Segoe UI Variable, whose axes are set as Newsreader's are.
    ui_axes: Option<IDWriteFactory6>,
}

static SHARED: OnceLock<Option<Shared>> = OnceLock::new();

/// The factories and fonts, made now if `prepare` hasn't (a caller racing
/// `prepare`'s thread waits for it rather than making a second set).
pub fn shared() -> Option<&'static Shared> {
    SHARED.get_or_init(|| Shared::new(false).ok()).as_ref()
}

/// Sets each sample once in a scratch bitmap on this thread, so DirectWrite
/// has the faces, sizes and shaping the first frame asks for (the title
/// band's name, the empty window's words) before the window paints them.
pub fn warm(samples: &[(&str, TextStyle)]) {
    for (text, style) in samples {
        let _ = mask(text, style, 0.0, Align::Leading, 1.0, false);
    }
}

/// Starts making the factories and reading the fonts on a thread of their
/// own, then calls `done` there. Startup calls this first thing, so the
/// title bar's first paint finds them made.
pub fn prepare(done: impl FnOnce(&Shared) + Send + 'static) {
    if SHARED.get().is_some() {
        return;
    }
    let _ = std::thread::Builder::new().name("negs-drawing".into()).spawn(move || {
        if let Some(shared) = SHARED.get_or_init(|| Shared::new(true).ok()).as_ref() {
            // The WARP device beside the first frame's words, so it is there
            // when the inspector first paints, even while the engine starts.
            let _ = std::thread::Builder::new().name("negs-warp".into()).spawn(move || {
                let start = crate::platform::qpc();
                let made = WARP.get_or_init(|| Warp::new(shared).ok()).is_some();
                let at = crate::platform::qpc();
                let ms = crate::platform::qpc_ms(start, at);
                super::post(move |app: &mut super::App| app.report.line(&format!("drawing_warp {at} made {made} in {ms:.1} ms")));
            });
            done(shared);
        }
    });
}

// MARK: Drawing on the calling thread

/// A Direct2D device on WARP, Direct3D's software rasteriser, made single
/// threaded and told to make no threads of its own: everything it draws, it
/// draws on the thread that paints. The DC render target's rasteriser hands
/// its work to a pool of threads, and while the engine starts and fills
/// every core, each control's paint waited for that pool (40 ms in all when
/// the inspector first showed). Without `SINGLETHREADED`, reading the pixels
/// back still waited, most of all while the machine was busy (50 ms).
struct Warp(ID2D1Device);

// SAFETY: the device comes from the multithreaded factory, which takes its
// lock around every use of the device, so the single-threaded Direct3D
// device is never used by two threads at once; each thread draws through a
// device context of its own.
unsafe impl Send for Warp {}
unsafe impl Sync for Warp {}

static WARP: OnceLock<Option<Warp>> = OnceLock::new();

impl Warp {
    fn new(shared: &Shared) -> windows::core::Result<Self> {
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
        use windows::Win32::Graphics::Direct3D11::*;
        let factory: ID2D1Factory1 = shared.d2d.cast()?;
        let mut device = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED | D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
            let device: windows::Win32::Graphics::Dxgi::IDXGIDevice = device.ok_or(windows::core::Error::empty())?.cast()?;
            Ok(Self(factory.CreateDevice(&device)?))
        }
    }
}

/// One thread's device context on `WARP`, with the bitmap it draws in and a
/// copy the processor reads back, both as large as the largest paint yet.
struct WarpTarget {
    context: ID2D1DeviceContext,
    rt: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    bitmaps: Option<(ID2D1Bitmap1, ID2D1Bitmap1, u32, u32)>,
}

impl WarpTarget {
    fn new(warp: &Warp) -> windows::core::Result<Self> {
        unsafe {
            let context = warp.0.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let brush = context.CreateSolidColorBrush(&Ink::hex(0).color(), None)?;
            let rt = context.cast()?;
            Ok(Self { context, rt, brush, bitmaps: None })
        }
    }

    /// The bitmaps, made again larger when `w` × `h` doesn't fit.
    fn bitmaps(&mut self, w: u32, h: u32) -> windows::core::Result<(ID2D1Bitmap1, ID2D1Bitmap1)> {
        if let Some((target, readback, bw, bh)) = &self.bitmaps {
            if w <= *bw && h <= *bh {
                return Ok((target.clone(), readback.clone()));
            }
        }
        let (old_w, old_h) = self.bitmaps.as_ref().map_or((0, 0), |b| (b.2, b.3));
        let (bw, bh) = (w.max(old_w).next_multiple_of(64), h.max(old_h).next_multiple_of(64));
        let props = |options: D2D1_BITMAP_OPTIONS| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let size = D2D_SIZE_U { width: bw, height: bh };
        let (target, readback) = unsafe {
            (
                self.context.CreateBitmap(size, None, 0, &props(D2D1_BITMAP_OPTIONS_TARGET))?,
                self.context.CreateBitmap(size, None, 0, &props(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?,
            )
        };
        self.bitmaps = Some((target.clone(), readback.clone(), bw, bh));
        Ok((target, readback))
    }
}

/// `paint` through `WARP` on this thread: the buffer's pixels (`bits`, rows
/// `stride` bytes apart, top down) go up, are drawn over and come back. None
/// when that path isn't there (not made yet, or the device was lost), for
/// the DC render target to paint instead.
fn paint_warp(device: &mut Device, shared: &'static Shared, (bits, stride): (*mut u8, usize), w: i32, h: i32, scale: f32, plain: bool, draw: &mut impl FnMut(&mut Painter)) -> Option<bool> {
    use windows::Win32::Graphics::Gdi::GdiFlush;
    let warp = WARP.get()?.as_ref()?;
    if w <= 0 || h <= 0 {
        return None;
    }
    let Device { warp: slot, caches, .. } = device;
    if slot.is_none() {
        *slot = WarpTarget::new(warp).ok();
    }
    let target = slot.as_mut()?;
    let (bitmap, readback) = target.bitmaps(w as u32, h as u32).ok()?;
    let rect = D2D_RECT_U { left: 0, top: 0, right: w as u32, bottom: h as u32 };
    unsafe {
        let _ = GdiFlush();
        bitmap.CopyFromMemory(Some(&rect), bits as *const _, stride as u32).ok()?;
        target.context.SetTarget(&bitmap);
        target.context.SetDpi(96.0 * scale, 96.0 * scale);
        target.context.BeginDraw();
        target.context.SetTransform(&IDENTITY);
        target.context.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_DEFAULT);
    }
    let mut painter = Painter { shared, rt: &target.rt, brush: &target.brush, caches, plain };
    draw(&mut painter);
    let ended = unsafe { target.context.EndDraw(None, None) };
    unsafe {
        target.context.SetTarget(None::<&ID2D1Image>);
    }
    match ended {
        Ok(()) => {}
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            *slot = None;
            return None;
        }
        Err(_) => return Some(false),
    }
    unsafe {
        if readback.CopyFromBitmap(Some(&D2D_POINT_2U { x: 0, y: 0 }), &bitmap, Some(&rect)).is_err() {
            return Some(false);
        }
        let Ok(mapped) = readback.Map(D2D1_MAP_OPTIONS_READ) else { return Some(false) };
        for y in 0..h as usize {
            std::ptr::copy_nonoverlapping(mapped.bits.add(y * mapped.pitch as usize), bits.add(y * stride), w as usize * 4);
        }
        let _ = readback.Unmap();
    }
    Some(true)
}

impl Shared {
    fn new(thread: bool) -> windows::core::Result<Self> {
        let start = Instant::now();
        let d2d: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)? };
        let made_d2d = Instant::now();
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let made_dwrite = Instant::now();
        let fonts = Fonts::new(&dwrite);
        let cost = Cost { d2d: made_d2d - start, dwrite: made_dwrite - made_d2d, fonts: made_dwrite.elapsed(), thread };
        Ok(Self { d2d, dwrite, fonts, cost })
    }

    pub fn serif(&self) -> &Serif {
        &self.fonts.serif
    }

    /// The system font's family name.
    pub fn ui_family(&self) -> String {
        String::from_utf16_lossy(&self.fonts.ui[..self.fonts.ui.len() - 1])
    }
}

impl Fonts {
    fn new(dwrite: &IDWriteFactory) -> Self {
        let f6 = dwrite.cast::<IDWriteFactory6>().ok();
        let serif = newsreader(dwrite, f6.as_ref()).unwrap_or(Serif::Missing);
        // Windows 11's variable system font, else Segoe UI.
        let variable = f6.as_ref().is_some_and(|f6| unsafe {
            let Ok(system) = f6.GetSystemFontCollection(false, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC) else { return false };
            let (mut index, mut exists) = (0u32, BOOL(0));
            system.FindFamilyName(w!("Segoe UI Variable"), &mut index, &mut exists).is_ok() && exists.as_bool()
        });
        let ui = wide(if variable { "Segoe UI Variable" } else { "Segoe UI" });
        Self { serif, ui, ui_axes: if variable { f6 } else { None } }
    }
}

/// Newsreader from memory, as one family with both faces.
fn newsreader(dwrite: &IDWriteFactory, f6: Option<&IDWriteFactory6>) -> windows::core::Result<Serif> {
    unsafe {
        let f5: IDWriteFactory5 = dwrite.cast()?;
        let loader = f5.CreateInMemoryFontFileLoader()?;
        dwrite.RegisterFontFileLoader(&loader)?;
        let builder = IDWriteFactory5::CreateFontSetBuilder(&f5)?;
        for data in [NEWSREADER, NEWSREADER_ITALIC] {
            // The factory owns the (static) bytes: DirectWrite doesn't copy them.
            let file = loader.CreateInMemoryFontFileReference(dwrite, data.as_ptr() as *const _, data.len() as u32, dwrite)?;
            builder.AddFontFile(&file)?;
        }
        let set = builder.CreateFontSet()?;
        if let Some(f6) = f6 {
            let collection: IDWriteFontCollection = f6.CreateFontCollectionFromFontSet(&set, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC)?.cast()?;
            let family = family_name(&collection)?;
            return Ok(Serif::Variable { f6: f6.clone(), collection, family });
        }
        let collection: IDWriteFontCollection = IDWriteFactory3::CreateFontCollectionFromFontSet(&f5, &set)?.cast()?;
        let family = family_name(&collection)?;
        Ok(Serif::Instances { collection, family })
    }
}

/// The first family's name, null-terminated.
fn family_name(collection: &IDWriteFontCollection) -> windows::core::Result<Vec<u16>> {
    unsafe {
        let family = IDWriteFontCollection::GetFontFamily(collection, 0)?;
        let names = IDWriteFontFamily::GetFamilyNames(&family)?;
        let n = names.GetStringLength(0)? as usize;
        let mut name = vec![0u16; n + 1];
        names.GetString(0, &mut name)?;
        Ok(name)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// MARK: Type

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Newsreader: names, tabs, values, hints, headings, the caption, buttons.
    Serif,
    /// The system font: control labels, the status line, small UI text.
    Ui,
}

/// A text style in the design's terms (IMPLEMENTATION.md §2.2). Figures
/// are always tabular and lining.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub face: Face,
    /// Points (DIPs).
    pub size: f32,
    /// 400–700 for Newsreader.
    pub weight: u16,
    pub italic: bool,
    /// Letter spacing, in ems.
    pub tracking: f32,
    /// A multiple of the size; 0 for the font's own.
    pub line_height: f32,
    /// Wraps at the width it's given, else one line.
    pub wrap: bool,
}

impl TextStyle {
    pub const fn serif(size: f32, weight: u16) -> Self {
        Self { face: Face::Serif, size, weight, italic: false, tracking: 0.0, line_height: 0.0, wrap: false }
    }
    pub const fn ui(size: f32, weight: u16) -> Self {
        Self { face: Face::Ui, ..Self::serif(size, weight) }
    }
    pub const fn italic(self) -> Self {
        Self { italic: true, ..self }
    }
    pub const fn tracking(self, em: f32) -> Self {
        Self { tracking: em, ..self }
    }
    pub const fn line_height(self, multiple: f32) -> Self {
        Self { line_height: multiple, wrap: true, ..self }
    }

    /// Plain sets the system font instead, upright, at 0.95 of the size
    /// (the same cap height as Newsreader's, near the same width), as the
    /// macOS app does with SF.
    fn resolved(self, plain: bool) -> Self {
        if !plain || self.face != Face::Serif {
            return self;
        }
        let weight = if self.weight >= 650 { 600 } else if self.weight >= 500 { 500 } else { 400 };
        Self { face: Face::Ui, size: self.size * 0.95, weight, italic: false, ..self }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    face: Face,
    size: u32,
    weight: u16,
    italic: bool,
    line_height: u32,
}

impl FormatKey {
    fn of(style: &TextStyle) -> Self {
        Self { face: style.face, size: style.size.to_bits(), weight: style.weight, italic: style.italic, line_height: style.line_height.to_bits() }
    }
}

fn make_format(shared: &Shared, style: &TextStyle) -> windows::core::Result<IDWriteTextFormat> {
    let fonts = &shared.fonts;
    let (size, weight) = (style.size, style.weight as f32);
    let axes = |weight: f32, opsz: f32| {
        [
            DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT, value: weight },
            DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE, value: opsz },
            DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_ITALIC, value: if style.italic { 1.0 } else { 0.0 } },
        ]
    };
    let classic = |family: PCWSTR, collection: Option<&IDWriteFontCollection>| unsafe {
        let italic = if style.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
        shared.dwrite.CreateTextFormat(family, collection, DWRITE_FONT_WEIGHT(style.weight as i32), italic, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
    };
    let format: IDWriteTextFormat = unsafe {
        match (style.face, &fonts.serif) {
            // The subset keeps wght 400–700 and the whole opsz axis, 6–72.
            (Face::Serif, Serif::Variable { f6, collection, family }) => {
                f6.CreateTextFormat(PCWSTR(family.as_ptr()), collection, &axes(weight.clamp(400.0, 700.0), size.clamp(6.0, 72.0)), size, w!("en-us"))?.cast()?
            }
            (Face::Serif, Serif::Instances { collection, family }) => classic(PCWSTR(family.as_ptr()), Some(collection))?,
            _ => match &fonts.ui_axes {
                Some(f6) => f6.CreateTextFormat(PCWSTR(fonts.ui.as_ptr()), None, &axes(weight, size)[..2], size, w!("en-us"))?.cast()?,
                None => classic(PCWSTR(fonts.ui.as_ptr()), None)?,
            },
        }
    };
    unsafe {
        format.SetWordWrapping(if style.wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
        if style.line_height > 0.0 {
            let line = size * style.line_height;
            format.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, line * 0.8)?;
        }
    }
    Ok(format)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Leading,
    Center,
    Trailing,
}

/// A laid-out line's size, in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// The advance, trailing spaces included, so words can follow on.
    pub width: f32,
    pub height: f32,
    /// From the top of the layout to the first line's baseline.
    pub baseline: f32,
}

// MARK: Colour

/// A colour to paint with: sRGB and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub rgb: u32,
    pub alpha: f32,
}

impl Ink {
    /// A token, 0xRRGGBB.
    pub const fn hex(rgb: u32) -> Self {
        Self { rgb, alpha: 1.0 }
    }
    /// A GDI colour, 0x00bbggrr.
    pub const fn gdi(color: theme::Color) -> Self {
        Self { rgb: ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff), alpha: 1.0 }
    }
    pub const fn alpha(self, alpha: f32) -> Self {
        Self { alpha: self.alpha * alpha, ..self }
    }
    fn color(self) -> D2D1_COLOR_F {
        let c = |shift: u32| ((self.rgb >> shift) & 0xff) as f32 / 255.0;
        D2D1_COLOR_F { r: c(16), g: c(8), b: c(0), a: self.alpha }
    }
}

// MARK: Per thread: the target and the caches

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
}

struct Target {
    dc: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
}

impl Target {
    /// Opaque (ClearType text), or premultiplied alpha for a layered
    /// window's pixels (greyscale text).
    fn new(shared: &Shared, premultiplied: bool) -> windows::core::Result<Self> {
        let alpha = if premultiplied { D2D1_ALPHA_MODE_PREMULTIPLIED } else { D2D1_ALPHA_MODE_IGNORE };
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: alpha },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        unsafe {
            let dc = shared.d2d.CreateDCRenderTarget(&props)?;
            let brush = dc.CreateSolidColorBrush(&Ink::hex(0).color(), None)?;
            let rt = dc.cast()?;
            Ok(Self { dc, rt, brush })
        }
    }
}

/// Geometry made once: one figure set, one turned round its picture's
/// centre by some degrees, or strokes each with its opacity.
enum Shape {
    One(ID2D1PathGeometry),
    Tilted(ID2D1PathGeometry, f64),
    Strokes(Vec<(ID2D1PathGeometry, f32)>),
}

#[derive(Default)]
struct Caches {
    round: Option<ID2D1StrokeStyle>,
    typography: Option<IDWriteTypography>,
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<u64, (Box<str>, IDWriteTextLayout)>,
    shapes: HashMap<u64, Shape>,
}

/// Past these, a cache starts again (a window of new words or names).
const MOST_LAYOUTS: usize = 1024;
const MOST_SHAPES: usize = 512;

#[derive(Default)]
struct Device {
    /// Drawing on this thread, once `WARP` is made.
    warp: Option<WarpTarget>,
    target: Option<Target>,
    /// For layered windows (the alert card): premultiplied pixels.
    layer: Option<Target>,
    caches: Caches,
}

thread_local! {
    static DEVICE: RefCell<Device> = RefCell::new(Device::default());
}

/// Paints into `dc` (a `w` × `h` pixel buffer) with Direct2D, in DIPs at
/// `scale`, with `plain` marks and type (Plain). Draws a second time on a
/// fresh target if the first was lost. False when Direct2D isn't there or
/// drawing failed; the buffer is then as `draw` left it, or untouched.
pub fn paint(dc: HDC, w: i32, h: i32, scale: f32, plain: bool, draw: impl FnMut(&mut Painter)) -> bool {
    paint_with(dc, None, w, h, scale, plain, false, draw)
}

/// `paint` into a buffer whose pixels this thread can reach (`bits`, top
/// down, rows `stride` bytes apart; `dc` has it selected): drawn on this
/// thread, through `WARP`, once that is made.
pub fn paint_pixels(dc: HDC, bits: *mut u8, stride: usize, w: i32, h: i32, scale: f32, plain: bool, draw: impl FnMut(&mut Painter)) -> bool {
    paint_with(dc, (!bits.is_null()).then_some((bits, stride)), w, h, scale, plain, false, draw)
}

/// `paint` into a 32-bit buffer whose alpha counts: premultiplied pixels for
/// `UpdateLayeredWindow`, text in greyscale. Clear it first.
pub fn paint_layer(dc: HDC, w: i32, h: i32, scale: f32, plain: bool, draw: impl FnMut(&mut Painter)) -> bool {
    paint_with(dc, None, w, h, scale, plain, true, draw)
}

fn paint_with(dc: HDC, pixels: Option<(*mut u8, usize)>, w: i32, h: i32, scale: f32, plain: bool, layer: bool, mut draw: impl FnMut(&mut Painter)) -> bool {
    let Some(shared) = shared() else { return false };
    DEVICE.with(|cell| {
        let Ok(mut device) = cell.try_borrow_mut() else { return false };
        let device = &mut *device;
        if let Some(pixels) = pixels.filter(|_| !layer) {
            if let Some(drawn) = paint_warp(device, shared, pixels, w, h, scale, plain, &mut draw) {
                return drawn;
            }
        }
        for _ in 0..2 {
            let slot = if layer { &mut device.layer } else { &mut device.target };
            if slot.is_none() {
                *slot = Target::new(shared, layer).ok();
            }
            let Some(target) = slot.as_ref() else { return false };
            unsafe {
                if target.dc.BindDC(dc, &RECT { left: 0, top: 0, right: w, bottom: h }).is_err() {
                    return false;
                }
                target.dc.SetDpi(96.0 * scale, 96.0 * scale);
                target.dc.BeginDraw();
                target.dc.SetTransform(&IDENTITY);
                target.dc.SetTextAntialiasMode(if layer { D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE } else { D2D1_TEXT_ANTIALIAS_MODE_DEFAULT });
            }
            let mut painter = Painter { shared, rt: &target.rt, brush: &target.brush, caches: &mut device.caches, plain };
            draw(&mut painter);
            match unsafe { target.dc.EndDraw(None, None) } {
                Ok(()) => return true,
                Err(e) if e.code() == D2DERR_RECREATE_TARGET => *(if layer { &mut device.layer } else { &mut device.target }) = None,
                Err(_) => return false,
            }
        }
        false
    })
}

/// Words as a coverage mask (one byte a pixel, greyscale antialiased), for
/// the canvas to colour on the GPU: Newsreader there too. `scale` is pixels
/// per DIP; the mask is in pixels.
pub struct Mask {
    pub coverage: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Marks drawn in white by `draw` (in DIPs) on a `w` × `h` DIP mask, as a
/// coverage mask: the canvas colours them on the GPU, as it does words.
pub fn shape_mask(w: f32, h: f32, scale: f32, plain: bool, mut draw: impl FnMut(&mut Painter)) -> Option<Mask> {
    use windows::Win32::Graphics::Gdi::*;
    let (pw, ph) = (((w * scale).ceil() as i32).max(1), ((h * scale).ceil() as i32).max(1));
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: pw,
                biHeight: -ph,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let mut result = None;
        if let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = SelectObject(dc, bitmap);
            let drawn = paint(dc, pw, ph, scale, plain, |p| {
                p.grayscale_text();
                p.fill_rect(RectF::new(0.0, 0.0, pw as f32 / scale, ph as f32 / scale), Ink::hex(0));
                draw(p);
            });
            let _ = GdiFlush();
            if drawn && !bits.is_null() {
                let pixels = std::slice::from_raw_parts(bits as *const u8, (pw * ph * 4) as usize);
                let coverage = pixels.chunks_exact(4).map(|p| p[1]).collect();
                result = Some(Mask { coverage, width: pw as u32, height: ph as u32 });
            }
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap);
        }
        let _ = DeleteDC(dc);
        result
    }
}

pub fn mask(text: &str, style: &TextStyle, width: f32, align: Align, scale: f32, plain: bool) -> Option<Mask> {
    use windows::Win32::Graphics::Gdi::*;
    if text.is_empty() {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        // Measured first on a one-pixel buffer: the layout is then cached.
        let dib = |w: i32, h: i32| {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok().map(|b| (b, bits))
        };
        let mut result = None;
        if let Some((one, _)) = dib(1, 1) {
            let old = SelectObject(dc, one);
            let mut size = TextMetrics::default();
            paint(dc, 1, 1, scale, plain, |p| size = p.measure_in(text, style, width, align));
            SelectObject(dc, old);
            let _ = DeleteObject(one);
            let layout_width = if width > 0.0 { width } else { size.width };
            let (w, h) = (((layout_width * scale).ceil() as i32 + 2).max(1), ((size.height * scale).ceil() as i32 + 2).max(1));
            if let Some((bitmap, bits)) = dib(w, h) {
                let old = SelectObject(dc, bitmap);
                let drawn = paint(dc, w, h, scale, plain, |p| {
                    p.grayscale_text();
                    p.fill_rect(RectF::new(0.0, 0.0, w as f32 / scale, h as f32 / scale), Ink::hex(0));
                    p.text_in(text, style, RectF::new(1.0 / scale, 1.0 / scale, layout_width, 0.0), align, Ink::hex(0xffffff));
                });
                let _ = GdiFlush();
                if drawn && !bits.is_null() {
                    let pixels = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
                    // White on black: the green channel is the coverage.
                    let coverage = pixels.chunks_exact(4).map(|p| p[1]).collect();
                    result = Some(Mask { coverage, width: w as u32, height: h as u32 });
                }
                SelectObject(dc, old);
                let _ = DeleteObject(bitmap);
            }
        }
        let _ = DeleteDC(dc);
        result
    }
}

const IDENTITY: Matrix3x2 = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 };

/// Turned `degrees` clockwise round (`cx`, `cy`), then moved by (`tx`, `ty`).
fn turned(degrees: f64, cx: f64, cy: f64, tx: f64, ty: f64) -> Matrix3x2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Matrix3x2 {
        M11: c as f32,
        M12: s as f32,
        M21: -s as f32,
        M22: c as f32,
        M31: (cx - (cx * c - cy * s) + tx) as f32,
        M32: (cy - (cx * s + cy * c) + ty) as f32,
    }
}

fn moved(tx: f64, ty: f64) -> Matrix3x2 {
    Matrix3x2 { M31: tx as f32, M32: ty as f32, ..IDENTITY }
}

fn key(parts: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// A length as a cache key, to a quarter DIP.
fn q(v: f64) -> i64 {
    (v * 4.0).round() as i64
}

fn geometry(factory: &ID2D1Factory, segs: &[Seg], filled: bool) -> windows::core::Result<ID2D1PathGeometry> {
    let p = |p: negs_marks::Pt| D2D_POINT_2F { x: p.x as f32, y: p.y as f32 };
    unsafe {
        let geometry = factory.CreatePathGeometry()?;
        let sink = geometry.Open()?;
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        let begin = if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
        let mut open = false;
        for seg in segs {
            match *seg {
                Seg::Move(at) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(p(at), begin);
                    open = true;
                }
                Seg::Line(to) => sink.AddLine(p(to)),
                Seg::Cubic(c1, c2, to) => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p(c1), point2: p(c2), point3: p(to) }),
                Seg::Close => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    }
                    open = false;
                }
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
        Ok(geometry)
    }
}

/// Outlines as one filled path.
fn outlines<'a>(outlines: impl IntoIterator<Item = &'a Vec<negs_marks::Pt>>) -> Vec<Seg> {
    outlines.into_iter().flat_map(|o| Path::polygon(o).0).collect()
}

/// Which hand-drawn box round a picture (PickBox.swift).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoxInk {
    /// A pick in the grid: 3 pt out.
    Grid,
    /// A pick in the strip: 2 pt out, short overshoots.
    Strip,
    /// Selection in the grid: a second box 8 pt out, heavier.
    Selection,
}

fn shape_in<'c>(caches: &'c mut Caches, factory: &ID2D1Factory, id: u64, make: impl FnOnce(&ID2D1Factory) -> Option<Shape>) -> Option<&'c Shape> {
    if !caches.shapes.contains_key(&id) {
        if caches.shapes.len() >= MOST_SHAPES {
            caches.shapes.clear();
        }
        let shape = make(factory)?;
        caches.shapes.insert(id, shape);
    }
    caches.shapes.get(&id)
}

fn draw_on(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, g: &ID2D1PathGeometry, transform: Matrix3x2, stroke: Option<f32>, style: Option<&ID2D1StrokeStyle>, ink: Ink) {
    unsafe {
        brush.SetColor(&ink.color());
        rt.SetTransform(&transform);
        match stroke {
            Some(width) => rt.DrawGeometry(g, brush, width, style),
            None => rt.FillGeometry(g, brush, None),
        }
        rt.SetTransform(&IDENTITY);
    }
}

/// A mark's dark halo (drop-shadow 1.5 pt black at 0.95, then 4 pt at 0.45
/// on macOS), here as two soft outlines under it: Direct2D's shadow effect
/// needs a device context, which a DC target over GDI doesn't give.
fn halo_on(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, g: &ID2D1PathGeometry, transform: Matrix3x2, round: Option<&ID2D1StrokeStyle>) {
    draw_on(rt, brush, g, transform, Some(4.0), round, Ink::hex(0).alpha(0.22));
    draw_on(rt, brush, g, transform, Some(1.5), round, Ink::hex(0).alpha(0.6));
}

// MARK: Painting

/// Drawing on one bound buffer, in DIPs. Made by `paint`.
pub struct Painter<'a> {
    shared: &'static Shared,
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    caches: &'a mut Caches,
    plain: bool,
}

impl Painter<'_> {
    pub fn plain(&self) -> bool {
        self.plain
    }

    fn ink(&self, ink: Ink) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&ink.color()) };
        self.brush
    }

    fn round(&mut self) -> Option<ID2D1StrokeStyle> {
        if self.caches.round.is_none() {
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            self.caches.round = unsafe { self.shared.d2d.CreateStrokeStyle(&props, None) }.ok();
        }
        self.caches.round.clone()
    }

    pub fn fill_rect(&mut self, r: RectF, ink: Ink) {
        unsafe { self.rt.FillRectangle(&r.d2d(), self.ink(ink)) };
    }

    /// A rectangle's outline, centred on its edge.
    pub fn stroke_rect(&mut self, r: RectF, width: f32, ink: Ink) {
        unsafe { self.rt.DrawRectangle(&r.d2d(), self.ink(ink), width, None) };
    }

    pub fn fill_round_rect(&mut self, r: RectF, radius: f32, ink: Ink) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.ink(ink)) };
    }

    /// A rounded rectangle's outline, centred on its edge.
    pub fn stroke_round_rect(&mut self, r: RectF, radius: f32, width: f32, ink: Ink) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.rt.DrawRoundedRectangle(&rr, self.ink(ink), width, None) };
    }

    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, ink: Ink) {
        let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: rx, radiusY: ry };
        unsafe { self.rt.FillEllipse(&e, self.ink(ink)) };
    }

    /// A line with round caps.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, ink: Ink) {
        let round = self.round();
        unsafe { self.rt.DrawLine(D2D_POINT_2F { x: a.0, y: a.1 }, D2D_POINT_2F { x: b.0, y: b.1 }, self.ink(ink), width, round.as_ref()) };
    }

    /// The cached geometry for `id`, made by `make` the first time.
    fn shape(&mut self, id: u64, make: impl FnOnce(&ID2D1Factory) -> Option<Shape>) -> Option<&Shape> {
        shape_in(self.caches, &self.shared.d2d, id, make)
    }

    /// A path of one's own, cached under `id`: filled, or stroked `width`
    /// wide with round caps and joins, moved by (`x`, `y`).
    pub fn path(&mut self, id: u64, make: impl FnOnce() -> Path, x: f32, y: f32, stroke: Option<f32>, ink: Ink) {
        let round = self.round();
        let filled = stroke.is_none();
        let Some(Shape::One(g)) = self.shape(key(("path", id, filled)), |f| geometry(f, &make().0, filled).ok().map(Shape::One)) else { return };
        let g = g.clone();
        self.draw(&g, moved(x as f64, y as f64), stroke, round.as_ref(), ink);
    }

    fn draw(&self, g: &ID2D1PathGeometry, transform: Matrix3x2, stroke: Option<f32>, style: Option<&ID2D1StrokeStyle>, ink: Ink) {
        draw_on(self.rt, self.brush, g, transform, stroke, style, ink);
    }

    fn halo(&mut self, g: &ID2D1PathGeometry, transform: Matrix3x2) {
        let round = self.round();
        halo_on(self.rt, self.brush, g, transform, round.as_ref());
    }

    // MARK: The pencil's marks

    /// The pencil stroke stretched along `r`, the 6 pt box under a word
    /// (`negs_marks::PENCIL_HEIGHT` tall): 2.1 pt, round caps. Straight in Plain.
    pub fn pencil(&mut self, r: RectF, ink: Ink) {
        self.pencil_width(r, negs_marks::PENCIL_WIDTH as f32, ink);
    }

    pub fn pencil_width(&mut self, r: RectF, width: f32, ink: Ink) {
        let (w, h, plain) = (r.w as f64, r.h as f64, self.plain);
        let round = self.round();
        let id = key(("pencil", q(w), q(h), plain));
        let Some(Shape::One(g)) = self.shape(id, |f| geometry(f, &negs_marks::pencil(negs_marks::Rect::new(0.0, 0.0, w, h), plain).0, false).ok().map(Shape::One)) else { return };
        let g = g.clone();
        self.draw(&g, moved(r.x as f64, r.y as f64), Some(width), round.as_ref(), ink);
    }

    /// The mark under a word, in `tokens`' way: the pencil in `accent` (the
    /// film's on the strip and tray), or on Light table the highlighter,
    /// a yellow swipe lifted behind the lower half of the word. Draw it
    /// before the word, so the word stays on top.
    pub fn underline(&mut self, r: RectF, tokens: &Tokens, on_film: bool) {
        if tokens.highlighter {
            let lifted = RectF { y: r.y - 5.0, ..r };
            let ink = Ink::hex(theme::HIGHLIGHTER_INK).alpha(theme::HIGHLIGHTER_OPACITY);
            self.pencil_width(lifted, negs_marks::HIGHLIGHTER_WIDTH as f32, ink);
        } else {
            self.pencil(r, Ink::hex(if on_film { tokens.film.accent } else { tokens.hex.accent }));
        }
    }

    /// A ring round `r`, turned `tilt` degrees (−0.8 round thumbnails, −1
    /// round tiles; upright in Plain), 2 pt wide.
    pub fn ring(&mut self, r: RectF, radii: Radii, tilt: f64, ink: Ink) {
        let (w, h, plain) = (r.w as f64, r.h as f64, self.plain);
        let id = key(("ring", q(w), q(h), plain, radii.relative, radii.horizontal.map(f64::to_bits), radii.vertical.map(f64::to_bits)));
        let Some(Shape::One(g)) = self.shape(id, |f| geometry(f, &negs_marks::ring(negs_marks::Rect::new(0.0, 0.0, w, h), radii, plain).0, false).ok().map(Shape::One)) else { return };
        let g = g.clone();
        let tilt = negs_marks::ring_tilt(tilt, plain);
        self.draw(&g, turned(tilt, w / 2.0, h / 2.0, r.x as f64, r.y as f64), Some(negs_marks::RING_LINE_WIDTH as f32), None, ink);
    }

    /// The `index`th hand-drawn star of `name`'s row, centred on (`cx`, `cy`).
    pub fn star(&mut self, name: &str, index: usize, cx: f32, cy: f32, ink: Ink, halo: bool) {
        let id = key(("star", name, index));
        let Some(Shape::One(g)) = self.shape(id, |f| geometry(f, &negs_marks::star_path(name, index).0, true).ok().map(Shape::One)) else { return };
        let g = g.clone();
        let at = moved(cx as f64, cy as f64);
        if halo {
            self.halo(&g, at);
        }
        self.draw(&g, at, None, None, ink);
    }

    /// A reject's cross over the picture at `r`, as `name` draws it.
    pub fn cross(&mut self, name: &str, r: RectF, ink: Ink, halo: bool) {
        let (w, h) = (r.w as f64, r.h as f64);
        let id = key(("cross", name, q(w), q(h)));
        let round = self.round();
        let Painter { shared, rt, brush, caches, .. } = self;
        let made = shape_in(caches, &shared.d2d, id, |f| {
            let strokes = negs_marks::cross_strokes(w, h, name);
            strokes.iter().map(|s| geometry(f, &Path::polygon(&s.outline).0, true).ok().map(|g| (g, s.opacity as f32))).collect::<Option<Vec<_>>>().map(Shape::Strokes)
        });
        let Some(Shape::Strokes(strokes)) = made else { return };
        let at = moved(r.x as f64, r.y as f64);
        if halo {
            for (g, _) in strokes {
                halo_on(rt, brush, g, at, round.as_ref());
            }
        }
        for (g, opacity) in strokes {
            draw_on(rt, brush, g, at, None, None, ink.alpha(*opacity));
        }
    }

    /// A hand-drawn box round the picture at `r`, as `name` draws it. Plain
    /// draws straight rounded boxes instead, without a halo: a pick 2 pt
    /// wide (3 pt out, 2 in the strip), selection 3 pt wide 7 pt out.
    pub fn pick_box(&mut self, which: BoxInk, name: &str, r: RectF, ink: Ink, halo: bool) {
        if self.plain {
            let (outset, width) = match which {
                BoxInk::Grid => (3.0, 2.0),
                BoxInk::Strip => (2.0, 2.0),
                BoxInk::Selection => (7.0, 3.0),
            };
            let o = outset;
            self.stroke_round_rect(RectF::new(r.x - o, r.y - o, r.w + 2.0 * o, r.h + 2.0 * o), 4.0, width, ink);
            return;
        }
        let (w, h) = (r.w as f64, r.h as f64);
        let id = key(("box", which, name, q(w), q(h)));
        let made = self.shape(id, |f| {
            let b = match which {
                BoxInk::Grid => negs_marks::pick_box_grid(w, h, name),
                BoxInk::Strip => negs_marks::pick_box_strip(w, h, name),
                BoxInk::Selection => negs_marks::pick_box_selection(w, h, name),
            };
            geometry(f, &outlines(&b.strokes), true).ok().map(|g| Shape::Tilted(g, b.tilt))
        });
        let Some(Shape::Tilted(g, tilt)) = made else { return };
        let (g, tilt) = (g.clone(), *tilt);
        let at = turned(tilt, w / 2.0, h / 2.0, r.x as f64, r.y as f64);
        if halo {
            self.halo(&g, at);
        }
        self.draw(&g, at, None, None, ink);
    }

    // MARK: Text

    /// `text` laid out in `style`, `width` wide (unbounded for 0), cached.
    pub fn layout(&mut self, text: &str, style: &TextStyle, width: f32, align: Align) -> Option<IDWriteTextLayout> {
        layout_in(self.shared, self.caches, self.plain, text, style, width, align)
    }

    /// The size of `text` in `style` on one line (or wrapped at `width`).
    pub fn measure(&mut self, text: &str, style: &TextStyle, width: f32) -> TextMetrics {
        self.measure_in(text, style, width, Align::Leading)
    }

    /// `measure` for the layout `text_in` makes with `align`.
    pub fn measure_in(&mut self, text: &str, style: &TextStyle, width: f32, align: Align) -> TextMetrics {
        if text.is_empty() {
            return TextMetrics::default();
        }
        let Some(layout) = self.layout(text, style, width, align) else { return TextMetrics::default() };
        metrics(&layout)
    }

    /// Text from here on in greyscale, as on a surface that isn't opaque.
    pub fn grayscale_text(&mut self) {
        unsafe { self.rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
    }

    /// Draws `draw` clipped to `r`.
    pub fn clipped(&mut self, r: RectF, draw: impl FnOnce(&mut Self)) {
        unsafe { self.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        draw(self);
        unsafe { self.rt.PopAxisAlignedClip() };
    }

    /// `text` with its layout's top left at (`x`, `y`); returns its size.
    pub fn text(&mut self, text: &str, style: &TextStyle, x: f32, y: f32, ink: Ink) -> TextMetrics {
        self.text_in(text, style, RectF::new(x, y, 0.0, 0.0), Align::Leading, ink)
    }

    /// `text` on one line turned a quarter anticlockwise, reading upward
    /// from (`x`, `y`): the line's start at `y`, its top along `x` (the
    /// tray's group names, `rotationEffect(.degrees(-90))`).
    pub fn text_up(&mut self, text: &str, style: &TextStyle, x: f32, y: f32, ink: Ink) -> TextMetrics {
        if text.is_empty() {
            return TextMetrics::default();
        }
        let Some(layout) = self.layout(text, style, 0.0, Align::Leading) else { return TextMetrics::default() };
        unsafe {
            self.rt.SetTransform(&turned(-90.0, 0.0, 0.0, x as f64, y as f64));
            self.rt.DrawTextLayout(D2D_POINT_2F { x: 0.0, y: 0.0 }, &layout, self.ink(ink), D2D1_DRAW_TEXT_OPTIONS_NONE);
            self.rt.SetTransform(&IDENTITY);
        }
        metrics(&layout)
    }

    /// `text` in `r`'s width (0: unbounded), aligned, from its top.
    pub fn text_in(&mut self, text: &str, style: &TextStyle, r: RectF, align: Align, ink: Ink) -> TextMetrics {
        if text.is_empty() {
            return TextMetrics::default();
        }
        let Some(layout) = self.layout(text, style, r.w, align) else { return TextMetrics::default() };
        unsafe { self.rt.DrawTextLayout(D2D_POINT_2F { x: r.x, y: r.y }, &layout, self.ink(ink), D2D1_DRAW_TEXT_OPTIONS_NONE) };
        metrics(&layout)
    }
}

fn format_in(shared: &Shared, caches: &mut Caches, style: &TextStyle) -> Option<IDWriteTextFormat> {
    let k = FormatKey::of(style);
    if let Some(f) = caches.formats.get(&k) {
        return Some(f.clone());
    }
    let format = make_format(shared, style).ok()?;
    caches.formats.insert(k, format.clone());
    Some(format)
}

fn typography_in(shared: &Shared, caches: &mut Caches) -> Option<IDWriteTypography> {
    if caches.typography.is_none() {
        caches.typography = unsafe {
            shared.dwrite.CreateTypography().ok().filter(|t| {
                [DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWRITE_FONT_FEATURE_TAG_LINING_FIGURES]
                    .into_iter()
                    .all(|tag| t.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: tag, parameter: 1 }).is_ok())
            })
        };
    }
    caches.typography.clone()
}

/// `text` laid out in `style`, `width` wide (unbounded for 0), cached per
/// thread: painting the same words again makes nothing.
fn layout_in(shared: &Shared, caches: &mut Caches, plain: bool, text: &str, style: &TextStyle, width: f32, align: Align) -> Option<IDWriteTextLayout> {
    let style = style.resolved(plain);
    let id = key((text, FormatKey::of(&style), style.tracking.to_bits(), style.wrap, width.to_bits(), align));
    if let Some((cached, layout)) = caches.layouts.get(&id) {
        if **cached == *text {
            return Some(layout.clone());
        }
    }
    let format = format_in(shared, caches, &style)?;
    let typography = typography_in(shared, caches);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let range = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
    let width = if width > 0.0 { width } else { 1.0e6 };
    let layout = unsafe {
        let layout = shared.dwrite.CreateTextLayout(&wide, &format, width, 1.0e6).ok()?;
        if let Some(typography) = &typography {
            let _ = layout.SetTypography(typography, range);
        }
        if style.tracking != 0.0 {
            if let Ok(layout1) = layout.cast::<IDWriteTextLayout1>() {
                let _ = layout1.SetCharacterSpacing(0.0, style.tracking * style.size, 0.0, range);
            }
        }
        let _ = layout.SetTextAlignment(match align {
            Align::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
        });
        layout
    };
    if caches.layouts.len() >= MOST_LAYOUTS {
        caches.layouts.clear();
    }
    caches.layouts.insert(id, (text.into(), layout.clone()));
    Some(layout)
}

/// The size of `text` in `style`, wrapped at `width` (0: one line), without
/// painting: for laying out rows whose height follows their words. The
/// layout is cached for the paint that follows. Zero when Direct2D isn't there.
pub fn measure_text(text: &str, style: &TextStyle, width: f32, plain: bool) -> TextMetrics {
    if text.is_empty() {
        return TextMetrics::default();
    }
    let Some(shared) = shared() else { return TextMetrics::default() };
    DEVICE.with(|cell| {
        let Ok(mut device) = cell.try_borrow_mut() else { return TextMetrics::default() };
        layout_in(shared, &mut device.caches, plain, text, style, width, Align::Leading).map_or_else(TextMetrics::default, |l| metrics(&l))
    })
}

fn metrics(layout: &IDWriteTextLayout) -> TextMetrics {
    unsafe {
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = layout.GetMetrics(&mut m);
        let mut lines = [DWRITE_LINE_METRICS::default(); 4];
        let mut count = 0u32;
        let baseline = if layout.GetLineMetrics(Some(&mut lines), &mut count).is_ok() && count > 0 { lines[0].baseline } else { m.height * 0.8 };
        TextMetrics { width: m.widthIncludingTrailingWhitespace, height: m.height, baseline }
    }
}
