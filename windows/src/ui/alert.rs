//! The alert card (IMPLEMENTATION.md §4.14, round 18; `AlertSheet.swift`):
//! for errors, warnings and questions that aren't about the photograph on
//! screen. A card in the theme's `panel` with a 1 DIP `line` border, every
//! corner rounded 10, hangs 12 DIP under the title bar over the window,
//! which dims below the band. The heading is Newsreader 20 600 `paper`, the
//! detail italic 16 `label`, and the buttons outlined at the right, the
//! default one (Enter) at the far right with ↵ after its word. It replaces
//! the task dialogs; Windows' own open, save and colour dialogs stay.
//!
//! One layered popup window over the window's client area below the title
//! bar, drawn with Direct2D into premultiplied pixels: the dimming, the
//! card's shadow and the card. It is modal, like the task dialog it
//! replaces: `ask` returns the button chosen. Enter presses the focused
//! button (the default one until Tab or an arrow moves the focus, which then
//! shows the cream focus box), Esc the Cancel one, and Tab, Shift+Tab, Left
//! and Right move between them. The window class is registered on first use.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{self, Align, Ink, Painter, RectF, TextStyle};
use super::theme::{self, Tokens};
use crate::platform::{pcwstr, wide};

/// What an alert says and offers (`AppAlert`).
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub title: String,
    pub detail: Option<String>,
    /// The first is the default (Enter) and sits at the right; the rest
    /// follow it leftwards.
    pub buttons: Vec<String>,
    /// Esc's button. None: Esc does nothing, unless there's only one.
    pub cancel: Option<usize>,
}

impl Alert {
    pub fn new(title: impl Into<String>, detail: Option<String>) -> Self {
        Self { title: title.into(), detail, buttons: vec!["OK".into()], cancel: Some(0) }
    }

    pub fn buttons(mut self, buttons: &[&str], cancel: Option<usize>) -> Self {
        self.buttons = buttons.iter().map(|b| b.to_string()).collect();
        self.cancel = cancel;
        self
    }

    /// One of the app's error messages: its first sentence is the heading,
    /// the rest the detail ("Could not save the editing session. Negs
    /// stayed open." reads "Could not save the editing session" over "Negs
    /// stayed open.").
    pub fn error(message: &str) -> Self {
        let text = message.trim();
        if let Some(end) = text.find(". ") {
            let rest = text[end + 2..].trim();
            if !rest.is_empty() {
                return Self::new(&text[..end], Some(rest.to_string()));
            }
        }
        Self::new(text, None)
    }

    fn cancel_index(&self) -> Option<usize> {
        self.cancel.or((self.buttons.len() == 1).then_some(0))
    }
}

/// An error, with an exporter's log after its first line offered to copy.
pub fn error(owner: HWND, message: &str) {
    let (first, log) = message.split_once('\n').unwrap_or((message, ""));
    let log = log.trim();
    let mut alert = Alert::error(first);
    if !log.is_empty() {
        alert = alert.buttons(&["OK", "Copy Details"], Some(0));
    }
    if ask(owner, alert) == 1 {
        super::copy_text(owner, log);
    }
}

/// A short message with an explanation below it.
pub fn notice(owner: HWND, title: &str, detail: &str) {
    ask(owner, Alert::new(title, (!detail.trim().is_empty()).then(|| detail.to_string())));
}

// MARK: The card's measures, in DIPs

const WIDTH: f32 = 440.0;
/// Between the title bar and the card.
const GAP: f32 = 12.0;
const RADIUS: f32 = 10.0;
const PAD_TOP: f32 = 22.0;
const PAD_SIDE: f32 = 26.0;
const PAD_BOTTOM: f32 = 20.0;
const BUTTON_HEIGHT: f32 = 23.0;
const BUTTON_PAD: f32 = 12.0;
const BUTTON_GAP: f32 = 10.0;
const BUTTON_MIN: f32 = 52.0;
const TITLE: TextStyle = TextStyle::serif(20.0, 600).line_height(1.35);
const DETAIL: TextStyle = TextStyle::serif(16.0, 400).italic().line_height(1.5);
const BUTTON: TextStyle = TextStyle::serif(13.5, 500);
const DEFAULT_BUTTON: TextStyle = TextStyle::serif(13.5, 650);
const ENTER_MARK: TextStyle = TextStyle::ui(10.5, 400);

