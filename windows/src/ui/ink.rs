//! The inspector in ink (`InkControls.swift`, `InkSlider.swift`,
//! IMPLEMENTATION.md §4.8–§4.13): words and rules instead of boxes, cream
//! ink, and the pencil only ever as a mark. Every control stays a window,
//! so Tab, the arrows, access keys and UI Automation work as before; only
//! how each one paints changes, by its `Role`.
//!
//! Painting reads what `inspector::update_values` stored here (which values
//! are changed, a module's summary) and never the app, which may be
//! borrowed while a control repaints. It allocates nothing once warm: the
//! window text goes through one buffer, and text layouts are cached
//! (`d2d.rs`).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::d2d::{Align, Ink, Painter, RectF, TextMetrics, TextStyle};
use super::draw::{self, Canvas, Role};
use super::theme::{self, Tokens};

// MARK: Sizes (`Theme.Size`)

pub const GUTTER: f64 = 22.0;
pub const ROW: f64 = 29.0;
pub const SLIDER_LABEL: f64 = 132.0;
pub const VALUE_WIDTH: f64 = 46.0;
pub const RESET_WIDTH: f64 = 18.0;
pub const SECTION_HEAD: f64 = 34.0;
pub const MODULE_ROW: f64 = 37.0;
pub const TAB_ROW: f64 = 50.0;
pub const PANE_HEADER: f64 = 42.0;
/// Room round a word for its focus box: the Mac draws it 5 pt outside,
/// 1.5 pt wide; here it is drawn inside the control, so controls reach this
/// far past their ink.
pub const MARGIN: f64 = 6.0;
/// The gap between the tab words (`TabWord.gap`).
pub const TAB_GAP: f64 = 24.0;
/// The capitals of the tab words, at 21 pt: centred in their row.
const TAB_CAP: f32 = 14.5;

// MARK: Type

pub const LABEL: TextStyle = TextStyle::ui(12.0, 400);
pub const VALUE: TextStyle = TextStyle::serif(14.5, 400);
pub const VALUE_CHANGED: TextStyle = TextStyle::serif(14.5, 700);
pub const HINT: TextStyle = TextStyle::serif(13.0, 400).italic().line_height(1.3);
pub const SECTION: TextStyle = TextStyle::serif(11.0, 700).tracking(0.13);
const SKIPPED: TextStyle = TextStyle::serif(13.0, 400).italic();
const MODULE: TextStyle = TextStyle::serif(16.0, 600);
const MODULE_OFF: TextStyle = TextStyle::serif(16.0, 400);
const SUMMARY: TextStyle = TextStyle::serif(14.0, 400);
const SUMMARY_OFF: TextStyle = TextStyle::serif(14.0, 400).italic();
pub const WORD: TextStyle = TextStyle::serif(14.0, 400).italic();
pub const MORE: TextStyle = TextStyle::serif(13.0, 400).italic();
pub const PANE_WORD: TextStyle = TextStyle::ui(12.0, 500);
pub const SCOPE_WORD: TextStyle = TextStyle::ui(11.0, 500);
pub const SCOPE_NOTE: TextStyle = TextStyle::ui(10.0, 400);
pub const EMPTY_SCOPE: TextStyle = TextStyle::serif(13.0, 400).italic();
pub const TAB_SIZE: f32 = 21.0;
pub const COUNT: TextStyle = TextStyle::ui(10.5, 600);
pub const EDITS: TextStyle = TextStyle::serif(14.0, 400).italic();
pub const EDITS_NUMBER: TextStyle = TextStyle::serif(14.0, 700).italic();
pub const OUTLINED: TextStyle = TextStyle::serif(13.5, 500);
pub const EXPORT: TextStyle = TextStyle::serif(18.0, 700);
pub const FOOT: TextStyle = TextStyle::serif(14.0, 400).italic();
const GLYPH: TextStyle = TextStyle::ui(12.0, 400);
/// Pressed words drop to 0.62 (§4.13).
const PRESSED: f32 = 0.62;

/// The tab word's style: 700 chosen, 500 not, tracked so that both set as
/// wide (`TabWord.tracking`).
pub fn tab_style(p: &mut Painter, title: &str, chosen: bool) -> TextStyle {
    let (bold, regular) = (TextStyle::serif(TAB_SIZE, 700), TextStyle::serif(TAB_SIZE, 500));
    let n = title.chars().count();
    if n < 2 {
        return if chosen { bold } else { regular };
    }
    let half = (p.measure(title, &bold, 0.0).width - p.measure(title, &regular, 0.0).width) / (n - 1) as f32 / 2.0;
    if chosen { bold.tracking(-half / TAB_SIZE) } else { regular.tracking(half / TAB_SIZE) }
}

/// A tab word's width, chosen or not (they match), for laying out the row.
pub fn tab_width(title: &str, plain: bool) -> f32 {
    let regular = TextStyle::serif(TAB_SIZE, 500);
    let bold = TextStyle::serif(TAB_SIZE, 700);
    let n = title.chars().count().max(2);
    let (b, r) = (super::d2d::measure_text(title, &bold, 0.0, plain).width, super::d2d::measure_text(title, &regular, 0.0, plain).width);
    let half = (b - r) / (n - 1) as f32 / 2.0;
    r + half * n as f32
}

/// A word's width in `style`, for laying out controls that fit their words.
pub fn width(text: &str, style: &TextStyle) -> f32 {
    super::d2d::measure_text(text, style, 0.0, theme::tokens().plain).width
}

