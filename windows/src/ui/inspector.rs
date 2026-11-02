//! The adjustments column (`Inspector.swift`), with the common controls: a
//! histogram, a Lab | Post switch, scrolling rows of trackbars with buddy
//! edit boxes, checkboxes and drop-down lists, and Reset Adjustments. The
//! rows of collapsed groups are created when the group first opens, and the
//! whole column only after the window's first frame (§17). While cropping,
//! the crop panel takes its place.

use std::cell::{Cell, RefCell};

use serde_json::Value;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::{App, TBM_GETPOS, create, post, with_app};
use crate::border::{self, BorderColor, BorderStyle};
use crate::crop::CropAspect;
use crate::json;
use crate::model::{BorderWidth, Model, dirty};
use crate::platform::{pcwstr, wide};
use crate::state::{self, ADJUSTMENT_GROUPS, Adjustment, Section};

const FIRST_ID: u16 = 1000;
const PARTS: u16 = 8;
const LABEL: u16 = 0;
const SLIDER: u16 = 1;
const EDIT: u16 = 2;
const RESET: u16 = 3;
const CHECK: u16 = 4;
const COMBO: u16 = 5;
const HEADER: u16 = 6;
const ACTION: u16 = 7;
const TAB_LAB: u16 = 900;
const TAB_POST: u16 = 901;
const RESET_ALL: u16 = 902;
const CROP_FIRST: u16 = 950;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Lab,
    Post,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum When {
    Always,
    /// Input colour, for files that aren't RAW.
    NotRaw,
    AssumeInput,
    ManualInput,
    FilmTimes,
    NotScanning,
    PaperTimes,
    AutoExposure,
    Raw,
    /// A RAW whose decode found corrections for its lens.
    LensCorrection,
    CustomWhiteBalance,
    Scanning,
    Ratio35,
    LongestSide,
    SizeNote,
    Mat,
    Keyline,
    FilmStyle,
    /// Hidden for B&W films (one emulsion): per-channel grain lanes.
    ColorFilm,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Section(&'static str),
    Footer(&'static str),
    Group { title: &'static str, enable: Option<&'static str> },
    Slider(Adjustment),
    BorderSlider(&'static str, BorderWidth),
    Toggle(&'static str, &'static str),
    LimitPreview,
    /// The colour space the preview develops in (a preference).
    PreviewSpace,
    /// Image Size: Original or Longest side.
    SizeMode,
    LongestSide,
    /// The exported photograph's size.
    Photograph,
    /// An Image Size error, or that the preview shows fewer pixels.
    SizeNote,
    BorderToggle(&'static str, u8),
    Choice(&'static str, &'static str, &'static [&'static str]),
    Stock(bool),
    /// The film or paper as a foldout of preview tiles (Settings › previews).
    Foldout(bool),
    DevTime(bool),
    BorderStyles,
    Color(bool),
    /// The Balance Print button: title, tooltip.
    BalancePrint(&'static str, &'static str),
}

struct Row {
    kind: Kind,
    tab: Tab,
    /// The row index of the group header this row belongs to.
    group: Option<usize>,
    when: When,
    /// Disabled while scanning film directly (the print groups).
    print: bool,
    /// Border controls, disabled while the border is off.
    border: bool,
    controls: Vec<HWND>,
    shown: bool,
}

pub struct Inspector {
    pub panel: HWND,
    histogram: HWND,
    tabs: [HWND; 2],
    rows_panel: HWND,
    reset: HWND,
    crop: Option<CropPanel>,
    pub rect: RECT,
    rows: Vec<Row>,
    scroll: i32,
    content: i32,
    built: bool,
    dpi: u32,
    /// The stock lists were filled from this catalog.
    stocks_filled: usize,
    /// The film and paper tiles, built the first time each foldout opens.
    grids: [Option<StockGrid>; 2],
    dev_times: [Vec<f64>; 2],
    decimal: char,
}

thread_local! {
    /// The current film and paper names, for the foldout headers.
    static FOLDOUT_NAMES: RefCell<[String; 2]> = const { RefCell::new([String::new(), String::new()]) };
    /// The tiles as drawn, by grid (film, paper) and position.
    static TILES: RefCell<[Vec<TileLook>; 2]> = const { RefCell::new([Vec::new(), Vec::new()]) };
    static HISTOGRAM: RefCell<Option<crate::histogram::Histogram>> = const { RefCell::new(None) };
    static SWATCHES: Cell<(u32, u32, u32)> = const { Cell::new((0xffffff, 0, 0)) };
    static ITEM_HEIGHT: Cell<i32> = const { Cell::new(20) };
}

pub fn register() {
    unsafe {
        let classes: [(PCWSTR, WNDPROC, SYS_COLOR_INDEX); 5] = [
            (w!("NegsInspector"), Some(panel_proc), COLOR_BTNFACE),
            (w!("NegsRows"), Some(rows_proc), COLOR_BTNFACE),
            (w!("NegsHistogram"), Some(histogram_proc), COLOR_BTNFACE),
            (w!("NegsCropPanel"), Some(crop_proc), COLOR_BTNFACE),
            (w!("NegsStockGrid"), Some(grid_proc), COLOR_BTNFACE),
        ];
        for (name, proc_, background) in classes {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: proc_,
                hInstance: super::instance(),
                lpszClassName: name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: GetSysColorBrush(background),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    }
}

fn decimal_separator() -> char {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SDECIMAL};
    let mut buffer = [0u16; 8];
    let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SDECIMAL, Some(&mut buffer)) };
    if n > 1 { char::from_u32(buffer[0] as u32).unwrap_or('.') } else { '.' }
}

impl Inspector {
    /// The empty column for the first frame; `build` fills it.
    pub fn create(main: HWND, fonts: &super::Fonts, dpi: u32) -> Self {
        let panel = create(w!("NegsInspector"), "Adjustments", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), main, 0, HFONT::default());
        let _ = fonts;
        Self {
            panel,
            histogram: HWND::default(),
            tabs: [HWND::default(); 2],
            rows_panel: HWND::default(),
            reset: HWND::default(),
            crop: None,
            rect: RECT::default(),
            rows: vec![],
            scroll: 0,
            content: 0,
            built: false,
            dpi,
            stocks_filled: 0,
            grids: [None, None],
            dev_times: [vec![], vec![]],
            decimal: '.',
        }
    }

    fn px(&self, dip: f64) -> i32 {
        (dip * self.dpi as f64 / 96.0).round() as i32
    }

    pub fn built(&self) -> bool {
        self.built
    }

    pub fn layout(&mut self, x: i32, y: i32, width: i32, height: i32, visible: bool) {
        self.rect = RECT { left: x, top: y, right: x + width, bottom: y + height };
        unsafe {
            let _ = MoveWindow(self.panel, x, y, width, height, true);
            let _ = ShowWindow(self.panel, if visible { SW_SHOWNA } else { SW_HIDE });
        }
        self.layout_frame();
    }

    /// The histogram, the tab switch, the rows and the reset button.
    fn layout_frame(&mut self) {
        if !self.built {
            return;
        }
        let (w, h) = (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top);
        let pad = self.px(16.0);
        let histogram = self.px(88.0);
        let tabs = self.px(26.0);
        let reset = self.px(32.0);
        let rows_top = pad / 2 + histogram + self.px(10.0) + tabs + self.px(8.0);
        let rows_bottom = h - reset - self.px(20.0);
        unsafe {
            let _ = MoveWindow(self.histogram, pad, pad / 2, w - 2 * pad, histogram, true);
            let half = (w - 2 * pad) / 2;
            let _ = MoveWindow(self.tabs[0], pad, pad / 2 + histogram + self.px(10.0), half, tabs, true);
            let _ = MoveWindow(self.tabs[1], pad + half, pad / 2 + histogram + self.px(10.0), w - 2 * pad - half, tabs, true);
            let _ = MoveWindow(self.rows_panel, 0, rows_top, w, (rows_bottom - rows_top).max(0), true);
            let _ = MoveWindow(self.reset, pad, h - reset - self.px(12.0), w - 2 * pad, reset, true);
            if let Some(crop) = &self.crop {
                let _ = MoveWindow(crop.panel, 0, 0, w, h, true);
            }
        }
    }
}

/// The rows of both tabs, in order (`Inspector.swift`, `lab` and `post`).
fn rows() -> Vec<Row> {
    let mut rows = vec![];
    let add = |rows: &mut Vec<Row>, kind: Kind, tab: Tab, group: Option<usize>, when: When| {
        rows.push(Row { kind, tab, group, when, print: false, border: false, controls: vec![], shown: false });
        rows.len() - 1
    };
    let lab = Tab::Lab;
    add(&mut rows, Kind::Section("Film & Paper"), lab, None, When::Always);
    add(&mut rows, Kind::Stock(false), lab, None, When::Always);
    add(&mut rows, Kind::Foldout(false), lab, None, When::Always);
    add(&mut rows, Kind::DevTime(false), lab, None, When::FilmTimes);
    add(&mut rows, Kind::Toggle("Scan film directly", "io.scan_film"), lab, None, When::Always);
    add(&mut rows, Kind::Stock(true), lab, None, When::NotScanning);
    add(&mut rows, Kind::Foldout(true), lab, None, When::NotScanning);
    add(&mut rows, Kind::DevTime(true), lab, None, When::PaperTimes);
    add(&mut rows, Kind::Section("Image Size"), lab, None, When::Always);
    add(&mut rows, Kind::SizeMode, lab, None, When::Always);
    add(&mut rows, Kind::LongestSide, lab, None, When::LongestSide);
    add(&mut rows, Kind::Photograph, lab, None, When::Always);
    add(&mut rows, Kind::SizeNote, lab, None, When::SizeNote);
    add(&mut rows, Kind::Footer("Resizes before film simulation, including halation and grain."), lab, None, When::Always);
    add(&mut rows, Kind::Section("Camera"), lab, None, When::Always);
    add(&mut rows, Kind::Toggle("Auto exposure", "camera.auto_exposure"), lab, None, When::Always);
    add(&mut rows, Kind::Choice("Metering", "camera.auto_exposure_method", state::METERING), lab, None, When::AutoExposure);
    for slider in state::CAMERA_SLIDERS {
        add(&mut rows, Kind::Slider(*slider), lab, None, When::Always);
    }
    let input = add(&mut rows, Kind::Group { title: "Input Colour", enable: None }, lab, None, When::NotRaw);
    add(&mut rows, Kind::Choice("Read as", "io.input_interpretation.mode", state::INPUT_MODES), lab, Some(input), When::NotRaw);
    add(&mut rows, Kind::Choice("Colour space", "io.input_interpretation.color_space", state::ASSUMED_SPACES), lab, Some(input), When::AssumeInput);
    add(&mut rows, Kind::Toggle("Linear values", "io.input_interpretation.linear"), lab, Some(input), When::AssumeInput);
    add(&mut rows, Kind::Choice("Colour space", "io.input_color_space", state::MANUAL_SPACES), lab, Some(input), When::ManualInput);
    add(&mut rows, Kind::Toggle("Decode the curve", "io.input_cctf_decoding"), lab, Some(input), When::ManualInput);
    let wb = add(&mut rows, Kind::Group { title: "RAW White Balance", enable: None }, lab, None, When::Raw);
    add(&mut rows, Kind::Choice("White balance", "io.raw_white_balance.mode", state::WHITE_BALANCE), lab, Some(wb), When::Raw);
    add(&mut rows, Kind::Slider(state::WB_TEMPERATURE), lab, Some(wb), When::CustomWhiteBalance);
    add(&mut rows, Kind::Slider(state::WB_TINT), lab, Some(wb), When::CustomWhiteBalance);
    add(&mut rows, Kind::Toggle("Lens corrections", state::LENS_CORRECTION), lab, None, When::LensCorrection);
    let groups = |rows: &mut Vec<Row>, section: Section| {
        for group in ADJUSTMENT_GROUPS.iter().filter(|g| g.section == section) {
            let header = add(rows, Kind::Group { title: group.title, enable: group.enable }, lab, None, When::Always);
            rows[header].print = section == Section::Print;
            for control in group.controls {
                let kind = match *control {
                    Adjustment::Toggle { title, path } => Kind::Toggle(title, path),
                    Adjustment::Choice { title, path, options } => Kind::Choice(title, path, options),
                    Adjustment::BalancePrint { title, help } => Kind::BalancePrint(title, help),
                    slider => Kind::Slider(slider),
                };
                let when = if state::color_only(control.path()) { When::ColorFilm } else { When::Always };
                let row = add(rows, kind, lab, Some(header), when);
                rows[row].print = section == Section::Print;
            }
        }
    };
    groups(&mut rows, Section::Camera);
    add(&mut rows, Kind::Section("Film"), lab, None, When::Always);
    groups(&mut rows, Section::Film);
    add(&mut rows, Kind::Section("Print"), lab, None, When::Always);
    groups(&mut rows, Section::Print);
    add(&mut rows, Kind::Footer("Printing is skipped while scanning film directly."), lab, None, When::Scanning);
    add(&mut rows, Kind::Section("Scan & Color"), lab, None, When::Always);
    groups(&mut rows, Section::Scan);
    let output = add(&mut rows, Kind::Group { title: "Output", enable: None }, lab, None, When::Always);
    add(&mut rows, Kind::LimitPreview, lab, Some(output), When::Always);
    add(&mut rows, Kind::PreviewSpace, lab, Some(output), When::Always);
    add(&mut rows, Kind::Toggle("Encode with the colour space’s curve", "io.output_cctf_encoding"), lab, Some(output), When::Always);
    add(&mut rows, Kind::Toggle("Use less memory for export", "settings.serial_grain"), lab, Some(output), When::Always);

    let post = Tab::Post;
    add(&mut rows, Kind::Section("Tone"), post, None, When::Always);
    for slider in state::POST_TONE {
        add(&mut rows, Kind::Slider(*slider), post, None, When::Always);
    }
    add(&mut rows, Kind::Footer("Adjusts the finished scan/print. To recover a clipped highlight, use exposure in the Lab."), post, None, When::Always);
    add(&mut rows, Kind::Section("Color"), post, None, When::Always);
    add(&mut rows, Kind::Slider(state::POST_SATURATION), post, None, When::Always);
    add(&mut rows, Kind::Section("Border"), post, None, When::Always);
    add(&mut rows, Kind::BorderToggle("Add border", 0), post, None, When::Always);
    let border_row = |rows: &mut Vec<Row>, kind: Kind, when: When| {
        let row = add(rows, kind, post, None, when);
        rows[row].border = true;
    };
    border_row(&mut rows, Kind::BorderStyles, When::Always);
    border_row(&mut rows, Kind::Footer(""), When::Ratio35);
    border_row(&mut rows, Kind::BorderSlider("Width (%)", BorderWidth::Mat), When::Mat);
    border_row(&mut rows, Kind::BorderSlider("Line width (%)", BorderWidth::Line), When::Keyline);
    border_row(&mut rows, Kind::Color(false), When::Mat);
    border_row(&mut rows, Kind::Color(true), When::Keyline);
    border_row(&mut rows, Kind::BorderSlider("Width (%)", BorderWidth::Film), When::FilmStyle);
    border_row(&mut rows, Kind::BorderToggle("Extend to 3:4 for Instagram", 1), When::Always);
    border_row(&mut rows, Kind::BorderToggle("Include in export", 2), When::Always);
    rows
}

