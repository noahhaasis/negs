//! Painting for the owner-drawn and custom-drawn controls: a back buffer
//! with anti-aliased rounded shapes (GDI's own aren't), and the custom draw
//! of buttons, checkboxes, radio buttons, segmented switches and trackbars.
//! The controls stay the real Win32 ones, with their focus, keys and
//! accessible names; only their painting is ours.

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::theme::{self, Color};

/// A back buffer for one control or part: GDI text and our shapes, then
/// copied to the window in one go. Paints share one buffer per thread
/// (growing as needed), so painting a control allocates nothing.
pub struct Canvas {
    pub dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    /// Pixels per row of the buffer, which may be wider than `w`.
    stride: i32,
    pub w: i32,
    pub h: i32,
    x: i32,
    y: i32,
    gdi: bool,
    /// The thread's shared buffer, or one of its own (large or nested).
    shared: bool,
}

struct Buffer {
    dc: isize,
    bitmap: isize,
    old: isize,
    bits: usize,
    w: i32,
    h: i32,
}

thread_local! {
    static SHARED: std::cell::RefCell<Option<Buffer>> = const { std::cell::RefCell::new(None) };
    static SHARED_IN_USE: Cell<bool> = const { Cell::new(false) };
    /// Coverage for `Canvas::stroke` and `fill_under`, all zero between uses.
    static MASK: std::cell::RefCell<Vec<f32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Buffers above this many pixels aren't kept.
const SHARED_LIMIT: i32 = 1 << 20;

fn new_buffer(target: HDC, w: i32, h: i32) -> Buffer {
    unsafe {
        let dc = CreateCompatibleDC(target);
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
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        let old = SelectObject(dc, bitmap);
        SetBkMode(dc, TRANSPARENT);
        Buffer { dc: dc.0 as isize, bitmap: bitmap.0 as isize, old: old.0 as isize, bits: bits as usize, w, h }
    }
}

fn free_buffer(b: &Buffer) {
    unsafe {
        let dc = HDC(b.dc as *mut _);
        SelectObject(dc, HGDIOBJ(b.old as *mut _));
        let _ = DeleteObject(HBITMAP(b.bitmap as *mut _));
        let _ = DeleteDC(dc);
    }
}

impl Canvas {
    /// A buffer for `rect` of `target`; drawing uses coordinates from the
    /// rectangle's corner.
    pub fn new(target: HDC, rect: RECT) -> Self {
        let (w, h) = ((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
        let canvas = |b: &Buffer, shared: bool| Canvas {
            dc: HDC(b.dc as *mut _),
            bitmap: HBITMAP(b.bitmap as *mut _),
            old: HGDIOBJ(b.old as *mut _),
            bits: b.bits as *mut u32,
            stride: b.w,
            w,
            h,
            x: rect.left,
            y: rect.top,
            gdi: false,
            shared,
        };
        if w * h <= SHARED_LIMIT && !SHARED_IN_USE.with(|u| u.replace(true)) {
            return SHARED.with(|cell| {
                let mut cell = cell.borrow_mut();
                if cell.as_ref().is_none_or(|b| b.w < w || b.h < h) {
                    let (bw, bh) = cell.as_ref().map_or((w, h), |b| (b.w.max(w), b.h.max(h)));
                    if let Some(old) = cell.take() {
                        free_buffer(&old);
                    }
                    *cell = Some(new_buffer(target, bw, bh));
                }
                canvas(cell.as_ref().unwrap(), true)
            });
        }
        canvas(&new_buffer(target, w, h), false)
    }

    /// The DC for GDI drawing of one's own.
    pub fn hdc(&mut self) -> HDC {
        self.gdi = true;
        self.dc
    }

    pub fn rect(&self) -> RECT {
        RECT { left: 0, top: 0, right: self.w, bottom: self.h }
    }

    fn pixels(&mut self) -> &mut [u32] {
        if self.gdi {
            unsafe {
                let _ = GdiFlush();
            }
            self.gdi = false;
        }
        if self.bits.is_null() {
            return &mut [];
        }
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.stride * self.h) as usize) }
    }

    /// The rows of the part in use.
    fn rows(&mut self) -> impl Iterator<Item = &mut [u32]> {
        let (w, stride) = (self.w as usize, self.stride as usize);
        self.pixels().chunks_exact_mut(stride).map(move |row| &mut row[..w])
    }

    pub fn clear(&mut self, color: Color) {
        let bgr = bgr(color);
        for row in self.rows() {
            row.fill(bgr);
        }
    }

    /// Blends `color` by the coverage `shape` gives each pixel centre
    /// (a signed distance: negative inside) within the box.
    fn cover(&mut self, bounds: (f32, f32, f32, f32), color: Color, alpha: f32, shape: impl Fn(f32, f32) -> f32) {
        let (w, h, stride) = (self.w, self.h, self.stride);
        let x0 = (bounds.0.floor() as i32 - 1).max(0);
        let y0 = (bounds.1.floor() as i32 - 1).max(0);
        let x1 = (bounds.2.ceil() as i32 + 1).min(w);
        let y1 = (bounds.3.ceil() as i32 + 1).min(h);
        let pixels = self.pixels();
        if pixels.is_empty() {
            return;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let coverage = (0.5 - shape(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0) * alpha;
                if coverage > 0.0 {
                    blend(&mut pixels[(y * stride + x) as usize], color, coverage);
                }
            }
        }
    }

    /// Blends `color` by the coverage `draw` leaves in a mask (and returns
    /// the box of), clearing it: the parts of one shape merge before they
    /// are blended, so a translucent polyline's joints don't darken.
    fn masked(&mut self, color: Color, alpha: f32, draw: impl FnOnce(&mut [f32], i32, i32) -> Option<(i32, i32, i32, i32)>) {
        let (w, h, stride) = (self.w, self.h, self.stride);
        MASK.with(|mask| {
            let mut mask = mask.borrow_mut();
            let n = (w * h) as usize;
            if mask.len() < n {
                mask.resize(n, 0.0);
            }
            let Some((x0, y0, x1, y1)) = draw(&mut mask[..n], w, h) else { return };
            let pixels = self.pixels();
            for y in y0..y1 {
                for x in x0..x1 {
                    let coverage = std::mem::take(&mut mask[(y * w + x) as usize]) * alpha;
                    if coverage > 0.0 && !pixels.is_empty() {
                        blend(&mut pixels[(y * stride + x) as usize], color, coverage);
                    }
                }
            }
        });
    }

    /// A shape given by its signed distance (negative inside), `alpha`
    /// opaque.
    pub fn shape(&mut self, bounds: (f32, f32, f32, f32), color: Color, alpha: f32, distance: impl Fn(f32, f32) -> f32) {
        self.cover(bounds, color, alpha, distance);
    }

    /// A polyline `width` wide, `alpha` opaque, its joints even.
    pub fn stroke(&mut self, points: &[(f32, f32)], width: f32, color: Color, alpha: f32) {
        if points.len() < 2 {
            return;
        }
        let pad = width / 2.0 + 1.0;
        self.masked(color, alpha, |mask, w, h| {
            let mut all: Option<(i32, i32, i32, i32)> = None;
            for pair in points.windows(2) {
                let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
                let span = span((ax.min(bx) - pad, ay.min(by) - pad, ax.max(bx) + pad, ay.max(by) + pad), w, h);
                for y in span.1..span.3 {
                    for x in span.0..span.2 {
                        let d = segment_distance(x as f32 + 0.5, y as f32 + 0.5, ax, ay, bx, by);
                        let m = &mut mask[(y * w + x) as usize];
                        *m = m.max((width / 2.0 + 0.5 - d).clamp(0.0, 1.0));
                    }
                }
                all = Some(all.map_or(span, |a| (a.0.min(span.0), a.1.min(span.1), a.2.max(span.2), a.3.max(span.3))));
            }
            all
        });
    }

    /// The area under a polyline whose x increases, down to `bottom`.
    pub fn fill_under(&mut self, points: &[(f32, f32)], bottom: f32, color: Color, alpha: f32) {
        let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
        if points.len() < 2 {
            return;
        }
        self.masked(color, alpha, |mask, w, h| {
            let span = span((first.0, 0.0, last.0, bottom), w, h);
            let mut segment = 0;
            for x in span.0..span.2 {
                let px = x as f32 + 0.5;
                if px < first.0 || px > last.0 {
                    continue;
                }
                while segment + 2 < points.len() && points[segment + 1].0 < px {
                    segment += 1;
                }
                let ((ax, ay), (bx, by)) = (points[segment], points[segment + 1]);
                let top = if bx > ax { ay + (by - ay) * (px - ax) / (bx - ax) } else { ay };
                for y in span.1..span.3.min(bottom.ceil() as i32) {
                    mask[(y * w + x) as usize] = (y as f32 + 1.0 - top).clamp(0.0, 1.0);
                }
            }
            Some(span)
        });
    }

    /// A filled rounded rectangle.
    pub fn round_rect(&mut self, r: RectF, radius: f32, color: Color) {
        self.round_rect_alpha(r, radius, color, 1.0);
    }

    pub fn round_rect_alpha(&mut self, r: RectF, radius: f32, color: Color, alpha: f32) {
        self.cover((r.x, r.y, r.x + r.w, r.y + r.h), color, alpha, |x, y| rounded_box(x, y, r, radius));
    }

    /// A rounded rectangle's outline, `width` inside its edge.
    pub fn round_frame(&mut self, r: RectF, radius: f32, width: f32, color: Color) {
        self.cover((r.x, r.y, r.x + r.w, r.y + r.h), color, 1.0, |x, y| (rounded_box(x, y, r, radius) + width / 2.0).abs() - width / 2.0);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.cover((cx - radius, cy - radius, cx + radius, cy + radius), color, 1.0, |x, y| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - radius);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, radius: f32, width: f32, color: Color) {
        self.cover((cx - radius, cy - radius, cx + radius, cy + radius), color, 1.0, |x, y| {
            (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - radius + width / 2.0).abs() - width / 2.0
        });
    }

    /// A polyline `width` wide (for check marks), anti-aliased.
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color) {
        for pair in points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let bounds = (ax.min(bx) - width, ay.min(by) - width, ax.max(bx) + width, ay.max(by) + width);
            self.cover(bounds, color, 1.0, |x, y| segment_distance(x, y, ax, ay, bx, by) - width / 2.0);
        }
    }

    /// One line or a wrapped block of text, with GDI (ClearType reads the
    /// pixels already drawn).
    pub fn text(&mut self, text: &str, mut rect: RECT, font: HFONT, color: Color, format: DRAW_TEXT_FORMAT) {
        if text.is_empty() {
            return;
        }
        let mut text: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let old = SelectObject(self.dc, font);
            SetTextColor(self.dc, COLORREF(color));
            DrawTextW(self.dc, &mut text, &mut rect, format | DT_NOPREFIX);
            SelectObject(self.dc, old);
        }
        self.gdi = true;
    }

    /// Text with `&` mnemonics, underlined only while keyboard cues show.
    pub fn label(&mut self, text: &str, rect: RECT, font: HFONT, color: Color, format: DRAW_TEXT_FORMAT, cues: bool) {
        if text.is_empty() {
            return;
        }
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut rect = rect;
        unsafe {
            let old = SelectObject(self.dc, font);
            SetTextColor(self.dc, COLORREF(color));
            DrawTextW(self.dc, &mut wide, &mut rect, format | if cues { DRAW_TEXT_FORMAT(0) } else { DT_HIDEPREFIX });
            SelectObject(self.dc, old);
        }
        self.gdi = true;
    }

    /// Copies the buffer to `target`, opaque: buttons paint into 32-bit
    /// buffers whose alpha the system composes with.
    pub fn present(mut self, target: HDC) {
        for row in self.rows() {
            for p in row {
                *p |= 0xff00_0000;
            }
        }
        unsafe {
            let _ = BitBlt(target, self.x, self.y, self.w, self.h, self.dc, 0, 0, SRCCOPY);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        if self.shared {
            SHARED_IN_USE.with(|u| u.set(false));
        } else {
            free_buffer(&Buffer { dc: self.dc.0 as isize, bitmap: self.bitmap.0 as isize, old: self.old.0 as isize, bits: 0, w: 0, h: 0 });
        }
    }
}

