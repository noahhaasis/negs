//! The adjustments column (`Inspector.swift`), with the common controls: the
//! scope (histogram or waveform, resizable), the film and paper pinned above
//! a Prep | Lab | Output switch, scrolling rows of trackbars with buddy edit
//! boxes, checkboxes and drop-down lists, and Reset Adjustments. The rows of
//! collapsed groups are created when the group first opens, and the whole
//! column only after the window's first frame (§17). While cropping, the crop
//! panel takes its place.

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

use super::draw::{self, Canvas, RectF, Role};
use super::motion;
use super::theme;
use super::{App, TBM_GETPOS, create, post, with_app};
use crate::border::{self, BorderColor, BorderStyle};
use crate::crop::CropAspect;
use crate::json;
use crate::model::{BorderWidth, Model, dirty};
use crate::platform::{pcwstr, wide};
use crate::session::SCOPE_HEIGHTS;
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
const TAB_OUTPUT: u16 = 901;
const RESET_ALL: u16 = 902;
const TAB_PREP: u16 = 903;
const SCOPE_MODE: u16 = 904;
/// The Film grid's search field, on the pinned panel.
const FILM_SEARCH: u16 = 905;
const TABS: [(u16, &str); 3] = [(TAB_PREP, "Prep"), (TAB_LAB, "Lab"), (TAB_OUTPUT, "Output")];
/// An open stock grid's height, in DIPs: it scrolls inside itself, keeping
/// the tabs in reach.
const GRID_HEIGHT: f64 = 290.0;
/// The search field over the Film grid: 4 DIPs above a 24 DIP field, 4 below.
const SEARCH_HEIGHT: f64 = 32.0;
const CROP_FIRST: u16 = 950;

