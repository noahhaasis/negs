//! Settings (`Settings.swift`, "B · Index" in design/proposals/r19-settings.html):
//! a window of its own, owned by the editor, with the title band of the
//! main window (Newsreader's "Settings" and Close), then the inspector's
//! tab words, Photographs · Speed · Upkeep · Look, each a short page of the
//! inspector's rows. Changes apply at once, as on the Mac; Esc or Close
//! hides the window. The window fits its page's height, keeping its top
//! edge where it is. Built the first time it opens (File › Settings…,
//! Ctrl+,), then hidden and shown again.
//!
//! Where the Windows settings went: Photographs has the new photographs'
//! film and paper and the brightness match, Films the Fotufilm films
//! (Keep paper became the paper lock on the label), then RAW + JPEG pairing
//! and the sidecars, as on the Mac. Speed has the previews
//! and the export memory, Upkeep the updates, and Look the themes and the
//! viewer sheet.

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::{SS_NOPREFIX, SS_NOTIFY, SS_OWNERDRAW};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::{Ink, RectF};
use super::draw::{self, Canvas, Role};
use super::theme::{self, ThemeName};
use super::{App, create, ink, titlebar, with_app};
use crate::model::Model;
use crate::platform::wide;
use crate::state::FotufilmFilms;
use super::sheet::ViewerSheet;

/// The window's width in DIPs (`SettingsWindowController.width`).
const WIDTH: f64 = 440.0;
/// The theme swatches: three to a row, 12 apart, 62 tall (`ThemePicker`).
const SWATCH_GAP: f64 = 12.0;
const SWATCH_COLUMNS: usize = 3;
const MINIATURE: f64 = 62.0;

const FIRST_ID: u16 = 1000;
const PARTS: u16 = 8;
const TAB_ID: u16 = 900;

