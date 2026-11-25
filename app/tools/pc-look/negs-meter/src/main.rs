//! negs-meter: measures what reaches the screen, the same way for any app.
//!
//!   negs-meter <hwnd> <pid> <script.txt> <out.json>
//!
//! A capture thread reads DXGI Desktop Duplication: for every desktop image
//! update it copies the script's probe rows (1 px tall) out of the desktop
//! texture and hashes them, so a frame counts for a probe only if those
//! pixels changed. The main thread plays the script (SendInput) and stamps
//! every event with QPC, the clock DXGI's LastPresentTime uses. The app's
//! working set and private bytes are sampled every 100 ms.
//!
//! Script lines (coordinates in physical pixels from the window's top-left,
//! DWM extended frame bounds):
//!   probe NAME X Y W     a sample row (several per name allowed)
//!   front                bring the window to the front
//!   wait MS
//!   mark TEXT            an event in the output, e.g. begin:grid / end:grid
//!   move X Y | click X Y | dclick X Y
//!   key VK | keydown VK | keyup VK   (VK decimal or 0x..)
//!   wheel X Y DELTA COUNT INTERVAL_MS
//!   drag X0 Y0 X1 Y1 SWEEP_MS SWEEPS
//!   resize W H | place X Y W H

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SetForegroundWindow, SetWindowPos, ShowWindow, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOMOVE, SWP_NOZORDER, SW_RESTORE};

fn qpc() -> i64 {
    let mut t = 0;
    unsafe { QueryPerformanceCounter(&mut t).unwrap() };
    t
}

#[derive(Clone)]
struct Probe {
    group: usize,
    x: i32,
    y: i32,
    w: i32,
}

struct Frame {
    t: i64,
    accumulated: u32,
    window_dirty: bool,
    changed: u64,
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).unwrap() };
    r
}