/// Prepare the photograph, develop the film, then finish it; the pinned
/// rows (film and paper) sit above the tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Prep,
    Lab,
    Output,
    Pinned,
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
    Raw,
    /// A RAW whose decode found corrections for its lens.
    LensCorrection,
    CustomWhiteBalance,
    Scanning,
    Ratio35,
    LongestSide,
    /// The row's note has text.
    Note,
    /// Auto exposure, while the brightness match doesn't hide the meter.
    Meter,
    MeterAuto,
    /// Film format › Custom…: the long-edge slider.
    CustomFormat,
    /// Not among the Lab's basics: with Advanced controls, or once changed.
    Shows(&'static str),
    ScanToggle,
    ScanSection,
    OutputGroup,
    /// An adjustment group: it has basics, or something in it changed.
    Group(usize),
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
    /// Prep › Match camera brightness (Match original brightness for files).
    MatchBrightness,
    /// Prep › Exposure: the user's exposure above the brightness match.
    ExposureOffset,
    /// Lab › Camera › Film format: stills formats, or Custom….
    FilmFormat,
    /// Lab › Advanced controls (a preference).
    AllControls,
    /// The paper row while scanning film directly.
    PaperSkipped,
    /// The colour space the preview develops in (a preference).
    PreviewSpace,
    /// Image Size: Original or Longest side.
    SizeMode,
    LongestSide,
    /// The exported photograph's size.
    Photograph,
    /// Small text that follows the model.
    Note(Note),
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
    /// The tone graph atop Print Curves, with its label and channel buttons.
    ToneGraph,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Note {
    /// An Image Size error, or that the preview shows fewer pixels.
    Size,
    /// What the brightness match did, or why it can't.
    Brightness,
    /// What Advanced controls shows.
    AllControls,
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
    /// An adjustment group's control: the group and its place, for the
    /// Lab's basics.
    basic: Option<(usize, usize)>,
    controls: Vec<HWND>,
    shown: bool,
}

pub struct Inspector {
    pub panel: HWND,
    scope: HWND,
    scope_mode: HWND,
    scope_label: HWND,
    grip: HWND,
    /// The scope's height in DIPs, as dragged.
    scope_height: f64,
    /// The film and paper rows, above the tabs.
    pinned: HWND,
    pinned_height: i32,
    tabs: [HWND; 3],
    rows_panel: HWND,
    reset: HWND,
    crop: Option<CropPanel>,
    pub rect: RECT,
    rows: Vec<Row>,
    scroll: i32,
    content: i32,
    built: bool,
    dpi: u32,
    /// The stock lists were filled from this catalog and these favourites.
    stocks_filled: String,
    /// The film and paper tiles, built the first time each foldout opens.
    grids: [Option<StockGrid>; 2],
    /// The search field over the Film grid, made with its first grid. It
    /// outlives the grids' rebuilds, which starring a film makes, so typing
    /// in it goes on.
    film_search: HWND,
    dev_times: [Vec<f64>; 2],
    decimal: char,
    /// The UI font's line height, for value boxes inside their fields.
    line: i32,
    /// Motion: the next layout slides rows to their places; the slide
    /// running.
    slide: bool,
    sliding: Option<Slide>,
    /// Where the last layout put each control (its top), to slide from.
    placed: std::collections::HashMap<isize, i32>,
    /// Whether the crop panel showed at the last sync.
    crop_shown: bool,
}

/// Rows moving to their new places after a group opens or closes; the
/// rows it revealed show when they arrive.
struct Slide {
    start: std::time::Instant,
    moves: Vec<(HWND, i32, i32, i32, i32, i32)>,
    reveal: Vec<HWND>,
}

thread_local! {
    /// The current film and paper names, for the foldout headers.
    static FOLDOUT_NAMES: RefCell<[String; 2]> = const { RefCell::new([String::new(), String::new()]) };
    /// The tiles as drawn, by grid (film, paper) and position.
    static TILES: RefCell<[Vec<TileLook>; 2]> = const { RefCell::new([Vec::new(), Vec::new()]) };
    /// What the scope draws: the histogram, the waveform, and whether it shows the waveform.
    static SCOPE: RefCell<(Option<crate::histogram::Histogram>, Option<crate::histogram::Waveform>, bool)> = const { RefCell::new((None, None, false)) };
    /// The scope's height while its handle is dragged: where the drag
    /// started on screen, and the height then.
    static GRIP_DRAG: Cell<Option<(i32, f64)>> = const { Cell::new(None) };
    /// The tile under the pointer, by grid and position, which shows its star.
    static HOVER_TILE: Cell<Option<(bool, usize)>> = const { Cell::new(None) };
    static SWATCHES: Cell<(u32, u32, u32)> = const { Cell::new((0xffffff, 0, 0)) };
    static ITEM_HEIGHT: Cell<i32> = const { Cell::new(20) };
}

pub fn register() {
    unsafe {
        // Their backgrounds are painted from the palette.
        let classes: [(PCWSTR, WNDPROC, WNDCLASS_STYLES); 7] = [
            (w!("NegsInspector"), Some(panel_proc), WNDCLASS_STYLES(0)),
            (w!("NegsRows"), Some(rows_proc), WNDCLASS_STYLES(0)),
            (w!("NegsScope"), Some(scope_proc), WNDCLASS_STYLES(0)),
            (w!("NegsScopeGrip"), Some(grip_proc), CS_DBLCLKS),
            (w!("NegsCropPanel"), Some(crop_proc), WNDCLASS_STYLES(0)),
            // Its own class only so its scroll bar takes the dark theme.
            (w!("NegsCropRows"), Some(crop_proc), WNDCLASS_STYLES(0)),
            (w!("NegsStockGrid"), Some(grid_proc), WNDCLASS_STYLES(0)),
        ];
        for (name, proc_, style) in classes {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style,
                lpfnWndProc: proc_,
                hInstance: super::instance(),
                lpszClassName: name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
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
        Self::with_panel(panel, fonts.line, dpi)
    }

    /// Tests: an inspector with no windows, never built, for a headless `App`.
    #[cfg(test)]
    pub fn headless() -> Self {
        Self::with_panel(HWND::default(), 16, 96)
    }

    fn with_panel(panel: HWND, line: i32, dpi: u32) -> Self {
        Self {
            panel,
            scope: HWND::default(),
            scope_mode: HWND::default(),
            scope_label: HWND::default(),
            grip: HWND::default(),
            scope_height: SCOPE_HEIGHTS.0,
            pinned: HWND::default(),
            pinned_height: 0,
            tabs: [HWND::default(); 3],
            rows_panel: HWND::default(),
            reset: HWND::default(),
            crop: None,
            rect: RECT::default(),
            rows: vec![],
            scroll: 0,
            content: 0,
            built: false,
            dpi,
            stocks_filled: String::new(),
            grids: [None, None],
            film_search: HWND::default(),
            dev_times: [vec![], vec![]],
            decimal: '.',
            line,
            slide: false,
            sliding: None,
            placed: Default::default(),
            crop_shown: false,
        }
    }

    fn px(&self, dip: f64) -> i32 {
        (dip * self.dpi as f64 / 96.0).round() as i32
    }

    /// A value box's rectangle inside a 24 DIP field at `x`, `y`.
    fn field(&self, x: i32, y: i32, width: i32) -> RECT {
        draw::edit_in_field(RECT { left: x, top: y, right: x + width, bottom: y + self.px(24.0) }, self.line, self.dpi as f64 / 96.0)
    }

    pub fn built(&self) -> bool {
        self.built
    }

    pub fn layout(&mut self, x: i32, y: i32, width: i32, height: i32, visible: bool) {
        self.rect = RECT { left: x, top: y, right: x + width, bottom: y + height };
        let rows_panel = self.rows_panel;
        let rows_height = move || {
            let mut client = RECT::default();
            unsafe {
                let _ = GetClientRect(rows_panel, &mut client);
            }
            client.bottom
        };
        let before = rows_height();
        unsafe {
            let _ = MoveWindow(self.panel, x, y, width, height, true);
            let _ = ShowWindow(self.panel, if visible { SW_SHOWNA } else { SW_HIDE });
        }
        self.layout_frame();
        // A resized window: the rows' scroll range and position follow.
        if self.built && rows_height() != before {
            post(|app: &mut App| layout_rows(&mut app.inspector, &app.model));
        }
    }

    /// The scope, the pinned stocks, the tab switch, the rows and the reset
    /// button.
    fn layout_frame(&mut self) {
        if !self.built {
            return;
        }
        let (w, h) = (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top);
        let pad = self.px(16.0);
        // On the 8 DIP grid: the scope's menu, the scope and its grabber,
        // the pinned stocks, the tabs, the rows and the reset button.
        let inner = w - 2 * pad;
        let mode = self.px(24.0);
        let scope = self.px(self.scope_height);
        let grip = self.px(8.0);
        let tabs = self.px(32.0);
        let reset = self.px(32.0);
        let mut y = self.px(8.0);
        // Moved together (`DeferWindowPos`): one at a time, each move
        // repainted what it uncovered before the next filled it, which
        // flickered on every step of dragging the scope's grabber.
        let mut positions = vec![(self.scope_mode, pad, y, self.px(112.0).min(inner), self.px(200.0))];
        positions.push((self.scope_label, w - pad - self.px(40.0), y + self.px(4.0), self.px(40.0), self.px(16.0)));
        y += mode + self.px(8.0);
        positions.push((self.scope, pad, y, inner, scope));
        y += scope;
        positions.push((self.grip, pad, y, inner, grip));
        y += grip;
        positions.push((self.pinned, 0, y, w, self.pinned_height));
        y += self.pinned_height + self.px(8.0);
        let each = inner / 3;
        for (i, &tab) in self.tabs.iter().enumerate() {
            let x = pad + each * i as i32;
            let width = if i == 2 { inner - 2 * each } else { each };
            positions.push((tab, x, y, width, tabs));
        }
        y += tabs + self.px(8.0);
        let rows_bottom = h - reset - pad - self.px(8.0);
        positions.push((self.rows_panel, 0, y, w, (rows_bottom - y).max(0)));
        positions.push((self.reset, pad, h - reset - pad, inner, reset));
        if let Some(crop) = &self.crop {
            positions.push((crop.panel, 0, 0, w, h));
        }
        place(&positions, 0);
        layout_crop(self);
    }
}

/// The rows of the pinned area and the three tabs, in order
/// (`Inspector.swift`: `filmAndPaper`, `prep`, `lab` and `output`).
fn rows() -> Vec<Row> {
    let mut rows = vec![];
    let add = |rows: &mut Vec<Row>, kind: Kind, tab: Tab, group: Option<usize>, when: When| {
        rows.push(Row { kind, tab, group, when, print: false, border: false, basic: None, controls: vec![], shown: false });
        rows.len() - 1
    };
    // Above the tabs: the stocks shape everything the tabs adjust.
    let pinned = Tab::Pinned;
    add(&mut rows, Kind::Stock(false), pinned, None, When::Always);
    add(&mut rows, Kind::Foldout(false), pinned, None, When::Always);
    add(&mut rows, Kind::DevTime(false), pinned, None, When::FilmTimes);
    add(&mut rows, Kind::Stock(true), pinned, None, When::NotScanning);
    add(&mut rows, Kind::Foldout(true), pinned, None, When::NotScanning);
    add(&mut rows, Kind::DevTime(true), pinned, None, When::PaperTimes);
    add(&mut rows, Kind::PaperSkipped, pinned, None, When::Scanning);

    let prep = Tab::Prep;
    add(&mut rows, Kind::Choice("White balance", "io.raw_white_balance.mode", state::WHITE_BALANCE), prep, None, When::Raw);
    add(&mut rows, Kind::Slider(state::WB_TEMPERATURE), prep, None, When::CustomWhiteBalance);
    add(&mut rows, Kind::Slider(state::WB_TINT), prep, None, When::CustomWhiteBalance);
    add(&mut rows, Kind::Section("Brightness"), prep, None, When::Always);
    add(&mut rows, Kind::MatchBrightness, prep, None, When::Always);
    add(&mut rows, Kind::ExposureOffset, prep, None, When::Always);
    add(&mut rows, Kind::Note(Note::Brightness), prep, None, When::Note);
    add(&mut rows, Kind::Toggle("Auto exposure", "camera.auto_exposure"), prep, None, When::Meter);
    add(&mut rows, Kind::Choice("Metering", "camera.auto_exposure_method", state::METERING), prep, None, When::MeterAuto);
    for (title, sliders) in [("Tone", state::PREP_TONE), ("Color", state::PREP_COLOR), ("Detail", state::PREP_DETAIL)] {
        add(&mut rows, Kind::Section(title), prep, None, When::Always);
        for slider in sliders {
            add(&mut rows, Kind::Slider(*slider), prep, None, When::Always);
        }
    }
    // How the file is read; set once per photograph.
    add(&mut rows, Kind::Section("Lens"), prep, None, When::LensCorrection);
    add(&mut rows, Kind::Toggle("Lens corrections", state::LENS_CORRECTION), prep, None, When::LensCorrection);
    let input = add(&mut rows, Kind::Group { title: "Input Colour", enable: None }, prep, None, When::NotRaw);
    add(&mut rows, Kind::Choice("Read as", "io.input_interpretation.mode", state::INPUT_MODES), prep, Some(input), When::NotRaw);
    add(&mut rows, Kind::Choice("Colour space", "io.input_interpretation.color_space", state::ASSUMED_SPACES), prep, Some(input), When::AssumeInput);
    add(&mut rows, Kind::Toggle("Linear values", "io.input_interpretation.linear"), prep, Some(input), When::AssumeInput);
    add(&mut rows, Kind::Choice("Colour space", "io.input_color_space", state::MANUAL_SPACES), prep, Some(input), When::ManualInput);
    add(&mut rows, Kind::Toggle("Decode the curve", "io.input_cctf_decoding"), prep, Some(input), When::ManualInput);

    let lab = Tab::Lab;
    let groups = |rows: &mut Vec<Row>, section: Section| {
        for (index, group) in ADJUSTMENT_GROUPS.iter().enumerate().filter(|(_, g)| g.section == section) {
            let header = add(rows, Kind::Group { title: group.title, enable: group.enable }, lab, None, When::Group(index));
            rows[header].print = section == Section::Print;
            if group.title == "Print Curves" {
                let graph = add(rows, Kind::ToneGraph, lab, Some(header), When::Always);
                rows[graph].print = true;
            }
            for (place, control) in group.controls.iter().enumerate() {
                let kind = match *control {
                    Adjustment::Toggle { title, path } => Kind::Toggle(title, path),
                    Adjustment::Choice { title, path, options } => Kind::Choice(title, path, options),
                    Adjustment::BalancePrint { title, help } => Kind::BalancePrint(title, help),
                    slider => Kind::Slider(slider),
                };
                let when = if state::color_only(control.path()) { When::ColorFilm } else { When::Always };
                let row = add(rows, kind, lab, Some(header), when);
                rows[row].print = section == Section::Print;
                rows[row].basic = Some((index, place));
            }
        }
    };
    add(&mut rows, Kind::Section("Camera"), lab, None, When::Always);
    add(&mut rows, Kind::Slider(state::NEGATIVE_EXPOSURE), lab, None, When::Always);
    add(&mut rows, Kind::FilmFormat, lab, None, When::Always);
    add(&mut rows, Kind::Slider(state::FILM_FORMAT), lab, None, When::CustomFormat);
    add(&mut rows, Kind::Slider(state::LENS_BLUR), lab, None, When::Shows(state::LENS_BLUR.path()));
    groups(&mut rows, Section::Camera);
    add(&mut rows, Kind::Section("Film"), lab, None, When::Always);
    groups(&mut rows, Section::Film);
    add(&mut rows, Kind::Section("Print"), lab, None, When::Always);
    // Here rather than beside the stocks: few photographs want it.
    add(&mut rows, Kind::Toggle("Scan film directly", "io.scan_film"), lab, None, When::ScanToggle);
    groups(&mut rows, Section::Print);
    add(&mut rows, Kind::Footer("Printing and the paper are skipped while scanning film directly."), lab, None, When::Scanning);
    add(&mut rows, Kind::Section("Scan & Color"), lab, None, When::ScanSection);
    groups(&mut rows, Section::Scan);
    let output = add(&mut rows, Kind::Group { title: "Output", enable: None }, lab, None, When::OutputGroup);
    add(&mut rows, Kind::PreviewSpace, lab, Some(output), When::OutputGroup);
    add(&mut rows, Kind::Toggle("Encode with the colour space’s curve", "io.output_cctf_encoding"), lab, Some(output), When::OutputGroup);
    add(&mut rows, Kind::Section(""), lab, None, When::Always);
    add(&mut rows, Kind::AllControls, lab, None, When::Always);
    add(&mut rows, Kind::Note(Note::AllControls), lab, None, When::Always);

    // The size and frame of the finished photograph. Tone and colour edits
    // live in Prep, before the film, where nothing has clipped yet.
    let post = Tab::Output;
    add(&mut rows, Kind::Section("Image Size"), post, None, When::Always);
    add(&mut rows, Kind::SizeMode, post, None, When::Always);
    add(&mut rows, Kind::LongestSide, post, None, When::LongestSide);
    add(&mut rows, Kind::Photograph, post, None, When::Always);
    add(&mut rows, Kind::Note(Note::Size), post, None, When::Note);
    add(&mut rows, Kind::Footer("Resizes before film simulation, including halation and grain."), post, None, When::Always);
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
    ins.scope_height = app.model.prefs.scope_height.clamp(SCOPE_HEIGHTS.0, SCOPE_HEIGHTS.1).round();
    let visible = WS_CHILD | WS_VISIBLE;
    ins.scope_mode = create(w!("COMBOBOX"), "", visible | WS_TABSTOP | WS_GROUP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32), WINDOW_EX_STYLE(0), ins.panel, SCOPE_MODE, fonts.small);
    for title in ["Histogram", "Waveform"] {
        let text = wide(title);
        unsafe {
            SendMessageW(ins.scope_mode, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
        }
    }
    draw::paint_combo(ins.scope_mode);
    super::accessible_name(ins.scope_mode, "Scope view");
    ins.scope_label = create(w!("STATIC"), "RGB", visible | WINDOW_STYLE(SS_RIGHT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), ins.panel, 0, fonts.small);
    ins.scope = create(w!("NegsScope"), "Histogram", visible, WINDOW_EX_STYLE(0), ins.panel, 0, HFONT::default());
    ins.grip = create(w!("NegsScopeGrip"), "Scope height", visible | WS_TABSTOP, WINDOW_EX_STYLE(0), ins.panel, 0, HFONT::default());
    draw::track_hover(ins.grip);
    super::dialogs::tooltip(ins.grip, "Drag to resize the scope; double-click for its smallest height");
    ins.pinned = create(w!("NegsRows"), "", visible | WS_CLIPCHILDREN | WS_GROUP, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), ins.panel, 0, HFONT::default());
    let tab_style = visible | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    for (i, (id, title)) in TABS.into_iter().enumerate() {
        ins.tabs[i] = create(w!("BUTTON"), title, tab_style | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), ins.panel, id, fonts.ui);
        // Prep | Lab | Output: Lab is the switch's middle segment.
        if i == 1 {
            draw::set_role(ins.tabs[i], Role::SegmentMiddle);
        }
    }
    ins.rows_panel = create(w!("NegsRows"), "", visible | WS_VSCROLL | WS_CLIPCHILDREN | WS_GROUP, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), ins.panel, 0, HFONT::default());
    ins.reset = create(w!("BUTTON"), "Reset Adjustments", visible | WS_TABSTOP | WS_GROUP, WINDOW_EX_STYLE(0), ins.panel, RESET_ALL, fonts.ui);
    super::dialogs::tooltip(ins.reset, "Restore defaults on all tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this.");
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
    ins.line = app.fonts.line;
    let fonts = &app.fonts;
    let font = fonts.ui;
    for row in &ins.rows {
        for (i, &c) in row.controls.iter().enumerate() {
            // The font each control was created with, at the new size.
            let f = match (row.kind, i) {
                (Kind::Section(_), _) => fonts.section,
                (Kind::Footer(_) | Kind::Note(_) | Kind::BorderStyles | Kind::ToneGraph, _) => fonts.small,
                (Kind::Group { .. }, 0) => fonts.bold,
                (Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset, 3) => fonts.glyph_small,
                _ => fonts.ui,
            };
            unsafe {
                SendMessageW(c, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
            }
        }
    }
    for &c in ins.tabs.iter().chain([&ins.reset]).chain([&ins.film_search].into_iter().filter(|s| !s.is_invalid())) {
        unsafe {
            SendMessageW(c, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }
    for &c in [&ins.scope_mode, &ins.scope_label] {
        let small = app.fonts.small;
        unsafe {
            SendMessageW(c, WM_SETFONT, WPARAM(small.0 as usize), LPARAM(1));
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
    match model.prefs.tab() {
        "Lab" => Tab::Lab,
        "Output" => Tab::Output,
        _ => Tab::Prep,
    }
}

/// The preview's colour space or curve is off its default, so the Lab shows them.
fn output_changed(model: &Model) -> bool {
    model.preview_space() != crate::model::COLOR_SPACES[0] || model.is_modified("io.output_cctf_encoding")
}

fn control_modified(model: &Model, control: &Adjustment) -> bool {
    !matches!(control, Adjustment::BalancePrint { .. }) && model.is_modified(control.path())
}

/// Whether the Lab shows a group: it has basics, or something in it changed.
fn group_shows(model: &Model, group: &state::AdjustmentGroup) -> bool {
    model.prefs.lab_all_controls
        || group.basic > 0
        || group.enable.is_some_and(|path| model.is_modified(path))
        || group.controls.iter().any(|c| control_modified(model, c))
}

fn photo_size(model: &Model) -> Option<(u32, u32)> {
    let size = model.photo_size();
    (!size.is_zero()).then(|| (size.width as u32, size.height as u32))
}

fn stock_times(model: &Model, paper: bool) -> Vec<f64> {
    let id = if paper { &model.state.paper } else { &model.state.film };
    model.stocks.iter().find(|s| s.paper == paper && &s.id == id).map(|s| s.times.clone()).unwrap_or_default()
}

/// Whether a row shows. `group_shown` says whether a row's group header is
/// on screen and open.
fn visible(row: &Row, model: &Model, group_shown: &dyn Fn(usize) -> bool) -> bool {
    let scanning = json::boolean(model.value("io.scan_film"));
    let border = model.state.border;
    let all = model.prefs.lab_all_controls;
    let shown = match row.when {
        When::Always => true,
        When::FilmTimes => stock_times(model, false).len() > 1,
        When::NotScanning => !scanning,
        When::PaperTimes => !scanning && stock_times(model, true).len() > 1,
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
        When::Note => matches!(row.kind, Kind::Note(note) if !note_text(model, note).is_empty()),
        When::Meter => model.shows_meter(),
        When::MeterAuto => model.shows_meter() && json::boolean(model.value("camera.auto_exposure")),
        When::CustomFormat => model.film_format_custom(),
        When::Shows(path) => all || model.is_modified(path),
        When::ScanToggle => all || scanning,
        When::ScanSection => {
            all || output_changed(model) || ADJUSTMENT_GROUPS.iter().any(|g| g.section == Section::Scan && group_shows(model, g))
        }
        When::OutputGroup => all || output_changed(model),
        When::Group(index) => group_shows(model, &ADJUSTMENT_GROUPS[index]),
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
    // The Lab shows each group's basics, and anything changed from its default.
    let basic = row.basic.is_none_or(|(index, place)| {
        let group = &ADJUSTMENT_GROUPS[index];
        all || place < group.basic || control_modified(model, &group.controls[place])
    });
    shown && basic && row.group.is_none_or(|g| group_shown(g))
}

/// A note's text; empty hides it.
fn note_text(model: &Model, note: Note) -> String {
    match note {
        Note::Size => size_note(model),
        Note::Brightness => model.brightness_match_note().unwrap_or_default(),
        Note::AllControls if model.prefs.lab_all_controls => "Every camera, film, print and scan setting.".into(),
        Note::AllControls => "The settings most photographs need. Any setting you or a preset changed also shows.".into(),
    }
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
    if histogram || app.inspector.built {
        let waveform = app.model.prefs.scope_mode == "Waveform";
        let changed = SCOPE.with(|scope| {
            let mut scope = scope.borrow_mut();
            let next = (app.model.histogram.clone(), app.model.waveform.clone(), waveform);
            let changed = *scope != next;
            *scope = next;
            changed
        });
        if changed && !app.inspector.scope.is_invalid() {
            unsafe {
                let _ = InvalidateRect(app.inspector.scope, None, false);
            }
        }
    }
    if !app.inspector.built {
        return;
    }
    // Entering or leaving crop swaps the whole column while it doesn't
    // draw, then it paints once, like a tab's rows below. The incoming side
    // used to fade in, a layered window: entering showed the old controls
    // through it and repainted the crop panel piece by piece when the fade
    // ended; leaving blanked the column for a few frames.
    let swap = app.inspector.crop_shown != app.model.cropping();
    let panel = app.inspector.panel;
    if swap {
        hold_redraw(panel, true);
    }
    sync_column(app, structure, swap);
    if swap {
        hold_redraw(panel, false);
    }
}

/// Stops the column drawing, or starts it again and paints it once.
fn hold_redraw(panel: HWND, hold: bool) {
    unsafe {
        SendMessageW(panel, WM_SETREDRAW, WPARAM(!hold as usize), LPARAM(0));
        if !hold {
            let _ = RedrawWindow(panel, None, None, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW);
        }
    }
}

/// `sync` past the scope; `held` when the column already doesn't draw.
fn sync_column(app: &mut App, structure: bool, held: bool) {
    sync_crop(app);
    let cropping = app.model.cropping();
    let ins = &mut app.inspector;
    unsafe {
        for c in [ins.scope, ins.scope_mode, ins.scope_label, ins.grip, ins.pinned, ins.tabs[0], ins.tabs[1], ins.tabs[2], ins.rows_panel, ins.reset] {
            let _ = ShowWindow(c, if cropping { SW_HIDE } else { SW_SHOWNA });
        }
    }
    if cropping {
        app.model.show_tone_graph(false);
        return;
    }
    let current = tab(&app.model);
    unsafe {
        for (i, tab) in [Tab::Prep, Tab::Lab, Tab::Output].into_iter().enumerate() {
            SendMessageW(ins.tabs[i], BM_SETCHECK, WPARAM((current == tab) as usize), LPARAM(0));
        }
        let _ = EnableWindow(ins.reset, app.model.has_catalog());
        let mode = (app.model.prefs.scope_mode == "Waveform") as isize;
        if SendMessageW(ins.scope_mode, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != mode {
            SendMessageW(ins.scope_mode, CB_SETCURSEL, WPARAM(mode as usize), LPARAM(0));
            super::accessible_name(ins.scope, if mode == 1 { "RGB waveform" } else { "Histogram" });
        }
    }
    let model = &app.model;
    let expanded: Vec<bool> = ins.rows.iter().map(|r| matches!(r.kind, Kind::Group { .. }) && model.prefs.expanded(row_title(&r.kind))).collect();
    // A group's rows show only below its header, on screen and open.
    let mut want = vec![false; ins.rows.len()];
    for (i, row) in ins.rows.iter().enumerate() {
        let on_tab = row.tab == current || row.tab == Tab::Pinned;
        let shown = on_tab && visible(row, model, &|g| want[g] && expanded[g]);
        want[i] = shown;
    }
    // Rows and grids appearing and going (a tab, a group, a foldout) change
    // while the column doesn't draw, then it paints once: shown one by one,
    // rows drew at their old places, over a part-cleared panel, before the
    // layout moved them. The tabs used to fade the rows in instead, a
    // layered window that blanked them first and flashed again when it ended.
    let foldout_changed = |paper: bool| {
        let open = model.previews.open(paper) && ins.rows.iter().zip(&want).any(|(r, w)| r.kind == Kind::Foldout(paper) && *w);
        open != ins.grids[paper as usize].as_ref().is_some_and(|g| g.was_open)
    };
    let lock = !held && (want.iter().zip(&ins.rows).any(|(w, r)| *w != r.shown) || foldout_changed(false) || foldout_changed(true));
    if lock {
        hold_redraw(ins.panel, true);
    }
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
    // An open foldout's tiles, built the first time it opens, and again when
    // the catalog or the favourites change.
    for paper in [false, true] {
        let key = grid_key(model, paper);
        let open = model.previews.open(paper) && ins.rows.iter().any(|r| r.kind == Kind::Foldout(paper) && r.shown);
        // A grid rebuilt while open keeps its scroll and the tile under the
        // pointer, rather than jumping back to the current stock.
        let mut kept = None;
        if ins.grids[paper as usize].as_ref().is_some_and(|g| g.key != key) {
            if let Some(grid) = ins.grids[paper as usize].take() {
                kept = grid.was_open.then(|| (grid.scroll, pointed_tile(&grid)));
                // The hovered tile's position now names another tile.
                if HOVER_TILE.get().is_some_and(|(p, _)| p == paper) {
                    HOVER_TILE.set(None);
                }
                unsafe {
                    let _ = DestroyWindow(grid.window);
                }
            }
        }
        if open && ins.grids[paper as usize].is_none() && model.has_catalog() {
            let mut grid = build_grid(ins.pinned, &app.fonts, model, paper, key);
            if let Some((scroll, anchor)) = kept {
                (grid.scroll, grid.was_open, grid.anchor) = (scroll, true, anchor);
            }
            ins.grids[paper as usize] = Some(grid);
            if !paper && ins.film_search.is_invalid() {
                ins.film_search = create_film_search(ins.pinned, &app.fonts, model);
            }
            created = true;
            fix_z_order(ins);
        }
    }
    // The search changed elsewhere: Settings turning previews off clears it.
    let refiltered = sync_film_search(ins, model);
    // A note's height follows its text, so set it before laying out.
    let mut note_changed = false;
    for row in ins.rows.iter().filter(|r| r.shown) {
        if let Kind::Note(note) = row.kind {
            let text = note_text(model, note);
            if window_text(row.controls[0]) != text {
                set_text(row.controls[0], &text);
                note_changed = true;
            }
        }
    }
    let t1 = crate::platform::qpc();
    if changed || created || structure || note_changed || refiltered {
        layout_rows(ins, model);
    }
    let t2 = crate::platform::qpc();
    update_values(ins, model, &expanded);
    if lock {
        hold_redraw(ins.panel, false);
    }
    let t3 = crate::platform::qpc();
    // Curves are computed only while the graph is on screen.
    let graph = ins.rows.iter().any(|r| r.kind == Kind::ToneGraph && r.shown);
    app.model.show_tone_graph(graph && app.model.inspector_visible && app.model.pane() == crate::model::Pane::Adjustments);
    if created {
        let ms = |a, b| crate::platform::qpc_ms(a, b);
        app.report.line(&format!("inspector sync: create {:.1} ms, layout {:.1} ms, values {:.1} ms", ms(t0, t1), ms(t1, t2), ms(t2, t3)));
    }
}

fn create_row(ins: &mut Inspector, fonts: &super::Fonts, index: usize) {
    let parent = if ins.rows[index].tab == Tab::Pinned { ins.pinned } else { ins.rows_panel };
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
        draw::paint_combo(c);
        c
    };
    let mut controls = vec![];
    match kind {
        Kind::Section(title) => controls.push(label(title, fonts.section)),
        Kind::Footer(text) => {
            let footer = create(w!("STATIC"), text, hidden | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(LABEL), fonts.small);
            controls.push(footer);
        }
        Kind::Group { title, enable } => {
            let header = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.bold);
            draw::track_hover(header);
            if let Some(help) = crate::help::group(title) {
                super::dialogs::tooltip(header, help);
            }
            controls.push(header);
            if enable.is_some() {
                let on = check("On");
                super::dialogs::tooltip(on, &format!("Turns {title} on or off, keeping its settings."));
                controls.push(on);
            }
        }
        Kind::Slider(Adjustment::Slider { .. }) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
            let (title, low, high, step) = match kind {
                Kind::Slider(Adjustment::Slider { title, low, high, step, .. }) => (title, low, high, step),
                Kind::BorderSlider(title, field) => {
                    let (low, high, step) = field.range();
                    (title, low, high, step)
                }
                // The range moves with the match; the number of steps doesn't.
                Kind::ExposureOffset => ("Exposure (EV)", state::PREP_EXPOSURE_RANGE.0, state::PREP_EXPOSURE_RANGE.1, state::PREP_EXPOSURE_STEP),
                _ => unreachable!(),
            };
            let text = label(title, fonts.ui);
            controls.push(text);
            let slider = create(TRACKBAR_CLASSW, title, hidden | WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), WINDOW_EX_STYLE(0), parent, id(SLIDER), HFONT::default());
            if let Kind::Slider(Adjustment::Slider { path, .. }) = kind
                && let Some(help) = crate::help::text(path)
            {
                super::dialogs::tooltip(text, help);
                super::dialogs::tooltip(slider, help);
            }
            let steps = ((high - low) / step).round() as isize;
            unsafe {
                SendMessageW(slider, TBM_SETRANGEMIN, WPARAM(0), LPARAM(0));
                SendMessageW(slider, TBM_SETRANGEMAX, WPARAM(0), LPARAM(steps));
                SendMessageW(slider, TBM_SETLINESIZE, WPARAM(0), LPARAM(1));
                SendMessageW(slider, TBM_SETPAGESIZE, WPARAM(0), LPARAM((steps / 10).max(1)));
                let _ = SetWindowSubclass(slider, Some(trackbar_subclass), 1, id(RESET) as usize);
            }
            draw::track_hover(slider);
            controls.push(slider);
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            draw::set_role(edit, Role::Value);
            draw::track_hover(edit);
            super::accessible_name(edit, title);
            unsafe {
                let _ = SetWindowSubclass(edit, Some(edit_subclass), 1, 0);
            }
            controls.push(edit);
            let reset = create(w!("BUTTON"), "\u{E777}", hidden | WINDOW_STYLE(BS_PUSHBUTTON as u32 | BS_FLAT as u32), WINDOW_EX_STYLE(0), parent, id(RESET), fonts.glyph_small);
            draw::track_hover(reset);
            super::accessible_name(reset, &format!("Reset {title} to Default"));
            controls.push(reset);
        }
        Kind::Slider(_) => {}
        Kind::Toggle(title, _) | Kind::BorderToggle(title, _) => {
            let c = check(title);
            match kind {
                Kind::Toggle("Scan film directly", _) => super::dialogs::tooltip(c, "Skip printing and scan the developed film, as for slides."),
                Kind::Toggle("Lens corrections", _) => super::dialogs::tooltip(c, "Corrects distortion, vignetting and colour fringing with the data the camera recorded for its lens."),
                Kind::Toggle("Auto exposure", _) => super::dialogs::tooltip(c, "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it."),
                Kind::Toggle("Encode with the colour space’s curve", _) => super::dialogs::tooltip(c, "Off writes linear values, for further processing."),
                Kind::Toggle(_, path) => {
                    if let Some(help) = crate::help::text(path) {
                        super::dialogs::tooltip(c, help);
                    }
                }
                Kind::BorderToggle(_, 0) => super::dialogs::tooltip(c, "Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it."),
                Kind::BorderToggle(_, 1) => super::dialogs::tooltip(c, "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post."),
                Kind::BorderToggle(_, 2) => super::dialogs::tooltip(c, "Adds the border to exported photographs and saved previews."),
                _ => {}
            }
            controls.push(c);
        }
        Kind::MatchBrightness => controls.push(check("Match camera brightness")),
        Kind::AllControls => {
            let c = check("Advanced controls");
            super::dialogs::tooltip(c, "Shows every Lab setting. Off, the Lab shows the few most photographs need, and any that you or a preset changed.");
            controls.push(c);
        }
        Kind::FilmFormat => {
            controls.push(label("Film format", fonts.ui));
            let c = combo(false);
            for title in state::FILM_FORMATS.iter().map(|(name, _)| *name).chain(["Custom…"]) {
                let text = wide(title);
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            super::dialogs::tooltip(c, crate::help::text(state::FILM_FORMAT.path()).unwrap_or_default());
            controls.push(c);
        }
        Kind::PaperSkipped => {
            controls.push(label("Paper", fonts.ui));
            let value = create(w!("STATIC"), "Skipped – scanning film", hidden | WINDOW_STYLE(SS_RIGHT.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            super::dialogs::tooltip(value, "Scan film directly is on in the Lab’s Print section.");
            controls.push(value);
        }
        Kind::ToneGraph => {
            let text = create(w!("STATIC"), "Tone graph", hidden | WINDOW_STYLE(SS_LEFT.0 | SS_CENTERIMAGE.0 | SS_ENDELLIPSIS.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.small);
            controls.push(text);
            controls.push(super::tone_graph::create(parent, id(ACTION), text));
            let names = ["the neutral curve", "the red layer", "the green layer", "the blue layer"];
            for (i, title) in ["RGB", "R", "G", "B"].into_iter().enumerate() {
                let group = if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
                let style = hidden | WS_TABSTOP | group | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
                let b = create(w!("BUTTON"), title, style, WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.small);
                // One switch of four segments; the style tells the ends.
                if (1..3).contains(&i) {
                    draw::set_role(b, Role::SegmentMiddle);
                }
                super::dialogs::tooltip(b, &format!("Show {}", names[i]));
                controls.push(b);
            }
        }
        Kind::BalancePrint(title, help) => {
            let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(ACTION), fonts.ui);
            super::dialogs::tooltip(c, help);
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
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL | ES_NUMBER) as u32), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            draw::set_role(edit, Role::Field);
            draw::track_hover(edit);
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
        Kind::Note(_) => {
            controls.push(create(w!("STATIC"), "", hidden | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(LABEL), fonts.small));
        }
        Kind::Choice(title, path, options) => {
            controls.push(label(title, fonts.ui));
            let c = combo(false);
            for option in options {
                let text = wide(state::option_title(option));
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            if let Some(help) = crate::help::text(path) {
                super::dialogs::tooltip(c, help);
            }
            controls.push(c);
        }
        Kind::Stock(paper) => {
            controls.push(label(if paper { "Paper" } else { "Film" }, fonts.ui));
            let c = combo(true);
            super::dialogs::tooltip(c, if paper { crate::help::PAPER } else { crate::help::FILM });
            controls.push(c);
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
            super::dialogs::tooltip(header, if paper { crate::help::PAPER } else { crate::help::FILM });
            draw::track_hover(header);
            controls.push(header);
        }
        Kind::BorderStyles => {
            for (i, style) in border::STYLES.iter().enumerate() {
                let b = create(w!("BUTTON"), border::title(*style), hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.small);
                super::dialogs::tooltip(b, border::help(*style));
                draw::track_hover(b);
                controls.push(b);
            }
        }
        Kind::Color(line) => {
            controls.push(label(if line { "Line color" } else { "Color" }, fonts.ui));
            let b = create(w!("BUTTON"), if line { "Line color" } else { "Color" }, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.ui);
            draw::track_hover(b);
            controls.push(b);
        }
    }
    ins.rows[index].controls = controls;
}

/// Tab order follows the rows, though they were created in another order.
/// The pinned rows and the scrolling rows are each their own panel's children.
fn fix_z_order(ins: &Inspector) {
    for pinned in [true, false] {
        let mut previous = HWND_TOP;
        for row in ins.rows.iter().filter(|r| (r.tab == Tab::Pinned) == pinned) {
            // The Film grid's search field, then its tiles.
            let grid: Vec<HWND> = match row.kind {
                Kind::Foldout(paper) => {
                    let search = (!paper && !ins.film_search.is_invalid()).then_some(ins.film_search);
                    search.into_iter().chain(ins.grids[paper as usize].as_ref().map(|g| g.window)).collect()
                }
                _ => vec![],
            };
            for &c in row.controls.iter().chain(grid.iter()) {
                unsafe {
                    let _ = SetWindowPos(c, previous, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                previous = c;
            }
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
        r.bottom + ins.px(2.0)
    }
}

/// Lays out the pinned rows, which sizes their panel and moves the tabs,
/// then the scrolling rows.
fn layout_rows(ins: &mut Inspector, model: &Model) {
    let pinned = place_rows(ins, model, true);
    if pinned != ins.pinned_height {
        ins.pinned_height = pinned;
        ins.layout_frame();
    }
    place_rows(ins, model, false);
}

/// Places one panel's rows; returns the height they take.
fn place_rows(ins: &mut Inspector, model: &Model, pinned: bool) -> i32 {
    let panel = if pinned { ins.pinned } else { ins.rows_panel };
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(panel, &mut client);
    }
    let width = if pinned { ins.rect.right - ins.rect.left } else { client.right };
    // An 8 DIP grid: rows are multiples of 8 high, and every row's control
    // starts at one column, grouped or not.
    let pad = ins.px(16.0);
    let indent = ins.px(16.0);
    let column = pad + ins.px(128.0);
    let text = ins.px(20.0);
    let mut y = ins.px(if pinned { 0.0 } else { 8.0 });
    let mut positions: Vec<(HWND, i32, i32, i32, i32)> = vec![];
    let mut grids_shown = [false; 2];
    let mut first = true;
    let dpi = ins.dpi;
    let grid_height = ins.px(GRID_HEIGHT);
    for i in 0..ins.rows.len() {
        let row = &ins.rows[i];
        if !row.shown || row.controls.is_empty() || (row.tab == Tab::Pinned) != pinned {
            continue;
        }
        let x = pad + if row.group.is_some() { indent } else { 0 };
        let right = width - pad;
        let label = (column - x - ins.px(8.0)).max(ins.px(48.0));
        let c = &row.controls;
        let height = match row.kind {
            // An untitled section only adds a gap.
            Kind::Section("") => ins.px(8.0),
            Kind::Section(_) => {
                // The first section needs no space above it.
                let top = if first { 0 } else { ins.px(16.0) };
                positions.push((c[0], pad, y + top, right - pad, ins.px(24.0)));
                top + ins.px(32.0)
            }
            Kind::Footer(_) | Kind::Note(_) => {
                let text_height = footer_height(ins, c[0], right - x);
                positions.push((c[0], x, y, right - x, text_height));
                text_height + ins.px(8.0)
            }
            Kind::LongestSide => {
                let (field, unit) = (ins.px(72.0), ins.px(24.0));
                positions.push((c[0], x, y + ins.px(6.0), label, text));
                let r = ins.field(right - unit - field, y + ins.px(4.0), field);
                positions.push((c[1], r.left, r.top, r.right - r.left, r.bottom - r.top));
                positions.push((c[2], right - unit + ins.px(6.0), y + ins.px(6.0), unit - ins.px(6.0), text));
                ins.px(32.0)
            }
            Kind::Photograph => {
                positions.push((c[0], x, y + ins.px(2.0), label, text));
                positions.push((c[1], column, y + ins.px(2.0), right - column, text));
                ins.px(24.0)
            }
            Kind::PaperSkipped => {
                // In the paper foldout's place: as tall, the title and the
                // value where its title and its stock are.
                let title = pad + ins.px(16.0);
                positions.push((c[0], title, y + ins.px(6.0), column - title - ins.px(8.0), text));
                positions.push((c[1], column, y + ins.px(6.0), right - column - ins.px(8.0), text));
                ins.px(32.0)
            }
            Kind::Group { enable, .. } => {
                let check = if enable.is_some() { ins.px(56.0) } else { 0 };
                positions.push((c[0], pad - ins.px(8.0), y + ins.px(2.0), right - pad + ins.px(8.0) - check, ins.px(28.0)));
                if enable.is_some() {
                    positions.push((c[1], right - check + ins.px(8.0), y + ins.px(4.0), check - ins.px(8.0), ins.px(24.0)));
                }
                ins.px(32.0)
            }
            Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
                let edit = ins.px(56.0);
                let reset = ins.px(24.0);
                positions.push((c[0], x, y + ins.px(2.0), right - x - reset - edit, text));
                positions.push((c[3], right - edit - reset, y, reset, ins.px(22.0)));
                // 26 DIPs: the thumb's focus ring, uncut.
                positions.push((c[1], x - ins.px(8.0), y + ins.px(22.0), right - x - edit, ins.px(26.0)));
                let r = ins.field(right - edit, y + ins.px(22.0), edit);
                positions.push((c[2], r.left, r.top, r.right - r.left, r.bottom - r.top));
                ins.px(48.0)
            }
            Kind::Toggle(..) | Kind::BorderToggle(..) | Kind::MatchBrightness | Kind::AllControls => {
                positions.push((c[0], x, y + ins.px(4.0), right - x, ins.px(24.0)));
                ins.px(32.0)
            }
            Kind::ToneGraph => {
                // The label and the RGB | R | G | B switch on one 32 DIP
                // line, the graph below on the grid.
                let button = ins.px(32.0);
                let buttons = 4 * button;
                positions.push((c[0], x, y + ins.px(4.0), right - x - buttons - ins.px(8.0), ins.px(24.0)));
                for (i, &b) in c[2..].iter().enumerate() {
                    positions.push((b, right - buttons + button * i as i32, y + ins.px(4.0), button, ins.px(24.0)));
                }
                positions.push((c[1], x, y + ins.px(32.0), right - x, ins.px(168.0)));
                ins.px(32.0 + 168.0 + 8.0)
            }
            Kind::BalancePrint(..) => {
                let button = ins.px(128.0).min(right - x);
                positions.push((c[0], right - button, y + ins.px(4.0), button, ins.px(32.0)));
                ins.px(40.0)
            }
            Kind::Foldout(paper) => {
                positions.push((c[0], pad - ins.px(8.0), y + ins.px(2.0), right - pad + ins.px(8.0), ins.px(28.0)));
                let header = ins.px(32.0);
                let open = model.previews.open(paper);
                // The Film grid's search field sits over the grid, outside its
                // scrolling, so it stays in reach.
                let search = (!paper && !ins.film_search.is_invalid()).then(|| (ins.film_search, ins.field(x, y + header + ins.px(4.0), right - x), ins.px(SEARCH_HEIGHT)));
                match &mut ins.grids[paper as usize] {
                    Some(grid) if open => {
                        grids_shown[paper as usize] = true;
                        let below = match search {
                            Some((field, r, height)) => {
                                positions.push((field, r.left, r.top, r.right - r.left, r.bottom - r.top));
                                height
                            }
                            None => 0,
                        };
                        let height = layout_grid(grid, right - x, grid_height, dpi);
                        positions.push((grid.window, x, y + header + below, right - x, height));
                        header + below + height + ins.px(8.0)
                    }
                    _ => header,
                }
            }
            Kind::Choice(..) | Kind::Stock(_) | Kind::DevTime(_) | Kind::SizeMode | Kind::PreviewSpace | Kind::FilmFormat => {
                positions.push((c[0], x, y + ins.px(6.0), label, text));
                positions.push((c[1], column, y + ins.px(4.0), right - column, ins.px(300.0)));
                ins.px(32.0)
            }
            Kind::BorderStyles => {
                let gap = ins.px(8.0);
                let each = (right - x + gap) / 4;
                for (i, &b) in c.iter().enumerate() {
                    positions.push((b, x + each * i as i32, y + ins.px(4.0), each - gap, ins.px(72.0)));
                }
                ins.px(80.0)
            }
            Kind::Color(_) => {
                positions.push((c[0], x, y + ins.px(6.0), label, text));
                positions.push((c[1], column, y + ins.px(4.0), ins.px(64.0), ins.px(24.0)));
                ins.px(32.0)
            }
        };
        first = false;
        y += height;
    }
    if pinned {
        for (grid, shown) in ins.grids.iter_mut().zip(grids_shown) {
            if let Some(grid) = grid {
                grid.was_open = shown;
                unsafe {
                    let _ = ShowWindow(grid.window, if shown { SW_SHOWNA } else { SW_HIDE });
                }
            }
        }
        if !ins.film_search.is_invalid() {
            unsafe {
                let _ = ShowWindow(ins.film_search, if grids_shown[0] { SW_SHOWNA } else { SW_HIDE });
            }
        }
        place(&positions, 0);
        unsafe {
            let _ = InvalidateRect(ins.pinned, None, true);
        }
        return y;
    }
    ins.content = y + ins.px(8.0);
    let visible = client.bottom;
    ins.scroll = ins.scroll.clamp(0, (ins.content - visible).max(0));
    // A slide still running ends where this layout puts things.
    let interrupted = ins.sliding.take().map(|s| s.reveal).unwrap_or_default();
    let slide = std::mem::take(&mut ins.slide) && motion::enabled() && !ins.placed.is_empty();
    let mut moves = vec![];
    let mut reveal = vec![];
    let mut placed = std::collections::HashMap::new();
    unsafe {
        let mut defer = BeginDeferWindowPos(positions.len() as i32).ok();
        for (hwnd, x, y, w, h) in positions {
            let top = y - ins.scroll;
            placed.insert(hwnd.0 as isize, top);
            let mut at = top;
            if slide {
                match ins.placed.get(&(hwnd.0 as isize)) {
                    Some(&from) if from != top => {
                        moves.push((hwnd, x, from, top, w.max(0), h.max(0)));
                        at = from;
                    }
                    Some(_) => {}
                    None => reveal.push(hwnd),
                }
            }
            if let Some(d) = defer
                && !combo_in_place(hwnd, x, at, w.max(0))
            {
                defer = DeferWindowPos(d, hwnd, None, x, at, w.max(0), h.max(0), SWP_NOZORDER | SWP_NOACTIVATE).ok();
            }
            if interrupted.contains(&hwnd) {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
        }
        if let Some(d) = defer {
            let _ = EndDeferWindowPos(d);
        }
        if !moves.is_empty() {
            // Revealed rows wait until the rows below have made room.
            for &hwnd in &reveal {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            ins.sliding = Some(Slide { start: std::time::Instant::now(), moves, reveal });
            super::after(std::time::Duration::from_millis(15), slide_step);
        }
        ins.placed = placed;
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
    ins.content
}

/// Moves windows to their places, `scroll` up.
fn place(positions: &[(HWND, i32, i32, i32, i32)], scroll: i32) {
    unsafe {
        let mut defer = BeginDeferWindowPos(positions.len() as i32).ok();
        for &(hwnd, x, y, w, h) in positions {
            if let Some(d) = defer
                && !combo_in_place(hwnd, x, y - scroll, w.max(0))
            {
                defer = DeferWindowPos(d, hwnd, None, x, y - scroll, w.max(0), h.max(0), SWP_NOZORDER | SWP_NOACTIVATE).ok();
            }
        }
        if let Some(d) = defer {
            let _ = EndDeferWindowPos(d);
        }
    }
}

/// One frame of a slide; the last shows the revealed rows.
fn slide_step(app: &mut App) {
    let ins = &mut app.inspector;
    let Some(slide) = &ins.sliding else { return };
    let t = slide.start.elapsed().as_secs_f64() / motion::DURATION.as_secs_f64();
    let k = motion::ease(t);
    unsafe {
        let mut defer = BeginDeferWindowPos(slide.moves.len() as i32).ok();
        for &(hwnd, x, from, to, w, h) in &slide.moves {
            let y = if t >= 1.0 { to } else { from + ((to - from) as f64 * k).round() as i32 };
            if let Some(d) = defer {
                defer = DeferWindowPos(d, hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE).ok();
            }
        }
        if let Some(d) = defer {
            let _ = EndDeferWindowPos(d);
        }
    }
    if t >= 1.0 {
        finish_slide(ins);
    } else {
        super::after(std::time::Duration::from_millis(15), slide_step);
    }
}

/// Ends a slide at once: everything at its place, the revealed rows shown.
fn finish_slide(ins: &mut Inspector) {
    let Some(slide) = ins.sliding.take() else { return };
    unsafe {
        for &(hwnd, x, _, to, w, h) in &slide.moves {
            let _ = MoveWindow(hwnd, x, to, w, h, true);
        }
        for &hwnd in &slide.reveal {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

fn scroll_to(ins: &mut Inspector, target: i32) {
    finish_slide(ins);
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
        for top in ins.placed.values_mut() {
            *top += delta;
        }
        let info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_POS, nPos: target, ..Default::default() };
        SetScrollInfo(ins.rows_panel, SB_VERT, &info, true);
    }
}

/// Scrolls the rows, or the crop panel's, so the focused control's row is in
/// view. Only after keys: a click may be starting a drag.
pub fn reveal_focus(app: &mut App) {
    let ins = &mut app.inspector;
    let crop_rows = ins.crop.as_ref().map(|c| c.rows);
    unsafe {
        // The scrolling panel's child that holds the focus.
        let mut child = GetFocus();
        let panel = loop {
            let Ok(parent) = GetParent(child) else { return };
            if parent == ins.rows_panel || Some(parent) == crop_rows {
                break parent;
            }
            child = parent;
        };
        // Measured where a running slide ends.
        finish_slide(ins);
        // Top and bottom in the panel's client area, as scrolled now.
        let span = |hwnds: &[HWND]| {
            hwnds.iter().filter(|&&h| IsWindowVisible(h).as_bool()).fold(None, |span: Option<(i32, i32)>, &h| {
                let mut r = RECT::default();
                let _ = GetWindowRect(h, &mut r);
                let mut corners = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
                MapWindowPoints(None, panel, &mut corners);
                let (top, bottom) = (corners[0].y, corners[1].y);
                Some(span.map_or((top, bottom), |(t, b)| (t.min(top), b.max(bottom))))
            })
        };
        let margin = ins.px(8.0);
        let mut client = RECT::default();
        let _ = GetClientRect(panel, &mut client);
        // A row taller than the panel shows the focused control's top.
        let target = |scroll: i32, (top, bottom): (i32, i32), own: i32| {
            let (top, bottom) = (top - margin + scroll, bottom + margin + scroll);
            if top < scroll {
                Some(top)
            } else if bottom > scroll + client.bottom {
                Some((bottom - client.bottom).min(own - margin + scroll))
            } else {
                None
            }
        };
        let Some(own) = span(&[child]) else { return };
        if panel == ins.rows_panel {
            let row = ins.rows.iter().find(|r| r.controls.contains(&child));
            let whole = row.and_then(|r| span(&r.controls)).unwrap_or(own);
            if let Some(to) = target(ins.scroll, whole, own.0) {
                scroll_to(ins, to);
            }
        } else if let Some(crop) = ins.crop.as_mut() {
            // The angle's label is above it.
            let whole = if child == crop.get(C_ANGLE) { span(&[child, crop.get(C_LABELS + 5)]).unwrap_or(own) } else { own };
            if let Some(to) = target(crop.scroll, whole, own.0) {
                scroll_crop(crop, to);
            }
        }
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
    draw::enable(hwnd, on);
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
        Kind::ExposureOffset => {
            let (low, high) = model.exposure_offset_range();
            Some((model.exposure_offset(), 0.0, low, high, state::PREP_EXPOSURE_STEP))
        }
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
    let stocks_key = format!("{}|{}", grid_key(model, false), grid_key(model, true));
    let refill_stocks = ins.stocks_filled != stocks_key;
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
            Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
                let Some((value, default, low, _high, step)) = slider_value(model, &row.kind) else { continue };
                if let Kind::Slider(Adjustment::Slider { title, path, .. }) = row.kind
                    && let Some(bw) = state::bw_title(path)
                {
                    set_text(c[0], if model.film_is_bw() { bw } else { title });
                }
                let pos = ((value - low) / step).round() as isize;
                draw::set_default_pos(c[1], ((default - low) / step).round() as isize);
                unsafe {
                    if SendMessageW(c[1], TBM_GETPOS, WPARAM(0), LPARAM(0)).0 != pos {
                        SendMessageW(c[1], TBM_SETPOS, WPARAM(1), LPARAM(pos));
                    }
                    if GetFocus() != c[2] {
                        set_text(c[2], &format(value, step, decimal));
                    }
                    let modified = match row.kind {
                        Kind::Slider(Adjustment::Slider { path, .. }) => model.is_modified(path),
                        Kind::ExposureOffset => model.exposure_offset_modified(),
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
            Kind::MatchBrightness => {
                let raw = model.is_raw;
                set_text(c[0], if raw { "Match camera brightness" } else { "Match original brightness" });
                super::dialogs::tooltip(c[0], if raw {
                    "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it."
                } else {
                    "Develops the photograph about as bright as the file itself, and keeps it there when you change film, paper or a preset. Exposure adds to it."
                });
                set_check(c[0], model.matches_brightness());
                enable(c[0], on);
            }
            Kind::AllControls => {
                set_check(c[0], model.prefs.lab_all_controls);
                enable(c[0], catalog);
            }
            Kind::FilmFormat => {
                let selected = if model.film_format_custom() {
                    state::FILM_FORMATS.len()
                } else {
                    state::film_format(json::number(model.value(state::FILM_FORMAT.path()))).unwrap_or(state::FILM_FORMATS.len())
                } as isize;
                unsafe {
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != selected {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(selected as usize), LPARAM(0));
                    }
                }
                enable_all(c, on);
            }
            Kind::PaperSkipped => {}
            Kind::ToneGraph => {
                // Not off with the print curves: a contrast drag turns them on.
                let on = catalog && !scanning;
                let channel = super::tone_graph::channel(&model.prefs.tone_graph_channel);
                for (j, &b) in c[2..].iter().enumerate() {
                    set_check(b, j.checked_sub(1) == channel);
                }
                enable_all(c, on);
                super::tone_graph::update(c[1], model, on, ins.dpi, decimal);
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
            Kind::Note(_) => {}
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
                if !paper && !ins.film_search.is_invalid() {
                    enable(ins.film_search, catalog);
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
        ins.stocks_filled = stocks_key;
    }
    ins.dev_times = [stock_times(model, false), stock_times(model, true)];
}

fn enable_all(controls: &[HWND], on: bool) {
    for &c in controls {
        enable(c, on);
    }
}

/// What a grid or menu was built from: the catalog and the favourites.
fn grid_key(model: &Model, paper: bool) -> String {
    let catalog = model.stocks.len() + model.stocks.iter().map(|s| s.id.len()).sum::<usize>();
    format!("{catalog}|{}", model.prefs.favorites(paper).join(","))
}

/// The film or paper menu's entries: the favourites, starred, then each
/// group's heading (None, can't be chosen) and stocks. A favourite stays in
/// its group too; the closed menu shows that entry, with the plain name.
fn stock_entries(model: &Model, paper: bool) -> Vec<(String, Option<(String, bool)>)> {
    let (films, papers) = (model.films(), model.papers());
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    let mut entries: Vec<(String, Option<(String, bool)>)> =
        model.favorite_stocks(paper).iter().map(|s| (format!("★ {}", s.name), Some((s.id.clone(), true)))).collect();
    for group in groups {
        entries.push((group.title.into(), None));
        entries.extend(group.stocks.iter().map(|s| (s.name.clone(), Some((s.id.clone(), false)))));
    }
    entries
}

/// The film or paper list, with group names as headers that can't be chosen.
fn fill_stocks(combo: HWND, model: &Model, paper: bool) {
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        for (text, entry) in stock_entries(model, paper) {
            let text = wide(&text);
            let i = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize)).0;
            if entry.is_none() {
                SendMessageW(combo, CB_SETITEMDATA, WPARAM(i as usize), LPARAM(1));
            }
        }
    }
}

/// The catalog id of the stock at `index` in the list, or None for a header.
fn stock_at(model: &Model, paper: bool, index: usize) -> Option<String> {
    stock_entries(model, paper).into_iter().nth(index).and_then(|(_, entry)| entry).map(|(id, _)| id)
}

fn select_stock(combo: HWND, id: &str, model: &Model, paper: bool) {
    let index = stock_entries(model, paper)
        .iter()
        .position(|(_, entry)| entry.as_ref().is_some_and(|(stock, favorite)| stock == id && !favorite))
        .map_or(-1, |i| i as isize);
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
        Kind::ExposureOffset => app.model.set_exposure_offset(value),
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
        Kind::ExposureOffset => app.model.reset_exposure_offset(),
        _ => {}
    }
}

fn row_modified(app: &App, index: usize) -> bool {
    match app.inspector.rows[index].kind {
        Kind::Slider(a) => app.model.is_modified(a.path()),
        Kind::Toggle(_, path) | Kind::Choice(_, path, _) => app.model.is_modified(path),
        Kind::BorderSlider(_, field) => (field.get(&app.model.state.border) - field.get(&crate::border::BorderSettings::default())).abs() > 1e-9,
        Kind::ExposureOffset => app.model.exposure_offset_modified(),
        _ => false,
    }
}

fn command(app: &mut App, id: u16, code: u32, hwnd: HWND) {
    if let Some((_, next)) = TABS.iter().find(|(tab, _)| *tab == id) {
        if app.model.prefs.tab() != *next || !app.model.prefs.inspector_tab_chosen {
            app.model.prefs.choose_tab(next);
            app.model.prefs.save();
            app.inspector.scroll = 0;
            app.model.dirty |= dirty::STRUCTURE;
        }
        return;
    }
    if id == SCOPE_MODE {
        if code == CBN_SELCHANGE {
            let waveform = unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 } == 1;
            app.model.prefs.scope_mode = if waveform { "Waveform" } else { "Histogram" }.into();
            app.model.prefs.save();
            app.model.dirty |= dirty::HISTOGRAM | dirty::INSPECTOR;
        }
        return;
    }
    if id == RESET_ALL {
        app.model.reset();
        return;
    }
    if id == FILM_SEARCH {
        if code == EN_CHANGE {
            film_search_changed(app, window_text(hwnd));
        }
        return;
    }
    let Some((index, part)) = row_of(id) else { return };
    let Some(row) = app.inspector.rows.get(index) else { return };
    let kind = row.kind;
    let checked = || unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 };
    let selection = || unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    match (kind, part) {
        (Kind::Group { title, .. }, HEADER) if code == BN_CLICKED => {
            app.inspector.slide = true;
            let open = !app.model.prefs.expanded(title);
            app.model.prefs.set_expanded(title, open);
            app.model.prefs.save();
            app.model.dirty |= dirty::STRUCTURE;
        }
        (Kind::Group { enable: Some(path), .. }, CHECK) if code == BN_CLICKED => app.model.set(path, Value::Bool(checked())),
        (Kind::Toggle(_, path), CHECK) if code == BN_CLICKED => app.model.set(path, Value::Bool(checked())),
        (Kind::MatchBrightness, CHECK) if code == BN_CLICKED => app.model.set_matches_brightness(checked()),
        (Kind::AllControls, CHECK) if code == BN_CLICKED => {
            app.model.prefs.lab_all_controls = checked();
            app.model.prefs.save();
            app.model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR;
        }
        (Kind::FilmFormat, COMBO) if code == CBN_SELCHANGE => {
            let index = selection();
            app.model.choose_film_format(usize::try_from(index).ok().filter(|&i| i < state::FILM_FORMATS.len()));
        }
        (Kind::BalancePrint(..), ACTION) if code == BN_CLICKED => app.model.balance_print(),
        (Kind::ToneGraph, p) if code == BN_CLICKED && (p as usize) < super::tone_graph::CHANNELS.len() => {
            app.model.prefs.tone_graph_channel = super::tone_graph::CHANNELS[p as usize].into();
            app.model.prefs.save();
            app.model.dirty |= dirty::INSPECTOR;
        }
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
        (Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset, RESET) if code == BN_CLICKED => reset_row(app, index),
        (_, LABEL) if code == STN_DBLCLK && !draw::is_dimmed(hwnd) => reset_row(app, index),
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
        (Kind::Foldout(paper), ACTION) if code == BN_CLICKED => {
            app.inspector.slide = true;
            app.model.toggle_foldout(paper);
        }
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
pub(super) fn choose_color(owner: HWND, current: BorderColor) -> Option<BorderColor> {
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
    if !matches!(app.inspector.rows[index].kind, Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset | Kind::Toggle(..) | Kind::Choice(..)) {
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
                if id == SCOPE_MODE && matches!(code, CBN_DROPDOWN | CBN_CLOSEUP) {
                    // The scope's menu, as the column's drop-down lists.
                    if code == CBN_DROPDOWN && theme::dark() {
                        theme::combo_list(HWND(lparam.0 as *mut _));
                    }
                    let _ = InvalidateRect(HWND(lparam.0 as *mut _), None, false);
                }
                if code == BN_CLICKED || (id == SCOPE_MODE && code == CBN_SELCHANGE) {
                    with_app(|app| command(app, id, code, HWND(lparam.0 as *mut _)));
                }
                LRESULT(0)
            }
            WM_CTLCOLORLISTBOX => {
                let dc = HDC(wparam.0 as *mut _);
                let p = theme::palette();
                SetTextColor(dc, COLORREF(p.text));
                SetBkColor(dc, COLORREF(p.control));
                LRESULT(theme::brush(p.control).0 as isize)
            }
            WM_CTLCOLORSTATIC => {
                // The scope's channel label.
                let dc = HDC(wparam.0 as *mut _);
                let p = theme::palette();
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(p.secondary));
                LRESULT(theme::brush(p.panel).0 as isize)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => draw::erase(hwnd, wparam, theme::palette().panel),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn rows_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if code == CBN_DROPDOWN {
                    if theme::dark() {
                        theme::combo_list(HWND(lparam.0 as *mut _));
                    }
                    let _ = InvalidateRect(HWND(lparam.0 as *mut _), None, false);
                }
                if code == CBN_CLOSEUP {
                    let _ = InvalidateRect(HWND(lparam.0 as *mut _), None, false);
                }
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
                    if hwnd != ins.rows_panel {
                        return;
                    }
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
                    if hwnd == ins.rows_panel {
                        let target = ins.scroll - delta * ins.px(60.0) / 120;
                        scroll_to(ins, target);
                    }
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
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                draw::paint_panel(hwnd, theme::palette().panel);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => draw::color_edit(HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _)),
            WM_CTLCOLORLISTBOX => {
                // A drop-down list's colours.
                let dc = HDC(wparam.0 as *mut _);
                let p = theme::palette();
                SetTextColor(dc, COLORREF(p.text));
                SetBkColor(dc, COLORREF(p.control));
                LRESULT(theme::brush(p.control).0 as isize)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                let control = HWND(lparam.0 as *mut _);
                if draw::is_field(control) {
                    return draw::color_edit(dc, control);
                }
                let id = GetDlgCtrlID(control) as u16;
                SetBkMode(dc, TRANSPARENT);
                let p = theme::palette();
                let footer = row_of(id).is_some_and(|(index, _)| with_footer(index));
                let color = if draw::is_dimmed(control) { p.disabled } else if footer { p.secondary } else { p.text };
                SetTextColor(dc, COLORREF(color));
                LRESULT(theme::brush(p.panel).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Footers are drawn grey; a static row index tells which rows are footers.
fn with_footer(index: usize) -> bool {
    thread_local! {
        static FOOTERS: Vec<bool> = rows().iter().map(|r| matches!(r.kind, Kind::Footer(_) | Kind::Note(_) | Kind::ToneGraph | Kind::PaperSkipped)).collect();
    }
    FOOTERS.with(|f| f.get(index).copied().unwrap_or(false))
}

fn draw_item(item: &DRAWITEMSTRUCT) {
    unsafe {
        let r = item.rcItem;
        let p = theme::palette();
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let focused = item.itemState.0 & ODS_FOCUS.0 != 0 && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0;
        let mut text = [0u16; 128];
        if item.CtlType == ODT_COMBOBOX {
            if item.itemID == u32::MAX {
                return;
            }
            let header = SendMessageW(item.hwndItem, CB_GETITEMDATA, WPARAM(item.itemID as usize), LPARAM(0)).0 == 1;
            let n = SendMessageW(item.hwndItem, CB_GETLBTEXT, WPARAM(item.itemID as usize), LPARAM(text.as_mut_ptr() as isize)).0.max(0) as usize;
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0 && !header;
            let in_edit = item.itemState.0 & ODS_COMBOBOXEDIT.0 != 0;
            let s = draw::dpi_of(item.hwndItem);
            let mut c = Canvas::new(item.hDC, r);
            c.clear(p.control);
            if selected && !in_edit {
                c.round_rect(RectF::of(c.rect()).inset(2.0 * s, s), 3.0 * s, p.accent);
            }
            let ink = if disabled { p.disabled } else if selected && !in_edit { p.on_accent } else if header { p.secondary } else { p.text };
            let indent = (if header || in_edit { 4.0 } else { 14.0 } * s) as i32;
            let label = String::from_utf16_lossy(&text[..n]);
            let tr = RECT { left: indent, top: 0, right: c.w, bottom: c.h };
            c.text(&label, tr, draw::font_of(item.hwndItem), ink, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            c.present(item.hDC);
            return;
        }
        let id = GetDlgCtrlID(item.hwndItem) as u16;
        let Some((_, part)) = row_of(id) else { return };
        let n = GetWindowTextW(item.hwndItem, &mut text) as usize;
        let title = String::from_utf16_lossy(&text[..n]);
        let s = draw::dpi_of(item.hwndItem);
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let mut c = Canvas::new(item.hDC, r);
        c.clear(p.panel);
        let whole = RectF::of(c.rect());
        let label_color = if disabled { p.disabled } else { p.text };
        if part == ACTION {
            draw::header_background(&mut c, item.hwndItem, pressed, disabled);
            draw_foldout(&mut c, item, &title, label_color);
        } else if part == HEADER && n > 0 && !["Color", "Line color"].contains(&title.as_str()) {
            // A group header: chevron, then the title.
            draw::header_background(&mut c, item.hwndItem, pressed, disabled);
            let open = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA) != 0;
            let height = c.h as f32;
            draw::chevron(&mut c, 12.0 * s, height / 2.0, open, s, if disabled { p.disabled } else { p.secondary });
            let tr = RECT { left: (24.0 * s) as i32, top: 0, right: c.w - (4.0 * s) as i32, bottom: c.h };
            c.text(&title, tr, draw::font_of(item.hwndItem), label_color, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        } else if part == HEADER {
            // A colour swatch button.
            let (mat, line, _) = SWATCHES.with(|s| s.get());
            let color = if title == "Line color" { line } else { mat };
            let fill = if pressed { p.control_pressed } else if draw::is_hot(item.hwndItem) && !disabled { p.control_hover } else { p.control };
            c.round_rect(whole, 4.0 * s, p.border);
            c.round_rect(whole.inset(s, s), 3.0 * s, fill);
            let swatch = whole.inset(5.0 * s, 5.0 * s);
            c.round_rect(swatch, 2.0 * s, if disabled { p.border } else { color });
            c.round_frame(swatch, 2.0 * s, s, theme::mix(p.border, p.strong_border, 0.3));
        } else {
            // A border style sample with its caption.
            let (mat, line, selected) = SWATCHES.with(|s| s.get());
            let style = part as usize;
            let caption = (20.0 * s) as i32;
            let sample = RECT { left: (6.0 * s) as i32, top: (6.0 * s) as i32, right: c.w - (6.0 * s) as i32, bottom: c.h - caption - (2.0 * s) as i32 };
            if style as u32 == selected {
                c.round_frame(RectF::of(sample).inset(-4.0 * s, -4.0 * s), 6.0 * s, 2.0 * s, if disabled { p.disabled } else { p.accent });
            } else if draw::is_hot(item.hwndItem) && !disabled {
                c.round_rect(RectF::of(sample).inset(-4.0 * s, -4.0 * s), 6.0 * s, p.subtle_hover);
            }
            draw_swatch(&mut c, sample, style, mat, line, disabled);
            let tr = RECT { top: c.h - caption, ..c.rect() };
            c.text(&title, tr, draw::font_of(item.hwndItem), if disabled { p.disabled } else if style as u32 == selected { p.text } else { p.secondary }, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        }
        if focused {
            draw::focus_ring(&mut c, whole, 4.0 * s, s);
        }
        c.present(item.hDC);
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
fn draw_swatch(c: &mut Canvas, r: RECT, style: usize, mat: u32, line: u32, disabled: bool) {
    let p = theme::palette();
    let s = ((r.bottom - r.top) / 9).max(2) as f32;
    let rf = RectF::of(r);
    let fill = |c: &mut Canvas, rect: RectF, color: u32| c.round_rect(rect, 0.0, color);
    let photo = |c: &mut Canvas, rect: RectF| {
        // The gradient, a band per row.
        let (top, bottom) = (rect.y.round() as i32, (rect.y + rect.h).round() as i32);
        for y in top..bottom {
            let t = (y - top) as f64 / (bottom - top).max(1) as f64;
            let color = theme::mix(theme::rgb(0x8c, 0xad, 0xc7), theme::rgb(0xdb, 0xbd, 0x8f), t);
            c.round_rect(RectF::new(rect.x, y as f32, rect.w, 1.0), 0.0, color);
        }
    };
    let corner = 3.0 * draw::dpi_of(super::main_hwnd());
    match style {
        0 => {
            fill(c, rf, mat);
            photo(c, rf.inset(s + 1.0, s + 1.0));
        }
        1 => {
            fill(c, rf, 0);
            photo(c, rf.inset(s / 2.0 + 1.0, s / 2.0 + 1.0));
        }
        2 => {
            fill(c, rf, mat);
            let inner = rf.inset(s, s);
            fill(c, inner, line);
            photo(c, inner.inset(1.0, 1.0));
        }
        _ => {
            fill(c, rf, 0);
            let band = rf.h / 5.0;
            for edge in [rf.y + band / 4.0, rf.y + rf.h - band * 3.0 / 4.0] {
                let mut x = rf.x + 2.0;
                while x + 3.0 < rf.x + rf.w {
                    c.round_rect(RectF::new(x, edge, 3.0, band / 2.0), 0.5, 0xf2f2f2);
                    x += 6.0;
                }
            }
            photo(c, RectF::new(rf.x + 1.0, rf.y + band, rf.w - 2.0, rf.h - 2.0 * band));
        }
    }
    c.round_corners(rf, corner, p.panel);
    c.round_frame(rf, corner, 1.0, theme::mix(p.border, p.strong_border, 0.3));
    if disabled {
        // Faded like the Mac's disabled samples.
        c.round_rect_alpha(rf.inset(-1.0, -1.0), corner, p.panel, 0.62);
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
        // `paint_trackbar` covers the whole control. Its class erases in
        // the system's light button face, which showed as white bars when
        // the rows moved (dragging the scope's grabber, opening a foldout).
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
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
        // The control repaints only its own narrow thumb when it moves,
        // leaving pieces of the round thumb and the accent fill behind:
        // the whole slider repaints when its position, press or focus changes.
        if !matches!(msg, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_CAPTURECHANGED | WM_KEYDOWN | WM_TIMER | WM_MOUSEWHEEL | WM_SETFOCUS | WM_KILLFOCUS | TBM_SETPOS) {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
        let state = || (DefSubclassProc(hwnd, TBM_GETPOS, WPARAM(0), LPARAM(0)).0, GetCapture() == hwnd);
        let before = state();
        let result = DefSubclassProc(hwnd, msg, wparam, lparam);
        if matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) || state() != before {
            let _ = InvalidateRect(hwnd, None, false);
        }
        result
    }
}

/// Whether `hwnd` is a drop-down list already at this place and width.
/// Layouts leave those alone: a move asks for the list's height; the box
/// sizes itself back to its field at once, and what it covered meanwhile
/// is erased and repainted, a flash of the controls below it.
fn combo_in_place(hwnd: HWND, x: i32, y: i32, width: i32) -> bool {
    if draw::class_of(hwnd) != "ComboBox" {
        return false;
    }
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        let mut points = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        MapWindowPoints(None, GetParent(hwnd).unwrap_or_default(), &mut points);
        (points[0].x, points[0].y, points[1].x - points[0].x) == (x, y, width)
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

/// An open foldout: the favourites, untitled, then headings and a
/// two-column grid of tiles, one owner-drawn button per stock, under the
/// menu's headings in catalog order. A favourite's tile is there twice. At
/// most `GRID_HEIGHT` tall, it scrolls inside itself. While the Film grid
/// searches, its matches show once each, under their groups, and the other
/// tiles hide: they and their thumbnails stay for when the search clears.
struct StockGrid {
    window: HWND,
    /// Each tile's stock id and button, in order.
    tiles: Vec<(String, HWND)>,
    /// Each heading (none above the favourites) and the index of the first
    /// tile under it.
    headings: Vec<(Option<HWND>, usize)>,
    /// The catalog and favourites it was built from (`grid_key`).
    key: String,
    scroll: i32,
    content: i32,
    view: i32,
    /// It was on screen at the last layout; opening it again scrolls to the
    /// current stock.
    was_open: bool,
    /// The tile to keep in place at the next layout (`pointed_tile`).
    anchor: Option<(String, bool, i32)>,
    /// The search the tiles are filtered for, as typed, and which tiles
    /// match it (never the favourites' copies); None while not searching.
    query: String,
    matches: Option<Vec<bool>>,
    /// The Film grid's "No films match …" line.
    no_matches: Option<HWND>,
    /// The search was cleared: the next layout goes back to the current stock.
    recenter: bool,
}

/// A tile as drawn.
#[derive(Clone, Default)]
struct TileLook {
    image: Option<std::sync::Arc<crate::previews::Bitmap>>,
    dimmed: bool,
    selected: bool,
    favorite: bool,
}

const TILE_FIRST: u16 = 100;
/// The thumbnail's height, the caption and the gaps, in DIPs.
const TILE_IMAGE: f64 = 92.0;
const TILE_CAPTION: f64 = 18.0;
const TILE_GAP: f64 = 8.0;
const TILE_HEADING: f64 = 24.0;
/// The star over a thumbnail's corner.
const TILE_STAR: f64 = 24.0;

fn build_grid(parent: HWND, fonts: &super::Fonts, model: &Model, paper: bool, key: String) -> StockGrid {
    let window = create(w!("NegsStockGrid"), if paper { "Papers" } else { "Films" }, WS_CHILD | WS_CLIPCHILDREN | WS_VSCROLL, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), parent, 0, HFONT::default());
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, paper as isize);
    }
    let (films, papers) = (model.films(), model.papers());
    let favorites = model.favorite_stocks(paper);
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    let sections = std::iter::once((None, favorites)).chain(groups.into_iter().map(|g| (Some(g.title), g.stocks)));
    let (mut tiles, mut headings) = (vec![], vec![]);
    for (title, stocks) in sections {
        if stocks.is_empty() {
            continue;
        }
        let heading = title.map(|title| create(w!("STATIC"), title, WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), window, 0, fonts.small));
        headings.push((heading, tiles.len()));
        for stock in stocks {
            let id = TILE_FIRST + tiles.len() as u16;
            let tile = create(w!("BUTTON"), &stock.name, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), window, id, fonts.small);
            draw::track_hover(tile);
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
            TileLook { image: thumb.and_then(|t| t.image.clone()), dimmed: thumb.is_some_and(|t| t.dimmed), selected: false, favorite: model.prefs.is_favorite(id, paper) }
        })
        .collect();
    TILES.with(|t| t.borrow_mut()[paper as usize] = looks);
    // A quiet line in the grid when nothing matches; hidden until then.
    let no_matches = (!paper).then(|| create(w!("STATIC"), "", WS_CHILD | WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0), WINDOW_EX_STYLE(0), window, 0, fonts.ui));
    let mut grid = StockGrid { window, tiles, headings, key, scroll: 0, content: 0, view: 0, was_open: false, anchor: None, query: String::new(), matches: None, no_matches, recenter: false };
    if !paper {
        filter_grid(&mut grid, model);
    }
    select_tile(&grid, if paper { &model.state.paper } else { &model.state.film });
    grid
}

/// Which of the Film grid's tiles the search shows (`state::matching`):
/// each match once, under its group, so never the favourites' copies.
/// None while the search is empty or only spaces.
fn search_matches(grid: &StockGrid, model: &Model, query: &str) -> Option<Vec<bool>> {
    if query.trim().is_empty() {
        return None;
    }
    let (films, papers) = (model.films(), model.papers());
    let matched: std::collections::HashSet<&str> =
        state::matching(query, state::film_groups(&films, &papers)).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.as_str())).collect();
    Some((0..grid.tiles.len()).map(|i| !among_favorites(grid, i) && matched.contains(grid.tiles[i].0.as_str())).collect())
}

/// Filters the Film grid for the model's search, hiding rather than
/// rebuilding tiles; the next layout places what shows.
fn filter_grid(grid: &mut StockGrid, model: &Model) {
    let query = model.previews.film_query.clone();
    grid.matches = search_matches(grid, model, &query);
    if let Some(label) = grid.no_matches {
        set_text(label, &format!("No films match \u{201c}{}\u{201d}", query.trim()));
    }
    // The tile under the pointer may hide.
    if HOVER_TILE.get().is_some_and(|(paper, _)| !paper) {
        HOVER_TILE.set(None);
    }
    grid.query = query;
}

/// The Film grid's search field: "Search films" until something is typed.
fn create_film_search(parent: HWND, fonts: &super::Fonts, model: &Model) -> HWND {
    let search = create(w!("EDIT"), &model.previews.film_query, WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WINDOW_EX_STYLE(0), parent, FILM_SEARCH, fonts.ui);
    draw::set_role(search, Role::Field);
    draw::track_hover(search);
    super::accessible_name(search, "Search films");
    let prompt = wide("Search films");
    unsafe {
        // Shown while focused too, as the Mac's placeholder is.
        SendMessageW(search, EM_SETCUEBANNER, WPARAM(1), LPARAM(prompt.as_ptr() as isize));
        let _ = SetWindowSubclass(search, Some(film_search_subclass), 1, 0);
    }
    search
}

/// The field and the Film grid follow the model's search when something
/// other than typing changed it. True when the grid was filtered again.
fn sync_film_search(ins: &mut Inspector, model: &Model) -> bool {
    let query = &model.previews.film_query;
    if !ins.film_search.is_invalid() && window_text(ins.film_search) != *query {
        set_text(ins.film_search, query);
    }
    match ins.grids[0].as_mut() {
        Some(grid) if grid.query != *query => {
            filter_grid(grid, model);
            true
        }
        _ => false,
    }
}

/// Typing in the search field filters the Film grid at once, and never
/// edits the photograph. Each search starts at the top of its matches;
/// clearing it goes back to the current film.
pub(crate) fn film_search_changed(app: &mut App, text: String) {
    if app.model.previews.film_query == text {
        return;
    }
    let cleared = text.is_empty();
    app.model.previews.film_query = text;
    // The thumbnails asked for follow the matches.
    app.model.dirty |= dirty::PREVIEWS;
    let ins = &mut app.inspector;
    let (limit, dpi) = (ins.px(GRID_HEIGHT), ins.dpi);
    let Some(grid) = ins.grids[0].as_mut() else { return };
    filter_grid(grid, &app.model);
    if cleared {
        grid.recenter = true;
    } else {
        grid.scroll = 0;
    }
    grid.anchor = None;
    // Only the grid moves: it keeps its height while searching, so the
    // field and the rows below stay where they are, undrawn.
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(grid.window, &mut r);
    }
    if layout_grid(grid, r.right - r.left, limit, dpi) != r.bottom - r.top {
        layout_rows(ins, &app.model);
    }
}

/// Return in the search field chooses the first match, as listed: one
/// undo step, as clicking its tile makes.
pub(crate) fn choose_first_match(app: &mut App) {
    let query = app.model.previews.film_query.clone();
    if query.trim().is_empty() {
        return;
    }
    let (films, papers) = (app.model.films(), app.model.papers());
    let first = state::matching(&query, state::film_groups(&films, &papers)).first().and_then(|g| g.stocks.first()).map(|s| s.id.clone());
    if let Some(id) = first {
        app.model.choose_film(&id);
    }
}

/// The search field: Return chooses the first match, Escape clears it.
extern "system" fn film_search_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_GETDLGCODE if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_RETURN | VK_ESCAPE) => LRESULT(DLGC_WANTALLKEYS as isize),
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                post(choose_first_match);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                // Sends EN_CHANGE, which clears the search.
                if GetWindowTextLengthW(hwnd) > 0 {
                    set_text(hwnd, "");
                }
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => LRESULT(0),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Where the headings and tiles go for `width`, and the content's height:
/// those the search shows if `filtered`, else every one. A heading shows
/// only over tiles; with none, the "No films match" line does.
fn grid_positions(grid: &StockGrid, width: i32, dpi: u32, filtered: bool) -> (Vec<(HWND, i32, i32, i32, i32)>, i32) {
    let px = |dip: f64| (dip * dpi as f64 / 96.0).round() as i32;
    let gap = px(TILE_GAP);
    let column = ((width - gap) / 2).max(1);
    let tile = px(TILE_IMAGE + TILE_CAPTION);
    let matches = grid.matches.as_ref().filter(|_| filtered);
    let mut y = 0;
    let mut positions = vec![];
    for (g, &(heading, first)) in grid.headings.iter().enumerate() {
        let end = grid.headings.get(g + 1).map_or(grid.tiles.len(), |h| h.1);
        let shown: Vec<HWND> = (first..end).filter(|&i| matches.is_none_or(|m| m[i])).map(|i| grid.tiles[i].1).collect();
        if shown.is_empty() {
            continue;
        }
        match heading {
            Some(heading) => {
                positions.push((heading, 0, y + px(6.0), width, px(16.0)));
                y += px(TILE_HEADING);
            }
            None => y += px(4.0),
        }
        for (i, button) in shown.iter().enumerate() {
            let (col, row) = (i as i32 % 2, i as i32 / 2);
            positions.push((*button, col * (column + gap), y + row * (tile + gap), column, tile));
        }
        let rows = shown.len().div_ceil(2) as i32;
        y += rows * tile + (rows - 1).max(0) * gap + px(2.0);
    }
    if let (Some(_), Some(label), true) = (matches, grid.no_matches, positions.is_empty()) {
        positions.push((label, 0, px(24.0), width, px(20.0)));
        y = px(24.0 + 20.0);
    }
    (positions, y)
}

/// Places the headings and tiles for `width`, at most `limit` tall with a
/// scroll bar beyond it; returns the grid's height. Opening it shows the
/// current stock's tile in the middle.
fn layout_grid(grid: &mut StockGrid, width: i32, limit: i32, dpi: u32) -> i32 {
    // While searching the grid keeps its height, as the Mac's fixed frame
    // does: the rows below stay put as the matches change.
    let view = if grid.matches.is_some() { grid_positions(grid, width, dpi, false).1.min(limit) } else { limit };
    let (mut positions, mut content) = grid_positions(grid, width, dpi, true);
    if content > view {
        let bar = unsafe { windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXVSCROLL, dpi) };
        (positions, content) = grid_positions(grid, width - bar, dpi, true);
    }
    grid.content = content;
    grid.view = if grid.matches.is_some() { view } else { content.min(limit) };
    if let Some((id, favorite, top)) = grid.anchor.take() {
        let index = (0..grid.tiles.len()).find(|&i| grid.tiles[i].0 == id && among_favorites(grid, i) == favorite);
        if let Some(&(_, _, y, _, _)) = index.and_then(|i| positions.iter().find(|p| p.0 == grid.tiles[i].1)) {
            grid.scroll = y - top;
        }
    } else if !grid.was_open || std::mem::take(&mut grid.recenter) {
        // The current stock's first tile on show: the favourites' copy if
        // it is starred, unless a search hides that.
        let selected: Vec<usize> = TILES.with(|t| {
            let paper = unsafe { GetWindowLongPtrW(grid.window, GWLP_USERDATA) } != 0;
            t.borrow()[paper as usize].iter().enumerate().filter(|(_, look)| look.selected).map(|(i, _)| i).collect()
        });
        let tile = selected.iter().find_map(|&i| positions.iter().find(|p| p.0 == grid.tiles[i].1));
        grid.scroll = tile.map_or(0, |&(_, _, y, _, h)| y + h / 2 - grid.view / 2);
    }
    grid.scroll = grid.scroll.clamp(0, (grid.content - grid.view).max(0));
    // What the search hides goes first, then the rest move, then what it
    // brings back shows in its place.
    let wanted = |hwnd: HWND| positions.iter().any(|p| p.0 == hwnd);
    let children: Vec<HWND> = grid.tiles.iter().map(|t| t.1).chain(grid.headings.iter().filter_map(|h| h.0)).chain(grid.no_matches).collect();
    for &child in &children {
        if !wanted(child) {
            show_child(child, false);
        }
    }
    place(&positions, grid.scroll);
    for &child in &children {
        if wanted(child) {
            show_child(child, true);
        }
    }
    unsafe {
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: grid.content,
            nPage: grid.view.max(0) as u32,
            nPos: grid.scroll,
            ..Default::default()
        };
        SetScrollInfo(grid.window, SB_VERT, &info, true);
        let _ = InvalidateRect(grid.window, None, true);
    }
    grid.view
}

/// Shows or hides a grid's tile, heading or line, if it isn't already.
fn show_child(hwnd: HWND, show: bool) {
    unsafe {
        let visible = GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0;
        if visible != show {
            let _ = ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
        }
    }
}

/// Whether the tile at `index` is among the favourites, the untitled section.
fn among_favorites(grid: &StockGrid, index: usize) -> bool {
    match grid.headings.first() {
        Some(&(None, _)) => index < grid.headings.get(1).map_or(grid.tiles.len(), |h| h.1),
        _ => false,
    }
}

/// The tile under the pointer: its stock, whether it is the copy among the
/// favourites, and its top in the grid's view. Starring adds a tile above
/// it; the rebuilt grid scrolls so this one stays under the pointer.
fn pointed_tile(grid: &StockGrid) -> Option<(String, bool, i32)> {
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        let _ = ScreenToClient(grid.window, &mut p);
        grid.tiles.iter().enumerate().find_map(|(i, (id, tile))| {
            // A tile the search hides keeps its old place.
            if GetWindowLongW(*tile, GWL_STYLE) as u32 & WS_VISIBLE.0 == 0 {
                return None;
            }
            let mut r = RECT::default();
            let _ = GetWindowRect(*tile, &mut r);
            let mut corners = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
            MapWindowPoints(None, grid.window, &mut corners);
            let inside = p.x >= corners[0].x && p.x < corners[1].x && p.y >= corners[0].y && p.y < corners[1].y;
            inside.then(|| (id.clone(), among_favorites(grid, i), corners[0].y))
        })
    }
}

/// Scrolls an open grid to `target`.
fn scroll_grid(grid: &mut StockGrid, target: i32) {
    let target = target.clamp(0, (grid.content - grid.view).max(0));
    if target == grid.scroll {
        return;
    }
    let delta = grid.scroll - target;
    grid.scroll = target;
    unsafe {
        ScrollWindowEx(grid.window, 0, delta, None, None, None, None, SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE);
        let info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_POS, nPos: target, ..Default::default() };
        SetScrollInfo(grid.window, SB_VERT, &info, true);
    }
}

/// Scrolls a grid so a focused tile is in view.
fn reveal_tile(grid: &mut StockGrid, tile: HWND) {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(tile, &mut r);
        let mut top_left = POINT { x: r.left, y: r.top };
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(grid.window, &mut top_left);
        let (top, height) = (top_left.y + grid.scroll, r.bottom - r.top);
        if top < grid.scroll {
            scroll_grid(grid, top);
        } else if top + height > grid.scroll + grid.view {
            scroll_grid(grid, top + height - grid.view);
        }
    }
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
                let favorite = if look.favorite { ", favorite" } else { "" };
                super::accessible_name(*button, &if now { format!("{name}, current{favorite}") } else { format!("{name}{favorite}") });
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
fn draw_foldout(c: &mut Canvas, item: &DRAWITEMSTRUCT, title: &str, color: u32) {
    unsafe {
        let p = theme::palette();
        let s = draw::dpi_of(item.hwndItem);
        let data = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA);
        let (open, paper) = (data & 1 != 0, data & 2 != 0);
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let height = c.h as f32;
        draw::chevron(c, 12.0 * s, height / 2.0, open, s, if disabled { p.disabled } else { p.secondary });
        let font = draw::font_of(item.hwndItem);
        let mut measure = RECT::default();
        let mut wide_title: Vec<u16> = title.encode_utf16().collect();
        let old = SelectObject(c.dc, font);
        DrawTextW(c.dc, &mut wide_title, &mut measure, DT_SINGLELINE | DT_CALCRECT);
        SelectObject(c.dc, old);
        let left = (24.0 * s) as i32;
        c.text(title, RECT { left, top: 0, right: c.w, bottom: c.h }, font, color, DT_SINGLELINE | DT_VCENTER);
        let name = FOLDOUT_NAMES.with(|n| n.borrow()[paper as usize].clone());
        let nr = RECT { left: left + measure.right + (16.0 * s) as i32, top: 0, right: c.w - (8.0 * s) as i32, bottom: c.h };
        c.text(&name, nr, font, if disabled { p.disabled } else { p.secondary }, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS);
    }
}

/// A tile: the thumbnail cropped to fill a rounded frame (grey until it
/// arrives, dimmed while out of date), the accent outline on the current
/// stock, and the name below, cut with an ellipsis.
fn draw_tile(item: &DRAWITEMSTRUCT, look: &TileLook, hovered: bool) {
    let p = theme::palette();
    let s = draw::dpi_of(item.hwndItem);
    let px = |dip: f64| (dip * s as f64).round() as i32;
    let mut c = Canvas::new(item.hDC, item.rcItem);
    c.clear(p.panel);
    let inset = px(2.0);
    let frame = RECT { left: inset, top: inset, right: c.w - inset, bottom: px(TILE_IMAGE) - inset };
    let radius = 6.0 * s;
    let hot = hovered || draw::is_hot(item.hwndItem);
    draw_thumbnail(&mut c, frame, radius, look.image.as_deref(), look.dimmed, p.well, p.panel);
    if look.selected {
        c.round_frame(RectF::of(frame).inset(-inset as f32, -inset as f32), radius + inset as f32, 2.0 * s, p.accent);
    } else if hot {
        c.round_frame(RectF::of(frame), radius, s, p.strong_border);
    }
    // The star sits over the thumbnail's corner, on the photograph, so its
    // colours don't follow the theme: on a favourite, or under the pointer.
    if look.favorite || hot {
        let star = RECT { left: frame.right - px(TILE_STAR), top: frame.top, right: frame.right, bottom: frame.top + px(TILE_STAR) };
        let shadow = RECT { left: star.left + 1, top: star.top + 1, right: star.right + 1, bottom: star.bottom + 1 };
        let glyph = if look.favorite { "\u{E735}" } else { "\u{E734}" };
        let font = HFONT(with_app_fonts_glyph().0);
        let center = DT_SINGLELINE | DT_VCENTER | DT_CENTER;
        c.text(glyph, shadow, font, theme::rgb(0x20, 0x20, 0x20), center);
        c.text(glyph, star, font, if look.favorite { theme::rgb(0xff, 0xc8, 0x00) } else { theme::rgb(0xff, 0xff, 0xff) }, center);
    }
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let ink = if disabled { p.disabled } else if look.selected || hot { p.text } else { p.secondary };
    let tr = RECT { left: inset, top: frame.bottom + px(4.0), right: c.w - inset, bottom: c.h };
    c.text(&draw::window_text(item.hwndItem), tr, draw::font_of(item.hwndItem), ink, DT_SINGLELINE | DT_TOP | DT_LEFT | DT_END_ELLIPSIS);
    if item.itemState.0 & ODS_FOCUS.0 != 0 && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0 {
        let whole = RectF::of(c.rect());
        draw::focus_ring(&mut c, whole, radius, s);
    }
    c.present(item.hDC);
}

/// A thumbnail cropped to fill `frame` with rounded corners, or the
/// placeholder; dimmed (55 % opacity over the background) while out of date.
pub fn draw_thumbnail(c: &mut Canvas, frame: RECT, radius: f32, image: Option<&crate::previews::Bitmap>, dimmed: bool, placeholder: u32, background: u32) {
    unsafe {
        let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
        if fw <= 0 || fh <= 0 {
            return;
        }
        let f = RectF::of(frame);
        c.round_rect(f, 0.0, placeholder);
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
            let dc = c.hdc();
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
                c.round_rect_alpha(f, 0.0, background, 0.45);
            }
        }
        c.round_corners(f, radius, background);
    }
}