/// Settings' pages, in the order of their tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Photographs,
    Speed,
    Upkeep,
    Look,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Photographs, Tab::Speed, Tab::Upkeep, Tab::Look];

    fn title(self) -> &'static str {
        match self {
            Tab::Photographs => "Photographs",
            Tab::Speed => "Speed",
            Tab::Upkeep => "Upkeep",
            Tab::Look => "Look",
        }
    }

    /// As the Mac stores it (`settingsTab`); anything else is Photographs.
    pub fn parse(text: &str) -> Tab {
        Tab::ALL.into_iter().find(|t| t.title() == text).unwrap_or(Tab::Photographs)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tick {
    MatchNew,
    SeparateJpegs,
    Sidecars,
    Previews,
    LimitPreview,
    LowMemory,
    AutoUpdates,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    /// Settings › New photographs › Film and Paper (`newPhotoFilm`).
    NewFilm,
    NewPaper,
    Fotufilm,
    /// The viewer's sheet (`ViewerSheet`).
    Sheet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hint {
    Text(&'static str),
    /// What the new photographs' film and paper do, and a slide's lack of paper.
    NewPhotos,
    /// The chosen theme's line.
    Theme,
    /// What Show camera JPEGs separately does, on or off.
    Pairing,
    /// What Save ratings and edits next to photos does, on or off.
    Sidecars,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Section(&'static str),
    Tick(&'static str, Tick),
    Choice(&'static str, Choice),
    Hint(Hint),
    /// A row's words alone, such as "Updates are not configured…".
    Readout(&'static str),
    /// "Last checked …" and Check for Updates….
    CheckUpdates,
    Themes,
}

/// Under Settings › New photographs' film and paper (`Settings.swift`).
const NEW_PHOTOS: &str = "A photo you haven’t edited opens with this film and paper, and its thumbnail shows them. Photos you’ve edited keep their own.";
/// A slide film is scanned, without a paper (the Mac strikes the paper through).
const NEW_PHOTOS_SLIDE: &str = "Not printed: scanning the film. A photo you haven’t edited opens with this film, and its thumbnail shows it. Photos you’ve edited keep their own.";

/// Under Settings › RAW + JPEG, as the switch stands (`Settings.swift`).
const SEPARATE_JPEGS: &str = "A RAW and the camera’s JPEG of the same name, such as IMG_0412.CR3 and IMG_0412.JPG, are two photos.";
const PAIRED_JPEGS: &str = "A RAW and the camera’s JPEG of the same name, such as IMG_0412.CR3 and IMG_0412.JPG, are one photo: the RAW develops, the JPEG goes to the Recycle Bin with it, and Original can show it. A JPEG you’ve already rated or edited stays a photo of its own.";
/// Under Settings › Edits.
const SIDECARS_ON: &str = "Each photo remembers its own edits in a small file beside it, such as IMG_0412.CR3.negs, which travels with the folder. A photo you haven’t edited starts with the film and paper above, default Prep and Lab settings and no border; the Image Size and output colour stay.";
const SIDECARS_OFF: &str = "Ratings and edits won’t be saved. Reopening a photo starts from the defaults.";

/// Each page's rows, as the Mac's sections (`SettingsView`).
fn items(tab: Tab) -> Vec<Item> {
    match tab {
        Tab::Photographs => vec![
            Item::Section("New photographs"),
            Item::Choice("Film", Choice::NewFilm),
            Item::Choice("Paper", Choice::NewPaper),
            Item::Hint(Hint::NewPhotos),
            Item::Tick("Match to camera brightness", Tick::MatchNew),
            Item::Hint(Hint::Text("New photographs start with Prep › Match camera brightness on: a RAW develops about as bright as the camera's own preview, a JPEG or TIFF as the file itself.")),
            Item::Section("Films"),
            Item::Choice("Fotufilm films", Choice::Fotufilm),
            Item::Hint(Hint::Text("Films ported from Fotufilm. From datasheets hides the eight it models from sample photographs rather than the manufacturer’s data. Photos already using a hidden film keep it.")),
            Item::Section("RAW + JPEG"),
            Item::Tick("Show camera JPEGs separately", Tick::SeparateJpegs),
            Item::Hint(Hint::Pairing),
            Item::Section("Edits"),
            Item::Tick("Save ratings and edits next to photos", Tick::Sidecars),
            Item::Hint(Hint::Sidecars),
        ],
        Tab::Speed => vec![
            Item::Section("Previews"),
            Item::Tick("Film, paper and preset previews", Tick::Previews),
            Item::Hint(Hint::Text("Show each film, paper and preset on your photograph. Turn off to use simple menus and lists on slower computers.")),
            Item::Tick("Limit preview to 12 MP", Tick::LimitPreview),
            Item::Hint(Hint::Text("Develops the preview faster on large photographs. Export keeps the full Image Size.")),
            Item::Section("Export"),
            Item::Tick("Use less memory for export", Tick::LowMemory),
            Item::Hint(Hint::Text("Develops the grain one colour at a time. Same result, less memory, longer export.")),
        ],
        Tab::Upkeep if crate::updates::configured() => vec![
            Item::Section("Updates"),
            Item::Tick("Automatically check for updates", Tick::AutoUpdates),
            Item::Hint(Hint::Text("Updates are installed only when you choose to restart Negs.")),
            Item::CheckUpdates,
        ],
        Tab::Upkeep => vec![
            Item::Section("Updates"),
            Item::Readout("Updates are not configured for this build."),
            Item::Hint(Hint::Text("Install a release build to receive updates.")),
        ],
        Tab::Look => vec![
            Item::Section("Theme"),
            Item::Themes,
            Item::Hint(Hint::Theme),
            Item::Section("Viewer"),
            Item::Choice("Sheet", Choice::Sheet),
            Item::Hint(Hint::Text("The colour behind the photograph. Theme’s own follows the theme; a grey is fairer for judging a photograph. The caption and the label under it change their ink to suit.")),
        ],
    }
}

struct Row {
    tab: Tab,
    item: Item,
    controls: Vec<HWND>,
}

pub struct Settings {
    window: HWND,
    page: HWND,
    tabs: [HWND; 4],
    rows: Vec<Row>,
    tab: Tab,
}

thread_local! {
    /// The window, for routing its keys before the editor's accelerators.
    static WINDOW: Cell<isize> = const { Cell::new(0) };
}

fn window() -> HWND {
    HWND(WINDOW.with(Cell::get) as *mut _)
}

fn px(hwnd: HWND, dip: f64) -> i32 {
    (dip * unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0).round() as i32
}

fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        for (class, proc_) in [(w!("NegsSettings"), Some(window_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT)), (w!("NegsSettingsPage"), Some(page_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT))] {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: proc_,
                hInstance: super::instance(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    });
}

/// File › Settings… (Ctrl+,): shows the window, building it the first time.
pub fn open(app: &mut App) {
    if app.settings.is_none() {
        build(app);
    }
    let Some(s) = &app.settings else { return };
    let window = s.window;
    sync(app);
    layout(app);
    unsafe {
        if !IsWindowVisible(window).as_bool() {
            center(app.main, window);
        }
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

fn build(app: &mut App) {
    register();
    let font = app.fonts.ui;
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let title = wide("Settings");
    let window = unsafe {
        CreateWindowExW(WS_EX_CONTROLPARENT, w!("NegsSettings"), crate::platform::pcwstr(&title), style, 0, 0, app.px(WIDTH), app.px(300.0), app.main, None, super::instance(), None)
            .unwrap_or_default()
    };
    theme::apply_window(window);
    titlebar::round_corners(window);
    unsafe {
        // The band replaces the caption (`WM_NCCALCSIZE`).
        let _ = SetWindowPos(window, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE);
    }
    WINDOW.with(|w| w.set(window.0 as isize));
    let page = create(w!("NegsSettingsPage"), "Settings", WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), window, 0, HFONT::default());
    let tab_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    let mut tabs = [HWND::default(); 4];
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        tabs[i] = create(w!("BUTTON"), tab.title(), tab_style | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), page, TAB_ID + i as u16, font);
        draw::set_role(tabs[i], Role::TabWord);
        draw::track_hover(tabs[i]);
    }
    let mut rows = vec![];
    for tab in Tab::ALL {
        for item in items(tab) {
            let index = rows.len() as u16;
            let controls = create_row(page, font, item, FIRST_ID + index * PARTS);
            rows.push(Row { tab, item, controls });
        }
    }
    THEMES_ROW.with(|r| r.set(rows.iter().position(|r| r.item == Item::Themes).map(|i| i as u16)));
    let tab = Tab::parse(&app.model.prefs.settings_tab);
    app.settings = Some(Settings { window, page, tabs, rows, tab });
}

fn create_row(page: HWND, font: HFONT, item: Item, first: u16) -> Vec<HWND> {
    let hidden = WS_CHILD;
    let ink_static = |text: &str, role: Role, part: u16, notify: bool| {
        let style = SS_OWNERDRAW.0 | SS_NOPREFIX.0 | if notify { SS_NOTIFY.0 } else { 0 };
        let c = create(w!("STATIC"), text, hidden | WINDOW_STYLE(style), WINDOW_EX_STYLE(0), page, first + part, font);
        draw::set_role(c, role);
        super::accessible_role(c, 0x29);
        if notify {
            draw::track_hover(c);
        }
        c
    };
    match item {
        Item::Section(title) => {
            let head = ink_static(&title.to_uppercase(), Role::Section, 0, false);
            super::accessible_name(head, title);
            vec![head]
        }
        Item::Tick(title, _) => {
            let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0), page, first, font);
            draw::set_role(c, Role::Tick);
            draw::track_hover(c);
            vec![c]
        }
        Item::Choice(title, choice) => {
            let label = ink_static(title, Role::Label, 0, true);
            let combo = create(w!("COMBOBOX"), "", hidden | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0), WINDOW_EX_STYLE(0), page, first + 1, font);
            let options: Vec<&str> = match choice {
                // Filled by `sync` from the catalog's stocks.
                Choice::NewFilm | Choice::NewPaper => vec![],
                Choice::Fotufilm => FotufilmFilms::ALL.iter().map(|f| f.title()).collect(),
                Choice::Sheet => ViewerSheet::ALL.iter().map(|s| s.title()).collect(),
            };
            for option in options {
                let text = wide(option);
                unsafe {
                    SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            unsafe {
                SendMessageW(combo, CB_SETITEMHEIGHT, WPARAM(usize::MAX), LPARAM(px(page, ink::ROW) as isize - 6));
            }
            draw::set_role(combo, Role::Choice);
            draw::paint_combo(combo);
            super::accessible_name(combo, title);
            vec![label, combo]
        }
        Item::Hint(hint) => {
            let text = match hint {
                Hint::Text(text) => text,
                _ => "",
            };
            vec![ink_static(text, Role::Hint, 0, false)]
        }
        Item::Readout(text) => vec![ink_static(text, Role::Label, 0, false)],
        Item::CheckUpdates => {
            // Painted here: "Last checked" in italic `dim`, the time in `paper`.
            let lead = create(w!("STATIC"), "", hidden | WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), page, first, font);
            super::accessible_role(lead, 0x29);
            let button = create(w!("BUTTON"), "Check for Updates…", hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), page, first + 1, font);
            draw::set_role(button, Role::Outlined);
            draw::track_hover(button);
            vec![lead, button]
        }
        Item::Themes => ThemeName::ALL
            .iter()
            .enumerate()
            .map(|(i, theme)| {
                let style = hidden | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32) | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
                let c = create(w!("BUTTON"), theme.title(), style, WINDOW_EX_STYLE(0), page, first + i as u16, font);
                draw::track_hover(c);
                super::dialogs::tooltip(c, theme.hint());
                c
            })
            .collect(),
    }
}

fn hint_text(model: &Model, hint: Hint) -> &'static str {
    match hint {
        Hint::Text(text) => text,
        Hint::NewPhotos if model.new_photo_film_is_slide() => NEW_PHOTOS_SLIDE,
        Hint::NewPhotos => NEW_PHOTOS,
        Hint::Theme => theme::chosen().hint(),
        Hint::Pairing if model.prefs.library.separate_camera_jpegs => SEPARATE_JPEGS,
        Hint::Pairing => PAIRED_JPEGS,
        Hint::Sidecars if model.prefs.library.sidecars() => SIDECARS_ON,
        Hint::Sidecars => SIDECARS_OFF,
    }
}

