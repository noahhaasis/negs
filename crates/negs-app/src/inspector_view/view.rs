//! What the inspector shows, from the facts and the inspector's own state:
//! each tab's edit count, every row's visibility, and for each row that
//! shows its words, value, range, state and hints (`AdjustmentInspector`).
//! A platform draws `InspectorView` as it is.

use std::borrow::Cow;

use crate::border::{self, BorderColor, BorderSettings, BorderStyle};
use crate::edits::{self, MoreAction, Tab};
use crate::json;
use crate::state::{self, ADJUSTMENT_GROUPS, Adjustment, Section};

use super::facts::InspectorFacts;
use super::output::{COLOR_SPACES, OutputFormat, QUALITY_RANGE, effective_space, percent};
use super::schema::{Kind, More, NEGATIVE_SKIPS, Note, Row, RowId, When, rows, tone_channel};

/// Which module headers are open.
#[derive(Clone, Copy, Debug)]
pub enum Expanded<'a> {
    /// The open modules' titles, comma-separated
    /// (`Preferences::expanded_adjustments`).
    Titles(&'a str),
    /// Every module (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    All,
}

impl Expanded<'_> {
    pub fn contains(&self, title: &str) -> bool {
        match self {
            Expanded::Titles(titles) => titles.split(',').any(|t| t == title),
            Expanded::All => true,
        }
    }
}

/// What the inspector keeps itself: the tab shown, the open modules and
/// the locale's decimal separator.
#[derive(Clone, Copy, Debug)]
pub struct InspectorUi<'a> {
    pub tab: Tab,
    pub expanded: Expanded<'a>,
    pub decimal: char,
}

/// A slider's place: the value, its default, the range and step, and both
/// as steps from the low end (a trackbar's positions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderValue {
    pub value: f64,
    pub default: f64,
    pub low: f64,
    pub high: f64,
    pub step: f64,
    pub position: isize,
    pub default_position: isize,
}

/// One row as it shows now. Fields a row's kind doesn't use stay at their
/// defaults; rows that don't show have only `id`, `kind` and `visible`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RowView {
    pub id: RowId,
    pub kind: Option<Kind>,
    pub visible: bool,
    /// Its controls take input.
    pub enabled: bool,
    /// Off its default: the changed mark (a section's or module's dot).
    pub changed: bool,
    /// A module or section the photograph skips: Print while scanning film
    /// directly, the camera and the negative's grain, halation and couplers
    /// on a negative. Struck through.
    pub skipped: bool,
    /// A module header: open (expanded, and not skipped by scanning).
    pub open: bool,
    /// A module whose switch is off.
    pub off: bool,
    /// A toggle's tick, a module's switch.
    pub checked: bool,
    /// A module's switch takes input.
    pub switch_enabled: bool,
    /// The row's words: a label, a button's title, a "N more" line.
    pub title: Cow<'static, str>,
    /// The value as text: a slider's box, a readout, the light frame's name.
    pub value: String,
    /// A note's or footer's text, the colour filter's hint.
    pub hint: String,
    /// The colour filter's warning on colour film.
    pub warning: String,
    /// The hint reads as a warning.
    pub warn: bool,
    /// A tooltip that follows the model (Match brightness's; a slider's
    /// reset: "Reset to 0"); fixed ones are `Kind::help`.
    pub help: Option<Cow<'static, str>>,
    /// A folded module's value, a colour well's colour name.
    pub summary: String,
    /// A module's state for screen readers: expanded, collapsed, skipped…
    pub status: &'static str,
    pub slider: Option<SliderValue>,
    /// A list's choice; None selects nothing.
    pub selected: Option<usize>,
    /// A development time's choices ("Profile default", then the times).
    pub options: Vec<String>,
    /// Each of the row's buttons takes input (border styles; flat field's
    /// Choose… and Off).
    pub buttons: Vec<bool>,
    /// A colour well's colour.
    pub color: Option<BorderColor>,
    /// Longest side: an error keeps the typed text for correction.
    pub keep_typed: bool,
}

