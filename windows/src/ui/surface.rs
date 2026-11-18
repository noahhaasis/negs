//! Direct2D on the GPU, for the film strip and the grid (and the tray's
//! tiles once it moves here): one Direct3D 11 device with a Direct2D device
//! over it, a flip-model swap chain per window, thumbnails as GPU bitmaps,
//! words through DirectWrite and the marks as cached geometry, all through
//! `d2d::Painter`. Phase 7 measured this against the wgpu canvas device
//! (`grid_bench.rs`) and kept it: the same speed, far less code, and the
//! same painter as the rest of the window.
//!
//! - The device is made on a thread of its own after the inspector shows
//!   (`prepare`): 10–30 ms that launch never waits for. Until then the
//!   windows paint plain film with GDI (`ready` is false).
//! - Without a hardware adapter it's WARP, Direct3D's software rasteriser,
//!   so a PC without Vulkan (or any GPU driver) still has a strip and grid.
//! - A lost device (driver reset, GPU removed) is dropped and made again;
//!   swap chains and bitmaps from the old one are noticed by their
//!   generation and made again from the pixels kept beside them.
//! - Painting allocates nothing once warm: each window keeps its swap
//!   chain and the bitmap over its back buffer; the UI thread keeps one
//!   device context, its brush and its halos.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

use super::d2d::{self, Halos, Painter};

/// The device, shared by every window that paints through it.
struct Gpu {
    d3d: ID3D11Device,
    dxgi: IDXGIFactory2,
    d2d: ID2D1Device,
    /// A hardware adapter, not WARP.
    hardware: bool,
    generation: u64,
}

// SAFETY: Direct3D 11 devices are free-threaded, the DXGI factory too, and
// the Direct2D device comes from the multithreaded factory, which locks
// around each use; device contexts, which aren't, stay on the UI thread.
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

static GPU: Mutex<Option<Arc<Gpu>>> = Mutex::new(None);
static MAKING: Mutex<bool> = Mutex::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn current() -> Option<Arc<Gpu>> {
    GPU.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The device is made: the strip and grid paint through it.
pub fn ready() -> bool {
    current().is_some()
}

/// Whether it runs on a GPU rather than WARP (for the report).
pub fn hardware() -> Option<bool> {
    current().map(|g| g.hardware)
}

/// Makes the device on a thread of its own, then `done` on the UI thread
/// (the windows repaint through it). Once; again after a lost device.
pub fn prepare(done: impl FnOnce(&mut super::App, f64, bool) + Send + 'static) {
    {
        let mut making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
        if *making || ready() {
            return;
        }
        *making = true;
    }
    let _ = std::thread::Builder::new().name("negs-surface".into()).spawn(move || {
        let start = crate::platform::qpc();
        let made = make();
        let ms = crate::platform::qpc_ms(start, crate::platform::qpc());
        let hardware = made.as_ref().is_some_and(|g| g.hardware);
        if let Some(gpu) = made {
            *GPU.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(gpu));
        }
        *MAKING.lock().unwrap_or_else(|e| e.into_inner()) = false;
        super::post(move |app: &mut super::App| done(app, ms, hardware));
    });
}

fn make() -> Option<Gpu> {
    let shared = d2d::shared()?;
    let factory: ID2D1Factory1 = shared.factory().cast().ok()?;
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    for (driver, hardware) in [(D3D_DRIVER_TYPE_HARDWARE, true), (D3D_DRIVER_TYPE_WARP, false)] {
        let mut device = None;
        let made = unsafe {
            D3D11CreateDevice(None, driver, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None)
        };
        let Some(d3d) = made.ok().and(device) else { continue };
        let made = (|| -> windows::core::Result<Gpu> {
            let dxgi_device: IDXGIDevice1 = d3d.cast()?;
            // Each present waits for the one before: input never runs
            // more than a frame ahead of the screen.
            unsafe { dxgi_device.SetMaximumFrameLatency(1)? };
            let adapter = unsafe { dxgi_device.GetAdapter()? };
            let dxgi: IDXGIFactory2 = unsafe { adapter.GetParent()? };
            let d2d = unsafe { factory.CreateDevice(&dxgi_device)? };
            Ok(Gpu { d3d: d3d.clone(), dxgi, d2d, hardware, generation })
        })();
        if let Ok(gpu) = made {
            return Some(gpu);
        }
    }
    None
}

/// The device was lost: drop it, and make another.
fn lose(generation: u64) {
    let mut gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if gpu.as_ref().is_some_and(|g| g.generation == generation) {
        *gpu = None;
        drop(gpu);
        CONTEXT.with(|c| c.borrow_mut().take());
        prepare(|app, _, _| app.surfaces_ready());
    }
}

