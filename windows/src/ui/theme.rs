//! The colours every owner-draw and `WM_CTLCOLOR*` handler uses, one palette
//! for the whole window: light or dark as the system's app theme says
//! (switching live), the system colours in high contrast, and the accent
//! colour from the system.
//!
//! Dark common controls use the `DarkMode_Explorer` and `DarkMode_CFD`
//! themes; dark menus and scroll bars need three undocumented uxtheme
//! exports (by ordinal, as every Win32 app that does this uses them). They
//! are looked up only on builds known to have them, and without them menus
//! stay light. See docs/windows-app.md, Dark mode.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Accessibility::{HIGHCONTRASTW, HCF_HIGHCONTRASTON};
use windows::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo, SetWindowTheme};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCSTR, PCWSTR, w};

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

pub const DARK: Palette = Palette {
    window: rgb(0x1c, 0x1c, 0x1c),
    panel: rgb(0x20, 0x20, 0x20),
    control: rgb(0x2d, 0x2d, 0x2d),
    control_hover: rgb(0x32, 0x32, 0x32),
    control_pressed: rgb(0x27, 0x27, 0x27),
    subtle_hover: rgb(0x2d, 0x2d, 0x2d),
    subtle_pressed: rgb(0x29, 0x29, 0x29),
    border: rgb(0x3a, 0x3a, 0x3a),
    strong_border: rgb(0x9a, 0x9a, 0x9a),
    text: rgb(0xf2, 0xf2, 0xf2),
    secondary: rgb(0xa8, 0xa8, 0xa8),
    disabled: rgb(0x6e, 0x6e, 0x6e),
    accent: rgb(0x60, 0xcd, 0xff),
    accent_hover: rgb(0x5a, 0xbc, 0xe8),
    accent_pressed: rgb(0x52, 0xab, 0xd2),
    on_accent: rgb(0x00, 0x00, 0x00),
    well: rgb(0x2f, 0x2f, 0x2f),
    selection: rgb(0x33, 0x33, 0x33),
    canvas: [0x2b, 0x2b, 0x2b],
};