/// The border's swatches: its colour, its line's, the style chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swatches {
    pub color: BorderColor,
    pub line_color: BorderColor,
    pub style: usize,
}

/// The whole inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorView {
    pub tab: Tab,
    /// Each tab's edits, Prep · Lab · Output (`TabRow`).
    pub counts: [usize; 3],
    /// The pane header's "N edits": all of them, none without a photograph.
    pub total: usize,
    /// Every row of `schema::rows()`, by id.
    pub rows: Vec<RowView>,
    /// Over Output's Export: "6048 × 4024 · PNG 16-bit · sRGB".
    pub export_summary: String,
    pub can_export: bool,
    pub swatches: Swatches,
    /// A tone graph shows (its curves are computed only then).
    pub tone_graph: bool,
    /// The development times the film's and the paper's profiles were
    /// measured at (a development time list fills again when they change).
    pub dev_times: [Vec<f64>; 2],
}

impl InspectorView {
    pub fn row(&self, id: RowId) -> &RowView {
        &self.rows[id]
    }
}

/// A tab's title with its count, for its word's screen-reader name.
pub fn count_words(count: usize) -> &'static str {
    if count == 1 { "edit" } else { "edits" }
}

/// Builds the view.
pub fn build(facts: &InspectorFacts, ui: &InspectorUi) -> InspectorView {
    let schema = rows();
    let counts = Tab::ALL.map(|t| edits::count(t, facts));
    let total = if facts.has_source { counts.iter().sum() } else { 0 };
    let scanning = json::boolean(facts.value("io.scan_film"));
    // Scanning the film skips the print's modules: struck, their bodies hidden.
    let expanded: Vec<bool> = schema.iter().map(|r| matches!(r.kind, Kind::Group { .. }) && ui.expanded.contains(r.kind.group_title()) && !(r.print && scanning)).collect();
    // A group's rows show only below its header, on screen and open.
    let mut want = vec![false; schema.len()];
    for (i, row) in schema.iter().enumerate() {
        let shown = row.tab == ui.tab && visible(row, facts, &|g| want[g] && expanded[g]);
        want[i] = shown;
    }
    // A group's controls are off while its effect is.
    let mut group_off = vec![false; schema.len()];
    for (i, row) in schema.iter().enumerate() {
        if let Kind::Group { enable: Some(path), .. } = row.kind {
            // The glass can be changed with the filter off (`disablesContent: false`).
            group_off[i] = path != state::ColorFilter::PATH && !switch_on(facts, path);
        }
        if let Kind::Group { title, .. } = row.kind {
            group_off[i] |= negative_skips(facts, title);
        }
    }
    let rows: Vec<RowView> = schema
        .iter()
        .enumerate()
        .map(|(i, row)| if want[i] { row_view(facts, ui, i, row, expanded[i], &group_off, scanning) } else { RowView { id: i, kind: Some(row.kind), ..Default::default() } })
        .collect();
    let tone_graph = rows.iter().any(|r| r.visible && r.kind == Some(Kind::ToneGraph));
    let b = facts.state().border;
    InspectorView {
        tab: ui.tab,
        counts,
        total,
        rows,
        export_summary: export_summary(facts),
        can_export: facts.can_export && !facts.cropping,
        swatches: Swatches { color: b.color, line_color: b.line_color, style: border::STYLES.iter().position(|s| *s == b.style).unwrap_or(0) },
        tone_graph,
        dev_times: [facts.stock_times(false), facts.stock_times(true)],
    }
}

/// A "N more" line's words and what a click does; None hides it.
pub fn more_line(facts: &InspectorFacts, more: More) -> Option<(String, MoreAction)> {
    match more {
        More::Module(index) => edits::module_more(facts, index),
        More::Section(section) => edits::section_more(facts, section),
        More::All => facts.prefs.lab_all_controls.then(|| ("Showing all controls. Show fewer".into(), MoreAction::ShowFewer)),
        More::Backlight => (facts.is_negative() && !facts.negative.as_ref().is_some_and(|n| n.backlight_shown)).then(|| ("Backlight…".into(), MoreAction::ShowFewer)),
    }
}