/// The tile's grid (paper or not) and position.
fn tile_of(hwnd: HWND) -> (bool, usize) {
    unsafe {
        let paper = GetWindowLongPtrW(GetParent(hwnd).unwrap_or_default(), GWLP_USERDATA) != 0;
        (paper, (GetDlgCtrlID(hwnd) as u16).wrapping_sub(TILE_FIRST) as usize)
    }
}

/// Whether a point in the tile's client area is on its star.
fn on_star(hwnd: HWND, lparam: LPARAM) -> bool {
    unsafe {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as i32;
        let px = |dip: f64| (dip * dpi as f64 / 96.0).round() as i32;
        let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
        let right = r.right - px(2.0);
        x >= right - px(TILE_STAR) && x < right && y >= px(2.0) && y < px(2.0) + px(TILE_STAR)
    }
}

fn toggle_tile_favorite(hwnd: HWND) {
    let (paper, index) = tile_of(hwnd);
    post(move |app: &mut App| {
        let id = app.inspector.grids[paper as usize].as_ref().and_then(|g| g.tiles.get(index)).map(|t| t.0.clone());
        if let Some(id) = id {
            app.model.toggle_favorite(&id, paper);
        }
    });
}

/// Enter chooses a focused tile, as Space does: owner-drawn buttons aren't
/// push buttons to `IsDialogMessage`. A click on the star (or the context
/// menu, for the keyboard) stars or unstars the stock instead; the pointer
/// over a tile shows its star.
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
        match msg {
            // `draw_tile` covers the whole tile; erasing first blanked the
            // thumbnails for a frame whenever the grid moved.
            WM_ERASEBKGND => return LRESULT(1),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if on_star(hwnd, lparam) => {
                toggle_tile_favorite(hwnd);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                let tile = tile_of(hwnd);
                if HOVER_TILE.get() != Some(tile) {
                    HOVER_TILE.set(Some(tile));
                    let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut track);
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            WM_MOUSELEAVE => {
                if HOVER_TILE.get() == Some(tile_of(hwnd)) {
                    HOVER_TILE.set(None);
                }
                let _ = InvalidateRect(hwnd, None, false);
            }
            WM_CONTEXTMENU => {
                let (paper, index) = tile_of(hwnd);
                let favorite = TILES.with(|t| t.borrow()[paper as usize].get(index).is_some_and(|l| l.favorite));
                let menu = CreatePopupMenu().unwrap();
                let text = wide(if favorite { "Remove from &Favorites" } else { "Add to &Favorites" });
                let _ = AppendMenuW(menu, MF_STRING, 1, pcwstr(&text));
                let (mut x, mut y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
                if x == -1 && y == -1 {
                    let mut r = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut r);
                    (x, y) = (r.left, r.bottom);
                }
                let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, None);
                let _ = DestroyMenu(menu);
                if chosen.0 == 1 {
                    toggle_tile_favorite(hwnd);
                }
                return LRESULT(0);
            }
            WM_SETFOCUS => {
                let raw = hwnd.0 as isize;
                post(move |app: &mut App| {
                    let (paper, _) = tile_of(HWND(raw as *mut _));
                    if let Some(grid) = app.inspector.grids[paper as usize].as_mut() {
                        reveal_tile(grid, HWND(raw as *mut _));
                    }
                });
            }
            _ => {}
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
                draw_tile(item, &look, HOVER_TILE.get() == Some((paper, index)));
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                let p = theme::palette();
                SetTextColor(dc, COLORREF(p.secondary));
                LRESULT(theme::brush(p.panel).0 as isize)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_ERASEBKGND => draw::erase(hwnd, wparam, theme::palette().panel),
            WM_VSCROLL | WM_MOUSEWHEEL => {
                with_app(|app| {
                    let line = app.inspector.px(40.0);
                    let Some(grid) = app.inspector.grids[paper as usize].as_mut() else { return };
                    let target = if msg == WM_MOUSEWHEEL {
                        grid.scroll - ((wparam.0 >> 16) & 0xffff) as i16 as i32 * line * 3 / 2 / 120
                    } else {
                        let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_ALL, ..Default::default() };
                        let _ = GetScrollInfo(hwnd, SB_VERT, &mut info);
                        match SCROLLBAR_COMMAND((wparam.0 & 0xffff) as i32) {
                            SB_LINEUP => info.nPos - line,
                            SB_LINEDOWN => info.nPos + line,
                            SB_PAGEUP => info.nPos - info.nPage as i32,
                            SB_PAGEDOWN => info.nPos + info.nPage as i32,
                            SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
                            SB_TOP => 0,
                            SB_BOTTOM => i32::MAX / 2,
                            _ => info.nPos,
                        }
                    };
                    scroll_grid(grid, target);
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The scope on a dark well: the histogram's red, green and blue added, as
/// in photo editors (overlaps mix to cyan, magenta, yellow and white), or
/// the RGB waveform with its levels in percent at the left. Grid lines at a
/// quarter, half and three quarters.
extern "system" fn scope_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let (w, h) = (r.right.max(1) as usize, r.bottom.max(1) as usize);
                let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as usize;
                let well = 0x1a1a1a;
                let mut pixels = vec![well; w * h];
                let (has_photo, waveform) = SCOPE.with(|scope| {
                    let scope = scope.borrow();
                    if scope.2 {
                        // The levels' column, then the raster scaled into the plot.
                        let left = 26 * dpi / 96;
                        let (top, bottom) = (6 * dpi / 96, 6 * dpi / 96);
                        let (plot_w, plot_h) = (w.saturating_sub(left + 4).max(1), h.saturating_sub(top + bottom).max(1));
                        grid_lines(&mut pixels, w, left, top, plot_w, plot_h);
                        if let Some(waveform) = scope.1.as_ref().filter(|wf| wf.samples > 0) {
                            let raster = waveform.raster();
                            for y in 0..plot_h {
                                let sy = (y * 256 / plot_h).min(255);
                                for x in 0..plot_w {
                                    let [rr, gg, bb, a] = raster[sy * waveform.width + (x * waveform.width / plot_w).min(waveform.width - 1)];
                                    if a == 0 {
                                        continue;
                                    }
                                    let p = &mut pixels[(top + y) * w + left + x];
                                    let under = |shift: u32| ((*p >> shift) & 0xff) * (255 - a as u32) / 255;
                                    *p = ((rr as u32 + under(16)).min(255)) << 16 | ((gg as u32 + under(8)).min(255)) << 8 | (bb as u32 + under(0)).min(255);
                                }
                            }
                        }
                        (scope.0.is_some(), Some((left, top, plot_h)))
                    } else {
                        let (inset, top) = (4, 6);
                        let (plot_w, plot_h) = (w.saturating_sub(2 * inset).max(1), h.saturating_sub(top).max(1));
                        grid_lines(&mut pixels, w, inset, top, plot_w, plot_h);
                        if let Some(histogram) = &scope.0 {
                            let peak = histogram.peak() as f64;
                            if peak > 0.0 {
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
                                        if heights.iter().all(|&height| height < level) {
                                            continue;
                                        }
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
                            }
                        }
                        (scope.0.is_some(), None)
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
                let mut c = Canvas::new(dc, r);
                SetDIBitsToDevice(c.hdc(), 0, 0, w as u32, h as u32, 0, 0, 0, h as u32, pixels.as_ptr() as *const _, &info, DIB_RGB_COLORS);
                // Labels on the dark well, which is dark in either theme.
                let ink = theme::rgb(0x9a, 0x9a, 0x9a);
                let font = HFONT(with_app_fonts_small().0);
                if let Some((left, top, plot_h)) = waveform {
                    for (i, level) in [100, 75, 50, 25, 0].into_iter().enumerate() {
                        let y = (top + plot_h * i / 4) as i32;
                        let tr = RECT { left: 2, top: y - 8, right: left as i32 - 3, bottom: y + 8 };
                        c.text(&level.to_string(), tr, font, ink, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
                    }
                }
                if !has_photo {
                    c.text("No developed photograph", r, font, ink, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
                }
                c.round_corners(RectF::of(r), 6.0 * draw::dpi_of(hwnd), theme::palette().panel);
                c.present(dc);
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

/// Faint lines across the plot at a quarter, half and three quarters.
fn grid_lines(pixels: &mut [u32], width: usize, left: usize, top: usize, plot_w: usize, plot_h: usize) {
    for fraction in [1, 2, 3] {
        let y = top + plot_h * fraction / 4;
        for x in left..(left + plot_w).min(width) {
            if let Some(p) = pixels.get_mut(y * width + x) {
                *p = 0x2c2c2c;
            }
        }
    }
}

/// The small UI font, for the scope's labels.
fn with_app_fonts_small() -> HGDIOBJ {
    thread_local! {
        static SMALL: Cell<isize> = const { Cell::new(0) };
    }
    SMALL.with(|f| {
        if f.get() == 0 {
            with_app(|app| f.set(app.fonts.small.0 as isize));
        }
        HGDIOBJ(f.get() as *mut _)
    })
}

/// The scope's height: a grabber under it. Drag to resize (88–420 DIPs,
/// remembered), double-click for the smallest; Up and Down change it in
/// steps when focused.
extern "system" fn grip_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let set_height = |height: f64, save: bool| {
        with_app(|app| {
            let height = height.round().clamp(SCOPE_HEIGHTS.0, SCOPE_HEIGHTS.1);
            if height != app.inspector.scope_height {
                app.inspector.scope_height = height;
                app.inspector.layout_frame();
                app.model.dirty |= dirty::STRUCTURE;
            }
            if save && app.model.prefs.scope_height != height {
                app.model.prefs.scope_height = height;
                app.model.prefs.save();
            }
        });
    };
    let current = || with_app(|app| app.inspector.scope_height).unwrap_or(SCOPE_HEIGHTS.0);
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                // A pill in the middle, darker under the pointer or while dragged.
                let p = theme::palette();
                let s = draw::dpi_of(hwnd);
                let mut c = Canvas::new(dc, r);
                c.clear(p.panel);
                let (cw, ch) = (32.0 * s, 4.0 * s);
                let pill = RectF::new(((r.right as f32 - cw) / 2.0).round(), ((r.bottom as f32 - ch) / 2.0).round(), cw, ch);
                let active = draw::is_hot(hwnd) || GRIP_DRAG.get().is_some();
                c.round_rect(pill, ch / 2.0, if active { p.secondary } else { theme::mix(p.border, p.strong_border, 0.4) });
                if GetFocus() == hwnd && draw::focus_cues(hwnd) {
                    draw::focus_ring(&mut c, RectF::new(pill.x - 8.0 * s, 0.0, cw + 16.0 * s, r.bottom as f32), 3.0 * s, s);
                }
                c.present(dc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                SetCursor(LoadCursorW(None, IDC_SIZENS).unwrap_or_default());
                LRESULT(1)
            }
            WM_LBUTTONDOWN => {
                // Measured on screen: the handle moves with the height.
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                GRIP_DRAG.set(Some((point.y, current())));
                SetCapture(hwnd);
                let _ = SetFocus(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some((start, height)) = GRIP_DRAG.get() {
                    let mut point = POINT::default();
                    let _ = GetCursorPos(&mut point);
                    let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f64;
                    set_height(height + (point.y - start) as f64 * 96.0 / dpi, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if GRIP_DRAG.take().is_some() {
                    if msg == WM_LBUTTONUP {
                        let _ = ReleaseCapture();
                    }
                    set_height(current(), true);
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                set_height(SCOPE_HEIGHTS.0, true);
                LRESULT(0)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTARROWS as isize),
            WM_KEYDOWN if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_UP | VK_DOWN) => {
                let step = if wparam.0 == VK_DOWN.0 as usize { 20.0 } else { -20.0 };
                set_height(current() + step, true);
                LRESULT(0)
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
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
    let exposure = |kind: &Kind| matches!(kind, Kind::ExposureOffset | Kind::Slider(Adjustment::Slider { path: "camera.exposure_compensation_ev", .. }));
    let found = app.inspector.rows.iter().find(|r| exposure(&r.kind) && r.shown);
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

/// Replaces the adjustments while cropping (`CropInspector`). Cancel and
/// Done stay at the bottom, like Reset below the adjustments; the controls
/// above them scroll when the column is short.
struct CropPanel {
    panel: HWND,
    rows: HWND,
    controls: Vec<(u16, HWND)>,
    /// The custom ratio's fields show.
    custom: bool,
    scroll: i32,
    content: i32,
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
    let rows = create(w!("NegsCropRows"), "", WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), panel, 0, HFONT::default());
    let mut controls = vec![];
    let mut add = |id: u16, class: PCWSTR, text: &str, style: WINDOW_STYLE, font: HFONT| {
        let parent = if matches!(id, C_CANCEL | C_DONE) { panel } else { rows };
        let c = create(class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), parent, id, font);
        controls.push((id, c));
        c
    };
    add(C_LABELS, w!("STATIC"), "Aspect Ratio", WINDOW_STYLE(0), fonts.section);
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
        let e = add(id, w!("EDIT"), "", WS_TABSTOP | WS_GROUP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), fonts.ui);
        draw::set_role(e, Role::Field);
        draw::track_hover(e);
        super::accessible_name(e, if id == C_WIDTH { "Custom crop ratio width" } else { "Custom crop ratio height" });
        unsafe {
            let _ = SetWindowSubclass(e, Some(ratio_subclass), 1, 0);
        }
    }
    add(C_LABELS + 2, w!("STATIC"), "×", WINDOW_STYLE(SS_CENTER.0), fonts.ui);
    add(C_LABELS + 3, w!("STATIC"), "Orientation", WINDOW_STYLE(0), fonts.ui);
    add(C_LANDSCAPE, w!("BUTTON"), "Landscape", WS_TABSTOP | WS_GROUP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32), fonts.ui);
    add(C_PORTRAIT, w!("BUTTON"), "Portrait", WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32), fonts.ui);
    add(C_LABELS + 4, w!("STATIC"), "Straighten", WS_GROUP, fonts.section);
    add(C_LABELS + 5, w!("STATIC"), "Angle", WINDOW_STYLE(0), fonts.ui);
    add(C_ANGLE_VALUE, w!("STATIC"), "0.0°", WINDOW_STYLE(SS_RIGHT.0), fonts.ui);
    let reset = add(C_ANGLE_RESET, w!("BUTTON"), "\u{E777}", WINDOW_STYLE(BS_PUSHBUTTON as u32 | BS_FLAT as u32), fonts.glyph_small);
    draw::track_hover(reset);
    super::dialogs::tooltip(reset, "Straighten to 0°");
    let angle = add(C_ANGLE, TRACKBAR_CLASSW, "Angle", WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), HFONT::default());
    unsafe {
        SendMessageW(angle, TBM_SETRANGEMIN, WPARAM(0), LPARAM(-450));
        SendMessageW(angle, TBM_SETRANGEMAX, WPARAM(0), LPARAM(450));
        SendMessageW(angle, TBM_SETLINESIZE, WPARAM(0), LPARAM(5));
        SendMessageW(angle, TBM_SETPAGESIZE, WPARAM(0), LPARAM(50));
        let _ = SetWindowSubclass(angle, Some(trackbar_subclass), 1, C_ANGLE_RESET as usize);
    }
    draw::track_hover(angle);
    draw::set_default_pos(angle, 0);
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
    let custom = app.model.crop_session.as_ref().is_some_and(|s| s.aspect == CropAspect::Custom);
    let ins = &mut app.inspector;
    unsafe {
        let _ = MoveWindow(panel, 0, 0, ins.rect.right - ins.rect.left, ins.rect.bottom - ins.rect.top, false);
    }
    ins.crop = Some(CropPanel { panel, rows, controls, custom, scroll: 0, content: 0 });
    layout_crop(ins);
}

/// Pins Cancel and Done, then places the controls above them.
fn layout_crop(ins: &mut Inspector) {
    let Some(crop) = &ins.crop else { return };
    let (w, h) = (ins.rect.right - ins.rect.left, ins.rect.bottom - ins.rect.top);
    let pad = ins.px(16.0);
    let (button, tall) = (ins.px(88.0), ins.px(32.0));
    let footer = h - pad - tall;
    let rows = crop.rows;
    place(
        &[
            (rows, 0, 0, w, (footer - ins.px(8.0)).max(0)),
            (crop.get(C_CANCEL), w - pad - 2 * button - ins.px(8.0), footer, button, tall),
            (crop.get(C_DONE), w - pad - button, footer, button, tall),
        ],
        0,
    );
    // Once more when the scroll bar comes or goes, which changes the width.
    for _ in 0..2 {
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(rows, &mut client);
        }
        let (positions, content) = crop_rows(ins, client.right);
        let Some(crop) = ins.crop.as_mut() else { return };
        crop.content = content;
        crop.scroll = crop.scroll.clamp(0, (content - client.bottom).max(0));
        place(&positions, crop.scroll);
        let mut after = RECT::default();
        unsafe {
            let info = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: content,
                nPage: client.bottom.max(0) as u32,
                nPos: crop.scroll,
                ..Default::default()
            };
            SetScrollInfo(rows, SB_VERT, &info, true);
            let _ = GetClientRect(rows, &mut after);
        }
        if after.right == client.right {
            break;
        }
    }
    // The panel draws the fields' frames, which reach past the fields.
    unsafe {
        let _ = InvalidateRect(rows, None, false);
    }
}

/// Where the controls above Cancel and Done go in a `width` wide panel,
/// unscrolled, and the height they take.
fn crop_rows(ins: &Inspector, width: i32) -> (Vec<(HWND, i32, i32, i32, i32)>, i32) {
    let Some(crop) = &ins.crop else { return (vec![], 0) };
    let pad = ins.px(16.0);
    let right = width - pad;
    let row = ins.px(24.0);
    let mut y = ins.px(16.0);
    let mut positions = vec![];
    let mut place = |id: u16, x: i32, y: i32, w: i32, h: i32| positions.push((crop.get(id), x, y, w, h));
    place(C_LABELS, pad, y, right - pad, ins.px(24.0));
    y += ins.px(32.0);
    for i in 0..CropAspect::ALL.len() as u16 {
        place(C_ASPECT + i, pad, y, right - pad, row);
        y += row + ins.px(4.0);
    }
    y += ins.px(4.0);
    for id in [C_LABELS + 1, C_WIDTH, C_LABELS + 2, C_HEIGHT] {
        unsafe {
            let _ = ShowWindow(crop.get(id), if crop.custom { SW_SHOWNA } else { SW_HIDE });
        }
    }
    if crop.custom {
        let field = ins.px(56.0);
        place(C_LABELS + 1, pad, y + ins.px(2.0), ins.px(88.0), ins.px(20.0));
        let r = ins.field(pad + ins.px(96.0), y, field);
        place(C_WIDTH, r.left, r.top, r.right - r.left, r.bottom - r.top);
        place(C_LABELS + 2, pad + ins.px(96.0) + field, y + ins.px(2.0), ins.px(20.0), ins.px(20.0));
        let r = ins.field(pad + ins.px(116.0) + field, y, field);
        place(C_HEIGHT, r.left, r.top, r.right - r.left, r.bottom - r.top);
        y += ins.px(32.0);
    }
    place(C_LABELS + 3, pad, y + ins.px(6.0), ins.px(88.0), ins.px(20.0));
    let half = (right - pad - ins.px(96.0)) / 2;
    place(C_LANDSCAPE, pad + ins.px(96.0), y, half, ins.px(32.0));
    place(C_PORTRAIT, pad + ins.px(96.0) + half, y, half, ins.px(32.0));
    y += ins.px(48.0);
    place(C_LABELS + 4, pad, y, right - pad, ins.px(24.0));
    y += ins.px(32.0);
    place(C_LABELS + 5, pad, y + ins.px(2.0), ins.px(88.0), ins.px(20.0));
    place(C_ANGLE_RESET, right - ins.px(24.0) - ins.px(56.0), y, ins.px(24.0), ins.px(22.0));
    place(C_ANGLE_VALUE, right - ins.px(56.0), y + ins.px(2.0), ins.px(56.0), ins.px(20.0));
    y += ins.px(22.0);
    place(C_ANGLE, pad - ins.px(8.0), y, right - pad + ins.px(16.0), ins.px(26.0));
    y += ins.px(40.0);
    place(C_LABELS + 6, pad, y, ins.px(88.0), ins.px(20.0));
    place(C_SIZE, pad + ins.px(88.0), y, right - pad - ins.px(88.0), ins.px(20.0));
    y += ins.px(24.0);
    place(C_SIZE_NOTE, pad, y, right - pad, ins.px(48.0));
    y += ins.px(56.0);
    place(C_RESET, pad, y, right - pad, ins.px(32.0));
    y += ins.px(48.0);
    // The hint wraps to as many lines as the width needs.
    let hint = footer_height(ins, crop.get(C_LABELS + 7), right - pad);
    place(C_LABELS + 7, pad, y, right - pad, hint);
    y += hint + ins.px(8.0);
    (positions, y)
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
    app.inspector.crop_shown = cropping;
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
    let custom = session.aspect == CropAspect::Custom;
    if let Some(crop) = app.inspector.crop.as_mut()
        && crop.custom != custom
    {
        crop.custom = custom;
        layout_crop(&mut app.inspector);
    }
}

/// Scrolls the controls above Cancel and Done to `target`.
fn scroll_crop(crop: &mut CropPanel, target: i32) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(crop.rows, &mut client);
    }
    let target = target.clamp(0, (crop.content - client.bottom).max(0));
    if target == crop.scroll {
        return;
    }
    let delta = crop.scroll - target;
    crop.scroll = target;
    unsafe {
        ScrollWindowEx(crop.rows, 0, delta, None, None, None, None, SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE);
        let info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_POS, nPos: target, ..Default::default() };
        SetScrollInfo(crop.rows, SB_VERT, &info, true);
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
            WM_VSCROLL => {
                with_app(|app| {
                    let line = app.inspector.px(40.0);
                    let Some(crop) = app.inspector.crop.as_mut() else { return };
                    if hwnd != crop.rows {
                        return;
                    }
                    let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_ALL, ..Default::default() };
                    let _ = GetScrollInfo(hwnd, SB_VERT, &mut info);
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
                    scroll_crop(crop, target);
                });
                LRESULT(0)
            }
            // From the controls, the angle slider without focus, or Cancel and Done.
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                with_app(|app| {
                    let step = delta * app.inspector.px(60.0) / 120;
                    if let Some(crop) = app.inspector.crop.as_mut() {
                        scroll_crop(crop, crop.scroll - step);
                    }
                });
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => draw::color_edit(HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _)),
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                let control = HWND(lparam.0 as *mut _);
                if draw::is_field(control) {
                    return draw::color_edit(dc, control);
                }
                SetBkMode(dc, TRANSPARENT);
                let p = theme::palette();
                let id = GetDlgCtrlID(control) as u16;
                let footer = id == C_LABELS + 7 || id == C_SIZE_NOTE;
                let color = if draw::is_dimmed(control) { p.disabled } else if footer { p.secondary } else { p.text };
                SetTextColor(dc, COLORREF(color));
                LRESULT(theme::brush(p.panel).0 as isize)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                draw::paint_panel(hwnd, theme::palette().panel);
                LRESULT(0)
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

    /// A grid of a starred Gold (its copy among the favourites), Color
    /// Negative's Gold and Portra, and Black & White's Tri-X. The windows are
    /// stand-ins: laying out only reads them.
    fn search_grid() -> StockGrid {
        let h = |n: usize| HWND(n as *mut _);
        StockGrid {
            window: HWND::default(),
            tiles: vec![("gold".into(), h(1)), ("gold".into(), h(2)), ("portra".into(), h(3)), ("trix".into(), h(4))],
            headings: vec![(None, 0), (Some(h(10)), 1), (Some(h(11)), 3)],
            key: String::new(),
            scroll: 0,
            content: 0,
            view: 0,
            was_open: true,
            anchor: None,
            query: String::new(),
            matches: None,
            no_matches: Some(h(20)),
            recenter: false,
        }
    }

    /// While searching, each match shows once under its group, without the
    /// favourites or empty groups' headings; with none, the quiet line.
    #[test]
    fn a_search_shows_each_match_once_under_its_group() {
        use crate::session::Preferences;
        let stock = |id: &str, name: &str, bw: bool| state::Stock {
            id: id.into(), name: name.into(), paper: false, positive: false, bw, target: String::new(), times: vec![], grain: Value::Null,
        };
        let mut model = Model::new(Preferences { favorite_films: vec!["gold".into()], ..Preferences::default() });
        model.stocks = vec![stock("gold", "Gold 200", false), stock("portra", "Portra 400", false), stock("trix", "Tri-X 400", true)];
        let mut grid = search_grid();
        let shown = |grid: &StockGrid| grid_positions(grid, 200, 96, true).0.iter().map(|p| p.0.0 as usize).collect::<Vec<_>>();
        // Not searching: the favourites first, then every group.
        for query in ["", "   "] {
            assert_eq!(search_matches(&grid, &model, query), None);
        }
        assert_eq!(shown(&grid), [1, 10, 2, 3, 11, 4]);
        // A starred match shows once, under its group.
        grid.matches = search_matches(&grid, &model, "GOLD");
        assert_eq!(grid.matches, Some(vec![false, true, false, false]));
        assert_eq!(shown(&grid), [10, 2]);
        grid.matches = search_matches(&grid, &model, "black 400");
        assert_eq!(shown(&grid), [11, 4]);
        grid.matches = search_matches(&grid, &model, "400");
        assert_eq!(shown(&grid), [10, 3, 11, 4]);
        grid.matches = search_matches(&grid, &model, "gold 400");
        assert_eq!(shown(&grid), [20]);
        // The unfiltered layout, which keeps the grid's height while searching.
        assert_eq!(grid_positions(&grid, 200, 96, false).0.len(), 6);
        assert!(grid_positions(&grid, 200, 96, false).1 > grid_positions(&grid, 200, 96, true).1);
        // Searching never edits.
        assert!(model.undo_title().is_none());
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
        // Prep's sliders are rows too.
        for control in state::PREP_TONE.iter().chain(state::PREP_COLOR).chain(state::PREP_DETAIL) {
            assert!(paths.contains(&control.path()), "{}", control.path());
        }
    }

    /// The rows a model shows, by kind, with every group open.
    fn shown(model: &crate::model::Model) -> Vec<Kind> {
        let rows = rows();
        let current = tab(model);
        let mut want = vec![false; rows.len()];
        for (i, row) in rows.iter().enumerate() {
            let on_tab = row.tab == current || row.tab == Tab::Pinned;
            let visible = on_tab && visible(row, model, &|g| want[g]);
            want[i] = visible;
        }
        rows.iter().zip(want).filter(|(_, w)| *w).map(|(r, _)| r.kind).collect()
    }

    fn model() -> crate::model::Model {
        let mut model = crate::model::Model::new(crate::session::Preferences::default());
        model.defaults = serde_json::json!({"camera": {"lens_blur_um": 0.0, "auto_exposure": true}, "scanner": {"lens_blur": 0.0},
            "enlarger": {"print_exposure": 1.0, "preflash_exposure": 0.0}, "io": {"output_cctf_encoding": true}});
        model.state.params = model.defaults.clone();
        model
    }

    /// `Inspector.swift`'s Lab: each group's basics, anything changed from
    /// its default, and everything with Advanced controls.
    #[test]
    fn the_lab_shows_basics_and_changes() {
        let mut model = model();
        model.prefs.choose_tab("Lab");
        let slider = |path: &'static str| move |k: &Kind| matches!(k, Kind::Slider(a) if a.path() == path);
        let title = |name: &'static str| move |k: &Kind| matches!(k, Kind::Group { title, .. } if *title == name);
        let rows = shown(&model);
        assert!(rows.iter().any(slider("enlarger.print_exposure")));
        assert!(rows.iter().any(|k| matches!(k, Kind::BalancePrint(..))), "Balance Print is the Enlarger's fourth basic");
        assert!(!rows.iter().any(slider("enlarger.preflash_exposure")));
        assert!(!rows.iter().any(slider("camera.lens_blur_um")));
        assert!(!rows.iter().any(title("Scanner")) && !rows.iter().any(|k| *k == Kind::Section("Scan & Color")));
        assert!(!rows.iter().any(|k| matches!(k, Kind::Toggle("Scan film directly", _))));
        // A change shows its control, and its group and section.
        crate::json::set(&mut model.state.params, "enlarger.preflash_exposure", serde_json::json!(0.1));
        crate::json::set(&mut model.state.params, "scanner.lens_blur", serde_json::json!(1.0));
        let rows = shown(&model);
        assert!(rows.iter().any(slider("enlarger.preflash_exposure")) && rows.iter().any(title("Scanner")));
        assert!(rows.iter().any(|k| *k == Kind::Section("Scan & Color")));
        model.prefs.lab_all_controls = true;
        let rows = shown(&model);
        assert!(rows.iter().any(slider("camera.lens_blur_um")) && rows.iter().any(title("Color Management")));
        assert!(rows.iter().any(|k| matches!(k, Kind::Toggle("Scan film directly", _))) && rows.iter().any(title("Output")));
        assert!(rows.iter().all(|k| !matches!(k, Kind::ExposureOffset)), "Prep's rows stay on Prep");
    }

    /// Prep shows the meter while matching is off or unavailable, or when
    /// it's changed; the pinned stocks show on every tab.
    #[test]
    fn prep_shows_the_meter_when_the_match_doesnt_set_it() {
        let mut model = model();
        let meter = |rows: &[Kind]| rows.iter().any(|k| matches!(k, Kind::Toggle("Auto exposure", _)));
        assert_eq!(tab(&model), Tab::Prep, "The inspector opens on Prep");
        assert!(meter(&shown(&model)));
        model.state.brightness_match = Some(state::BrightnessMatch::default());
        model.is_raw = true;
        let rows = shown(&model);
        assert!(!meter(&rows) && rows.iter().any(|k| *k == Kind::Note(Note::Brightness)));
        crate::json::set(&mut model.state.params, "camera.auto_exposure", serde_json::json!(false));
        assert!(meter(&shown(&model)));
        for name in ["Prep", "Lab", "Output"] {
            model.prefs.choose_tab(name);
            assert!(shown(&model).iter().any(|k| matches!(k, Kind::Foldout(false))), "{name}");
        }
    }
}

/// The small Segoe MDL2 Assets font the headers' chevrons use.
pub fn glyph_font() -> HGDIOBJ {
    with_app_fonts_glyph()
}
