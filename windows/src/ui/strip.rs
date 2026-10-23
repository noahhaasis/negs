//! The film strip under the canvas (`PhotoBrowser.swift`): every supported
//! photograph in the current photograph's folder, in Explorer's order, with
//! thumbnails decoded lazily for the visible cells, one at a time, from a
//! bounded cache. Click one to open it; Left and Right open the neighbours.
//! No folder watcher, and never a full development.

use std::collections::{HashMap, VecDeque};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::{App, post, with_app};
use crate::model::is_photo;
use crate::platform::{pcwstr, wide};

const CELL: f64 = 96.0;
const THUMB_HEIGHT: f64 = 68.0;
const GAP: f64 = 8.0;
const PAD: f64 = 12.0;
const CACHE: usize = 160;

#[derive(Default, PartialEq)]
enum Status {
    #[default]
    Loading,
    Failed,
    Ready,
}

struct Thumb {
    bitmap: HBITMAP,
    width: i32,
    height: i32,
}

#[derive(Default)]
pub struct Strip {
    folder: Option<PathBuf>,
    files: Vec<PathBuf>,
    status: Status,
    scroll: i32,
    thumbs: HashMap<PathBuf, Option<Thumb>>,
    order: VecDeque<PathBuf>,
    selected: Option<PathBuf>,
    requests: Option<Arc<(Mutex<Vec<PathBuf>>, Condvar)>>,
    generation: u64,
}

pub fn register() {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsStrip"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

/// Explorer's order: `IMG_2` before `IMG_10`.
fn natural_order(a: &Path, b: &Path) -> std::cmp::Ordering {
    let (x, y) = (wide(a.file_name().unwrap_or_default()), wide(b.file_name().unwrap_or_default()));
    unsafe { StrCmpLogicalW(pcwstr(&x), pcwstr(&y)).cmp(&0) }
}

fn scan(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)?
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file() && m.file_attributes() & (HIDDEN | SYSTEM) == 0))
        .map(|e| folder.join(e.file_name()))
        .filter(|p| is_photo(p))
        .collect();
    files.sort_by(|a, b| natural_order(a, b));
    Ok(files)
}

impl Strip {
    fn px(&self, dpi: u32, dip: f64) -> i32 {
        (dip * dpi as f64 / 96.0).round() as i32
    }

    fn content_width(&self, dpi: u32) -> i32 {
        let n = self.files.len() as f64;
        self.px(dpi, PAD * 2.0 + n * CELL + (n - 1.0).max(0.0) * GAP)
    }

    fn cell_x(&self, dpi: u32, index: usize) -> i32 {
        self.px(dpi, PAD + index as f64 * (CELL + GAP)) - self.scroll
    }

    fn index_at(&self, dpi: u32, x: i32) -> Option<usize> {
        let content = x + self.scroll - self.px(dpi, PAD);
        if content < 0 {
            return None;
        }
        let pitch = self.px(dpi, CELL + GAP);
        let index = (content / pitch) as usize;
        (index < self.files.len() && content % pitch < self.px(dpi, CELL)).then_some(index)
    }
}

/// Follows the current photograph: rescans when its folder changes, and
/// keeps it selected and in the middle.
pub fn sync(app: &mut App) {
    let visible = app.model.prefs.filmstrip_visible && app.model.source.is_some();
    unsafe {
        let _ = EnableWindow(app.strip, !app.model.cropping());
    }
    if !visible {
        return;
    }
    let source = app.model.source.clone();
    let folder = source.as_ref().and_then(|s| s.parent()).map(Path::to_path_buf);
    let strip = &mut app.strip_view;
    if folder != strip.folder {
        strip.folder = folder.clone();
        strip.files.clear();
        strip.status = Status::Loading;
        strip.generation += 1;
        let generation = strip.generation;
        if let Some(folder) = folder {
            std::thread::spawn(move || {
                let result = scan(&folder);
                post(move |app: &mut App| {
                    let strip = &mut app.strip_view;
                    if strip.generation != generation {
                        return;
                    }
                    match result {
                        Ok(files) => {
                            strip.files = files;
                            strip.status = Status::Ready;
                        }
                        Err(_) => strip.status = Status::Failed,
                    }
                    strip.selected = None;
                    sync(app);
                });
            });
        }
    }
    if app.strip_view.selected != source {
        app.strip_view.selected = source.clone();
        center_selection(app);
    }
    request_visible(app);
    unsafe {
        let _ = InvalidateRect(app.strip, None, false);
    }
}