/// The preview's colour space or curve is off its default, so the Lab shows them.
fn output_changed(facts: &InspectorFacts) -> bool {
    facts.preview_space() != COLOR_SPACES[0] || facts.is_modified("io.output_cctf_encoding")
}

/// Whether a row shows. `group_shown` says whether a row's group header is
/// on screen and open.
pub fn visible(row: &Row, facts: &InspectorFacts, group_shown: &dyn Fn(usize) -> bool) -> bool {
    let scanning = json::boolean(facts.value("io.scan_film"));
    let border = facts.state().border;
    let negative = facts.is_negative();
    if row.scene && negative {
        return false;
    }
    let shown = match row.when {
        When::Always => true,
        When::FilmTimes => facts.stock_times(false).len() > 1,
        When::PaperTimes => !scanning && facts.stock_times(true).len() > 1,
        When::Raw => facts.is_raw,
        When::LensCorrection => facts.is_raw && facts.lens_supported,
        When::NotRaw => !facts.is_raw,
        When::AssumeInput => !facts.is_raw && json::string(facts.value("io.input_interpretation.mode")) == "assume",
        When::ManualInput => !facts.is_raw && json::string(facts.value("io.input_interpretation.mode")) == "manual",
        When::CustomWhiteBalance => facts.is_raw && json::string(facts.value("io.raw_white_balance.mode")) == "custom",
        When::Scanning => scanning,
        When::ColorFilm => !facts.film_is_bw(),
        When::ColorFilter => facts.shows_color_filter(),
        When::Negative => negative,
        When::Backlight => negative && facts.negative.as_ref().is_some_and(|n| n.backlight_shown),
        When::OwnFiltration => negative && facts.state().roll.as_ref().is_some_and(|r| !r.filtered(facts.state())),
        When::Ratio35 => facts.photo_pixels().is_some_and(|(w, h)| !border::fits(BorderStyle::Film35, w, h)),
        When::LongestSide => facts.size_mode() == state::SizeMode::LongestSide,
        When::Note => matches!(row.kind, Kind::Note(note) if !note_text(facts, note).is_empty()),
        When::Meter => facts.shows_meter(),
        When::MeterAuto => facts.shows_meter() && json::boolean(facts.value("camera.auto_exposure")),
        When::CustomFormat => facts.film_format_custom(),
        When::Shows(section, path) => edits::shows_section(facts, section) || facts.is_modified(path),
        When::ScanToggle => scanning || edits::shows_section(facts, Section::Print),
        When::OutputGroup => output_changed(facts) || edits::shows_section(facts, Section::Scan),
        When::More(more) => more_line(facts, more).is_some(),
        When::Group(index) => edits::group_shows(facts, index),
        When::Mat => matches!(border.style, BorderStyle::Solid | BorderStyle::Keyline),
        When::Keyline => border.style == BorderStyle::Keyline,
        When::FilmStyle => border.style == BorderStyle::Film,
    };
    // The Lab shows each module's basics, those revealed, and anything
    // changed from its default.
    let basic = row.basic.is_none_or(|(index, place)| edits::control_shows(facts, index, place));
    shown && basic && row.group.is_none_or(|g| group_shown(g))
}

/// A Film module a negative skips: it has its grain, halation and couplers already.
fn negative_skips(facts: &InspectorFacts, title: &str) -> bool {
    NEGATIVE_SKIPS.contains(&title) && facts.is_negative()
}

/// A module's switch: on for a bool path, and for Color Filter while a
/// glass is in front of the lens.
pub fn switch_on(facts: &InspectorFacts, path: &str) -> bool {
    if path == state::ColorFilter::PATH { facts.color_filter() != state::ColorFilter::NONE } else { json::boolean(facts.value(path)) }
}

