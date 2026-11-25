//! The inspector's rows: the pinned area's and the three tabs', in order
//! (`Inspector.swift`: `filmAndPaper`, `prep`, `lab` and `output`), each
//! with its kind, its tab, the module it folds into and the rule that shows
//! it. Static: the view (`view::build`) says what each shows and holds.

use std::sync::OnceLock;

use crate::border;
use crate::edits::Tab;
use crate::state::{self, ADJUSTMENT_GROUPS, Adjustment, Section};

use super::output::BorderWidth;

/// A row's place in `rows()`, its stable id. Win32 control ids, macOS and
/// Linux views and commands name rows by it.
pub type RowId = usize;

impl Tab {
    /// `Preferences::tab()`'s words: Prep, Lab or Output.
    pub fn from_title(title: &str) -> Tab {
        match title {
            "Lab" => Tab::Lab,
            "Output" => Tab::Output,
            _ => Tab::Prep,
        }
    }
}

/// When a row shows (`visible` in `view.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    Always,
    /// Input colour, for files that aren't RAW.
    NotRaw,
    AssumeInput,
    ManualInput,
    FilmTimes,
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
    /// A section's loose row: its "N more" was clicked, or it changed.
    Shows(Section, &'static str),
    ScanToggle,
    OutputGroup,
    /// A "N more" line with something to say.
    More(More),
    /// An adjustment group: it has basics, or something in it changed.
    Group(usize),
    Mat,
    Keyline,
    FilmStyle,
    /// Hidden for B&W films (one emulsion): per-channel grain lanes.
    ColorFilm,
    /// Lab › Camera › Color Filter: B&W films, or while a filter is set.
    ColorFilter,
    /// The photograph is read as a negative (Prep's Film base and Balance).
    Negative,
    /// Prep › Backlight, once its "Backlight…" line was clicked.
    Backlight,
    /// A frame of a roll with a filtration of its own.
    OwnFiltration,
}