/// Fills the column once the window is on screen.
pub fn build(app: &mut App) {
    let main = app.main;
    let _ = main;
    let fonts = &app.fonts;
    let ins = &mut app.inspector;
    ins.decimal = decimal_separator();
    ins.histogram = create(w!("NegsHistogram"), "Histogram", WS_CHILD | WS_VISIBLE, WINDOW_EX_STYLE(0), ins.panel, 0, HFONT::default());
    let tab_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    ins.tabs[0] = create(w!("BUTTON"), "Lab", tab_style | WS_GROUP, WINDOW_EX_STYLE(0), ins.panel, TAB_LAB, fonts.ui);
    ins.tabs[1] = create(w!("BUTTON"), "Post", tab_style, WINDOW_EX_STYLE(0), ins.panel, TAB_POST, fonts.ui);
    ins.rows_panel = create(w!("NegsRows"), "", WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN | WS_GROUP, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), ins.panel, 0, HFONT::default());
    ins.reset = create(w!("BUTTON"), "Reset Adjustments", WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP, WINDOW_EX_STYLE(0), ins.panel, RESET_ALL, fonts.ui);
    super::dialogs::tooltip(ins.reset, "Restore defaults on both tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this.");
    ins.rows = rows();
    ins.built = true;
    ITEM_HEIGHT.with(|h| h.set(ins.px(20.0)));
    ins.layout_frame();
    sync(app, true, true);
}