/// High contrast: the system's colours only.
fn high_contrast_palette() -> Palette {
    let sys = |i: SYS_COLOR_INDEX| unsafe { GetSysColor(i) };
    let face = sys(COLOR_WINDOW);
    let text = sys(COLOR_WINDOWTEXT);
    Palette {
        window: face,
        panel: face,
        control: sys(COLOR_BTNFACE),
        control_hover: sys(COLOR_BTNFACE),
        control_pressed: sys(COLOR_BTNFACE),
        subtle_hover: face,
        subtle_pressed: face,
        border: sys(COLOR_BTNTEXT),
        strong_border: text,
        text,
        secondary: text,
        disabled: sys(COLOR_GRAYTEXT),
        accent: sys(COLOR_HIGHLIGHT),
        accent_hover: sys(COLOR_HIGHLIGHT),
        accent_pressed: sys(COLOR_HIGHLIGHT),
        on_accent: sys(COLOR_HIGHLIGHTTEXT),
        well: face,
        selection: sys(COLOR_HIGHLIGHT),
        canvas: [face as u8, (face >> 8) as u8, (face >> 16) as u8],
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub dark: bool,
    pub high_contrast: bool,
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

pub fn dark() -> bool {
    current().dark
}

/// Reads the system again; true if anything changed.
pub fn refresh() -> bool {
    let theme = detect();
    let changed = CURRENT.with(|c| c.replace(Some(theme)) != Some(theme));
    if changed {
        set_app_mode(theme);
    }
    changed
}

fn detect() -> Theme {
    if high_contrast() {
        return Theme { palette: high_contrast_palette(), dark: false, high_contrast: true };
    }
    // NEGS_THEME=light|dark overrides the system for tests and screenshots.
    let dark = match std::env::var("NEGS_THEME").as_deref() {
        Ok("dark") => true,
        Ok("light") => false,
        _ => apps_use_dark(),
    };
    let mut palette = if dark { DARK } else { LIGHT };
    if let Some([light, base, deep]) = accent_palette() {
        if dark {
            palette.accent = light;
            palette.accent_hover = mix(light, base, 0.2);
            palette.accent_pressed = mix(light, base, 0.35);
        } else {
            palette.accent = deep;
            palette.accent_hover = mix(deep, base, 0.25);
            palette.accent_pressed = mix(deep, base, 0.45);
        }
    }
    Theme { palette, dark, high_contrast: false }
}

fn high_contrast() -> bool {
    let mut info = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let ok = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, info.cbSize, Some(&mut info as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok();
    ok && info.dwFlags & HCF_HIGHCONTRASTON == HCF_HIGHCONTRASTON
}

fn apps_use_dark() -> bool {
    let mut value = 1u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut _),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && value == 0
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

// MARK: The system's dark mode

/// The Windows build number (10.0.x).
pub fn build() -> u32 {
    thread_local! {
        static BUILD: Cell<u32> = const { Cell::new(0) };
    }
    BUILD.with(|b| {
        if b.get() == 0 {
            #[repr(C)]
            struct Version {
                size: u32,
                major: u32,
                minor: u32,
                build: u32,
                platform: u32,
                service_pack: [u16; 128],
            }
            let mut v = Version { size: std::mem::size_of::<Version>() as u32, major: 0, minor: 0, build: 0, platform: 0, service_pack: [0; 128] };
            unsafe {
                let ntdll = LoadLibraryW(w!("ntdll.dll")).unwrap_or_default();
                if let Some(f) = GetProcAddress(ntdll, PCSTR(c"RtlGetVersion".as_ptr() as *const u8)) {
                    let f: extern "system" fn(*mut Version) -> i32 = std::mem::transmute(f);
                    f(&mut v);
                }
            }
            b.set(v.build.max(1));
        }
        b.get()
    })
}

/// The uxtheme exports by ordinal (1903 and later: 18362).
struct UxTheme {
    allow_dark_for_window: extern "system" fn(HWND, BOOL) -> BOOL,
    set_preferred_app_mode: extern "system" fn(i32) -> i32,
    flush_menu_themes: extern "system" fn(),
}

fn uxtheme() -> Option<&'static UxTheme> {
    thread_local! {
        static UX: Cell<Option<&'static UxTheme>> = const { Cell::new(None) };
        static LOOKED: Cell<bool> = const { Cell::new(false) };
    }
    if !LOOKED.with(|l| l.replace(true)) {
        let found = (|| unsafe {
            if build() < 18362 {
                return None;
            }
            let module = LoadLibraryW(w!("uxtheme.dll")).ok()?;
            let ordinal = |n: usize| GetProcAddress(module, PCSTR(n as *const u8));
            let allow = ordinal(133)?;
            let mode = ordinal(135)?;
            let flush = ordinal(136)?;
            Some(&*Box::leak(Box::new(UxTheme {
                allow_dark_for_window: std::mem::transmute(allow),
                set_preferred_app_mode: std::mem::transmute(mode),
                flush_menu_themes: std::mem::transmute(flush),
            })))
        })();
        UX.with(|u| u.set(found));
    }
    UX.with(Cell::get)
}

fn set_app_mode(theme: Theme) {
    if let Some(ux) = uxtheme() {
        // Default, AllowDark, ForceDark, ForceLight.
        let mode = if theme.high_contrast { 0 } else if theme.dark { 2 } else { 3 };
        (ux.set_preferred_app_mode)(mode);
        (ux.flush_menu_themes)();
    }
}

/// Before the first window: menus follow the theme from the start.
pub fn init() {
    let theme = current();
    if theme.dark {
        set_app_mode(theme);
    }
}

/// A top-level window's title bar and frame follow the theme.
pub fn apply_window(hwnd: HWND) {
    let dark = dark();
    unsafe {
        if let Some(ux) = uxtheme() {
            let _ = (ux.allow_dark_for_window)(hwnd, BOOL(dark as i32));
        }
        // DWMWA_USE_IMMERSIVE_DARK_MODE: 20, or 19 before 20H1.
        let attribute = if build() >= 18985 { 20 } else { 19 };
        let value = BOOL(dark as i32);
        if dark || build() >= 18362 {
            let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(attribute), &value as *const _ as *const _, 4);
        }
    }
}

/// A common control's dark or light theme (nothing in light at creation,
/// so light startup is unchanged).
pub fn apply_control(hwnd: HWND, created: bool) {
    let dark = dark();
    if created && !dark {
        return;
    }
    let mut name = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut name) } as usize;
    let class = String::from_utf16_lossy(&name[..n]);
    let theme: Option<PCWSTR> = match class.as_str() {
        "SysListView32" | "tooltips_class32" | "NegsRows" | "NegsCropRows" | "NegsStockGrid" | "NegsStrip" | "ScrollBar" | "Edit" | "msctls_progress32" => {
            Some(if dark { w!("DarkMode_Explorer") } else { w!("Explorer") })
        }
        _ => None,
    };
    let Some(theme) = theme else { return };
    unsafe {
        if let Some(ux) = uxtheme() {
            let _ = (ux.allow_dark_for_window)(hwnd, BOOL(dark as i32));
        }
        let _ = SetWindowTheme(hwnd, theme, PCWSTR::null());
    }
}

