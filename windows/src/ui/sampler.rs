//! `NEGS_DRAW_SAMPLER=<theme>`: instead of the app, a window that draws
//! every primitive of the drawing foundation in one theme, to compare by
//! eye with the macOS app and the mockup (design/proposals/final.html): the
//! type ramp of IMPLEMENTATION.md §2.2, the pencil under words of several
//! widths, rings, stars, the reject's crosses, the pick's boxes and every
//! token. Without the variable it costs one environment lookup.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmFlush, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Align, BoxInk, Ink, Painter, RectF, TextStyle};
use super::draw::{self, Canvas};
use super::theme::{self, ThemeName, Tokens};
use crate::platform;

thread_local! {
    static THEME: std::cell::Cell<ThemeName> = const { std::cell::Cell::new(ThemeName::Darkroom) };
}

/// Runs the sampler if `NEGS_DRAW_SAMPLER` names a theme; true if it ran.
pub fn run_if_asked() -> bool {
    let Some(name) = std::env::var_os("NEGS_DRAW_SAMPLER") else { return false };
    let theme = ThemeName::parse(&name.to_string_lossy()).unwrap_or(ThemeName::Darkroom).resolved();
    THEME.with(|t| t.set(theme));
    let report = std::env::var_os("NEGS_STARTUP_REPORT");
    let stamp = |line: String| {
        if let Some(path) = &report {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{line}");
            }
        }
    };
    stamp(format!("frequency {}", platform::qpc_frequency()));
    stamp(format!("main {}", platform::qpc()));
    unsafe {
        let instance = super::instance();
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("NegsDrawSampler"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let title = platform::wide(format!("Negs drawing sampler: {}", theme.title()));
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("NegsDrawSampler"),
            platform::pcwstr(&title),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1440,
            900,
            None,
            None,
            instance,
            None,
        ) else {
            return true;
        };
        let dark = BOOL(theme.tokens().dark as i32);
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const _ as *const _, 4);
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
        let _ = DwmFlush();
        stamp(format!("first_window_composited {}", platform::qpc()));
        if let Some(shared) = super::d2d::shared() {
            let c = shared.cost;
            stamp(format!("drawing_cost d2d_us {} dwrite_us {} fonts_us {}", c.d2d.as_micros(), c.dwrite.as_micros(), c.fonts.as_micros()));
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let theme = THEME.with(|t| t.get());
                let tokens = theme.tokens();
                let scale = draw::dpi_of(hwnd);
                let mut canvas = Canvas::new(dc, client);
                // GDI first, Direct2D over it: the two mix in one buffer.
                canvas.clear(theme::gdi(tokens.hex.bg));
                let drawn = canvas.d2d(scale, tokens.plain, |p| paint(p, theme, tokens, client.right as f32 / scale));
                if !drawn {
                    let font = HFONT(GetStockObject(DEFAULT_GUI_FONT).0);
                    canvas.text("Direct2D isn't available.", RECT { left: 20, top: 60, right: 600, bottom: 90 }, font, theme::gdi(tokens.hex.warn), DT_LEFT);
                }
                canvas.present(dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// A photograph's stand-in: a mid grey, warm, as the sheets show one.
const PICTURE: u32 = 0x8a8478;

fn paint(p: &mut Painter, theme: ThemeName, t: &Tokens, width: f32) {
    let h = &t.hex;
    let hex = Ink::hex;
    let caption = TextStyle::ui(10.5, 400);

    // The title band, with what the fonts are on this Windows.
    p.fill_rect(RectF::new(0.0, 0.0, width, 38.0), hex(h.bg));
    let m = p.text(theme.title(), &TextStyle::serif(15.0, 600), 18.0, 9.0, hex(h.paper));
    let hint = TextStyle::serif(13.0, 400).italic();
    p.text(theme.hint(), &hint, 18.0 + m.width + 14.0, 11.0, hex(h.dim));
    let about = about_fonts(p.plain());
    p.text_in(&about, &caption, RectF::new(width - 760.0, 13.0, 742.0, 0.0), Align::Trailing, hex(h.dim));
    p.line((0.0, 38.5), (width, 38.5), 1.0, hex(h.line));

    // Column 1: the type ramp, on the panel as in the inspector.
    let (x, w) = (0.0, 480.0);
    p.fill_rect(RectF::new(x, 39.0, w, 2000.0), hex(h.panel));
    p.fill_rect(RectF::new(x + w, 39.0, 1.0, 2000.0), hex(h.line));
    let x = x + 22.0;
    let mut y = 52.0;
    let row = |p: &mut Painter, y: &mut f32, name: &str, draw: &mut dyn FnMut(&mut Painter, f32) -> f32| {
        let used = draw(p, *y);
        p.text(name, &caption, x, *y + used + 2.0, hex(h.dim));
        *y += used + 24.0;
    };
    row(p, &mut y, "Film name · Newsreader 24, 600, tracking −0.01 em, pencil under", &mut |p, y| {
        let style = TextStyle::serif(24.0, 600).tracking(-0.01);
        let m = p.measure("Kodak Portra 400", &style, 0.0);
        p.underline(RectF::new(x, y + m.height - 1.0, m.width, 6.0), t, false);
        p.text("Kodak Portra 400", &style, x, y, hex(h.paper));
        m.height + 6.0
    });
    row(p, &mut y, "“printed on” · 14 italic dim; paper name · 17.5, 400", &mut |p, y| {
        let paper = TextStyle::serif(17.5, 400);
        let base = p.measure("Ilford", &paper, 0.0).baseline;
        let small = TextStyle::serif(14.0, 400).italic();
        let lift = base - p.measure("printed on", &small, 0.0).baseline;
        let m = p.text("printed on", &small, x, y + lift, hex(h.dim));
        p.text("Ilford Multigrade IV RC Deluxe", &paper, x + m.width + 6.0, y, hex(h.paper)).height
    });
    row(p, &mut y, "Caption · 15 italic label, file name upright dim", &mut |p, y| {
        let style = TextStyle::serif(15.0, 400).italic();
        let m = p.text("Nikon D750 · 50 mm · f/2 · 1/250 s · ISO 400", &style, x, y, hex(h.label));
        p.text("_NHA9786.NEF", &TextStyle::serif(15.0, 400), x + m.width + 10.0, y, hex(h.dim)).height
    });
    row(p, &mut y, "Title bar · 15, 600 paper", &mut |p, y| p.text("_NHA9786.NEF", &TextStyle::serif(15.0, 600), x, y, hex(h.paper)).height);
    row(p, &mut y, "Tabs · 21, 500 dim; chosen 700 paper, pencil under", &mut |p, y| {
        let mut at = x;
        let mut tall: f32 = 0.0;
        for (name, chosen) in [("Prep", false), ("Lab", true), ("Output", false)] {
            let style = TextStyle::serif(21.0, if chosen { 700 } else { 500 });
            let m = p.measure(name, &style, 0.0);
            if chosen {
                p.underline(RectF::new(at, y + m.height, m.width, 6.0), t, false);
            }
            p.text(name, &style, at, y, hex(if chosen { h.paper } else { h.dim }));
            at += m.width + 22.0;
            tall = tall.max(m.height + 6.0);
        }
        tall
    });
    row(p, &mut y, "Section head · 11, 700, caps, tracking 0.13 em, label", &mut |p, y| {
        p.text("DEVELOPING AND PRINTING", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label)).height
    });
    row(p, &mut y, "Module title · 16, 600 paper; off: 400 dim", &mut |p, y| {
        let m = p.text("Grain", &TextStyle::serif(16.0, 600), x, y, hex(h.paper));
        p.text("Halation", &TextStyle::serif(16.0, 400), x + m.width + 24.0, y, hex(h.dim)).height
    });
    row(p, &mut y, "Value · 14.5 dim; changed 700 paper, pencil under (5 pt)", &mut |p, y| {
        let style = TextStyle::serif(14.5, 400);
        let m = p.text("0.35", &style, x, y, hex(h.dim));
        let bold = TextStyle::serif(14.5, 700);
        let n = p.measure("+1.20", &bold, 0.0);
        let at = x + m.width + 24.0;
        p.underline(RectF::new(at, y + n.height, n.width, 5.0), t, false);
        p.text("+1.20", &bold, at, y, hex(h.paper));
        n.height + 5.0
    });
    row(p, &mut y, "Hint · 13 italic dim, line height 1.25", &mut |p, y| {
        let style = TextStyle::serif(13.0, 400).italic().line_height(1.25);
        p.text_in("Grain follows the film’s own size, so a 35 mm frame shows more of it than a 6×7 one printed the same size.", &style, RectF::new(x, y, 330.0, 0.0), Align::Leading, hex(h.dim)).height
    });
    row(p, &mut y, "Control label · system font 12 label; changed paper", &mut |p, y| {
        let style = TextStyle::ui(12.0, 400);
        let m = p.text("Exposure", &style, x, y, hex(h.label));
        p.text("Developer exhaustion", &style, x + m.width + 24.0, y, hex(h.paper)).height
    });
    row(p, &mut y, "Status line · system font 11 dim; the word that's on, paper", &mut |p, y| {
        let style = TextStyle::ui(11.0, 400);
        let m = p.text("6016 × 4016 · developed in 1.2 s · ", &style, x, y, hex(h.dim));
        let n = p.text("Fit", &style, x + m.width, y, hex(h.paper));
        p.text("  100 %", &style, x + m.width + n.width, y, hex(h.dim)).height
    });
    row(p, &mut y, "Export button · 18, 700 on fill", &mut |p, y| {
        p.fill_round_rect(RectF::new(x, y, 200.0, 40.0), 6.0, hex(h.fill));
        p.text_in("Export", &TextStyle::serif(18.0, 700), RectF::new(x, y + 9.0, 200.0, 0.0), Align::Center, hex(h.on_fill));
        40.0
    });
    row(p, &mut y, "Figures · tabular and lining (tnum, lnum), right aligned", &mut |p, y| {
        let style = TextStyle::serif(14.5, 400);
        let mut a = 0.0;
        for value in ["1 111.11", "8 888.88", "−0.70"] {
            a += p.text_in(value, &style, RectF::new(x, y + a, 90.0, 0.0), Align::Trailing, hex(h.paper)).height;
        }
        a
    });

    // Column 2: the pencil, rings and stars, on the sheet.
    let x = 512.0;
    let mut y = 54.0;
    p.text("PENCIL UNDER WORDS", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    y += 26.0;
    let word = TextStyle::serif(17.5, 400);
    for text in ["a", "Lab", "Output", "Ilford Multigrade IV", "Kodak Portra 400 printed on Fomabrom"] {
        let m = p.measure(text, &word, 0.0);
        p.underline(RectF::new(x, y + m.height, m.width, 6.0), t, false);
        p.text(text, &word, x, y, hex(h.paper));
        y += m.height + 14.0;
    }
    // Struck: always a thin pencil line, never the highlighter.
    let m = p.text("Fomabrom Variant 111", &word, x, y, hex(h.dim));
    p.pencil(RectF::new(x - 3.0, y + m.height * 0.55 - 2.5, m.width + 6.0, 5.0), hex(h.accent));
    p.text("— skipped while scanning film", &TextStyle::serif(14.0, 400).italic(), x + m.width + 12.0, y + 3.0, hex(h.dim));
    y += m.height + 26.0;

    p.text("RINGS", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    y += 30.0;
    let picture = RectF::new(x + 10.0, y, 176.0, 117.0);
    p.fill_rect(picture, hex(h.blank));
    let (ox, oy) = (negs_marks::RING_OUTSET.0 as f32, negs_marks::RING_OUTSET.1 as f32);
    p.ring(RectF::new(picture.x - ox, picture.y - oy, picture.w + 2.0 * ox, picture.h + 2.0 * oy), negs_marks::Radii::BOX, -0.8, hex(h.accent));
    let tile = RectF::new(x + 230.0, y + 10.0, 120.0, 80.0);
    p.fill_rect(tile, hex(h.blank));
    p.ring(RectF::new(tile.x - ox, tile.y - oy, tile.w + 2.0 * ox, tile.h + 2.0 * oy), negs_marks::Radii::BOX, -1.0, hex(h.accent));
    let three = TextStyle::serif(16.0, 600);
    let n = p.text("3", &three, x + 395.0, y + 40.0, hex(h.paper));
    p.ring(RectF::new(x + 395.0 - 6.0, y + 40.0 - 2.0, n.width + 12.0, n.height + 4.0), negs_marks::Radii::NUMBER, -1.0, hex(h.accent));
    y += 117.0 + 34.0;

    p.text("STARS 1–5, ON THE FILM", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    y += 24.0;
    let film = RectF::new(x, y, 440.0, 5.0 * 22.0 + 16.0);
    p.fill_rect(film, hex(h.film));
    let names = ["DSC_6435.NEF", "IMG_0001.JPG", "DSC_7041.NEF", "f03.jpg", "Ålesund 2024.dng"];
    for (row, name) in names.iter().enumerate() {
        let cy = film.y + 14.0 + row as f32 * 22.0;
        for index in 0..=row {
            let cx = film.x + 20.0 + index as f32 * (negs_marks::STAR_SIZE + negs_marks::STAR_GAP) as f32;
            p.star(name, index, cx, cy, hex(t.film.paper), true);
        }
        p.text(name, &TextStyle::ui(11.0, 400), film.x + 110.0, cy - 8.0, hex(t.film.dim));
    }

    // Column 3: the crosses, the boxes and every token.
    let x = 980.0;
    let mut y = 54.0;
    p.text("REJECT CROSSES (LOOSE, GONE OVER TWICE)", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    y += 24.0;
    for (i, name) in ["DSC_6435.NEF", "IMG_0001.JPG"].iter().enumerate() {
        let r = RectF::new(x + i as f32 * 200.0, y, 180.0, 120.0);
        p.fill_rect(r, hex(PICTURE).alpha(0.4));
        p.cross(name, r, hex(h.paper), true);
        p.text(name, &caption, r.x, r.y + r.h + 4.0, hex(h.dim));
    }
    y += 120.0 + 34.0;
    p.text("PICK BOXES (FOUR, LOOP, TWO; STRIP; SELECTED)", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    y += 34.0;
    for (i, name) in ["DSC_6435.NEF", "DSC_5784.NEF", "DSC_7006.NEF"].iter().enumerate() {
        let r = RectF::new(x + 14.0 + i as f32 * 140.0, y, 104.0, 70.0);
        p.fill_rect(r, hex(PICTURE));
        p.pick_box(BoxInk::Grid, name, r, hex(h.paper), true);
    }
    y += 70.0 + 36.0;
    let strip = RectF::new(x + 14.0, y, 104.0, 70.0);
    p.fill_rect(RectF::new(strip.x - 14.0, strip.y - 10.0, 132.0, 90.0), hex(h.film));
    p.fill_rect(strip, hex(PICTURE));
    p.pick_box(BoxInk::Strip, "IMG_0001.JPG", strip, hex(t.film.paper), true);
    let picked = RectF::new(x + 170.0, y, 104.0, 70.0);
    p.fill_rect(picked, hex(PICTURE));
    p.pick_box(BoxInk::Grid, "DSC_6435.NEF", picked, hex(h.paper), true);
    p.pick_box(BoxInk::Selection, "DSC_6435.NEF", picked, hex(h.accent), true);
    p.text("edited, picked, selected", &caption, picked.x + picked.w + 22.0, y + 4.0, hex(h.dim));
    // A path of one's own: the chevron after a choice (⌄), stroked.
    let chevron = |_: ()| negs_marks::Path(vec![negs_marks::Seg::Move(negs_marks::pt(0.0, 0.0)), negs_marks::Seg::Line(negs_marks::pt(4.0, 4.0)), negs_marks::Seg::Line(negs_marks::pt(8.0, 0.0))]);
    let m = p.text("Portra 400", &TextStyle::serif(15.0, 400), picked.x + picked.w + 22.0, y + 26.0, hex(h.paper));
    p.path(1, || chevron(()), picked.x + picked.w + 28.0 + m.width, y + 34.0, Some(1.4), hex(h.dim));
    p.fill_ellipse(picked.x + picked.w - 14.5, picked.y + 14.5, 3.5, 3.5, hex(h.accent));
    y += 70.0 + 34.0;

    p.text("TOKENS", &TextStyle::serif(11.0, 700).tracking(0.13), x, y, hex(h.label));
    p.text("FILM INKS", &TextStyle::serif(11.0, 700).tracking(0.13), x + 220.0, y, hex(h.label));
    y += 22.0;
    let swatch = |p: &mut Painter, x: f32, y: f32, name: &str, rgb: u32, alpha: f32| {
        p.fill_rect(RectF::new(x, y, 26.0, 16.0), hex(h.panel));
        p.fill_rect(RectF::new(x, y, 26.0, 16.0), hex(rgb).alpha(alpha));
        p.stroke_round_rect(RectF::new(x + 0.5, y + 0.5, 25.0, 15.0), 0.0, 1.0, hex(h.line));
        p.text(name, &caption, x + 34.0, y + 1.0, hex(h.label));
    };
    let mut label = String::with_capacity(32);
    for (i, (name, rgb)) in h.named().into_iter().enumerate() {
        label.clear();
        use std::fmt::Write;
        let _ = write!(label, "{name} #{rgb:06x}");
        swatch(p, x, y + i as f32 * 21.0, &label, rgb, 1.0);
    }
    let film_inks = [("paper", t.film.paper), ("label", t.film.label), ("dim", t.film.dim), ("track", t.film.track), ("line", t.film.line), ("accent", t.film.accent), ("blank", t.film.blank)];
    for (i, (name, rgb)) in film_inks.into_iter().enumerate() {
        label.clear();
        use std::fmt::Write;
        let _ = write!(label, "{name} #{rgb:06x}");
        swatch(p, x + 220.0, y + i as f32 * 21.0, &label, rgb, 1.0);
    }
    let (dim_rgb, dim_alpha) = t.dimming();
    let extras = [
        ("selection: accent 0.32", h.accent, theme::SELECTION_OPACITY),
        ("highlighter 0.85", theme::HIGHLIGHTER_INK, theme::HIGHLIGHTER_OPACITY),
        ("crop shade 0.6", theme::CROP_SHADE, theme::CROP_SHADE_OPACITY),
        ("dimming", dim_rgb, dim_alpha),
        ("on photo", t.on_photo(), 1.0),
        ("disabled 0.36", h.paper, theme::DISABLED_OPACITY),
        ("strip dim 0.42", h.paper, theme::STRIP_DIM),
    ];
    for (i, (name, rgb, alpha)) in extras.into_iter().enumerate() {
        swatch(p, x + 220.0, y + (8 + i) as f32 * 21.0, name, rgb, alpha);
    }
    // What the window shows today: the system palette's accent.
    swatch(p, x, y + 15.0 * 21.0, "system accent (shown today)", 0, 0.0);
    p.fill_rect(RectF::new(x, y + 15.0 * 21.0, 26.0, 16.0), Ink::gdi(theme::palette().accent));
    let shadow = t.shadow_strength();
    p.text(if shadow < 1.0 { "shadows 0.3" } else { "shadows 1" }, &caption, x + 254.0, y + 15.0 * 21.0, hex(h.dim));
}

/// What the type is set in on this Windows, and what it cost to load.
fn about_fonts(plain: bool) -> String {
    let Some(shared) = super::d2d::shared() else { return "Direct2D unavailable".into() };
    let serif = match shared.serif() {
        super::d2d::Serif::Variable { .. } => "Newsreader, variable (opsz, wght)",
        super::d2d::Serif::Instances { .. } => "Newsreader, named instances",
        super::d2d::Serif::Missing => "no Newsreader: system font",
    };
    let c = shared.cost;
    format!(
        "{serif} · {}{} · Direct2D {:.1} ms, DirectWrite {:.1} ms, fonts {:.1} ms{}",
        shared.ui_family(),
        if plain { " for the serif too" } else { "" },
        if c.thread { " (own thread)" } else { "" },
        c.d2d.as_secs_f64() * 1e3,
        c.dwrite.as_secs_f64() * 1e3,
        c.fonts.as_secs_f64() * 1e3
    )
}