/// What a row is, and so what it draws and does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// A section's head in small capitals; untitled, only a gap.
    Section(&'static str),
    /// Fixed small text under a section's head; "" is the 35mm warning.
    Footer(&'static str),
    /// A module's header (a foldout), with its switch when `enable` is set.
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
    /// "N more controls…", "Hide …", or "Showing all controls. Show fewer".
    More(More),
    /// The colour space the preview develops in (a preference).
    PreviewSpace,
    /// Image Size: Original or Longest side.
    SizeMode,
    LongestSide,
    /// The exported photograph's size.
    Photograph,
    /// Small text that follows the model.
    Note(Note),
    /// Add border (0), Extend to 3:4 for Instagram (1), Include in export (2).
    BorderToggle(&'static str, u8),
    Choice(&'static str, &'static str, &'static [&'static str]),
    /// The film's (false) or paper's (true) development time.
    DevTime(bool),
    BorderStyles,
    /// The border's colour, or its line's (true).
    Color(bool),
    /// The Balance Print button: title, tooltip.
    BalancePrint(&'static str, &'static str),
    /// Enlarger › Shifts move (a preference): its tooltip.
    FilterLink(&'static str),
    /// The tone graph atop Print Curves, with its label and channel buttons.
    ToneGraph,
    /// Color Filter's glass discs, the hint and the colour film warning.
    ColorFilter,
    /// Prep's buttons on a negative, at the row's end: title, tooltip.
    Actions(&'static [(&'static str, &'static str)]),
    /// Prep › Flat field: the light frame's name, Choose… and Off (`FlatFieldRow`).
    FlatField,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// An Image Size error, or that the preview shows fewer pixels.
    Size,
    /// What the brightness match did, or why it can't.
    Brightness,
    /// Where the negative's film base came from, or what's being read.
    NegativeBase,
    /// The scan doesn't look like a colour negative.
    NegativeWarning,
    /// The base is a guess: pick it from the rebate.
    NegativeGuess,
    /// A RAW negative not on Daylight white balance.
    NegativeDaylight,
    /// The light frame isn't used on this frame, and why.
    FlatField,
    /// A frame of a roll: what's the roll's and what's its own.
    Roll,
    /// A frame of a roll with its own filtration: why.
    OwnFiltration,
}

/// What a "N more" line folds: a module's controls, a section's modules
/// and rows, or (at the Lab's top) View › Show All Controls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum More {
    Module(usize),
    Section(Section),
    All,
    /// Prep › Film base on a negative: "Backlight…" (`NegativePrep`'s `MoreLine`).
    Backlight,
}

/// Prep's buttons on a negative (`NegativePrep`).
pub const FIND_BUTTONS: &[(&str, &str)] = &[
    ("Find again", "Looks for the clear film again, forgetting a picked point."),
    ("Pick from rebate", "Click the clear film between frames or along the edge. Esc cancels."),
];
pub const CROP_BUTTONS: &[(&str, &str)] = &[("Crop to picture", "Crops to the picture inside the clear film, levelled by the rebate.")];
pub const ROLL_BUTTONS: &[(&str, &str)] = &[(
    "Use for whole roll",
    "Prints every photograph in view as a frame of this roll: this film, backlight, base and filtration, each frame with its own exposure. Frames that set their own filtration keep it.",
)];
pub const ROLL_FILTRATION_BUTTONS: &[(&str, &str)] = &[("Roll\u{2019}s filtration", "Prints this frame with the roll\u{2019}s magenta and yellow again, and meters its exposure.")];
pub const BALANCE_BUTTONS: &[(&str, &str)] = &[
    ("Auto", "Sets the enlarger's magenta, yellow and print exposure so this frame prints as a minilab's auto printer would."),
    ("Pick neutral", "Click something that should be grey. Esc cancels."),
];
/// Prep › Flat field's buttons, after the light frame's name.
pub const FLAT_FIELD_BUTTONS: [(&str, &str); 2] = [
    ("Choose…", "Choose the light frame: a frame of the bare light, shot with this roll's camera, lens and aperture."),
    ("Off", "Stop dividing the scan by the light frame."),
];
/// The Film module's words for what a negative skips.
pub const NEGATIVE_SKIPS: [&str; 3] = ["Grain", "Halation", "DIR Couplers"];
/// The tone graph's channels: RGB, then the red, green and blue layers.
pub const TONE_CHANNELS: [&str; 4] = ["rgb", "r", "g", "b"];
/// The tone graph's buttons, and what each shows.
pub const TONE_CHANNEL_BUTTONS: [(&str, &str); 4] =
    [("RGB", "Show the neutral curve"), ("R", "Show the red layer"), ("G", "Show the green layer"), ("B", "Show the blue layer")];
/// Image Size › Size's choices.
pub const SIZE_MODES: [&str; 2] = ["Original", "Longest side"];
/// A "N more" line's tooltip.
pub const MORE_HELP: &str = "Shows them here. View › Show All Controls shows every control.";

/// The tone graph's selected layer: None for RGB.
pub fn tone_channel(key: &str) -> Option<usize> {
    TONE_CHANNELS.iter().position(|c| *c == key).and_then(|i| i.checked_sub(1))
}

/// One row of the inspector.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub kind: Kind,
    pub tab: Tab,
    /// The row index of the group header this row belongs to.
    pub group: Option<RowId>,
    pub when: When,
    /// Disabled while scanning film directly (the print groups).
    pub print: bool,
    /// Border controls, disabled while the border is off.
    pub border: bool,
    /// Prep's tone, colour and detail: on a scene, before the film; a
    /// negative is film already (the convert-film route).
    pub scene: bool,
    /// An adjustment group's control: the group and its place, for the
    /// Lab's basics.
    pub basic: Option<(usize, usize)>,
}

/// The rows, in order; built once.
pub fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(build_rows)
}

