//! `NEGS_GRID_BENCH=d2d|warp|wgpu`: instead of the app, the grid's scroll
//! case drawn by one painting path, to choose between them (phase 7): 300
//! thumbnails packed as the grid packs them, flung down and back up for
//! six seconds in a 1600 × 1000 window, a sentence over them, the
//! marks on some and the scroll bar, then the frame times written to
//! `grid-bench-<mode>.json` in `NEGS_PROFILE_DIR` (else the working
//! folder) and the window closed.
//!
//! - `d2d`: Direct2D on the GPU (`surface.rs`), the path the strip and grid use.
//! - `warp`: the same on WARP, as on a PC without a GPU driver.
//! - `wgpu`: the canvas's Vulkan device, thumbnails as textures and the
//!   words and stars as DirectWrite masks, as Noah's plan had it.
//!
//! Without the variable it costs one environment lookup.

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{BoxInk, Ink, RectF, TextStyle};
use crate::sheet_rules::{self as rules, Rect, SheetLayout};

const COUNT: usize = 300;
const SECONDS: f64 = 6.0;
const FLING: f64 = 0.75;
const SENTENCE: TextStyle = TextStyle::serif(17.0, 400).italic();

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    D2d,
    Warp,
    Wgpu,
}

/// Runs the bench if `NEGS_GRID_BENCH` names a mode; true if it ran.
pub fn run_if_asked() -> bool {
    let Some(mode) = std::env::var_os("NEGS_GRID_BENCH") else { return false };
    let mode = match mode.to_string_lossy().as_ref() {
        "wgpu" => Mode::Wgpu,
        "warp" => Mode::Warp,
        _ => Mode::D2d,
    };
    let name = match mode {
        Mode::D2d => "d2d",
        Mode::Warp => "warp",
        Mode::Wgpu => "wgpu",
    };
    let out = std::env::var_os("NEGS_PROFILE_DIR").map(std::path::PathBuf::from).unwrap_or_default().join(format!("grid-bench-{name}.json"));
    let result = run(mode).unwrap_or_else(|e| serde_json::json!({"mode": name, "error": format!("{e:#}")}));
    let _ = std::fs::write(out, serde_json::to_string_pretty(&result).unwrap_or_default());
    true
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Where the grid is scrolled `t` seconds in: four flings down, four back
/// up, each from 8000 DIP/s easing off as a touchpad's inertia does.
fn scrolled(t: f64, max: f64) -> f64 {
    let tau = 0.35;
    let distance = |dt: f64| 8000.0 * tau * (1.0 - (-dt / tau).exp());
    let mut y: f64 = 0.0;
    let mut start = 0.0;
    for fling in 0..8 {
        let dt = (t - start).clamp(0.0, FLING);
        let d = distance(dt);
        y = if fling < 4 { (y + d).min(max) } else { (y - d).max(0.0) };
        if t < start + FLING {
            break;
        }
        // Each fling lands where it ends.
        let full = distance(FLING);
        y = if fling < 4 { (y - d + full).min(max) } else { (y + d - full).max(0.0) };
        start += FLING;
    }
    y
}

/// A thumbnail's pixels: a hue per photograph, a gradient and grain.
fn picture(index: usize, portrait: bool, bgra: bool) -> (Vec<u8>, u32, u32) {
    let (w, h) = if portrait { (256u32, 384u32) } else { (384u32, 256u32) };
    let mut seed = index as u32 * 2654435761 + 1;
    let hue = (index * 37 % 360) as f32;
    let (r0, g0, b0) = (
        (0.5 + 0.4 * (hue.to_radians()).cos()) * 255.0,
        (0.5 + 0.4 * ((hue + 120.0).to_radians()).cos()) * 255.0,
        (0.5 + 0.4 * ((hue + 240.0).to_radians()).cos()) * 255.0,
    );
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let shade = 0.55 + 0.45 * (x as f32 / w as f32) * (1.0 - y as f32 / h as f32 * 0.5) + ((seed & 31) as f32 - 16.0) / 255.0;
            let c = |v: f32| (v * shade).clamp(0.0, 255.0) as u8;
            let (r, g, b) = (c(r0), c(g0), c(b0));
            if bgra { out.extend_from_slice(&[b, g, r, 255]) } else { out.extend_from_slice(&[r, g, b, 255]) }
        }
    }
    (out, w, h)
}

