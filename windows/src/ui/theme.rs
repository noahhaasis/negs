//! The colours every owner-draw and `WM_CTLCOLOR*` handler uses, one palette
//! for the whole window, made from the chosen theme's tokens (the macOS
//! app's themes, below). The theme doesn't follow the system's light or dark
//! mode: Plain is always light and Plain dark always dark. A new user starts
//! on whichever of the two matches Windows (`first_launch_theme`). High
//! contrast keeps the system colours.
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

pub use negs_app::theme::*;

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

/// The light system palette, for tests of drawing in it.
#[cfg(test)]
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
    /// The theme chosen (View › Theme, kept in the settings).
    static CHOSEN: Cell<ThemeName> = const { Cell::new(ThemeName::Darkroom) };
    /// The tokens the window is drawn with: the shown theme's, or the
    /// system's colours in high contrast.
    static TOKENS: Cell<Tokens> = const { Cell::new(DARKROOM) };
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
        let palette = high_contrast_palette();
        let tokens = high_contrast_tokens(&palette);
        TOKENS.with(|t| t.set(tokens));
        return Theme { palette, dark: tokens.dark, high_contrast: true };
    }
    let tokens = *shown().tokens();
    TOKENS.with(|t| t.set(tokens));
    Theme { palette: palette_of(&tokens), dark: tokens.dark, high_contrast: false }
}

/// `NEGS_THEME`, for tests and screenshots: a theme's name, or light and
/// dark for Plain and Plain dark.
fn forced() -> Option<ThemeName> {
    let value = std::env::var("NEGS_THEME").ok()?;
    match value.to_ascii_lowercase().as_str() {
        "light" => Some(ThemeName::Plain),
        "dark" => Some(ThemeName::PlainDark),
        other => ThemeName::parse(other),
    }
}

/// The theme a new user starts on: Plain, or Plain dark when Windows'
/// apps are dark. It's stored at once, so it stays when Windows changes.
pub fn first_launch_theme() -> ThemeName {
    if apps_use_dark() { ThemeName::PlainDark } else { ThemeName::Plain }
}

/// The theme chosen, whatever `NEGS_THEME` forces.
pub fn chosen() -> ThemeName {
    CHOSEN.with(Cell::get)
}

/// Sets the theme chosen; `refresh` then repaints with it.
pub fn set_chosen(theme: ThemeName) {
    CHOSEN.with(|c| c.set(theme));
}

/// Whether `NEGS_THEME` decides the theme (View › Theme then only ticks).
pub fn forced_by_environment() -> bool {
    forced().is_some()
}

/// The theme the window shows: `NEGS_THEME`'s or the chosen one.
pub fn shown() -> ThemeName {
    forced().unwrap_or_else(chosen)
}

/// The tokens the window is drawn with now.
pub fn tokens() -> Tokens {
    current();
    TOKENS.with(Cell::get)
}

/// The palette's roles from a theme's tokens, for what still paints with
/// it: the inspector's controls (until phase 3), Settings, the strip.
fn palette_of(t: &Tokens) -> Palette {
    let h = &t.hex;
    let toward_paper = |amount: f64| mix(gdi(h.panel), gdi(h.paper), amount);
    let control = if t.dark { toward_paper(0.07) } else { mix(gdi(h.panel), gdi(0xffffff), 0.6) };
    // Words on a filled control: the theme's own on its fill, else
    // whichever of near black and white reads better on the pencil.
    let on_accent = if h.accent == h.fill {
        h.on_fill
    } else if contrast(h.accent, 0x14120c) >= contrast(h.accent, 0xffffff) {
        0x14120c
    } else {
        0xffffff
    };
    Palette {
        window: gdi(h.bg),
        panel: gdi(h.panel),
        control,
        control_hover: mix(control, gdi(h.paper), 0.05),
        control_pressed: mix(control, gdi(h.paper), 0.1),
        subtle_hover: toward_paper(0.07),
        subtle_pressed: toward_paper(0.12),
        border: gdi(h.line),
        strong_border: gdi(h.track),
        text: gdi(h.paper),
        secondary: gdi(h.dim),
        disabled: mix(gdi(h.panel), gdi(h.dim), 0.55),
        accent: gdi(h.accent),
        accent_hover: mix(gdi(h.accent), gdi(h.paper), 0.15),
        accent_pressed: mix(gdi(h.accent), gdi(h.paper), 0.3),
        on_accent: gdi(on_accent),
        well: gdi(h.blank),
        selection: toward_paper(0.12),
        canvas: [(h.bg >> 16) as u8, (h.bg >> 8) as u8, h.bg as u8],
    }
}

/// High contrast: the system's colours as tokens, for what Direct2D draws,
/// with Plain's straight marks and system font.
fn high_contrast_tokens(p: &Palette) -> Tokens {
    let hex = |c: Color| ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff);
    let h = Hex {
        bg: hex(p.window),
        panel: hex(p.panel),
        film: hex(p.window),
        grid: hex(p.window),
        line: hex(p.border),
        track: hex(p.text),
        dim: hex(p.text),
        label: hex(p.text),
        paper: hex(p.text),
        accent: hex(p.accent),
        fill: hex(p.accent),
        on_fill: hex(p.on_accent),
        warn: hex(p.text),
        blank: hex(p.window),
    };
    Tokens { hex: h, dark: luminance(h.bg) < 0.5, highlighter: false, plain: true, film: h }
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

/// A solid brush for `color`, kept for the life of the thread.
pub fn brush(color: Color) -> HBRUSH {
    BRUSHES.with(|b| HBRUSH(*b.borrow_mut().entry(color).or_insert_with(|| unsafe { CreateSolidBrush(COLORREF(color)).0 as isize }) as *mut _))
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

/// Whether the file dialogs come up dark. They follow Windows' own app
/// mode, whatever the theme: forcing the app light (`set_app_mode`) left
/// Windows 10's file dialog dark on a dark Windows (2026-11-19), so the
/// Mac's way, where the theme sets the panels' appearance, isn't open here.
pub fn dialogs_dark() -> bool {
    !current().high_contrast && apps_use_dark()
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

/// Lets a window that isn't the app's own, such as a file dialog's
/// control, take a dark theme.
pub fn allow_dark(hwnd: HWND) {
    if let Some(ux) = uxtheme() {
        let _ = (ux.allow_dark_for_window)(hwnd, TRUE);
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

// MARK: The redesign's themes (pure-Rust definitions live in `negs_app::theme`)

/// A token as a GDI colour (0x00bbggrr).
pub fn gdi(hex: u32) -> Color {
    rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
