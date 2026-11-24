//! What the inspector's controls do: an action on a row becomes the model
//! changes it makes, as data. A platform turns its controls' events into
//! `RowAction`s, calls `dispatch`, and applies each `Effect` to the model
//! in order; the effects keep the model's own undo names and steps.

use serde_json::Value;

use crate::border::{self, BorderColor, BorderSettings, BorderStyle};
use crate::edits::{Edit, MoreAction, Tab, Target};
use crate::json;
use crate::negative::Pick;
use crate::state::{self, Adjustment, FilterLink, SizeMode};

use super::facts::InspectorFacts;
use super::output::{BorderWidth, COLOR_SPACES};
use super::schema::{Kind, More, RowId, TONE_CHANNELS, rows};
use super::view::{InspectorUi, more_line, slider_value};

/// How a trackbar moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    /// Dragged: a drag is one undo step with live frames.
    Drag,
    /// The drag ended.
    End,
    /// A click on the track or a key: one edit.
    Step,
}

/// What happened to a row's control.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RowAction {
    /// A slider's value, typed or set.
    Set(f64),
    /// A slider's trackbar at `position` steps from its low end.
    Track(i32, Track),
    /// A tick box, or a module's switch, now on or off.
    Check(bool),
    /// A list's choice; negative for none.
    Select(isize),
    /// The row's button `n`: a module's header, a "N more" line, Balance
    /// Print, a negative's buttons, Choose… (0) or Off (1) on Flat field, a
    /// tone graph channel, a border style, a colour well.
    Press(usize),
    /// Back to the default: the ↺, a double-click on the label, Reset to
    /// Default; `double_click` when a double-click's first click on a
    /// shift's track moved it.
    Reset { double_click: bool },
}

/// A change to the border, one undo step ("Change Border").
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BorderChange {
    Enabled(bool),
    Instagram(bool),
    Export(bool),
    Style(BorderStyle),
    /// A width, as a fraction (`BorderWidth::set`).
    Width(BorderWidth, f64),
}

impl BorderChange {
    pub fn apply(self, b: &mut BorderSettings) {
        match self {
            BorderChange::Enabled(on) => b.enabled = on,
            BorderChange::Instagram(on) => b.instagram = on,
            BorderChange::Export(on) => b.export = on,
            BorderChange::Style(style) => b.style = style,
            BorderChange::Width(field, value) => field.set(b, value),
        }
    }
}

/// The undo name of the border's steps.
pub const CHANGE_BORDER: &str = "Change Border";
/// The undo name of a slider's drag.
pub const CHANGE_ADJUSTMENT: &str = "Change Adjustment";

/// The negative's buttons in Prep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegativeAction {
    FindAgain,
    Pick(Pick),
    CropToPicture,
    AutoPrint,
    UseForRoll,
    RollFiltration,
}

