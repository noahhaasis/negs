//! What each inspector tab holds (`TabEdits.swift`, `InspectorEdits`): the
//! controls that differ from their defaults, in the order the tab shows
//! them, for the count beside each tab, the pane header's "3 edits", and the
//! list a click on a count opens (each edit can go back alone, or the whole
//! tab at once).

use serde_json::Value;

use crate::json;
use crate::inspector_view::{COLOR_SPACES, InspectorFacts};
use crate::state::{self, ADJUSTMENT_GROUPS, Adjustment, Section};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tab {
    Prep,
    Lab,
    Output,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Prep, Tab::Lab, Tab::Output];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Prep => "Prep",
            Tab::Lab => "Lab",
            Tab::Output => "Output",
        }
    }
}

/// What a count stands for, and what going back does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Param(&'static str),
    /// Prep › Match camera brightness, back to its setting for new photographs.
    MatchBrightness,
    /// Prep › Exposure, back to the match (or 0).
    Exposure,
    /// Lab › Scan film directly, back to what the film is (negative or slide).
    ScanFilm,
    /// Lab › Output › Colour space, back to the first of the spaces.
    PreviewSpace,
    Border,
    /// Camera › Film format: the value, and the Custom pick.
    FilmFormat,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Edit {
    pub title: String,
    pub value: String,
    pub target: Target,
}

pub const WHITE_BALANCE_PATHS: [&str; 3] = ["io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint"];
pub const INPUT_PATHS: [&str; 5] =
    ["io.input_interpretation.mode", "io.input_interpretation.color_space", "io.input_interpretation.linear", "io.input_color_space", "io.input_cctf_decoding"];
pub const FILM_DEVELOPMENT: &str = "film_render.development_time";
pub const PRINT_DEVELOPMENT: &str = "print_render.development_time";
const CAMERA_LOOSE: [&str; 4] = ["camera.exposure_compensation_ev", "camera.film_format_mm", "camera.lens_blur_um", state::ColorFilter::PATH];

fn control_modified(control: &Adjustment, facts: &InspectorFacts) -> bool {
    !matches!(control, Adjustment::BalancePrint { .. } | Adjustment::FilterLink { .. }) && facts.is_modified(control.path())
}

/// A module's count: its switch, then its controls.
pub fn group_count(index: usize, facts: &InspectorFacts) -> usize {
    let group = &ADJUSTMENT_GROUPS[index];
    group.enable.is_some_and(|path| facts.is_modified(path)) as usize + group.controls.iter().filter(|c| control_modified(c, facts)).count()
}

pub fn scan_film_modified(facts: &InspectorFacts) -> bool {
    let positive = facts.stocks().iter().find(|s| !s.paper && s.id == facts.state().film).is_some_and(|s| s.positive);
    json::boolean(facts.value("io.scan_film")) != positive
}

/// The Lab's Output module: the preview's colour space and its curve.
pub fn output_count(facts: &InspectorFacts) -> usize {
    (facts.preview_space() != COLOR_SPACES[0]) as usize + facts.is_modified("io.output_cctf_encoding") as usize
}

pub fn section_count(section: Section, facts: &InspectorFacts) -> usize {
    let groups: usize = ADJUSTMENT_GROUPS.iter().enumerate().filter(|(_, g)| g.section == section).map(|(i, _)| group_count(i, facts)).sum();
    groups
        + match section {
            Section::Camera => CAMERA_LOOSE.iter().filter(|p| facts.is_modified(p)).count(),
            Section::Film => facts.is_modified(FILM_DEVELOPMENT) as usize,
            Section::Print => scan_film_modified(facts) as usize + facts.is_modified(PRINT_DEVELOPMENT) as usize,
            Section::Scan => output_count(facts),
        }
}