/// Under Color Filter's discs: what the glass does and costs this film, and
/// on colour film the warning (`ColorFilterPicker`).
fn color_filter_hints(facts: &InspectorFacts, decimal: char) -> (String, String) {
    let chosen = facts.chosen_color_filter();
    let film = facts.stocks().iter().find(|s| !s.paper && s.id == facts.state().film).map_or("this film", |s| s.name.as_str());
    // Auto exposure meters through the glass, and so does matching the camera's brightness.
    let metered = json::boolean(facts.value("camera.auto_exposure")) || facts.matches_brightness();
    let stops = chosen.and_then(|f| facts.color_filters.stops(&f.key));
    let hint = state::ColorFilter::hint(chosen, stops, film, metered, decimal);
    let warning = if chosen.is_some() && !facts.film_is_bw() {
        "Taking filters are made for black-and-white film. On colour film this one tints the whole photograph.".to_string()
    } else {
        String::new()
    };
    (hint, warning)
}

/// What an export makes, over the Export button: "6048 × 4024 · PNG 16-bit
/// · sRGB", from the last save's choices (`ExportFoot.summary`).
pub fn export_summary(facts: &InspectorFacts) -> String {
    let prefs = &facts.prefs;
    let format = OutputFormat::from_key(prefs.output_format);
    let depth = |d: u32| if d == 16 { 16 } else { 8 };
    let (kind, bits) = match format {
        OutputFormat::Jpeg => (format!("JPEG {}", prefs.jpeg_quality.clamp(QUALITY_RANGE.0, QUALITY_RANGE.1)), 8),
        OutputFormat::Png => (format!("PNG {}-bit", depth(prefs.png_bit_depth)), depth(prefs.png_bit_depth)),
        OutputFormat::Tiff => (format!("TIFF {}-bit", depth(prefs.tiff_bit_depth)), depth(prefs.tiff_bit_depth)),
    };
    let chosen = COLOR_SPACES.iter().position(|s| *s == prefs.color_space).unwrap_or(0);
    let space = COLOR_SPACES[effective_space(chosen, bits)];
    let size = facts.photo_pixels().map(|(w, h)| format!("{w} × {h}"));
    size.into_iter().chain([kind, space.to_string()]).collect::<Vec<_>>().join(" · ")
}

/// A note's text; empty hides it.
pub fn note_text(facts: &InspectorFacts, note: Note) -> String {
    let state = facts.state();
    let negative = facts.negative.as_ref().filter(|_| facts.is_negative());
    match note {
        Note::Size => size_note(facts),
        Note::Brightness => facts.brightness.note().unwrap_or_default(),
        _ if negative.is_none() => String::new(),
        Note::NegativeBase => negative.map(|n| n.sentence.clone()).unwrap_or_default(),
        Note::NegativeWarning => negative.and_then(|n| n.warning).unwrap_or_default().into(),
        Note::NegativeGuess => match negative.and_then(|n| n.base.as_ref()) {
            Some(base) if base.guessed() && state.roll.is_some() => "Pick from the rebate if the scan shows clear film; otherwise Use for whole roll on a frame that has it.".into(),
            Some(base) if base.guessed() => "Pick from the rebate if the scan shows clear film; otherwise this guess can tint the print.".into(),
            _ => String::new(),
        },
        Note::FlatField => negative.and_then(|n| n.flat_field.hint()).unwrap_or_default(),
        Note::Roll if state.roll.is_some() => {
            "A frame of a roll: the film, backlight, base and filtration are the roll\u{2019}s; the print exposure is this frame\u{2019}s own.".into()
        }
        Note::OwnFiltration => match &state.roll {
            Some(roll) if !roll.filtered(state) => roll.own_filtration(state).into(),
            _ => String::new(),
        },
        Note::Roll => String::new(),
        Note::NegativeDaylight => {
            if facts.is_raw && json::string(facts.value("io.raw_white_balance.mode")) != "daylight" {
                "Set White balance to Daylight for a negative on a daylight backlight; As shot can tint it.".into()
            } else {
                String::new()
            }
        }
    }
}

