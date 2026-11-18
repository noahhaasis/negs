//! Lab › Camera › Color Filter's glass discs (`ColorFilterPicker.swift`,
//! issue #21, design r30 · B): None, then each filter in the catalog's
//! order, the chosen one ringed in pencil, its name and glass under them.
//! The inspector owns the module's header and the hint lines under it.

use std::cell::RefCell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::{ink, theme, with_app};
use crate::model::Model;
use crate::state::ColorFilter;

/// A disc's diameter, and the least room each takes with its name (DIPs).
const DISC: f32 = 26.0;
const ITEM: f32 = 34.0;
const NAME: TextStyle = TextStyle::serif(11.5, 400);
const NAME_CHOSEN: TextStyle = TextStyle::serif(11.5, 600);
const TITLE: TextStyle = TextStyle::serif(15.0, 600);
const GLASS: TextStyle = TextStyle::serif(13.0, 400).italic();
/// Its height: 8 above the discs, the disc, 7, the names, then 10 above
/// the chosen one's name and glass and 3 below them.
pub const HEIGHT: f64 = 8.0 + 26.0 + 7.0 + 15.0 + 10.0 + 20.0 + 3.0;

#[derive(Default)]
struct View {
    filters: Vec<ColorFilter>,
    chosen: String,
    hovered: Option<usize>,
    enabled: bool,
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
}

impl View {
    /// The choices: None, then the catalog's filters.
    fn keys(&self) -> Vec<&str> {
        std::iter::once(ColorFilter::NONE).chain(self.filters.iter().map(|f| f.key.as_str())).collect()
    }
    fn filter(&self, index: usize) -> Option<&ColorFilter> {
        index.checked_sub(1).and_then(|i| self.filters.get(i))
    }
    fn name(&self, index: usize) -> &str {
        self.filter(index).map_or("None", |f| f.name.as_str())
    }
    fn chosen_index(&self) -> usize {
        self.keys().iter().position(|k| *k == self.chosen).unwrap_or(0)
    }
}

pub fn register() {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc_),
        hInstance: super::instance(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        lpszClassName: w!("NegsColorFilter"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&class);
    }
}

pub fn create(parent: HWND, id: u16) -> HWND {
    let hwnd = super::create(w!("NegsColorFilter"), "Color filter", WS_CHILD | WS_TABSTOP, WINDOW_EX_STYLE(0), parent, id, HFONT::default());
    super::accessible_name(hwnd, "Color filter");
    hwnd
}