/// One change to the model, as `Model`'s method of that name makes it.
#[derive(Clone, PartialEq, Debug)]
pub enum Effect {
    /// `Model::set`: one value, one undo step ("Change Adjustment").
    Set(&'static str, Value),
    /// `Model::set_filter_shift`: a magenta or yellow shift, moving the other
    /// as Shifts move says.
    SetFilterShift(&'static str, f64),
    /// `Model::reset_value`.
    ResetValue(&'static str),
    /// `Model::reset_filter_shift`.
    ResetFilterShift(&'static str),
    /// `Model::reset_filter_shift_after_click`: a double-click on the track,
    /// whose first click is undone first.
    ResetFilterShiftAfterClick(&'static str),
    /// `Model::set_exposure_offset`, `reset_exposure_offset`.
    SetExposureOffset(f64),
    ResetExposureOffset,
    /// `Model::set_border_percent`: a width in percent.
    SetBorderPercent(BorderWidth, f64),
    /// `Model::set_border`, with its undo name.
    Border(BorderChange, &'static str),
    /// `Model::begin_interaction`, `end_interaction` with its undo name.
    BeginInteraction,
    EndInteraction(&'static str),
    SetMatchesBrightness(bool),
    SwitchColorFilter(bool),
    ChooseFilmFormat(Option<usize>),
    ChoosePreviewSpace(&'static str),
    ChooseImageSize(SizeMode),
    SetFilterLink(FilterLink),
    BalancePrint,
    /// Ask for the light frame (a file dialog), then `choose_flat_field`.
    ChooseLightFrame,
    ClearFlatField,
    Negative(NegativeAction),
    /// The inspector's own preferences (no undo step).
    ToneGraphChannel(&'static str),
    /// A module opens or closes.
    SetExpanded(&'static str, bool),
    /// A "N more" line's click.
    More(MoreAction),
    /// Prep's "Backlight…".
    ShowBacklight,
    /// Ask for a colour (a colour dialog) starting at `current`, then
    /// `set_border_color` (one undo step per OK).
    ChooseBorderColor { line: bool, current: BorderColor },
    /// `Model::reset_edits`: the edits a tab's list names, one undo step.
    ResetEdits(Vec<Target>, String),
}

/// A slider set to `value`: snapped to its step (`set_slider`).
fn set_slider(kind: Kind, value: f64) -> Vec<Effect> {
    match kind {
        Kind::Slider(Adjustment::Slider { path, low, high, step, .. }) if FilterLink::links(path) => vec![Effect::SetFilterShift(path, state::snap(value, low, high, step))],
        Kind::Slider(Adjustment::Slider { path, low, high, step, .. }) => vec![Effect::Set(path, json::num(state::snap(value, low, high, step)))],
        Kind::BorderSlider(_, field) => vec![Effect::SetBorderPercent(field, value)],
        Kind::ExposureOffset => vec![Effect::SetExposureOffset(value)],
        _ => vec![],
    }
}

/// A row back at its default (`reset_row`).
fn reset_row(kind: Kind, double_click: bool) -> Vec<Effect> {
    match kind {
        Kind::Slider(adjustment) if FilterLink::links(adjustment.path()) && double_click => vec![Effect::ResetFilterShiftAfterClick(adjustment.path())],
        Kind::Slider(adjustment) if FilterLink::links(adjustment.path()) => vec![Effect::ResetFilterShift(adjustment.path())],
        Kind::Slider(adjustment) => vec![Effect::ResetValue(adjustment.path())],
        Kind::Toggle(_, path) | Kind::Choice(_, path, _) => vec![Effect::ResetValue(path)],
        Kind::BorderSlider(_, field) => {
            let initial = field.get(&BorderSettings::default());
            vec![Effect::Border(BorderChange::Width(field, initial), CHANGE_BORDER)]
        }
        Kind::ExposureOffset => vec![Effect::ResetExposureOffset],
        _ => vec![],
    }
}

/// Whether Reset to Default can do anything: the row is off its default.
pub fn resettable(facts: &InspectorFacts, id: RowId) -> bool {
    match rows()[id].kind {
        Kind::Slider(a) => facts.is_modified(a.path()),
        Kind::Toggle(_, path) | Kind::Choice(_, path, _) => facts.is_modified(path),
        Kind::BorderSlider(_, field) => (field.get(&facts.state().border) - field.get(&BorderSettings::default())).abs() > 1e-9,
        Kind::ExposureOffset => facts.exposure_offset_modified(),
        _ => false,
    }
}

/// Whether a row offers Reset to Default (its context menu).
pub fn has_reset(id: RowId) -> bool {
    matches!(rows()[id].kind, Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset | Kind::Toggle(..) | Kind::Choice(..))
}

/// What `action` on row `id` does.
pub fn dispatch(facts: &InspectorFacts, ui: &InspectorUi, id: RowId, action: RowAction) -> Vec<Effect> {
    let Some(row) = rows().get(id) else { return vec![] };
    let kind = row.kind;
    match (kind, action) {
        (_, RowAction::Reset { double_click }) => reset_row(kind, double_click),
        (_, RowAction::Set(value)) => set_slider(kind, value),
        (_, RowAction::Track(position, track)) => {
            let Some((_, _, low, _, step)) = slider_value(facts, &kind) else { return vec![] };
            let value = low + position as f64 * step;
            let undo = if matches!(kind, Kind::BorderSlider(..)) { CHANGE_BORDER } else { CHANGE_ADJUSTMENT };
            match track {
                Track::Drag => [Effect::BeginInteraction].into_iter().chain(set_slider(kind, value)).collect(),
                Track::End => vec![Effect::EndInteraction(undo)],
                Track::Step => set_slider(kind, value),
            }
        }
        (Kind::Group { title, .. }, RowAction::Press(_)) => vec![Effect::SetExpanded(title, !ui.expanded.contains(title))],
        (Kind::Group { enable: Some(state::ColorFilter::PATH), .. }, RowAction::Check(on)) => vec![Effect::SwitchColorFilter(on)],
        (Kind::Group { enable: Some(path), .. }, RowAction::Check(on)) | (Kind::Toggle(_, path), RowAction::Check(on)) => vec![Effect::Set(path, Value::Bool(on))],
        (Kind::MatchBrightness, RowAction::Check(on)) => vec![Effect::SetMatchesBrightness(on)],
        (Kind::More(More::Backlight), RowAction::Press(_)) => vec![Effect::ShowBacklight],
        (Kind::More(more), RowAction::Press(_)) => more_line(facts, more).map(|(_, action)| Effect::More(action)).into_iter().collect(),
        (Kind::FilmFormat, RowAction::Select(index)) => vec![Effect::ChooseFilmFormat(usize::try_from(index).ok().filter(|&i| i < state::FILM_FORMATS.len()))],
        (Kind::BalancePrint(..), RowAction::Press(_)) => vec![Effect::BalancePrint],
        (Kind::FlatField, RowAction::Press(0)) => vec![Effect::ChooseLightFrame],
        (Kind::FlatField, RowAction::Press(1)) => vec![Effect::ClearFlatField],
        (Kind::Actions(buttons), RowAction::Press(n)) => {
            let action = match buttons.get(n).map(|b| b.0) {
                Some("Find again") => NegativeAction::FindAgain,
                Some("Pick from rebate") => NegativeAction::Pick(Pick::Base),
                Some("Crop to picture") => NegativeAction::CropToPicture,
                Some("Auto") => NegativeAction::AutoPrint,
                Some("Pick neutral") => NegativeAction::Pick(Pick::Neutral),
                Some("Use for whole roll") => NegativeAction::UseForRoll,
                Some("Roll\u{2019}s filtration") => NegativeAction::RollFiltration,
                _ => return vec![],
            };
            vec![Effect::Negative(action)]
        }
        (Kind::ToneGraph, RowAction::Press(n)) if n < TONE_CHANNELS.len() => vec![Effect::ToneGraphChannel(TONE_CHANNELS[n])],
        (Kind::PreviewSpace, RowAction::Select(index)) => usize::try_from(index).ok().and_then(|i| COLOR_SPACES.get(i)).map(|space| Effect::ChoosePreviewSpace(space)).into_iter().collect(),
        (Kind::SizeMode, RowAction::Select(index)) => vec![Effect::ChooseImageSize(if index == 1 { SizeMode::LongestSide } else { SizeMode::Original })],
        (Kind::BorderToggle(_, which), RowAction::Check(on)) => {
            let change = match which {
                0 => BorderChange::Enabled(on),
                1 => BorderChange::Instagram(on),
                _ => BorderChange::Export(on),
            };
            vec![Effect::Border(change, CHANGE_BORDER)]
        }
        (Kind::FilterLink(_), RowAction::Select(index)) => FilterLink::ALL.get(index as usize).map(|&link| Effect::SetFilterLink(link)).into_iter().collect(),
        (Kind::Choice(_, path, options), RowAction::Select(index)) => options.get(index as usize).map(|option| Effect::Set(path, Value::String(option.to_string()))).into_iter().collect(),
        (Kind::DevTime(paper), RowAction::Select(index)) => {
            let times = facts.stock_times(paper);
            let path = if paper { crate::edits::PRINT_DEVELOPMENT } else { crate::edits::FILM_DEVELOPMENT };
            let value = match index {
                0 => Value::Null,
                i => times.get(i as usize - 1).map_or(Value::Null, |t| json::num(*t)),
            };
            vec![Effect::Set(path, value)]
        }
        (Kind::BorderStyles, RowAction::Press(n)) => border::STYLES.get(n).map(|&style| Effect::Border(BorderChange::Style(style), CHANGE_BORDER)).into_iter().collect(),
        (Kind::Color(line), RowAction::Press(_)) => {
            let b = facts.state().border;
            vec![Effect::ChooseBorderColor { line, current: if line { b.line_color } else { b.color } }]
        }
        _ => vec![],
    }
}

/// What a click in a tab's edit list does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditChoice {
    /// One edit back to its default.
    One(usize),
    /// The whole tab.
    All,
}

/// A tab's edit list's head: "3 edits in Lab".
pub fn edits_head(tab: Tab, count: usize) -> String {
    format!("{count} {} in {}", if count == 1 { "edit" } else { "edits" }, tab.title())
}

/// What a choice in `tab`'s edit list does (`TabEditsList`): one undo step
/// named after the edit, or after the tab.
pub fn edit_list(tab: Tab, edits: &[Edit], choice: EditChoice) -> Option<Effect> {
    match choice {
        EditChoice::All if !edits.is_empty() => Some(Effect::ResetEdits(edits.iter().map(|e| e.target).collect(), format!("Reset {}", tab.title()))),
        EditChoice::All => None,
        EditChoice::One(i) => edits.get(i).map(|edit| Effect::ResetEdits(vec![edit.target], format!("Reset {}", edit.title))),
    }
}