/// Moves a buffer pixel (0x00rrggbb) towards `color` by `coverage`.
fn blend(p: &mut u32, color: Color, coverage: f32) {
    let mix = |at: u32, from: u32| {
        let (d, s) = (((*p >> at) & 0xff) as f32, ((color >> from) & 0xff) as f32);
        ((d + (s - d) * coverage).round() as u32) << at
    };
    *p = mix(0, 16) | mix(8, 8) | mix(16, 0);
}

/// The pixels a box covers, clipped to `w` × `h`.
fn span((x0, y0, x1, y1): (f32, f32, f32, f32), w: i32, h: i32) -> (i32, i32, i32, i32) {
    let clip = |v: f32, n: i32| (v.max(0.0) as i32).min(n);
    (clip(x0.floor(), w), clip(y0.floor(), h), clip(x1.ceil() + 1.0, w), clip(y1.ceil() + 1.0, h))
}

fn bgr(color: Color) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn of(r: RECT) -> Self {
        Self { x: r.left as f32, y: r.top as f32, w: (r.right - r.left) as f32, h: (r.bottom - r.top) as f32 }
    }
    pub fn inset(self, dx: f32, dy: f32) -> Self {
        Self { x: self.x + dx, y: self.y + dy, w: self.w - 2.0 * dx, h: self.h - 2.0 * dy }
    }
}