pub fn dpi_changed(app: &mut App) {
    let dpi = app.dpi;
    let ins = &mut app.inspector;
    ins.dpi = dpi;
    ITEM_HEIGHT.with(|h| h.set(ins.px(20.0)));
    let font = app.fonts.ui;
    for row in &ins.rows {
        for &c in &row.controls {
            unsafe {
                SendMessageW(c, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
        }
    }
    for &c in ins.tabs.iter().chain([&ins.reset]) {
        unsafe {
            SendMessageW(c, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }
    if let Some(crop) = app.inspector.crop.take() {
        unsafe {
            let _ = DestroyWindow(crop.panel);
        }
    }
    // Built again at the new size when shown.
    for grid in app.inspector.grids.iter_mut().filter_map(Option::take) {
        unsafe {
            let _ = DestroyWindow(grid.window);
        }
    }
}

fn tab(model: &Model) -> Tab {
    if model.prefs.inspector_tab == "Post" { Tab::Post } else { Tab::Lab }
}

fn photo_size(model: &Model) -> Option<(u32, u32)> {
    let size = model.photo_size();
    (!size.is_zero()).then(|| (size.width as u32, size.height as u32))
}

fn stock_times(model: &Model, paper: bool) -> Vec<f64> {
    let id = if paper { &model.state.paper } else { &model.state.film };
    model.stocks.iter().find(|s| s.paper == paper && &s.id == id).map(|s| s.times.clone()).unwrap_or_default()
}

fn visible(row: &Row, model: &Model, expanded: &dyn Fn(usize) -> bool) -> bool {
    let scanning = json::boolean(model.value("io.scan_film"));
    let border = model.state.border;
    let shown = match row.when {
        When::Always => true,
        When::FilmTimes => stock_times(model, false).len() > 1,
        When::NotScanning => !scanning,
        When::PaperTimes => !scanning && stock_times(model, true).len() > 1,
        When::AutoExposure => json::boolean(model.value("camera.auto_exposure")),
        When::Raw => model.is_raw,
        When::LensCorrection => model.is_raw && model.lens_supported,
        When::NotRaw => !model.is_raw,
        When::AssumeInput => !model.is_raw && json::string(model.value("io.input_interpretation.mode")) == "assume",
        When::ManualInput => !model.is_raw && json::string(model.value("io.input_interpretation.mode")) == "manual",
        When::CustomWhiteBalance => model.is_raw && json::string(model.value("io.raw_white_balance.mode")) == "custom",
        When::Scanning => scanning,
        When::ColorFilm => !model.film_is_bw(),
        When::Ratio35 => photo_size(model).is_some_and(|(w, h)| !border::fits(BorderStyle::Film35, w, h)),
        When::LongestSide => model.size_mode() == state::SizeMode::LongestSide,
        When::SizeNote => !size_note(model).is_empty(),
        When::Mat => matches!(border.style, BorderStyle::Solid | BorderStyle::Keyline),
        When::Keyline => border.style == BorderStyle::Keyline,
        When::FilmStyle => border.style == BorderStyle::Film,
    };
    // The previews setting shows the foldouts or the menus.
    let shown = shown
        && match row.kind {
            Kind::Stock(_) => !model.prefs.previews,
            Kind::Foldout(_) => model.prefs.previews,
            _ => true,
        };
    shown && row.group.is_none_or(|g| expanded(g))
}

/// Why the Image Size can't be used, or that the preview has fewer pixels
/// than the export; empty otherwise.
fn size_note(model: &Model) -> String {
    if let Some(error) = &model.size_error {
        return error.clone();
    }
    if let Some(Err(error)) = model.resolved_size() {
        return error.to_string();
    }
    if model.preview_scale < 1.0 && model.image.is_some() {
        return format!(
            "The preview shows {} of these pixels. Exports develop all of them; inspect grain in an exported file.",
            crate::model::percent(model.preview_scale)
        );
    }
    String::new()
}

fn window_text(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
    String::from_utf16_lossy(&text[..n])
}

fn row_title(kind: &Kind) -> &'static str {
    match kind {
        Kind::Group { title, .. } => title,
        _ => "",
    }
}

/// Creates the rows that became visible, shows and hides rows, lays them out
/// and brings every control up to date with the model.
pub fn sync(app: &mut App, structure: bool, histogram: bool) {
    if histogram {
        HISTOGRAM.with(|h| *h.borrow_mut() = app.model.histogram.clone());
        if !app.inspector.histogram.is_invalid() {
            unsafe {
                let _ = InvalidateRect(app.inspector.histogram, None, false);
            }
        }
    }
    if !app.inspector.built {
        return;
    }
    sync_crop(app);
    let cropping = app.model.cropping();
    let ins = &mut app.inspector;
    unsafe {
        for c in [ins.histogram, ins.tabs[0], ins.tabs[1], ins.rows_panel, ins.reset] {
            let _ = ShowWindow(c, if cropping { SW_HIDE } else { SW_SHOWNA });
        }
    }
    if cropping {
        return;
    }
    let current = tab(&app.model);
    unsafe {
        SendMessageW(ins.tabs[0], BM_SETCHECK, WPARAM((current == Tab::Lab) as usize), LPARAM(0));
        SendMessageW(ins.tabs[1], BM_SETCHECK, WPARAM((current == Tab::Post) as usize), LPARAM(0));
        let _ = EnableWindow(ins.reset, app.model.has_catalog());
    }
    let model = &app.model;
    let expanded: Vec<bool> = ins.rows.iter().map(|r| matches!(r.kind, Kind::Group { .. }) && model.prefs.expanded(row_title(&r.kind))).collect();
    let want: Vec<bool> = ins.rows.iter().map(|r| r.tab == current && visible(r, model, &|g| expanded[g])).collect();
    let t0 = crate::platform::qpc();
    let mut created = false;
    for i in 0..ins.rows.len() {
        if want[i] && ins.rows[i].controls.is_empty() {
            create_row(ins, &app.fonts, i);
            created = true;
        }
    }
    if created {
        fix_z_order(ins);
    }
    let changed = want.iter().zip(&ins.rows).any(|(w, r)| *w != r.shown);
    for (i, row) in ins.rows.iter_mut().enumerate() {
        if row.shown != want[i] {
            row.shown = want[i];
            for &c in &row.controls {
                unsafe {
                    let _ = ShowWindow(c, if want[i] { SW_SHOWNA } else { SW_HIDE });
                }
            }
        }
    }
    // An open foldout's tiles, built the first time it opens.
    let catalog_size = model.stocks.len() + model.stocks.iter().map(|s| s.id.len()).sum::<usize>();
    for paper in [false, true] {
        let open = model.previews.open(paper) && ins.rows.iter().any(|r| r.kind == Kind::Foldout(paper) && r.shown);
        if ins.grids[paper as usize].as_ref().is_some_and(|g| g.catalog != catalog_size) {
            if let Some(grid) = ins.grids[paper as usize].take() {
                unsafe {
                    let _ = DestroyWindow(grid.window);
                }
            }
        }
        if open && ins.grids[paper as usize].is_none() && model.has_catalog() {
            ins.grids[paper as usize] = Some(build_grid(ins.rows_panel, &app.fonts, model, paper, catalog_size));
            created = true;
            fix_z_order(ins);
        }
    }
    // The note's height follows its text, so set it before laying out.
    let mut note_changed = false;
    for row in ins.rows.iter().filter(|r| r.kind == Kind::SizeNote && r.shown) {
        let note = size_note(model);
        if window_text(row.controls[0]) != note {
            set_text(row.controls[0], &note);
            note_changed = true;
        }
    }
    let t1 = crate::platform::qpc();
    if changed || created || structure || note_changed {
        layout_rows(ins, model);
    }
    let t2 = crate::platform::qpc();
    update_values(ins, model, &expanded);
    let t3 = crate::platform::qpc();
    if created {
        let ms = |a, b| crate::platform::qpc_ms(a, b);
        app.report.line(&format!("inspector sync: create {:.1} ms, layout {:.1} ms, values {:.1} ms", ms(t0, t1), ms(t1, t2), ms(t2, t3)));
    }
}

fn create_row(ins: &mut Inspector, fonts: &super::Fonts, index: usize) {
    let parent = ins.rows_panel;
    let id = |part: u16| FIRST_ID + index as u16 * PARTS + part;
    let kind = ins.rows[index].kind;
    let hidden = WS_CHILD;
    let label = |text: &str, font: HFONT| create(w!("STATIC"), text, hidden | WINDOW_STYLE(SS_NOTIFY.0 | SS_ENDELLIPSIS.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(LABEL), font);
    let check = |text: &str| create(w!("BUTTON"), text, hidden | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0), parent, id(CHECK), fonts.ui);
    let combo = |owner_draw: bool| {
        let style = CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | if owner_draw { (CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32 } else { 0 };
        let c = create(w!("COMBOBOX"), "", hidden | WS_TABSTOP | WINDOW_STYLE(style), WINDOW_EX_STYLE(0), parent, id(COMBO), fonts.ui);
        unsafe {
            SendMessageW(c, CB_SETMINVISIBLE, WPARAM(30), LPARAM(0));
        }
        c
    };
    let mut controls = vec![];
    match kind {
        Kind::Section(title) => controls.push(label(title, fonts.bold)),
        Kind::Footer(text) => {
            let footer = create(w!("STATIC"), text, hidden | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(LABEL), fonts.small);
            controls.push(footer);
        }
        Kind::Group { title, enable } => {
            let header = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.bold);
            controls.push(header);
            if enable.is_some() {
                let on = check("On");
                super::dialogs::tooltip(on, &format!("Turn {title} on or off"));
                controls.push(on);
            }
        }
        Kind::Slider(Adjustment::Slider { .. }) | Kind::BorderSlider(..) => {
            let (title, low, high, step) = match kind {
                Kind::Slider(Adjustment::Slider { title, low, high, step, .. }) => (title, low, high, step),
                Kind::BorderSlider(title, field) => {
                    let (low, high, step) = field.range();
                    (title, low, high, step)
                }
                _ => unreachable!(),
            };
            controls.push(label(title, fonts.ui));
            let slider = create(TRACKBAR_CLASSW, title, hidden | WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), WINDOW_EX_STYLE(0), parent, id(SLIDER), HFONT::default());
            let steps = ((high - low) / step).round() as isize;
            unsafe {
                SendMessageW(slider, TBM_SETRANGEMIN, WPARAM(0), LPARAM(0));
                SendMessageW(slider, TBM_SETRANGEMAX, WPARAM(0), LPARAM(steps));
                SendMessageW(slider, TBM_SETLINESIZE, WPARAM(0), LPARAM(1));
                SendMessageW(slider, TBM_SETPAGESIZE, WPARAM(0), LPARAM((steps / 10).max(1)));
                let _ = SetWindowSubclass(slider, Some(trackbar_subclass), 1, id(RESET) as usize);
            }
            controls.push(slider);
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), WINDOW_EX_STYLE(WS_EX_CLIENTEDGE.0), parent, id(EDIT), fonts.ui);
            super::accessible_name(edit, title);
            unsafe {
                let _ = SetWindowSubclass(edit, Some(edit_subclass), 1, 0);
            }
            controls.push(edit);
            let reset = create(w!("BUTTON"), "\u{E777}", hidden | WINDOW_STYLE(BS_PUSHBUTTON as u32 | BS_FLAT as u32), WINDOW_EX_STYLE(0), parent, id(RESET), fonts.glyph_small);
            super::accessible_name(reset, &format!("Reset {title} to Default"));
            controls.push(reset);
        }
        Kind::Slider(_) => {}
        Kind::Toggle(title, _) | Kind::BorderToggle(title, _) => {
            let c = check(title);
            match kind {
                Kind::Toggle("Scan film directly", _) => super::dialogs::tooltip(c, "Skip printing and scan the developed film, as for slides."),
                Kind::Toggle("Lens corrections", _) => super::dialogs::tooltip(c, "Corrects distortion, vignetting and colour fringing with the data the camera recorded for its lens."),
                Kind::Toggle("Use less memory for export", _) => super::dialogs::tooltip(c, "Processes grain channels one at a time. Same result, less memory, longer export."),
                Kind::BorderToggle(_, 1) => super::dialogs::tooltip(c, "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post."),
                Kind::BorderToggle(_, 2) => super::dialogs::tooltip(c, "Adds the border to exported photographs and saved previews."),
                _ => {}
            }
            controls.push(c);
        }
        Kind::BalancePrint(title, help) => {
            let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(ACTION), fonts.ui);
            super::dialogs::tooltip(c, help);
            controls.push(c);
        }
        Kind::LimitPreview => {
            let c = check("Limit preview to 12 MP");
            super::dialogs::tooltip(c, "Develops the preview faster on large photographs. Export keeps the full Image Size.");
            controls.push(c);
        }
        Kind::PreviewSpace => {
            controls.push(label("Colour space", fonts.ui));
            let c = combo(false);
            for title in crate::model::COLOR_SPACES {
                let text = wide(title);
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            super::dialogs::tooltip(c, "The preview develops in this colour space, so it shows the colours an export in it can hold. Save and Export use it next, and switch it when you pick another.");
            controls.push(c);
        }
        Kind::SizeMode => {
            controls.push(label("Size", fonts.ui));
            let c = combo(false);
            for title in ["Original", "Longest side"] {
                let text = wide(title);
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            super::dialogs::tooltip(c, "Original develops the photograph’s own pixels. Longest side enlarges or reduces it first, so the cropped photograph has that many pixels on its longest side.");
            controls.push(c);
        }
        Kind::LongestSide => {
            controls.push(label("Longest side", fonts.ui));
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL | ES_NUMBER) as u32), WINDOW_EX_STYLE(WS_EX_CLIENTEDGE.0), parent, id(EDIT), fonts.ui);
            super::accessible_name(edit, "Longest side in pixels");
            super::dialogs::tooltip(edit, "The cropped photograph’s longest side, before any border. Press Enter to apply. Exports can still be reduced in the save dialog.");
            unsafe {
                let _ = SetWindowSubclass(edit, Some(size_edit_subclass), 1, 0);
            }
            controls.push(edit);
            controls.push(create(w!("STATIC"), "px", hidden | WINDOW_STYLE(SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(RESET), fonts.ui));
        }
        Kind::Photograph => {
            controls.push(label("Photograph", fonts.ui));
            let value = create(w!("STATIC"), "", hidden | WINDOW_STYLE(SS_RIGHT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            super::dialogs::tooltip(value, "The exported photograph, cropped, before rotation, the border, and any export size.");
            controls.push(value);
        }
        Kind::SizeNote => {
            controls.push(create(w!("STATIC"), "", hidden | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(LABEL), fonts.small));
        }
        Kind::Choice(title, _, options) => {
            controls.push(label(title, fonts.ui));
            let c = combo(false);
            for option in options {
                let text = wide(state::option_title(option));
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            controls.push(c);
        }
        Kind::Stock(paper) => {
            controls.push(label(if paper { "Paper" } else { "Film" }, fonts.ui));
            controls.push(combo(true));
        }
        Kind::DevTime(paper) => {
            controls.push(label(if paper { "Print development" } else { "Development" }, fonts.ui));
            controls.push(combo(false));
        }
        Kind::Foldout(paper) => {
            let title = if paper { "Paper" } else { "Film" };
            let header = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(ACTION), fonts.ui);
            unsafe {
                SetWindowLongPtrW(header, GWLP_USERDATA, (paper as isize) << 1);
            }
            super::dialogs::tooltip(header, &format!("Show each {} on your photograph", title.to_lowercase()));
            controls.push(header);
        }
        Kind::BorderStyles => {
            for (i, style) in border::STYLES.iter().enumerate() {
                let b = create(w!("BUTTON"), border::title(*style), hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.small);
                super::dialogs::tooltip(b, border::help(*style));
                controls.push(b);
            }
        }
        Kind::Color(line) => {
            controls.push(label(if line { "Line color" } else { "Color" }, fonts.ui));
            let b = create(w!("BUTTON"), if line { "Line color" } else { "Color" }, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.ui);
            controls.push(b);
        }
    }
    ins.rows[index].controls = controls;
}

/// Tab order follows the rows, though they were created in another order.
fn fix_z_order(ins: &Inspector) {
    let mut previous = HWND_TOP;
    for row in &ins.rows {
        let grid = match row.kind {
            Kind::Foldout(paper) => ins.grids[paper as usize].as_ref().map(|g| g.window),
            _ => None,
        };
        for &c in row.controls.iter().chain(grid.iter()) {
            unsafe {
                let _ = SetWindowPos(c, previous, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            previous = c;
        }
    }
}

fn footer_height(ins: &Inspector, hwnd: HWND, width: i32) -> i32 {
    unsafe {
        let mut text = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut text);
        let dc = GetDC(hwnd);
        let font = HFONT(SendMessageW(hwnd, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _);
        let old = SelectObject(dc, font);
        let mut r = RECT { left: 0, top: 0, right: width, bottom: 0 };
        DrawTextW(dc, &mut text[..n as usize], &mut r, DT_CALCRECT | DT_WORDBREAK);
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
        r.bottom + ins.px(6.0)
    }
}

fn layout_rows(ins: &mut Inspector, model: &Model) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(ins.rows_panel, &mut client);
    }
    let width = client.right;
    let pad = ins.px(16.0);
    let indent = ins.px(14.0);
    let mut y = ins.px(4.0);
    let mut positions: Vec<(HWND, i32, i32, i32, i32)> = vec![];
    let mut grids_shown = [false; 2];
    for row in &ins.rows {
        if !row.shown || row.controls.is_empty() {
            continue;
        }
        let x = pad + if row.group.is_some() { indent } else { 0 };
        let right = width - pad;
        let c = &row.controls;
        let height = match row.kind {
            Kind::Section(_) => {
                positions.push((c[0], pad, y + ins.px(10.0), right - pad, ins.px(18.0)));
                ins.px(32.0)
            }
            Kind::Footer(_) | Kind::SizeNote => {
                let text_height = footer_height(ins, c[0], right - x);
                positions.push((c[0], x, y, right - x, text_height));
                text_height + ins.px(4.0)
            }
            Kind::LongestSide => {
                let label = ins.px(108.0).min((right - x) / 2);
                let (field, unit) = (ins.px(72.0), ins.px(22.0));
                positions.push((c[0], x, y + ins.px(6.0), label - ins.px(4.0), ins.px(18.0)));
                positions.push((c[1], right - unit - field, y + ins.px(2.0), field, ins.px(23.0)));
                positions.push((c[2], right - unit + ins.px(4.0), y + ins.px(6.0), unit - ins.px(4.0), ins.px(18.0)));
                ins.px(30.0)
            }
            Kind::Photograph => {
                let label = ins.px(108.0).min((right - x) / 2);
                positions.push((c[0], x, y + ins.px(4.0), label - ins.px(4.0), ins.px(18.0)));
                positions.push((c[1], x + label, y + ins.px(4.0), right - x - label, ins.px(18.0)));
                ins.px(26.0)
            }
            Kind::Group { enable, .. } => {
                let check = if enable.is_some() { ins.px(46.0) } else { 0 };
                positions.push((c[0], pad - ins.px(4.0), y + ins.px(2.0), right - pad + ins.px(4.0) - check, ins.px(26.0)));
                if enable.is_some() {
                    positions.push((c[1], right - check + ins.px(6.0), y + ins.px(5.0), check - ins.px(6.0), ins.px(20.0)));
                }
                ins.px(30.0)
            }
            Kind::Slider(_) | Kind::BorderSlider(..) => {
                let edit = ins.px(56.0);
                let reset = ins.px(22.0);
                positions.push((c[0], x, y + ins.px(2.0), right - x - reset, ins.px(18.0)));
                positions.push((c[3], right - reset, y, reset, ins.px(20.0)));
                positions.push((c[1], x - ins.px(6.0), y + ins.px(21.0), right - x - edit, ins.px(26.0)));
                positions.push((c[2], right - edit + ins.px(4.0), y + ins.px(22.0), edit - ins.px(4.0), ins.px(23.0)));
                ins.px(50.0)
            }
            Kind::Toggle(..) | Kind::LimitPreview | Kind::BorderToggle(..) => {
                positions.push((c[0], x, y + ins.px(3.0), right - x, ins.px(20.0)));
                ins.px(26.0)
            }
            Kind::BalancePrint(..) => {
                let button = ins.px(120.0).min(right - x);
                positions.push((c[0], right - button, y + ins.px(3.0), button, ins.px(26.0)));
                ins.px(34.0)
            }
            Kind::Foldout(paper) => {
                positions.push((c[0], pad - ins.px(4.0), y + ins.px(2.0), right - pad + ins.px(4.0), ins.px(26.0)));
                let header = ins.px(30.0);
                match &ins.grids[paper as usize] {
                    Some(grid) if model.previews.open(paper) => {
                        grids_shown[paper as usize] = true;
                        let height = layout_grid(grid, right - x, ins.dpi);
                        positions.push((grid.window, x, y + header, right - x, height));
                        header + height + ins.px(6.0)
                    }
                    _ => header,
                }
            }
            Kind::Choice(..) | Kind::Stock(_) | Kind::DevTime(_) | Kind::SizeMode | Kind::PreviewSpace => {
                let label = ins.px(108.0).min((right - x) / 2);
                positions.push((c[0], x, y + ins.px(6.0), label - ins.px(4.0), ins.px(18.0)));
                positions.push((c[1], x + label, y + ins.px(2.0), right - x - label, ins.px(300.0)));
                ins.px(32.0)
            }
            Kind::BorderStyles => {
                let each = (right - x) / 4;
                for (i, &b) in c.iter().enumerate() {
                    positions.push((b, x + each * i as i32, y + ins.px(2.0), each - ins.px(4.0), ins.px(68.0)));
                }
                ins.px(74.0)
            }
            Kind::Color(_) => {
                let label = ins.px(108.0);
                positions.push((c[0], x, y + ins.px(6.0), label, ins.px(18.0)));
                positions.push((c[1], x + label, y + ins.px(2.0), ins.px(64.0), ins.px(24.0)));
                ins.px(30.0)
            }
        };
        y += height;
    }
    for (grid, shown) in ins.grids.iter().zip(grids_shown) {
        if let Some(grid) = grid {
            unsafe {
                let _ = ShowWindow(grid.window, if shown { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }
    ins.content = y + ins.px(8.0);
    let visible = client.bottom;
    ins.scroll = ins.scroll.clamp(0, (ins.content - visible).max(0));
    unsafe {
        let mut defer = BeginDeferWindowPos(positions.len() as i32).ok();
        for (hwnd, x, y, w, h) in positions {
            if let Some(d) = defer {
                defer = DeferWindowPos(d, hwnd, None, x, y - ins.scroll, w.max(0), h.max(0), SWP_NOZORDER | SWP_NOACTIVATE).ok();
            }
        }
        if let Some(d) = defer {
            let _ = EndDeferWindowPos(d);
        }
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: ins.content,
            nPage: visible.max(0) as u32,
            nPos: ins.scroll,
            ..Default::default()
        };
        SetScrollInfo(ins.rows_panel, SB_VERT, &info, true);
        let _ = InvalidateRect(ins.rows_panel, None, true);
    }
}

fn scroll_to(ins: &mut Inspector, target: i32) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(ins.rows_panel, &mut client);
    }
    let target = target.clamp(0, (ins.content - client.bottom).max(0));
    if target == ins.scroll {
        return;
    }
    let delta = ins.scroll - target;
    ins.scroll = target;
    unsafe {
        ScrollWindowEx(ins.rows_panel, 0, delta, None, None, None, None, SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE);
        let info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_POS, nPos: target, ..Default::default() };
        SetScrollInfo(ins.rows_panel, SB_VERT, &info, true);
    }
}

fn set_text(hwnd: HWND, text: &str) {
    unsafe {
        let mut current = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut current);
        if String::from_utf16_lossy(&current[..n as usize]) != text {
            let w = wide(text);
            let _ = SetWindowTextW(hwnd, pcwstr(&w));
        }
    }
}

fn enable(hwnd: HWND, on: bool) {
    unsafe {
        if IsWindowEnabled(hwnd).as_bool() != on {
            let _ = EnableWindow(hwnd, on);
        }
    }
}