/// The UI thread's device context on the current device.
struct Context {
    context: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    halos: Halos,
    generation: u64,
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

fn with_context<R>(gpu: &Gpu, f: impl FnOnce(&mut Context) -> R) -> Option<R> {
    CONTEXT.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        if slot.as_ref().is_some_and(|c| c.generation != gpu.generation) {
            *slot = None;
        }
        if slot.is_none() {
            let context = unsafe { gpu.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }.ok()?;
            let brush = unsafe { context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, None) }.ok()?;
            *slot = Some(Context { context, brush, halos: Halos::default(), generation: gpu.generation });
        }
        Some(f(slot.as_mut()?))
    })
}

/// A picture on the GPU, made from pixels the caller keeps, so it can be
/// made again on a new device.
pub struct Image {
    bitmap: ID2D1Bitmap1,
    generation: u64,
    pub width: u32,
    pub height: u32,
}

impl Image {
    /// Still on the current device.
    pub fn current(&self) -> bool {
        GENERATION.load(Ordering::SeqCst) == self.generation && ready()
    }

    pub fn bitmap(&self) -> &ID2D1Bitmap1 {
        &self.bitmap
    }

    pub fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// Uploads opaque BGRA pixels, rows `width * 4` bytes; `None` before the
/// device is made. On the UI thread.
pub fn upload(bgra: &[u8], width: u32, height: u32) -> Option<Image> {
    if width == 0 || height == 0 || bgra.len() < width as usize * height as usize * 4 {
        return None;
    }
    let gpu = current()?;
    with_context(&gpu, |c| {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let bitmap = unsafe { c.context.CreateBitmap(D2D_SIZE_U { width, height }, Some(bgra.as_ptr() as *const _), width * 4, &props) }.ok()?;
        Some(Image { bitmap, generation: gpu.generation, width, height })
    })
    .flatten()
}

/// One window's swap chain, and the bitmap over its back buffer.
#[derive(Default)]
pub struct Surface {
    swap: Option<IDXGISwapChain1>,
    target: Option<ID2D1Bitmap1>,
    size: (u32, u32),
    generation: u64,
}

impl Surface {
    /// Paints `hwnd` through the GPU: a swap chain `buffer` pixels (at
    /// least the window; the grid keeps one as large as the window it
    /// grows into, `DXGI_SCALING_NONE` showing its top left), `draw` in DIPs
    /// at `scale`, then presented. False before the device is made, or when
    /// it couldn't paint: the caller paints with GDI instead.
    pub fn paint(&mut self, hwnd: HWND, buffer: (u32, u32), scale: f32, plain: bool, draw: impl FnOnce(&mut Painter)) -> bool {
        let Some(gpu) = current() else { return false };
        let (w, h) = (buffer.0.max(1), buffer.1.max(1));
        if self.generation != gpu.generation {
            self.release();
        }
        if self.swap.is_none() {
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w,
                Height: h,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: FALSE,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_NONE,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
            };
            let Ok(swap) = (unsafe { gpu.dxgi.CreateSwapChainForHwnd(&gpu.d3d, hwnd, &desc, None, None) }) else { return false };
            // Alt+Enter is the app's own (F11), not DXGI's.
            unsafe {
                let _ = gpu.dxgi.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
            }
            self.swap = Some(swap);
            self.size = (w, h);
            self.generation = gpu.generation;
        } else if self.size != (w, h) {
            self.target = None;
            let resized = unsafe { self.swap.as_ref().expect("swap chain").ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) };
            if resized.is_err() {
                self.release();
                return false;
            }
            self.size = (w, h);
        }
        let swap = self.swap.clone().expect("swap chain");
        let painted = with_context(&gpu, |c| {
            if self.target.is_none() {
                let surface: IDXGISurface = unsafe { swap.GetBuffer(0) }.ok()?;
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    colorContext: std::mem::ManuallyDrop::new(None),
                };
                self.target = unsafe { c.context.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }.ok();
            }
            let target = self.target.as_ref()?;
            unsafe {
                c.context.SetTarget(target);
                c.context.SetDpi(96.0 * scale, 96.0 * scale);
                c.context.BeginDraw();
                c.context.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                c.context.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            }
            d2d::paint_context(&c.context, &c.brush, &mut c.halos, plain, draw);
            let ended = unsafe { c.context.EndDraw(None, None) };
            unsafe { c.context.SetTarget(None::<&ID2D1Image>) };
            Some(ended)
        })
        .flatten();
        match painted {
            Some(Ok(())) => {}
            Some(Err(e)) if e.code() == D2DERR_RECREATE_TARGET => {
                self.release();
                lose(gpu.generation);
                return false;
            }
            _ => return false,
        }
        let presented = unsafe { swap.Present(1, DXGI_PRESENT(0)) };
        if presented == DXGI_ERROR_DEVICE_REMOVED || presented == DXGI_ERROR_DEVICE_RESET {
            self.release();
            lose(gpu.generation);
            return false;
        }
        true
    }

    /// Lets the swap chain go (the window hides, or the device was lost).
    pub fn release(&mut self) {
        self.target = None;
        self.swap = None;
        self.size = (0, 0);
    }

    pub fn has_swap_chain(&self) -> bool {
        self.swap.is_some()
    }
}
