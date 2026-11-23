//! The adjustments column (`Inspector.swift`), with the common controls: the
//! scope (histogram or waveform, resizable), the film and paper pinned above
//! a Prep | Lab | Output switch, scrolling rows of trackbars with buddy edit
//! boxes, checkboxes and drop-down lists, and Reset Adjustments. The rows of
//! collapsed groups are created when the group first opens, and the whole
//! column only after the window's first frame (§17). While cropping, the crop
//! panel takes its place.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::d2d::{Ink, RectF as DRect, TextStyle};
use super::draw::{self, Canvas, RectF, Role};
use super::ink;
use super::motion;
use super::theme;
use super::{App, TBM_GETPOS, create, post, with_app};
use crate::border::{self, BorderColor};
use crate::crop::CropAspect;
use crate::edits::{MoreAction, Tab};
use crate::inspector_view::schema::{FLAT_FIELD_BUTTONS, TONE_CHANNEL_BUTTONS};
use crate::inspector_view::{self as view, Effect, Expanded, InspectorUi, InspectorView, Kind, More, NegativeAction, RowAction, Track};
use crate::model::{Model, dirty};
use crate::platform::{pcwstr, wide};
use crate::session::SCOPE_HEIGHTS;
use crate::state::{self, Adjustment};

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
const TAB_PREP: u16 = 903;
/// Histogram · Waveform.
const SCOPE_HISTOGRAM: u16 = 904;
const SCOPE_WAVEFORM: u16 = 906;
/// Each tab's dot and count, which opens its edits: 907–909.
const TAB_COUNT: u16 = 907;
/// What an export makes, over Export (`ExportFoot`).
const FOOT_SUMMARY: u16 = 910;
const TABS: [(u16, &str); 3] = [(TAB_PREP, "Prep"), (TAB_LAB, "Lab"), (TAB_OUTPUT, "Output")];
const EDIT_TABS: [crate::edits::Tab; 3] = crate::edits::Tab::ALL;
const CROP_FIRST: u16 = 950;

/// A row of `inspector_view::rows()` (the same index) and its controls,
/// created when it first shows.
struct Row {
    kind: Kind,
    controls: Vec<HWND>,
    shown: bool,
}

pub struct Inspector {
    pub panel: HWND,
    scope: HWND,
    /// Histogram · Waveform, as words.
    scope_words: [HWND; 2],
    scope_label: HWND,
    grip: HWND,
    /// The scope's height in DIPs, as dragged.
    scope_height: f64,
    tabs: [HWND; 3],
    /// Each tab's dot and count; hidden while it has no edits.
    counts: [HWND; 3],
    /// The edits each tab held at the last sync.
    tab_counts: [usize; 3],
    rows_panel: HWND,
    /// The Output tab's foot: what an export makes, and Export.
    foot_summary: HWND,
    export: HWND,
    /// Open in Lightroom, Open in Lightroom Classic under Export (ticket
    /// 48), and how many are laid out: those installed.
    lightroom: [HWND; 2],
    lightroom_shown: Vec<crate::lightroom::Kind>,
    /// Whether the foot shows (the Output tab).
    foot_shown: bool,
    crop: Option<CropPanel>,
    pub rect: RECT,
    rows: Vec<Row>,
    scroll: i32,
    content: i32,
    built: bool,
    dpi: u32,
    dev_times: [Vec<f64>; 2],
    decimal: char,
    /// The UI font's line height, for value boxes inside their fields.
    line: i32,
    /// Motion: the next layout slides rows to their places; the slide
    /// running.
    pub(super) slide: bool,
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
    /// The value each slider last gave UI Automation.
    static ACC_VALUES: RefCell<std::collections::HashMap<isize, String>> = RefCell::new(std::collections::HashMap::new());
    /// What the scope draws: the histogram, the waveform, and whether it shows the waveform.
    static SCOPE: RefCell<(Option<crate::histogram::Histogram>, Option<crate::histogram::Waveform>, bool)> = const { RefCell::new((None, None, false)) };
    /// The scope's height while its handle is dragged: where the drag
    /// started on screen, and the height then.
    static GRIP_DRAG: Cell<Option<(i32, f64)>> = const { Cell::new(None) };
    static SWATCHES: Cell<(u32, u32, u32)> = const { Cell::new((0xffffff, 0, 0)) };
    static ITEM_HEIGHT: Cell<i32> = const { Cell::new(20) };
}