fn capture(hwnd: HWND, probes: Vec<Probe>, stop: Arc<AtomicBool>, frames: Arc<Mutex<Vec<Frame>>>, info: Arc<Mutex<String>>) {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().unwrap();
        let win = window_rect(hwnd);
        let (cx, cy) = ((win.left + win.right) / 2, (win.top + win.bottom) / 2);
        let mut found = None;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                let d = output.GetDesc().unwrap();
                let r = d.DesktopCoordinates;
                if cx >= r.left && cx < r.right && cy >= r.top && cy < r.bottom {
                    found = Some((adapter.clone(), output, r));
                }
                o += 1;
            }
            a += 1;
        }
        let (adapter, output, origin) = found.expect("no output under the window");
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_FLAG(0), None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)).unwrap();
        let device = device.unwrap();
        let context = context.unwrap();
        let output5: IDXGIOutput5 = output.cast().unwrap();
        let dup = output5.DuplicateOutput1(&device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM]).unwrap();
        let max_w = probes.iter().map(|p| p.w).max().unwrap_or(1).max(1) as u32;
        let rows = probes.len().max(1) as u32;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: max_w,
            Height: rows,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        const RING: usize = 8;
        let mut ring = vec![];
        for _ in 0..RING {
            let mut staging = None;
            device.CreateTexture2D(&desc, None, Some(&mut staging)).unwrap();
            ring.push(staging.unwrap());
        }
        *info.lock().unwrap() = format!("\"output\": [{}, {}, {}, {}]", origin.left, origin.top, origin.right, origin.bottom);
        let mut last: Vec<u64> = vec![0; probes.len()];
        let mut dirty_buf: Vec<RECT> = vec![RECT::default(); 512];
        let mut first = true;
        // Copied, not yet read: (slot, present time, accumulated, window dirty).
        let mut pending: std::collections::VecDeque<(usize, i64, u32, bool)> = Default::default();
        let mut next_slot = 0usize;
        // Reads the oldest copies in order; `wait` blocks on the oldest.
        let mut drain = |pending: &mut std::collections::VecDeque<(usize, i64, u32, bool)>, wait: bool, last: &mut Vec<u64>, first: &mut bool| {
            while let Some(&(slot, t, acc, wd)) = pending.front() {
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                let flags = if wait { 0 } else { D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32 };
                if context.Map(&ring[slot], 0, D3D11_MAP_READ, flags, Some(&mut m)).is_err() {
                    if wait {
                        continue;
                    }
                    return;
                }
                let mut changed = 0u64;
                for (i, p) in probes.iter().enumerate() {
                    let row = std::slice::from_raw_parts((m.pData as *const u8).add(i * m.RowPitch as usize), (p.w * 4) as usize);
                    let mut h: u64 = 0xcbf29ce484222325;
                    for &byte in row {
                        h ^= byte as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    if !*first && h != last[i] {
                        changed |= 1 << p.group;
                    }
                    last[i] = h;
                }
                context.Unmap(&ring[slot], 0);
                *first = false;
                frames.lock().unwrap().push(Frame { t, accumulated: acc, window_dirty: wd, changed });
                pending.pop_front();
            }
        };
        while !stop.load(Ordering::Relaxed) {
            let mut fi = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match dup.AcquireNextFrame(20, &mut fi, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    drain(&mut pending, false, &mut last, &mut first);
                    continue;
                }
                Err(e) => panic!("AcquireNextFrame: {e}"),
            }
            if fi.LastPresentTime != 0 {
                let win = window_rect(hwnd);
                let (wx, wy) = (win.left - origin.left, win.top - origin.top);
                let mut need = 0u32;
                let mut window_dirty = false;
                if dup.GetFrameDirtyRects((dirty_buf.len() * std::mem::size_of::<RECT>()) as u32, dirty_buf.as_mut_ptr(), &mut need).is_ok() {
                    let n = need as usize / std::mem::size_of::<RECT>();
                    for r in &dirty_buf[..n] {
                        if r.right > wx && r.left < wx + (win.right - win.left) && r.bottom > wy && r.top < wy + (win.bottom - win.top) {
                            window_dirty = true;
                        }
                    }
                } else {
                    window_dirty = true;
                }
                if pending.len() == RING {
                    drain(&mut pending, true, &mut last, &mut first);
                }
                let slot = next_slot;
                next_slot = (next_slot + 1) % RING;
                let tex: ID3D11Texture2D = res.as_ref().unwrap().cast().unwrap();
                for (i, p) in probes.iter().enumerate() {
                    let b = D3D11_BOX { left: (wx + p.x) as u32, top: (wy + p.y) as u32, front: 0, right: (wx + p.x + p.w) as u32, bottom: (wy + p.y + 1) as u32, back: 1 };
                    context.CopySubresourceRegion(&ring[slot], 0, 0, i as u32, 0, &tex, 0, Some(&b));
                }
                pending.push_back((slot, fi.LastPresentTime, fi.AccumulatedFrames, window_dirty));
            }
            drop(res);
            let _ = dup.ReleaseFrame();
            context.Flush();
            drain(&mut pending, false, &mut last, &mut first);
        }
        drain(&mut pending, true, &mut last, &mut first);
    }
}

struct Input {
    hwnd: HWND,
    events: Vec<(i64, String)>,
}

impl Input {
    fn origin(&self) -> (i32, i32) {
        let r = window_rect(self.hwnd);
        (r.left, r.top)
    }
    fn abs(&self, x: i32, y: i32) -> (i32, i32) {
        let (ox, oy) = self.origin();
        unsafe {
            let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            let nx = ((ox + x - vx) as f64 * 65535.0 / (vw - 1) as f64).round() as i32;
            let ny = ((oy + y - vy) as f64 * 65535.0 / (vh - 1) as f64).round() as i32;
            (nx, ny)
        }
    }
    fn mouse(&self, flags: MOUSE_EVENT_FLAGS, x: i32, y: i32, data: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: x, dy: y, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    }
    fn move_to(&self, x: i32, y: i32) {
        let (nx, ny) = self.abs(x, y);
        self.mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0);
    }
    fn button(&mut self, down: bool) {
        self.mouse(if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP }, 0, 0, 0);
        self.events.push((qpc(), if down { "down".into() } else { "up".into() }));
    }
    fn key(&mut self, vk: u16, up: bool) {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
        };
        unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        self.events.push((qpc(), format!("{} {vk}", if up { "keyup" } else { "keydown" })));
    }
}