fn build_rows() -> Vec<Row> {
    let mut rows = vec![];
    let add = |rows: &mut Vec<Row>, kind: Kind, tab: Tab, group: Option<usize>, when: When| {
        rows.push(Row { kind, tab, group, when, print: false, border: false, scene: false, basic: None });
        rows.len() - 1
    };
    let prep = Tab::Prep;
    // An untitled section: 8 DIPs, the white balance, a rule.
    add(&mut rows, Kind::Section(""), prep, None, When::Raw);
    add(&mut rows, Kind::Choice("White balance", "io.raw_white_balance.mode", state::WHITE_BALANCE), prep, None, When::Raw);
    add(&mut rows, Kind::Slider(state::WB_TEMPERATURE), prep, None, When::CustomWhiteBalance);
    add(&mut rows, Kind::Slider(state::WB_TINT), prep, None, When::CustomWhiteBalance);
    // Your negative: the film base, the scan light and the balance.
    add(&mut rows, Kind::Section("Film base"), prep, None, When::Negative);
    add(&mut rows, Kind::Note(Note::NegativeBase), prep, None, When::Note);
    add(&mut rows, Kind::Note(Note::NegativeWarning), prep, None, When::Note);
    add(&mut rows, Kind::Note(Note::NegativeGuess), prep, None, When::Note);
    add(&mut rows, Kind::Actions(FIND_BUTTONS), prep, None, When::Negative);
    add(&mut rows, Kind::FlatField, prep, None, When::Negative);
    add(&mut rows, Kind::Note(Note::FlatField), prep, None, When::Note);
    add(&mut rows, Kind::Actions(CROP_BUTTONS), prep, None, When::Negative);
    // Behind "Backlight…", as on the Mac: Daylight suits most light pads and scanners.
    add(&mut rows, Kind::Choice("Backlight", crate::negative::SCAN_ILLUMINANT, &["D55", "D50", "D65"]), prep, None, When::Backlight);
    add(&mut rows, Kind::More(More::Backlight), prep, None, When::More(More::Backlight));
    add(&mut rows, Kind::Note(Note::Roll), prep, None, When::Note);
    add(&mut rows, Kind::Actions(ROLL_BUTTONS), prep, None, When::Negative);
    add(&mut rows, Kind::Section("Balance"), prep, None, When::Negative);
    add(&mut rows, Kind::Footer("Balance Print ran when the base was found, and the print exposure was metered. Auto prints this frame as a minilab would, and runs again on each new paper; the picker balances on something grey. Both set Lab › Enlarger."), prep, None, When::Negative);
    add(&mut rows, Kind::Actions(BALANCE_BUTTONS), prep, None, When::Negative);
    add(&mut rows, Kind::Note(Note::OwnFiltration), prep, None, When::Note);
    add(&mut rows, Kind::Actions(ROLL_FILTRATION_BUTTONS), prep, None, When::OwnFiltration);
    add(&mut rows, Kind::Note(Note::NegativeDaylight), prep, None, When::Note);
    let scene = rows.len();
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
    for row in &mut rows[scene..] {
        row.scene = true;
    }
    // How the file is read; set once per photograph.
    add(&mut rows, Kind::Section("Lens"), prep, None, When::LensCorrection);
    add(&mut rows, Kind::Toggle("Lens corrections", state::LENS_CORRECTION), prep, None, When::LensCorrection);
    add(&mut rows, Kind::Section(""), prep, None, When::NotRaw);
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
                    Adjustment::FilterLink { help } => Kind::FilterLink(help),
                    slider => Kind::Slider(slider),
                };
                let when = if state::color_only(control.path()) { When::ColorFilm } else { When::Always };
                let row = add(rows, kind, lab, Some(header), when);
                rows[row].print = section == Section::Print;
                rows[row].basic = Some((index, place));
            }
            let more = More::Module(index);
            let row = add(rows, Kind::More(more), lab, Some(header), When::More(more));
            rows[row].print = section == Section::Print;
        }
    };
    let section_more = |rows: &mut Vec<Row>, section: Section| {
        let more = More::Section(section);
        add(rows, Kind::More(more), lab, None, When::More(more));
    };
    // View › Show All Controls says so where it shows, with the way back.
    add(&mut rows, Kind::More(More::All), lab, None, When::More(More::All));
    add(&mut rows, Kind::Section("Camera"), lab, None, When::Always);
    add(&mut rows, Kind::Footer("The camera is skipped: your negative was exposed already."), lab, None, When::Negative);
    add(&mut rows, Kind::Slider(state::NEGATIVE_EXPOSURE), lab, None, When::Always);
    add(&mut rows, Kind::FilmFormat, lab, None, When::Always);
    add(&mut rows, Kind::Slider(state::FILM_FORMAT), lab, None, When::CustomFormat);
    add(&mut rows, Kind::Slider(state::LENS_BLUR), lab, None, When::Shows(Section::Camera, state::LENS_BLUR.path()));
    // Its switch takes the glass off and puts the last one back.
    let filter = add(&mut rows, Kind::Group { title: "Color Filter", enable: Some(state::ColorFilter::PATH) }, lab, None, When::ColorFilter);
    add(&mut rows, Kind::ColorFilter, lab, Some(filter), When::ColorFilter);
    groups(&mut rows, Section::Camera);
    section_more(&mut rows, Section::Camera);
    add(&mut rows, Kind::Section("Film"), lab, None, When::Always);
    add(&mut rows, Kind::Footer("Your negative is read through the film's curves and base. Its grain, halation and couplers are skipped: the negative has them already."), lab, None, When::Negative);
    // The stock's development time, from the times it was measured at.
    add(&mut rows, Kind::DevTime(false), lab, None, When::FilmTimes);
    groups(&mut rows, Section::Film);
    section_more(&mut rows, Section::Film);
    add(&mut rows, Kind::Section("Print"), lab, None, When::Always);
    add(&mut rows, Kind::DevTime(true), lab, None, When::PaperTimes);
    // Here rather than beside the stocks: few photographs want it.
    add(&mut rows, Kind::Toggle("Scan film directly", "io.scan_film"), lab, None, When::ScanToggle);
    add(&mut rows, Kind::Footer("Printing and the paper are skipped while scanning film directly."), lab, None, When::Scanning);
    groups(&mut rows, Section::Print);
    section_more(&mut rows, Section::Print);
    add(&mut rows, Kind::Section("Scan & Color"), lab, None, When::Always);
    groups(&mut rows, Section::Scan);
    let output = add(&mut rows, Kind::Group { title: "Output", enable: None }, lab, None, When::OutputGroup);
    add(&mut rows, Kind::PreviewSpace, lab, Some(output), When::OutputGroup);
    add(&mut rows, Kind::Toggle("Encode with the colour space’s curve", "io.output_cctf_encoding"), lab, Some(output), When::OutputGroup);
    section_more(&mut rows, Section::Scan);

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