fn set_check(hwnd: HWND, on: bool) {
    unsafe {
        SendMessageW(hwnd, BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
    }
}

fn format(value: f64, step: f64, decimal: char) -> String {
    let text = state::format_value(value, step);
    if decimal != '.' { text.replace('.', &decimal.to_string()) } else { text }
}

fn slider_value(model: &Model, kind: &Kind) -> Option<(f64, f64, f64, f64, f64)> {
    match *kind {
        Kind::Slider(Adjustment::Slider { path, low, high, step, .. }) => {
            Some((json::number(model.value(path)), json::number(model.default_value(path)), low, high, step))
        }
        Kind::BorderSlider(_, field) => {
            let (low, high, step) = field.range();
            Some((field.get(&model.state.border) * 100.0, field.get(&crate::border::BorderSettings::default()) * 100.0, low, high, step))
        }
        _ => None,
    }
}

fn update_values(ins: &mut Inspector, model: &Model, expanded: &[bool]) {
    let catalog = model.has_catalog();
    let scanning = json::boolean(model.value("io.scan_film"));
    let photo = photo_size(model);
    SWATCHES.with(|s| s.set((model.state.border.color.colorref(), model.state.border.line_color.colorref(), border::STYLES.iter().position(|s| *s == model.state.border.style).unwrap_or(0) as u32)));
    // A group's controls are off while its effect is.
    let mut group_off = vec![false; ins.rows.len()];
    for (i, row) in ins.rows.iter().enumerate() {
        if let Kind::Group { enable: Some(path), .. } = row.kind {
            group_off[i] = !json::boolean(model.value(path));
        }
    }
    let decimal = ins.decimal;
    let catalog_size = model.stocks.len() + model.stocks.iter().map(|s| s.id.len()).sum::<usize>();
    let refill_stocks = ins.stocks_filled != catalog_size;
    for (i, row) in ins.rows.iter().enumerate() {
        if row.controls.is_empty() || !row.shown {
            continue;
        }
        let c = &row.controls;
        let on = catalog
            && !(row.print && scanning)
            && !(row.border && !model.state.border.enabled)
            && !row.group.is_some_and(|g| group_off[g]);
        match row.kind {
            Kind::Section(_) | Kind::Footer(_) => {
                if let Kind::Footer("") = row.kind {
                    if let Some((w, h)) = photo {
                        set_text(c[0], &format!("35mm needs a 3:2 or 3:4 photograph. This one is {}; crop it to 3:2 or 4:3 to use it.", state::ratio_name(w as f64, h as f64)));
                    }
                }
            }
            Kind::Group { enable, .. } => {
                unsafe {
                    let open = expanded[i] as isize;
                    if GetWindowLongPtrW(c[0], GWLP_USERDATA) != open {
                        SetWindowLongPtrW(c[0], GWLP_USERDATA, open);
                        let _ = InvalidateRect(c[0], None, false);
                    }
                }
                enable_all(c, catalog && !(row.print && scanning));
                if let Some(path) = enable {
                    set_check(c[1], json::boolean(model.value(path)));
                }
            }
            Kind::Slider(_) | Kind::BorderSlider(..) => {
                let Some((value, default, low, _high, step)) = slider_value(model, &row.kind) else { continue };
                if let Kind::Slider(Adjustment::Slider { title, path, .. }) = row.kind
                    && let Some(bw) = state::bw_title(path)
                {
                    set_text(c[0], if model.film_is_bw() { bw } else { title });
                }
                let pos = ((value - low) / step).round() as isize;
                unsafe {
                    if SendMessageW(c[1], TBM_GETPOS, WPARAM(0), LPARAM(0)).0 != pos {
                        SendMessageW(c[1], TBM_SETPOS, WPARAM(1), LPARAM(pos));
                    }
                    if GetFocus() != c[2] {
                        set_text(c[2], &format(value, step, decimal));
                    }
                    let modified = match row.kind {
                        Kind::Slider(Adjustment::Slider { path, .. }) => model.is_modified(path),
                        _ => (value - default).abs() > 1e-7,
                    };
                    let _ = ShowWindow(c[3], if modified && row.shown { SW_SHOWNA } else { SW_HIDE });
                    let tip = format!("Reset to {}", format(default, step, decimal));
                    super::dialogs::tooltip(c[3], &tip);
                }
                enable_all(c, on);
            }
            Kind::Toggle(_, path) => {
                set_check(c[0], json::boolean(model.value(path)));
                enable(c[0], on);
            }
            Kind::LimitPreview => {
                set_check(c[0], model.state.limit_preview);
                enable(c[0], on);
            }
            Kind::BalancePrint(title, _) => {
                set_text(c[0], if model.balancing_print { "Balancing…" } else { title });
                enable(c[0], on && model.can_balance_print());
            }
            Kind::PreviewSpace => {
                let selected = crate::model::COLOR_SPACES.iter().position(|s| *s == model.preview_space()).unwrap_or(0) as isize;
                unsafe {
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != selected {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(selected as usize), LPARAM(0));
                    }
                }
                enable_all(c, catalog);
            }
            Kind::SizeMode => {
                unsafe {
                    let selected = (model.size_mode() == state::SizeMode::LongestSide) as isize;
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != selected {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(selected as usize), LPARAM(0));
                    }
                }
                enable_all(c, catalog);
            }
            Kind::LongestSide => {
                // A rejected value stays for correction until it is fixed or Esc restores it.
                let focused = unsafe { GetFocus() } == c[1];
                if !focused && model.size_error.is_none() {
                    set_text(c[1], &state::size_target(&model.state.params).map_or_else(String::new, |v| v.to_string()));
                }
                enable_all(c, catalog);
            }
            Kind::Photograph => {
                let size = model.photograph_size();
                set_text(c[1], &if size.is_zero() {
                    "—".to_string()
                } else {
                    let mp = size.width * size.height / 1e6;
                    format!("{} × {} · {:.*} MP", size.width as u64, size.height as u64, if mp < 10.0 { 1 } else { 0 }, mp)
                });
            }
            Kind::SizeNote => {}
            Kind::BorderToggle(_, which) => {
                let b = model.state.border;
                set_check(c[0], [b.enabled, b.instagram, b.export][which as usize]);
                enable(c[0], catalog && (which == 0 || b.enabled));
            }
            Kind::Choice(_, path, options) => {
                let index = options.iter().position(|o| *o == json::string(model.value(path))).map_or(-1, |i| i as isize);
                unsafe {
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != index {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
                    }
                }
                enable_all(c, on);
            }
            Kind::Stock(paper) => {
                if refill_stocks || unsafe { SendMessageW(c[1], CB_GETCOUNT, WPARAM(0), LPARAM(0)).0 } == 0 {
                    fill_stocks(c[1], model, paper);
                }
                select_stock(c[1], if paper { &model.state.paper } else { &model.state.film }, model, paper);
                enable_all(c, catalog);
            }
            Kind::Foldout(paper) => {
                let open = model.previews.open(paper);
                let name = if paper { model.paper_name() } else { model.film_name() };
                let title = if paper { "Paper" } else { "Film" };
                let changed = FOLDOUT_NAMES.with(|n| std::mem::replace(&mut n.borrow_mut()[paper as usize], name.clone()) != name);
                unsafe {
                    let data = (paper as isize) << 1 | open as isize;
                    if changed || GetWindowLongPtrW(c[0], GWLP_USERDATA) != data {
                        SetWindowLongPtrW(c[0], GWLP_USERDATA, data);
                        let _ = InvalidateRect(c[0], None, false);
                        super::accessible_name(c[0], &format!("{title}: {name}, {}", if open { "expanded" } else { "collapsed" }));
                    }
                }
                enable_all(c, catalog);
                if let Some(grid) = &ins.grids[paper as usize] {
                    select_tile(grid, if paper { &model.state.paper } else { &model.state.film });
                    enable(grid.window, catalog);
                }
            }
            Kind::DevTime(paper) => {
                let times = stock_times(model, paper);
                if ins.dev_times[paper as usize] != times || unsafe { SendMessageW(c[1], CB_GETCOUNT, WPARAM(0), LPARAM(0)).0 } == 0 {
                    unsafe {
                        SendMessageW(c[1], CB_RESETCONTENT, WPARAM(0), LPARAM(0));
                        let default = wide("Profile default");
                        SendMessageW(c[1], CB_ADDSTRING, WPARAM(0), LPARAM(default.as_ptr() as isize));
                        for t in &times {
                            let text = wide(format!("{} min", state::format_value(*t, 0.01)));
                            SendMessageW(c[1], CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                        }
                    }
                }
                let path = if paper { "print_render.development_time" } else { "film_render.development_time" };
                let value = model.value(path).as_f64();
                let index = value.and_then(|v| times.iter().position(|t| (t - v).abs() < 1e-9)).map_or(0, |i| i + 1);
                unsafe {
                    SendMessageW(c[1], CB_SETCURSEL, WPARAM(index), LPARAM(0));
                }
                enable_all(c, catalog);
            }
            Kind::BorderStyles => {
                for (j, &b) in c.iter().enumerate() {
                    let fits = photo.is_none_or(|(w, h)| border::fits(border::STYLES[j], w, h));
                    enable(b, on && fits);
                    unsafe {
                        let _ = InvalidateRect(b, None, false);
                    }
                }
            }
            Kind::Color(_) => {
                enable_all(c, on);
                unsafe {
                    let _ = InvalidateRect(c[1], None, false);
                }
            }
        }
    }
    if refill_stocks {
        ins.stocks_filled = catalog_size;
    }
    ins.dev_times = [stock_times(model, false), stock_times(model, true)];
}

fn enable_all(controls: &[HWND], on: bool) {
    for &c in controls {
        enable(c, on);
    }
}

/// The film or paper list, with group names as headers that can't be chosen.
fn fill_stocks(combo: HWND, model: &Model, paper: bool) {
    let (films, papers) = (model.films(), model.papers());
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        for group in groups {
            let title = wide(group.title);
            let i = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(title.as_ptr() as isize)).0;
            SendMessageW(combo, CB_SETITEMDATA, WPARAM(i as usize), LPARAM(1));
            for stock in group.stocks {
                let name = wide(&stock.name);
                SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(name.as_ptr() as isize));
            }
        }
    }
}

/// The catalog id of the stock at `index` in the list, or None for a header.
fn stock_at(model: &Model, paper: bool, index: usize) -> Option<String> {
    let (films, papers) = (model.films(), model.papers());
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    let mut i = 0;
    for group in groups {
        if i == index {
            return None;
        }
        i += 1;
        for stock in group.stocks {
            if i == index {
                return Some(stock.id.clone());
            }
            i += 1;
        }
    }
    None
}

fn select_stock(combo: HWND, id: &str, model: &Model, paper: bool) {
    let count = unsafe { SendMessageW(combo, CB_GETCOUNT, WPARAM(0), LPARAM(0)).0 } as usize;
    let index = (0..count).find(|&i| stock_at(model, paper, i).as_deref() == Some(id)).map_or(-1, |i| i as isize);
    unsafe {
        if SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != index {
            SendMessageW(combo, CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
        }
    }
}

fn row_of(id: u16) -> Option<(usize, u16)> {
    (id >= FIRST_ID).then(|| (((id - FIRST_ID) / PARTS) as usize, (id - FIRST_ID) % PARTS))
}

/// Parses a typed number, with either decimal separator.
pub fn parse_number(text: &str) -> Option<f64> {
    // Reject malformed input rather than silently turning "1x2" into 12.
    let cleaned = text.trim().trim_end_matches('%').trim().replace(',', ".");
    cleaned.parse().ok().filter(|v: &f64| v.is_finite())
}

fn set_slider(app: &mut App, index: usize, value: f64) {
    let kind = app.inspector.rows[index].kind;
    match kind {
        Kind::Slider(Adjustment::Slider { path, low, high, step, .. }) => {
            app.model.set(path, json::num(state::snap(value, low, high, step)));
        }
        Kind::BorderSlider(_, field) => app.model.set_border_percent(field, value),
        _ => {}
    }
}

fn reset_row(app: &mut App, index: usize) {
    match app.inspector.rows[index].kind {
        Kind::Slider(adjustment) => app.model.reset_value(adjustment.path()),
        Kind::Toggle(_, path) | Kind::Choice(_, path, _) => app.model.reset_value(path),
        Kind::BorderSlider(_, field) => {
            let initial = field.get(&crate::border::BorderSettings::default());
            app.model.set_border(|b| field.set(b, initial), "Change Border");
        }
        _ => {}
    }
}

fn row_modified(app: &App, index: usize) -> bool {
    match app.inspector.rows[index].kind {
        Kind::Slider(a) => app.model.is_modified(a.path()),
        Kind::Toggle(_, path) | Kind::Choice(_, path, _) => app.model.is_modified(path),
        Kind::BorderSlider(_, field) => (field.get(&app.model.state.border) - field.get(&crate::border::BorderSettings::default())).abs() > 1e-9,
        _ => false,
    }
}

fn command(app: &mut App, id: u16, code: u32, hwnd: HWND) {
    if id == TAB_LAB || id == TAB_POST {
        let next = if id == TAB_LAB { "Lab" } else { "Post" };
        if app.model.prefs.inspector_tab != next {
            app.model.prefs.inspector_tab = next.into();
            app.model.prefs.save();
            app.inspector.scroll = 0;
            app.model.dirty |= dirty::STRUCTURE;
        }
        return;
    }
    if id == RESET_ALL {
        app.model.reset();
        return;
    }
    let Some((index, part)) = row_of(id) else { return };
    let Some(row) = app.inspector.rows.get(index) else { return };
    let kind = row.kind;
    let checked = || unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 };
    let selection = || unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    match (kind, part) {
        (Kind::Group { title, .. }, HEADER) if code == BN_CLICKED => {
            let open = !app.model.prefs.expanded(title);
            app.model.prefs.set_expanded(title, open);
            app.model.prefs.save();
            app.model.dirty |= dirty::STRUCTURE;
        }
        (Kind::Group { enable: Some(path), .. }, CHECK) if code == BN_CLICKED => app.model.set(path, Value::Bool(checked())),
        (Kind::Toggle(_, path), CHECK) if code == BN_CLICKED => app.model.set(path, Value::Bool(checked())),
        (Kind::LimitPreview, CHECK) if code == BN_CLICKED => app.model.limit_preview(checked()),
        (Kind::BalancePrint(..), ACTION) if code == BN_CLICKED => app.model.balance_print(),
        (Kind::PreviewSpace, COMBO) if code == CBN_SELCHANGE => {
            let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
            if let Some(space) = usize::try_from(index).ok().and_then(|i| crate::model::COLOR_SPACES.get(i)) {
                app.model.choose_preview_space(space);
            }
        }
        (Kind::SizeMode, COMBO) if code == CBN_SELCHANGE => {
            let longest = unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 } == 1;
            app.model.choose_image_size(if longest { state::SizeMode::LongestSide } else { state::SizeMode::Original });
        }
        (Kind::BorderToggle(_, which), CHECK) if code == BN_CLICKED => {
            let on = checked();
            app.model.set_border(
                |b| match which {
                    0 => b.enabled = on,
                    1 => b.instagram = on,
                    _ => b.export = on,
                },
                "Change Border",
            );
        }
        (Kind::Slider(_) | Kind::BorderSlider(..), RESET) if code == BN_CLICKED => reset_row(app, index),
        (_, LABEL) if code == STN_DBLCLK => reset_row(app, index),
        (Kind::Choice(_, path, options), COMBO) if code == CBN_SELCHANGE => {
            if let Some(option) = options.get(selection() as usize) {
                app.model.set(path, Value::String(option.to_string()));
            }
        }
        (Kind::Stock(paper), COMBO) if code == CBN_SELCHANGE => {
            let index = selection() as usize;
            match stock_at(&app.model, paper, index) {
                Some(stock) if paper => app.model.choose_paper(&stock),
                Some(stock) => app.model.choose_film(&stock),
                // A header: back to the current stock.
                None => app.model.dirty |= dirty::INSPECTOR,
            }
        }
        (Kind::Foldout(paper), ACTION) if code == BN_CLICKED => app.model.toggle_foldout(paper),
        (Kind::DevTime(paper), COMBO) if code == CBN_SELCHANGE => {
            let times = stock_times(&app.model, paper);
            let path = if paper { "print_render.development_time" } else { "film_render.development_time" };
            let value = match selection() {
                0 => Value::Null,
                i => times.get(i as usize - 1).map_or(Value::Null, |t| json::num(*t)),
            };
            app.model.set(path, value);
        }
        (Kind::BorderStyles, p) if code == BN_CLICKED => {
            let style = border::STYLES[p as usize];
            app.model.set_border(|b| b.style = style, "Change Border");
        }
        (Kind::Color(line), HEADER) if code == BN_CLICKED => {
            let current = if line { app.model.state.border.line_color } else { app.model.state.border.color };
            if let Some(color) = choose_color(app.main, current) {
                app.model.set_border_color(line, color);
            }
        }
        _ => {}
    }
}