/// A hint's height wrapped at `width` DIPs.
pub fn hint_height(text: &str, width: f32) -> f32 {
    super::d2d::measure_text(text, &HINT, width, theme::tokens().plain).height
}

// MARK: What painting reads

/// What a module header, a section head, a foldout or a colour well shows
/// besides its title.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Look {
    pub open: bool,
    /// Switched off: the title in 400 `dim`, "Off" for the summary.
    pub off: bool,
    /// Something in it differs from its default: the yellow dot.
    pub edited: bool,
    /// Scanning the film skips it: struck through in pencil.
    pub skipped: bool,
    /// A hint that warns, in `warn`.
    pub warn: bool,
    /// The folded module's value; a foldout's stock; a colour's name.
    pub summary: String,
    /// A colour well's colour, 0xRRGGBB.
    pub color: u32,
}

thread_local! {
    /// Controls whose value is off its default: their label in `paper`,
    /// the value in 700 with the pencil under it.
    static CHANGED: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    static LOOKS: RefCell<HashMap<isize, Look>> = RefCell::new(HashMap::new());
    /// The window text being painted: read into one buffer.
    static TEXT: RefCell<(Vec<u16>, String)> = RefCell::new((vec![0; 512], String::with_capacity(512)));
    /// The rules painted across a panel, by panel: tops in content
    /// coordinates (the rows panel scrolls).
    static RULES: RefCell<HashMap<isize, Vec<i32>>> = RefCell::new(HashMap::new());
}