struct State {
    alert: Alert,
    main: HWND,
    /// The focused button, and whether the keyboard put it there (the focus box shows).
    focus: usize,
    keyboard: bool,
    hot: Option<usize>,
    pressed: Option<usize>,
    answer: Option<usize>,
    /// Where the buttons were drawn, in DIPs.
    buttons: Vec<RectF>,
    /// 0 → 1 as the card comes down.
    shown: f32,
    started: std::time::Instant,
    still: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

const TIMER: usize = 1;
const SLIDE: f32 = 0.2;

fn register() {
    if REGISTERED.with(|r| r.replace(true)) {
        return;
    }
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: super::instance(),
            lpszClassName: w!("NegsAlert"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
}

/// Where the cover goes: the owner's client area below the title bar, on screen.
fn cover_rect(main: HWND) -> RECT {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(main, &mut client);
        let mut corner = POINT { x: 0, y: super::titlebar::height(main) };
        let _ = ClientToScreen(main, &mut corner);
        RECT { left: corner.x, top: corner.y, right: corner.x + client.right, bottom: corner.y + (client.bottom - super::titlebar::height(main)).max(1) }
    }
}

/// Shows `alert` over `owner` and waits for an answer: the index of the
/// button chosen (Esc's, if the window closes first). Another alert asked
/// meanwhile waits its turn.
pub fn ask(owner: HWND, alert: Alert) -> usize {
    if STATE.with(|s| s.borrow().is_some()) {
        // One at a time: this one after the one showing.
        let mut msg = MSG::default();
        while STATE.with(|s| s.borrow().is_some()) && unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    register();
    let fallback = alert.cancel_index().unwrap_or(0);
    let cover = cover_rect(owner);
    let still = !client_animations();
    let words = match &alert.detail {
        Some(detail) => format!("{}. {detail}", alert.title.trim_end_matches('.')),
        None => alert.title.clone(),
    };
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            alert,
            main: owner,
            focus: 0,
            keyboard: false,
            hot: None,
            pressed: None,
            answer: None,
            buttons: vec![],
            shown: if still { 1.0 } else { 0.0 },
            started: std::time::Instant::now(),
            still,
        })
    });
    let title = wide(words);
    unsafe {
        let Ok(popup) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            w!("NegsAlert"),
            pcwstr(&title),
            WS_POPUP,
            cover.left,
            cover.top,
            cover.right - cover.left,
            cover.bottom - cover.top,
            owner,
            None,
            super::instance(),
            None,
        ) else {
            STATE.with(|s| s.borrow_mut().take());
            return fallback;
        };
        render(popup);
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(popup, SW_SHOW);
        let _ = SetForegroundWindow(popup);
        let _ = SetFocus(popup);
        if !still {
            SetTimer(popup, TIMER, 15, None);
        }
        let mut msg = MSG::default();
        loop {
            if STATE.with(|s| s.borrow().as_ref().is_none_or(|s| s.answer.is_some())) {
                break;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // Quitting: let the outer loop see it too.
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // The owner first, so it keeps the activation.
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(popup);
        let _ = SetForegroundWindow(owner);
    }
    STATE.with(|s| s.borrow_mut().take()).and_then(|s| s.answer).unwrap_or(fallback)
}

/// Windows' "Show animations in Windows" setting.
fn client_animations() -> bool {
    let mut on = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    on.as_bool()
}

/// The card's size for `alert`, its parts measured: (height, the title's
/// and detail's heights, the buttons' widths).
fn measure(p: &mut Painter, alert: &Alert) -> (f32, f32, f32, Vec<f32>) {
    let inner = WIDTH - 2.0 * PAD_SIDE;
    let title = p.measure(&alert.title, &TITLE, inner).height;
    let detail = alert.detail.as_ref().map_or(0.0, |d| 8.0 + p.measure(d, &DETAIL, inner).height);
    let widths = alert
        .buttons
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let words = p.measure(b, if i == 0 { &DEFAULT_BUTTON } else { &BUTTON }, 0.0).width;
            let mark = if i == 0 { 5.0 + p.measure("↵", &ENTER_MARK, 0.0).width } else { 0.0 };
            (words + mark).max(BUTTON_MIN) + 2.0 * BUTTON_PAD
        })
        .collect();
    (PAD_TOP + title + detail + 20.0 + BUTTON_HEIGHT + PAD_BOTTOM, title, detail, widths)
}