pub fn register() {
    unsafe {
        // Their backgrounds are painted from the palette.
        let classes: [(PCWSTR, WNDPROC, WNDCLASS_STYLES); 6] = [
            (w!("NegsInspector"), Some(panel_proc), WNDCLASS_STYLES(0)),
            (w!("NegsRows"), Some(rows_proc), WNDCLASS_STYLES(0)),
            (w!("NegsScope"), Some(scope_proc), WNDCLASS_STYLES(0)),
            (w!("NegsScopeGrip"), Some(grip_proc), CS_DBLCLKS),
            (w!("NegsCropPanel"), Some(crop_proc), WNDCLASS_STYLES(0)),
            // Its own class only so its scroll bar takes the dark theme.
            (w!("NegsCropRows"), Some(crop_proc), WNDCLASS_STYLES(0)),
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

pub(crate) fn decimal_separator() -> char {
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
            scope_words: [HWND::default(); 2],
            scope_label: HWND::default(),
            grip: HWND::default(),
            scope_height: SCOPE_HEIGHTS.0,
            tabs: [HWND::default(); 3],
            counts: [HWND::default(); 3],
            tab_counts: [usize::MAX; 3],
            rows_panel: HWND::default(),
            foot_summary: HWND::default(),
            export: HWND::default(),
            lightroom: [HWND::default(); 2],
            lightroom_shown: vec![],
            foot_shown: false,
            crop: None,
            rect: RECT::default(),
            rows: vec![],
            scroll: 0,
            content: 0,
            built: false,
            dpi,
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
            post(|app: &mut App| layout_rows(&mut app.inspector));
        }
    }

    /// The scope with its words and grabber, the tabs
    /// between two rules, the rows, and on Output the Export foot
    /// (`AdjustmentInspector`, `ScopePanelView`).
    fn layout_frame(&mut self) {
        if !self.built {
            return;
        }
        let (w, h) = (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top);
        let gutter = self.px(ink::GUTTER);
        let margin = self.px(ink::MARGIN);
        let inner = w - 2 * gutter;
        let scope = self.px(self.scope_height);
        let words = self.px(18.0);
        // Moved together (`DeferWindowPos`): one at a time, each move
        // repainted what it uncovered before the next filled it, which
        // flickered on every step of dragging the scope's grabber.
        let mut positions = vec![];
        let mut x = gutter - margin;
        for (&word, title) in self.scope_words.iter().zip(["Histogram", "Waveform"]) {
            let width = self.px(ink::width(title, &ink::SCOPE_WORD) as f64) + 2 * margin;
            positions.push((word, x, 0, width, words));
            x += width - 2 * margin + self.px(14.0);
        }
        positions.push((self.scope_label, w - gutter - self.px(40.0), 0, self.px(40.0), words));
        let mut y = words + self.px(4.0);
        positions.push((self.scope, gutter, y, inner, scope));
        y += scope;
        // The grabber in the 6 DIPs under the scope and its 8 DIP line, then 10 to the tabs.
        positions.push((self.grip, gutter, y + self.px(2.0), inner, self.px(12.0)));
        y += self.px(24.0);
        let tabs_top = y;
        self.place_tabs(&mut positions, tabs_top);
        y += self.px(ink::TAB_ROW);
        // Quieter than Export: words under it, only with Lightroom installed.
        let words = if self.lightroom_shown.is_empty() { 0 } else { self.px(28.0) };
        let foot = if self.foot_shown { self.px(91.0) + words } else { 0 };
        positions.push((self.rows_panel, 0, y, w, (h - foot - y).max(0)));
        let mut rules = vec![tabs_top, tabs_top + self.px(ink::TAB_ROW) - self.px(1.0).max(1)];
        if self.foot_shown {
            let top = h - foot;
            rules.push(top);
            positions.push((self.foot_summary, gutter, top + self.px(11.0), inner, self.px(20.0)));
            positions.push((self.export, gutter - margin, h - self.px(16.0 + 36.0) - margin - words, inner + 2 * margin, self.px(36.0) + 2 * margin));
            // Centred, 14 DIPs apart; each control is its word and the margins it draws in.
            let m = ink::MARGIN;
            let widths: Vec<i32> = self
                .lightroom_shown
                .iter()
                .map(|k| self.px(super::d2d::measure_text(&format!("Open in {}", k.title()), &ink::WORD, 0.0, true).width as f64 + 2.0 * m))
                .collect();
            let total: i32 = widths.iter().sum::<i32>() + self.px(14.0 - 2.0 * m) * (widths.len() as i32 - 1).max(0);
            let mut x = gutter + (inner - total) / 2;
            for (kind, width) in self.lightroom_shown.iter().zip(widths) {
                positions.push((self.lightroom[*kind as usize], x, h - self.px(16.0 + 22.0), width, self.px(22.0)));
                x += width + self.px(14.0 - 2.0 * m);
            }
        }
        ink::set_rules(self.panel, rules);
        if let Some(crop) = &self.crop {
            positions.push((crop.panel, 0, 0, w, h));
        }
        place(&positions, 0);
        layout_crop(self);
    }

    /// Prep · Lab · Output from the gutter, one gap apart, each followed by
    /// its count when it has edits (`TabRow`).
    fn place_tabs(&self, positions: &mut Vec<(HWND, i32, i32, i32, i32)>, top: i32) {
        let margin = self.px(ink::MARGIN);
        let plain = theme::tokens().plain;
        let mut x = self.px(ink::GUTTER);
        let (y, height) = (top + 1, self.px(ink::TAB_ROW) - 2);
        for (i, (_, title)) in TABS.iter().enumerate() {
            let word = self.px(ink::tab_width(title, plain) as f64);
            let count = self.tab_counts[i];
            // The count follows the word's last ink; the word's own box stops there.
            let counted = count > 0 && count != usize::MAX;
            positions.push((self.tabs[i], x - margin, y, word + if counted { margin } else { 2 * margin }, height));
            x += word;
            if counted {
                let width = self.px(13.0 + ink::width(&count.to_string(), &ink::COUNT) as f64) + margin;
                positions.push((self.counts[i], x, y, width, height));
                x += width - margin;
            }
            x += self.px(ink::TAB_GAP);
        }
    }
}

/// The rows of the pinned area and the three tabs, in order, without
/// their controls yet.
fn rows() -> Vec<Row> {
    view::rows().iter().map(|r| Row { kind: r.kind, controls: vec![], shown: false }).collect()
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
    // Histogram · Waveform as words, like the pane header's.
    let word_style = visible | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    for (i, (id, title)) in [(SCOPE_HISTOGRAM, "Histogram"), (SCOPE_WAVEFORM, "Waveform")].into_iter().enumerate() {
        let word = create(w!("BUTTON"), title, word_style | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), ins.panel, id, fonts.small);
        draw::set_role(word, Role::ScopeWord);
        draw::track_hover(word);
        ins.scope_words[i] = word;
    }
    ins.scope_label = create(w!("STATIC"), "RGB", visible | WINDOW_STYLE(SS_RIGHT.0 | SS_NOPREFIX.0 | SS_CENTERIMAGE.0), WINDOW_EX_STYLE(0), ins.panel, 0, fonts.small);
    ins.scope = create(w!("NegsScope"), "Histogram", visible, WINDOW_EX_STYLE(0), ins.panel, 0, HFONT::default());
    ins.grip = create(w!("NegsScopeGrip"), "Scope height", visible | WS_TABSTOP | WS_GROUP, WINDOW_EX_STYLE(0), ins.panel, 0, HFONT::default());
    draw::track_hover(ins.grip);
    super::dialogs::tooltip(ins.grip, "Drag to resize the scope; double-click for its smallest height");
    // Prep · Lab · Output in type (§4.8).
    let tab_style = visible | WS_TABSTOP | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
    for (i, (id, title)) in TABS.into_iter().enumerate() {
        ins.tabs[i] = create(w!("BUTTON"), title, tab_style | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), ins.panel, id, fonts.ui);
        draw::set_role(ins.tabs[i], Role::TabWord);
        draw::track_hover(ins.tabs[i]);
    }
    // Their counts after the switch, so the arrows keep to the tabs.
    for i in 0..3 {
        let count = create(w!("BUTTON"), "", WS_CHILD | WS_TABSTOP | if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) }, WINDOW_EX_STYLE(0), ins.panel, TAB_COUNT + i as u16, fonts.small);
        draw::set_role(count, Role::Count);
        draw::track_hover(count);
        ins.counts[i] = count;
    }
    ins.rows_panel = create(w!("NegsRows"), "", visible | WS_VSCROLL | WS_CLIPCHILDREN | WS_GROUP, WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0), ins.panel, 0, HFONT::default());
    ins.foot_summary = create(w!("STATIC"), "", WS_CHILD | WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), ins.panel, FOOT_SUMMARY, fonts.ui);
    draw::set_role(ins.foot_summary, Role::Summary);
    super::accessible_role(ins.foot_summary, 0x29);
    ins.export = create(w!("BUTTON"), "Export Photograph…", WS_CHILD | WS_TABSTOP | WS_GROUP, WINDOW_EX_STYLE(0), ins.panel, super::id::EXPORT, fonts.ui);
    draw::set_role(ins.export, Role::Export);
    draw::track_hover(ins.export);
    super::dialogs::tooltip(ins.export, "Export photograph (Ctrl+Shift+E)");
    for (i, kind) in crate::lightroom::Kind::ALL.into_iter().enumerate() {
        let id = super::id::LIGHTROOM + i as u16;
        let c = create(w!("BUTTON"), &format!("Open in {}", kind.title()), WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), ins.panel, id, fonts.ui);
        draw::set_role(c, Role::WordButton);
        draw::track_hover(c);
        super::dialogs::tooltip(c, &format!("Open the photograph in {} as full-size 16-bit TIFFs", kind.title()));
        ins.lightroom[i] = c;
    }
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
                (Kind::BorderStyles | Kind::ToneGraph, _) => fonts.small,
                _ => fonts.ui,
            };
            unsafe {
                SendMessageW(c, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
            }
        }
    }
    for &c in ins.tabs.iter().chain([&ins.export]) {
        unsafe {
            SendMessageW(c, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }
    for &c in ins.scope_words.iter().chain([&ins.scope_label]) {
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
}

/// The tab shown and the open modules, as the preferences keep them.
fn ui_state(model: &Model, decimal: char) -> InspectorUi<'_> {
    InspectorUi { tab: Tab::from_title(model.prefs.tab()), expanded: Expanded::Titles(&model.prefs.expanded_adjustments), decimal }
}

/// What the inspector shows now.
fn build_view(model: &Model, decimal: char) -> InspectorView {
    view::build(&model.inspector_facts(), &ui_state(model, decimal))
}

/// What `action` on row `index` does to the model.
fn dispatch(app: &App, index: usize, action: RowAction) -> Vec<Effect> {
    view::dispatch(&app.model.inspector_facts(), &ui_state(&app.model, app.inspector.decimal), index, action)
}

fn window_text(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
    String::from_utf16_lossy(&text[..n])
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
    let current = Tab::from_title(app.model.prefs.tab());
    let foot = current == Tab::Output;
    unsafe {
        let frame = [ins.scope, ins.scope_words[0], ins.scope_words[1], ins.scope_label, ins.grip, ins.tabs[0], ins.tabs[1], ins.tabs[2], ins.rows_panel];
        for c in frame {
            let _ = ShowWindow(c, if cropping { SW_HIDE } else { SW_SHOWNA });
        }
        for c in [ins.foot_summary, ins.export] {
            let _ = ShowWindow(c, if cropping || !foot { SW_HIDE } else { SW_SHOWNA });
        }
        let kinds = app.model.lightroom_kinds();
        for (i, kind) in crate::lightroom::Kind::ALL.into_iter().enumerate() {
            let _ = ShowWindow(ins.lightroom[i], if cropping || !foot || !kinds.contains(&kind) { SW_HIDE } else { SW_SHOWNA });
        }
        if ins.lightroom_shown != kinds {
            ins.lightroom_shown = kinds;
            ins.layout_frame();
        }
    }
    if cropping {
        for c in ins.counts {
            unsafe {
                let _ = ShowWindow(c, SW_HIDE);
            }
        }
        app.model.show_tone_graph(false);
        return;
    }
    // What shows: built once a sync, from the model as it is now.
    let view = build_view(&app.model, ins.decimal);
    // The Export foot on Output (§4.11).
    if ins.foot_shown != foot {
        ins.foot_shown = foot;
        ins.layout_frame();
    }
    if foot {
        set_text(ins.foot_summary, &view.export_summary);
        enable(ins.export, view.can_export);
        for (i, kind) in crate::lightroom::Kind::ALL.into_iter().enumerate() {
            enable(ins.lightroom[i], app.model.can_open_in_lightroom(kind));
        }
    }
    // Each tab's count, and the header's "3 edits" and Reset.
    let counts = view.counts;
    if counts != ins.tab_counts {
        ins.tab_counts = counts;
        for (i, &count) in counts.iter().enumerate() {
            let words = view::view::count_words;
            let title = TABS[i].1;
            unsafe {
                let _ = ShowWindow(ins.counts[i], if count > 0 { SW_SHOWNA } else { SW_HIDE });
            }
            set_text(ins.counts[i], &count.to_string());
            super::accessible_name(ins.counts[i], &format!("{count} {} on {title}", words(count)));
            super::dialogs::tooltip(ins.counts[i], &format!("Show the {} on {title}", words(count)));
            super::accessible_name(ins.tabs[i], &if count > 0 { format!("{title}, {count} {}", words(count)) } else { title.to_string() });
        }
        ins.layout_frame();
    }
    super::panes::sync_edits(app.panes, view.total, app.model.has_catalog() && !app.model.cropping(), app.model.can_change_presets());
    let ins = &mut app.inspector;
    unsafe {
        for (i, tab) in [Tab::Prep, Tab::Lab, Tab::Output].into_iter().enumerate() {
            SendMessageW(ins.tabs[i], BM_SETCHECK, WPARAM((current == tab) as usize), LPARAM(0));
        }
        let waveform = app.model.prefs.scope_mode == "Waveform";
        for (i, &word) in ins.scope_words.iter().enumerate() {
            if (SendMessageW(word, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1) != (waveform == (i == 1)) {
                SendMessageW(word, BM_SETCHECK, WPARAM((waveform == (i == 1)) as usize), LPARAM(0));
                super::accessible_name(ins.scope, if waveform { "RGB waveform" } else { "Histogram" });
            }
        }
    }
    let want: Vec<bool> = view.rows.iter().map(|r| r.visible).collect();
    // Rows appearing and going (a tab, a group) change while the column
    // doesn't draw, then it paints once: shown one by one, rows drew at
    // their old places, over a part-cleared panel, before the layout moved
    // them. The tabs used to fade the rows in instead, a layered window
    // that blanked them first and flashed again when it ended.
    let lock = !held && want.iter().zip(&ins.rows).any(|(w, r)| *w != r.shown);
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
    // A note's height follows its text, so set it before laying out.
    let mut note_changed = false;
    for (row, v) in ins.rows.iter().zip(&view.rows).filter(|(r, _)| r.shown) {
        let texts = match row.kind {
            Kind::Note(_) => vec![(row.controls[0], &v.hint)],
            Kind::ColorFilter => vec![(row.controls[1], &v.hint), (row.controls[2], &v.warning)],
            _ => continue,
        };
        for (control, text) in texts {
            if window_text(control) != *text {
                set_text(control, text);
                note_changed = true;
            }
        }
    }
    let t1 = crate::platform::qpc();
    if changed || created || structure || note_changed {
        layout_rows(ins);
    }
    let t2 = crate::platform::qpc();
    update_values(ins, &view, &app.model);
    let t_paint = crate::platform::qpc();
    if lock {
        hold_redraw(ins.panel, false);
    }
    let t3 = crate::platform::qpc();
    // Curves are computed only while the graph is on screen.
    let graph = view.tone_graph;
    app.model.show_tone_graph(graph && app.model.inspector_visible && app.model.pane() == crate::model::Pane::Adjustments);
    if created {
        let ms = |a, b| crate::platform::qpc_ms(a, b);
        app.report.line(&format!("inspector sync: create {:.1} ms, layout {:.1} ms, values {:.1} ms (painting {:.1} ms)", ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t_paint, t3)));
    }
}

fn create_row(ins: &mut Inspector, fonts: &super::Fonts, index: usize) {
    let parent = ins.rows_panel;
    let id = |part: u16| FIRST_ID + index as u16 * PARTS + part;
    let kind = ins.rows[index].kind;
    let hidden = WS_CHILD;
    // Owner-drawn statics in ink (`ink::draw_item`), which keep their text
    // for UI Automation.
    let ink_static = |text: &str, role: Role, part: u16, notify: bool| {
        let style = SS_OWNERDRAW.0 | SS_NOPREFIX.0 | if notify { SS_NOTIFY.0 } else { 0 };
        let c = create(w!("STATIC"), text, hidden | WINDOW_STYLE(style), WINDOW_EX_STYLE(0), parent, id(part), fonts.ui);
        draw::set_role(c, role);
        // Text, as the plain statics were, not a graphic.
        super::accessible_role(c, 0x29);
        if notify {
            draw::track_hover(c);
        }
        c
    };
    let label = |text: &str, _: HFONT| ink_static(text, Role::Label, LABEL, true);
    let check = |text: &str| {
        let c = create(w!("BUTTON"), text, hidden | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0), parent, id(CHECK), fonts.ui);
        draw::set_role(c, Role::Tick);
        draw::track_hover(c);
        c
    };
    let combo = |owner_draw: bool| {
        let style = CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | if owner_draw { (CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32 } else { 0 };
        let c = create(w!("COMBOBOX"), "", hidden | WS_TABSTOP | WINDOW_STYLE(style), WINDOW_EX_STYLE(0), parent, id(COMBO), fonts.ui);
        unsafe {
            SendMessageW(c, CB_SETMINVISIBLE, WPARAM(30), LPARAM(0));
            // The closed box as tall as the row's line.
            SendMessageW(c, CB_SETITEMHEIGHT, WPARAM(usize::MAX), LPARAM(ins.px(ink::ROW) as isize - 6));
        }
        draw::set_role(c, Role::Choice);
        draw::paint_combo(c);
        c
    };
    let mut controls = vec![];
    match kind {
        Kind::Section(title) => {
            // Small capitals, read as the title.
            let head = ink_static(&title.to_uppercase(), Role::Section, LABEL, false);
            if !title.is_empty() {
                super::accessible_name(head, title);
            }
            controls.push(head);
        }
        Kind::Footer(text) => controls.push(ink_static(text, Role::Hint, LABEL, false)),
        Kind::More(_) => {
            let more = create(w!("BUTTON"), "", hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(ACTION), fonts.ui);
            draw::set_role(more, Role::More);
            draw::track_hover(more);
            super::dialogs::tooltip(more, view::schema::MORE_HELP);
            controls.push(more);
        }
        Kind::Group { title, enable } => {
            let header = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.bold);
            draw::set_role(header, Role::Module);
            draw::track_hover(header);
            if let Some(help) = kind.help() {
                super::dialogs::tooltip(header, help);
            }
            controls.push(header);
            if enable.is_some() {
                // The module's own tick box, at the end of its line.
                let on = check("");
                super::accessible_name(on, &format!("{title} on"));
                super::dialogs::tooltip(on, &format!("Turns {title} on or off, keeping its settings."));
                controls.push(on);
            }
        }
        Kind::Slider(Adjustment::Slider { .. }) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
            // The Exposure's range moves with the match; the number of steps doesn't.
            let title = kind.label();
            let Some((low, high, step)) = kind.range() else { unreachable!() };
            let text = label(title, fonts.ui);
            controls.push(text);
            let slider = create(TRACKBAR_CLASSW, title, hidden | WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), WINDOW_EX_STYLE(0), parent, id(SLIDER), HFONT::default());
            if let Some(help) = kind.help() {
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
            draw::set_role(slider, Role::InkSlider);
            draw::track_hover(slider);
            controls.push(slider);
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            ink::value_box(edit);
            draw::track_hover(edit);
            super::accessible_name(edit, title);
            unsafe {
                let _ = SetWindowSubclass(edit, Some(edit_subclass), 1, 0);
            }
            controls.push(edit);
            let reset = create(w!("BUTTON"), "↺", hidden | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(RESET), fonts.ui);
            draw::set_role(reset, Role::ResetMark);
            draw::track_hover(reset);
            super::accessible_name(reset, &format!("Reset {title} to Default"));
            controls.push(reset);
        }
        Kind::Slider(_) => {}
        Kind::Toggle(title, _) | Kind::BorderToggle(title, _) => {
            let c = check(title);
            if let Some(help) = kind.help() {
                super::dialogs::tooltip(c, help);
            }
            controls.push(c);
        }
        Kind::MatchBrightness => controls.push(check(kind.label())),
        Kind::FilmFormat | Kind::PreviewSpace | Kind::SizeMode | Kind::FilterLink(_) | Kind::Choice(..) => {
            // The label, then the choices in a drop-down list with the row's tooltip.
            controls.push(label(kind.label(), fonts.ui));
            let c = combo(false);
            for title in kind.options() {
                let text = wide(&title);
                unsafe {
                    SendMessageW(c, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                }
            }
            if let Some(help) = kind.help() {
                super::dialogs::tooltip(c, help);
            }
            controls.push(c);
        }
        Kind::ColorFilter => {
            controls.push(super::color_filter::create(parent, id(ACTION)));
            controls.push(ink_static("", Role::Hint, LABEL, false));
            controls.push(ink_static("", Role::Hint, EDIT, false));
        }
        Kind::ToneGraph => {
            let text = create(w!("STATIC"), "Tone graph", hidden | WINDOW_STYLE(SS_LEFT.0 | SS_CENTERIMAGE.0 | SS_ENDELLIPSIS.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.small);
            controls.push(text);
            controls.push(super::tone_graph::create(parent, id(ACTION), text));
            for (i, (title, help)) in TONE_CHANNEL_BUTTONS.into_iter().enumerate() {
                let group = if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
                let style = hidden | WS_TABSTOP | group | WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32);
                let b = create(w!("BUTTON"), title, style, WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.small);
                // One switch of four segments; the style tells the ends.
                if (1..3).contains(&i) {
                    draw::set_role(b, Role::SegmentMiddle);
                }
                super::dialogs::tooltip(b, help);
                controls.push(b);
            }
        }
        Kind::Actions(buttons) => {
            for (i, (title, help)) in buttons.iter().enumerate() {
                let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.ui);
                draw::set_role(c, Role::Outlined);
                draw::track_hover(c);
                super::dialogs::tooltip(c, help);
                controls.push(c);
            }
        }
        Kind::FlatField => {
            let help = kind.help().unwrap_or_default();
            let text = label(kind.label(), fonts.ui);
            super::dialogs::tooltip(text, help);
            controls.push(text);
            let value = ink_static("Off", Role::Readout, EDIT, true);
            super::dialogs::tooltip(value, help);
            controls.push(value);
            for (part, (title, help)) in [ACTION, RESET].into_iter().zip(FLAT_FIELD_BUTTONS) {
                let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(part), fonts.ui);
                draw::set_role(c, Role::Outlined);
                draw::track_hover(c);
                super::dialogs::tooltip(c, help);
                controls.push(c);
            }
        }
        Kind::BalancePrint(title, help) => {
            let c = create(w!("BUTTON"), title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0), parent, id(ACTION), fonts.ui);
            draw::set_role(c, Role::Outlined);
            draw::track_hover(c);
            super::dialogs::tooltip(c, help);
            controls.push(c);
        }
        Kind::LongestSide => {
            controls.push(label(kind.label(), fonts.ui));
            let edit = create(w!("EDIT"), "", hidden | WS_TABSTOP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL | ES_NUMBER) as u32), WINDOW_EX_STYLE(0), parent, id(EDIT), fonts.ui);
            draw::set_role(edit, Role::Field);
            draw::track_hover(edit);
            super::accessible_name(edit, "Longest side in pixels");
            super::dialogs::tooltip(edit, kind.help().unwrap_or_default());
            unsafe {
                let _ = SetWindowSubclass(edit, Some(size_edit_subclass), 1, 0);
            }
            controls.push(edit);
            controls.push(ink_static("px", Role::Unit, RESET, false));
        }
        Kind::Photograph => {
            controls.push(label(kind.label(), fonts.ui));
            let value = ink_static("", Role::Readout, EDIT, true);
            super::dialogs::tooltip(value, kind.help().unwrap_or_default());
            controls.push(value);
        }
        Kind::Note(_) => controls.push(ink_static("", Role::Hint, LABEL, false)),
        Kind::DevTime(_) => {
            controls.push(label(kind.label(), fonts.ui));
            controls.push(combo(false));
        }
        Kind::BorderStyles => {
            for (i, (style, title)) in border::STYLES.iter().zip(kind.options()).enumerate() {
                let b = create(w!("BUTTON"), &title, hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(i as u16), fonts.small);
                super::dialogs::tooltip(b, border::help(*style));
                draw::track_hover(b);
                controls.push(b);
            }
        }
        Kind::Color(_) => {
            controls.push(label(kind.label(), fonts.ui));
            let b = create(w!("BUTTON"), kind.label(), hidden | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), WINDOW_EX_STYLE(0), parent, id(HEADER), fonts.ui);
            draw::set_role(b, Role::Well);
            draw::track_hover(b);
            controls.push(b);
        }
    }
    ins.rows[index].controls = controls;
}