/// A combo box's drop-down list, themed when it opens (`CBN_DROPDOWN`)
/// rather than when the box is made.
pub fn combo_list(combo: HWND) {
    let dark = dark();
    unsafe {
        let mut info = COMBOBOXINFO { cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32, ..Default::default() };
        if GetComboBoxInfo(combo, &mut info).is_ok() && !info.hwndList.is_invalid() {
            let _ = SetWindowTheme(info.hwndList, if dark { w!("DarkMode_Explorer") } else { w!("Explorer") }, PCWSTR::null());
        }
    }
}

/// A control just made with `class`: themed only if it is one that needs it
/// (and only in dark), without asking the window for its class.
pub fn apply_created(hwnd: HWND, class: PCWSTR) {
    if !dark() || (class.0 as usize) < 0x10000 {
        return;
    }
    let name = unsafe { class.to_string() }.unwrap_or_default().to_ascii_lowercase();
    if matches!(name.as_str(), "syslistview32" | "tooltips_class32" | "negsrows" | "negscroprows" | "negsstockgrid" | "negsstrip" | "msctls_progress32" | "scrollbar") {
        apply_control(hwnd, true);
    }
}

/// After a switch: every window of this thread and its children.
pub fn apply_all() {
    unsafe extern "system" fn child(hwnd: HWND, _: LPARAM) -> BOOL {
        apply_control(hwnd, false);
        TRUE
    }
    unsafe extern "system" fn top(hwnd: HWND, _: LPARAM) -> BOOL {
        unsafe {
            if GetParent(hwnd).is_err() || GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CHILD.0 == 0 {
                apply_window(hwnd);
            }
            apply_control(hwnd, false);
            let _ = EnumChildWindows(hwnd, Some(child), LPARAM(0));
            let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
        }
        TRUE
    }
    unsafe {
        let _ = EnumThreadWindows(windows::Win32::System::Threading::GetCurrentThreadId(), Some(top), LPARAM(0));
    }
}

// MARK: The redesign's themes

// The macOS app's five themes (round 28), one token table each, exactly as
// `AppTheme.swift` and `Theme.swift` have them (design/proposals/
// IMPLEMENTATION-THEMES.md, IMPLEMENTATION.md §2.1). Not shown yet: the
// window still paints with the system palette above until the window shell
// and inspector move over (Windows parity phases 2 and 3).

/// A built-in theme. Darkroom is the default. On Windows, Plain follows
/// Windows' light or dark setting (`resolved`): Plain dark is never chosen
/// on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeName {
    Darkroom,
    ContactSheet,
    LightTable,
    Daylight,
    Plain,
    PlainDark,
}

impl ThemeName {
    pub const ALL: [ThemeName; 6] = [Self::Darkroom, Self::ContactSheet, Self::LightTable, Self::Daylight, Self::Plain, Self::PlainDark];

    /// The stored name, as the macOS app's `AppTheme` raw value.
    pub fn key(self) -> &'static str {
        match self {
            Self::Darkroom => "darkroom",
            Self::ContactSheet => "contactSheet",
            Self::LightTable => "lightTable",
            Self::Daylight => "daylight",
            Self::Plain => "plain",
            Self::PlainDark => "plainDark",
        }
    }

    /// A stored name or a title, any case, with or without spaces.
    pub fn parse(text: &str) -> Option<Self> {
        let squashed: String = text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
        Self::ALL.into_iter().find(|t| t.key().to_ascii_lowercase() == squashed)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Darkroom => "Darkroom",
            Self::ContactSheet => "Contact sheet",
            Self::LightTable => "Light table",
            Self::Daylight => "Daylight",
            Self::Plain => "Plain",
            Self::PlainDark => "Plain dark",
        }
    }

    /// The line under the swatches in Settings › Look.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Darkroom => "Warm black, cream ink and the yellow pencil. The default.",
            Self::ContactSheet => "Ivory paper and the red grease pencil; the strip stays black film.",
            Self::LightTable => "Lit white, graphite type and a yellow highlighter.",
            Self::Daylight => "Soft grey with an amber pencil, the calmest of the light ones.",
            Self::Plain => "Black on light grey in the system font, with plain blue marks. The easiest to read.",
            Self::PlainDark => "Plain in dark greys: light words in the system font, blue marks.",
        }
    }

    /// Plain follows Windows' app mode (or `NEGS_THEME`), as the window
    /// does: Plain dark when apps are dark.
    pub fn resolved(self) -> Self {
        match self {
            Self::Plain | Self::PlainDark if dark() => Self::PlainDark,
            Self::Plain | Self::PlainDark => Self::Plain,
            other => other,
        }
    }

    pub fn tokens(self) -> &'static Tokens {
        match self {
            Self::Darkroom => &DARKROOM,
            Self::ContactSheet => &CONTACT_SHEET,
            Self::LightTable => &LIGHT_TABLE,
            Self::Daylight => &DAYLIGHT,
            Self::Plain => &PLAIN,
            Self::PlainDark => &PLAIN_DARK,
        }
    }
}