impl Kind {
    /// A module header's title; empty for other rows.
    pub fn group_title(&self) -> &'static str {
        match self {
            Kind::Group { title, .. } => title,
            _ => "",
        }
    }

    /// The label of a row that has one (a slider's, a list's); a
    /// B&W film may rename a slider (`RowView::title`).
    pub fn label(&self) -> &'static str {
        match *self {
            Kind::Slider(Adjustment::Slider { title, .. }) | Kind::BorderSlider(title, _) => title,
            Kind::ExposureOffset => "Exposure (EV)",
            Kind::Toggle(title, _) | Kind::BorderToggle(title, _) | Kind::Choice(title, _, _) | Kind::BalancePrint(title, _) => title,
            Kind::MatchBrightness => "Match camera brightness",
            Kind::FilmFormat => "Film format",
            Kind::FilterLink(_) => "Shifts move",
            Kind::PreviewSpace => "Colour space",
            Kind::SizeMode => "Size",
            Kind::LongestSide => "Longest side",
            Kind::Photograph => "Photograph",
            Kind::DevTime(paper) => {
                if paper {
                    "Print development"
                } else {
                    "Development"
                }
            }
            Kind::Color(line) => {
                if line {
                    "Line color"
                } else {
                    "Color"
                }
            }
            Kind::FlatField => "Flat field",
            Kind::ToneGraph => "Tone graph",
            Kind::Section(title) | Kind::Footer(title) => title,
            Kind::Group { title, .. } => title,
            Kind::Slider(_) | Kind::More(_) | Kind::Note(_) | Kind::BorderStyles | Kind::ColorFilter | Kind::Actions(_) => "",
        }
    }

    /// A slider's range and step: the Exposure's moves with the match
    /// (`RowView::slider` has the current one), the number of steps doesn't.
    pub fn range(&self) -> Option<(f64, f64, f64)> {
        match *self {
            Kind::Slider(Adjustment::Slider { low, high, step, .. }) => Some((low, high, step)),
            Kind::BorderSlider(_, field) => Some(field.range()),
            Kind::ExposureOffset => Some((state::PREP_EXPOSURE_RANGE.0, state::PREP_EXPOSURE_RANGE.1, state::PREP_EXPOSURE_STEP)),
            _ => None,
        }
    }

    /// The choices a list offers, in order; a development time's are the
    /// stock's (`RowView::options`).
    pub fn options(&self) -> Vec<String> {
        match *self {
            Kind::Choice(_, _, options) => options.iter().map(|o| state::option_title(o)).collect(),
            Kind::FilmFormat => state::FILM_FORMATS.iter().map(|(name, _)| name.to_string()).chain(["Custom…".to_string()]).collect(),
            Kind::PreviewSpace => super::output::COLOR_SPACES.iter().map(|s| s.to_string()).collect(),
            Kind::SizeMode => SIZE_MODES.iter().map(|s| s.to_string()).collect(),
            Kind::FilterLink(_) => state::FilterLink::ALL.iter().map(|l| l.title().to_string()).collect(),
            Kind::BorderStyles => border::STYLES.iter().map(|s| border::title(*s).to_string()).collect(),
            _ => vec![],
        }
    }

    /// The row's tooltip, where it doesn't follow the model (`RowView::help`
    /// has those).
    pub fn help(&self) -> Option<&'static str> {
        match *self {
            Kind::Slider(Adjustment::Slider { path, .. }) => crate::help::text(path),
            Kind::Group { title, .. } => crate::help::group(title),
            Kind::More(_) => Some(MORE_HELP),
            Kind::Toggle("Scan film directly", _) => Some("Skip printing and scan the developed film, as for slides."),
            Kind::Toggle("Lens corrections", _) => Some("Corrects distortion, vignetting and colour fringing with the data the camera recorded for its lens."),
            Kind::Toggle("Auto exposure", _) => Some("Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it."),
            Kind::Toggle("Encode with the colour space’s curve", _) => Some("Off writes linear values, for further processing."),
            Kind::Toggle(_, path) | Kind::Choice(_, path, _) => crate::help::text(path),
            Kind::BorderToggle(_, 0) => Some("Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it."),
            Kind::BorderToggle(_, 1) => Some("Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post."),
            Kind::BorderToggle(_, 2) => Some("Adds the border to exported photographs and saved previews."),
            Kind::FilmFormat => Some(crate::help::text(state::FILM_FORMAT.path()).unwrap_or_default()),
            Kind::FlatField => Some(crate::negative::FlatField::HELP),
            Kind::BalancePrint(_, help) | Kind::FilterLink(help) => Some(help),
            Kind::PreviewSpace => Some("The preview develops in this colour space, so it shows the colours an export in it can hold. Save and Export use it next, and switch it when you pick another."),
            Kind::SizeMode => Some("Original develops the photograph’s own pixels. Longest side enlarges or reduces it first, so the cropped photograph has that many pixels on its longest side."),
            Kind::LongestSide => Some("The cropped photograph’s longest side, before any border. Press Enter to apply. Exports can still be reduced in the save dialog."),
            Kind::Photograph => Some("The exported photograph, cropped, before rotation, the border, and any export size."),
            _ => None,
        }
    }

    /// Text drawn grey: footers, notes and the tone graph's label.
    pub fn quiet(&self) -> bool {
        matches!(self, Kind::Footer(_) | Kind::Note(_) | Kind::ToneGraph)
    }
}