/// Marks `hwnd` changed or not, repainting it when that changes.
pub fn set_changed(hwnd: HWND, changed: bool) {
    let flipped = CHANGED.with(|c| {
        let mut c = c.borrow_mut();
        if changed { c.insert(hwnd.0 as isize) } else { c.remove(&(hwnd.0 as isize)) }
    });
    if flipped {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

pub fn is_changed(hwnd: HWND) -> bool {
    CHANGED.with(|c| c.borrow().contains(&(hwnd.0 as isize)))
}

/// Sets what `hwnd` shows besides its title, repainting it when that changes.
pub fn set_look(hwnd: HWND, look: Look) {
    let differs = LOOKS.with(|l| {
        let mut l = l.borrow_mut();
        if l.get(&(hwnd.0 as isize)) == Some(&look) {
            false
        } else {
            l.insert(hwnd.0 as isize, look);
            true
        }
    });
    if differs {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

fn with_look<R>(hwnd: HWND, f: impl FnOnce(&Look) -> R) -> R {
    LOOKS.with(|l| match l.try_borrow() {
        Ok(l) => match l.get(&(hwnd.0 as isize)) {
            Some(look) => f(look),
            None => f(&Look::default()),
        },
        Err(_) => f(&Look::default()),
    })
}

/// `f` with `hwnd`'s text, read without allocating.
pub fn with_text<R>(hwnd: HWND, f: impl FnOnce(&str) -> R) -> R {
    TEXT.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => {
            let (buffer, text) = &mut *guard;
            let n = unsafe { GetWindowTextW(hwnd, buffer) }.max(0) as usize;
            text.clear();
            text.extend(char::decode_utf16(buffer[..n].iter().copied()).map(|c| c.unwrap_or('\u{fffd}')));
            f(text)
        }
        Err(_) => f(&draw::window_text(hwnd)),
    })
}

/// The rules across `panel`, by their tops in content coordinates.
pub fn set_rules(panel: HWND, rules: Vec<i32>) {
    let differs = RULES.with(|r| {
        let mut r = r.borrow_mut();
        let key = panel.0 as isize;
        if r.get(&key) == Some(&rules) {
            false
        } else {
            r.insert(key, rules);
            true
        }
    });
    if differs {
        unsafe {
            let _ = InvalidateRect(panel, None, true);
        }
    }
}

/// Paints `panel`'s rules in `line`, a pixel high, from the gutter-less
/// edge to edge (`InkRule`), scrolled with its rows.
pub fn paint_rules(panel: HWND, dc: HDC, dirty: &RECT) {
    RULES.with(|r| {
        let Ok(r) = r.try_borrow() else { return };
        let Some(rules) = r.get(&(panel.0 as isize)) else { return };
        let scroll = unsafe { GetScrollPos(panel, SB_VERT) };
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(panel, &mut client);
        }
        let s = draw::dpi_of(panel);
        let height = s.max(1.0).floor() as i32;
        let brush = theme::brush(theme::gdi(theme::tokens().hex.line));
        for &top in rules {
            let y = top - scroll;
            if y + height < dirty.top || y > dirty.bottom {
                continue;
            }
            let rect = RECT { left: client.left, top: y, right: client.right, bottom: y + height };
            unsafe {
                FillRect(dc, &rect, brush);
            }
        }
    });
}

/// The scope's words over its well: the waveform's levels (100 … 0) right
/// of `levels.0` pixels at the plot's quarters, and "No developed
/// photograph" in the middle while there is none.
pub fn scope_labels(c: &mut Canvas, s: f32, levels: Option<(usize, usize, usize)>, empty: bool) {
    let t = theme::tokens();
    let (w, h) = (c.w as f32 / s, c.h as f32 / s);
    c.d2d(s, t.plain, |p| {
        if let Some((left, top, plot_h)) = levels {
            for (i, level) in ["100", "75", "50", "25", "0"].into_iter().enumerate() {
                let y = (top + plot_h * i / 4) as f32 / s;
                let right = left as f32 / s - 4.0;
                let metrics = p.measure(level, &SCOPE_NOTE, 0.0);
                p.text(level, &SCOPE_NOTE, (right - metrics.width).round(), (y - SCOPE_NOTE.size * 0.35 - metrics.baseline + SCOPE_NOTE.size * 0.7).round(), Ink::hex(t.hex.dim));
            }
        }
        if empty {
            let text = "No developed photograph";
            let metrics = p.measure(text, &EMPTY_SCOPE, 0.0);
            p.text(text, &EMPTY_SCOPE, ((w - metrics.width) / 2.0).round(), centred(h, &metrics, EMPTY_SCOPE.size), Ink::hex(t.hex.dim));
        }
    });
}

// MARK: Rows and hover

const ROWS_FIRST: i32 = 1000;
const ROW_PARTS: i32 = 8;

/// The row a control belongs to: its panel and its block of ids.
fn row_of(hwnd: HWND) -> Option<(HWND, i32)> {
    unsafe {
        let id = GetDlgCtrlID(hwnd);
        if id < ROWS_FIRST {
            return None;
        }
        Some((GetParent(hwnd).ok()?, (id - ROWS_FIRST) / ROW_PARTS))
    }
}

/// Whether the pointer is over `hwnd`'s row (any of its controls): the
/// label brightens and the knob grows, as on the Mac.
pub fn row_hot(hwnd: HWND) -> bool {
    let hot = draw::hot();
    if hot.is_invalid() {
        return false;
    }
    hot == hwnd || (row_of(hot).is_some() && row_of(hot) == row_of(hwnd))
}

/// Repaints the other controls of `hwnd`'s row when the pointer enters or
/// leaves it.
pub fn repaint_row(hwnd: HWND) {
    let Some((panel, block)) = row_of(hwnd) else { return };
    if !matches!(draw::class_of(panel).as_str(), "NegsRows" | "NegsSettingsPage") {
        return;
    }
    for part in 0..ROW_PARTS {
        unsafe {
            if let Ok(c) = GetDlgItem(panel, ROWS_FIRST + block * ROW_PARTS + part)
                && c != hwnd
            {
                let _ = InvalidateRect(c, None, false);
            }
        }
    }
}

// MARK: Marks

fn hex(rgb: u32, disabled: bool, pressed: bool) -> Ink {
    Ink::hex(rgb).alpha(if disabled { theme::DISABLED_OPACITY } else if pressed { PRESSED } else { 1.0 })
}

/// The keyboard focus box: 1.5 pt `paper`, radius 3, inside `r`.
pub fn focus_box(p: &mut Painter, r: RectF, t: &Tokens) {
    p.stroke_round_rect(RectF::new(r.x + 0.75, r.y + 0.75, r.w - 1.5, r.h - 1.5), 3.0, 1.5, Ink::hex(t.hex.paper));
}

/// Where a line of `style` starts so its capitals sit in the middle of `h`.
fn centred(h: f32, m: &TextMetrics, size: f32) -> f32 {
    ((h + 0.7 * size) / 2.0 - m.baseline).round()
}

/// The yellow edit dot, 5 pt.
pub fn dot(p: &mut Painter, cx: f32, cy: f32, t: &Tokens) {
    p.fill_ellipse(cx, cy, 2.5, 2.5, Ink::hex(t.hex.accent));
}

/// A small down chevron, `⌄`, its top at `y`.
fn caret(p: &mut Painter, x: f32, y: f32, ink: Ink) {
    p.line((x, y), (x + 3.5, y + 3.5), 1.2, ink);
    p.line((x + 3.5, y + 3.5), (x + 7.0, y), 1.2, ink);
}

/// The module's `›`, turned down when open.
fn chevron(p: &mut Painter, x: f32, cy: f32, open: bool, ink: Ink) {
    if open {
        p.line((x, cy - 2.0), (x + 4.0, cy + 2.0), 1.3, ink);
        p.line((x + 4.0, cy + 2.0), (x + 8.0, cy - 2.0), 1.3, ink);
    } else {
        p.line((x + 2.0, cy - 4.0), (x + 6.0, cy), 1.3, ink);
        p.line((x + 6.0, cy), (x + 2.0, cy + 4.0), 1.3, ink);
    }
}

/// The 13 pt tick box (`TickBox`), crossed when on.
fn tick_box(p: &mut Painter, x: f32, y: f32, on: bool, hot: bool, pressed: bool, t: &Tokens, alpha: f32) {
    let r = RectF::new(x, y, 13.0, 13.0);
    if pressed {
        p.fill_round_rect(r, 2.0, Ink::hex(t.hex.paper).alpha(0.12 * alpha));
    }
    let edge = if hot || pressed { t.hex.label } else if on { t.hex.dim } else { t.hex.track };
    p.stroke_round_rect(RectF::new(x + 0.6, y + 0.6, 11.8, 11.8), 2.0, 1.2, Ink::hex(edge).alpha(alpha));
    if on {
        let ink = Ink::hex(t.hex.paper).alpha(alpha);
        p.line((x + 1.5, y + 1.5), (x + 11.5, y + 11.5), 1.4, ink);
        p.line((x + 11.5, y + 1.5), (x + 1.5, y + 11.5), 1.4, ink);
    }
}

// MARK: Buttons (`NM_CUSTOMDRAW`)

/// Paints a button whose role is one of the ink roles.
pub fn paint_button(draw: &NMCUSTOMDRAW, role: Role) {
    let hwnd = draw.hdr.hwndFrom;
    let state = draw.uItemState;
    let disabled = state.0 & CDIS_DISABLED.0 != 0;
    let pressed = state.0 & CDIS_SELECTED.0 != 0;
    let focused = state.0 & CDIS_FOCUS.0 != 0 && draw::focus_cues(hwnd);
    let own_hot = !disabled && draw::is_hot(hwnd);
    let hot = !disabled && (own_hot || row_hot(hwnd));
    let checked = unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 } == 1;
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let mut c = Canvas::new(draw.hdc, draw.rc);
    c.clear(theme::gdi(t.hex.panel));
    let (w, h) = ((draw.rc.right - draw.rc.left) as f32 / s, (draw.rc.bottom - draw.rc.top) as f32 / s);
    let m = MARGIN as f32;
    let skipped = with_look(hwnd, |l| l.skipped);
    with_text(hwnd, |text| {
        c.d2d(s, t.plain, |p| match role {
            Role::PaneWord | Role::ScopeWord => {
                let style = if role == Role::PaneWord { PANE_WORD } else { SCOPE_WORD };
                let ink = if checked { t.hex.paper } else if hot { t.hex.label } else { t.hex.dim };
                let metrics = p.measure(text, &style, 0.0);
                p.text(text, &style, m, centred(h, &metrics, style.size), hex(ink, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            Role::TabWord => {
                let style = tab_style(p, text, checked);
                let metrics = p.measure(text, &style, 0.0);
                // The capitals centred in the row the button spans, a rule above and below.
                let baseline = (TAB_ROW as f32 + TAB_CAP) / 2.0 - 1.0;
                if checked {
                    p.underline(RectF::new(m, baseline + 4.7, metrics.width, 6.0), &t, false);
                }
                let ink = if checked { t.hex.paper } else if hot { t.hex.label } else { t.hex.dim };
                p.text(text, &style, m, baseline - metrics.baseline, hex(ink, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, (baseline - 22.0).max(0.0), w, 32.0f32.min(h)), &t);
                }
            }
            Role::Count => {
                // Hangs from the tab word's top: the dot, then the count.
                let word = p.measure("Prep", &TextStyle::serif(TAB_SIZE, 700), 0.0);
                let top = (TAB_ROW as f32 + TAB_CAP) / 2.0 - 1.0 - word.baseline;
                dot(p, 7.5, top + 8.5, &t);
                p.text(text, &COUNT, 13.0, top + 1.0, hex(if hot { t.hex.paper } else { t.hex.label }, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, top - 2.0, w, 20.0), &t);
                }
            }
            Role::WordButton => {
                let metrics = p.measure(text, &WORD, 0.0);
                let y = centred(h, &metrics, WORD.size);
                p.fill_rect(RectF::new(m, y + metrics.baseline + 2.0, metrics.width, 1.0), hex(if hot { t.hex.dim } else { t.hex.track }, disabled, pressed));
                p.text(text, &WORD, m, y, hex(if hot { t.hex.paper } else { t.hex.label }, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            Role::More => {
                let metrics = p.measure(text, &MORE, 0.0);
                p.text(text, &MORE, m, centred(h, &metrics, MORE.size), hex(if own_hot { t.hex.label } else { t.hex.dim }, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, (metrics.width + 2.0 * m).min(w), h), &t);
                }
            }
            Role::Tick => {
                // The label, then the box at the row's end; the module's
                // switch has no label.
                let alpha = if disabled { theme::DISABLED_OPACITY } else if skipped { 0.4 } else { 1.0 };
                if !text.is_empty() {
                    let bright = is_changed(hwnd) || hot;
                    let metrics = p.measure(text, &LABEL, 0.0);
                    p.clipped(RectF::new(0.0, 0.0, w - m - 21.0, h), |p| {
                        p.text(text, &LABEL, m, centred(h, &metrics, LABEL.size), Ink::hex(if bright { t.hex.paper } else { t.hex.label }).alpha(alpha));
                    });
                }
                tick_box(p, w - m - 13.0, ((h - 13.0) / 2.0).round(), checked, own_hot || (hot && !text.is_empty()), pressed, &t, alpha);
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            Role::Outlined => {
                let metrics = p.measure(text, &OUTLINED, 0.0);
                let pill = RectF::new(m, ((h - 23.0) / 2.0).round(), w - 2.0 * m, 23.0);
                let alpha = if disabled { theme::DISABLED_OPACITY } else { 1.0 };
                if pressed || hot {
                    p.fill_round_rect(pill, 11.5, Ink::hex(t.hex.paper).alpha(if pressed { 0.09 } else { 0.04 }));
                }
                let edge = if hot || pressed { t.hex.dim } else { t.hex.track };
                p.stroke_round_rect(RectF::new(pill.x + 0.5, pill.y + 0.5, pill.w - 1.0, pill.h - 1.0), 11.0, 1.0, Ink::hex(edge).alpha(alpha));
                let y = pill.y + 1.0 + centred(pill.h, &metrics, OUTLINED.size);
                p.text(text, &OUTLINED, pill.x + (pill.w - metrics.width) / 2.0, y, Ink::hex(t.hex.paper).alpha(alpha));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            Role::Export => {
                // Filled, radius 3: hover brightens, pressing sinks 1 pt.
                let sink = if pressed && !disabled { 1.0 } else { 0.0 };
                let face = RectF::new(m, m + sink, w - 2.0 * m, h - 2.0 * m);
                let alpha = if disabled { theme::DISABLED_OPACITY } else { 1.0 };
                p.fill_round_rect(face, 3.0, Ink::hex(t.hex.fill).alpha(alpha));
                if pressed && !disabled {
                    p.fill_round_rect(face, 3.0, Ink::hex(0).alpha(0.12));
                } else if hot {
                    p.fill_round_rect(face, 3.0, Ink::hex(0xffffff).alpha(0.08));
                }
                let metrics = p.measure(text, &EXPORT, 0.0);
                let y = face.y + centred(face.h, &metrics, EXPORT.size) - 1.0;
                p.text(text, &EXPORT, face.x + (face.w - metrics.width) / 2.0, y, Ink::hex(t.hex.on_fill).alpha(alpha));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            Role::ResetMark => {
                // ↺, once the value changed.
                let metrics = p.measure("↺", &GLYPH, 0.0);
                let ink = if own_hot { t.hex.paper } else if hot { t.hex.label } else { t.hex.dim };
                p.text("↺", &GLYPH, ((w - metrics.width) / 2.0).round(), centred(h, &metrics, GLYPH.size), hex(ink, disabled, pressed));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            }
            _ => {}
        });
    });
    c.present(draw.hdc);
}

// MARK: Owner-drawn statics and buttons (`WM_DRAWITEM`)

/// Paints an owner-drawn static or button that has an ink role. False when
/// it has none, for the caller to draw.
pub fn draw_item(item: &DRAWITEMSTRUCT) -> bool {
    let hwnd = item.hwndItem;
    let Some(role) = draw::role_of(hwnd) else { return false };
    if !matches!(role, Role::Label | Role::Section | Role::Hint | Role::Readout | Role::EditCount | Role::Summary | Role::Unit | Role::Module | Role::Well) {
        return false;
    }
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let r = item.rcItem;
    let (w, h) = ((r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s);
    // Statics report ODS_DISABLED unreliably; their own state decides.
    let disabled = draw::is_dimmed(hwnd) || (item.CtlType != ODT_STATIC && item.itemState.0 & ODS_DISABLED.0 != 0);
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0 && draw::focus_cues(hwnd);
    let alpha = if disabled { theme::DISABLED_OPACITY } else { 1.0 };
    let hot = !disabled && (draw::is_hot(hwnd) || row_hot(hwnd));
    let m = MARGIN as f32;
    let mut c = Canvas::new(item.hDC, r);
    c.clear(theme::gdi(t.hex.panel));
    with_text(hwnd, |text| {
        with_look(hwnd, |look| {
            c.d2d(s, t.plain, |p| match role {
                Role::Label => {
                    let bright = is_changed(hwnd) || hot;
                    let metrics = p.measure(text, &LABEL, 0.0);
                    p.clipped(RectF::new(0.0, 0.0, w, h), |p| {
                        p.text(text, &LABEL, 0.0, centred(h, &metrics, LABEL.size), Ink::hex(if bright { t.hex.paper } else { t.hex.label }).alpha(alpha));
                    });
                }
                Role::Unit => {
                    let metrics = p.measure(text, &LABEL, 0.0);
                    p.text(text, &LABEL, 0.0, centred(h, &metrics, LABEL.size), Ink::hex(t.hex.dim).alpha(alpha));
                }
                Role::Section => {
                    // Small bold caps, tracked; struck through and "— skipped"
                    // while scanning the film; the dot when edited.
                    let metrics = p.measure(text, &SECTION, 0.0);
                    let y = centred(h, &metrics, SECTION.size);
                    p.text(text, &SECTION, 0.0, y, Ink::hex(if look.skipped { t.hex.dim } else { t.hex.label }));
                    let mut x = metrics.width;
                    if look.skipped {
                        p.pencil(RectF::new(-1.0, y + metrics.baseline - 6.0, metrics.width + 2.0, 5.0), Ink::hex(t.hex.accent));
                        let skipped = p.measure("— skipped", &SKIPPED, 0.0);
                        x += 10.0;
                        p.text("— skipped", &SKIPPED, x, y + metrics.baseline - skipped.baseline, Ink::hex(t.hex.dim));
                        x += skipped.width;
                    }
                    if look.edited {
                        dot(p, x + 8.0 + 2.5, h / 2.0 + 1.0, &t);
                    }
                }
                Role::Hint => {
                    let ink = if look.warn { t.hex.warn } else { t.hex.dim };
                    p.text_in(text, &HINT, RectF::new(0.0, 0.0, w, 0.0), Align::Leading, Ink::hex(ink).alpha(alpha));
                }
                Role::Readout => {
                    let metrics = p.measure(text, &VALUE, w);
                    p.text_in(text, &VALUE, RectF::new(0.0, centred(h, &metrics, VALUE.size), w, 0.0), Align::Trailing, Ink::hex(t.hex.paper).alpha(alpha));
                }
                Role::Summary => {
                    let metrics = p.measure(text, &FOOT, 0.0);
                    p.clipped(RectF::new(0.0, 0.0, w, h), |p| {
                        p.text(text, &FOOT, ((w - metrics.width) / 2.0).max(0.0).round(), centred(h, &metrics, FOOT.size), Ink::hex(t.hex.label));
                    });
                }
                Role::EditCount => {
                    // "3 edits": the number 700 in `paper`, the word `dim`.
                    let (number, word) = text.split_once(' ').unwrap_or((text, ""));
                    let n = p.measure(number, &EDITS_NUMBER, 0.0);
                    let rest = p.measure(word, &EDITS, 0.0);
                    let x = (w - m - n.width - 4.0 - rest.width).max(0.0);
                    let y = centred(h, &n, EDITS.size);
                    p.text(number, &EDITS_NUMBER, x, y, Ink::hex(t.hex.paper));
                    p.text(word, &EDITS, x + n.width + 4.0, y + n.baseline - rest.baseline, Ink::hex(t.hex.dim));
                }
                Role::Module => paint_module(p, text, look, w, h, hot, pressed, focused, disabled, &t),
                Role::Well => {
                    // A colour well: the colour's name, then a round swatch.
                    let swatch = 14.0;
                    let cx = w - m - swatch / 2.0;
                    let cy = h / 2.0;
                    p.fill_ellipse(cx, cy, swatch / 2.0, swatch / 2.0, Ink::hex(look.color).alpha(alpha));
                    p.stroke_round_rect(RectF::new(cx - swatch / 2.0, cy - swatch / 2.0, swatch, swatch), swatch / 2.0, 1.0, Ink::hex(if hot { t.hex.dim } else { t.hex.track }).alpha(alpha));
                    let name = p.measure(&look.summary, &VALUE, 0.0);
                    p.text(&look.summary, &VALUE, (cx - swatch / 2.0 - 8.0 - name.width).round(), centred(h, &name, VALUE.size), Ink::hex(t.hex.paper).alpha(alpha));
                    if focused {
                        focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                    }
                }
                _ => {}
            });
        });
    });
    c.present(item.hDC);
    true
}

/// A module (Grain, Halation, …): the chevron, the title in the serif, the
/// dot and the folded summary (`InkModule`). Its tick box is a control of
/// its own beside it.
#[allow(clippy::too_many_arguments)]
fn paint_module(p: &mut Painter, title: &str, look: &Look, w: f32, h: f32, hot: bool, pressed: bool, focused: bool, disabled: bool, t: &Tokens) {
    let m = MARGIN as f32;
    let alpha = if disabled { theme::DISABLED_OPACITY } else if pressed { PRESSED } else { 1.0 };
    let quiet = look.off || look.skipped;
    chevron(p, m, h / 2.0, look.open && !look.skipped, Ink::hex(if hot { t.hex.label } else { t.hex.dim }).alpha(alpha));
    let style = if quiet { MODULE_OFF } else { MODULE };
    let metrics = p.measure(title, &style, 0.0);
    let (x, y) = (m + 19.0, centred(h, &metrics, style.size));
    p.text(title, &style, x, y, Ink::hex(if quiet { t.hex.dim } else { t.hex.paper }).alpha(alpha));
    if look.skipped {
        p.pencil(RectF::new(x - 1.0, y + metrics.baseline - 7.0, metrics.width + 2.0, 5.0), Ink::hex(t.hex.accent).alpha(alpha));
    }
    let mut right = w - 11.0;
    if !look.summary.is_empty() && (!look.open || look.skipped) {
        let (text, style) = if look.off { ("Off", SUMMARY_OFF) } else { (look.summary.as_str(), SUMMARY) };
        let s = p.measure(text, &style, 0.0);
        right -= s.width;
        let ink = if look.edited { t.hex.paper } else { t.hex.dim };
        p.text(text, &style, right.max(x + metrics.width + 8.0), y + metrics.baseline - s.baseline, Ink::hex(ink).alpha(alpha));
        right -= 8.0;
    }
    if look.edited {
        dot(p, right - 2.5, h / 2.0, t);
    }
    if focused {
        focus_box(p, RectF::new(0.0, 0.0, w, h), t);
    }
}

// MARK: Sliders

const TBM_GETRANGEMIN: u32 = WM_USER + 1;
const TBM_GETRANGEMAX: u32 = WM_USER + 2;
const TBM_GETTHUMBRECT: u32 = WM_USER + 25;
const TBM_GETCHANNELRECT: u32 = WM_USER + 26;

/// The ink slider (`InkSliderView`): a 1 pt track in `track`, a tick at the
/// default, a 2 pt pencil fill from the default to the knob once changed,
/// and a 9 pt knob that turns `paper` and grows while the row is pointed at.
/// The trackbar still places the knob and takes the mouse and the keys.
pub fn paint_trackbar(draw: &NMCUSTOMDRAW) {
    let hwnd = draw.hdr.hwndFrom;
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut channel = RECT::default();
        let mut thumb = RECT::default();
        SendMessageW(hwnd, TBM_GETCHANNELRECT, WPARAM(0), LPARAM(&mut channel as *mut _ as isize));
        SendMessageW(hwnd, TBM_GETTHUMBRECT, WPARAM(0), LPARAM(&mut thumb as *mut _ as isize));
        let min = SendMessageW(hwnd, TBM_GETRANGEMIN, WPARAM(0), LPARAM(0)).0;
        let max = SendMessageW(hwnd, TBM_GETRANGEMAX, WPARAM(0), LPARAM(0)).0;
        let default = draw::default_pos(hwnd).unwrap_or(min).clamp(min, max);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let dragging = GetCapture() == hwnd;
        let hot = enabled && (row_hot(hwnd) || dragging);
        let focused = GetFocus() == hwnd && draw::focus_cues(hwnd);
        let changed = is_changed(hwnd);
        let mut c = Canvas::new(draw.hdc, client);
        c.clear(theme::gdi(t.hex.panel));
        let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
        // In DIPs: where the control puts the knob's centre at either end.
        let half = (thumb.right - thumb.left) as f32 / 2.0 / s;
        let (left, right) = (channel.left as f32 / s + half, channel.right as f32 / s - half);
        let knob_x = (thumb.left + thumb.right) as f32 / 2.0 / s;
        let span = (max - min).max(1) as f32;
        let default_x = left + (right - left) * (default - min) as f32 / span;
        let alpha = if enabled { 1.0 } else { theme::DISABLED_OPACITY };
        if !focused {
            // Without the focus box it's all rectangles and circles: the
            // canvas draws them, without a Direct2D pass (0.4 ms a slider
            // when the column first shows).
            let ink = |hex: u32| theme::mix(theme::gdi(t.hex.panel), theme::gdi(hex), alpha as f64);
            let px = |v: f32| v * s;
            let cy = (h / 2.0).floor();
            c.round_rect(draw::RectF::new(px(left - 5.0), px(cy), px(right - left + 10.0), px(1.0).max(1.0).round()), 0.0, ink(t.hex.track));
            c.round_rect(draw::RectF::new(px(default_x.round() - 0.5), px(cy - 4.0), px(1.0).max(1.0).round(), px(9.0)), 0.0, ink(t.hex.dim));
            if changed {
                c.round_rect(draw::RectF::new(px(default_x.min(knob_x)), px(cy - 0.5), px((knob_x - default_x).abs()), px(2.0)), 0.0, ink(t.hex.accent));
            }
            let radius = if hot { 4.5 * 1.2 } else { 4.5 };
            let stroke = if dragging { t.hex.paper } else if changed { t.hex.accent } else if hot { t.hex.paper } else { t.hex.label };
            let fill = if dragging { t.hex.paper } else if changed { t.hex.accent } else { t.hex.panel };
            c.circle(px(knob_x), px(cy + 0.5), px(radius), ink(fill));
            c.ring(px(knob_x), px(cy + 0.5), px(radius + 0.75), px(1.5), ink(stroke));
            c.present(draw.hdc);
            return;
        }
        c.d2d(s, t.plain, |p| {
            let cy = (h / 2.0).floor();
            p.fill_rect(RectF::new(left - 5.0, cy, right - left + 10.0, 1.0), Ink::hex(t.hex.track).alpha(alpha));
            p.fill_rect(RectF::new(default_x.round() - 0.5, cy - 4.0, 1.0, 9.0), Ink::hex(t.hex.dim).alpha(alpha));
            if changed {
                p.fill_rect(RectF::new(default_x.min(knob_x), cy - 0.5, (knob_x - default_x).abs(), 2.0), Ink::hex(t.hex.accent).alpha(alpha));
            }
            let radius = if hot { 4.5 * 1.2 } else { 4.5 };
            let stroke = if dragging { t.hex.paper } else if changed { t.hex.accent } else if hot { t.hex.paper } else { t.hex.label };
            let fill = if dragging { t.hex.paper } else if changed { t.hex.accent } else { t.hex.panel };
            p.fill_ellipse(knob_x, cy + 0.5, radius, radius, Ink::hex(fill).alpha(alpha));
            p.stroke_round_rect(RectF::new(knob_x - radius, cy + 0.5 - radius, 2.0 * radius, 2.0 * radius), radius, 1.5, Ink::hex(stroke).alpha(alpha));
            if focused {
                focus_box(p, RectF::new(0.0, (cy - 11.0).max(0.0), w, 22.0f32.min(h)), &t);
            }
        });
        c.present(draw.hdc);
    }
}

// MARK: Value boxes

/// A slider's value (`ValueText`): the number in the serif, 700 in `paper`
/// with the pencil under it once changed, else `dim`. A field only while it
/// has the keys, with a `paper` rule under it. Subclasses the EDIT.
pub fn value_box(edit: HWND) {
    draw::set_role(edit, Role::InkValue);
    unsafe {
        let _ = SetWindowSubclass(edit, Some(value_subclass), 0x4e56, 0);
    }
}

extern "system" fn value_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        let focused = GetFocus() == hwnd;
        match msg {
            WM_PAINT if !focused => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_value(hwnd, dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PAINT => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                // The rule under the field while typing.
                let dc = GetDC(hwnd);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let height = draw::dpi_of(hwnd).max(1.0).floor() as i32;
                let rule = RECT { top: client.bottom - height, ..client };
                FillRect(dc, &rule, theme::brush(theme::gdi(theme::tokens().hex.paper)));
                ReleaseDC(hwnd, dc);
                result
            }
            WM_ERASEBKGND if !focused => LRESULT(1),
            // The control draws new text at once, outside `WM_PAINT`:
            // unfocused, it paints in ink instead.
            WM_SETTEXT if !focused => {
                SendMessageW(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                SendMessageW(hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));
                let _ = InvalidateRect(hwnd, None, false);
                result
            }
            WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                let _ = InvalidateRect(hwnd, None, true);
                result
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn paint_value(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let enabled = unsafe { IsWindowEnabled(hwnd) }.as_bool();
    let changed = is_changed(hwnd);
    let hot = enabled && row_hot(hwnd);
    let mut c = Canvas::new(dc, client);
    c.clear(theme::gdi(t.hex.panel));
    let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
    let alpha = if enabled { 1.0 } else { theme::DISABLED_OPACITY };
    with_text(hwnd, |text| {
        c.d2d(s, t.plain, |p| {
            let style = if changed { VALUE_CHANGED } else { VALUE };
            let metrics = p.measure(text, &style, 0.0);
            let x = (w - metrics.width).round();
            let y = (h - 8.0 - metrics.baseline).round();
            if changed {
                p.underline(RectF::new(x, y + metrics.baseline + 2.0, metrics.width, 5.0), &t, false);
            } else if hot {
                p.fill_rect(RectF::new(x, y + metrics.baseline + 2.0, metrics.width, 1.0), Ink::hex(t.hex.dim).alpha(alpha));
            }
            p.text(text, &style, x, y, Ink::hex(if changed { t.hex.paper } else { t.hex.dim }).alpha(alpha));
        });
    });
    c.present(dc);
}

/// `WM_CTLCOLOREDIT` for a value box while typing: `paper` on the panel.
pub fn color_value(dc: HDC) -> LRESULT {
    let t = theme::tokens();
    unsafe {
        SetTextColor(dc, COLORREF(theme::gdi(t.hex.paper)));
        SetBkColor(dc, COLORREF(theme::gdi(t.hex.panel)));
        LRESULT(theme::brush(theme::gdi(t.hex.panel)).0 as isize)
    }
}

// MARK: Choices

/// A choice's closed box (`ChoiceRow`): the value in the serif at the
/// row's end, 700 with the pencil once changed, and a `⌄`.
pub fn paint_choice(hwnd: HWND, dc: HDC) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let hot = enabled && row_hot(hwnd);
        let focused = GetFocus() == hwnd && draw::focus_cues(hwnd);
        let changed = is_changed(hwnd);
        let mut c = Canvas::new(dc, client);
        c.clear(theme::gdi(t.hex.panel));
        let (w, h) = (client.right as f32 / s, client.bottom as f32 / s);
        let alpha = if enabled { 1.0 } else { theme::DISABLED_OPACITY };
        let m = MARGIN as f32;
        with_choice(hwnd, |text| {
            c.d2d(s, t.plain, |p| {
                let style = if changed { VALUE_CHANGED } else { VALUE };
                let metrics = p.measure(text, &style, 0.0);
                let caret_x = w - m - 7.0;
                let x = (caret_x - 6.0 - metrics.width).max(m).round();
                let y = centred(h, &metrics, style.size);
                if changed {
                    p.underline(RectF::new(x, y + metrics.baseline + 2.0, metrics.width, 5.0), &t, false);
                }
                p.clipped(RectF::new(m, 0.0, (caret_x - 6.0 - m).max(0.0), h), |p| {
                    p.text(text, &style, x, y, Ink::hex(t.hex.paper).alpha(alpha));
                });
                caret(p, caret_x, y + metrics.baseline - 5.5, Ink::hex(if hot { t.hex.paper } else { t.hex.dim }).alpha(alpha));
                if focused {
                    focus_box(p, RectF::new(0.0, 0.0, w, h), &t);
                }
            });
        });
        c.present(dc);
    }
}

/// `f` with a drop-down list's chosen item.
fn with_choice<R>(hwnd: HWND, f: impl FnOnce(&str) -> R) -> R {
    TEXT.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => {
            let (buffer, text) = &mut *guard;
            text.clear();
            unsafe {
                let selected = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                if selected >= 0 {
                    let length = SendMessageW(hwnd, CB_GETLBTEXTLEN, WPARAM(selected as usize), LPARAM(0)).0.max(0) as usize;
                    if length < buffer.len() {
                        SendMessageW(hwnd, CB_GETLBTEXT, WPARAM(selected as usize), LPARAM(buffer.as_mut_ptr() as isize));
                        text.extend(char::decode_utf16(buffer[..length].iter().copied()).map(|c| c.unwrap_or('\u{fffd}')));
                    }
                }
            }
            f(text)
        }
        Err(_) => f(""),
    })
}

/// Opens a choice's menu under its value, the current item ticked; a pick
/// selects it and tells the parent, as the list would (`CBN_SELCHANGE`).
pub fn open_choice(hwnd: HWND) {
    unsafe {
        if !IsWindowEnabled(hwnd).as_bool() {
            return;
        }
        let _ = SetFocus(hwnd);
        let count = SendMessageW(hwnd, CB_GETCOUNT, WPARAM(0), LPARAM(0)).0.max(0) as usize;
        let selected = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let Ok(menu) = CreatePopupMenu() else { return };
        for i in 0..count {
            let length = SendMessageW(hwnd, CB_GETLBTEXTLEN, WPARAM(i), LPARAM(0)).0.max(0) as usize;
            let mut text = vec![0u16; length + 1];
            SendMessageW(hwnd, CB_GETLBTEXT, WPARAM(i), LPARAM(text.as_mut_ptr() as isize));
            let flags = if i as isize == selected { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(menu, flags, i + 1, windows::core::PCWSTR(text.as_ptr()));
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTALIGN | TPM_TOPALIGN, r.right, r.bottom, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        let index = chosen.0 as isize - 1;
        if index >= 0 && index != selected {
            SendMessageW(hwnd, CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
            if let Ok(parent) = GetParent(hwnd) {
                let id = GetDlgCtrlID(hwnd) as usize & 0xffff;
                SendMessageW(parent, WM_COMMAND, WPARAM(((CBN_SELCHANGE as usize) << 16) | id), LPARAM(hwnd.0 as isize));
            }
        }
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// Whether a key opens a choice's menu: F4, Alt+↓ or ↑, Space or Return.
pub fn opens_choice(msg: u32, key: VIRTUAL_KEY) -> bool {
    match msg {
        WM_KEYDOWN => matches!(key, VK_F4 | VK_SPACE | VK_RETURN),
        WM_SYSKEYDOWN => matches!(key, VK_DOWN | VK_UP),
        _ => false,
    }
}