pub fn brightness_count(facts: &InspectorFacts) -> usize {
    (facts.matches_brightness() != facts.prefs.match_new_photos) as usize
        + facts.exposure_offset_modified() as usize
        + ["camera.auto_exposure", "camera.auto_exposure_method"].iter().filter(|p| facts.is_modified(p)).count()
}

fn prep_count(facts: &InspectorFacts) -> usize {
    let mut count = brightness_count(facts);
    count += state::PREP_TONE.iter().chain(state::PREP_COLOR).chain(state::PREP_DETAIL).filter(|c| control_modified(c, facts)).count();
    if facts.is_raw {
        count += WHITE_BALANCE_PATHS.iter().filter(|p| facts.is_modified(p)).count();
        if facts.lens_supported && facts.is_modified(state::LENS_CORRECTION) {
            count += 1;
        }
    } else {
        count += INPUT_PATHS.iter().filter(|p| facts.is_modified(p)).count();
    }
    count
}

pub fn count(tab: Tab, facts: &InspectorFacts) -> usize {
    match tab {
        Tab::Prep => prep_count(facts),
        Tab::Lab => [Section::Camera, Section::Film, Section::Print, Section::Scan].into_iter().map(|s| section_count(s, facts)).sum(),
        Tab::Output => facts.is_modified(state::SIZE_TARGET) as usize + facts.state().border.enabled as usize,
    }
}

/// The controls on `tab` that are off their defaults, in the tab's order.
/// Always as many as `count(tab, facts)`.
pub fn edits(tab: Tab, facts: &InspectorFacts, decimal: char) -> Vec<Edit> {
    let mut out = vec![];
    let f = Format { decimal };
    let path = |out: &mut Vec<Edit>, title: &str, path: &'static str, unit: &str| {
        if facts.is_modified(path) {
            out.push(Edit { title: title.into(), value: f.display(facts.value(path)) + unit, target: Target::Param(path) });
        }
    };
    match tab {
        Tab::Prep => {
            if facts.is_raw {
                path(&mut out, "White balance", WHITE_BALANCE_PATHS[0], "");
                path(&mut out, "Temperature (K)", WHITE_BALANCE_PATHS[1], "");
                path(&mut out, "Tint", WHITE_BALANCE_PATHS[2], "");
            }
            if facts.matches_brightness() != facts.prefs.match_new_photos {
                out.push(Edit { title: "Match camera brightness".into(), value: on_off(facts.matches_brightness()), target: Target::MatchBrightness });
            }
            if facts.exposure_offset_modified() {
                out.push(Edit { title: "Exposure".into(), value: f.signed(facts.exposure_offset()), target: Target::Exposure });
            }
            path(&mut out, "Auto exposure", "camera.auto_exposure", "");
            path(&mut out, "Metering", "camera.auto_exposure_method", "");
            for control in state::PREP_TONE.iter().chain(state::PREP_COLOR).chain(state::PREP_DETAIL) {
                append(&mut out, control, None, facts, &f);
            }
            if facts.is_raw {
                if facts.lens_supported {
                    path(&mut out, "Lens corrections", state::LENS_CORRECTION, "");
                }
            } else {
                for input in INPUT_PATHS {
                    path(&mut out, &format!("Input {}", words(input)), input, "");
                }
            }
        }
        Tab::Lab => {
            for section in [Section::Camera, Section::Film, Section::Print, Section::Scan] {
                match section {
                    Section::Camera => {
                        path(&mut out, "Exposure compensation", "camera.exposure_compensation_ev", "");
                        let format = state::FILM_FORMAT.path();
                        if facts.is_modified(format) {
                            let mm = json::number(facts.value(format));
                            let value = state::film_format(mm).map_or_else(|| format!("{} mm", f.number(mm)), |i| state::FILM_FORMATS[i].0.to_string());
                            out.push(Edit { title: "Film format".into(), value, target: Target::FilmFormat });
                        }
                        path(&mut out, "Lens blur (µm)", "camera.lens_blur_um", "");
                        if facts.is_modified(state::ColorFilter::PATH) {
                            let value = facts.chosen_color_filter().map_or_else(|| "None".into(), |f| f.name.clone());
                            out.push(Edit { title: "Color filter".into(), value, target: Target::Param(state::ColorFilter::PATH) });
                        }
                    }
                    Section::Film => path(&mut out, "Film development", FILM_DEVELOPMENT, " min"),
                    Section::Print => {
                        if scan_film_modified(facts) {
                            out.push(Edit { title: "Scan film directly".into(), value: on_off(json::boolean(facts.value("io.scan_film"))), target: Target::ScanFilm });
                        }
                        path(&mut out, "Print development", PRINT_DEVELOPMENT, " min");
                    }
                    Section::Scan => {
                        if facts.preview_space() != COLOR_SPACES[0] {
                            out.push(Edit { title: "Colour space".into(), value: facts.preview_space().into(), target: Target::PreviewSpace });
                        }
                        path(&mut out, "Encode with the colour space’s curve", "io.output_cctf_encoding", "");
                    }
                }
                for group in ADJUSTMENT_GROUPS.iter().filter(|g| g.section == section) {
                    if let Some(enable) = group.enable
                        && facts.is_modified(enable)
                    {
                        out.push(Edit { title: group.title.into(), value: on_off(json::boolean(facts.value(enable))), target: Target::Param(enable) });
                    }
                    for control in group.controls {
                        append(&mut out, control, Some(group.title), facts, &f);
                    }
                }
            }
        }
        Tab::Output => {
            if facts.is_modified(state::SIZE_TARGET) {
                out.push(Edit { title: "Image size".into(), value: format!("{} px", f.number(json::number(facts.value(state::SIZE_TARGET)))), target: Target::Param(state::SIZE_TARGET) });
            }
            if facts.state().border.enabled {
                out.push(Edit { title: "Border".into(), value: "On".into(), target: Target::Border });
            }
        }
    }
    out
}