/// The signed distance from a rounded box.
fn rounded_box(x: f32, y: f32, r: RectF, radius: f32) -> f32 {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let qx = (x - cx).abs() - r.w / 2.0 + radius;
    let qy = (y - cy).abs() - r.h / 2.0 + radius;
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius
}

fn segment_distance(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
}

pub fn dpi_of(hwnd: HWND) -> f32 {
    unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0 }
}

/// Whether focus rectangles show for `hwnd` (after keyboard use).
pub fn focus_cues(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, WM_QUERYUISTATE, WPARAM(0), LPARAM(0)).0 as u32 & UISF_HIDEFOCUS == 0 }
}

pub fn accel_cues(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, WM_QUERYUISTATE, WPARAM(0), LPARAM(0)).0 as u32 & UISF_HIDEACCEL == 0 }
}

/// The keyboard focus outline around `r`.
pub fn focus_ring(canvas: &mut Canvas, r: RectF, radius: f32, scale: f32) {
    let p = theme::palette();
    canvas.round_frame(r, radius, 2.0 * scale, p.text);
}

pub fn window_text(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
    String::from_utf16_lossy(&text[..n])
}

pub fn font_of(hwnd: HWND) -> HFONT {
    unsafe { HFONT(SendMessageW(hwnd, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _) }
}

pub fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut name) } as usize;
    String::from_utf16_lossy(&name[..n])
}

// MARK: Roles

/// How a control is drawn, when its style doesn't say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Accent filled (Open Photograph…, Done, OK).
    Primary = 1,
    Secondary = 2,
    /// A small glyph, grey until hovered (reset buttons).
    Glyph = 3,
    /// A segment of a two-part switch.
    SegmentLeft = 4,
    SegmentRight = 5,
    /// A segment between two others (a switch of more than two parts).
    SegmentMiddle = 6,
    /// A value box: no border until hovered or focused.
    Value = 10,
    /// A text field with a border.
    Field = 11,
    /// Accent text, underlined under the pointer (recent photographs).
    Link = 12,
}

const ROLE: PCWSTR = w!("NegsRole");

pub fn set_role(hwnd: HWND, role: Role) {
    unsafe {
        let _ = SetPropW(hwnd, ROLE, HANDLE(role as isize as *mut _));
    }
}

fn role(hwnd: HWND) -> Option<Role> {
    let value = unsafe { GetPropW(hwnd, ROLE) }.0 as isize;
    [Role::Primary, Role::Secondary, Role::Glyph, Role::SegmentLeft, Role::SegmentRight, Role::SegmentMiddle, Role::Value, Role::Field, Role::Link].into_iter().find(|r| *r as isize == value)
}