/// Tab order follows the rows, though they were created in another order.
fn fix_z_order(ins: &Inspector) {
    let mut previous = HWND_TOP;
    for row in &ins.rows {
        for &c in &row.controls {
            unsafe {
                let _ = SetWindowPos(c, previous, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            previous = c;
        }
    }
}

/// A hint's height wrapped at `width` pixels, in pixels.
fn footer_height(ins: &Inspector, hwnd: HWND, width: i32) -> i32 {
    let s = ins.dpi as f32 / 96.0;
    let text = window_text(hwnd);
    let height = ink::hint_height(&text, width as f32 / s);
    (height * s).ceil() as i32
}

/// Lays out the rows.
fn layout_rows(ins: &mut Inspector) {
    place_rows(ins);
}

/// Places the rows; returns the height they take. Rows are 29 DIPs from a
/// 22 DIP gutter (`Theme.Size`); sections end 9 DIPs under their last row
/// with a rule, and modules have a rule above them.
fn place_rows(ins: &mut Inspector) -> i32 {
    place_rows_at(ins, true)
}

/// `place_rows`; `fit` lets it show or hide the scroll bar first when the
/// rows need it (or no longer do) and place them once, at the width left.
fn place_rows_at(ins: &mut Inspector, fit: bool) -> i32 {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(ins.rows_panel, &mut client);
    }
    let width = client.right;
    let gutter = ins.px(ink::GUTTER);
    let margin = ins.px(ink::MARGIN);
    let row_h = ins.px(ink::ROW);
    let right = width - gutter;
    // The labels' column, then the slider, the value and its ↺.
    let column = gutter + ins.px(ink::SLIDER_LABEL);
    let value = ins.px(ink::VALUE_WIDTH);
    let reset = ins.px(ink::RESET_WIDTH);
    let rule = ins.px(1.0).max(1);
    let mut y = 0;
    let mut positions: Vec<(HWND, i32, i32, i32, i32)> = vec![];
    let mut rules = vec![];
    // Whether a section is open (its rule still to draw), and whether the
    // last row was a section's head (a module right under it has no rule).
    let mut in_section = false;
    let mut after_head = false;
    for i in 0..ins.rows.len() {
        let row = &ins.rows[i];
        if !row.shown || row.controls.is_empty() {
            continue;
        }
        let c = &row.controls;
        // A section closes the one before it: 9 DIPs, then a rule.
        if let Kind::Section(_) = row.kind {
            if in_section {
                y += ins.px(9.0);
                rules.push(y);
                y += rule;
            }
            in_section = true;
        }
        let was_head = std::mem::replace(&mut after_head, matches!(row.kind, Kind::Section(_)));
        let height = match row.kind {
            // An untitled section only adds a gap.
            Kind::Section("") => {
                positions.push((c[0], 0, y, 0, 0));
                ins.px(8.0)
            }
            Kind::Section(_) => {
                positions.push((c[0], gutter, y + ins.px(8.0), right - gutter, ins.px(ink::SECTION_HEAD - 8.0)));
                ins.px(ink::SECTION_HEAD)
            }
            Kind::Footer(_) | Kind::Note(_) => {
                let text_height = footer_height(ins, c[0], right - gutter);
                positions.push((c[0], gutter, y, right - gutter, text_height));
                text_height + ins.px(7.0)
            }
            Kind::More(more) => {
                // A section's line sits 6 DIPs lower, the Lab's top one 10.
                let top = ins.px(match more {
                    More::Module(_) => 2.0,
                    More::Section(_) | More::Backlight => 8.0,
                    More::All => 10.0,
                });
                positions.push((c[0], gutter - margin, y + top - ins.px(4.0), right - gutter + 2 * margin, ins.px(25.0)));
                top + ins.px(17.0 + 8.0)
            }
            Kind::LongestSide => {
                let (field, unit) = (ins.px(72.0), ins.px(20.0));
                positions.push((c[0], gutter, y, column - gutter, row_h));
                let r = ins.field(right - unit - field, y + (row_h - ins.px(24.0)) / 2, field);
                positions.push((c[1], r.left, r.top, r.right - r.left, r.bottom - r.top));
                positions.push((c[2], right - unit + ins.px(6.0), y, unit - ins.px(6.0), row_h));
                row_h
            }
            Kind::Photograph => {
                positions.push((c[0], gutter, y, column - gutter, row_h));
                positions.push((c[1], column, y, right - column, row_h));
                row_h
            }
            Kind::Group { enable, .. } => {
                // A rule above, unless the section's head is right above.
                let ruled = !was_head && y > 0;
                let top = if ruled {
                    rules.push(y);
                    rule
                } else {
                    0
                };
                let line = ins.px(ink::MODULE_ROW);
                let tick = if enable.is_some() { ins.px(13.0) + 2 * margin } else { 0 };
                positions.push((c[0], gutter - margin, y + top, right - gutter + 2 * margin - tick, line));
                if enable.is_some() {
                    positions.push((c[1], right + margin - tick, y + top, tick, line));
                }
                top + line
            }
            Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
                // One line: the label, the track, the value and its ↺.
                positions.push((c[0], gutter, y, column - gutter, row_h));
                let reset_x = right - ins.px(10.0);
                let value_x = reset_x - value;
                positions.push((c[1], column, y, (value_x - column - ins.px(4.0)).max(0), row_h));
                let line = ins.px(24.0);
                positions.push((c[2], value_x, y + (row_h - line) / 2, value, line));
                positions.push((c[3], reset_x, y, reset, row_h));
                row_h
            }
            Kind::Toggle(..) | Kind::BorderToggle(..) | Kind::MatchBrightness => {
                positions.push((c[0], gutter - margin, y, right - gutter + 2 * margin, row_h));
                row_h
            }
            Kind::ToneGraph => {
                // The label and the RGB | R | G | B switch on one line, the graph below.
                let button = ins.px(32.0);
                let buttons = 4 * button;
                positions.push((c[0], gutter, y + ins.px(6.0), right - gutter - buttons - ins.px(8.0), ins.px(24.0)));
                for (i, &b) in c[2..].iter().enumerate() {
                    positions.push((b, right - buttons + button * i as i32, y + ins.px(6.0), button, ins.px(24.0)));
                }
                positions.push((c[1], gutter, y + ins.px(36.0), right - gutter, ins.px(168.0)));
                ins.px(36.0 + 168.0 + 6.0)
            }
            Kind::ColorFilter => {
                let picker = ins.px(super::color_filter::HEIGHT);
                positions.push((c[0], 0, y, width, picker));
                let mut height = picker;
                for &hint in &c[1..] {
                    let text_height = if window_text(hint).is_empty() { 0 } else { footer_height(ins, hint, right - gutter) };
                    positions.push((hint, gutter, y + height, right - gutter, text_height));
                    if text_height > 0 {
                        height += text_height + ins.px(6.0);
                    }
                }
                height + ins.px(4.0)
            }
            Kind::BalancePrint(title, _) => {
                let button = ins.px(ink::width(title, &ink::OUTLINED) as f64 + 24.0) + 2 * margin;
                positions.push((c[0], right - button + margin, y, button, ins.px(35.0)));
                ins.px(35.0)
            }
            Kind::FlatField => {
                // The label, the light frame's name, then Choose… and (when on) Off at the row's end.
                let on = window_text(c[1]) != "Off";
                let mut x = right + margin;
                for (i, title) in [(3, "Off"), (2, "Choose…")] {
                    let button = ins.px(ink::width(title, &ink::OUTLINED) as f64 + 24.0) + 2 * margin;
                    if i == 3 && !on {
                        positions.push((c[3], x, y, 0, 0));
                        continue;
                    }
                    x -= button;
                    positions.push((c[i], x, y + (row_h - ins.px(35.0)) / 2, button, ins.px(35.0)));
                    x -= ins.px(8.0) - 2 * margin;
                }
                let label = (column - gutter).min(ins.px(ink::width("Flat field", &ink::LABEL) as f64 + 12.0));
                positions.push((c[0], gutter, y, label, row_h));
                // Clear of the buttons' windows, which paint their margins.
                positions.push((c[1], gutter + label, y, (x - margin - gutter - label).max(0), row_h));
                row_h.max(ins.px(39.0))
            }
            Kind::Actions(buttons) => {
                // At the row's end, 8 apart, as `NegativePrep.buttons`.
                let mut x = right + margin;
                for (i, (title, _)) in buttons.iter().enumerate().rev() {
                    let button = ins.px(ink::width(title, &ink::OUTLINED) as f64 + 24.0) + 2 * margin;
                    x -= button;
                    positions.push((c[i], x, y, button, ins.px(35.0)));
                    x -= ins.px(8.0) - 2 * margin;
                }
                ins.px(39.0)
            }
            Kind::Choice(..) | Kind::FilterLink(_) | Kind::DevTime(_) | Kind::SizeMode | Kind::PreviewSpace | Kind::FilmFormat => {
                // The label, and the value at the row's end; the box takes
                // the rest of the line, so a click anywhere after the label opens it.
                let label = (column - gutter).min(ins.px(ink::width(&window_text(c[0]), &ink::LABEL) as f64 + 12.0)).max(ins.px(60.0));
                positions.push((c[0], gutter, y, label, row_h));
                positions.push((c[1], gutter + label, y + ins.px(3.0), right - gutter - label + margin, ins.px(300.0)));
                row_h
            }
            Kind::BorderStyles => {
                let gap = ins.px(8.0);
                let each = (right - gutter + gap) / 4;
                for (i, &b) in c.iter().enumerate() {
                    positions.push((b, gutter + each * i as i32, y + ins.px(6.0), each - gap, ins.px(72.0)));
                }
                ins.px(84.0)
            }
            Kind::Color(_) => {
                positions.push((c[0], gutter, y, column - gutter, row_h));
                positions.push((c[1], column, y, right - column + margin, row_h));
                row_h
            }
        };
        y += height;
    }
    // The last section's rule.
    if in_section {
        y += ins.px(9.0);
        rules.push(y);
        y += rule;
    }
    if fit {
        let content = y + ins.px(8.0);
        let mut window = RECT::default();
        unsafe {
            let _ = GetWindowRect(ins.rows_panel, &mut window);
        }
        let has_bar = client.right < window.right - window.left;
        if (content >= client.bottom) != has_bar {
            let info = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE,
                nMin: 0,
                nMax: content,
                nPage: client.bottom.max(0) as u32,
                ..Default::default()
            };
            unsafe {
                SetScrollInfo(ins.rows_panel, SB_VERT, &info, false);
            }
            return place_rows_at(ins, false);
        }
    }
    ink::set_rules(ins.rows_panel, rules);
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
        // Wrapped hints repaint whole when they move: a static resized
        // repaints only what it gained, leaving two wrappings side by side.
        let mut hints = vec![];
        for (hwnd, x, y, w, h) in positions {
            if draw::role_of(hwnd) == Some(Role::Hint) {
                hints.push(hwnd);
            }
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
                    // Only what shows: a hidden ↺ stays hidden after the slide.
                    None if IsWindowVisible(hwnd).as_bool() => reveal.push(hwnd),
                    None => {}
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
        for hwnd in hints {
            let _ = InvalidateRect(hwnd, None, false);
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

/// Brings every shown row's controls up to `view`. The colour filter's
/// discs and the tone graph draw from the model themselves.
fn update_values(ins: &mut Inspector, view: &InspectorView, model: &Model) {
    let swatches = view.swatches;
    SWATCHES.with(|s| s.set((swatches.color.colorref(), swatches.line_color.colorref(), swatches.style as u32)));
    let decimal = ins.decimal;
    for (i, row) in ins.rows.iter().enumerate() {
        if row.controls.is_empty() || !row.shown {
            continue;
        }
        let c = &row.controls;
        let v = view.row(i);
        match row.kind {
            Kind::Section(_) => {
                // The Lab's sections: the dot when edited, Print struck while scanning.
                ink::set_look(c[0], ink::Look { edited: v.changed, skipped: v.skipped, ..Default::default() });
            }
            Kind::Footer(text) => {
                if text.is_empty() {
                    if !v.hint.is_empty() {
                        set_text(c[0], &v.hint);
                    }
                    ink::set_look(c[0], ink::Look { warn: v.warn, ..Default::default() });
                }
            }
            Kind::More(_) => {
                if !v.title.is_empty() {
                    set_text(c[0], &v.title);
                }
                enable(c[0], v.enabled);
            }
            Kind::Group { title, enable: switch } => {
                unsafe {
                    // Read out with its state, as the foldouts are.
                    let open = v.open as isize + 2 * v.skipped as isize + 4;
                    if GetWindowLongPtrW(c[0], GWLP_USERDATA) != open {
                        SetWindowLongPtrW(c[0], GWLP_USERDATA, open);
                        super::accessible_name(c[0], &format!("{title}, {}", v.status));
                    }
                }
                ink::set_look(c[0], ink::Look { open: v.open, off: v.off, edited: v.changed, skipped: v.skipped, summary: v.summary.clone(), ..Default::default() });
                enable(c[0], v.enabled);
                if switch.is_some() {
                    set_check(c[1], v.checked);
                    enable(c[1], v.switch_enabled);
                    ink::set_look(c[1], ink::Look { skipped: v.skipped, ..Default::default() });
                }
            }
            Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
                let Some(slider) = v.slider else { continue };
                if let Kind::Slider(Adjustment::Slider { path, .. }) = row.kind
                    && state::bw_title(path).is_some()
                {
                    set_text(c[0], &v.title);
                }
                let pos = slider.position;
                draw::set_default_pos(c[1], slider.default_position);
                unsafe {
                    if SendMessageW(c[1], TBM_GETPOS, WPARAM(0), LPARAM(0)).0 != pos {
                        SendMessageW(c[1], TBM_SETPOS, WPARAM(1), LPARAM(pos));
                    }
                    let text = &v.value;
                    if GetFocus() != c[2] {
                        set_text(c[2], text);
                    }
                    // The slider reads its value, as the Mac's does.
                    if ACC_VALUES.with(|values| values.borrow_mut().insert(c[1].0 as isize, text.clone()).as_ref() != Some(text)) {
                        super::accessible_value(c[1], text);
                    }
                    let modified = v.changed;
                    let _ = ShowWindow(c[3], if modified && row.shown { SW_SHOWNA } else { SW_HIDE });
                    super::dialogs::tooltip(c[3], v.help.as_deref().unwrap_or_default());
                    ink::set_changed(c[0], modified);
                    ink::set_changed(c[2], modified);
                }
                enable_all(c, v.enabled);
            }
            Kind::Toggle(..) => {
                set_check(c[0], v.checked);
                ink::set_changed(c[0], v.changed);
                enable(c[0], v.enabled);
            }
            Kind::MatchBrightness => {
                set_text(c[0], &v.title);
                super::dialogs::tooltip(c[0], v.help.as_deref().unwrap_or_default());
                set_check(c[0], v.checked);
                enable(c[0], v.enabled);
            }
            Kind::FilmFormat | Kind::PreviewSpace | Kind::SizeMode | Kind::FilterLink(_) | Kind::Choice(..) => {
                if !matches!(row.kind, Kind::FilterLink(_)) {
                    ink::set_changed(c[1], v.changed);
                }
                let selected = v.selected.map_or(-1, |i| i as isize);
                unsafe {
                    if SendMessageW(c[1], CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != selected {
                        SendMessageW(c[1], CB_SETCURSEL, WPARAM(selected as usize), LPARAM(0));
                    }
                }
                enable_all(c, v.enabled);
            }
            Kind::ColorFilter => {
                super::color_filter::update(c[0], model, v.enabled);
                ink::set_look(c[2], ink::Look { warn: v.warn, ..Default::default() });
            }
            Kind::ToneGraph => {
                // Not off with the print curves: a contrast drag turns them on.
                for (j, &b) in c[2..].iter().enumerate() {
                    set_check(b, j.checked_sub(1) == v.selected);
                }
                enable_all(c, v.enabled);
                super::tone_graph::update(c[1], model, v.enabled, ins.dpi, decimal);
            }
            Kind::Actions(_) => enable_all(c, v.enabled),
            Kind::FlatField => {
                set_text(c[1], &v.value);
                ink::set_changed(c[0], v.changed);
                enable_all(c, v.enabled);
                // Off only while a light frame is used.
                if !v.changed {
                    enable(c[3], false);
                }
            }
            Kind::BalancePrint(..) => {
                set_text(c[0], &v.title);
                enable(c[0], v.enabled);
            }
            Kind::LongestSide => {
                // A rejected value stays for correction until it is fixed or Esc restores it.
                let focused = unsafe { GetFocus() } == c[1];
                if !focused && !v.keep_typed {
                    set_text(c[1], &v.value);
                }
                enable_all(c, v.enabled);
            }
            Kind::Photograph => set_text(c[1], &v.value),
            Kind::Note(_) => {}
            Kind::BorderToggle(..) => {
                set_check(c[0], v.checked);
                enable(c[0], v.enabled);
            }
            Kind::DevTime(paper) => {
                if ins.dev_times[paper as usize] != view.dev_times[paper as usize] || unsafe { SendMessageW(c[1], CB_GETCOUNT, WPARAM(0), LPARAM(0)).0 } == 0 {
                    unsafe {
                        SendMessageW(c[1], CB_RESETCONTENT, WPARAM(0), LPARAM(0));
                        for option in &v.options {
                            let text = wide(option);
                            SendMessageW(c[1], CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                        }
                    }
                }
                ink::set_changed(c[1], v.changed);
                unsafe {
                    SendMessageW(c[1], CB_SETCURSEL, WPARAM(v.selected.unwrap_or(0)), LPARAM(0));
                }
                enable_all(c, v.enabled);
            }
            Kind::BorderStyles => {
                for (j, &b) in c.iter().enumerate() {
                    enable(b, v.buttons[j]);
                    unsafe {
                        let _ = InvalidateRect(b, None, false);
                    }
                }
            }
            Kind::Color(_) => {
                enable_all(c, v.enabled);
                let color = v.color.unwrap_or(BorderColor::WHITE);
                let [r, g, bl] = color.0;
                ink::set_changed(c[0], v.changed);
                ink::set_look(c[1], ink::Look { summary: v.summary.clone(), color: (r as u32) << 16 | (g as u32) << 8 | bl as u32, ..Default::default() });
            }
        }
    }
    ins.dev_times = view.dev_times.clone();
}

fn enable_all(controls: &[HWND], on: bool) {
    for &c in controls {
        enable(c, on);
    }
}

/// The film or paper menu's entries: the favourites, starred, then each
/// group's heading (None, can't be chosen) and stocks. A favourite stays in
/// its group too; the closed menu shows that entry, with the plain name.
pub(super) fn stock_entries(model: &Model, paper: bool) -> Vec<(String, Option<(String, bool)>)> {
    let (films, papers) = (model.listed_films(), model.papers());
    let groups = if paper { state::paper_groups(&papers) } else { state::film_groups(&films, &papers) };
    let mut entries: Vec<(String, Option<(String, bool)>)> =
        model.favorite_stocks(paper).iter().map(|s| (format!("★ {}", s.name), Some((s.id.clone(), true)))).collect();
    for group in groups {
        entries.push((group.title.into(), None));
        entries.extend(group.stocks.iter().map(|s| (s.name.clone(), Some((s.id.clone(), false)))));
    }
    entries
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

/// Makes the model changes a row's action asks for, in order.
/// What applying an effect to the model left for the window.
#[derive(Debug, PartialEq)]
enum Applied {
    Done,
    /// The inspector's rows change: slide them into place.
    Slide,
    /// A dialog first.
    Dialog(Effect),
}

/// An effect on the model alone: everything but the dialogs.
fn apply_to_model(model: &mut Model, effect: Effect) -> Applied {
    match effect {
        Effect::Set(path, value) => model.set(path, value),
        Effect::SetFilterShift(path, value) => model.set_filter_shift(path, value),
        Effect::ResetValue(path) => model.reset_value(path),
        Effect::ResetFilterShift(path) => model.reset_filter_shift(path),
        Effect::ResetFilterShiftAfterClick(path) => model.reset_filter_shift_after_click(path),
        Effect::SetExposureOffset(value) => model.set_exposure_offset(value),
        Effect::ResetExposureOffset => model.reset_exposure_offset(),
        Effect::SetBorderPercent(field, value) => model.set_border_percent(field, value),
        Effect::Border(change, undo) => model.set_border(|b| change.apply(b), undo),
        Effect::BeginInteraction => model.begin_interaction(),
        Effect::EndInteraction(undo) => model.end_interaction(undo),
        Effect::SetMatchesBrightness(on) => model.set_matches_brightness(on),
        Effect::SwitchColorFilter(on) => model.switch_color_filter(on),
        Effect::ChooseFilmFormat(format) => model.choose_film_format(format),
        Effect::ChoosePreviewSpace(space) => model.choose_preview_space(space),
        Effect::ChooseImageSize(mode) => model.choose_image_size(mode),
        Effect::SetFilterLink(link) => model.set_filter_link(link),
        Effect::BalancePrint => model.balance_print(),
        Effect::ClearFlatField => model.clear_flat_field(),
        Effect::Negative(action) => {
            use crate::negative::Pick;
            match action {
                NegativeAction::FindAgain => model.redetect_negative_base(),
                NegativeAction::Pick(Pick::Base) => model.begin_negative_pick(Pick::Base),
                NegativeAction::CropToPicture => model.crop_to_picture(),
                NegativeAction::AutoPrint => model.auto_print_negative(),
                NegativeAction::Pick(Pick::Neutral) => model.begin_negative_pick(Pick::Neutral),
                NegativeAction::UseForRoll => model.use_negative_for_roll(),
                NegativeAction::RollFiltration => model.use_roll_filtration(),
            }
        }
        Effect::ToneGraphChannel(channel) => {
            model.prefs.tone_graph_channel = channel.into();
            model.prefs.save();
            model.dirty |= dirty::INSPECTOR;
        }
        Effect::SetExpanded(title, open) => {
            model.prefs.set_expanded(title, open);
            model.prefs.save();
            model.dirty |= dirty::STRUCTURE;
            return Applied::Slide;
        }
        Effect::More(action) => {
            let prefs = &mut model.prefs;
            match action {
                MoreAction::Reveal(key) => prefs.set_revealed(&key, true),
                MoreAction::Conceal(key) => prefs.set_revealed(&key, false),
                MoreAction::ShowFewer => prefs.lab_all_controls = false,
            }
            prefs.save();
            model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR;
            return Applied::Slide;
        }
        Effect::ShowBacklight => {
            model.negative.backlight_shown = true;
            model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR;
            return Applied::Slide;
        }
        Effect::ResetEdits(targets, undo) => model.reset_edits(&targets, undo),
        dialog @ (Effect::ChooseLightFrame | Effect::ChooseBorderColor { .. }) => return Applied::Dialog(dialog),
    }
    Applied::Done
}

fn apply(app: &mut App, effects: Vec<Effect>) {
    for effect in effects {
        match apply_to_model(&mut app.model, effect) {
            Applied::Done => {}
            Applied::Slide => app.inspector.slide = true,
            Applied::Dialog(Effect::ChooseLightFrame) => {
                if let Some(light) = super::dialogs::choose_light_frame(app.main, app.model.source.as_deref()) {
                    app.model.choose_flat_field(&light);
                }
            }
            Applied::Dialog(Effect::ChooseBorderColor { line, current }) => {
                if let Some(color) = choose_color(app.main, current) {
                    app.model.set_border_color(line, color);
                }
            }
            Applied::Dialog(_) => {}
        }
    }
}

/// Back to the default: the ↺, a double-click on the label, Reset to
/// Default.
fn reset_row(app: &mut App, index: usize) {
    let effects = dispatch(app, index, RowAction::Reset { double_click: DOUBLE_CLICK_RESET.get() });
    apply(app, effects);
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
    if id == SCOPE_HISTOGRAM || id == SCOPE_WAVEFORM {
        let mode = if id == SCOPE_WAVEFORM { "Waveform" } else { "Histogram" };
        if app.model.prefs.scope_mode != mode {
            app.model.prefs.scope_mode = mode.into();
            app.model.prefs.save();
            app.model.dirty |= dirty::HISTOGRAM | dirty::INSPECTOR;
        }
        return;
    }
    if (TAB_COUNT..TAB_COUNT + 3).contains(&id) {
        tab_edits(app, EDIT_TABS[(id - TAB_COUNT) as usize], hwnd);
        return;
    }
    let Some((index, part)) = row_of(id) else { return };
    let Some(row) = app.inspector.rows.get(index) else { return };
    let kind = row.kind;
    let checked = || unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 };
    let selection = || unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    let clicked = code == BN_CLICKED;
    // The control's event as the row's action.
    let action = match (kind, part) {
        (Kind::Group { .. } | Kind::Color(_), HEADER) | (Kind::More(_) | Kind::BalancePrint(..) | Kind::FlatField, ACTION) if clicked => RowAction::Press(0),
        (Kind::FlatField, RESET) if clicked => RowAction::Press(1),
        (Kind::Actions(_) | Kind::ToneGraph | Kind::BorderStyles, p) if clicked => RowAction::Press(p as usize),
        (Kind::Group { enable: Some(_), .. } | Kind::Toggle(..) | Kind::MatchBrightness | Kind::BorderToggle(..), CHECK) if clicked => RowAction::Check(checked()),
        (Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset, RESET) if clicked => RowAction::Reset { double_click: DOUBLE_CLICK_RESET.get() },
        (_, LABEL) if code == STN_DBLCLK && !draw::is_dimmed(hwnd) => RowAction::Reset { double_click: DOUBLE_CLICK_RESET.get() },
        (Kind::FilmFormat | Kind::PreviewSpace | Kind::SizeMode | Kind::FilterLink(_) | Kind::Choice(..) | Kind::DevTime(_), COMBO) if code == CBN_SELCHANGE => RowAction::Select(selection()),
        _ => return,
    };
    let effects = dispatch(app, index, action);
    apply(app, effects);
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
    let pos = unsafe { SendMessageW(slider, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 } as i32;
    let track = match code {
        TB_THUMBTRACK => Track::Drag,
        TB_ENDTRACK => Track::End,
        _ => Track::Step,
    };
    let effects = dispatch(app, index, RowAction::Track(pos, track));
    apply(app, effects);
}

fn context_menu(app: &mut App, control: HWND, x: i32, y: i32) {
    let id = unsafe { GetDlgCtrlID(control) } as u16;
    let Some((index, _)) = row_of(id) else { return };
    if !view::has_reset(index) {
        return;
    }
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        let text = wide("&Reset to Default");
        let flags = if view::resettable(&app.model.inspector_facts(), index) { MF_STRING } else { MF_STRING | MF_GRAYED };
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

/// A tab's edits under its count, each to reset on its own, then the whole
/// tab: the Mac's popover (`TabEdits`) as a menu.
fn tab_edits(app: &mut App, tab: crate::edits::Tab, count: HWND) {
    let edits = crate::edits::edits(tab, &app.model.inspector_facts(), app.inspector.decimal);
    if edits.is_empty() {
        return;
    }
    let escape = |text: &str| text.replace('&', "&&");
    let title = tab.title();
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        let head = wide(&view::edits_head(tab, edits.len()));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, pcwstr(&head));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        for (i, edit) in edits.iter().enumerate() {
            let text = wide(&format!("Reset {}\t{}", escape(&edit.title), escape(&edit.value)));
            let _ = AppendMenuW(menu, MF_STRING, 1 + i, pcwstr(&text));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let all = wide(&format!("&Reset {title}"));
        let _ = AppendMenuW(menu, MF_STRING, 1000, pcwstr(&all));
        let mut r = RECT::default();
        let _ = GetWindowRect(count, &mut r);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, r.left, r.bottom, 0, app.inspector.panel, None);
        let _ = DestroyMenu(menu);
        let chosen = chosen.0 as usize;
        let choice = if chosen == 1000 { Some(view::EditChoice::All) } else { chosen.checked_sub(1).map(view::EditChoice::One) };
        if let Some(effect) = choice.and_then(|c| view::edit_list(tab, &edits, c)) {
            apply(app, vec![effect]);
        }
    }
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u16, ((wparam.0 >> 16) & 0xffff) as u32);
                if id == super::id::EXPORT || (super::id::LIGHTROOM..super::id::LIGHTROOM + 2).contains(&id) {
                    // The foot's Export is File › Export, and its Lightroom words File's.
                    if code == BN_CLICKED {
                        let _ = PostMessageW(GetAncestor(hwnd, GA_ROOT), WM_COMMAND, WPARAM(id as usize), LPARAM(0));
                    }
                    return LRESULT(0);
                }
                if code == BN_CLICKED {
                    with_app(|app| command(app, id, code, HWND(lparam.0 as *mut _)));
                }
                LRESULT(0)
            }
            WM_DRAWITEM => {
                ink::draw_item(&*(lparam.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
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
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                // The column, and the rules about the tabs and the foot.
                draw::paint_panel(hwnd, theme::palette().panel);
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
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                if !ink::draw_item(item) {
                    draw_item(item);
                }
                LRESULT(1)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                draw::paint_panel(hwnd, theme::palette().panel);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                let (dc, control) = (HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _));
                if draw::role_of(control) == Some(Role::InkValue) { ink::color_value(dc) } else { draw::color_edit(dc, control) }
            }
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
    view::rows().get(index).is_some_and(|r| r.kind.quiet())
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
        if part == HEADER && n > 0 && !["Color", "Line color"].contains(&title.as_str()) {
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
    /// The reset under way is a double-click's on the track, whose first
    /// click moved the thumb.
    static DOUBLE_CLICK_RESET: Cell<bool> = const { Cell::new(false) };
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
        // Return types a value: it moves to the slider's value box.
        let returned = |m: *const MSG| !m.is_null() && (*m).message == WM_KEYDOWN && (*m).wParam.0 == VK_RETURN.0 as usize;
        if msg == WM_GETDLGCODE && returned(lparam.0 as *const MSG) {
            return LRESULT(DefSubclassProc(hwnd, msg, wparam, lparam).0 | DLGC_WANTMESSAGE as isize);
        }
        if msg == WM_KEYDOWN && wparam.0 == VK_RETURN.0 as usize {
            let parent = GetParent(hwnd).unwrap_or_default();
            if let Ok(edit) = GetDlgItem(parent, (reset as u16 - RESET + EDIT) as i32) {
                let _ = SetFocus(edit);
                SendMessageW(edit, EM_SETSEL, WPARAM(0), LPARAM(-1));
            }
            return LRESULT(0);
        }
        if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) {
            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
            let double = msg == WM_LBUTTONDBLCLK || second_click(hwnd, lparam);
            if (alt || double) && IsWindowEnabled(hwnd).as_bool() {
                RESET_TRACKBAR.set(hwnd.0 as usize);
                let parent = GetParent(hwnd).unwrap_or_default();
                let button = GetDlgItem(parent, reset as i32).unwrap_or_default();
                DOUBLE_CLICK_RESET.set(double && !alt);
                SendMessageW(parent, WM_COMMAND, WPARAM(((BN_CLICKED as usize) << 16) | reset), LPARAM(button.0 as isize));
                DOUBLE_CLICK_RESET.set(false);
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
                Some(value) => {
                    let effects = dispatch(app, index, RowAction::Set(value));
                    apply(app, effects);
                }
                None => {}
            }
        }
        // Show the stepped value, or restore an invalid one.
        app.model.dirty |= dirty::INSPECTOR;
        let _ = app;
    });
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
                // The well is the window's `bg`, its grid in `line` (`scopeGrid`).
                let hex = theme::tokens().hex;
                let well = hex.bg;
                let mut pixels = vec![well; w * h];
                let (has_photo, waveform) = SCOPE.with(|scope| {
                    let scope = scope.borrow();
                    if scope.2 {
                        // The levels' column, then the raster scaled into the plot.
                        let left = 32 * dpi / 96;
                        let (top, bottom) = (6 * dpi / 96, 6 * dpi / 96);
                        let (plot_w, plot_h) = (w.saturating_sub(left + 6 * dpi / 96).max(1), h.saturating_sub(top + bottom).max(1));
                        grid_lines(&mut pixels, w, left, top, plot_w, plot_h, hex.line);
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
                        let (inset, top) = (4 * dpi / 96, 6 * dpi / 96);
                        let (plot_w, plot_h) = (w.saturating_sub(2 * inset).max(1), h.saturating_sub(top).max(1));
                        grid_lines(&mut pixels, w, inset, top, plot_w, plot_h, hex.line);
                        if let Some(histogram) = &scope.0 {
                            let peak = histogram.peak() as f64;
                            if peak > 0.0 {
                                // Each channel filled at 0.4 with a 0.9 outline, the
                                // three added together (`plusLighter` in the Mac's
                                // canvas), then laid over the well: in light themes the
                                // colours stay, rather than adding up to white.
                                let colors = [[1.0, 0.42, 0.42], [0.42, 0.8, 0.47], [0.3, 0.59, 1.0]];
                                let line = (dpi / 96).max(1);
                                for x in 0..plot_w {
                                    let bin = x as f64 * 255.0 / (plot_w - 1).max(1) as f64;
                                    let (i, f) = (bin.floor() as usize, bin.fract());
                                    let at = |bins: &[u32; 256]| {
                                        let a = bins[i.min(255)] as f64;
                                        let b = bins[(i + 1).min(255)] as f64;
                                        ((a * (1.0 - f) + b * f) / peak).min(1.0)
                                    };
                                    let heights = [at(&histogram.red), at(&histogram.green), at(&histogram.blue)];
                                    let tops = heights.map(|v| ((1.0 - v) * plot_h as f64).round() as usize);
                                    let first = tops.iter().copied().min().unwrap_or(plot_h).saturating_sub(line);
                                    for y in first..plot_h {
                                        let p = &mut pixels[(top + y) * w + inset + x];
                                        let (mut c, mut alpha) = ([0.0f64; 3], 0.0f64);
                                        for k in 0..3 {
                                            let weight = if y + line > tops[k] && y <= tops[k] { 0.9 } else if y > tops[k] { 0.4 } else { 0.0 };
                                            alpha += weight;
                                            for j in 0..3 {
                                                c[j] += colors[k][j] * weight;
                                            }
                                        }
                                        let alpha = alpha.min(1.0);
                                        let byte = |v: f64, shift: u32| ((v.min(1.0) * 255.0 + (*p >> shift & 0xff) as f64 * (1.0 - alpha)).round().min(255.0)) as u32;
                                        *p = byte(c[0], 16) << 16 | byte(c[1], 8) << 8 | byte(c[2], 0);
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
                // The levels and the empty well's words, in `dim`.
                let s = draw::dpi_of(hwnd);
                ink::scope_labels(&mut c, s, waveform, !has_photo);
                c.round_corners(RectF::of(r), 3.0 * s, theme::palette().panel);
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
fn grid_lines(pixels: &mut [u32], width: usize, left: usize, top: usize, plot_w: usize, plot_h: usize, line: u32) {
    for fraction in [1, 2, 3] {
        let y = top + plot_h * fraction / 4;
        for x in left..(left + plot_w).min(width) {
            if let Some(p) = pixels.get_mut(y * width + x) {
                *p = line;
            }
        }
    }
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
                // A 32 × 3 capsule in `track`, `dim` under the pointer or while dragged.
                let p = theme::palette();
                let hex = theme::tokens().hex;
                let s = draw::dpi_of(hwnd);
                let mut c = Canvas::new(dc, r);
                c.clear(p.panel);
                let (cw, ch) = (32.0 * s, (3.0 * s).round());
                let pill = RectF::new(((r.right as f32 - cw) / 2.0).round(), ((r.bottom as f32 - ch) / 2.0).round(), cw, ch);
                let active = draw::is_hot(hwnd) || GRIP_DRAG.get().is_some();
                c.round_rect(pill, ch / 2.0, theme::gdi(if active { hex.dim } else { hex.track }));
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

/// Replaces the adjustments while cropping (`CropInspector`), in the ink:
/// "Crop" and Reset Crop over a rule; the aspect ratios as rows with a dot
/// and their formats in italic, Orientation as two words, the angle as an
/// ink slider, the size and the hints; under a rule at the foot "return
/// applies · esc cancels" over Cancel and Done, Done twice as wide. The
/// rows between scroll when the column is short.
struct CropPanel {
    panel: HWND,
    rows: HWND,
    controls: Vec<(u16, HWND)>,
    /// The custom ratio's fields show.
    custom: bool,
    scroll: i32,
    content: i32,
    /// The rules' heights in the panel: under the header, over the foot.
    rules: (i32, i32),
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
/// Flip left to right (`io.flip`), a tick box.
const C_FLIP: u16 = CROP_FIRST + 22;
const C_LABELS: u16 = CROP_FIRST + 30;
/// The labels: Ratio, ×, Orientation, Angle, Size, the size's hint, the
/// instructions, and the two sections' heads.
const L_RATIO: u16 = C_LABELS + 1;
const L_TIMES: u16 = C_LABELS + 2;
const L_ORIENTATION: u16 = C_LABELS + 3;
const L_STRAIGHTEN: u16 = C_LABELS + 4;
const L_ANGLE: u16 = C_LABELS + 5;
const L_SIZE: u16 = C_LABELS + 6;
const L_HINT: u16 = C_LABELS + 7;
const L_SIZE_HINT: u16 = C_LABELS + 8;
const L_FLIP: u16 = C_LABELS + 9;

/// The foot's words, the aspect formats and the orientation words (`Crop.swift`).
const CROP_FOOT: TextStyle = TextStyle::serif(14.0, 400).italic();
const CROP_HEAD: TextStyle = TextStyle::ui(12.0, 500);
const ASPECT_FORMAT: TextStyle = TextStyle::serif(13.5, 400).italic();
const ORIENTATION_WORD: TextStyle = TextStyle::serif(14.5, 400);
const CROP_BUTTON: TextStyle = TextStyle::serif(15.0, 600);

thread_local! {
    /// The crop panel, and its rules' heights: under the header, over the foot.
    static CROP_RULES: Cell<(isize, i32, i32)> = const { Cell::new((0, 0, 0)) };
}

/// `text`'s width in `style`, DIPs.
fn measure_dips(text: &str, style: &TextStyle) -> f64 {
    super::d2d::measure_text(text, style, 0.0, theme::tokens().plain).width as f64
}

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
        let parent = if matches!(id, C_CANCEL | C_DONE | C_RESET) { panel } else { rows };
        let c = create(class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), parent, id, font);
        controls.push((id, c));
        c
    };
    let mut ink_static = |id: u16, text: &str, role: Role| {
        let c = add(id, w!("STATIC"), text, WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0), fonts.ui);
        draw::set_role(c, role);
        super::accessible_role(c, 0x29);
        c
    };
    ink_static(C_LABELS, "ASPECT RATIO", Role::Section);
    ink_static(L_RATIO, "Ratio", Role::Label);
    ink_static(L_TIMES, "×", Role::Unit);
    ink_static(L_ORIENTATION, "Orientation", Role::Label);
    ink_static(L_STRAIGHTEN, "STRAIGHTEN", Role::Section);
    ink_static(L_ANGLE, "Angle", Role::Label);
    ink_static(C_ANGLE_VALUE, "0°", Role::Readout);
    ink_static(L_FLIP, "FLIP", Role::Section);
    ink_static(L_SIZE, "Size", Role::Label);
    let size = ink_static(C_SIZE, "", Role::Readout);
    super::dialogs::tooltip(size, "The exported photograph’s size, before the border.");
    ink_static(L_SIZE_HINT, "The exported photograph’s size, before the border.", Role::Hint);
    ink_static(C_SIZE_NOTE, "", Role::Hint);
    ink_static(
        L_HINT,
        "Drag the corners or edges to crop, or inside to move it. Drag the dial under the photograph to straighten it; the arrows at its ends turn it a quarter. The border is added after cropping.",
        Role::Hint,
    );
    // The aspect ratios and the orientation words, painted here (`WM_DRAWITEM`).
    for (i, aspect) in CropAspect::ALL.iter().enumerate() {
        let text = match aspect.format() {
            Some(format) => format!("{}\t{format}", aspect.title()),
            None => aspect.title().to_string(),
        };
        let group = if i == 0 { WS_GROUP | WS_TABSTOP } else { WS_TABSTOP };
        let c = add(C_ASPECT + i as u16, w!("BUTTON"), &text, group | WINDOW_STYLE(BS_OWNERDRAW as u32), fonts.ui);
        draw::track_hover(c);
        super::accessible_name(c, &text.replace('\t', ", "));
    }
    for id in [C_WIDTH, C_HEIGHT] {
        let e = add(id, w!("EDIT"), "", WS_TABSTOP | WS_GROUP | WINDOW_STYLE((ES_RIGHT | ES_AUTOHSCROLL) as u32), fonts.ui);
        draw::set_role(e, Role::Field);
        draw::track_hover(e);
        super::accessible_name(e, if id == C_WIDTH { "Custom crop ratio width" } else { "Custom crop ratio height" });
        unsafe {
            let _ = SetWindowSubclass(e, Some(ratio_subclass), 1, 0);
        }
    }
    for (id, title) in [(C_LANDSCAPE, "Landscape"), (C_PORTRAIT, "Portrait")] {
        let group = if id == C_LANDSCAPE { WS_GROUP | WS_TABSTOP } else { WS_TABSTOP };
        let c = add(id, w!("BUTTON"), title, group | WINDOW_STYLE(BS_OWNERDRAW as u32), fonts.ui);
        draw::track_hover(c);
    }
    let reset = add(C_ANGLE_RESET, w!("BUTTON"), "↺", WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), fonts.ui);
    draw::set_role(reset, Role::ResetMark);
    draw::track_hover(reset);
    super::accessible_name(reset, "Straighten to 0°");
    super::dialogs::tooltip(reset, "Straighten to 0°");
    let angle = add(C_ANGLE, TRACKBAR_CLASSW, "Angle", WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS | TBS_HORZ | TBS_BOTH), HFONT::default());
    unsafe {
        SendMessageW(angle, TBM_SETRANGEMIN, WPARAM(0), LPARAM(-450));
        SendMessageW(angle, TBM_SETRANGEMAX, WPARAM(0), LPARAM(450));
        SendMessageW(angle, TBM_SETLINESIZE, WPARAM(0), LPARAM(5));
        SendMessageW(angle, TBM_SETPAGESIZE, WPARAM(0), LPARAM(50));
        let _ = SetWindowSubclass(angle, Some(trackbar_subclass), 1, C_ANGLE_RESET as usize);
    }
    draw::set_role(angle, Role::InkSlider);
    draw::track_hover(angle);
    draw::set_default_pos(angle, 0);
    super::dialogs::tooltip(angle, "Turns the photograph to level a crooked horizon. The crop shrinks to stay on the photograph.");
    let flip = add(C_FLIP, w!("BUTTON"), "Flip left to right", WS_TABSTOP | WS_GROUP | WINDOW_STYLE(BS_CHECKBOX as u32), fonts.ui);
    draw::set_role(flip, Role::Tick);
    draw::track_hover(flip);
    super::dialogs::tooltip(flip, "Mirrors the photograph, as a negative photographed from its base side needs. With a half turn it mirrors top to bottom.");
    let reset_crop = add(C_RESET, w!("BUTTON"), "Reset Crop", WS_TABSTOP | WS_GROUP | WINDOW_STYLE(BS_PUSHBUTTON as u32), fonts.ui);
    draw::set_role(reset_crop, Role::WordButton);
    draw::track_hover(reset_crop);
    super::dialogs::tooltip(reset_crop, "Show the whole, unturned photograph again.");
    let cancel = add(C_CANCEL, w!("BUTTON"), "Cancel", WS_TABSTOP | WS_GROUP | WINDOW_STYLE(BS_OWNERDRAW as u32), fonts.ui);
    draw::track_hover(cancel);
    super::dialogs::tooltip(cancel, "Discard the crop (Esc)");
    let done = add(C_DONE, w!("BUTTON"), "Done", WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32), fonts.ui);
    draw::track_hover(done);
    super::dialogs::tooltip(done, "Apply the crop (Return)");
    let custom = app.model.crop_session.as_ref().is_some_and(|s| s.aspect == CropAspect::Custom);
    let ins = &mut app.inspector;
    unsafe {
        let _ = MoveWindow(panel, 0, 0, ins.rect.right - ins.rect.left, ins.rect.bottom - ins.rect.top, false);
    }
    ins.crop = Some(CropPanel { panel, rows, controls, custom, scroll: 0, content: 0, rules: (0, 0) });
    layout_crop(ins);
}

/// Places the header and the foot, then the rows between them.
fn layout_crop(ins: &mut Inspector) {
    let Some(crop) = &ins.crop else { return };
    let (w, h) = (ins.rect.right - ins.rect.left, ins.rect.bottom - ins.rect.top);
    let gutter = ins.px(ink::GUTTER);
    let header = ins.px(ink::PANE_HEADER);
    // The foot: 11 above the words, 8 between them and the buttons, 16 below.
    let tall = ins.px(36.0);
    let foot = ins.px(11.0 + 18.0 + 8.0) + tall + ins.px(16.0);
    let top_of_foot = h - foot;
    let third = ((w - 2 * gutter - ins.px(10.0)) / 3).max(1);
    let reset_width = ins.px(measure_dips("Reset Crop", &ink::WORD) + 2.0 * ink::MARGIN);
    let rows = crop.rows;
    place(
        &[
            (rows, 0, header + 1, w, (top_of_foot - header - 1).max(0)),
            (crop.get(C_RESET), w - gutter - reset_width + ins.px(ink::MARGIN), ins.px(8.0), reset_width, ins.px(24.0)),
            (crop.get(C_CANCEL), gutter, h - ins.px(16.0) - tall, third, tall),
            (crop.get(C_DONE), gutter + third + ins.px(10.0), h - ins.px(16.0) - tall, w - 2 * gutter - third - ins.px(10.0), tall),
        ],
        0,
    );
    if let Some(crop) = ins.crop.as_mut() {
        crop.rules = (header, top_of_foot);
        // For its paint, which comes while the app may be busy.
        CROP_RULES.with(|r| r.set((crop.panel.0 as isize, header, top_of_foot)));
    }
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
    unsafe {
        let _ = InvalidateRect(rows, None, false);
        if let Some(crop) = &ins.crop {
            let _ = InvalidateRect(crop.panel, None, false);
        }
    }
}

/// Where the rows go in a `width` wide panel, unscrolled, and the height
/// they take: the ink's gutters and row heights.
fn crop_rows(ins: &Inspector, width: i32) -> (Vec<(HWND, i32, i32, i32, i32)>, i32) {
    let Some(crop) = &ins.crop else { return (vec![], 0) };
    let gutter = ins.px(ink::GUTTER);
    let right = width - gutter;
    let row = ins.px(ink::ROW);
    let mut y = 0;
    let mut positions = vec![];
    let mut place = |id: u16, x: i32, y: i32, w: i32, h: i32| positions.push((crop.get(id), x, y, w, h));
    place(C_LABELS, gutter, y, right - gutter, ins.px(ink::SECTION_HEAD));
    y += ins.px(ink::SECTION_HEAD);
    for i in 0..CropAspect::ALL.len() as u16 {
        place(C_ASPECT + i, 0, y, width, row - ins.px(3.0));
        y += row - ins.px(3.0);
    }
    for id in [L_RATIO, C_WIDTH, L_TIMES, C_HEIGHT] {
        unsafe {
            let _ = ShowWindow(crop.get(id), if crop.custom { SW_SHOWNA } else { SW_HIDE });
        }
    }
    if crop.custom {
        let field = ins.px(52.0);
        place(L_RATIO, gutter, y, ins.px(ink::SLIDER_LABEL), row);
        let times = ins.px(16.0);
        let r = ins.field(right - field, y + (row - ins.px(24.0)) / 2, field);
        place(C_HEIGHT, r.left, r.top, r.right - r.left, r.bottom - r.top);
        place(L_TIMES, right - field - times, y, times, row);
        let r = ins.field(right - 2 * field - times, y + (row - ins.px(24.0)) / 2, field);
        place(C_WIDTH, r.left, r.top, r.right - r.left, r.bottom - r.top);
        y += row;
    }
    let words = ins.px(measure_dips("Landscape", &ORIENTATION_WORD) + 4.0);
    let portrait = ins.px(measure_dips("Portrait", &ORIENTATION_WORD) + 4.0);
    place(L_ORIENTATION, gutter, y, ins.px(ink::SLIDER_LABEL), row + ins.px(4.0));
    place(C_PORTRAIT, right - portrait, y, portrait, row + ins.px(4.0));
    place(C_LANDSCAPE, right - portrait - ins.px(14.0) - words, y, words, row + ins.px(4.0));
    y += row + ins.px(4.0) + ins.px(10.0);
    place(L_STRAIGHTEN, gutter, y, right - gutter, ins.px(ink::SECTION_HEAD));
    y += ins.px(ink::SECTION_HEAD);
    // As a Lab slider: the label, the slider, the value and ↺.
    let label = ins.px(ink::SLIDER_LABEL);
    let value = ins.px(ink::VALUE_WIDTH);
    let reset = ins.px(ink::RESET_WIDTH);
    place(L_ANGLE, gutter, y, label, row);
    place(C_ANGLE, gutter + label, y, (right - gutter - label - value - reset).max(0), row);
    place(C_ANGLE_VALUE, right - reset - value, y, value, row);
    place(C_ANGLE_RESET, right - reset, y, reset, row);
    y += row + ins.px(10.0);
    place(L_FLIP, gutter, y, right - gutter, ins.px(ink::SECTION_HEAD));
    y += ins.px(ink::SECTION_HEAD);
    let margin = ins.px(ink::MARGIN);
    place(C_FLIP, gutter - margin, y, right - gutter + 2 * margin, row);
    y += row + ins.px(14.0);
    place(L_SIZE, gutter, y, label, row);
    place(C_SIZE, gutter + label, y, right - gutter - label, row);
    y += row;
    for id in [L_SIZE_HINT, C_SIZE_NOTE, L_HINT] {
        let text_height = footer_height(ins, crop.get(id), right - gutter);
        if text_height > 0 && !window_text(crop.get(id)).is_empty() {
            place(id, gutter, y, right - gutter, text_height);
            y += text_height + ins.px(6.0);
        } else {
            place(id, gutter, y, right - gutter, 0);
        }
    }
    y += ins.px(10.0);
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
    // The painted rows read the session; they repaint when it changes.
    for i in 0..CropAspect::ALL.len() as u16 {
        let c = crop.get(C_ASPECT + i);
        let chosen = CropAspect::ALL[i as usize] == session.aspect;
        super::accessible_value(c, if chosen { "Chosen" } else { "" });
        unsafe {
            let _ = InvalidateRect(c, None, false);
        }
    }
    let portrait = session.orientation_portrait();
    for (id, chosen) in [(C_LANDSCAPE, !portrait), (C_PORTRAIT, portrait)] {
        let c = crop.get(id);
        enable(c, session.aspect.turns());
        super::accessible_value(c, if chosen { "Chosen" } else { "" });
        unsafe {
            let _ = InvalidateRect(c, None, false);
        }
    }
    enable(crop.get(L_ORIENTATION), session.aspect.turns());
    unsafe {
        for (id, value) in [(C_WIDTH, session.custom.width), (C_HEIGHT, session.custom.height)] {
            if GetFocus() != crop.get(id) {
                set_text(crop.get(id), &view::view::format(value, 0.01, decimal));
            }
        }
        let pos = (session.angle * 10.0).round() as isize;
        if SendMessageW(crop.get(C_ANGLE), TBM_GETPOS, WPARAM(0), LPARAM(0)).0 != pos {
            SendMessageW(crop.get(C_ANGLE), TBM_SETPOS, WPARAM(1), LPARAM(pos));
        }
        let _ = ShowWindow(crop.get(C_ANGLE_RESET), if session.angle != 0.0 { SW_SHOWNA } else { SW_HIDE });
    }
    ink::set_changed(crop.get(L_ANGLE), session.angle != 0.0);
    let flip = crop.get(C_FLIP);
    ink::set_changed(flip, session.flip);
    unsafe {
        SendMessageW(flip, BM_SETCHECK, WPARAM(if session.flip { 1 } else { 0 }), LPARAM(0));
        let _ = InvalidateRect(flip, None, false);
    }
    set_text(crop.get(C_ANGLE_VALUE), &format!("{}°", view::view::format(session.angle, 0.1, decimal)));
    // The draft's pixels in the frame an export develops; with a target, a
    // smaller crop needs a larger frame, up to the working limit.
    let (size, note, warn) = match app.model.crop_draft_size() {
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
            ((size.photo.0 as f64, size.photo.1 as f64), note, false)
        }
        Some(Err(error)) => {
            let shown = session.shown();
            ((shown.width, shown.height), error.to_string(), true)
        }
        None => {
            let shown = session.shown();
            ((shown.width, shown.height), String::new(), false)
        }
    };
    set_text(crop.get(C_SIZE), &format!("{} × {} px", size.0.round() as i64, size.1.round() as i64));
    let note_changed = window_text(crop.get(C_SIZE_NOTE)) != note;
    set_text(crop.get(C_SIZE_NOTE), &note);
    ink::set_look(crop.get(C_SIZE_NOTE), ink::Look { warn, ..Default::default() });
    enable(crop.get(C_RESET), session.modified());
    let custom = session.aspect == CropAspect::Custom;
    if let Some(crop) = app.inspector.crop.as_mut()
        && (crop.custom != custom || note_changed)
    {
        crop.custom = custom;
        layout_crop(&mut app.inspector);
    }
}

/// Paints a crop row the panel draws itself: an aspect ratio (a dot, the
/// ratio, its format in italic at the right), an orientation word (`paper`
/// with the pencil under it when chosen), or Cancel and Done.
fn draw_crop_item(item: &DRAWITEMSTRUCT, session: Option<&crate::crop::CropSession>) {
    let hwnd = item.hwndItem;
    let id = item.CtlID as u16;
    let s = draw::dpi_of(hwnd);
    let t = theme::tokens();
    let r = item.rcItem;
    let (w, h) = ((r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s);
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0 && draw::focus_cues(hwnd);
    let hot = !disabled && draw::is_hot(hwnd);
    let alpha = if disabled { theme::DISABLED_OPACITY } else { 1.0 };
    let gutter = ink::GUTTER as f32;
    let text = window_text(hwnd);
    let mut c = Canvas::new(item.hDC, r);
    c.clear(theme::gdi(t.hex.panel));
    c.d2d(s, t.plain, |p| {
        match id {
            i if (C_ASPECT..C_ASPECT + CropAspect::ALL.len() as u16).contains(&i) => {
                let chosen = session.is_some_and(|s| s.aspect == CropAspect::ALL[(i - C_ASPECT) as usize]);
                let (title, format) = text.split_once('\t').unwrap_or((&text, ""));
                let ring = if chosen { t.hex.paper } else if hot { t.hex.dim } else { t.hex.track };
                let (cx, cy) = (gutter + 4.5, h / 2.0);
                if chosen {
                    p.fill_ellipse(cx, cy, 4.5, 4.5, Ink::hex(t.hex.paper));
                } else {
                    p.stroke_round_rect(DRect::new(cx - 3.9, cy - 3.9, 7.8, 7.8), 3.9, 1.2, Ink::hex(ring));
                }
                let m = p.measure(title, &ink::LABEL, 0.0);
                let y = ((h + 0.7 * ink::LABEL.size) / 2.0 - m.baseline).round();
                p.text(title, &ink::LABEL, gutter + 19.0, y, Ink::hex(if chosen || hot { t.hex.paper } else { t.hex.label }));
                if !format.is_empty() {
                    let f = p.measure(format, &ASPECT_FORMAT, 0.0);
                    p.text(format, &ASPECT_FORMAT, w - gutter - f.width, y + m.baseline - f.baseline, Ink::hex(t.hex.dim));
                }
            }
            C_LANDSCAPE | C_PORTRAIT => {
                let chosen = session.is_some_and(|s| s.orientation_portrait() == (id == C_PORTRAIT));
                let m = p.measure(&text, &ORIENTATION_WORD, 0.0);
                let y = ((h + 0.7 * ORIENTATION_WORD.size) / 2.0 - m.baseline).round();
                if chosen && !disabled {
                    p.underline(DRect::new(2.0, y + m.baseline + 3.0, m.width, 6.0), &t, false);
                }
                let color = if chosen { t.hex.paper } else if hot { t.hex.label } else { t.hex.dim };
                p.text(&text, &ORIENTATION_WORD, 2.0, y, Ink::hex(color).alpha(alpha));
            }
            C_CANCEL => {
                // Outlined, square-cornered, 36 tall beside Done.
                if pressed || hot {
                    p.fill_rect(DRect::new(0.0, 0.0, w, h), Ink::hex(t.hex.paper).alpha(if pressed { 0.09 } else { 0.04 }));
                }
                p.stroke_rect(DRect::new(0.5, 0.5, w - 1.0, h - 1.0), 1.0, Ink::hex(if hot || pressed { t.hex.dim } else { t.hex.track }));
                let m = p.measure(&text, &CROP_BUTTON, 0.0);
                p.text(&text, &CROP_BUTTON, ((w - m.width) / 2.0).round(), ((h + 0.7 * CROP_BUTTON.size) / 2.0 - m.baseline).round(), Ink::hex(t.hex.paper));
            }
            C_DONE => {
                // The pencil's yellow, filled.
                let sink = if pressed { 1.0 } else { 0.0 };
                let face = DRect::new(0.0, sink, w, h - sink);
                p.fill_round_rect(face, 3.0, Ink::hex(t.hex.fill));
                if pressed {
                    p.fill_round_rect(face, 3.0, Ink::hex(0).alpha(0.12));
                } else if hot {
                    p.fill_round_rect(face, 3.0, Ink::hex(0xffffff).alpha(0.08));
                }
                let m = p.measure(&text, &CROP_BUTTON, 0.0);
                p.text(&text, &CROP_BUTTON, ((w - m.width) / 2.0).round(), face.y + ((h + 0.7 * CROP_BUTTON.size) / 2.0 - m.baseline).round(), Ink::hex(t.hex.on_fill));
            }
            _ => {}
        }
        if focused {
            ink::focus_box(p, DRect::new(0.0, 0.0, w, h), &t);
        }
    });
    c.present(item.hDC);
}

/// The panel's header word, its rules, and the foot's line.
fn paint_crop_panel(hwnd: HWND) {
    let t = theme::tokens();
    let s = draw::dpi_of(hwnd);
    let (_, header, foot) = CROP_RULES.with(Cell::get);
    let rules = (header, foot);
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut c = Canvas::new(dc, client);
        c.clear(theme::gdi(t.hex.panel));
        let (w, _) = (client.right as f32 / s, client.bottom as f32 / s);
        let gutter = ink::GUTTER as f32;
        c.d2d(s, t.plain, |p| {
            let m = p.measure("Crop", &CROP_HEAD, 0.0);
            p.text("Crop", &CROP_HEAD, gutter, 13.0 + 12.0 * 0.8 - m.baseline + 3.0, Ink::hex(t.hex.paper));
            let (header, foot) = (rules.0 as f32 / s, rules.1 as f32 / s);
            p.fill_rect(DRect::new(0.0, header, w, 1.0), Ink::hex(t.hex.line));
            p.fill_rect(DRect::new(0.0, foot, w, 1.0), Ink::hex(t.hex.line));
            let words = "return applies · esc cancels";
            let f = p.measure(words, &CROP_FOOT, 0.0);
            p.text(words, &CROP_FOOT, ((w - f.width) / 2.0).round(), foot + 11.0, Ink::hex(t.hex.label));
        });
        c.present(dc);
        let _ = EndPaint(hwnd, &ps);
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
        // The edit, not the session: Done keeps it with the crop, Cancel takes it back.
        C_FLIP if code == BN_CLICKED => return app.model.flip(),
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
                SetTextColor(dc, COLORREF(p.text));
                LRESULT(theme::brush(p.panel).0 as isize)
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                if !ink::draw_item(item) {
                    // The session is read without the app when it's busy:
                    // a repaint follows once it's free.
                    if with_app(|app| draw_crop_item(item, app.model.crop_session.as_ref())).is_none() {
                        draw_crop_item(item, None);
                        let _ = InvalidateRect(item.hwndItem, None, false);
                    }
                }
                LRESULT(1)
            }
            WM_CTLCOLORBTN => draw::button_brush(),
            WM_NOTIFY => draw::custom_draw(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let is_panel = CROP_RULES.with(Cell::get).0 == hwnd.0 as isize;
                if is_panel {
                    paint_crop_panel(hwnd);
                } else {
                    draw::paint_panel(hwnd, theme::palette().panel);
                }
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
    use crate::inspector_view::Note;
    use crate::state::{ADJUSTMENT_GROUPS, Section};
    use serde_json::Value;

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

    /// The tray's sections, as the Mac's `StockTray`: the favourites,
    /// untitled, then each group; while searching each match once under
    /// its group, without the favourites or empty groups.
    #[test]
    fn a_search_shows_each_match_once_under_its_group() {
        use crate::session::Preferences;
        let stock = |id: &str, name: &str, bw: bool| state::Stock {
            id: id.into(), name: name.into(), paper: false, positive: false, bw, target: String::new(), times: vec![], grain: Value::Null, fotufilm: false, manufacturer_data: true, negs_built: false,
        };
        let mut model = Model::new(Preferences { previews: true, favorite_films: vec!["gold".into()], ..Preferences::default() });
        model.stocks = vec![stock("gold", "Gold 200", false), stock("portra", "Portra 400", false), stock("trix", "Tri-X 400", true)];
        model.source = Some(std::path::PathBuf::from("photo.png"));
        model.previews.tray = Some(crate::previews::TrayVisit::new(false, model.state.clone(), String::new()));
        let shown = |model: &Model| -> Vec<(bool, Vec<String>)> { model.tray_sections().into_iter().map(|s| (s.favourites, s.stocks.into_iter().map(|e| e.id).collect())).collect() };
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Not searching: Scan's Your negative, the favourites, then every group.
        for query in ["", "   "] {
            model.previews.tray.as_mut().unwrap().query = query.into();
            assert_eq!(shown(&model), [(false, ids(&[crate::negative::STOCK_ID])), (true, ids(&["gold"])), (false, ids(&["gold", "portra"])), (false, ids(&["trix"]))]);
        }
        let search = |model: &mut Model, query: &str| {
            model.previews.tray.as_mut().unwrap().query = query.into();
            shown(model)
        };
        // A starred match shows once, under its group.
        assert_eq!(search(&mut model, "GOLD"), [(false, ids(&["gold"]))]);
        assert_eq!(search(&mut model, "black 400"), [(false, ids(&["trix"]))]);
        assert_eq!(search(&mut model, "400"), [(false, ids(&["portra"])), (false, ids(&["trix"]))]);
        assert!(search(&mut model, "gold 400").is_empty());
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
                if let Adjustment::FilterLink { .. } = control {
                    assert!(rows.iter().any(|r| matches!(r.kind, Kind::FilterLink(_))));
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

    /// The rows a model shows, by kind, with every group open: the view.
    fn shown(model: &crate::model::Model) -> Vec<Kind> {
        let ui = InspectorUi { tab: Tab::from_title(model.prefs.tab()), expanded: Expanded::All, decimal: '.' };
        view::build(&model.inspector_facts(), &ui).rows.iter().filter(|r| r.visible).filter_map(|r| r.kind).collect()
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
        assert!(rows.iter().any(|k| matches!(k, Kind::FilterLink(..))), "Shifts move is the Enlarger's fourth basic");
        assert!(rows.iter().any(|k| matches!(k, Kind::BalancePrint(..))), "Balance Print is the Enlarger's fifth basic");
        assert!(!rows.iter().any(slider("enlarger.preflash_exposure")));
        assert!(!rows.iter().any(slider("camera.lens_blur_um")));
        // Scan & Color always heads its "N more" line, as on the Mac.
        assert!(!rows.iter().any(title("Scanner")) && rows.iter().any(|k| *k == Kind::Section("Scan & Color")));
        assert!(!rows.iter().any(|k| matches!(k, Kind::Toggle("Scan film directly", _))));
        assert!(rows.iter().any(|k| *k == Kind::More(More::Section(Section::Scan))));
        // Revealing a module's hidden controls shows them, and folds back.
        let key = "Enlarger".to_string();
        model.prefs.set_revealed(&key, true);
        assert!(shown(&model).iter().any(slider("enlarger.preflash_exposure")));
        model.prefs.set_revealed(&key, false);
        assert!(!shown(&model).iter().any(slider("enlarger.preflash_exposure")));
        // A change shows its control, and its group and section.
        crate::json::set(&mut model.state.params, "enlarger.preflash_exposure", serde_json::json!(0.1));
        crate::json::set(&mut model.state.params, "scanner.lens_blur", serde_json::json!(1.0));
        let rows = shown(&model);
        assert!(rows.iter().any(slider("enlarger.preflash_exposure")) && rows.iter().any(title("Scanner")));
        model.prefs.lab_all_controls = true;
        let rows = shown(&model);
        assert!(rows.iter().any(|k| *k == Kind::More(More::All)), "Showing all controls. Show fewer");
        assert!(rows.iter().any(slider("camera.lens_blur_um")) && rows.iter().any(title("Color Management")));
        assert!(rows.iter().any(|k| matches!(k, Kind::Toggle("Scan film directly", _))) && rows.iter().any(title("Output")));
        assert!(rows.iter().all(|k| !matches!(k, Kind::ExposureOffset)), "Prep's rows stay on Prep");
    }

    /// Prep shows the meter while matching is off or unavailable, or when
    /// it's changed. The stocks have no rows: the label under the print
    /// chooses them, as on the Mac.
    #[test]
    fn prep_shows_the_meter_when_the_match_doesnt_set_it() {
        let mut model = model();
        let meter = |rows: &[Kind]| rows.iter().any(|k| matches!(k, Kind::Toggle("Auto exposure", _)));
        assert_eq!(Tab::from_title(model.prefs.tab()), Tab::Prep, "The inspector opens on Prep");
        assert!(meter(&shown(&model)));
        model.state.brightness_match = Some(state::BrightnessMatch::default());
        model.is_raw = true;
        let rows = shown(&model);
        assert!(!meter(&rows) && rows.iter().any(|k| *k == Kind::Note(Note::Brightness)));
        crate::json::set(&mut model.state.params, "camera.auto_exposure", serde_json::json!(false));
        assert!(meter(&shown(&model)));
        for name in ["Prep", "Lab", "Output"] {
            model.prefs.choose_tab(name);
            assert!(shown(&model).iter().all(|k| !matches!(k, Kind::DevTime(_))) || name == "Lab", "{name}");
        }
    }

    /// The schema row of `test`.
    fn row_id(test: impl Fn(&Kind) -> bool) -> usize {
        view::rows().iter().position(|r| test(&r.kind)).unwrap()
    }

    /// An action on a real model, as a click does it.
    fn act(model: &mut Model, id: usize, action: RowAction) {
        for effect in view::dispatch(&model.inspector_facts(), &ui_state(model, '.'), id, action) {
            assert_eq!(apply_to_model(model, effect), Applied::Done);
        }
    }

    /// The view's counts are the edit lists' lengths on a real model, and
    /// the header's total is theirs while a photograph is open.
    #[test]
    fn counts_match_the_lists() {
        let mut model = model();
        model.source = Some(std::path::PathBuf::from("photo.png"));
        let before = build_view(&model, '.').counts;
        model.set("prep.highlights", crate::json::num(12.0));
        model.set("film_render.grain.active", Value::Bool(false));
        model.set_border(|b| b.enabled = true, "Change Border");
        let view = build_view(&model, '.');
        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            assert_eq!(crate::edits::edits(tab, &model.inspector_facts(), '.').len(), view.counts[i], "{tab:?}");
        }
        assert_eq!(view.counts[0], before[0] + 1);
        assert_eq!(view.counts[2], 1);
        assert_eq!(view.total, view.counts.iter().sum::<usize>());
        model.source = None;
        assert_eq!(build_view(&model, '.').total, 0);
    }

    /// Each control's action is one undo step with the Mac's name, and its
    /// reset another; undoing both puts the edit back.
    #[test]
    fn actions_and_resets_are_undo_steps() {
        let mut model = model();
        model.source = Some(std::path::PathBuf::from("photo.png"));
        model.prefs.choose_tab("Lab");
        let start = model.state.clone();
        let print = row_id(|k| matches!(k, Kind::Slider(a) if a.path() == "enlarger.print_exposure"));
        act(&mut model, print, RowAction::Set(1.5));
        assert_eq!(crate::json::number(model.value("enlarger.print_exposure")), 1.5);
        assert_eq!(model.undo_title(), Some("Change Adjustment"));
        assert!(view::resettable(&model.inspector_facts(), print) && build_view(&model, '.').row(print).changed);
        act(&mut model, print, RowAction::Reset { double_click: false });
        assert_eq!(crate::json::number(model.value("enlarger.print_exposure")), 1.0);
        assert!(!view::resettable(&model.inspector_facts(), print));
        model.perform_undo();
        model.perform_undo();
        assert!(crate::json::same(&model.state.params, &start.params));
        assert_eq!(model.undo_title(), None);
        // A drag: live values, one step when let go.
        act(&mut model, print, RowAction::Track(150, Track::Drag));
        act(&mut model, print, RowAction::Track(160, Track::Drag));
        assert_eq!(model.undo_title(), None, "no step while dragging");
        assert!(!crate::json::same(&model.state.params, &start.params));
        act(&mut model, print, RowAction::Track(160, Track::End));
        assert_eq!(model.undo_title(), Some("Change Adjustment"));
        model.perform_undo();
        assert!(crate::json::same(&model.state.params, &start.params));
        // The border's switch and the edit list's reset name their steps.
        model.prefs.choose_tab("Output");
        let border = row_id(|k| matches!(k, Kind::BorderToggle(_, 0)));
        act(&mut model, border, RowAction::Check(true));
        assert!(model.state.border.enabled);
        assert_eq!(model.undo_title(), Some(view::command::CHANGE_BORDER));
        let list = crate::edits::edits(Tab::Output, &model.inspector_facts(), '.');
        let all = view::edit_list(Tab::Output, &list, view::EditChoice::All).unwrap();
        assert_eq!(apply_to_model(&mut model, all), Applied::Done);
        assert_eq!(model.undo_title(), Some("Reset Output"));
        assert!(!model.state.border.enabled);
        // Dialogs are left to the window.
        assert_eq!(apply_to_model(&mut model, Effect::ChooseLightFrame), Applied::Dialog(Effect::ChooseLightFrame));
    }
}