fn center_selection(app: &mut App) {
    let dpi = app.dpi;
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(app.strip, &mut r);
    }
    let strip = &mut app.strip_view;
    if let Some(index) = strip.selected.as_ref().and_then(|s| strip.files.iter().position(|f| crate::session::same_path(f, s))) {
        let middle = strip.px(dpi, PAD + index as f64 * (CELL + GAP) + CELL / 2.0);
        strip.scroll = middle - r.right / 2;
    }
    clamp_scroll(app);
}

fn clamp_scroll(app: &mut App) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(app.strip, &mut r);
    }
    let dpi = app.dpi;
    let strip = &mut app.strip_view;
    let content = strip.content_width(dpi);
    strip.scroll = strip.scroll.clamp(0, (content - r.right).max(0));
    let info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: content,
        nPage: r.right.max(0) as u32,
        nPos: strip.scroll,
        ..Default::default()
    };
    unsafe {
        SetScrollInfo(app.strip, SB_HORZ, &info, true);
    }
}

/// Asks the worker for the visible cells' thumbnails, newest request first.
fn request_visible(app: &mut App) {
    let dpi = app.dpi;
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(app.strip, &mut r);
    }
    let strip = &mut app.strip_view;
    let wanted: Vec<PathBuf> = (0..strip.files.len())
        .filter(|&i| {
            let x = strip.cell_x(dpi, i);
            x + strip.px(dpi, CELL) >= 0 && x <= r.right
        })
        .map(|i| strip.files[i].clone())
        .filter(|p| !strip.thumbs.contains_key(p))
        .collect();
    let requests = strip.requests.get_or_insert_with(|| {
        let requests: Arc<(Mutex<Vec<PathBuf>>, Condvar)> = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let worker = requests.clone();
        let pixels = (192.0 * dpi as f64 / 96.0) as i32;
        std::thread::Builder::new()
            .name("thumbnails".into())
            .spawn(move || {
                crate::thumbnail::com_thread();
                loop {
                    let path = {
                        let (queue, ready) = &*worker;
                        let mut queue = queue.lock().unwrap();
                        while queue.is_empty() {
                            queue = ready.wait(queue).unwrap();
                        }
                        queue.remove(0)
                    };
                    let thumb = crate::thumbnail::strip_thumbnail(&path, pixels);
                    post(move |app: &mut App| store(app, path, thumb));
                }
            })
            .ok();
        requests
    });
    let (queue, ready) = &**requests;
    let mut queue = queue.lock().unwrap();
    // Cells that scrolled away are no longer wanted.
    queue.clear();
    queue.extend(wanted);
    ready.notify_one();
}

fn store(app: &mut App, path: PathBuf, thumb: Option<(Vec<u8>, u32, u32)>) {
    let strip = &mut app.strip_view;
    let bitmap = thumb.and_then(|(rgba, w, h)| unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let target = std::slice::from_raw_parts_mut(bits as *mut u8, rgba.len());
        for (out, p) in target.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            out.copy_from_slice(&[p[2], p[1], p[0], 255]);
        }
        Some(Thumb { bitmap, width: w as i32, height: h as i32 })
    });
    strip.thumbs.insert(path.clone(), bitmap);
    strip.order.push_back(path);
    while strip.order.len() > CACHE {
        if let Some(old) = strip.order.pop_front() {
            if let Some(Some(thumb)) = strip.thumbs.remove(&old) {
                unsafe {
                    let _ = DeleteObject(thumb.bitmap);
                }
            }
        }
    }
    unsafe {
        let _ = InvalidateRect(app.strip, None, false);
    }
}