/// One theme's colours as 0xRRGGBB, sRGB (Swift's `Palette.Hex`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hex {
    /// The sheet the photograph lies on, and the title band.
    pub bg: u32,
    /// The inspector, Settings and the alert.
    pub panel: u32,
    /// The strip, the tray and the leader.
    pub film: u32,
    /// Behind the grid.
    pub grid: u32,
    /// Rules between sections and modules.
    pub line: u32,
    /// Slider track, outlines, separators, outlined buttons.
    pub track: u32,
    /// Values at their default, hints, secondary words.
    pub dim: u32,
    /// Control labels.
    pub label: u32,
    /// What's being read: names, changed values, the focus box.
    pub paper: u32,
    /// The pencil's thin marks: rings, dots, fills, lines. Never words.
    pub accent: u32,
    /// The Export and Done buttons, and the words on them.
    pub fill: u32,
    pub on_fill: u32,
    /// Errors and warnings, always as words.
    pub warn: u32,
    /// A tile not yet developed.
    pub blank: u32,
}

impl Hex {
    /// Every token with its Swift name, for swatches and checks.
    pub fn named(&self) -> [(&'static str, u32); 14] {
        [
            ("bg", self.bg),
            ("panel", self.panel),
            ("film", self.film),
            ("grid", self.grid),
            ("line", self.line),
            ("track", self.track),
            ("dim", self.dim),
            ("label", self.label),
            ("paper", self.paper),
            ("accent", self.accent),
            ("fill", self.fill),
            ("onFill", self.on_fill),
            ("warn", self.warn),
            ("blank", self.blank),
        ]
    }
}

/// A theme: its colours, the inks on its film, and how its marks are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub hex: Hex,
    /// Light ink on dark: menus and pop-ups are drawn dark.
    pub dark: bool,
    /// The pencil under a word is a highlighter swiped behind its lower
    /// half (Light table); strikes stay a pencil line in `accent`.
    pub highlighter: bool,
    /// The system font for the serif, and straight, upright pencil marks.
    pub plain: bool,
    /// The words and marks on the film (strip, tray, leader): the theme's
    /// own, except where the film is dark under light panels (Contact sheet).
    pub film: Hex,
}

/// The T2 · Label tokens (IMPLEMENTATION.md §2.1).
const DARKROOM_HEX: Hex = Hex {
    bg: 0x0c0b0a,
    panel: 0x161513,
    film: 0x000000,
    grid: 0x000000,
    line: 0x2b2925,
    track: 0x47443d,
    dim: 0x918a7c,
    label: 0xbeb6a7,
    paper: 0xf2ebdd,
    accent: 0xffd23f,
    fill: 0xffd23f,
    on_fill: 0x14120c,
    warn: 0xff8a66,
    blank: 0x1c1b18,
};

pub const DARKROOM: Tokens = Tokens { hex: DARKROOM_HEX, dark: true, highlighter: false, plain: false, film: DARKROOM_HEX };

/// Ivory paper and the red grease pencil of a Magnum contact sheet; the
/// strip stays black film, its inks Darkroom's with a brighter red.
pub const CONTACT_SHEET: Tokens = Tokens {
    hex: Hex {
        bg: 0xf7f3ea,
        panel: 0xe9e2d3,
        film: 0x0c0b0a,
        grid: 0xf7f3ea,
        line: 0xd6cdbb,
        track: 0xb3aa99,
        dim: 0x655c4e,
        label: 0x463f35,
        paper: 0x1d1a15,
        accent: 0xc63a26,
        fill: 0xc63a26,
        on_fill: 0xfff8ee,
        warn: 0x9a2f18,
        blank: 0xe2dccd,
    },
    dark: false,
    highlighter: false,
    plain: false,
    film: Hex { accent: 0xe2492f, ..DARKROOM_HEX },
};

const LIGHT_TABLE_HEX: Hex = Hex {
    bg: 0xffffff,
    panel: 0xededeb,
    film: 0xf3f3f1,
    grid: 0xf3f3f1,
    line: 0xdcdcd9,
    track: 0xbdbdba,
    dim: 0x666663,
    label: 0x41413f,
    paper: 0x151514,
    accent: 0xad7b00,
    fill: 0xffd23f,
    on_fill: 0x14120c,
    warn: 0xb4381e,
    blank: 0xe4e4e1,
};