fn append(out: &mut Vec<Edit>, control: &Adjustment, group: Option<&str>, facts: &InspectorFacts, f: &Format) {
    if !control_modified(control, facts) {
        return;
    }
    let titled = |title: &str| group.map_or_else(|| title.to_string(), |g| format!("{g} · {title}"));
    match *control {
        Adjustment::Slider { title, path, low, .. } => {
            let title = if facts.film_is_bw() { state::bw_title(path).unwrap_or(title) } else { title };
            let value = json::number(facts.value(path));
            out.push(Edit { title: titled(title), value: if low < 0.0 { f.signed(value) } else { f.number(value) }, target: Target::Param(path) });
        }
        Adjustment::Toggle { title, path } => out.push(Edit { title: titled(title), value: on_off(json::boolean(facts.value(path))), target: Target::Param(path) }),
        Adjustment::Choice { title, path, .. } => out.push(Edit { title: titled(title), value: state::option_title(&json::string(facts.value(path))), target: Target::Param(path) }),
        Adjustment::BalancePrint { .. } | Adjustment::FilterLink { .. } => {}
    }
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.into()
}

/// "as_shot" as "As shot"; a path's last part likewise.
pub fn words(text: &str) -> String {
    let spaced = text.rsplit('.').next().unwrap_or(text).replace('_', " ");
    let mut chars = spaced.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

struct Format {
    decimal: char,
}

impl Format {
    /// Up to two decimals, without grouping.
    fn number(&self, value: f64) -> String {
        let text = format!("{value:.2}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        let text = if text == "-0" { "0" } else { text };
        if self.decimal != '.' { text.replace('.', &self.decimal.to_string()) } else { text.to_string() }
    }
    /// With its sign, except at zero.
    fn signed(&self, value: f64) -> String {
        let text = self.number(value);
        if value > 0.0 && text != "0" { format!("+{text}") } else { text }
    }
    fn display(&self, value: &Value) -> String {
        match value {
            Value::Number(n) => self.number(n.as_f64().unwrap_or(0.0)),
            Value::Bool(b) => on_off(*b),
            Value::String(s) => words(s),
            _ => "Changed".into(),
        }
    }
}

// MARK: What the Lab shows (`InspectorReveal`)

/// The Lab keeps back each module's controls past its basics, and each
/// section's expert modules and rarer rows, behind "N more" (round 11, B).
/// View › Show All Controls shows everything; a control off its default
/// always shows.
pub fn section_title(section: Section) -> &'static str {
    match section {
        Section::Camera => "Camera",
        Section::Film => "Film",
        Section::Print => "Print",
        Section::Scan => "Scan & Color",
    }
}

/// The `more_adjustments` key of a section's "N more".
pub fn section_key(section: Section) -> String {
    format!("section:{}", section_title(section))
}

/// Whether a section's extra modules and rows show.
pub fn shows_section(facts: &InspectorFacts, section: Section) -> bool {
    facts.prefs.lab_all_controls || facts.prefs.revealed(&section_key(section))
}

/// A module shows when it has basics, its section's extras show, or something in it changed.
pub fn group_shows(facts: &InspectorFacts, index: usize) -> bool {
    let group = &ADJUSTMENT_GROUPS[index];
    group.basic > 0 || shows_section(facts, group.section) || group_count(index, facts) > 0
}

/// Whether a module's control at `place` shows: a basic, revealed, or changed.
pub fn control_shows(facts: &InspectorFacts, index: usize, place: usize) -> bool {
    let group = &ADJUSTMENT_GROUPS[index];
    facts.prefs.lab_all_controls || facts.prefs.revealed(group.title) || place < group.basic || control_modified(&group.controls[place], facts)
}

/// Whether a control draws at all: a B&W film has one grain lane, not three.
fn drawn(facts: &InspectorFacts, control: &Adjustment) -> bool {
    !(state::color_only(control.path()) && facts.film_is_bw())
}

/// What a "N more" line does when clicked.
#[derive(Clone, PartialEq, Debug)]
pub enum MoreAction {
    Reveal(String),
    Conceal(String),
    /// Turns View › Show All Controls off.
    ShowFewer,
}

/// A module's "N more controls…", or once clicked "Hide N controls".
pub fn module_more(facts: &InspectorFacts, index: usize) -> Option<(String, MoreAction)> {
    let group = &ADJUSTMENT_GROUPS[index];
    let drawn: Vec<usize> = (0..group.controls.len()).filter(|&i| drawn(facts, &group.controls[i])).collect();
    let shown = drawn.iter().filter(|&&i| control_shows(facts, index, i)).count();
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let more = drawn.len() - shown;
    if more > 0 {
        return Some((format!("{more} more control{}…", plural(more)), MoreAction::Reveal(group.title.into())));
    }
    if facts.prefs.lab_all_controls || !facts.prefs.revealed(group.title) {
        return None;
    }
    let fewer = drawn.iter().filter(|&&i| i >= group.basic && !control_modified(&group.controls[i], facts)).count();
    (fewer > 0).then(|| (format!("Hide {fewer} control{}", plural(fewer)), MoreAction::Conceal(group.title.into())))
}

/// A section's "2 more: Scanner, Color Management…" for the modules and
/// rows it keeps back; once clicked, "Hide Scanner, Color Management".
pub fn section_more(facts: &InspectorFacts, section: Section) -> Option<(String, MoreAction)> {
    let scanning = json::boolean(facts.value("io.scan_film"));
    let loose: &[&str] = match section {
        Section::Camera if facts.is_modified("camera.lens_blur_um") => &[],
        Section::Camera => &["Lens blur"],
        Section::Film => &[],
        Section::Print if scanning => return None,
        Section::Print => &["Scan film directly"],
        Section::Scan if output_count(facts) > 0 => &[],
        Section::Scan => &["Output"],
    };
    let groups = || ADJUSTMENT_GROUPS.iter().enumerate().filter(|(_, g)| g.section == section);
    let key = section_key(section);
    if !shows_section(facts, section) {
        let hidden: Vec<&str> = groups().filter(|(i, _)| !group_shows(facts, *i)).map(|(_, g)| g.title).chain(loose.iter().copied()).collect();
        return (!hidden.is_empty()).then(|| (format!("{} more: {}…", hidden.len(), hidden.join(", ")), MoreAction::Reveal(key)));
    }
    if facts.prefs.lab_all_controls || !facts.prefs.revealed(&key) {
        return None;
    }
    let foldable: Vec<&str> = groups().filter(|(i, g)| g.basic == 0 && group_count(*i, facts) == 0).map(|(_, g)| g.title).chain(loose.iter().copied()).collect();
    (!foldable.is_empty()).then(|| (format!("Hide {}", foldable.join(", ")), MoreAction::Conceal(key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector_view::fixture::Fixture;

    #[test]
    fn more_lines_reveal_and_fold_back() {
        let mut model = Fixture::new();
        let (index, group) = ADJUSTMENT_GROUPS.iter().enumerate().find(|(_, g)| g.basic > 0 && g.basic < g.controls.len()).unwrap();
        let (text, action) = module_more(&model.facts(), index).unwrap();
        assert!(text.ends_with("more control…") || text.ends_with("more controls…"), "{text}");
        assert_eq!(action, MoreAction::Reveal(group.title.into()));
        model.reveal(group.title, true);
        assert!((0..group.controls.len()).all(|i| control_shows(&model.facts(), index, i)));
        let (text, action) = module_more(&model.facts(), index).unwrap();
        assert!(text.starts_with("Hide "), "{text}");
        assert_eq!(action, MoreAction::Conceal(group.title.into()));
        // A section keeps back its expert modules and loose rows.
        let (text, _) = section_more(&model.facts(), Section::Camera).unwrap();
        assert!(text.contains("Lens blur"), "{text}");
        model.lab_all_controls = true;
        assert_eq!(section_more(&model.facts(), Section::Camera), None);
        assert_eq!(module_more(&model.facts(), index), None);
    }

    #[test]
    fn words_read_like_the_mac() {
        assert_eq!(words("as_shot"), "As shot");
        assert_eq!(words("io.input_interpretation.mode"), "Mode");
        assert_eq!(words(""), "");
    }

    #[test]
    fn numbers_keep_two_decimals_and_a_sign() {
        let f = Format { decimal: ',' };
        assert_eq!(f.number(1.5), "1,5");
        assert_eq!(f.number(2.0), "2");
        assert_eq!(f.signed(12.0), "+12");
        assert_eq!(f.signed(-0.25), "-0,25");
        assert_eq!(f.signed(0.0), "0");
    }

    #[test]
    fn counts_match_the_lists() {
        let mut model = Fixture::new();
        // A fresh model already differs from new photos' brightness match.
        let before = count(Tab::Prep, &model.facts());
        assert_eq!(before, 1);
        model.set("prep.highlights", json::num(12.0));
        model.set("film_render.grain.active", Value::Bool(false));
        model.state.border.enabled = true;
        let facts = model.facts();
        for tab in Tab::ALL {
            assert_eq!(edits(tab, &facts, '.').len(), count(tab, &facts), "{tab:?}");
        }
        assert_eq!(count(Tab::Prep, &facts), before + 1);
        assert!(edits(Tab::Prep, &facts, '.').iter().any(|e| e.value == "+12"));
        assert_eq!(count(Tab::Output, &facts), 1);
        let lab: Vec<(String, String)> = edits(Tab::Lab, &facts, ',').into_iter().map(|e| (e.title, e.value)).collect();
        assert_eq!(lab, [("Grain".to_string(), "Off".to_string())]);
    }
}