/// Why the Image Size can't be used, or that the preview has fewer pixels
/// than the export; empty otherwise.
fn size_note(facts: &InspectorFacts) -> String {
    if let Some(error) = facts.size_error {
        return error.to_string();
    }
    if let Some(Err(error)) = facts.resolved_size() {
        return error.to_string();
    }
    if facts.preview_scale < 1.0 && facts.image_shown {
        return format!("The preview shows {} of these pixels. Exports develop all of them; inspect grain in an exported file.", percent(facts.preview_scale));
    }
    String::new()
}

/// A number in a slider's step, with the locale's separator.
pub fn format(value: f64, step: f64, decimal: char) -> String {
    let text = state::format_value(value, step);
    if decimal != '.' { text.replace('.', &decimal.to_string()) } else { text }
}

/// A folded module's value: its first slider's, without trailing zeros, or
/// its first choice's title (`summary(_ group:)`).
fn group_summary(facts: &InspectorFacts, index: usize, decimal: char) -> Option<String> {
    ADJUSTMENT_GROUPS[index].controls.iter().find_map(|control| match *control {
        Adjustment::Slider { path, step, .. } => {
            let text = format(json::number(facts.value(path)), step, decimal);
            let text = if text.contains(decimal) { text.trim_end_matches('0').trim_end_matches(decimal).to_string() } else { text };
            Some(if text == "-0" { "0".into() } else { text })
        }
        Adjustment::Choice { path, .. } => Some(state::option_title(&json::string(facts.value(path))).to_string()),
        _ => None,
    })
}

/// A slider row's value, default, range and step, as now: the Exposure's
/// range follows the match.
pub fn slider_value(facts: &InspectorFacts, kind: &Kind) -> Option<(f64, f64, f64, f64, f64)> {
    match *kind {
        Kind::ExposureOffset => {
            let (low, high) = facts.exposure_offset_range();
            Some((facts.exposure_offset(), 0.0, low, high, state::PREP_EXPOSURE_STEP))
        }
        Kind::Slider(Adjustment::Slider { path, low, high, step, .. }) => Some((json::number(facts.value(path)), json::number(facts.default_value(path)), low, high, step)),
        Kind::BorderSlider(_, field) => {
            let (low, high, step) = field.range();
            Some((field.get(&facts.state().border) * 100.0, field.get(&BorderSettings::default()) * 100.0, low, high, step))
        }
        _ => None,
    }
}

/// The section a section head titles, for its count and strike.
fn section_of(title: &str) -> Option<Section> {
    [Section::Camera, Section::Film, Section::Print, Section::Scan].into_iter().find(|&s| edits::section_title(s) == title)
}