/// A lit white table, graphite type; the yellow stays as a highlighter and
/// in the Export button, ochre where it's a thin line.
pub const LIGHT_TABLE: Tokens = Tokens { hex: LIGHT_TABLE_HEX, dark: false, highlighter: true, plain: false, film: LIGHT_TABLE_HEX };

const DAYLIGHT_HEX: Hex = Hex {
    bg: 0xe8e6e1,
    panel: 0xd8d5cf,
    film: 0xc9c6bf,
    grid: 0xc9c6bf,
    line: 0xc6c2bb,
    track: 0xa29e96,
    dim: 0x57524b,
    label: 0x3a3631,
    paper: 0x161411,
    accent: 0xa35000,
    fill: 0xa35000,
    on_fill: 0xffffff,
    warn: 0x912a16,
    blank: 0xd0cdc7,
};

/// Soft warm grey with an amber pencil.
pub const DAYLIGHT: Tokens = Tokens { hex: DAYLIGHT_HEX, dark: false, highlighter: false, plain: false, film: DAYLIGHT_HEX };

const PLAIN_HEX: Hex = Hex {
    bg: 0xf7f7f7,
    panel: 0xececec,
    film: 0xe2e2e2,
    grid: 0xf7f7f7,
    line: 0xd2d2d2,
    track: 0xa6a6a6,
    dim: 0x4a4a4a,
    label: 0x1d1d1d,
    paper: 0x000000,
    accent: 0x0058d0,
    fill: 0x0062d9,
    on_fill: 0xffffff,
    warn: 0xb0160a,
    blank: 0xe0e0e0,
};

/// Near native: neutral greys, near-black words (7 : 1 and up on every
/// ground), a plain blue mark and the system font.
pub const PLAIN: Tokens = Tokens { hex: PLAIN_HEX, dark: false, highlighter: false, plain: true, film: PLAIN_HEX };

const PLAIN_DARK_HEX: Hex = Hex {
    bg: 0x1e1e1e,
    panel: 0x282828,
    film: 0x141414,
    grid: 0x1e1e1e,
    line: 0x3a3a3a,
    track: 0x5e5e5e,
    dim: 0xb4b4b4,
    label: 0xdcdcdc,
    paper: 0xf5f5f5,
    accent: 0x0091ff,
    fill: 0x0059d1,
    on_fill: 0xffffff,
    warn: 0xff6961,
    blank: 0x2e2e2e,
};

/// Plain in dark greys: words 7 : 1 and up on every ground, a lighter blue
/// for marks, a readable dark red.
pub const PLAIN_DARK: Tokens = Tokens { hex: PLAIN_DARK_HEX, dark: true, highlighter: false, plain: true, film: PLAIN_DARK_HEX };

/// The highlighter's yellow and strength (Light table).
pub const HIGHLIGHTER_INK: u32 = 0xffd23f;
pub const HIGHLIGHTER_OPACITY: f32 = 0.85;
/// Text selection: the pencil at this opacity, never the system blue (§4.9).
pub const SELECTION_OPACITY: f32 = 0.32;
/// Outside the crop box the photograph darkens with this, at `CROP_SHADE_OPACITY` (§4.10).
pub const CROP_SHADE: u32 = 0x080706;
pub const CROP_SHADE_OPACITY: f32 = 0.6;
/// Disabled controls fade to this opacity (§2.1: 0.35–0.38).
pub const DISABLED_OPACITY: f32 = 0.36;
/// Strip frames other than the current one (§4.4).
pub const STRIP_DIM: f32 = 0.42;

impl Tokens {
    /// What dims the window under an alert: black at 0.28 on a dark theme, a
    /// warm brown at 0.12 on a light one (r28-light.html).
    pub fn dimming(&self) -> (u32, f32) {
        // sRGB 0.24, 0.16, 0.08.
        if self.dark { (0x000000, 0.28) } else { (0x3d2914, 0.12) }
    }
    /// Words on a dark badge laid over a photograph: Darkroom's `paper` in
    /// every theme, since the badge stays dark.
    pub fn on_photo(&self) -> u32 {
        DARKROOM.hex.paper
    }
    /// The strength of shadows under alerts and tags: softer on a light theme.
    pub fn shadow_strength(&self) -> f32 {
        if self.dark { 1.0 } else { 0.3 }
    }
}

/// A token as a GDI colour (0x00bbggrr).
pub fn gdi(hex: u32) -> Color {
    rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