/// The common colour dialog; one undo step per OK.
fn choose_color(owner: HWND, current: BorderColor) -> Option<BorderColor> {
    thread_local! {
        static CUSTOM: RefCell<[COLORREF; 16]> = RefCell::new([COLORREF(0xffffff); 16]);
    }
    CUSTOM.with(|custom| {
        let mut custom = custom.borrow_mut();
        let mut choose = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: owner,
            rgbResult: COLORREF(current.colorref()),
            lpCustColors: custom.as_mut_ptr(),
            Flags: CC_FULLOPEN | CC_RGBINIT,
            ..Default::default()
        };
        unsafe { ChooseColorW(&mut choose).as_bool() }.then(|| BorderColor::from_colorref(choose.rgbResult.0))
    })
}

/// A trackbar moved: a drag is one undo step with live frames; a click or
/// a key is one edit.
fn trackbar(app: &mut App, slider: HWND, code: u32) {
    let id = unsafe { GetDlgCtrlID(slider) } as u16;
    let Some((index, SLIDER)) = row_of(id) else { return };
    let kind = app.inspector.rows[index].kind;
    let Some((_, _, low, _, step)) = slider_value(&app.model, &kind) else { return };
    let pos = unsafe { SendMessageW(slider, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 } as f64;
    let value = low + pos * step;
    let action = if matches!(kind, Kind::BorderSlider(..)) { "Change Border" } else { "Change Adjustment" };
    match code {
        TB_THUMBTRACK => {
            app.model.begin_interaction();
            set_slider(app, index, value);
        }
        TB_ENDTRACK => app.model.end_interaction(action),
        _ => set_slider(app, index, value),
    }
}

fn context_menu(app: &mut App, control: HWND, x: i32, y: i32) {
    let id = unsafe { GetDlgCtrlID(control) } as u16;
    let Some((index, _)) = row_of(id) else { return };
    if !matches!(app.inspector.rows[index].kind, Kind::Slider(_) | Kind::BorderSlider(..) | Kind::Toggle(..) | Kind::Choice(..)) {
        return;
    }
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        let text = wide("&Reset to Default");
        let flags = if row_modified(app, index) { MF_STRING } else { MF_STRING | MF_GRAYED };
        let _ = AppendMenuW(menu, flags, 1, pcwstr(&text));
        let (x, y) = if x == -1 && y == -1 {
            let mut r = RECT::default();
            let _ = GetWindowRect(control, &mut r);
            (r.left, r.bottom)
        } else {
            (x, y)
        };
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, app.inspector.rows_panel, None);
        let _ = DestroyMenu(menu);
        if chosen.0 == 1 {
            reset_row(app, index);
        }
    }
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if code == BN_CLICKED {
                    with_app(|app| command(app, id, code, HWND(lparam.0 as *mut _)));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn rows_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                with_app(|app| command(app, id, code, HWND(lparam.0 as *mut _)));
                LRESULT(0)
            }
            WM_HSCROLL => {
                let code = (wparam.0 & 0xffff) as u32;
                let slider = HWND(lparam.0 as *mut _);
                with_app(|app| trackbar(app, slider, code));
                LRESULT(0)
            }
            WM_VSCROLL => {
                with_app(|app| {
                    let ins = &mut app.inspector;
                    let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_ALL, ..Default::default() };
                    let _ = GetScrollInfo(hwnd, SB_VERT, &mut info);
                    let line = ins.px(40.0);
                    let target = match SCROLLBAR_COMMAND((wparam.0 & 0xffff) as i32) {
                        SB_LINEUP => info.nPos - line,
                        SB_LINEDOWN => info.nPos + line,
                        SB_PAGEUP => info.nPos - info.nPage as i32,
                        SB_PAGEDOWN => info.nPos + info.nPage as i32,
                        SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
                        SB_TOP => 0,
                        SB_BOTTOM => i32::MAX / 2,
                        _ => info.nPos,
                    };
                    scroll_to(ins, target);
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                with_app(|app| {
                    let ins = &mut app.inspector;
                    let target = ins.scroll - delta * ins.px(60.0) / 120;
                    scroll_to(ins, target);
                });
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                let control = HWND(wparam.0 as *mut _);
                let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
                if control != hwnd {
                    with_app(|app| context_menu(app, control, x, y));
                }
                LRESULT(0)
            }
            WM_MEASUREITEM => {
                let item = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
                item.itemHeight = ITEM_HEIGHT.with(|h| h.get()) as u32;
                LRESULT(1)
            }
            WM_DRAWITEM => {
                draw_item(&*(lparam.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                let control = HWND(lparam.0 as *mut _);
                let id = GetDlgCtrlID(control) as u16;
                SetBkMode(dc, TRANSPARENT);
                let footer = row_of(id).is_some_and(|(index, _)| with_footer(index));
                SetTextColor(dc, if footer { COLORREF(0x6d6d6d) } else { COLORREF(GetSysColor(COLOR_WINDOWTEXT)) });
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Footers are drawn grey; a static row index tells which rows are footers.
fn with_footer(index: usize) -> bool {
    thread_local! {
        static FOOTERS: Vec<bool> = rows().iter().map(|r| matches!(r.kind, Kind::Footer(_))).collect();
    }
    FOOTERS.with(|f| f.get(index).copied().unwrap_or(false))
}

fn draw_item(item: &DRAWITEMSTRUCT) {
    unsafe {
        let dc = item.hDC;
        let r = item.rcItem;
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
        let mut text = [0u16; 128];
        if item.CtlType == ODT_COMBOBOX {
            if item.itemID == u32::MAX {
                return;
            }
            let header = SendMessageW(item.hwndItem, CB_GETITEMDATA, WPARAM(item.itemID as usize), LPARAM(0)).0 == 1;
            let n = SendMessageW(item.hwndItem, CB_GETLBTEXT, WPARAM(item.itemID as usize), LPARAM(text.as_mut_ptr() as isize)).0.max(0) as usize;
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0 && !header;
            let in_edit = item.itemState.0 & ODS_COMBOBOXEDIT.0 != 0;
            FillRect(dc, &r, GetSysColorBrush(if selected { COLOR_HIGHLIGHT } else { COLOR_WINDOW }));
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(GetSysColor(if disabled { COLOR_GRAYTEXT } else if selected { COLOR_HIGHLIGHTTEXT } else if header { COLOR_GRAYTEXT } else { COLOR_WINDOWTEXT })));
            let indent = if header || in_edit { 4 } else { 14 };
            let mut tr = RECT { left: r.left + indent, ..r };
            let font = SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0));
            let old = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
            DrawTextW(dc, &mut text[..n], &mut tr, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            SelectObject(dc, old);
            if focused && in_edit {
                let _ = DrawFocusRect(dc, &r);
            }
            return;
        }
        let id = GetDlgCtrlID(item.hwndItem) as u16;
        let Some((_, part)) = row_of(id) else { return };
        let n = GetWindowTextW(item.hwndItem, &mut text) as usize;
        FillRect(dc, &r, GetSysColorBrush(COLOR_BTNFACE));
        SetBkMode(dc, TRANSPARENT);
        let label_color = COLORREF(GetSysColor(if disabled { COLOR_GRAYTEXT } else { COLOR_WINDOWTEXT }));
        if part == ACTION {
            draw_foldout(item, &mut text[..n], label_color);
            return;
        }
        if part == HEADER && n > 0 && !["Color", "Line color"].contains(&String::from_utf16_lossy(&text[..n]).as_str()) {
            // A group header: chevron, then the title.
            let open = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA) != 0;
            let glyph_font = with_app_fonts_glyph();
            let old = SelectObject(dc, glyph_font);
            SetTextColor(dc, label_color);
            let mut chevron: Vec<u16> = (if open { "\u{E70D}" } else { "\u{E76C}" }).encode_utf16().collect();
            let height = r.bottom - r.top;
            let mut cr = RECT { left: r.left + 2, top: r.top, right: r.left + height, bottom: r.bottom };
            DrawTextW(dc, &mut chevron, &mut cr, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
            SelectObject(dc, old);
            let font = SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0));
            let old = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
            let mut tr = RECT { left: r.left + height + 2, ..r };
            DrawTextW(dc, &mut text[..n], &mut tr, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            SelectObject(dc, old);
            if focused {
                let _ = DrawFocusRect(dc, &r);
            }
            return;
        }
        let (mat, line, selected) = SWATCHES.with(|s| s.get());
        if part == HEADER {
            // A colour swatch button.
            let is_line = String::from_utf16_lossy(&text[..n]) == "Line color";
            let color = if is_line { line } else { mat };
            let pushed = item.itemState.0 & ODS_SELECTED.0 != 0;
            DrawFrameControl(dc, &mut r.clone(), DFC_BUTTON, DFCS_BUTTONPUSH | if pushed { DFCS_PUSHED } else { DFCS_STATE(0) });
            let inner = RECT { left: r.left + 5, top: r.top + 5, right: r.right - 5, bottom: r.bottom - 5 };
            let brush = CreateSolidBrush(COLORREF(if disabled { GetSysColor(COLOR_BTNSHADOW) } else { color }));
            FillRect(dc, &inner, brush);
            let _ = DeleteObject(brush);
            FrameRect(dc, &inner, GetSysColorBrush(COLOR_BTNSHADOW));
            if focused {
                let _ = DrawFocusRect(dc, &RECT { left: r.left + 3, top: r.top + 3, right: r.right - 3, bottom: r.bottom - 3 });
            }
            return;
        }
        // A border style sample with its caption.
        let style = part as usize;
        let is_selected = style as u32 == selected;
        let caption = (r.bottom - r.top) / 4;
        let sample = RECT { left: r.left + 6, top: r.top + 4, right: r.right - 6, bottom: r.bottom - caption - 4 };
        if is_selected {
            let accent = super::canvas::accent_color();
            let pen = CreatePen(PS_SOLID, 2, COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16));
            let old = SelectObject(dc, pen);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            let _ = RoundRect(dc, sample.left - 3, sample.top - 3, sample.right + 3, sample.bottom + 3, 8, 8);
            SelectObject(dc, old_brush);
            SelectObject(dc, old);
            let _ = DeleteObject(pen);
        }
        draw_swatch(dc, sample, style, mat, line, disabled);
        let font = SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0));
        let old = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
        SetTextColor(dc, label_color);
        let mut tr = RECT { top: r.bottom - caption - 2, ..r };
        DrawTextW(dc, &mut text[..n], &mut tr, DT_SINGLELINE | DT_CENTER | DT_TOP);
        SelectObject(dc, old);
        if focused {
            let _ = DrawFocusRect(dc, &r);
        }
    }
}

fn with_app_fonts_glyph() -> HGDIOBJ {
    thread_local! {
        static GLYPH: Cell<isize> = const { Cell::new(0) };
    }
    GLYPH.with(|g| {
        if g.get() == 0 {
            unsafe {
                let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(super::main_hwnd()).max(96) as i32;
                let mut lf = LOGFONTW { lfHeight: -(10 * dpi / 96), ..Default::default() };
                let face = wide("Segoe MDL2 Assets");
                lf.lfFaceName[..face.len()].copy_from_slice(&face);
                g.set(CreateFontIndirectW(&lf).0 as isize);
            }
        }
        HGDIOBJ(g.get() as *mut _)
    })
}