fn sleep_until(t: i64, freq: i64) {
    loop {
        let now = qpc();
        if now >= t {
            return;
        }
        let ms = (t - now) * 1000 / freq;
        if ms > 2 {
            std::thread::sleep(Duration::from_millis((ms - 1) as u64));
        } else {
            std::hint::spin_loop();
        }
    }
}

fn num(s: &str) -> i32 {
    if let Some(h) = s.strip_prefix("0x") {
        i32::from_str_radix(h, 16).unwrap()
    } else {
        s.parse().unwrap_or_else(|_| panic!("number: {s}"))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hwnd = HWND(num(&args[1]) as isize as *mut _);
    let pid: u32 = args[2].parse().unwrap();
    let script = std::fs::read_to_string(&args[3]).unwrap();
    let out = &args[4];
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        windows::Win32::Media::timeBeginPeriod(1);
    }
    let mut freq = 0;
    unsafe { QueryPerformanceFrequency(&mut freq).unwrap() };

    let mut groups: Vec<String> = vec![];
    let mut probes = vec![];
    let mut steps: Vec<Vec<String>> = vec![];
    for line in script.lines() {
        let w: Vec<String> = line.split_whitespace().map(String::from).collect();
        if w.is_empty() || w[0].starts_with('#') {
            continue;
        }
        if w[0] == "probe" {
            let g = groups.iter().position(|n| *n == w[1]).unwrap_or_else(|| {
                groups.push(w[1].clone());
                groups.len() - 1
            });
            probes.push(Probe { group: g, x: num(&w[2]), y: num(&w[3]), w: num(&w[4]) });
        } else if w[0] == "resize" || w[0] == "place" {
            // Before capture starts, so the probes land on the window.
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                if w[0] == "resize" {
                    let _ = SetWindowPos(hwnd, None, 0, 0, num(&w[1]), num(&w[2]), SWP_NOMOVE | SWP_NOZORDER);
                } else {
                    let _ = SetWindowPos(hwnd, None, num(&w[1]), num(&w[2]), num(&w[3]), num(&w[4]), SWP_NOZORDER);
                }
            }
            std::thread::sleep(Duration::from_millis(800));
        } else {
            steps.push(w);
        }
    }

    // Memory sampler.
    let stop = Arc::new(AtomicBool::new(false));
    let mem = Arc::new(Mutex::new((0usize, 0usize, 0usize)));
    let mem_thread = {
        let (stop, mem) = (stop.clone(), mem.clone());
        std::thread::spawn(move || unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid) else { return };
            while !stop.load(Ordering::Relaxed) {
                let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
                c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                if GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb).is_ok() {
                    let mut m = mem.lock().unwrap();
                    m.0 = m.0.max(c.WorkingSetSize);
                    m.1 = m.1.max(c.PrivateUsage);
                    m.2 = c.PeakWorkingSetSize;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = CloseHandle(h);
        })
    };

    let frames = Arc::new(Mutex::new(Vec::with_capacity(20000)));
    let info = Arc::new(Mutex::new(String::new()));
    let cap = {
        let (stop, frames, info, probes) = (stop.clone(), frames.clone(), info.clone(), probes.clone());
        let h = hwnd.0 as isize;
        std::thread::spawn(move || capture(HWND(h as *mut _), probes, stop, frames, info))
    };
    std::thread::sleep(Duration::from_millis(400));

    let mut input = Input { hwnd, events: vec![(qpc(), "start".into())] };
    for w in &steps {
        let n = |i: usize| num(&w[i]);
        match w[0].as_str() {
            "front" => unsafe {
                // Alt tap lets a background process take the foreground.
                input.key(0x12, false);
                input.key(0x12, true);
                let _ = SetForegroundWindow(hwnd);
                std::thread::sleep(Duration::from_millis(300));
            },
            "wait" => std::thread::sleep(Duration::from_millis(n(1) as u64)),
            "mark" => input.events.push((qpc(), w[1..].join(" "))),
            "move" => input.move_to(n(1), n(2)),
            "click" | "dclick" => {
                input.move_to(n(1), n(2));
                std::thread::sleep(Duration::from_millis(30));
                let times = if w[0] == "dclick" { 2 } else { 1 };
                for _ in 0..times {
                    input.button(true);
                    input.button(false);
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
            "key" => {
                input.key(n(1) as u16, false);
                std::thread::sleep(Duration::from_millis(30));
                input.key(n(1) as u16, true);
            }
            "keydown" => input.key(n(1) as u16, false),
            "keyup" => input.key(n(1) as u16, true),
            "wheel" => {
                input.move_to(n(1), n(2));
                std::thread::sleep(Duration::from_millis(50));
                let (delta, count, every) = (n(3), n(4), n(5) as i64);
                let t0 = qpc();
                for i in 0..count as i64 {
                    sleep_until(t0 + i * every * freq / 1000, freq);
                    input.mouse(MOUSEEVENTF_WHEEL, 0, 0, delta);
                    input.events.push((qpc(), "wheel".into()));
                }
            }
            "drag" => {
                let (x0, y0, x1, y1, sweep, sweeps) = (n(1), n(2), n(3), n(4), n(5) as i64, n(6) as i64);
                input.move_to(x0, y0);
                std::thread::sleep(Duration::from_millis(60));
                input.button(true);
                let t0 = qpc();
                let step = freq / 240;
                let end = t0 + sweep * sweeps * freq / 1000;
                let mut t = t0;
                while t < end {
                    let phase = ((t - t0) as f64 / (sweep as f64 * freq as f64 / 1000.0)) % 2.0;
                    let f = if phase < 1.0 { phase } else { 2.0 - phase };
                    let x = x0 + ((x1 - x0) as f64 * f).round() as i32;
                    let y = y0 + ((y1 - y0) as f64 * f).round() as i32;
                    input.move_to(x, y);
                    t += step;
                    sleep_until(t, freq);
                }
                input.button(false);
            }
            other => panic!("unknown step {other}"),
        }
    }
    input.events.push((qpc(), "stop".into()));
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::Relaxed);
    cap.join().expect("capture thread");
    mem_thread.join().unwrap();

    let win = window_rect(hwnd);
    let frames = frames.lock().unwrap();
    let m = *mem.lock().unwrap();
    let mut f = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    writeln!(f, "{{\"freq\": {freq}, \"window\": [{}, {}, {}, {}], {},", win.left, win.top, win.right, win.bottom, info.lock().unwrap()).unwrap();
    writeln!(f, "\"mem\": {{\"max_working_set\": {}, \"max_private\": {}, \"peak_working_set\": {}}},", m.0, m.1, m.2).unwrap();
    writeln!(f, "\"groups\": {:?},", groups).unwrap();
    writeln!(f, "\"events\": [").unwrap();
    for (i, (t, e)) in input.events.iter().enumerate() {
        writeln!(f, "[{t}, {:?}]{}", e, if i + 1 < input.events.len() { "," } else { "" }).unwrap();
    }
    writeln!(f, "], \"frames\": [").unwrap();
    for (i, fr) in frames.iter().enumerate() {
        writeln!(f, "[{}, {}, {}, {}]{}", fr.t, fr.accumulated, fr.window_dirty as u8, fr.changed, if i + 1 < frames.len() { "," } else { "" }).unwrap();
    }
    writeln!(f, "]}}").unwrap();
}