/// A button's role: set explicitly, else from its style.
fn button_role(hwnd: HWND) -> Option<ButtonKind> {
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    let kind = style & BS_TYPEMASK as u32;
    if let Some(role) = role(hwnd) {
        return Some(match role {
            Role::Primary => ButtonKind::Push(true),
            Role::Glyph => ButtonKind::Glyph,
            Role::SegmentLeft => ButtonKind::Segment(Part::First),
            Role::SegmentRight => ButtonKind::Segment(Part::Last),
            Role::SegmentMiddle => ButtonKind::Segment(Part::Middle),
            Role::Link => ButtonKind::Link,
            _ => ButtonKind::Push(false),
        });
    }
    match kind as i32 {
        BS_AUTOCHECKBOX | BS_CHECKBOX => Some(ButtonKind::Check),
        BS_AUTORADIOBUTTON | BS_RADIOBUTTON if style & BS_PUSHLIKE as u32 != 0 => Some(ButtonKind::Segment(if style & WS_GROUP.0 == 0 { Part::Last } else { Part::First })),
        BS_AUTORADIOBUTTON | BS_RADIOBUTTON => Some(ButtonKind::Radio),
        BS_DEFPUSHBUTTON => Some(ButtonKind::Push(true)),
        BS_PUSHBUTTON => {
            let text = window_text(hwnd);
            let glyph = text.chars().count() == 1 && text.chars().all(|c| ('\u{e000}'..='\u{f8ff}').contains(&c));
            Some(if glyph { ButtonKind::Glyph } else { ButtonKind::Push(false) })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ButtonKind {
    /// Primary or not.
    Push(bool),
    Glyph,
    Check,
    Radio,
    Segment(Part),
    Link,
}

/// Where a segment sits in its switch.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    First,
    Middle,
    Last,
}

// MARK: Dimmed labels

const DIMMED: PCWSTR = w!("NegsDimmed");

/// Enables or disables a control. A disabled static control embosses its
/// text whatever colour it is given (white over grey, which shows in dark),
/// so a label stays enabled and is dimmed instead, and its parent's
/// `WM_CTLCOLORSTATIC` gives it the disabled colour (`is_dimmed`).
pub fn enable(hwnd: HWND, on: bool) {
    unsafe {
        let enabled = IsWindowEnabled(hwnd).as_bool();
        if (enabled && !is_dimmed(hwnd)) == on {
            return;
        }
        if class_of(hwnd) == "Static" {
            if !enabled {
                let _ = EnableWindow(hwnd, true);
            }
            if on {
                let _ = RemovePropW(hwnd, DIMMED);
            } else {
                let _ = SetPropW(hwnd, DIMMED, HANDLE(1 as *mut _));
            }
            let _ = InvalidateRect(hwnd, None, true);
        } else {
            let _ = EnableWindow(hwnd, on);
        }
    }
}

/// Whether a label shows as disabled: disabled, or dimmed by `enable`.
pub fn is_dimmed(hwnd: HWND) -> bool {
    unsafe { !GetPropW(hwnd, DIMMED).is_invalid() || !IsWindowEnabled(hwnd).as_bool() }
}

// MARK: Hover

thread_local! {
    /// The control under the pointer, for the controls that don't track it.
    static HOT: Cell<isize> = const { Cell::new(0) };
    /// The controls whose hover the message loop follows.
    static TRACKED: std::cell::RefCell<std::collections::HashSet<isize>> = std::cell::RefCell::new(Default::default());
}

pub fn is_hot(hwnd: HWND) -> bool {
    HOT.with(Cell::get) == hwnd.0 as isize
}

/// Tracks the pointer over `hwnd` and repaints it when it enters or
/// leaves. The message loop follows it (`hover_message`), which costs
/// nothing per control; value boxes are subclassed instead, since their
/// field also repaints on focus.
pub fn track_hover(hwnd: HWND) {
    if is_field(hwnd) {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(hover_subclass), 0x4e48, 0);
        }
    } else {
        TRACKED.with(|t| t.borrow_mut().insert(hwnd.0 as isize));
    }
}

/// The message loop's part of hover: entering a tracked control, and the
/// `WM_MOUSELEAVE` it asked for.
pub fn hover_message(msg: &MSG) {
    let hwnd = msg.hwnd;
    match msg.message {
        WM_MOUSEMOVE if !is_hot(hwnd) && TRACKED.with(|t| t.borrow().contains(&(hwnd.0 as isize))) => unsafe {
            let old = HWND(HOT.with(|h| h.replace(hwnd.0 as isize)) as *mut _);
            if !old.is_invalid() {
                repaint(old);
            }
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            let _ = TrackMouseEvent(&mut track);
            repaint(hwnd);
        },
        WM_MOUSELEAVE if is_hot(hwnd) && TRACKED.with(|t| t.borrow().contains(&(hwnd.0 as isize))) => {
            HOT.with(|h| h.set(0));
            repaint(hwnd);
        }
        _ => {}
    }
}

fn repaint(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, true);
        if matches!(role(hwnd), Some(Role::Value | Role::Field)) {
            if let Ok(parent) = GetParent(hwnd) {
                let r = field_rect(hwnd);
                let _ = InvalidateRect(parent, Some(&r), true);
            }
        }
    }
}

extern "system" fn hover_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE if !is_hot(hwnd) => {
                let old = HWND(HOT.with(|h| h.replace(hwnd.0 as isize)) as *mut _);
                if !old.is_invalid() {
                    repaint(old);
                }
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                repaint(hwnd);
            }
            WM_MOUSELEAVE if is_hot(hwnd) => {
                HOT.with(|h| h.set(0));
                repaint(hwnd);
            }
            WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                repaint(hwnd);
                return result;
            }
            WM_NCDESTROY if is_hot(hwnd) => HOT.with(|h| h.set(0)),
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

// MARK: Value boxes and fields