/// The drawn samples of `BorderStylePicker`: a photograph (a sky-to-sand
/// gradient) inside each style.
fn draw_swatch(dc: HDC, r: RECT, style: usize, mat: u32, line: u32, disabled: bool) {
    unsafe {
        let fill = |rect: RECT, color: u32| {
            let brush = CreateSolidBrush(COLORREF(color));
            FillRect(dc, &rect, brush);
            let _ = DeleteObject(brush);
        };
        let inset = |rect: RECT, d: i32| RECT { left: rect.left + d, top: rect.top + d, right: rect.right - d, bottom: rect.bottom - d };
        let photo = |rect: RECT| {
            let vertices = [
                TRIVERTEX { x: rect.left, y: rect.top, Red: 0x8c00, Green: 0xad00, Blue: 0xc700, Alpha: 0 },
                TRIVERTEX { x: rect.right, y: rect.bottom, Red: 0xdb00, Green: 0xbd00, Blue: 0x8f00, Alpha: 0 },
            ];
            let rect_index = GRADIENT_RECT { UpperLeft: 0, LowerRight: 1 };
            let _ = GradientFill(dc, &vertices, &rect_index as *const _ as *const _, 1, GRADIENT_FILL_RECT_V);
        };
        let s = ((r.bottom - r.top) / 9).max(2);
        match style {
            0 => {
                fill(r, mat);
                photo(inset(r, s + 1));
            }
            1 => {
                fill(r, 0);
                photo(inset(r, s / 2 + 1));
            }
            2 => {
                fill(r, mat);
                let inner = inset(r, s);
                fill(inner, line);
                photo(inset(inner, 1));
            }
            _ => {
                fill(r, 0);
                let band = (r.bottom - r.top) / 5;
                for edge in [r.top + band / 4, r.bottom - band * 3 / 4] {
                    let mut x = r.left + 2;
                    while x + 3 < r.right {
                        fill(RECT { left: x, top: edge, right: x + 3, bottom: edge + band / 2 }, 0xf2f2f2);
                        x += 6;
                    }
                }
                photo(RECT { left: r.left + 1, top: r.top + band, right: r.right - 1, bottom: r.bottom - band });
            }
        }
        FrameRect(dc, &r, GetSysColorBrush(COLOR_BTNSHADOW));
        if disabled {
            // Faded like the Mac's disabled samples.
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 160, AlphaFormat: 0 };
            let mem = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, 1, 1);
            let old = SelectObject(mem, bitmap);
            let brush = GetSysColorBrush(COLOR_BTNFACE);
            FillRect(mem, &RECT { left: 0, top: 0, right: 1, bottom: 1 }, brush);
            let _ = AlphaBlend(dc, r.left, r.top, r.right - r.left, r.bottom - r.top, mem, 0, 0, 1, 1, blend);
            SelectObject(mem, old);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem);
        }
    }
}

thread_local! {
    /// The last click on a trackbar: its window, time and position.
    static LAST_TRACKBAR_CLICK: Cell<Option<(usize, u32, i32, i32)>> = const { Cell::new(None) };
    /// A trackbar whose reset click's button-up it must not see.
    static RESET_TRACKBAR: Cell<usize> = const { Cell::new(0) };
}

/// Whether this click on a trackbar completes a double-click. Trackbars
/// may not have CS_DBLCLKS, so this checks the time and distance as
/// Windows does.
unsafe fn second_click(hwnd: HWND, lparam: LPARAM) -> bool {
    let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
    let time = unsafe { GetMessageTime() } as u32;
    let last = LAST_TRACKBAR_CLICK.replace(Some((hwnd.0 as usize, time, x, y)));
    let double = last.is_some_and(|(window, at, lx, ly)| unsafe {
        window == hwnd.0 as usize
            && time.wrapping_sub(at) <= GetDoubleClickTime()
            && (x - lx).abs() <= GetSystemMetrics(SM_CXDOUBLECLK) / 2
            && (y - ly).abs() <= GetSystemMetrics(SM_CYDOUBLECLK) / 2
    });
    if double {
        LAST_TRACKBAR_CLICK.set(None);
    }
    double
}

/// Mouse wheel over a trackbar scrolls the panel unless it has focus.
/// Double-click or Alt-click resets it, as in Lightroom (macOS
/// `ResettableSlider`), by clicking the row's reset button, whose id is the
/// subclass's data. A double-click's first click still moves the thumb.
extern "system" fn trackbar_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, reset: usize) -> LRESULT {
    unsafe {
        if msg == WM_MOUSEWHEEL && GetFocus() != hwnd {
            return SendMessageW(GetParent(hwnd).unwrap_or_default(), msg, wparam, lparam);
        }
        if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) {
            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
            let double = msg == WM_LBUTTONDBLCLK || second_click(hwnd, lparam);
            if (alt || double) && IsWindowEnabled(hwnd).as_bool() {
                RESET_TRACKBAR.set(hwnd.0 as usize);
                let parent = GetParent(hwnd).unwrap_or_default();
                let button = GetDlgItem(parent, reset as i32).unwrap_or_default();
                SendMessageW(parent, WM_COMMAND, WPARAM(((BN_CLICKED as usize) << 16) | reset), LPARAM(button.0 as isize));
                return LRESULT(0);
            }
        }
        if msg == WM_LBUTTONUP && RESET_TRACKBAR.get() == hwnd.0 as usize {
            RESET_TRACKBAR.set(0);
            return LRESULT(0);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// A slider's value box: Return and leaving commit, Escape restores.
extern "system" fn edit_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_GETDLGCODE if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_RETURN | VK_ESCAPE) => LRESULT(DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                commit_edit(hwnd);
                SendMessageW(hwnd, EM_SETSEL, WPARAM(0), LPARAM(-1));
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                post(move |app: &mut App| {
                    app.model.dirty |= dirty::INSPECTOR;
                    let _ = SetFocus(app.inspector.rows_panel);
                });
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            WM_KILLFOCUS => {
                commit_edit(hwnd);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_SETFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                SendMessageW(hwnd, EM_SETSEL, WPARAM(0), LPARAM(-1));
                result
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// The Longest side field: Enter or leaving it applies the value; Esc
/// restores the current one. An invalid value stays, with its error shown.
extern "system" fn size_edit_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_GETDLGCODE if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_RETURN | VK_ESCAPE) => LRESULT(DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                commit_size(hwnd);
                SendMessageW(hwnd, EM_SETSEL, WPARAM(0), LPARAM(-1));
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                let raw = hwnd.0 as isize;
                post(move |app: &mut App| {
                    app.model.size_error = None;
                    let hwnd = HWND(raw as *mut _);
                    set_text(hwnd, &state::size_target(&app.model.state.params).map_or_else(String::new, |v| v.to_string()));
                    app.model.dirty |= dirty::INSPECTOR;
                    let _ = SetFocus(app.inspector.rows_panel);
                });
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            WM_KILLFOCUS => {
                commit_size(hwnd);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

fn commit_size(hwnd: HWND) {
    let text = window_text(hwnd);
    post(move |app: &mut App| {
        let digits: String = text.chars().filter(|c| !matches!(c, ',' | '.' | ' ' | '\u{a0}' | '\u{202f}' | '\'')).collect();
        let current = state::size_target(&app.model.state.params);
        let pixels = digits.parse::<u32>().unwrap_or(0);
        // Leaving an unchanged field changes nothing.
        if current == Some(pixels) && app.model.size_error.is_none() {
            return;
        }
        let _ = app.model.set_longest_side(pixels);
        app.model.dirty |= dirty::INSPECTOR;
    });
}

fn commit_edit(hwnd: HWND) {
    let mut text = [0u16; 64];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
    let text = String::from_utf16_lossy(&text[..n]);
    let id = unsafe { GetDlgCtrlID(hwnd) } as u16;
    post(move |app: &mut App| {
        if let Some((index, EDIT)) = row_of(id) {
            match parse_number(&text) {
                Some(value) => set_slider(app, index, value),
                None => {}
            }
        }
        // Show the stepped value, or restore an invalid one.
        app.model.dirty |= dirty::INSPECTOR;
        let _ = app;
    });
}

// MARK: Film and paper previews

/// An open foldout: headings and a two-column grid of tiles, one owner-drawn
/// button per stock, under the menu's headings in catalog order.
struct StockGrid {
    window: HWND,
    /// Each tile's stock id and button, in order.
    tiles: Vec<(String, HWND)>,
    /// Each heading and the index of the first tile under it.
    headings: Vec<(HWND, usize)>,
    /// The catalog it was built from.
    catalog: usize,
}

/// A tile as drawn.
#[derive(Clone, Default)]
struct TileLook {
    image: Option<std::sync::Arc<crate::previews::Bitmap>>,
    dimmed: bool,
    selected: bool,
}

const TILE_FIRST: u16 = 100;
/// The thumbnail's height, the caption and the gaps, in DIPs.
const TILE_IMAGE: f64 = 92.0;
const TILE_CAPTION: f64 = 18.0;
const TILE_GAP: f64 = 8.0;
const TILE_HEADING: f64 = 24.0;

fn build_grid(parent: HWND, fonts: &super::Fonts, model: &Model, paper: bool, catalog: usize) -> StockGrid {
    let window = create(w!("NegsStockGrid"), if paper { "Papers" } else { "Films" }, WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), parent, 0, HFONT::default());
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, paper as isize);
    }
    let (films, papers) = (model.films(), model.papers());
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    let (mut tiles, mut headings) = (vec![], vec![]);
    for group in groups {
        let heading = create(w!("STATIC"), group.title, WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), window, 0, fonts.small);
        headings.push((heading, tiles.len()));
        for stock in group.stocks {
            let id = TILE_FIRST + tiles.len() as u16;
            let tile = create(w!("BUTTON"), &stock.name, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), window, id, fonts.small);
            super::dialogs::tooltip(tile, &stock.name);
            unsafe {
                let _ = SetWindowSubclass(tile, Some(tile_subclass), 1, 0);
            }
            tiles.push((stock.id.clone(), tile));
        }
    }
    // Thumbnails made before the grid was (re)built show at once.
    let looks = tiles
        .iter()
        .map(|(id, _)| {
            let thumb = model.previews.thumb(&crate::previews::Target::Stock { id: id.clone(), paper });
            TileLook { image: thumb.and_then(|t| t.image.clone()), dimmed: thumb.is_some_and(|t| t.dimmed), selected: false }
        })
        .collect();
    TILES.with(|t| t.borrow_mut()[paper as usize] = looks);
    let grid = StockGrid { window, tiles, headings, catalog };
    select_tile(&grid, if paper { &model.state.paper } else { &model.state.film });
    grid
}

/// Places the headings and tiles for `width`; returns the grid's height.
fn layout_grid(grid: &StockGrid, width: i32, dpi: u32) -> i32 {
    let px = |dip: f64| (dip * dpi as f64 / 96.0).round() as i32;
    let gap = px(TILE_GAP);
    let column = ((width - gap) / 2).max(1);
    let tile = px(TILE_IMAGE + TILE_CAPTION);
    let mut y = 0;
    unsafe {
        for (g, &(heading, first)) in grid.headings.iter().enumerate() {
            let end = grid.headings.get(g + 1).map_or(grid.tiles.len(), |h| h.1);
            let _ = MoveWindow(heading, 0, y + px(6.0), width, px(16.0), false);
            y += px(TILE_HEADING);
            for (i, (_, button)) in grid.tiles[first..end].iter().enumerate() {
                let (col, row) = (i as i32 % 2, i as i32 / 2);
                let _ = MoveWindow(*button, col * (column + gap), y + row * (tile + gap), column, tile, false);
            }
            let rows = (end - first).div_ceil(2) as i32;
            y += rows * tile + (rows - 1).max(0) * gap + px(2.0);
        }
        let _ = InvalidateRect(grid.window, None, true);
    }
    y
}

/// Outlines the current stock's tile, and names it for screen readers.
fn select_tile(grid: &StockGrid, selected: &str) {
    let paper = unsafe { GetWindowLongPtrW(grid.window, GWLP_USERDATA) } != 0;
    TILES.with(|t| {
        let mut t = t.borrow_mut();
        let looks = &mut t[paper as usize];
        for (i, (id, button)) in grid.tiles.iter().enumerate() {
            let Some(look) = looks.get_mut(i) else { continue };
            let now = id == selected;
            if look.selected != now {
                look.selected = now;
                let name = window_text(*button);
                super::accessible_name(*button, &if now { format!("{name}, current") } else { name });
                unsafe {
                    let _ = InvalidateRect(*button, None, false);
                }
            }
        }
    });
}

/// Brings the open grids' tiles up to date with the model's thumbnails.
pub fn redraw_previews(app: &mut App) {
    let ins = &app.inspector;
    for (paper, grid) in ins.grids.iter().enumerate() {
        let Some(grid) = grid else { continue };
        if !unsafe { IsWindowVisible(grid.window) }.as_bool() {
            continue;
        }
        TILES.with(|t| {
            let mut t = t.borrow_mut();
            for (look, (id, button)) in t[paper].iter_mut().zip(&grid.tiles) {
                let thumb = app.model.previews.thumb(&crate::previews::Target::Stock { id: id.clone(), paper: paper == 1 });
                let image = thumb.and_then(|t| t.image.clone());
                let dimmed = thumb.is_some_and(|t| t.dimmed);
                let same = look.dimmed == dimmed
                    && match (&look.image, &image) {
                        (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    };
                if !same {
                    look.image = image;
                    look.dimmed = dimmed;
                    unsafe {
                        let _ = InvalidateRect(*button, None, false);
                    }
                }
            }
        });
    }
}

/// A foldout's header: chevron and title, the current stock at the right.
fn draw_foldout(item: &DRAWITEMSTRUCT, title: &mut [u16], color: COLORREF) {
    unsafe {
        let (dc, r) = (item.hDC, item.rcItem);
        let data = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA);
        let (open, paper) = (data & 1 != 0, data & 2 != 0);
        let height = r.bottom - r.top;
        let old = SelectObject(dc, with_app_fonts_glyph());
        SetTextColor(dc, color);
        let mut chevron: Vec<u16> = (if open { "\u{E70D}" } else { "\u{E76C}" }).encode_utf16().collect();
        let mut cr = RECT { left: r.left + 2, top: r.top, right: r.left + height, bottom: r.bottom };
        DrawTextW(dc, &mut chevron, &mut cr, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
        SelectObject(dc, HGDIOBJ(SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _));
        let mut measure = RECT::default();
        DrawTextW(dc, title, &mut measure, DT_SINGLELINE | DT_CALCRECT);
        let mut tr = RECT { left: r.left + height + 2, ..r };
        DrawTextW(dc, title, &mut tr, DT_SINGLELINE | DT_VCENTER);
        let mut name: Vec<u16> = FOLDOUT_NAMES.with(|n| n.borrow()[paper as usize].encode_utf16().collect());
        if !name.is_empty() {
            SetTextColor(dc, COLORREF(if item.itemState.0 & ODS_DISABLED.0 != 0 { GetSysColor(COLOR_GRAYTEXT) } else { 0x6d6d6d }));
            let mut nr = RECT { left: r.left + height + 2 + measure.right + height / 2, right: r.right - 4, ..r };
            DrawTextW(dc, &mut name, &mut nr, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS | DT_NOPREFIX);
        }
        SelectObject(dc, old);
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            let _ = DrawFocusRect(dc, &r);
        }
    }
}