/// Draws the cover into premultiplied pixels and hands them to the window.
fn render(popup: HWND) {
    let Some(main) = STATE.with(|s| s.borrow().as_ref().map(|s| s.main)) else { return };
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(popup, &mut r);
    }
    let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
    let scale = unsafe { GetDpiForWindow(main) }.max(96) as f32 / 96.0;
    let t = theme::tokens();
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
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
        let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            return;
        };
        let old = SelectObject(dc, bitmap);
        // Starts transparent: a fresh DIB section is zeroed.
        STATE.with(|cell| {
            let mut cell = cell.borrow_mut();
            let Some(state) = cell.as_mut() else { return };
            d2d::paint_layer(dc, w, h, scale, t.plain, |p| draw(p, state, &t, w as f32 / scale, h as f32 / scale));
        });
        let _ = GdiFlush();
        let size = SIZE { cx: w, cy: h };
        let source = POINT::default();
        let at = POINT { x: r.left, y: r.top };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let _ = UpdateLayeredWindow(popup, screen, Some(&at), Some(&size), dc, Some(&source), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn draw(p: &mut Painter, state: &mut State, t: &Tokens, w: f32, h: f32) {
    let shown = ease_out(state.shown);
    let (dim, dim_alpha) = t.dimming();
    p.fill_rect(RectF::new(0.0, 0.0, w, h), Ink::hex(dim).alpha(dim_alpha * shown));
    let (card_h, title_h, detail_h, widths) = measure(p, &state.alert);
    let x = ((w - WIDTH) / 2.0).round();
    // Down from under the title bar, or there at once without animations.
    let y = if state.still { GAP } else { GAP - (1.0 - shown) * (card_h + GAP) };
    let card = RectF::new(x, y.round(), WIDTH, card_h);
    // A deep shadow: soft rings spreading down and out.
    let strength = t.shadow_strength();
    for i in 0..14 {
        let spread = i as f32 * 2.0;
        let alpha = 0.7 * strength * shown * 0.045 * (1.0 - i as f32 / 14.0);
        p.fill_round_rect(RectF::new(card.x - spread, card.y + 18.0 * 0.6 - spread + i as f32, card.w + 2.0 * spread, card.h + 2.0 * spread), RADIUS + spread, Ink::hex(0).alpha(alpha));
    }
    p.fill_round_rect(RectF::new(card.x - 1.0, card.y + 2.0, card.w + 2.0, card.h + 2.0), RADIUS + 1.0, Ink::hex(0).alpha(0.5 * strength * shown * 0.6));
    p.fill_round_rect(card, RADIUS, Ink::hex(t.hex.panel));
    p.stroke_round_rect(RectF::new(card.x + 0.5, card.y + 0.5, card.w - 1.0, card.h - 1.0), RADIUS - 0.5, 1.0, Ink::hex(t.hex.line));
    let inner = WIDTH - 2.0 * PAD_SIDE;
    let mut top = card.y + PAD_TOP;
    p.text_in(&state.alert.title, &TITLE, RectF::new(card.x + PAD_SIDE, top, inner, 0.0), Align::Leading, Ink::hex(t.hex.paper));
    top += title_h;
    if let Some(detail) = &state.alert.detail {
        p.text_in(detail, &DETAIL, RectF::new(card.x + PAD_SIDE, top + 8.0, inner, 0.0), Align::Leading, Ink::hex(t.hex.label));
    }
    top += detail_h + 20.0;
    // The buttons, from the right.
    let mut right = card.x + card.w - PAD_SIDE;
    let mut places = vec![RectF::new(0.0, 0.0, 0.0, 0.0); widths.len()];
    for (i, width) in widths.iter().enumerate() {
        let r = RectF::new(right - width, top, *width, BUTTON_HEIGHT);
        places[i] = r;
        right = r.x - BUTTON_GAP;
        let default = i == 0;
        let hot = state.hot == Some(i);
        let pressed = state.pressed == Some(i) && hot;
        let radius = BUTTON_HEIGHT / 2.0;
        if pressed || hot {
            p.fill_round_rect(r, radius, Ink::hex(t.hex.paper).alpha(if pressed { 0.09 } else { 0.04 }));
        }
        let ring = match (hot || pressed, default) {
            (true, true) => t.hex.label,
            (true, false) | (false, true) => t.hex.dim,
            (false, false) => t.hex.track,
        };
        p.stroke_round_rect(RectF::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), radius - 0.5, 1.0, Ink::hex(ring));
        let style = if default { &DEFAULT_BUTTON } else { &BUTTON };
        let words = p.measure(&state.alert.buttons[i], style, 0.0);
        let mark = if default { Some(p.measure("↵", &ENTER_MARK, 0.0)) } else { None };
        let total = words.width + mark.map_or(0.0, |m| 5.0 + m.width);
        let left = r.x + (r.w - total) / 2.0;
        // Newsreader's tall ascent: centred by eye, a little low.
        let baseline = r.y + r.h / 2.0 + style.size * 0.36;
        p.text(&state.alert.buttons[i], style, left, baseline - words.baseline, Ink::hex(t.hex.paper));
        if let Some(m) = mark {
            p.text("↵", &ENTER_MARK, left + words.width + 5.0, baseline - m.baseline, Ink::hex(t.hex.dim));
        }
        if state.keyboard && state.focus == i {
            // The cream focus box: 1.5 `paper`, 5 out, radius 3 (§4.13).
            p.stroke_round_rect(RectF::new(r.x - 5.0, r.y - 5.0, r.w + 10.0, r.h + 10.0), 3.0, 1.5, Ink::hex(t.hex.paper));
        }
    }
    state.buttons = places;
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

fn answer(popup: HWND, button: usize) {
    with_state(|s| s.answer = Some(button));
    // Wakes the loop in `ask`.
    unsafe {
        let _ = PostMessageW(popup, WM_NULL, WPARAM(0), LPARAM(0));
    }
}

fn button_at(popup: HWND, lparam: LPARAM) -> Option<usize> {
    let main = with_state(|s| s.main)?;
    let scale = unsafe { GetDpiForWindow(main) }.max(96) as f32 / 96.0;
    let (x, y) = ((lparam.0 & 0xffff) as i16 as f32 / scale, ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale);
    let _ = popup;
    with_state(|s| s.buttons.iter().position(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)).flatten()
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER if wparam.0 == TIMER => {
                let done = with_state(|s| {
                    s.shown = (s.started.elapsed().as_secs_f32() / SLIDE).min(1.0);
                    s.shown >= 1.0
                })
                .unwrap_or(true);
                if done {
                    let _ = KillTimer(hwnd, TIMER);
                }
                render(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let hot = button_at(hwnd, lparam);
                if with_state(|s| std::mem::replace(&mut s.hot, hot) != hot).unwrap_or(false) {
                    render(hwnd);
                }
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                if with_state(|s| s.hot.take().is_some()).unwrap_or(false) {
                    render(hwnd);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let at = button_at(hwnd, lparam);
                with_state(|s| {
                    s.pressed = at;
                    s.hot = at;
                    s.keyboard = false;
                });
                // Off the buttons: nothing; the window behind waits for an answer.
                if at.is_some() {
                    SetCapture(hwnd);
                }
                render(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let at = button_at(hwnd, lparam);
                let pressed = with_state(|s| s.pressed.take()).flatten();
                render(hwnd);
                if let Some(button) = pressed.filter(|&b| Some(b) == at) {
                    answer(hwnd, button);
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let key = VIRTUAL_KEY(wparam.0 as u16);
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let count = with_state(|s| s.alert.buttons.len()).unwrap_or(1).max(1);
                match key {
                    VK_RETURN | VK_SPACE => {
                        if let Some(focus) = with_state(|s| s.focus) {
                            answer(hwnd, focus);
                        }
                    }
                    VK_ESCAPE => {
                        if let Some(cancel) = with_state(|s| s.alert.cancel_index()).flatten() {
                            answer(hwnd, cancel);
                        }
                    }
                    // Buttons run right to left: Tab and Left go leftwards.
                    VK_TAB | VK_LEFT | VK_RIGHT => {
                        let back = (key == VK_TAB && shift) || key == VK_RIGHT;
                        with_state(|s| {
                            s.focus = if back { (s.focus + count - 1) % count } else { (s.focus + 1) % count };
                            s.keyboard = true;
                        });
                        render(hwnd);
                    }
                    _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                }
                LRESULT(0)
            }
            WM_SYSCOMMAND if (wparam.0 & 0xfff0) as u32 == SC_CLOSE => {
                // Alt+F4: Esc's button, if there is one.
                if let Some(cancel) = with_state(|s| s.alert.cancel_index()).flatten() {
                    answer(hwnd, cancel);
                }
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
            WM_SETCURSOR => {
                let _ = SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An error's first sentence heads it, as on the Mac.
    #[test]
    fn errors_split_at_their_first_sentence() {
        let a = Alert::error("Could not save the editing session. Negs stayed open.");
        assert_eq!(a.title, "Could not save the editing session");
        assert_eq!(a.detail.as_deref(), Some("Negs stayed open."));
        let b = Alert::error("“x.NEF” can’t be opened.");
        assert_eq!(b.title, "“x.NEF” can’t be opened.");
        assert_eq!(b.detail, None);
    }

    /// Esc answers the Cancel button, or the only one.
    #[test]
    fn escape_takes_cancel_or_the_only_button() {
        assert_eq!(Alert::new("t", None).cancel_index(), Some(0));
        assert_eq!(Alert::new("t", None).buttons(&["Quit", "Cancel"], Some(1)).cancel_index(), Some(1));
        assert_eq!(Alert::new("t", None).buttons(&["A", "B"], None).cancel_index(), None);
    }
}