/// The rounded field around a value box, in its parent's coordinates: the
/// box plus room for the padding.
pub fn field_rect(edit: HWND) -> RECT {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(edit, &mut r);
        let parent = GetParent(edit).unwrap_or_default();
        let mut points = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        MapWindowPoints(None, parent, &mut points);
        let s = dpi_of(edit);
        let dx = (6.0 * s).round() as i32;
        let dy = (((24.0 * s).round() as i32) - (points[1].y - points[0].y)).max(0);
        RECT { left: points[0].x - dx, top: points[0].y - dy / 2, right: points[1].x + dx, bottom: points[1].y + dy - dy / 2 }
    }
}

/// Where a value box goes inside `field` (its height the font's line).
pub fn edit_in_field(field: RECT, line: i32, scale: f64) -> RECT {
    let dx = (6.0 * scale).round() as i32;
    let dy = (field.bottom - field.top - line).max(0);
    RECT { left: field.left + dx, top: field.top + dy / 2, right: field.right - dx, bottom: field.top + dy / 2 + line }
}

fn field_active(edit: HWND) -> bool {
    unsafe { is_hot(edit) || GetFocus() == edit || role(edit) == Some(Role::Field) }
}

/// The colour behind a value box's text.
pub fn field_background(edit: HWND) -> Color {
    let p = theme::palette();
    if field_active(edit) { p.control } else { p.panel }
}

/// `WM_CTLCOLOREDIT` (and a disabled box's `WM_CTLCOLORSTATIC`).
pub fn color_edit(dc: HDC, edit: HWND) -> LRESULT {
    let p = theme::palette();
    let background = field_background(edit);
    let enabled = unsafe { IsWindowEnabled(edit) }.as_bool();
    unsafe {
        SetTextColor(dc, COLORREF(if enabled { p.text } else { p.disabled }));
        SetBkColor(dc, COLORREF(background));
        LRESULT(theme::brush(background).0 as isize)
    }
}

pub fn is_field(hwnd: HWND) -> bool {
    matches!(role(hwnd), Some(Role::Value | Role::Field))
}