/// A tile: the thumbnail cropped to fill a rounded frame (grey until it
/// arrives, dimmed while out of date), the accent outline on the current
/// stock, and the name below, cut with an ellipsis.
fn draw_tile(item: &DRAWITEMSTRUCT, look: &TileLook) {
    unsafe {
        let (dc, r) = (item.hDC, item.rcItem);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(item.hwndItem).max(96) as i32;
        let px = |dip: f64| (dip * dpi as f64 / 96.0).round() as i32;
        FillRect(dc, &r, GetSysColorBrush(COLOR_BTNFACE));
        let inset = px(2.0);
        let frame = RECT { left: r.left + inset, top: r.top + inset, right: r.right - inset, bottom: r.top + px(TILE_IMAGE) - inset };
        let radius = px(10.0);
        draw_thumbnail(dc, frame, radius, look.image.as_deref(), look.dimmed, COLORREF(0xdedede));
        if look.selected {
            let accent = super::canvas::accent_color();
            let pen = CreatePen(PS_SOLID, px(2.0), COLORREF(accent[0] as u32 | (accent[1] as u32) << 8 | (accent[2] as u32) << 16));
            let old = SelectObject(dc, pen);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            let _ = RoundRect(dc, frame.left, frame.top, frame.right, frame.bottom, radius, radius);
            SelectObject(dc, old_brush);
            SelectObject(dc, old);
            let _ = DeleteObject(pen);
        }
        let mut text = [0u16; 128];
        let n = GetWindowTextW(item.hwndItem, &mut text) as usize;
        let font = SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0));
        let old = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
        SetBkMode(dc, TRANSPARENT);
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        SetTextColor(dc, COLORREF(if disabled { GetSysColor(COLOR_GRAYTEXT) } else if look.selected { GetSysColor(COLOR_WINDOWTEXT) } else { 0x5a5a5a }));
        let mut tr = RECT { left: r.left + inset, top: frame.bottom + px(2.0), right: r.right - inset, bottom: r.bottom };
        DrawTextW(dc, &mut text[..n], &mut tr, DT_SINGLELINE | DT_TOP | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX);
        SelectObject(dc, old);
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            let _ = DrawFocusRect(dc, &RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
        }
    }
}

/// A thumbnail cropped to fill `frame` with rounded corners, or the grey
/// placeholder; dimmed (55 % opacity over the background) while out of date.
pub fn draw_thumbnail(dc: HDC, frame: RECT, radius: i32, image: Option<&crate::previews::Bitmap>, dimmed: bool, placeholder: COLORREF) {
    unsafe {
        let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
        if fw <= 0 || fh <= 0 {
            return;
        }
        let region = CreateRoundRectRgn(frame.left, frame.top, frame.right + 1, frame.bottom + 1, radius, radius);
        let _ = SelectClipRgn(dc, region);
        let brush = CreateSolidBrush(placeholder);
        FillRect(dc, &frame, brush);
        let _ = DeleteObject(brush);
        if let Some(image) = image {
            // Cover: the largest centred part of the image with the frame's shape.
            let (iw, ih) = (image.width as f64, image.height as f64);
            let scale = (fw as f64 / iw).max(fh as f64 / ih);
            let (sw, sh) = ((fw as f64 / scale).min(iw), (fh as f64 / scale).min(ih));
            let (sx, sy) = ((iw - sw) / 2.0, (ih - sh) / 2.0);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: image.width as i32,
                    biHeight: -(image.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let previous = SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
            StretchDIBits(
                dc,
                frame.left,
                frame.top,
                fw,
                fh,
                sx.round() as i32,
                sy.round() as i32,
                sw.round() as i32,
                sh.round() as i32,
                Some(image.bgra.as_ptr() as *const _),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            SetStretchBltMode(dc, STRETCH_BLT_MODE(previous));
            if dimmed {
                let mem = CreateCompatibleDC(dc);
                let bitmap = CreateCompatibleBitmap(dc, 1, 1);
                let old = SelectObject(mem, bitmap);
                FillRect(mem, &RECT { left: 0, top: 0, right: 1, bottom: 1 }, GetSysColorBrush(COLOR_BTNFACE));
                let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 115, AlphaFormat: 0 };
                let _ = AlphaBlend(dc, frame.left, frame.top, fw, fh, mem, 0, 0, 1, 1, blend);
                SelectObject(mem, old);
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(mem);
            }
        }
        let _ = SelectClipRgn(dc, None);
        let _ = DeleteObject(region);
    }
}

/// Enter chooses a focused tile, as Space does: owner-drawn buttons aren't
/// push buttons to `IsDialogMessage`.
extern "system" fn tile_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        let enter = wparam.0 == VK_RETURN.0 as usize;
        if msg == WM_GETDLGCODE && enter {
            return LRESULT((DLGC_WANTMESSAGE | DLGC_BUTTON) as isize);
        }
        if msg == WM_KEYDOWN && enter {
            let parent = GetParent(hwnd).unwrap_or_default();
            SendMessageW(parent, WM_COMMAND, WPARAM(((BN_CLICKED as usize) << 16) | GetDlgCtrlID(hwnd) as usize), LPARAM(hwnd.0 as isize));
            return LRESULT(0);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

extern "system" fn grid_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let paper = GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0;
        match msg {
            WM_COMMAND if ((wparam.0 >> 16) & 0xffff) as u32 == BN_CLICKED => {
                let index = (wparam.0 & 0xffff) as u16;
                with_app(|app| {
                    let Some(index) = index.checked_sub(TILE_FIRST) else { return };
                    let Some(id) = app.inspector.grids[paper as usize].as_ref().and_then(|g| g.tiles.get(index as usize)).map(|t| t.0.clone()) else { return };
                    if paper { app.model.choose_paper(&id) } else { app.model.choose_film(&id) }
                });
                LRESULT(0)
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                let index = (item.CtlID as u16).wrapping_sub(TILE_FIRST) as usize;
                let look = TILES.with(|t| t.borrow()[paper as usize].get(index).cloned()).unwrap_or_default();
                draw_tile(item, &look);
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x6d6d6d));
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The histogram: red, green and blue added on a dark well, as in photo
/// editors; overlaps mix to cyan, magenta, yellow and white.
extern "system" fn histogram_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let (w, h) = (r.right.max(1) as usize, r.bottom.max(1) as usize);
                let mut pixels = vec![0u32; w * h];
                let well = 0x1a1a1a;
                pixels.iter_mut().for_each(|p| *p = well);
                HISTOGRAM.with(|histogram| {
                    let Some(histogram) = &*histogram.borrow() else { return };
                    let peak = histogram.peak() as f64;
                    if peak <= 0.0 {
                        return;
                    }
                    let inset = 4;
                    let top = 6;
                    let plot_w = w.saturating_sub(2 * inset).max(1);
                    let plot_h = h.saturating_sub(top).max(1);
                    for x in 0..plot_w {
                        let bin = x as f64 * 255.0 / (plot_w - 1).max(1) as f64;
                        let (i, f) = (bin.floor() as usize, bin.fract());
                        let at = |bins: &[u32; 256]| {
                            let a = bins[i.min(255)] as f64;
                            let b = bins[(i + 1).min(255)] as f64;
                            ((a * (1.0 - f) + b * f) / peak).min(1.0)
                        };
                        let heights = [at(&histogram.red), at(&histogram.green), at(&histogram.blue)];
                        let colors = [[1.0, 0.2, 0.2], [0.2, 1.0, 0.2], [0.25, 0.35, 1.0]];
                        for y in 0..plot_h {
                            let level = 1.0 - (y as f64 + 0.5) / plot_h as f64;
                            let mut c = [0.1f64, 0.1, 0.1];
                            for k in 0..3 {
                                if heights[k] >= level {
                                    for j in 0..3 {
                                        c[j] += colors[k][j] * 0.75;
                                    }
                                }
                            }
                            let byte = |v: f64| (v.min(1.0) * 255.0) as u32;
                            pixels[(top + y) * w + inset + x] = byte(c[0]) << 16 | byte(c[1]) << 8 | byte(c[2]);
                        }
                    }
                });
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
                let region = CreateRoundRectRgn(0, 0, w as i32 + 1, h as i32 + 1, 12, 12);
                FillRect(dc, &r, GetSysColorBrush(COLOR_BTNFACE));
                SelectClipRgn(dc, region);
                SetDIBitsToDevice(dc, 0, 0, w as u32, h as u32, 0, 0, 0, h as u32, pixels.as_ptr() as *const _, &info, DIB_RGB_COLORS);
                SelectClipRgn(dc, None);
                let _ = DeleteObject(region);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Logs where Exposure's trackbar is, for the startup probe.
pub fn report_geometry(app: &mut App) {
    if !app.report.active() {
        return;
    }
    let found = app.inspector.rows.iter().find(|r| matches!(r.kind, Kind::Slider(Adjustment::Slider { path: "camera.exposure_compensation_ev", .. })));
    if let Some(row) = found.filter(|r| r.shown && !r.controls.is_empty()) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(row.controls[1], &mut rect);
        }
        let line = format!("geometry exposure {} {} {} {}", rect.left, rect.top, rect.right, rect.bottom);
        app.report.line(&line);
    }
}

// MARK: Crop panel

/// Replaces the adjustments while cropping (`CropInspector`).
struct CropPanel {
    panel: HWND,
    controls: Vec<(u16, HWND)>,
}

const C_ASPECT: u16 = CROP_FIRST; // ..+8
const C_WIDTH: u16 = CROP_FIRST + 10;
const C_HEIGHT: u16 = CROP_FIRST + 11;
const C_LANDSCAPE: u16 = CROP_FIRST + 12;
const C_PORTRAIT: u16 = CROP_FIRST + 13;
const C_ANGLE: u16 = CROP_FIRST + 14;
const C_ANGLE_VALUE: u16 = CROP_FIRST + 15;
const C_ANGLE_RESET: u16 = CROP_FIRST + 16;
const C_SIZE: u16 = CROP_FIRST + 17;
const C_RESET: u16 = CROP_FIRST + 18;
const C_DONE: u16 = CROP_FIRST + 19;
const C_CANCEL: u16 = CROP_FIRST + 20;
const C_SIZE_NOTE: u16 = CROP_FIRST + 21;
const C_LABELS: u16 = CROP_FIRST + 30;

impl CropPanel {
    fn get(&self, id: u16) -> HWND {
        self.controls.iter().find(|(i, _)| *i == id).map(|(_, h)| *h).unwrap_or_default()
    }
}