fn middle_truncated(dc: HDC, name: &str, width: i32) -> String {
    let fits = |s: &str| {
        let w = wide(s);
        let mut size = SIZE::default();
        unsafe {
            let _ = GetTextExtentPoint32W(dc, &w[..w.len() - 1], &mut size);
        }
        size.cx <= width
    };
    if fits(name) {
        return name.into();
    }
    let chars: Vec<char> = name.chars().collect();
    for keep in (1..chars.len()).rev() {
        let head = keep.div_ceil(2);
        let tail = keep - head;
        let text = format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>());
        if fits(&text) {
            return text;
        }
    }
    "…".into()
}

fn paint(app: &App, hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let screen = BeginPaint(hwnd, &mut ps);
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        let dc = CreateCompatibleDC(screen);
        let buffer = CreateCompatibleBitmap(screen, r.right.max(1), r.bottom.max(1));
        let old_buffer = SelectObject(dc, buffer);
        FillRect(dc, &r, GetSysColorBrush(COLOR_WINDOW));
        let strip = &app.strip_view;
        let dpi = app.dpi;
        let px = |dip: f64| strip.px(dpi, dip);
        let old_font = SelectObject(dc, app.fonts.small);
        SetBkMode(dc, TRANSPARENT);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let message = match strip.status {
            Status::Loading => Some("Loading…"),
            Status::Failed => Some("This folder couldn’t be read."),
            Status::Ready if strip.files.is_empty() => Some("No supported photographs in this folder."),
            Status::Ready => None,
        };
        if let Some(message) = message {
            let mut text = wide(message);
            let len = text.len() - 1;
            SetTextColor(dc, COLORREF(0x707070));
            let mut tr = r;
            DrawTextW(dc, &mut text[..len], &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let accent = super::canvas::accent_color();
        let accent = COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16);
        let mem = CreateCompatibleDC(dc);
        SetStretchBltMode(dc, HALFTONE);
        for (i, path) in strip.files.iter().enumerate() {
            let x = strip.cell_x(dpi, i);
            if x + px(CELL) < 0 || x > r.right {
                continue;
            }
            let top = px(8.0);
            let cell = RECT { left: x, top, right: x + px(CELL), bottom: top + px(THUMB_HEIGHT) };
            let well = CreateSolidBrush(COLORREF(0xf3f3f3));
            FillRect(dc, &cell, well);
            let _ = DeleteObject(well);
            match strip.thumbs.get(path) {
                Some(Some(thumb)) => {
                    let inner = (cell.right - cell.left - px(6.0), cell.bottom - cell.top - px(6.0));
                    let scale = (inner.0 as f64 / thumb.width as f64).min(inner.1 as f64 / thumb.height as f64);
                    let (w, h) = ((thumb.width as f64 * scale) as i32, (thumb.height as f64 * scale) as i32);
                    let old = SelectObject(mem, thumb.bitmap);
                    let _ = StretchBlt(dc, cell.left + (cell.right - cell.left - w) / 2, cell.top + (cell.bottom - cell.top - h) / 2, w, h, mem, 0, 0, thumb.width, thumb.height, SRCCOPY);
                    SelectObject(mem, old);
                }
                _ => {
                    let mut glyph = wide("\u{E8B9}");
                    let len = glyph.len() - 1;
                    let old = SelectObject(dc, app.fonts.glyph);
                    SetTextColor(dc, COLORREF(0xb0b0b0));
                    let mut gr = cell;
                    DrawTextW(dc, &mut glyph[..len], &mut gr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    SelectObject(dc, old);
                }
            }
            let selected = strip.selected.as_ref().is_some_and(|s| crate::session::same_path(s, path));
            if selected {
                let pen = CreatePen(PS_SOLID, px(2.0), accent);
                let old = SelectObject(dc, pen);
                let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                let _ = RoundRect(dc, cell.left, cell.top, cell.right, cell.bottom, px(8.0), px(8.0));
                SelectObject(dc, old_brush);
                SelectObject(dc, old);
                let _ = DeleteObject(pen);
            }
            let name = crate::model::file_name(path);
            let shown = middle_truncated(dc, &name, px(CELL));
            let mut text = wide(&shown);
            let len = text.len() - 1;
            SetTextColor(dc, COLORREF(if !enabled { 0xa0a0a0 } else if selected { 0x202020 } else { 0x6a6a6a }));
            let mut tr = RECT { left: x - px(4.0), top: cell.bottom + px(4.0), right: x + px(CELL + 4.0), bottom: cell.bottom + px(22.0) };
            DrawTextW(dc, &mut text[..len], &mut tr, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX);
        }
        if GetFocus() == hwnd {
            let _ = DrawFocusRect(dc, &RECT { left: 2, top: 2, right: r.right - 2, bottom: r.bottom - 2 });
        }
        let _ = DeleteDC(mem);
        SelectObject(dc, old_font);
        let _ = BitBlt(screen, 0, 0, r.right, r.bottom, dc, 0, 0, SRCCOPY);
        SelectObject(dc, old_buffer);
        let _ = DeleteObject(buffer);
        let _ = DeleteDC(dc);
        let _ = EndPaint(hwnd, &ps);
    }
}

pub fn resized(app: &mut App) {
    clamp_scroll(app);
    request_visible(app);
}

fn scroll_by(app: &mut App, delta: i32) {
    app.strip_view.scroll += delta;
    clamp_scroll(app);
    request_visible(app);
    unsafe {
        let _ = InvalidateRect(app.strip, None, false);
    }
}

fn open_neighbour(app: &mut App, step: isize) {
    if app.model.cropping() {
        return;
    }
    let strip = &app.strip_view;
    let Some(index) = strip.selected.as_ref().and_then(|s| strip.files.iter().position(|f| crate::session::same_path(f, s))) else { return };
    let next = index as isize + step;
    if next >= 0 && (next as usize) < strip.files.len() {
        let path = strip.files[next as usize].clone();
        app.model.open(&path);
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                if with_app(|app| paint(app, hwnd)).is_none() {
                    let mut ps = PAINTSTRUCT::default();
                    BeginPaint(hwnd, &mut ps);
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                with_app(|app| {
                    clamp_scroll(app);
                    request_visible(app);
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                let x = (lparam.0 & 0xffff) as i16 as i32;
                with_app(|app| {
                    if app.model.cropping() {
                        return;
                    }
                    if let Some(index) = app.strip_view.index_at(app.dpi, x) {
                        let path = app.strip_view.files[index].clone();
                        if !app.model.source.as_ref().is_some_and(|s| crate::session::same_path(s, &path)) {
                            app.model.open(&path);
                        }
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                with_app(|app| {
                    let step = app.px(80.0) * delta / 120;
                    scroll_by(app, if msg == WM_MOUSEHWHEEL { step } else { -step });
                });
                LRESULT(0)
            }
            WM_HSCROLL => {
                with_app(|app| {
                    let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_ALL, ..Default::default() };
                    let _ = GetScrollInfo(hwnd, SB_HORZ, &mut info);
                    let target = match SCROLLBAR_COMMAND((wparam.0 & 0xffff) as i32) {
                        SB_LINELEFT => info.nPos - app.px(40.0),
                        SB_LINERIGHT => info.nPos + app.px(40.0),
                        SB_PAGELEFT => info.nPos - info.nPage as i32,
                        SB_PAGERIGHT => info.nPos + info.nPage as i32,
                        SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
                        _ => info.nPos,
                    };
                    scroll_by(app, target - app.strip_view.scroll);
                });
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_KEYDOWN => {
                match VIRTUAL_KEY(wparam.0 as u16) {
                    VK_LEFT => {
                        with_app(|app| open_neighbour(app, -1));
                    }
                    VK_RIGHT => {
                        with_app(|app| open_neighbour(app, 1));
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
