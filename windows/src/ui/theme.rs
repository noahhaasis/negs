//! The colours every owner-draw and `WM_CTLCOLOR*` handler uses, one palette
//! for the whole window, and the accent colour from the system.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::*;
use windows::core::w;

/// A GDI colour, 0x00bbggrr.
pub type Color = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The empty state, the film strip and lists.
    pub window: Color,
    /// The adjustments column, the toolbar and dialogs.
    pub panel: Color,
    /// Buttons and fields at rest.
    pub control: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    /// Hover behind flat things (headers, glyph buttons, toolbar buttons).
    pub subtle_hover: Color,
    pub subtle_pressed: Color,
    pub border: Color,
    /// Unfilled slider tracks, checkbox and radio outlines.
    pub strong_border: Color,
    pub text: Color,
    /// Values, notes, footers and captions.
    pub secondary: Color,
    pub disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    /// Text and glyphs on the accent.
    pub on_accent: Color,
    /// Placeholders behind thumbnails.
    pub well: Color,
    /// A selected row in a list without focus.
    pub selection: Color,
    /// Around the photograph.
    pub canvas: [u8; 3],
}

pub const LIGHT: Palette = Palette {
    window: rgb(0xfb, 0xfb, 0xfb),
    panel: rgb(0xf3, 0xf3, 0xf3),
    control: rgb(0xff, 0xff, 0xff),
    control_hover: rgb(0xf9, 0xf9, 0xf9),
    control_pressed: rgb(0xf0, 0xf0, 0xf0),
    subtle_hover: rgb(0xe8, 0xe8, 0xe8),
    subtle_pressed: rgb(0xdf, 0xdf, 0xdf),
    border: rgb(0xe0, 0xe0, 0xe0),
    strong_border: rgb(0x8a, 0x8a, 0x8a),
    text: rgb(0x1b, 0x1b, 0x1b),
    secondary: rgb(0x60, 0x60, 0x60),
    disabled: rgb(0xa3, 0xa3, 0xa3),
    accent: rgb(0x00, 0x5f, 0xb8),
    accent_hover: rgb(0x19, 0x6e, 0xbf),
    accent_pressed: rgb(0x31, 0x7d, 0xc6),
    on_accent: rgb(0xff, 0xff, 0xff),
    well: rgb(0xe4, 0xe4, 0xe4),
    selection: rgb(0xe5, 0xe5, 0xe5),
    canvas: [0x9a, 0x9a, 0x9a],
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub dark: bool,
}

thread_local! {
    static CURRENT: Cell<Option<Theme>> = const { Cell::new(None) };
    static BRUSHES: RefCell<HashMap<Color, isize>> = RefCell::new(HashMap::new());
}

/// The current palette (read from the system the first time).
pub fn palette() -> Palette {
    current().palette
}

pub fn current() -> Theme {
    CURRENT.with(|c| match c.get() {
        Some(theme) => theme,
        None => {
            let theme = detect();
            c.set(Some(theme));
            theme
        }
    })
}

/// Reads the system again; true if anything changed.
pub fn refresh() -> bool {
    let theme = detect();
    CURRENT.with(|c| c.replace(Some(theme)) != Some(theme))
}

fn detect() -> Theme {
    let mut palette = LIGHT;
    if let Some([light, base, dark]) = accent_palette() {
        let _ = light;
        palette.accent = dark;
        palette.accent_hover = mix(dark, base, 0.25);
        palette.accent_pressed = mix(dark, base, 0.45);
    }
    Theme { palette, dark: false }
}

/// The system's accent shades (AccentPalette: light 3, 2, 1, base, dark 1,
/// 2, 3, …): light 2, the base and dark 1.
fn accent_palette() -> Option<[Color; 3]> {
    let mut bytes = [0u8; 32];
    let mut size = bytes.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size < 32 {
        return None;
    }
    let at = |i: usize| rgb(bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2]);
    Some([at(1), at(3), at(4)])
}

/// `a` moved towards `b` by `t`.
pub fn mix(a: Color, b: Color, t: f64) -> Color {
    let channel = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as f64, ((b >> shift) & 0xff) as f64);
        ((x + (y - x) * t).round() as u32) << shift
    };
    channel(0) | channel(8) | channel(16)
}

/// A solid brush for `color`, kept for the life of the thread.
pub fn brush(color: Color) -> HBRUSH {
    BRUSHES.with(|b| HBRUSH(*b.borrow_mut().entry(color).or_insert_with(|| unsafe { CreateSolidBrush(COLORREF(color)).0 as isize }) as *mut _))
}

/// The accent as the canvas wants it, `[r, g, b]`.
pub fn accent_rgb() -> [u8; 3] {
    let c = palette().accent;
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}