fn summary(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values.get(((values.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
    serde_json::json!({"median": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": at(1.0)})
}

fn run(mode: Mode) -> anyhow::Result<serde_json::Value> {
    let hwnd = unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: super::instance(),
            lpszClassName: w!("NegsGridBench"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("NegsGridBench"), w!("Negs grid bench"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 40, 40, 1600, 1000, None, None, super::instance(), None)?;
        let _ = SetForegroundWindow(hwnd);
        hwnd
    };
    let scale = unsafe { GetDpiForWindow(hwnd) } as f64 / 96.0;
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let (pw, ph) = (client.right.max(1) as u32, client.bottom.max(1) as u32);
    let (width, height) = (pw as f64 / scale, ph as f64 / scale);
    let ratios: Vec<f64> = (0..COUNT).map(|i| if i % 5 == 3 { 2.0 / 3.0 } else { 1.5 }).collect();
    let layout = SheetLayout::new(&ratios, width, rules::GRID_DEFAULT);
    let viewport = height - rules::SENTENCE;
    let max = (layout.height - viewport).max(0.0);
    let t = super::theme::tokens();
    let name = |i: usize| format!("_NHA{:04}.NEF", 9000 + i);
    let names: Vec<String> = (0..COUNT).map(name).collect();
    let mut work = vec![];
    let mut intervals = vec![];
    let mut drawn_cells = 0usize;
    let started = Instant::now();
    let mut last = None::<Instant>;
    let pump = || unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    };
    let adapter;
    match mode {
        Mode::D2d | Mode::Warp => {
            super::d2d::shared().ok_or_else(|| anyhow::anyhow!("No Direct2D"))?;
            if !super::surface::make_now(mode == Mode::Warp) {
                anyhow::bail!("No Direct3D device");
            }
            adapter = if super::surface::hardware() == Some(true) { "hardware" } else { "warp" }.to_string();
            let images: Vec<super::surface::Image> = (0..COUNT)
                .map(|i| {
                    let (px, w, h) = picture(i, ratios[i] < 1.0, true);
                    super::surface::upload(&px, w, h).expect("upload")
                })
                .collect();
            let mut surface = super::surface::Surface::default();
            while started.elapsed().as_secs_f64() < SECONDS {
                pump();
                let frame = Instant::now();
                let scroll = scrolled(started.elapsed().as_secs_f64(), max);
                let range = layout.indices(scroll - rules::GRID_MARGIN, scroll + viewport + rules::GRID_MARGIN);
                drawn_cells += range.len();
                let presented = surface.paint(hwnd, (pw, ph), scale as f32, false, |p| {
                    p.fill_rect(RectF::new(0.0, 0.0, width as f32, height as f32), Ink::hex(t.hex.grid));
                    let top = rules::SENTENCE - scroll;
                    p.clipped(RectF::new(0.0, rules::SENTENCE as f32, width as f32, viewport as f32), |p| {
                        for i in range.clone() {
                            let f = layout.frames[i];
                            let r = RectF::new(f.x as f32, (f.y + top) as f32, f.w as f32, f.h as f32);
                            let image = &images[i];
                            p.picture(image.bitmap(), (image.width, image.height), r, 1.0);
                            if i % 5 == 0 {
                                for s in 0..3 {
                                    p.star(&names[i], s, r.x + 16.0 + s as f32 * 14.0, r.y + r.h - 16.0, Ink::hex(t.hex.paper), true);
                                }
                            }
                        }
                        for i in range.clone().filter(|i| i % 7 == 0) {
                            let f = layout.frames[i];
                            let r = RectF::new(f.x as f32, (f.y + top) as f32, f.w as f32, f.h as f32);
                            p.pick_box(BoxInk::Grid, &names[i], r, Ink::hex(t.hex.paper), true);
                        }
                    });
                    super::grid::paint_scroll_bar(p, &t, scroll, layout.height, viewport, width as f32, rules::SENTENCE as f32, 1.0, false);
                    p.text("Showing all 300 photographs", &SENTENCE, 22.0, 12.0, Ink::hex(t.hex.dim));
                });
                if !presented {
                    anyhow::bail!("Painting failed");
                }
                let done = Instant::now();
                work.push((done - frame).as_secs_f64() * 1000.0);
                if let Some(last) = last {
                    intervals.push((done - last).as_secs_f64() * 1000.0);
                }
                last = Some(done);
            }
        }
        Mode::Wgpu => {
            use crate::gpu::{self, Item, QuadInstance};
            let (gpu, surface) = gpu::init(hwnd.0 as isize)?;
            adapter = format!("{} ({:?})", gpu.info.name, gpu.info.backend);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: gpu.format,
                width: pw,
                height: ph,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 1,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            };
            surface.configure(&gpu.device, &config);
            let images: Vec<gpu::GpuImage> = (0..COUNT)
                .map(|i| {
                    let (px, w, h) = picture(i, ratios[i] < 1.0, false);
                    gpu.upload(&px, w, h)
                })
                .collect();
            let s = scale as f32;
            let sentence = super::d2d::mask("Showing all 300 photographs", &SENTENCE, 0.0, super::d2d::Align::Leading, s, false).ok_or_else(|| anyhow::anyhow!("No mask"))?;
            let sentence_size = (sentence.width as f32, sentence.height as f32);
            let sentence = gpu.upload_mask(&sentence.coverage, sentence.width, sentence.height);
            let star = super::d2d::shape_mask(14.0, 14.0, s, false, |p| p.star("_NHA9000.NEF", 0, 7.0, 7.0, Ink::hex(0xffffff), false)).ok_or_else(|| anyhow::anyhow!("No mask"))?;
            let star = gpu.upload_mask(&star.coverage, star.width, star.height);
            let lut = gpu.lut_texture(&[[0.0; 3], [1.0; 3]]);
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bench globals"),
                size: std::mem::size_of::<gpu::Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let globals = gpu.globals_bind_group(&buffer, &lut);
            gpu.queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&gpu::Globals { viewport: [pw as f32, ph as f32, 0.0, 0.0], ..Default::default() }));
            let c = |hex: u32, a: f32| [((hex >> 16) & 0xff) as f32 / 255.0, ((hex >> 8) & 0xff) as f32 / 255.0, (hex & 0xff) as f32 / 255.0, a];
            let clear = c(t.hex.grid, 1.0);
            while started.elapsed().as_secs_f64() < SECONDS {
                pump();
                let frame = Instant::now();
                let scroll = scrolled(started.elapsed().as_secs_f64(), max);
                let range = layout.indices(scroll - rules::GRID_MARGIN, scroll + viewport + rules::GRID_MARGIN);
                drawn_cells += range.len();
                let texture = surface.get_current_texture()?;
                let view = texture.texture.create_view(&Default::default());
                let top = rules::SENTENCE - scroll;
                let px = |r: Rect| [(r.x * scale) as f32, (r.y * scale) as f32, (r.w * scale) as f32, (r.h * scale) as f32];
                let mut items = vec![Item::Clip(Some([0, (rules::SENTENCE * scale) as u32, pw, ph]))];
                for i in range.clone() {
                    let f = layout.frames[i];
                    items.push(Item::Quad(Some(&images[i]), QuadInstance::image(px(Rect::new(f.x, f.y + top, f.w, f.h)), [0.0, 0.0, 1.0, 1.0], false)));
                }
                for i in range.clone().filter(|i| i % 5 == 0) {
                    let f = layout.frames[i];
                    for k in 0..3 {
                        let r = Rect::new(f.x + 9.0 + k as f64 * 14.0, f.y + top + f.h - 23.0, 14.0, 14.0);
                        items.push(Item::Quad(Some(&star), QuadInstance::text(px(r), c(t.hex.paper, 1.0))));
                    }
                }
                for i in range.clone().filter(|i| i % 7 == 0) {
                    let f = layout.frames[i];
                    items.push(Item::Quad(None, QuadInstance::rounded(px(Rect::new(f.x - 3.0, f.y + top - 3.0, f.w + 6.0, f.h + 6.0)), c(t.hex.paper, 1.0), 4.0 * s, 2.0 * s)));
                }
                items.push(Item::Clip(None));
                items.push(Item::Quad(Some(&sentence), QuadInstance::text([22.0 * s, 12.0 * s, sentence_size.0, sentence_size.1], c(t.hex.dim, 1.0))));
                gpu::draw(&gpu, &view, (pw, ph), &globals, [clear[0], clear[1], clear[2]], &items);
                let submitted = Instant::now();
                texture.present();
                let done = Instant::now();
                work.push((submitted - frame).as_secs_f64() * 1000.0);
                if let Some(last) = last {
                    intervals.push((done - last).as_secs_f64() * 1000.0);
                }
                last = Some(done);
            }
        }
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    let frames = work.len();
    let slow = intervals.iter().filter(|&&i| i > 20.0).count();
    let very_slow = intervals.iter().filter(|&&i| i > 34.0).count();
    Ok(serde_json::json!({
        "mode": match mode { Mode::D2d => "d2d", Mode::Warp => "warp", Mode::Wgpu => "wgpu" },
        "adapter": adapter,
        "window_px": [pw, ph],
        "scale": scale,
        "thumbnails": COUNT,
        "frames": frames,
        "cells_per_frame": drawn_cells as f64 / frames.max(1) as f64,
        "cpu_ms": summary(&mut work),
        "interval_ms": summary(&mut intervals),
        "intervals_over_20ms": slow,
        "intervals_over_34ms": very_slow,
    }))
}