fn ticked(model: &Model, tick: Tick) -> bool {
    let prefs = &model.prefs;
    match tick {
        Tick::MatchNew => prefs.match_new_photos,
        Tick::SeparateJpegs => prefs.library.separate_camera_jpegs,
        Tick::Sidecars => prefs.library.sidecars(),
        Tick::Previews => prefs.previews,
        Tick::LimitPreview => prefs.limits_preview(),
        Tick::LowMemory => prefs.low_memory_export,
        Tick::AutoUpdates => crate::updates::automatic(),
    }
}

fn set_text(hwnd: HWND, text: &str) {
    if draw::window_text(hwnd) != text {
        let w = wide(text);
        unsafe {
            let _ = SetWindowTextW(hwnd, crate::platform::pcwstr(&w));
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

fn set_check(hwnd: HWND, on: bool) {
    unsafe {
        if (SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1) != on {
            SendMessageW(hwnd, BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

thread_local! {
    /// What New photographs' Film and Paper lists were last filled with.
    static STOCK_LISTS: std::cell::RefCell<[Vec<(String, String)>; 2]> = const { std::cell::RefCell::new([Vec::new(), Vec::new()]) };
}

/// New photographs' Film or Paper list: the catalog's stocks (refilled when
/// they change), the chosen one selected; a slide's paper can't be chosen.
fn sync_stocks(model: &Model, paper: bool, combo: HWND) {
    let choices = if paper { model.new_photo_paper_choices() } else { model.new_photo_film_choices() };
    let (film, chosen_paper) = model.new_photo_stocks();
    let chosen = if paper { chosen_paper } else { film };
    unsafe {
        let refill = STOCK_LISTS.with(|lists| {
            let mut lists = lists.borrow_mut();
            let last = &mut lists[paper as usize];
            let changed = *last != choices || SendMessageW(combo, CB_GETCOUNT, WPARAM(0), LPARAM(0)).0 != choices.len() as isize;
            if changed {
                *last = choices.clone();
            }
            changed
        });
        if refill {
            SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
            for (_, name) in &choices {
                let text = wide(name);
                SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
            }
        }
        let index = choices.iter().position(|(id, _)| *id == chosen).map_or(-1, |i| i as isize);
        if SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != index {
            SendMessageW(combo, CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
        }
        let enabled = !choices.is_empty() && !(paper && model.new_photo_film_is_slide());
        if IsWindowEnabled(combo).as_bool() != enabled {
            let _ = EnableWindow(combo, enabled);
        }
    }
}

/// Brings the open window up to date with the app's settings. Called on
/// every refresh; it does nothing while Settings has never opened.
pub fn sync(app: &mut App) {
    let Some(s) = &app.settings else { return };
    let model = &app.model;
    for (i, &tab) in s.tabs.iter().enumerate() {
        set_check(tab, Tab::ALL[i] == s.tab);
    }
    let chosen = theme::chosen();
    let mut hints_changed = false;
    for row in &s.rows {
        let c = &row.controls;
        match row.item {
            Item::Tick(_, tick) => set_check(c[0], ticked(model, tick)),
            // Only while its page shows: the lists are long.
            Item::Choice(_, choice @ (Choice::NewFilm | Choice::NewPaper)) if row.tab == s.tab => sync_stocks(model, choice == Choice::NewPaper, c[1]),
            Item::Choice(_, Choice::NewFilm | Choice::NewPaper) => {}
            Item::Choice(_, choice) => {
                let index = match choice {
                    Choice::NewFilm | Choice::NewPaper => 0,
                    Choice::Fotufilm => FotufilmFilms::ALL.iter().position(|&f| f == model.fotufilm_films()).unwrap_or(0),
                    Choice::Sheet => ViewerSheet::ALL.iter().position(|&s| s == super::sheet::chosen()).unwrap_or(0),
                };
                unsafe {
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != index as isize {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(index), LPARAM(0));
                    }
                }
            }
            Item::Hint(hint) => {
                let text = hint_text(model, hint);
                hints_changed |= draw::window_text(c[0]) != text;
                set_text(c[0], text);
            }
            Item::CheckUpdates => {
                set_text(c[0], &crate::updates::last_check().unwrap_or_default());
            }
            Item::Themes => {
                for (i, &swatch) in c.iter().enumerate() {
                    let on = ThemeName::ALL[i] == chosen;
                    set_check(swatch, on);
                    super::accessible_value(swatch, if on { "Chosen" } else { "" });
                }
            }
            _ => {}
        }
    }
    // A hint of another length moves the rows under it.
    if hints_changed {
        layout(app);
    }
}

/// Places the shown page's rows, the tabs over them and the rules, and fits
/// the window to them, its top edge kept.
pub fn layout(app: &mut App) {
    let Some(s) = &app.settings else { return };
    let page = s.page;
    let window = s.window;
    let px = |dip: f64| px(window, dip);
    let scale = draw::dpi_of(window);
    let plain = theme::tokens().plain;
    let width = px(WIDTH);
    let (gutter, margin, row_h, rule) = (px(ink::GUTTER), px(ink::MARGIN), px(ink::ROW), px(1.0).max(1));
    let right = width - gutter;
    let mut positions: Vec<(HWND, i32, i32, i32, i32)> = vec![];
    let mut rules = vec![];
    // The tab words, then a rule under their row.
    let mut x = gutter;
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let word = px(ink::tab_width(tab.title(), plain) as f64);
        positions.push((s.tabs[i], x - margin, 1, word + 2 * margin, px(ink::TAB_ROW) - 2));
        x += word + px(ink::TAB_GAP);
    }
    let mut y = px(ink::TAB_ROW);
    rules.push(y - rule);
    let mut in_section = false;
    let hint_height = |hwnd: HWND| (ink::hint_height(&draw::window_text(hwnd), (right - gutter) as f32 / scale) * scale).ceil() as i32;
    for row in s.rows.iter().filter(|r| r.tab == s.tab) {
        let c = &row.controls;
        let height = match row.item {
            Item::Section(_) => {
                if in_section {
                    y += px(9.0);
                    rules.push(y);
                    y += rule;
                }
                in_section = true;
                positions.push((c[0], gutter, y + px(8.0), right - gutter, px(ink::SECTION_HEAD - 8.0)));
                px(ink::SECTION_HEAD)
            }
            Item::Tick(..) => {
                positions.push((c[0], gutter - margin, y, right - gutter + 2 * margin, row_h));
                row_h
            }
            Item::Choice(title, _) => {
                let label = px(ink::width(title, &ink::LABEL) as f64 + 12.0).max(px(60.0));
                positions.push((c[0], gutter, y, label, row_h));
                positions.push((c[1], gutter + label, y + px(3.0), right - gutter - label + margin, px(300.0)));
                row_h
            }
            Item::Hint(_) => {
                let h = hint_height(c[0]);
                positions.push((c[0], gutter, y, right - gutter, h));
                h + px(7.0)
            }
            Item::Readout(_) => {
                positions.push((c[0], gutter, y, right - gutter, row_h));
                row_h
            }
            Item::CheckUpdates => {
                let button = px(ink::width("Check for Updates…", &ink::OUTLINED) as f64 + 24.0) + 2 * margin;
                positions.push((c[0], gutter, y, right - gutter - button, px(35.0)));
                positions.push((c[1], right - button + margin, y, button, px(35.0)));
                px(35.0)
            }
            Item::Themes => {
                // Three to a row; a name too wide for its column takes two lines.
                let top = y + px(12.0);
                let gap = px(SWATCH_GAP);
                let each = (right - gutter - gap * (SWATCH_COLUMNS as i32 - 1)) / SWATCH_COLUMNS as i32;
                // A row's names take one line, or two if one of them wraps.
                let lines = |row: usize| {
                    let wraps = ThemeName::ALL.iter().skip(row * SWATCH_COLUMNS).take(SWATCH_COLUMNS).any(|name| {
                        super::d2d::measure_text(name.title(), &NAME_CHOSEN, 0.0, plain).width * scale > each as f32
                    });
                    if wraps { 2.0 } else { 1.0 }
                };
                let mut bottom = top;
                let mut row_top = top;
                for (i, &swatch) in c.iter().enumerate() {
                    let (column, line) = (i % SWATCH_COLUMNS, i / SWATCH_COLUMNS);
                    let height = px(MINIATURE) + px(7.0 + lines(line) * 18.0);
                    if column == 0 && line > 0 {
                        row_top = bottom + gap;
                    }
                    let top = row_top;
                    // The ring reaches 7 × 6 DIPs past the miniature.
                    positions.push((swatch, gutter + column as i32 * (each + gap) - px(8.0), top - px(7.0), each + px(16.0), height + px(7.0)));
                    bottom = top + height;
                }
                bottom - y + px(8.0)
            }
        };
        y += height;
    }
    if in_section {
        y += px(9.0);
        rules.push(y);
        y += rule;
    }
    let page_height = y;
    unsafe {
        let shown: std::collections::HashSet<isize> = positions.iter().map(|p| p.0.0 as isize).collect();
        for row in &s.rows {
            for &control in &row.controls {
                if !shown.contains(&(control.0 as isize)) {
                    let _ = ShowWindow(control, SW_HIDE);
                }
            }
        }
        for &(hwnd, x, y, w, h) in &positions {
            let _ = MoveWindow(hwnd, x, y, w.max(0), h.max(0), true);
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }
    ink::set_rules(page, rules);
    let band = titlebar::height(window);
    unsafe {
        let _ = MoveWindow(page, 0, band, width, page_height, true);
        let (mut frame, mut client) = (RECT::default(), RECT::default());
        let _ = GetWindowRect(window, &mut frame);
        let _ = GetClientRect(window, &mut client);
        let (extra_w, extra_h) = ((frame.right - frame.left) - client.right, (frame.bottom - frame.top) - client.bottom);
        let (w, h) = (width + extra_w, band + page_height + extra_h);
        if frame.right - frame.left != w || frame.bottom - frame.top != h {
            let _ = SetWindowPos(window, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Centred over the editor, a third of the way down, as the old dialog was.
fn center(main: HWND, window: HWND) {
    unsafe {
        let (mut owner, mut frame) = (RECT::default(), RECT::default());
        let _ = GetWindowRect(main, &mut owner);
        let _ = GetWindowRect(window, &mut frame);
        let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
        let x = owner.left + (owner.right - owner.left - w) / 2;
        let y = (owner.top + (owner.bottom - owner.top - h) / 3).max(owner.top);
        let _ = SetWindowPos(window, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// After a theme change: the tab words may set in another font.
pub fn retheme(app: &mut App) {
    if app.settings.is_some() {
        sync(app);
        layout(app);
    }
}

fn hide(window: HWND) {
    unsafe {
        let _ = ShowWindow(window, SW_HIDE);
        // The editor comes back to the front, as after a dialog.
        if let Ok(owner) = GetWindow(window, GW_OWNER) {
            let _ = SetForegroundWindow(owner);
        }
    }
}

/// Keys for the Settings window, before the editor's: Esc hides it, and Tab,
/// the arrows and Space move and act within it. True when handled.
pub fn pre_translate(msg: &MSG) -> Option<bool> {
    let window = window();
    if window.is_invalid() || unsafe { !IsWindowVisible(window).as_bool() } {
        return None;
    }
    let ours = msg.hwnd == window || unsafe { IsChild(window, msg.hwnd).as_bool() };
    if !ours {
        return None;
    }
    if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_ESCAPE.0 as usize {
        hide(window);
        return Some(true);
    }
    // Ctrl+W closes it too, as Ctrl+, opens it.
    if msg.message == WM_KEYDOWN && msg.wParam.0 == b'W' as usize && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
        hide(window);
        return Some(true);
    }
    Some(unsafe { IsDialogMessageW(window, msg).as_bool() })
}

fn command(app: &mut App, id: u16, code: u32) {
    if (TAB_ID..TAB_ID + 4).contains(&id) {
        if code == BN_CLICKED {
            let tab = Tab::ALL[(id - TAB_ID) as usize];
            if let Some(s) = &mut app.settings
                && s.tab != tab
            {
                s.tab = tab;
                app.model.prefs.settings_tab = tab.title().into();
                app.model.prefs.save();
                sync(app);
                layout(app);
            }
        }
        return;
    }
    if id < FIRST_ID {
        return;
    }
    let (index, part) = (((id - FIRST_ID) / PARTS) as usize, (id - FIRST_ID) % PARTS);
    let Some(s) = &app.settings else { return };
    let Some(row) = s.rows.get(index) else { return };
    let (item, controls) = (row.item, row.controls.clone());
    match item {
        Item::Tick(_, tick) if code == BN_CLICKED => {
            let on = unsafe { SendMessageW(controls[0], BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 };
            let model = &mut app.model;
            match tick {
                Tick::MatchNew => model.set_match_new_photos(on),
                Tick::SeparateJpegs => model.set_separate_camera_jpegs(on),
                Tick::Sidecars => model.set_sidecars(on),
                Tick::Previews => model.set_previews(on),
                Tick::LimitPreview => model.set_limit_preview(on),
                Tick::LowMemory => model.set_low_memory_export(on),
                Tick::AutoUpdates => {
                    crate::updates::set_automatic(on);
                    model.prefs.check_for_updates = on;
                    model.prefs.save();
                }
            }
        }
        Item::Choice(_, choice) if code == CBN_SELCHANGE => {
            let index = unsafe { SendMessageW(controls[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
            if index < 0 {
                return;
            }
            match choice {
                Choice::NewFilm => {
                    if let Some((id, _)) = app.model.new_photo_film_choices().get(index as usize) {
                        app.model.set_new_photo_film(&id.clone());
                    }
                }
                Choice::NewPaper => {
                    if let Some((id, _)) = app.model.new_photo_paper_choices().get(index as usize) {
                        app.model.set_new_photo_paper(&id.clone());
                    }
                }
                Choice::Fotufilm => app.model.set_fotufilm_films(FotufilmFilms::ALL[index as usize]),
                Choice::Sheet => {
                    let sheet = ViewerSheet::ALL[index as usize];
                    super::sheet::set_chosen(sheet);
                    app.model.prefs.viewer_sheet = sheet.key().into();
                    app.model.prefs.save();
                    super::canvas::sheet_changed(app.canvas);
                    super::empty::sheet_changed(app);
                }
            }
        }
        Item::CheckUpdates if code == BN_CLICKED && part == 1 => crate::updates::check(true),
        Item::Themes if code == BN_CLICKED => {
            let theme = ThemeName::ALL[part as usize];
            if theme::chosen() != theme {
                app.command(super::id::THEME + part);
            }
        }
        _ => return,
    }
    sync(app);
}

// MARK: Painting

/// One theme swatch (`ThemeSwatch`): the window in its colours, ringed in
/// the pencil when chosen, and its name under it.
fn paint_swatch(draw: &NMCUSTOMDRAW) {
    let hwnd = draw.hdr.hwndFrom;
    let Some(index) = (unsafe { GetDlgCtrlID(hwnd) } as u16).checked_sub(FIRST_ID).map(|id| (id % PARTS) as usize) else { return };
    let Some(&name) = ThemeName::ALL.get(index) else { return };
    let chosen = unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 } == 1;
    let hot = draw::is_hot(hwnd);
    let focused = draw.uItemState.0 & CDIS_FOCUS.0 != 0 && draw::focus_cues(hwnd);
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let mut c = Canvas::new(draw.hdc, draw.rc);
    c.clear(theme::gdi(t.hex.panel));
    let (w, h) = ((draw.rc.right - draw.rc.left) as f32 / s, (draw.rc.bottom - draw.rc.top) as f32 / s);
    c.d2d(s, t.plain, |p| {
        // The miniature sits 8 in from the control's sides and 7 down: room for the ring.
        let r = RectF::new(8.0, 7.0, w - 16.0, MINIATURE as f32);
        miniature(p, r, name.tokens(), t.hex.line);
        if chosen {
            let (ox, oy) = (negs_marks::RING_OUTSET.0 as f32, negs_marks::RING_OUTSET.1 as f32);
            p.ring(RectF::new(r.x - ox, r.y - oy, r.w + 2.0 * ox, r.h + 2.0 * oy), negs_marks::Radii::BOX, -1.0, Ink::hex(t.hex.accent));
        }
        let style = if chosen { NAME_CHOSEN } else { NAME };
        let ink = if chosen { t.hex.paper } else if hot { t.hex.label } else { t.hex.dim };
        p.text_in(name.title(), &style, RectF::new(r.x, r.y + r.h + 7.0, r.w, 0.0), super::d2d::Align::Leading, Ink::hex(ink));
        if focused {
            ink::focus_box(p, RectF::new(2.0, 1.0, w - 4.0, h - 2.0), &t);
        }
    });
    c.present(draw.hdc);
}

const NAME: super::d2d::TextStyle = super::d2d::TextStyle::serif(14.0, 400);
const NAME_CHOSEN: super::d2d::TextStyle = super::d2d::TextStyle::serif(14.0, 700);

/// The window in a theme's colours (`ThemeMiniature`): the title band, the
/// sheet with a picture and the label's two lines, the panel with its rule
/// and a marked line, and the strip with the current frame marked.
fn miniature(p: &mut super::d2d::Painter, r: RectF, tokens: &theme::Tokens, edge: u32) {
    let (c, f) = (&tokens.hex, &tokens.film);
    let ink = |hex: u32| Ink::hex(hex);
    p.clipped(r, |p| {
        p.fill_round_rect(r, 4.0, ink(c.bg));
        let panel = RectF::new(r.x + r.w - 30.0, r.y + 7.0, 30.0, r.h - 7.0);
        let sheet = RectF::new(r.x, r.y + 7.0, r.w - 30.0, r.h - 7.0);
        let strip = RectF::new(sheet.x, sheet.y + sheet.h - 13.0, sheet.w, 13.0);
        // The picture: a sky-to-sand gradient, drawn as bands.
        let picture = |p: &mut super::d2d::Painter, x: f32, y: f32, w: f32, h: f32, alpha: f32| {
            let (top, bottom) = ([0.55f32, 0.68, 0.78], [0.86f32, 0.74, 0.56]);
            let bands = 9;
            for i in 0..bands {
                let t = i as f32 / (bands - 1) as f32;
                let channel = |k: usize| ((top[k] + (bottom[k] - top[k]) * t) * 255.0).round() as u32;
                let hex = channel(0) << 16 | channel(1) << 8 | channel(2);
                let band = h / bands as f32;
                p.fill_rect(RectF::new(x, y + band * i as f32, w, band + 0.5), Ink::hex(hex).alpha(alpha));
            }
        };
        picture(p, sheet.x + 8.0, sheet.y + 3.0, 40.0, 27.0, 1.0);
        p.fill_round_rect(RectF::new(sheet.x + 8.0, sheet.y + 33.0, 20.0, 2.5), 1.25, ink(c.paper));
        p.fill_round_rect(RectF::new(sheet.x + 31.0, sheet.y + 33.0, 14.0, 2.5), 1.25, ink(c.dim));
        p.fill_rect(strip, ink(c.film));
        for i in 0..4 {
            let x = strip.x + i as f32 * 17.0;
            picture(p, x, strip.y + 2.0, 15.0, 9.5, if i == 1 { 1.0 } else { theme::STRIP_DIM });
            if i == 1 {
                p.fill_rect(RectF::new(x, strip.y + 11.5, 15.0, 1.5), ink(f.accent));
            }
        }
        p.fill_rect(panel, ink(c.panel));
        p.fill_rect(RectF::new(panel.x, panel.y, 1.0, panel.h), ink(c.line));
        p.fill_rect(RectF::new(panel.x, panel.y, panel.w, 1.0), ink(c.line));
        let (x, mut y) = (panel.x + 5.0, panel.y + 6.0);
        p.fill_round_rect(RectF::new(x, y, 15.0, 3.0), 1.5, ink(c.paper));
        p.fill_rect(RectF::new(x, y + 4.0, 15.0, 1.2), ink(c.accent));
        y += 7.0;
        for w in [0.7f32, 1.0, 0.6] {
            p.fill_round_rect(RectF::new(x, y, 20.0 * w, 2.0), 1.0, Ink::hex(c.label).alpha(0.7));
            y += 6.0;
        }
    });
    p.stroke_round_rect(RectF::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0), 3.5, 1.0, Ink::hex(edge));
}

/// "Last checked" in italic `dim`, then when in the serif (`FigureText`).
fn paint_last_check(item: &DRAWITEMSTRUCT) {
    let hwnd = item.hwndItem;
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let mut c = Canvas::new(item.hDC, item.rcItem);
    c.clear(theme::gdi(t.hex.panel));
    let h = (item.rcItem.bottom - item.rcItem.top) as f32 / s;
    let when = draw::window_text(hwnd);
    if !when.is_empty() {
        c.d2d(s, t.plain, |p| {
            const LEAD: super::d2d::TextStyle = super::d2d::TextStyle::serif(14.5, 400).italic();
            let lead = p.measure("Last checked", &LEAD, 0.0);
            let figure = p.measure(&when, &ink::VALUE, 0.0);
            let y = ((h - ink::VALUE.size) / 2.0 - (figure.baseline - ink::VALUE.size * 0.75)).round();
            p.text("Last checked", &LEAD, 0.0, y + figure.baseline - lead.baseline, Ink::hex(t.hex.dim));
            p.text(&when, &ink::VALUE, lead.width + 5.0, y, Ink::hex(t.hex.paper));
        });
    }
    c.present(item.hDC);
}

/// Whether `hwnd` is a theme swatch, by its id's row: painting runs
/// outside the app's borrow.
fn is_swatch(hwnd: HWND) -> bool {
    let id = unsafe { GetDlgCtrlID(hwnd) } as u16;
    THEMES_ROW.with(Cell::get).is_some_and(|row| id >= FIRST_ID && (id - FIRST_ID) / PARTS == row)
}

thread_local! {
    /// The row of the theme swatches.
    static THEMES_ROW: Cell<Option<u16>> = const { Cell::new(None) };
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCALCSIZE => titlebar::nccalcsize(hwnd, wparam, lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_NCHITTEST => titlebar::nchittest(hwnd, wparam, lparam),
            WM_NCMOUSEMOVE | WM_NCMOUSELEAVE | WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCLBUTTONUP => {
                titlebar::ncmouse(hwnd, msg, wparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }
            WM_MOUSEMOVE => {
                titlebar::ncmouse(hwnd, msg, wparam);
                LRESULT(0)
            }
            WM_NCACTIVATE => {
                titlebar::invalidate_band(hwnd);
                DefWindowProcW(hwnd, msg, wparam, LPARAM(-1))
            }
            WM_SETTEXT | WM_SETICON => {
                let style = GetWindowLongW(hwnd, GWL_STYLE);
                SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_VISIBLE.0 as i32));
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                SetWindowLongW(hwnd, GWL_STYLE, style);
                titlebar::invalidate_band(hwnd);
                result
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                titlebar::paint_band(hwnd, dc, "Settings");
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // Esc from the dialog manager, and the system menu's Close.
            WM_COMMAND if (wparam.0 & 0xffff) as i32 == IDCANCEL.0 => {
                hide(hwnd);
                LRESULT(0)
            }
            WM_CLOSE => {
                hide(hwnd);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                with_app(layout);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn page_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if code == CBN_DROPDOWN || code == CBN_CLOSEUP {
                    let _ = InvalidateRect(HWND(lparam.0 as *mut _), None, false);
                }
                with_app(|app| command(app, id, code));
                LRESULT(0)
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                if !ink::draw_item(item) {
                    paint_last_check(item);
                }
                LRESULT(1)
            }
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.code == NM_CUSTOMDRAW && is_swatch(header.hwndFrom) {
                    let draw = &*(lparam.0 as *const NMCUSTOMDRAW);
                    if draw.dwDrawStage == CDDS_PREERASE || draw.dwDrawStage == CDDS_PREPAINT {
                        paint_swatch(draw);
                    }
                    return LRESULT(CDRF_SKIPDEFAULT as isize);
                }
                draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_CTLCOLORLISTBOX => {
                let dc = HDC(wparam.0 as *mut _);
                let p = theme::palette();
                SetTextColor(dc, COLORREF(p.text));
                SetBkColor(dc, COLORREF(p.control));
                LRESULT(theme::brush(p.control).0 as isize)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                LRESULT(theme::brush(theme::palette().panel).0 as isize)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                draw::paint_panel(hwnd, theme::palette().panel);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each page has the Mac's sections, in its order.
    #[test]
    fn pages_follow_the_macs_sections() {
        let sections = |tab| items(tab).into_iter().filter_map(|i| if let Item::Section(t) = i { Some(t) } else { None }).collect::<Vec<_>>();
        assert_eq!(sections(Tab::Photographs), ["New photographs", "Films"]);
        // New photographs: the Mac's Film, Paper and brightness match, not the old Edits choice.
        let photographs = items(Tab::Photographs);
        let choices: Vec<_> = photographs.iter().filter_map(|i| if let Item::Choice(t, _) = i { Some(*t) } else { None }).collect();
        assert_eq!(choices, ["Film", "Paper", "Fotufilm films"]);
        assert_eq!(photographs[1..4], [Item::Choice("Film", Choice::NewFilm), Item::Choice("Paper", Choice::NewPaper), Item::Hint(Hint::NewPhotos)]);
        assert!(photographs.contains(&Item::Tick("Match to camera brightness", Tick::MatchNew)));
        assert_eq!(sections(Tab::Speed), ["Previews", "Export"]);
        assert_eq!(sections(Tab::Upkeep), ["Updates"]);
        assert_eq!(sections(Tab::Look), ["Theme", "Viewer"]);
        assert_eq!(Tab::parse("Look"), Tab::Look);
        assert_eq!(Tab::parse("nonsense"), Tab::Photographs);
    }
}