/// Paints a panel's background and the fields of its value boxes.
pub fn paint_panel(hwnd: HWND, background: Color) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        FillRect(dc, &ps.rcPaint, theme::brush(background));
        let s = dpi_of(hwnd);
        let p = theme::palette();
        let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
        while !child.is_invalid() {
            if is_field(child) && IsWindowVisible(child).as_bool() && field_active(child) {
                let r = field_rect(child);
                let mut overlap = RECT::default();
                if IntersectRect(&mut overlap, &r, &ps.rcPaint).as_bool() {
                    let mut canvas = Canvas::new(dc, r);
                    canvas.clear(background);
                    let f = RectF::of(canvas.rect());
                    let focused = GetFocus() == child;
                    canvas.round_rect(f, 4.0 * s, p.border);
                    canvas.round_rect(f.inset(s, s), 3.0 * s, field_background(child));
                    if focused {
                        // The Fluent underline of a focused text field.
                        canvas.round_rect(RectF::new(f.x + s, f.y + f.h - 2.0 * s, f.w - 2.0 * s, 2.0 * s), s, p.accent);
                    }
                    canvas.present(dc);
                }
            }
            child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

// MARK: Custom draw

/// Paints a button or trackbar's `NM_CUSTOMDRAW` itself; None for other
/// notifications, which the caller handles as before.
pub fn custom_draw(lparam: LPARAM) -> Option<LRESULT> {
    unsafe {
        let header = &*(lparam.0 as *const NMHDR);
        if header.code != NM_CUSTOMDRAW {
            return None;
        }
        let draw = &*(lparam.0 as *const NMCUSTOMDRAW);
        let class = class_of(header.hwndFrom);
        match class.as_str() {
            "Button" => {
                let kind = button_role(header.hwndFrom)?;
                // Buttons ask before erasing; skipping that skips the paint too.
                if draw.dwDrawStage == CDDS_PREERASE || draw.dwDrawStage == CDDS_PREPAINT {
                    paint_button(draw, kind);
                }
                Some(LRESULT(CDRF_SKIPDEFAULT as isize))
            }
            "msctls_trackbar32" => {
                if draw.dwDrawStage == CDDS_PREPAINT {
                    paint_trackbar(draw);
                }
                Some(LRESULT(CDRF_SKIPDEFAULT as isize))
            }
            _ => None,
        }
    }
}

fn paint_button(draw: &NMCUSTOMDRAW, kind: ButtonKind) {
    let hwnd = draw.hdr.hwndFrom;
    let p = theme::palette();
    let s = dpi_of(hwnd);
    let state = draw.uItemState;
    let disabled = state.0 & CDIS_DISABLED.0 != 0;
    let hot = state.0 & CDIS_HOT.0 != 0 || is_hot(hwnd);
    let pressed = state.0 & CDIS_SELECTED.0 != 0;
    let focused = state.0 & CDIS_FOCUS.0 != 0 && focus_cues(hwnd);
    let checked = unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 } == 1;
    let text = window_text(hwnd);
    let font = font_of(hwnd);
    let cues = accel_cues(hwnd);
    let background = parent_background(hwnd);
    let mut c = Canvas::new(draw.hdc, draw.rc);
    c.clear(background);
    let r = RectF::of(c.rect());
    let radius = 4.0 * s;
    let center = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
    match kind {
        ButtonKind::Push(primary) => {
            let (fill, ink) = if disabled {
                (if primary { p.border } else { p.control }, p.disabled)
            } else if primary {
                (if pressed { p.accent_pressed } else if hot { p.accent_hover } else { p.accent }, p.on_accent)
            } else {
                (if pressed { p.control_pressed } else if hot { p.control_hover } else { p.control }, p.text)
            };
            if !primary || disabled {
                c.round_rect(r, radius, p.border);
                c.round_rect(r.inset(s, s), radius - s, fill);
            } else {
                c.round_rect(r, radius, fill);
            }
            c.label(&text, c.rect(), font, ink, center, cues);
            if focused {
                focus_ring(&mut c, r, radius, s);
            }
        }
        ButtonKind::Link => {
            let ink = if disabled { p.disabled } else if pressed { p.accent_pressed } else if hot { p.accent_hover } else { p.accent };
            let mut measure = RECT::default();
            let mut wide_text: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let old = SelectObject(c.dc, font);
                if !wide_text.is_empty() {
                    DrawTextW(c.dc, &mut wide_text, &mut measure, DT_CALCRECT | DT_SINGLELINE);
                }
                SelectObject(c.dc, old);
            }
            let left = 2.0 * s;
            c.label(&text, RECT { left: left as i32, top: 0, right: c.w, bottom: c.h }, font, ink, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS, cues);
            let width = (measure.right as f32).min(r.w - left);
            if hot {
                let baseline = ((r.h + measure.bottom as f32) / 2.0).round();
                c.round_rect(RectF::new(left, baseline - s, width, s), 0.0, ink);
            }
            if focused {
                focus_ring(&mut c, RectF::new(0.0, 0.0, width + 2.0 * left, r.h), radius, s);
            }
        }
        ButtonKind::Glyph => {
            if hot && !disabled {
                c.round_rect(r, radius, if pressed { p.subtle_pressed } else { p.subtle_hover });
            }
            let ink = if disabled { p.disabled } else if hot { p.text } else { p.secondary };
            c.text(&text, c.rect(), font, ink, center);
            if focused {
                focus_ring(&mut c, r, radius, s);
            }
        }
        ButtonKind::Segment(part) => {
            // Each segment draws its part of one rounded outline, which
            // reaches past the sides that have a neighbour.
            let (before, after) = (part != Part::First, part != Part::Last);
            let x = if before { r.x - r.w } else { r.x };
            let whole = RectF::new(x, r.y, r.w * (1 + before as u8 + after as u8) as f32, r.h);
            c.round_rect(whole, radius + s, p.border);
            c.round_rect(whole.inset(s, s), radius, if hot && !checked && !disabled { p.control_hover } else { p.control });
            if before {
                c.round_rect(RectF::new(0.0, r.y + 5.0 * s, s, r.h - 10.0 * s), 0.0, p.border);
            }
            if checked {
                let (left, right) = (if before { 2.0 * s } else { 3.0 * s }, if after { 2.0 * s } else { 3.0 * s });
                let inset = RectF::new(r.x + left, r.y + 3.0 * s, r.w - left - right, r.h - 6.0 * s);
                c.round_rect(inset, radius - s, if disabled { p.disabled } else { p.accent });
            }
            // A disabled choice keeps its text readable on the grey fill.
            let ink = if checked { if disabled { p.control } else { p.on_accent } } else if disabled { p.disabled } else { p.text };
            c.label(&text, c.rect(), font, ink, center, cues);
            if focused {
                focus_ring(&mut c, r.inset(s, s), radius, s);
            }
        }
        ButtonKind::Check | ButtonKind::Radio => {
            let size = (16.0 * s).round();
            let top = ((r.h - size) / 2.0).round();
            let mark = RectF::new(s, top, size, size);
            let on = checked && !disabled;
            if kind == ButtonKind::Check {
                if checked {
                    c.round_rect(mark, 3.0 * s, if disabled { p.disabled } else if hot { p.accent_hover } else { p.accent });
                    let (x, y, k) = (mark.x, mark.y, size / 16.0);
                    c.polyline(&[(x + 4.0 * k, y + 8.3 * k), (x + 6.8 * k, y + 11.0 * k), (x + 12.0 * k, y + 5.5 * k)], 1.6 * s, p.on_accent);
                } else {
                    c.round_rect(mark, 3.0 * s, if disabled { p.disabled } else { p.strong_border });
                    c.round_rect(mark.inset(s, s), 2.0 * s, if hot && !disabled { p.control_hover } else { p.control });
                }
            } else {
                let (cx, cy, radius) = (mark.x + size / 2.0, mark.y + size / 2.0, size / 2.0);
                if checked {
                    c.circle(cx, cy, radius, if disabled { p.disabled } else if hot { p.accent_hover } else { p.accent });
                    c.circle(cx, cy, radius * if hot && on { 0.42 } else { 0.36 }, p.on_accent);
                } else {
                    c.circle(cx, cy, radius, if disabled { p.disabled } else { p.strong_border });
                    c.circle(cx, cy, radius - s, if hot && !disabled { p.control_hover } else { p.control });
                }
            }
            let left = (mark.x + size + 8.0 * s) as i32;
            let mut measure = RECT::default();
            let mut wide_text: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let old = SelectObject(c.dc, font);
                if !wide_text.is_empty() {
                    DrawTextW(c.dc, &mut wide_text, &mut measure, DT_CALCRECT | DT_SINGLELINE);
                }
                SelectObject(c.dc, old);
            }
            let tr = RECT { left, top: 0, right: c.w, bottom: c.h };
            c.label(&text, tr, font, if disabled { p.disabled } else { p.text }, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS, cues);
            if focused {
                let right = (left as f32 + measure.right as f32 + 4.0 * s).min(r.w);
                focus_ring(&mut c, RectF::new(0.0, 0.0, right, r.h), radius, s);
            }
        }
    }
    c.present(draw.hdc);
}