fn row_view(facts: &InspectorFacts, ui: &InspectorUi, id: RowId, row: &Row, open: bool, group_off: &[bool], scanning: bool) -> RowView {
    let catalog = facts.has_catalog();
    let decimal = ui.decimal;
    let border = facts.state().border;
    let on = catalog && !(row.print && scanning) && !(row.border && !border.enabled) && !row.group.is_some_and(|g| group_off[g]);
    let mut v = RowView { id, kind: Some(row.kind), visible: true, enabled: on, title: Cow::Borrowed(row.kind.label()), ..Default::default() };
    match row.kind {
        Kind::Section(title) => {
            // The Lab's sections: the dot when edited, Print struck while scanning.
            if let Some(s) = section_of(title) {
                v.changed = edits::section_count(s, facts) > 0;
                v.skipped = (s == Section::Print && scanning) || (s == Section::Camera && facts.is_negative());
            }
        }
        Kind::Footer(text) => {
            v.hint = text.to_string();
            if text.is_empty() {
                if let Some((w, h)) = facts.photo_pixels() {
                    v.hint = format!("35mm needs a 3:2 or 3:4 photograph. This one is {}; crop it to 3:2 or 4:3 to use it.", state::ratio_name(w as f64, h as f64));
                }
                v.warn = true;
            }
        }
        Kind::More(more) => {
            if let Some((text, _)) = more_line(facts, more) {
                v.title = Cow::Owned(text);
            }
            v.enabled = catalog;
        }
        Kind::Group { title, enable: switch } => {
            let index = match row.when {
                When::Group(index) => Some(index),
                _ => None,
            };
            v.changed = match (index, title) {
                (Some(index), _) => edits::group_count(index, facts) > 0,
                (None, "Output") => output_changed(facts),
                (None, "Input Colour") => edits::INPUT_PATHS.iter().any(|p| facts.is_modified(p)),
                (None, "Color Filter") => facts.is_modified(state::ColorFilter::PATH),
                _ => false,
            };
            v.off = switch.is_some_and(|path| !switch_on(facts, path));
            v.skipped = (row.print && scanning) || negative_skips(facts, title);
            v.summary = match title {
                "Color Filter" => facts.chosen_color_filter().map_or_else(|| "Off".into(), |f| f.name.clone()),
                _ => index.and_then(|index| group_summary(facts, index, decimal)).unwrap_or_default(),
            };
            v.open = open;
            v.status = if v.skipped && facts.is_negative() {
                "skipped on a negative"
            } else if v.skipped {
                "skipped while scanning film"
            } else if open {
                "expanded"
            } else {
                "collapsed"
            };
            v.enabled = catalog;
            if let Some(path) = switch {
                v.checked = switch_on(facts, path);
                v.switch_enabled = catalog && !v.skipped;
            }
        }
        Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
            if let Some((value, default, low, high, step)) = slider_value(facts, &row.kind) {
                if let Kind::Slider(Adjustment::Slider { title, path, .. }) = row.kind
                    && let Some(bw) = state::bw_title(path)
                {
                    v.title = Cow::Borrowed(if facts.film_is_bw() { bw } else { title });
                }
                let position = ((value - low) / step).round() as isize;
                let default_position = ((default - low) / step).round() as isize;
                v.slider = Some(SliderValue { value, default, low, high, step, position, default_position });
                v.value = format(value, step, decimal);
                v.changed = match row.kind {
                    Kind::Slider(Adjustment::Slider { path, .. }) => facts.is_modified(path),
                    Kind::ExposureOffset => facts.exposure_offset_modified(),
                    _ => (value - default).abs() > 1e-7,
                };
                v.help = Some(Cow::Owned(format!("Reset to {}", format(default, step, decimal))));
            }
        }
        Kind::Toggle(_, path) => {
            v.checked = json::boolean(facts.value(path));
            v.changed = facts.is_modified(path);
        }
        Kind::MatchBrightness => {
            let raw = facts.is_raw;
            v.title = Cow::Borrowed(if raw { "Match camera brightness" } else { "Match original brightness" });
            v.help = Some(Cow::Borrowed(if raw {
                "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it."
            } else {
                "Develops the photograph about as bright as the file itself, and keeps it there when you change film, paper or a preset. Exposure adds to it."
            }));
            v.checked = facts.matches_brightness();
        }
        Kind::FilmFormat => {
            v.changed = facts.is_modified(state::FILM_FORMAT.path());
            v.selected = Some(if facts.film_format_custom() {
                state::FILM_FORMATS.len()
            } else {
                state::film_format(json::number(facts.value(state::FILM_FORMAT.path()))).unwrap_or(state::FILM_FORMATS.len())
            });
        }
        Kind::ColorFilter => {
            let (hint, warning) = color_filter_hints(facts, decimal);
            v.hint = hint;
            v.warning = warning;
            v.warn = true;
        }
        Kind::ToneGraph => {
            // Not off with the print curves: a contrast drag turns them on.
            v.enabled = catalog && !scanning;
            v.selected = tone_channel(facts.prefs.tone_graph_channel);
        }
        Kind::Actions(buttons) => {
            v.enabled = catalog && !facts.negative_busy();
            v.buttons = vec![v.enabled; buttons.len()];
        }
        Kind::FlatField => {
            let flat = facts.negative.as_ref().map(|n| n.flat_field.clone()).unwrap_or_default();
            v.value = flat.value();
            v.changed = flat.is_on();
            v.enabled = catalog && !facts.negative_busy();
            v.buttons = vec![v.enabled, v.enabled && flat.is_on()];
        }
        Kind::BalancePrint(title, _) => {
            v.title = Cow::Borrowed(if facts.balancing_print { "Balancing…" } else { title });
            v.enabled = on && facts.can_balance_print;
        }
        Kind::PreviewSpace => {
            let selected = COLOR_SPACES.iter().position(|s| *s == facts.preview_space()).unwrap_or(0);
            v.changed = selected != 0;
            v.selected = Some(selected);
            v.enabled = catalog;
        }
        Kind::SizeMode => {
            let selected = (facts.size_mode() == state::SizeMode::LongestSide) as usize;
            v.changed = selected != 0;
            v.selected = Some(selected);
            v.enabled = catalog;
        }
        Kind::LongestSide => {
            v.value = state::size_target(&facts.state().params).map_or_else(String::new, |v| v.to_string());
            // A rejected value stays for correction until it is fixed or Esc restores it.
            v.keep_typed = facts.size_error.is_some();
            v.enabled = catalog;
        }
        Kind::Photograph => {
            let size = facts.photograph_size();
            v.value = if size.is_zero() {
                "—".to_string()
            } else {
                let mp = size.width * size.height / 1e6;
                format!("{} × {} · {:.*} MP", size.width as u64, size.height as u64, if mp < 10.0 { 1 } else { 0 }, mp)
            };
        }
        Kind::Note(note) => v.hint = note_text(facts, note),
        Kind::BorderToggle(_, which) => {
            v.checked = [border.enabled, border.instagram, border.export][which as usize];
            v.enabled = catalog && (which == 0 || border.enabled);
        }
        Kind::FilterLink(_) => {
            v.selected = Some(state::FilterLink::ALL.iter().position(|&l| l == facts.filter_link()).unwrap_or(0));
        }
        Kind::Choice(_, path, options) => {
            v.changed = facts.is_modified(path);
            v.selected = options.iter().position(|o| *o == json::string(facts.value(path)));
        }
        Kind::DevTime(paper) => {
            let times = facts.stock_times(paper);
            v.options = std::iter::once("Profile default".to_string()).chain(times.iter().map(|t| format!("{} min", state::format_value(*t, 0.01)))).collect();
            let path = if paper { edits::PRINT_DEVELOPMENT } else { edits::FILM_DEVELOPMENT };
            v.changed = facts.is_modified(path);
            let value = facts.value(path).as_f64();
            v.selected = Some(value.and_then(|v| times.iter().position(|t| (t - v).abs() < 1e-9)).map_or(0, |i| i + 1));
            v.enabled = catalog;
        }
        Kind::BorderStyles => {
            let photo = facts.photo_pixels();
            v.buttons = border::STYLES.iter().map(|&style| on && photo.is_none_or(|(w, h)| border::fits(style, w, h))).collect();
            v.selected = border::STYLES.iter().position(|s| *s == border.style);
        }
        Kind::Color(line) => {
            let color = if line { border.line_color } else { border.color };
            v.summary = match color {
                BorderColor::WHITE => "White".to_string(),
                BorderColor::BLACK => "Black".to_string(),
                c => c.hex().to_uppercase(),
            };
            let defaults = BorderSettings::default();
            v.changed = color != if line { defaults.line_color } else { defaults.color };
            v.color = Some(color);
        }
    }
    v
}