/// Brings the discs up to the model; repaints only when what they show changed.
pub fn update(hwnd: HWND, model: &Model, enabled: bool) {
    let changed = VIEW.with(|v| {
        let mut view = v.borrow_mut();
        let chosen = model.color_filter();
        if view.filters == model.color_filters && view.chosen == chosen && view.enabled == enabled {
            return false;
        }
        view.filters = model.color_filters.clone();
        view.chosen = chosen.to_string();
        view.enabled = enabled;
        true
    });
    if changed {
        let name = VIEW.with(|v| {
            let view = v.borrow();
            let index = view.chosen_index();
            view.filter(index).map_or_else(|| "None".to_string(), |f| format!("{}, {}", f.name, f.glass))
        });
        super::accessible_value(hwnd, &name);
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

/// Each choice's centre across, DIPs: each as wide as its name (at least
/// 34), the room left shared out between them, from 4 inside the gutter.
fn centres(view: &View, width: f32, plain: bool) -> Vec<(f32, f32)> {
    let chosen = view.chosen_index();
    let widths: Vec<f32> = (0..view.keys().len())
        .map(|i| {
            let style = if i == chosen { &NAME_CHOSEN } else { &NAME };
            super::d2d::measure_text(view.name(i), style, 0.0, plain).width.max(ITEM)
        })
        .collect();
    let inset = ink::GUTTER as f32 - 4.0;
    let room = (width - 2.0 * inset - widths.iter().sum::<f32>()).max(0.0);
    let gap = if widths.len() > 1 { (room / (widths.len() - 1) as f32).max(2.0) } else { 0.0 };
    let mut x = inset;
    widths
        .iter()
        .map(|w| {
            let item = (x + w / 2.0, *w);
            x += w + gap;
            item
        })
        .collect()
}

fn hit(hwnd: HWND, lparam: LPARAM) -> Option<usize> {
    let s = draw::dpi_of(hwnd);
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f32 / s, ((lparam.0 >> 16) & 0xffff) as i16 as f32 / s);
    if !(0.0..8.0 + DISC + 7.0 + 15.0).contains(&y) {
        return None;
    }
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    VIEW.with(|v| {
        let view = v.borrow();
        centres(&view, r.right as f32 / s, theme::tokens().plain).iter().position(|(c, w)| (x - c).abs() <= w / 2.0)
    })
}

fn choose(index: usize) {
    let key = VIEW.with(|v| v.borrow().keys().get(index).map(|k| k.to_string()));
    if let Some(key) = key {
        with_app(|app| {
            if app.model.color_filter() != key {
                app.model.set_color_filter(&key);
            }
        });
    }
}

fn paint(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(t.hex.panel));
    let width = client.right as f32 / s;
    let focused = unsafe { GetFocus() } == hwnd && draw::focus_cues(hwnd);
    VIEW.with(|v| {
        let view = v.borrow();
        let alpha = if view.enabled { 1.0 } else { theme::DISABLED_OPACITY };
        let chosen = view.chosen_index();
        c.d2d(s, t.plain, |p| {
            for (i, (cx, w)) in centres(&view, width, t.plain).into_iter().enumerate() {
                let selected = i == chosen;
                let lit = selected || view.hovered == Some(i);
                let (cy, r) = (8.0 + DISC / 2.0, DISC / 2.0);
                match view.filter(i) {
                    Some(filter) => {
                        p.fill_ellipse(cx, cy, r, r, Ink::hex(filter.colour).alpha(alpha));
                        // A highlight, so it reads as glass: white fading out
                        // from up and to the left of the centre, inside the rim.
                        let (hx, hy) = (cx - r + 0.34 * DISC, 8.0 + 0.3 * DISC);
                        for k in (1..=6).rev() {
                            let rr = 6.3 * k as f32 / 6.0;
                            p.fill_ellipse(hx, hy, rr, rr, Ink::hex(0xffffff).alpha(0.5 / 6.0 * alpha));
                        }
                    }
                    None => {
                        p.fill_ellipse(cx, cy, r, r, Ink::hex(t.hex.blank).alpha(alpha));
                        let d = 8.5 / std::f32::consts::SQRT_2;
                        p.line((cx - d, cy + d), (cx + d, cy - d), 1.2, Ink::hex(t.hex.dim).alpha(alpha));
                    }
                }
                // The rim.
                p.stroke_round_rect(RectF::new(cx - r + 0.75, cy - r + 0.75, DISC - 1.5, DISC - 1.5), r - 0.75, 1.5, Ink::hex(0).alpha(0.35 * alpha));
                if selected {
                    p.ring(RectF::new(cx - 17.0, cy - 16.0, 34.0, 32.0), negs_marks::Radii::NUMBER, -8.0, Ink::hex(t.hex.accent).alpha(alpha));
                }
                let style = if selected { &NAME_CHOSEN } else { &NAME };
                let name = view.name(i);
                let m = p.measure(name, style, 0.0);
                let color = if selected { t.hex.paper } else if lit { t.hex.label } else { t.hex.dim };
                p.text(name, style, (cx - m.width / 2.0).round(), 8.0 + DISC + 7.0, Ink::hex(color).alpha(alpha));
                if focused && selected {
                    ink::focus_box(p, RectF::new(cx - w / 2.0, 4.0, w, DISC + 7.0 + 19.0), &t);
                }
            }
            // The chosen one's name, and its glass in italic.
            let top = 8.0 + DISC + 7.0 + 15.0 + 10.0;
            let gutter = ink::GUTTER as f32;
            let title = view.name(chosen);
            let m = p.measure(title, &TITLE, 0.0);
            p.text(title, &TITLE, gutter, top, Ink::hex(t.hex.paper).alpha(alpha));
            if let Some(filter) = view.filter(chosen) {
                let g = p.measure(&filter.glass, &GLASS, 0.0);
                p.text(&filter.glass, &GLASS, gutter + m.width + 7.0, top + m.baseline - g.baseline, Ink::hex(t.hex.dim).alpha(alpha));
            }
        });
    });
    c.present(dc);
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE | WM_ENABLE | WM_SETFOCUS | WM_KILLFOCUS => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                let at = hit(hwnd, lparam);
                let changed = VIEW.with(|v| std::mem::replace(&mut v.borrow_mut().hovered, at) != at);
                if changed {
                    let tip = VIEW.with(|v| {
                        let view = v.borrow();
                        at.map(|i| ColorFilter::help(view.filter(i))).unwrap_or_default()
                    });
                    super::dialogs::tooltip(hwnd, &tip);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                if VIEW.with(|v| v.borrow_mut().hovered.take().is_some()) {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                let hand = VIEW.with(|v| v.borrow().hovered.is_some() && v.borrow().enabled);
                let _ = SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
                LRESULT(1)
            }
            WM_LBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                if let Some(index) = hit(hwnd, lparam)
                    && VIEW.with(|v| v.borrow().enabled)
                {
                    choose(index);
                }
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_KEYDOWN => {
                let key = VIRTUAL_KEY(wparam.0 as u16);
                let (count, chosen) = VIEW.with(|v| (v.borrow().keys().len(), v.borrow().chosen_index()));
                let next = match key {
                    VK_LEFT | VK_UP => chosen.checked_sub(1),
                    VK_RIGHT | VK_DOWN => (chosen + 1 < count).then_some(chosen + 1),
                    VK_HOME => Some(0),
                    VK_END => count.checked_sub(1),
                    _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                };
                if let Some(next) = next {
                    choose(next);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