/// What the control's parent paints behind it.
fn parent_background(hwnd: HWND) -> Color {
    let p = theme::palette();
    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
    match class_of(parent).as_str() {
        "NegsEmpty" | "NegsStrip" => p.window,
        "msctls_statusbar32" => p.panel,
        _ => p.panel,
    }
}

const DEFAULT_POS: PCWSTR = w!("NegsDefault");
/// Stored above zero, which means none.
const DEFAULT_OFFSET: isize = 1 << 20;

/// The position a slider's fill starts from (its default value).
pub fn set_default_pos(trackbar: HWND, pos: isize) {
    unsafe {
        if GetPropW(trackbar, DEFAULT_POS).0 as isize != pos + DEFAULT_OFFSET {
            let _ = SetPropW(trackbar, DEFAULT_POS, HANDLE((pos + DEFAULT_OFFSET) as *mut _));
            let _ = InvalidateRect(trackbar, None, false);
        }
    }
}

const TBM_GETRANGEMIN: u32 = WM_USER + 1;
const TBM_GETRANGEMAX: u32 = WM_USER + 2;
const TBM_GETTHUMBRECT: u32 = WM_USER + 25;
const TBM_GETCHANNELRECT: u32 = WM_USER + 26;

/// A thin rounded track, the accent from the default value to the thumb,
/// and a round thumb that grows under the pointer.
fn paint_trackbar(draw: &NMCUSTOMDRAW) {
    let hwnd = draw.hdr.hwndFrom;
    let p = theme::palette();
    let s = dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut channel = RECT::default();
        let mut thumb = RECT::default();
        SendMessageW(hwnd, TBM_GETCHANNELRECT, WPARAM(0), LPARAM(&mut channel as *mut _ as isize));
        SendMessageW(hwnd, TBM_GETTHUMBRECT, WPARAM(0), LPARAM(&mut thumb as *mut _ as isize));
        let min = SendMessageW(hwnd, TBM_GETRANGEMIN, WPARAM(0), LPARAM(0)).0;
        let max = SendMessageW(hwnd, TBM_GETRANGEMAX, WPARAM(0), LPARAM(0)).0;
        let stored = GetPropW(hwnd, DEFAULT_POS).0 as isize;
        let default = if stored == 0 { min } else { stored - DEFAULT_OFFSET }.clamp(min, max);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let hot = is_hot(hwnd);
        let pressed = GetCapture() == hwnd;
        let focused = GetFocus() == hwnd && focus_cues(hwnd);
        let mut c = Canvas::new(draw.hdc, client);
        c.clear(parent_background(hwnd));
        // Centred in the control, which is as tall as the focus ring.
        let cy = (client.top + client.bottom) as f32 / 2.0;
        // The thumb's centre runs from half its width inside the channel's ends.
        let half = (thumb.right - thumb.left) as f32 / 2.0;
        let (left, right) = (channel.left as f32 + half, channel.right as f32 - half);
        let track = 4.0 * s;
        c.round_rect(RectF::new(left - track / 2.0, cy - track / 2.0, right - left + track, track), track / 2.0, if enabled { p.strong_border } else { p.border });
        // Where a value sits on the track, as the control computes the thumb.
        let thumb_x = (thumb.left + thumb.right) as f32 / 2.0;
        let span = (max - min).max(1) as f32;
        let default_x = left + (right - left) * (default - min) as f32 / span;
        if enabled && (thumb_x - default_x).abs() > 0.5 {
            let (a, b) = (default_x.min(thumb_x), default_x.max(thumb_x));
            c.round_rect(RectF::new(a - track / 2.0, cy - track / 2.0, b - a + track, track), track / 2.0, p.accent);
        }
        let radius = 10.0 * s;
        c.circle(thumb_x, cy, radius, p.border);
        c.circle(thumb_x, cy, radius - s, p.control);
        let inner = if !enabled { 0.5 } else if pressed { 0.42 } else if hot { 0.62 } else { 0.52 };
        c.circle(thumb_x, cy, radius * inner, if enabled { if pressed { p.accent_pressed } else { p.accent } } else { p.disabled });
        if focused {
            c.ring(thumb_x, cy, radius + 3.0 * s, 2.0 * s, p.text);
        }
        c.present(draw.hdc);
    }
}

// MARK: Combo boxes

/// Paints a drop-down list's closed box from the palette (a rounded field
/// like the value boxes, the choice and a chevron): no theme needed, in
/// light or dark. The list itself takes `WM_CTLCOLORLISTBOX`.
pub fn paint_combo(combo: HWND) {
    unsafe {
        let _ = SetWindowSubclass(combo, Some(combo_subclass), 0x4e43, 0);
    }
    track_hover(combo);
}