fn build_crop(app: &mut App) {
    let fonts = &app.fonts;
    let parent = app.inspector.panel;
    let panel = create(w!("NegsCropPanel"), "Crop", WS_CHILD | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), parent, 0, HFONT::default());
    let mut controls = vec![];
    let mut add = |id: u16, class: PCWSTR, text: &str, style: WINDOW_STYLE, font: HFONT| {
        let c = create(class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), panel, id, font);
        controls.push((id, c));
        c
    };
    add(C_LABELS, w!("STATIC"), "Aspect Ratio", WINDOW_STYLE(0), fonts.bold);
    for (i, aspect) in CropAspect::ALL.iter().enumerate() {
        let text = match aspect.format() {
            Some(format) => format!("{}    {format}", aspect.title()),
            None => aspect.title().to_string(),
        };
        let group = if i == 0 { WS_GROUP | WS_TABSTOP } else { WINDOW_STYLE(0) };
        add(C_ASPECT + i as u16, w!("BUTTON"), &text, group | WINDOW_STYLE(BS_AUTORADIOBUTTON as u32), fonts.ui);
    }
    add(C_LABELS + 1, w!("STATIC"), "Ratio", WINDOW_STYLE(0), fonts.ui);
    for id in [C_WIDTH, C_HEIGHT] {
        let e = add(id, w!("EDIT"), "", WS_TABSTOP | WS_GROUP | WS_BORDER | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), fonts.ui);
        super::accessible_name(e, if id == C_WIDTH { "Custom crop ratio width" } else { "Custom crop ratio height" });
        unsafe {
            let _ = SetWindowSubclass(e, Some(ratio_subclass), 1, 0);
        }
    }
    add(C_LABELS + 2, w!("STATIC"), "×", WINDOW_STYLE(SS_CENTER.0), fonts.ui);
    add(C_LABELS + 3, w!("STATIC"), "Orientation", WINDOW_STYLE(0), fonts.ui);
    add(C_LANDSCAPE, w!("BUTTON"), "Landscape", WS_TABSTOP | WS_GROUP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32), fonts.ui);
    add(C_PORTRAIT, w!("BUTTON"), "Portrait", WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32), fonts.ui);
    add(C_LABELS + 4, w!("STATIC"), "Straighten", WS_GROUP, fonts.bold);
    add(C_LABELS + 5, w!("STATIC"), "Angle", WINDOW_STYLE(0), fonts.ui);
    add(C_ANGLE_VALUE, w!("STATIC"), "0.0°", WINDOW_STYLE(SS_RIGHT.0), fonts.ui);
    let reset = add(C_ANGLE_RESET, w!("BUTTON"), "\u{E777}", WINDOW_STYLE(BS_PUSHBUTTON as u32 | BS_FLAT as u32), fonts.glyph_small);
    super::dialogs::tooltip(reset, "Straighten to 0°");
    let angle = add(C_ANGLE, TRACKBAR_CLASSW, "Angle", WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), HFONT::default());
    unsafe {
        SendMessageW(angle, TBM_SETRANGEMIN, WPARAM(0), LPARAM(-450));
        SendMessageW(angle, TBM_SETRANGEMAX, WPARAM(0), LPARAM(450));
        SendMessageW(angle, TBM_SETLINESIZE, WPARAM(0), LPARAM(5));
        SendMessageW(angle, TBM_SETPAGESIZE, WPARAM(0), LPARAM(50));
        let _ = SetWindowSubclass(angle, Some(trackbar_subclass), 1, C_ANGLE_RESET as usize);
    }
    super::dialogs::tooltip(angle, "Turns the photograph to level a crooked horizon. The crop shrinks to stay on the photograph.");
    add(C_LABELS + 6, w!("STATIC"), "Size", WINDOW_STYLE(0), fonts.ui);
    let size = add(C_SIZE, w!("STATIC"), "", WINDOW_STYLE(SS_RIGHT.0), fonts.ui);
    super::dialogs::tooltip(size, "The exported photograph’s size, before the border.");
    add(C_SIZE_NOTE, w!("STATIC"), "", WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), fonts.small);
    let reset_crop = add(C_RESET, w!("BUTTON"), "Reset Crop", WS_TABSTOP | WS_GROUP, fonts.ui);
    super::dialogs::tooltip(reset_crop, "Show the whole, unturned photograph again.");
    add(
        C_LABELS + 7,
        w!("STATIC"),
        "Drag the corners or edges to crop, or drag inside to move it. Drag the dial below the photograph to straighten it. Press Enter to apply or Esc to cancel. The border is added after cropping.",
        WINDOW_STYLE(0),
        fonts.small,
    );
    add(C_CANCEL, w!("BUTTON"), "Cancel", WS_TABSTOP | WS_GROUP, fonts.ui);
    add(C_DONE, w!("BUTTON"), "Done", WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), fonts.ui);
    app.inspector.crop = Some(CropPanel { panel, controls });
    layout_crop(app);
}

fn layout_crop(app: &mut App) {
    let ins = &app.inspector;
    let Some(crop) = &ins.crop else { return };
    let (w, h) = (ins.rect.right - ins.rect.left, ins.rect.bottom - ins.rect.top);
    let pad = ins.px(16.0);
    let right = w - pad;
    let row = ins.px(24.0);
    let mut y = ins.px(12.0);
    let place = |id: u16, x: i32, y: i32, w: i32, h: i32| unsafe {
        let _ = MoveWindow(crop.get(id), x, y, w.max(0), h.max(0), true);
    };
    unsafe {
        let _ = MoveWindow(crop.panel, 0, 0, w, h, true);
    }
    place(C_LABELS, pad, y, right - pad, ins.px(18.0));
    y += ins.px(26.0);
    for i in 0..CropAspect::ALL.len() as u16 {
        place(C_ASPECT + i, pad + ins.px(4.0), y, right - pad, ins.px(20.0));
        y += row;
    }
    let custom = app.model.crop_session.as_ref().is_some_and(|s| s.aspect == CropAspect::Custom);
    for id in [C_LABELS + 1, C_WIDTH, C_LABELS + 2, C_HEIGHT] {
        unsafe {
            let _ = ShowWindow(crop.get(id), if custom { SW_SHOWNA } else { SW_HIDE });
        }
    }
    if custom {
        let field = ins.px(56.0);
        place(C_LABELS + 1, pad, y + ins.px(4.0), ins.px(90.0), ins.px(18.0));
        place(C_WIDTH, pad + ins.px(96.0), y, field, ins.px(23.0));
        place(C_LABELS + 2, pad + ins.px(96.0) + field, y + ins.px(4.0), ins.px(20.0), ins.px(18.0));
        place(C_HEIGHT, pad + ins.px(116.0) + field, y, field, ins.px(23.0));
        y += ins.px(30.0);
    }
    place(C_LABELS + 3, pad, y + ins.px(5.0), ins.px(90.0), ins.px(18.0));
    let half = (right - pad - ins.px(96.0)) / 2;
    place(C_LANDSCAPE, pad + ins.px(96.0), y, half, ins.px(26.0));
    place(C_PORTRAIT, pad + ins.px(96.0) + half, y, half, ins.px(26.0));
    y += ins.px(42.0);
    place(C_LABELS + 4, pad, y, right - pad, ins.px(18.0));
    y += ins.px(26.0);
    place(C_LABELS + 5, pad, y + ins.px(2.0), ins.px(90.0), ins.px(18.0));
    place(C_ANGLE_RESET, right - ins.px(22.0) - ins.px(56.0), y, ins.px(22.0), ins.px(20.0));
    place(C_ANGLE_VALUE, right - ins.px(56.0), y + ins.px(2.0), ins.px(56.0), ins.px(18.0));
    y += ins.px(22.0);
    place(C_ANGLE, pad - ins.px(6.0), y, right - pad + ins.px(12.0), ins.px(26.0));
    y += ins.px(40.0);
    place(C_LABELS + 6, pad, y, ins.px(90.0), ins.px(18.0));
    place(C_SIZE, pad + ins.px(90.0), y, right - pad - ins.px(90.0), ins.px(18.0));
    y += ins.px(22.0);
    place(C_SIZE_NOTE, pad, y, right - pad, ins.px(48.0));
    y += ins.px(56.0);
    place(C_RESET, pad, y, right - pad, ins.px(28.0));
    y += ins.px(40.0);
    place(C_LABELS + 7, pad, y, right - pad, ins.px(90.0));
    let button = ins.px(84.0);
    place(C_DONE, right - button, h - ins.px(44.0), button, ins.px(28.0));
    place(C_CANCEL, right - 2 * button - ins.px(8.0), h - ins.px(44.0), button, ins.px(28.0));
}

fn sync_crop(app: &mut App) {
    let cropping = app.model.cropping();
    if cropping && app.inspector.crop.is_none() {
        build_crop(app);
    }
    let Some(crop) = &app.inspector.crop else { return };
    unsafe {
        let _ = ShowWindow(crop.panel, if cropping { SW_SHOWNA } else { SW_HIDE });
    }
    let Some(session) = &app.model.crop_session else { return };
    let decimal = app.inspector.decimal;
    for (i, aspect) in CropAspect::ALL.iter().enumerate() {
        set_check(crop.get(C_ASPECT + i as u16), *aspect == session.aspect);
    }
    let portrait = session.orientation_portrait();
    set_check(crop.get(C_LANDSCAPE), !portrait);
    set_check(crop.get(C_PORTRAIT), portrait);
    enable(crop.get(C_LANDSCAPE), session.aspect.turns());
    enable(crop.get(C_PORTRAIT), session.aspect.turns());
    unsafe {
        for (id, value) in [(C_WIDTH, session.custom.width), (C_HEIGHT, session.custom.height)] {
            if GetFocus() != crop.get(id) {
                set_text(crop.get(id), &format(value, 0.01, decimal));
            }
        }
        let pos = (session.angle * 10.0).round() as isize;
        if SendMessageW(crop.get(C_ANGLE), TBM_GETPOS, WPARAM(0), LPARAM(0)).0 != pos {
            SendMessageW(crop.get(C_ANGLE), TBM_SETPOS, WPARAM(1), LPARAM(pos));
        }
        let _ = ShowWindow(crop.get(C_ANGLE_RESET), if session.angle != 0.0 { SW_SHOWNA } else { SW_HIDE });
    }
    set_text(crop.get(C_ANGLE_VALUE), &format!("{}°", format(session.angle, 0.1, decimal)));
    // The draft's pixels in the frame an export develops; with a target, a
    // smaller crop needs a larger frame, up to the working limit.
    let (size, note) = match app.model.crop_draft_size() {
        Some(Ok(size)) => {
            let source = app.model.source_size.0 as f64;
            let scale = size.working.0 as f64 / source;
            let note = if app.model.size_mode() != state::SizeMode::LongestSide {
                String::new()
            } else if scale >= 1.0 {
                format!("Image Size develops the whole frame {scale:.1}× the source.")
            } else {
                format!("Image Size develops the whole frame at {} of the source.", crate::model::percent(scale))
            };
            ((size.photo.0 as f64, size.photo.1 as f64), note)
        }
        Some(Err(error)) => {
            let shown = session.shown();
            ((shown.width, shown.height), error.to_string())
        }
        None => {
            let shown = session.shown();
            ((shown.width, shown.height), String::new())
        }
    };
    set_text(crop.get(C_SIZE), &format!("{} × {} px", size.0.round() as i64, size.1.round() as i64));
    set_text(crop.get(C_SIZE_NOTE), &note);
    enable(crop.get(C_RESET), session.modified());
    let custom_visible = unsafe { IsWindowVisible(crop.get(C_WIDTH)).as_bool() };
    if custom_visible != (session.aspect == CropAspect::Custom) {
        layout_crop(app);
    }
}

fn crop_command(app: &mut App, id: u16, code: u32) {
    let Some(session) = app.model.crop_session.as_mut() else { return };
    match id {
        i if (C_ASPECT..C_ASPECT + 9).contains(&i) && code == BN_CLICKED => session.choose(CropAspect::ALL[(i - C_ASPECT) as usize]),
        C_LANDSCAPE if code == BN_CLICKED => session.orient(false),
        C_PORTRAIT if code == BN_CLICKED => session.orient(true),
        C_ANGLE_RESET if code == BN_CLICKED => {
            session.begin_straightening();
            session.straighten(0.0);
            session.end_straightening();
        }
        C_RESET if code == BN_CLICKED => session.reset(),
        C_DONE if code == BN_CLICKED => return app.model.end_crop(true),
        C_CANCEL if code == BN_CLICKED => return app.model.end_crop(false),
        C_WIDTH | C_HEIGHT if code == EN_SETFOCUS => session.typing = true,
        C_WIDTH | C_HEIGHT if code == EN_KILLFOCUS => session.typing = false,
        _ => return,
    }
    app.model.crop_changed();
}

extern "system" fn crop_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                with_app(|app| crop_command(app, id, code));
                LRESULT(0)
            }
            WM_HSCROLL => {
                let code = (wparam.0 & 0xffff) as u32;
                let slider = HWND(lparam.0 as *mut _);
                with_app(|app| {
                    let pos = SendMessageW(slider, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 as f64 / 10.0;
                    let Some(session) = app.model.crop_session.as_mut() else { return };
                    match code {
                        TB_THUMBTRACK => {
                            session.begin_straightening();
                            session.straighten(pos);
                        }
                        TB_ENDTRACK => session.end_straightening(),
                        _ => {
                            session.begin_straightening();
                            session.straighten(pos);
                            session.end_straightening();
                        }
                    }
                    app.model.crop_changed();
                });
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let footer = GetDlgCtrlID(HWND(lparam.0 as *mut _)) as u16 == C_LABELS + 7;
                SetTextColor(dc, if footer { COLORREF(0x6d6d6d) } else { COLORREF(GetSysColor(COLOR_WINDOWTEXT)) });
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The custom ratio's fields: Return and leaving commit, Escape restores.
extern "system" fn ratio_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        let commit = |hwnd: HWND| {
            let mut text = [0u16; 32];
            let n = GetWindowTextW(hwnd, &mut text) as usize;
            let value = parse_number(&String::from_utf16_lossy(&text[..n]));
            let id = GetDlgCtrlID(hwnd) as u16;
            post(move |app: &mut App| {
                if let (Some(value), Some(session)) = (value, app.model.crop_session.as_mut()) {
                    if id == C_WIDTH {
                        session.set_custom(Some(value), None);
                    } else {
                        session.set_custom(None, Some(value));
                    }
                }
                app.model.crop_changed();
            });
        };
        match msg {
            WM_GETDLGCODE if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_RETURN | VK_ESCAPE) => LRESULT(DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                commit(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                post(|app: &mut App| app.model.crop_changed());
                let _ = SetFocus(GetParent(hwnd).unwrap_or_default());
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            WM_KILLFOCUS => {
                commit(hwnd);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_numbers_accept_either_separator() {
        assert_eq!(parse_number("1,5"), Some(1.5));
        assert_eq!(parse_number(" -0.25 "), Some(-0.25));
        assert_eq!(parse_number("3 %"), Some(3.0));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("1x2"), None);
        assert_eq!(parse_number("NaN"), None);
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn every_group_control_has_a_row() {
        let rows = rows();
        let paths: Vec<&str> = rows.iter().filter_map(|r| match r.kind {
            Kind::Slider(a) => Some(a.path()),
            Kind::Toggle(_, p) | Kind::Choice(_, p, _) => Some(p),
            _ => None,
        }).collect();
        for group in ADJUSTMENT_GROUPS {
            for control in group.controls {
                if let Adjustment::BalancePrint { title, .. } = control {
                    assert!(rows.iter().any(|r| matches!(r.kind, Kind::BalancePrint(t, _) if t == *title)));
                    continue;
                }
                assert!(paths.contains(&control.path()), "{}", control.path());
            }
        }
        // Control ids stay below the crop panel's.
        assert!(FIRST_ID as usize + rows.len() * PARTS as usize <= u16::MAX as usize);
    }
}

/// The small Segoe MDL2 Assets font the headers' chevrons use.
pub fn glyph_font() -> HGDIOBJ {
    with_app_fonts_glyph()
}