extern "system" fn combo_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                draw_combo(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PRINTCLIENT => {
                draw_combo(hwnd, HDC(wparam.0 as *mut _));
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // The control draws its choice itself, outside `WM_PAINT`, when
            // focus or the choice changes (keys, the wheel, its list's
            // `LBN_SELCHANGE`, `CB_SETCURSEL`), in the system's highlight
            // text: white on the box. Paint over it at once.
            WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE | WM_COMMAND | WM_KEYDOWN | WM_CHAR | WM_MOUSEWHEEL | WM_UPDATEUISTATE | CB_SETCURSEL | CB_SELECTSTRING => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
                result
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn draw_combo(hwnd: HWND, dc: HDC) {
    let p = theme::palette();
    let s = dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let dropped = SendMessageW(hwnd, CB_GETDROPPEDSTATE, WPARAM(0), LPARAM(0)).0 != 0;
        let hot = is_hot(hwnd) && enabled;
        let focused = GetFocus() == hwnd;
        let mut c = Canvas::new(dc, client);
        c.clear(parent_background(hwnd));
        let r = RectF::of(c.rect());
        let fill = if !enabled { p.panel } else if dropped { p.control_pressed } else if hot { p.control_hover } else { p.control };
        c.round_rect(r, 4.0 * s, if focused && !focus_cues(hwnd) { p.strong_border } else { p.border });
        c.round_rect(r.inset(s, s), 3.0 * s, fill);
        let selected = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        if selected >= 0 {
            let length = SendMessageW(hwnd, CB_GETLBTEXTLEN, WPARAM(selected as usize), LPARAM(0)).0.max(0) as usize;
            let mut text = vec![0u16; length + 1];
            SendMessageW(hwnd, CB_GETLBTEXT, WPARAM(selected as usize), LPARAM(text.as_mut_ptr() as isize));
            let text = String::from_utf16_lossy(&text[..length]);
            let tr = RECT { left: (8.0 * s) as i32, top: 0, right: c.w - (26.0 * s) as i32, bottom: c.h };
            c.text(&text, tr, font_of(hwnd), if enabled { p.text } else { p.disabled }, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS);
        }
        chevron(&mut c, r.w - 14.0 * s, r.h / 2.0, true, s, if enabled { p.secondary } else { p.disabled });
        if focused && focus_cues(hwnd) {
            focus_ring(&mut c, r, 4.0 * s, s);
        }
        c.present(dc);
    }
}

// MARK: Owner-drawn headers

/// A flat header row's background: hover and pressed fills.
pub fn header_background(c: &mut Canvas, hwnd: HWND, pressed: bool, disabled: bool) {
    let p = theme::palette();
    let s = dpi_of(hwnd);
    if is_hot(hwnd) && !disabled {
        c.round_rect(RectF::of(c.rect()), 4.0 * s, if pressed { p.subtle_pressed } else { p.subtle_hover });
    }
}

/// The chevron of a group or foldout, pointing right or down.
pub fn chevron(c: &mut Canvas, cx: f32, cy: f32, open: bool, scale: f32, color: Color) {
    let k = 3.5 * scale;
    let points = if open {
        [(cx - k, cy - k / 2.0), (cx, cy + k / 2.0), (cx + k, cy - k / 2.0)]
    } else {
        [(cx - k / 2.0, cy - k), (cx + k / 2.0, cy), (cx - k / 2.0, cy + k)]
    };
    c.polyline(&points, 1.4 * scale, color);
}

/// `WM_ERASEBKGND` with `color`.
/// `WM_CTLCOLORBTN`: an owner-drawn button erases itself with this brush
/// before its `WM_DRAWITEM`, often well before when it is shown or moved.
/// Unanswered, that is the system's light button face, which flashed white
/// in dark mode over tiles and headers.
pub fn button_brush() -> LRESULT {
    LRESULT(theme::brush(theme::palette().panel).0 as isize)
}

pub fn erase(hwnd: HWND, wparam: WPARAM, color: Color) -> LRESULT {
    unsafe {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        FillRect(HDC(wparam.0 as *mut _), &r, theme::brush(color));
    }
    LRESULT(1)
}

impl Canvas {
    /// Paints `color` outside a rounded rectangle, rounding what is drawn.
    pub fn round_corners(&mut self, r: RectF, radius: f32, color: Color) {
        self.cover((r.x, r.y, r.x + r.w, r.y + r.h), color, 1.0, |x, y| {
            // Only inside the rectangle's box.
            if x < r.x || y < r.y || x > r.x + r.w || y > r.y + r.h { 1.0 } else { -rounded_box(x, y, r, radius) }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = theme::rgb(255, 255, 255);
    const BLACK: Color = theme::rgb(0, 0, 0);

    fn red(c: &mut Canvas, x: i32, y: i32) -> f32 {
        let stride = c.stride;
        ((c.pixels()[(y * stride + x) as usize] >> 16) & 0xff) as f32
    }

    fn canvas(w: i32, h: i32) -> Canvas {
        let mut c = Canvas::new(HDC::default(), RECT { left: 0, top: 0, right: w, bottom: h });
        c.clear(BLACK);
        c
    }

    #[test]
    fn strokes_are_antialiased_and_joints_dont_darken() {
        let mut c = canvas(20, 10);
        // A horizontal line through pixel centres, in two segments, half opaque.
        c.stroke(&[(0.0, 4.5), (10.0, 4.5), (20.0, 4.5)], 1.0, WHITE, 0.5);
        assert_eq!(red(&mut c, 10, 4), red(&mut c, 5, 4));
        assert!((red(&mut c, 10, 4) - 127.5).abs() <= 1.0);
        assert_eq!(red(&mut c, 10, 3), 0.0);
        drop(c);
        // Between two rows: half on each.
        let mut c = canvas(20, 10);
        c.stroke(&[(0.0, 5.0), (20.0, 5.0)], 1.0, WHITE, 1.0);
        assert!((red(&mut c, 7, 4) - 127.5).abs() <= 1.0 && (red(&mut c, 7, 5) - 127.5).abs() <= 1.0);
        assert!(MASK.with(|m| m.borrow().iter().all(|v| *v == 0.0)));
    }

    #[test]
    fn the_area_fills_below_its_edge() {
        let mut c = canvas(10, 10);
        c.fill_under(&[(0.0, 10.0), (0.0, 4.5), (10.0, 4.5), (10.0, 10.0)], 10.0, WHITE, 1.0);
        assert_eq!(red(&mut c, 5, 3), 0.0);
        assert!((red(&mut c, 5, 4) - 127.5).abs() <= 1.0);
        assert_eq!(red(&mut c, 5, 9), 255.0);
    }
}
