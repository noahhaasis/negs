//! The view builder's and the dispatch's own tests, on `fixture::Fixture`:
//! every row kind, the visibility rules, notes, summaries, "N more" lines,
//! counts, and actions undone by their resets.

use std::mem::discriminant;

use serde_json::{Value, json};

use super::fixture::{Fixture, stock};
use super::*;
use crate::border::{BorderColor, BorderSettings, BorderStyle};
use crate::crop::Size;
use crate::edits::{self, MoreAction, Tab, Target};
use crate::json as js;
use crate::state::{self, Adjustment, Section};

/// The kinds of the rows that show.
fn shown(f: &Fixture) -> Vec<Kind> {
    f.view().rows.into_iter().filter(|r| r.visible).filter_map(|r| r.kind).collect()
}

fn shows(f: &Fixture, test: impl Fn(&Kind) -> bool) -> bool {
    shown(f).iter().any(test)
}

fn slider(path: &'static str) -> impl Fn(&Kind) -> bool {
    move |k| matches!(k, Kind::Slider(a) if a.path() == path)
}

fn group(name: &'static str) -> impl Fn(&Kind) -> bool {
    move |k| matches!(k, Kind::Group { title, .. } if *title == name)
}

/// The row of `kind` that shows.
#[track_caller]
fn row_of(view: &InspectorView, test: impl Fn(&Kind) -> bool) -> RowView {
    view.rows.iter().find(|r| r.visible && r.kind.as_ref().is_some_and(&test)).cloned().expect("no such row")
}

fn id_of(test: impl Fn(&Kind) -> bool) -> RowId {
    rows().iter().position(|r| test(&r.kind)).unwrap()
}

/// A fixture whose defaults hold every path a row edits, so a reset puts
/// back exactly what was there.
fn full() -> Fixture {
    let mut f = Fixture::new();
    for row in rows() {
        let (path, value) = match row.kind {
            Kind::Slider(Adjustment::Slider { path, low, high, .. }) => (path, js::num(0.0f64.clamp(low, high))),
            Kind::Toggle(_, path) => (path, Value::Bool(false)),
            Kind::Choice(_, path, options) => (path, Value::String(options[0].into())),
            _ => continue,
        };
        if state::grain_lane(path).is_none() && js::get(&f.defaults, path).is_null() {
            js::set(&mut f.defaults, path, value);
        }
    }
    js::set(&mut f.defaults, "camera.auto_exposure", json!(true));
    js::set(&mut f.defaults, "io.output_cctf_encoding", json!(true));
    f.state.params = f.defaults.clone();
    f
}

/// What `Model` does with each effect, for the edits the fixture holds.
fn apply(f: &mut Fixture, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Set(path, value) => f.set(path, value),
            Effect::SetFilterShift(path, value) => f.set(path, js::num(value)),
            Effect::ResetValue(path) | Effect::ResetFilterShift(path) | Effect::ResetFilterShiftAfterClick(path) => {
                let value = f.facts().default_value(path).clone();
                f.set(path, value);
            }
            Effect::SetExposureOffset(value) => f.set(state::PREP_EXPOSURE, js::num(value)),
            Effect::ResetExposureOffset => f.set(state::PREP_EXPOSURE, js::num(0.0)),
            Effect::SetBorderPercent(field, percent) => {
                let (low, high, step) = field.range();
                field.set(&mut f.state.border, state::snap(percent, low, high, step) / 100.0);
            }
            Effect::Border(change, undo) => {
                assert_eq!(undo, command::CHANGE_BORDER);
                change.apply(&mut f.state.border);
            }
            other => panic!("not an edit: {other:?}"),
        }
    }
}

/// Every kind of row shows in some state, and each state's rows are on
/// its tab.
#[test]
fn every_row_kind_shows() {
    let mut seen = vec![];
    let mut look = |f: &Fixture| {
        for kind in shown(f) {
            seen.push(discriminant(&kind));
        }
    };
    let mut f = full();
    f.all_open = true;
    f.lab_all_controls = true;
    f.stocks[0].times = vec![6.0, 8.0];
    f.stocks[2].times = vec![1.5, 2.0];
    f.filters = vec![state::ColorFilter { key: "red".into(), name: "Red".into(), glass: "25".into(), colour: 0xff0000 }];
    f.photo_size = Size::new(3000.0, 1000.0);
    f.source_size = (3000, 1000);
    for tab in Tab::ALL {
        f.tab = tab;
        look(&f);
    }
    f.is_raw = true;
    f.lens_supported = true;
    f.tab = Tab::Prep;
    look(&f);
    f.set(state::SIZE_TARGET, json!(2000));
    f.tab = Tab::Output;
    f.state.border.enabled = true;
    for style in [BorderStyle::Keyline, BorderStyle::Film, BorderStyle::Film35] {
        f.state.border.style = style;
        look(&f);
    }
    f.set("io.scan_film", json!(true));
    f.tab = Tab::Lab;
    look(&f);
    f.set("io.scan_film", json!(false));
    f.custom_film_format = true;
    look(&f);
    // A B&W film: its colour filter.
    f.state.film = "trix".into();
    look(&f);
    f.state.film = "portra".into();
    // A negative: Prep's film base, notes and buttons.
    f.set(crate::negative::ROUTE_PATH, json!(crate::negative::ROUTE));
    f.negative = Some(NegativeFacts { sentence: "Reading the negative as Portra 400.".into(), ..Default::default() });
    f.tab = Tab::Prep;
    look(&f);
    for row in rows() {
        assert!(seen.contains(&discriminant(&row.kind)), "{:?} never shows", row.kind);
    }
    // Each row shows only on its tab.
    for tab in Tab::ALL {
        f.tab = tab;
        for r in f.view().rows.iter().filter(|r| r.visible) {
            assert_eq!(rows()[r.id].tab, tab);
            assert_eq!(r.id, rows().iter().position(|s| Some(s.kind) == r.kind && s.tab == tab && rows()[r.id].when == s.when && rows()[r.id].group == s.group).unwrap_or(r.id));
        }
    }
}

#[test]
fn prep_follows_the_file_and_the_brightness_match() {
    let mut f = full();
    // A JPEG reads its colour; a RAW its white balance and lens.
    assert!(shows(&f, group("Input Colour")) && !shows(&f, |k| matches!(k, Kind::Choice("White balance", ..))));
    assert!(!shows(&f, |k| matches!(k, Kind::Choice("Read as", ..))), "inside the closed Input Colour");
    f.expanded = "Input Colour".into();
    assert!(shows(&f, |k| matches!(k, Kind::Choice("Read as", ..))));
    assert!(!shows(&f, |k| matches!(k, Kind::Toggle("Linear values", _))));
    f.set("io.input_interpretation.mode", json!("assume"));
    assert!(shows(&f, |k| matches!(k, Kind::Toggle("Linear values", _))));
    f.set("io.input_interpretation.mode", json!("auto"));
    f.is_raw = true;
    assert!(shows(&f, |k| matches!(k, Kind::Choice("White balance", ..))) && !shows(&f, group("Input Colour")));
    assert!(!shows(&f, slider("io.raw_white_balance.temperature")) && !shows(&f, |k| matches!(k, Kind::Toggle("Lens corrections", _))));
    f.set("io.raw_white_balance.mode", json!("custom"));
    f.lens_supported = true;
    assert!(shows(&f, slider("io.raw_white_balance.temperature")) && shows(&f, |k| matches!(k, Kind::Toggle("Lens corrections", _))));
    // The meter shows while not matching.
    let meter = |f: &Fixture| shows(f, |k| matches!(k, Kind::Toggle("Auto exposure", _)));
    assert!(meter(&f) && shows(&f, |k| matches!(k, Kind::Choice("Metering", ..))));
    f.state.brightness_match = Some(state::BrightnessMatch::default());
    assert!(!meter(&f));
    let view = f.view();
    assert_eq!(row_of(&view, |k| *k == Kind::Note(Note::Brightness)).hint, "Matching…");
    assert_eq!(row_of(&view, |k| *k == Kind::MatchBrightness).title, "Match camera brightness");
    assert!(row_of(&view, |k| *k == Kind::MatchBrightness).checked);
    // Matched at +0.5 EV: the Exposure's range and value are above it.
    f.state.brightness_match = Some(state::BrightnessMatch { ev: Some(0.5), inputs: Some(state::match_inputs(&f.state)), ..Default::default() });
    f.set(state::PREP_EXPOSURE, json!(0.75));
    let view = f.view();
    assert_eq!(row_of(&view, |k| *k == Kind::Note(Note::Brightness)).hint, "Starts at +0.50 EV to match the camera.");
    let exposure = row_of(&view, |k| *k == Kind::ExposureOffset);
    let s = exposure.slider.unwrap();
    assert_eq!((s.value, s.low, s.high, exposure.value.as_str()), (0.25, -3.5, 2.5, "0.25"));
    assert_eq!((s.position, s.default_position), (75, 70));
    assert!(exposure.changed);
    assert_eq!(exposure.help.as_deref(), Some("Reset to 0"));
    // A failure for this photograph says why.
    f.failure_set("No camera preview in this file.");
    assert_eq!(row_of(&f.view(), |k| *k == Kind::Note(Note::Brightness)).hint, "No camera preview in this file.");
    assert!(meter(&f), "matching can't run: the meter shows");
    // A negative is film already: no scene rows.
    f.set(crate::negative::ROUTE_PATH, json!(crate::negative::ROUTE));
    assert!(!shows(&f, |k| *k == Kind::MatchBrightness) && !shows(&f, slider("prep.highlights")));
}

impl Fixture {
    fn failure_set(&mut self, message: &str) {
        self.failure = Some((self.generation, None, message.into()));
    }
}

#[test]
fn negative_rows_and_notes() {
    let mut f = full();
    f.is_raw = true;
    f.set(crate::negative::ROUTE_PATH, json!(crate::negative::ROUTE));
    let base = crate::negative::BaseStatus { found: false, picked: false, edge: None, by_hand: false, from_roll: false };
    f.negative = Some(NegativeFacts { sentence: "Found the film base.".into(), warning: Some("Not a negative?"), base: Some(base), busy: false, ..Default::default() });
    let view = f.view();
    let note = |view: &InspectorView, note: Note| view.rows.iter().find(|r| r.visible && r.kind == Some(Kind::Note(note))).map(|r| r.hint.clone());
    assert_eq!(note(&view, Note::NegativeBase).as_deref(), Some("Found the film base."));
    assert_eq!(note(&view, Note::NegativeWarning).as_deref(), Some("Not a negative?"));
    assert!(note(&view, Note::NegativeGuess).is_some_and(|t| t.ends_with("this guess can tint the print.")));
    assert!(note(&view, Note::NegativeDaylight).is_some(), "As shot white balance");
    assert_eq!(note(&view, Note::Roll), None);
    assert!(row_of(&view, |k| matches!(k, Kind::Actions(b) if b[0].0 == "Find again")).buttons.iter().all(|&on| on));
    let flat = row_of(&view, |k| *k == Kind::FlatField);
    assert_eq!((flat.value.as_str(), flat.changed, flat.buttons.as_slice()), ("Off", false, &[true, false][..]));
    // "Backlight…" opens the scan light's row.
    assert!(row_of(&view, |k| *k == Kind::More(More::Backlight)).title == "Backlight…");
    assert!(!shows(&f, |k| matches!(k, Kind::Choice("Backlight", ..))));
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::More(More::Backlight)), RowAction::Press(0)), [Effect::ShowBacklight]);
    f.negative.as_mut().unwrap().backlight_shown = true;
    assert!(shows(&f, |k| matches!(k, Kind::Choice("Backlight", ..))) && !shows(&f, |k| *k == Kind::More(More::Backlight)));
    // Busy: the buttons wait.
    f.negative.as_mut().unwrap().busy = true;
    assert!(row_of(&f.view(), |k| matches!(k, Kind::Actions(b) if b[0].0 == "Find again")).buttons.iter().all(|&on| !on));
    // The Lab: the camera and the negative's film modules are skipped.
    f.tab = Tab::Lab;
    f.all_open = true;
    let view = f.view();
    assert!(row_of(&view, |k| *k == Kind::Section("Camera")).skipped);
    let grain = row_of(&view, group("Grain"));
    assert!(grain.skipped && grain.status == "skipped on a negative");
    let controls = |r: &&RowView| r.visible && matches!(r.kind, Some(Kind::Slider(_) | Kind::Toggle(..) | Kind::Choice(..))) && rows()[r.id].group.is_some_and(|g| rows()[g].kind.group_title() == "Grain");
    assert!(view.rows.iter().filter(controls).count() > 0 && view.rows.iter().filter(controls).all(|r| !r.enabled));
    // The dispatch names each button's action.
    let find = id_of(|k| matches!(k, Kind::Actions(b) if b[0].0 == "Find again"));
    assert_eq!(dispatch(&f.facts(), &f.ui(), find, RowAction::Press(1)), [Effect::Negative(NegativeAction::Pick(crate::negative::Pick::Base))]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::FlatField), RowAction::Press(0)), [Effect::ChooseLightFrame]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::FlatField), RowAction::Press(1)), [Effect::ClearFlatField]);
}

#[test]
fn the_lab_folds_modules_and_sections() {
    let mut f = full();
    f.tab = Tab::Lab;
    let view = f.view();
    // Closed modules show their summary and no controls.
    let enlarger = row_of(&view, group("Enlarger"));
    assert!(enlarger.open && enlarger.status == "expanded");
    assert!(shows(&f, slider("enlarger.print_exposure")));
    let curves = row_of(&view, group("Film Curves"));
    // Its switch has no default here: off, and the dot stays away.
    assert!(!curves.open && curves.status == "collapsed" && !curves.checked && curves.off && !curves.changed, "{curves:?}");
    let grain = row_of(&view, group("Grain"));
    assert!(grain.checked && grain.switch_enabled && !grain.off);
    assert!(!shows(&f, |k| *k == Kind::ToneGraph));
    // A module's basics and "N more"; a section's hidden modules.
    assert!(!shows(&f, slider("enlarger.preflash_exposure")));
    let module_more = |f: &Fixture| f.view().rows.iter().find(|r| r.visible && matches!(r.kind, Some(Kind::More(More::Module(i))) if state::ADJUSTMENT_GROUPS[i].title == "Enlarger")).map(|r| r.title.to_string());
    let more = module_more(&f).unwrap();
    assert!(more.ends_with("more control…") || more.ends_with("more controls…"), "{more}");
    let scan = row_of(&view, |k| *k == Kind::More(More::Section(Section::Scan)));
    assert_eq!(scan.title, "3 more: Scanner, Color Management, Output…");
    assert!(!shows(&f, group("Scanner")) && !shows(&f, group("Output")));
    let id = id_of(|k| *k == Kind::More(More::Section(Section::Scan)));
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Press(0)), [Effect::More(MoreAction::Reveal("section:Scan & Color".into()))]);
    f.reveal("section:Scan & Color", true);
    assert!(shows(&f, group("Scanner")) && shows(&f, group("Output")));
    assert_eq!(row_of(&f.view(), |k| *k == Kind::More(More::Section(Section::Scan))).title, "Hide Scanner, Color Management, Output");
    // A change shows its control, and the module's dot and the section's.
    f.reveal("section:Scan & Color", false);
    f.set("scanner.lens_blur", json!(1.0));
    let view = f.view();
    assert!(row_of(&view, group("Scanner")).changed && row_of(&view, |k| *k == Kind::Section("Scan & Color")).changed);
    assert!(!row_of(&view, |k| *k == Kind::Section("Film")).changed);
    // Show All Controls says so at the top.
    f.lab_all_controls = true;
    let view = f.view();
    assert_eq!(row_of(&view, |k| *k == Kind::More(More::All)).title, "Showing all controls. Show fewer");
    assert!(module_more(&f).is_none());
    // A group header click opens or closes it.
    let header = id_of(group("Enlarger"));
    assert_eq!(dispatch(&f.facts(), &f.ui(), header, RowAction::Press(0)), [Effect::SetExpanded("Enlarger", false)]);
    let curves = id_of(group("Film Curves"));
    assert_eq!(dispatch(&f.facts(), &f.ui(), curves, RowAction::Press(0)), [Effect::SetExpanded("Film Curves", true)]);
    // Scanning skips the print: struck and closed, its controls off.
    f.set("io.scan_film", json!(true));
    let view = f.view();
    let enlarger = row_of(&view, group("Enlarger"));
    assert!(enlarger.skipped && !enlarger.open && enlarger.status == "skipped while scanning film");
    assert!(row_of(&view, |k| *k == Kind::Section("Print")).skipped);
    assert!(!shows(&f, slider("enlarger.print_exposure")));
    assert!(shows(&f, |k| *k == Kind::Footer("Printing and the paper are skipped while scanning film directly.")));
}

#[test]
fn summaries_follow_the_first_control_and_the_locale() {
    let mut f = full();
    f.tab = Tab::Lab;
    f.decimal = ',';
    f.set("enlarger.print_exposure", json!(1.25));
    f.expanded = String::new();
    let view = f.view();
    assert_eq!(row_of(&view, group("Enlarger")).summary, "1,25");
    f.set("enlarger.print_exposure", json!(2.0));
    assert_eq!(row_of(&f.view(), group("Enlarger")).summary, "2");
    // A module switched off: its controls are off, its header says so.
    f.set("film_render.grain.active", json!(false));
    f.all_open = true;
    let view = f.view();
    let grain = row_of(&view, group("Grain"));
    assert!(grain.off && !grain.checked && grain.changed && grain.switch_enabled);
    assert!(!row_of(&view, slider("film_render.grain.rms_granularity.0")).enabled);
    let switch = id_of(group("Grain"));
    assert_eq!(dispatch(&f.facts(), &f.ui(), switch, RowAction::Check(true)), [Effect::Set("film_render.grain.active", Value::Bool(true))]);
    // Sliders read with the locale's separator.
    assert_eq!(row_of(&view, slider("enlarger.print_exposure")).value, "2", "whole numbers drop their decimals");
    f.set("enlarger.print_exposure", json!(1.25));
    assert_eq!(row_of(&f.view(), slider("enlarger.print_exposure")).value, "1,25");
}

#[test]
fn counts_and_the_pane_header() {
    let mut f = full();
    let view = f.view();
    // A fresh model differs from new photos' brightness match.
    assert_eq!((view.counts, view.total), ([1, 0, 0], 1));
    f.set("prep.highlights", json!(12.0));
    f.set("enlarger.print_exposure", json!(1.5));
    f.state.border.enabled = true;
    let view = f.view();
    assert_eq!((view.counts, view.total), ([2, 1, 1], 4));
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        assert_eq!(edits::edits(tab, &f.facts(), '.').len(), view.counts[i]);
    }
    f.has_source = false;
    assert_eq!(f.view().total, 0);
    // The edit list: each edit back alone, or the whole tab, one step each.
    let list = edits::edits(Tab::Prep, &f.facts(), '.');
    assert_eq!(command::edits_head(Tab::Prep, list.len()), "2 edits in Prep");
    assert_eq!(command::edits_head(Tab::Output, 1), "1 edit in Output");
    let all = edit_list(Tab::Prep, &list, EditChoice::All).unwrap();
    assert_eq!(all, Effect::ResetEdits(list.iter().map(|e| e.target).collect(), "Reset Prep".into()));
    let i = list.iter().position(|e| e.target == Target::Param("prep.highlights")).unwrap();
    let title = list[i].title.clone();
    assert_eq!(edit_list(Tab::Prep, &list, EditChoice::One(i)), Some(Effect::ResetEdits(vec![Target::Param("prep.highlights")], format!("Reset {title}"))));
    assert_eq!(edit_list(Tab::Prep, &list, EditChoice::One(99)), None);
    assert_eq!(edit_list(Tab::Prep, &[], EditChoice::All), None);
}

#[test]
fn output_rows_follow_the_size_and_the_border() {
    let mut f = full();
    f.tab = Tab::Output;
    f.photo_size = Size::new(4000.0, 2000.0);
    f.source_size = (4000, 2000);
    let view = f.view();
    assert_eq!(view.export_summary, "4000 × 2000 · PNG 8-bit · sRGB");
    assert!(view.can_export);
    assert_eq!(row_of(&view, |k| *k == Kind::Photograph).value, "4000 × 2000 · 8.0 MP");
    assert!(!shows(&f, |k| *k == Kind::LongestSide));
    // The border's rows are off until it's on; 35mm doesn't fit 2:1.
    let styles = row_of(&view, |k| *k == Kind::BorderStyles);
    assert!(styles.buttons.iter().all(|on| !on));
    f.state.border.enabled = true;
    let view = f.view();
    let styles = row_of(&view, |k| *k == Kind::BorderStyles);
    assert_eq!(styles.buttons, [true, true, true, false]);
    let warning = row_of(&view, |k| *k == Kind::Footer(""));
    assert!(warning.warn && warning.hint.starts_with("35mm needs a 3:2 or 3:4 photograph. This one is "), "{}", warning.hint);
    let color = row_of(&view, |k| *k == Kind::Color(false));
    assert_eq!((color.summary.as_str(), color.changed, color.color), ("White", false, Some(BorderColor::WHITE)));
    assert!(!shows(&f, |k| *k == Kind::Color(true)));
    f.state.border.style = BorderStyle::Keyline;
    f.state.border.line_color = BorderColor([0x12, 0xab, 0xff]);
    let view = f.view();
    let line = row_of(&view, |k| *k == Kind::Color(true));
    assert_eq!((line.summary.as_str(), line.changed), ("#12ABFF", true));
    assert_eq!(view.swatches, Swatches { color: BorderColor::WHITE, line_color: BorderColor([0x12, 0xab, 0xff]), style: 2 });
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::Color(true)), RowAction::Press(0)), [Effect::ChooseBorderColor { line: true, current: BorderColor([0x12, 0xab, 0xff]) }]);
    // Longest side and its error.
    f.set(state::SIZE_TARGET, json!(3000));
    f.size_error = Some("Too small.".into());
    let view = f.view();
    let longest = row_of(&view, |k| *k == Kind::LongestSide);
    assert_eq!((longest.value.as_str(), longest.keep_typed), ("3000", true));
    assert_eq!(row_of(&view, |k| *k == Kind::Note(Note::Size)).hint, "Too small.");
    assert_eq!(row_of(&view, |k| *k == Kind::SizeMode).selected, Some(1));
    f.size_error = None;
    f.set(state::SIZE_TARGET, Value::Null);
    f.image_shown = true;
    f.preview_scale = 0.5;
    assert_eq!(row_of(&f.view(), |k| *k == Kind::Note(Note::Size)).hint, "The preview shows 50% of these pixels. Exports develop all of them; inspect grain in an exported file.");
    // The export foot follows the last save.
    f.output_format = "jpeg".into();
    f.jpeg_quality = 120;
    f.color_space = "ProPhoto RGB".into();
    assert_eq!(f.view().export_summary, "4000 × 2000 · JPEG 100 · Adobe RGB (1998)");
}

#[test]
fn lists_select_their_value() {
    let mut f = full();
    f.tab = Tab::Lab;
    f.lab_all_controls = true;
    f.all_open = true;
    f.stocks[0].times = vec![6.0, 8.0];
    js::set(&mut f.defaults, state::FILM_FORMAT.path(), json!(35.0));
    f.set(state::FILM_FORMAT.path(), json!(35.0));
    let view = f.view();
    let dev = row_of(&view, |k| *k == Kind::DevTime(false));
    assert_eq!(dev.options, ["Profile default", "6 min", "8 min"]);
    assert_eq!(dev.selected, Some(0));
    assert_eq!(view.dev_times, [vec![6.0, 8.0], vec![]]);
    assert!(!shows(&f, |k| *k == Kind::DevTime(true)), "one time: no choice");
    let id = id_of(|k| *k == Kind::DevTime(false));
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Select(2)), [Effect::Set(edits::FILM_DEVELOPMENT, json!(8))]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Select(0)), [Effect::Set(edits::FILM_DEVELOPMENT, Value::Null)]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Select(-1)), [Effect::Set(edits::FILM_DEVELOPMENT, Value::Null)]);
    f.set(edits::FILM_DEVELOPMENT, json!(8.0));
    let dev = row_of(&f.view(), |k| *k == Kind::DevTime(false));
    assert_eq!((dev.selected, dev.changed), (Some(2), true));
    // Film format: a stills format, or Custom… and the long edge.
    let format = row_of(&view, |k| *k == Kind::FilmFormat);
    assert_eq!(format.selected, Some(2));
    assert!(!shows(&f, slider(state::FILM_FORMAT.path())));
    let id = id_of(|k| *k == Kind::FilmFormat);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Select(state::FILM_FORMATS.len() as isize)), [Effect::ChooseFilmFormat(None)]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Select(0)), [Effect::ChooseFilmFormat(Some(0))]);
    f.set(state::FILM_FORMAT.path(), json!(50.0));
    assert!(shows(&f, slider(state::FILM_FORMAT.path())));
    assert_eq!(row_of(&f.view(), |k| *k == Kind::FilmFormat).selected, Some(state::FILM_FORMATS.len()));
    // The preview's space, Shifts move, the tone graph's channel.
    f.color_space = "Display P3".into();
    let view = f.view();
    let space = row_of(&view, |k| *k == Kind::PreviewSpace);
    assert_eq!((space.selected, space.changed), (Some(1), true));
    assert!(row_of(&view, group("Output")).changed);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::PreviewSpace), RowAction::Select(3)), [Effect::ChoosePreviewSpace("ProPhoto RGB")]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::PreviewSpace), RowAction::Select(-1)), []);
    f.filter_shift_link = "together".into();
    let link = id_of(|k| matches!(k, Kind::FilterLink(_)));
    assert_eq!(row_of(&f.view(), |k| matches!(k, Kind::FilterLink(_))).selected, Some(1));
    assert_eq!(dispatch(&f.facts(), &f.ui(), link, RowAction::Select(2)), [Effect::SetFilterLink(state::FilterLink::Opposite)]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), link, RowAction::Select(-1)), []);
    // Linked shifts move together, and reset together.
    let magenta = id_of(slider(state::FilterLink::MAGENTA));
    assert_eq!(dispatch(&f.facts(), &f.ui(), magenta, RowAction::Set(12.34)), [Effect::SetFilterShift(state::FilterLink::MAGENTA, 12.3)]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), magenta, RowAction::Reset { double_click: true }), [Effect::ResetFilterShiftAfterClick(state::FilterLink::MAGENTA)]);
    f.tone_graph_channel = "g".into();
    let graph = row_of(&f.view(), |k| *k == Kind::ToneGraph);
    assert_eq!(graph.selected, Some(1));
    assert!(f.view().tone_graph);
    let id = id_of(|k| *k == Kind::ToneGraph);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Press(3)), [Effect::ToneGraphChannel("b")]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Press(6)), []);
}

#[test]
fn colour_filters_and_bw_films() {
    let mut f = full();
    f.tab = Tab::Lab;
    f.filters = vec![state::ColorFilter { key: "red".into(), name: "Red".into(), glass: "25".into(), colour: 0xff0000 }];
    assert!(!shows(&f, group("Color Filter")), "colour film without a filter");
    f.state.film = "trix".into();
    f.catalog = json!({"profiles": [{"id": "trix", "color_filter_stops": {"red": 3.0}}]});
    f.all_open = true;
    let view = f.view();
    let header = row_of(&view, group("Color Filter"));
    assert!(header.off && header.summary == "Off" && !header.checked);
    // B&W: one grain lane, and the sliders' B&W words.
    assert!(!shows(&f, slider("film_render.grain.rms_granularity.1")));
    f.set(state::ColorFilter::PATH, json!("red"));
    let view = f.view();
    let header = row_of(&view, group("Color Filter"));
    assert!(!header.off && header.summary == "Red" && header.changed && header.checked);
    let picker = row_of(&view, |k| *k == Kind::ColorFilter);
    assert!(picker.hint.contains("Takes 3 stops on Tri-X 400"), "{}", picker.hint);
    assert_eq!(picker.warning, "");
    let id = id_of(group("Color Filter"));
    assert_eq!(dispatch(&f.facts(), &f.ui(), id, RowAction::Check(false)), [Effect::SwitchColorFilter(false)]);
    // On colour film the filter warns.
    f.state.film = "portra".into();
    assert!(row_of(&f.view(), |k| *k == Kind::ColorFilter).warning.starts_with("Taking filters are made for black-and-white film."));
}

/// Every slider, toggle and list, on every tab and file, changed by its
/// control and put back by its reset, leaves the edit as it was.
#[test]
fn every_action_is_undone_by_its_reset() {
    for raw in [false, true] {
        for tab in Tab::ALL {
            let mut f = full();
            f.is_raw = raw;
            f.lens_supported = true;
            f.tab = tab;
            f.all_open = true;
            f.lab_all_controls = true;
            f.set("io.raw_white_balance.mode", json!("custom"));
            f.set("io.input_interpretation.mode", json!("assume"));
            f.defaults = f.state.params.clone();
            f.state.border.enabled = true;
            f.state.border.style = BorderStyle::Keyline;
            let initial = f.state.clone();
            let before = f.view();
            let mut tried = 0;
            for row in before.rows.iter().filter(|r| r.visible) {
                let kind = row.kind.unwrap();
                let action = match kind {
                    Kind::Slider(Adjustment::Slider { path, low, high, .. }) if state::grain_lane(path).is_none() => RowAction::Set(low + (high - low) * 0.37),
                    Kind::ExposureOffset => RowAction::Set(1.0),
                    Kind::BorderSlider(_, field) => RowAction::Set(field.range().1 * 0.5),
                    Kind::Toggle(..) => RowAction::Check(!row.checked),
                    Kind::Choice(_, _, options) if options.len() > 1 => RowAction::Select(if row.selected == Some(1) { 0 } else { 1 }),
                    _ => continue,
                };
                let id = row.id;
                let effects = dispatch(&f.facts(), &f.ui(), id, action);
                apply(&mut f, effects);
                assert!(command::resettable(&f.facts(), id), "{kind:?} on {tab:?}");
                assert!(f.view().rows[id].changed || !f.view().rows[id].visible, "{kind:?}");
                let effects = dispatch(&f.facts(), &f.ui(), id, RowAction::Reset { double_click: false });
                apply(&mut f, effects);
                assert!(js::same(&f.state.params, &initial.params) && f.state.border == initial.border, "{kind:?} on {tab:?}");
                assert!(!command::resettable(&f.facts(), id), "{kind:?}");
                tried += 1;
            }
            assert_eq!(f.view(), before);
            assert!(tried >= if tab == Tab::Output { 2 } else { 3 }, "{tab:?}: {tried}");
        }
    }
    // Border switches have no reset: switching back is the way.
    let mut f = full();
    f.tab = Tab::Output;
    for which in 0..3u8 {
        let id = id_of(|k| matches!(k, Kind::BorderToggle(_, w) if *w == which));
        assert!(!command::has_reset(id));
        let effects = dispatch(&f.facts(), &f.ui(), id, RowAction::Check(true));
        apply(&mut f, effects);
        let effects = dispatch(&f.facts(), &f.ui(), id, RowAction::Check(false));
        apply(&mut f, effects);
    }
    assert_eq!(f.state.border, BorderSettings { enabled: false, instagram: false, export: false, ..f.state.border });
    let style = id_of(|k| *k == Kind::BorderStyles);
    assert_eq!(dispatch(&f.facts(), &f.ui(), style, RowAction::Press(1)), [Effect::Border(BorderChange::Style(BorderStyle::Film), command::CHANGE_BORDER)]);
}

/// A trackbar's drag is one undo step with live frames: begin, values,
/// then the end with its name.
#[test]
fn trackbars_drag_in_one_step() {
    let mut f = full();
    let id = id_of(slider("prep.highlights"));
    let low = match rows()[id].kind {
        Kind::Slider(Adjustment::Slider { low, .. }) => low,
        _ => unreachable!(),
    };
    let ui = f.ui();
    assert_eq!(dispatch(&f.facts(), &ui, id, RowAction::Track(120, Track::Drag)), [Effect::BeginInteraction, Effect::Set("prep.highlights", js::num(state::snap(low + 120.0, -100.0, 100.0, 1.0)))]);
    assert_eq!(dispatch(&f.facts(), &ui, id, RowAction::Track(120, Track::End)), [Effect::EndInteraction(command::CHANGE_ADJUSTMENT)]);
    assert_eq!(dispatch(&f.facts(), &ui, id, RowAction::Track(10, Track::Step)).len(), 1);
    f.tab = Tab::Output;
    let width = id_of(|k| matches!(k, Kind::BorderSlider(_, BorderWidth::Line)));
    assert_eq!(dispatch(&f.facts(), &f.ui(), width, RowAction::Track(3, Track::End)), [Effect::EndInteraction(command::CHANGE_BORDER)]);
    assert_eq!(dispatch(&f.facts(), &f.ui(), width, RowAction::Track(3, Track::Step)), [Effect::SetBorderPercent(BorderWidth::Line, 0.05 + 3.0 * 0.05)]);
    // The Exposure's track starts at the match's low end.
    f.state.brightness_match = Some(state::BrightnessMatch { ev: Some(1.0), inputs: Some(state::match_inputs(&f.state)), ..Default::default() });
    let exposure = id_of(|k| *k == Kind::ExposureOffset);
    assert_eq!(dispatch(&f.facts(), &f.ui(), exposure, RowAction::Track(0, Track::Step)), [Effect::SetExposureOffset(-4.0)]);
    // Rows without a slider ignore a track.
    assert_eq!(dispatch(&f.facts(), &f.ui(), id_of(|k| *k == Kind::SizeMode), RowAction::Track(1, Track::Step)), []);
}

#[test]
fn rows_are_fixed_and_say_their_help() {
    let rows = rows();
    assert!(std::ptr::eq(rows, super::rows()), "built once");
    // Every group's control has a row of its module.
    for (index, group) in state::ADJUSTMENT_GROUPS.iter().enumerate() {
        let header = rows.iter().position(|r| r.when == When::Group(index)).unwrap();
        assert_eq!(rows.iter().filter(|r| r.group == Some(header) && r.basic.is_some()).count(), group.controls.len(), "{}", group.title);
    }
    assert_eq!(Kind::FilmFormat.options().last().map(String::as_str), Some("Custom…"));
    assert_eq!(Kind::SizeMode.options(), ["Original", "Longest side"]);
    assert_eq!(Kind::DevTime(true).label(), "Print development");
    assert_eq!(Kind::Toggle("Scan film directly", "io.scan_film").help(), Some("Skip printing and scan the developed film, as for slides."));
    assert!(Kind::More(More::All).help().is_some() && Kind::Note(Note::Size).help().is_none());
    assert!(Kind::Footer("x").quiet() && Kind::ToneGraph.quiet() && !Kind::Section("x").quiet());
    assert_eq!(Tab::from_title("Output"), Tab::Output);
    assert_eq!(Tab::from_title("Anything"), Tab::Prep);
    assert_eq!(schema::tone_channel("rgb"), None);
    assert_eq!(schema::tone_channel("b"), Some(2));
    // Rows not shown carry nothing but their kind.
    let f = full();
    let hidden = f.view().rows.into_iter().find(|r| !r.visible).unwrap();
    assert_eq!(hidden, RowView { id: hidden.id, kind: hidden.kind, ..Default::default() });
    // Stocks the fixture doesn't have give no times.
    let mut g = full();
    g.stocks = vec![stock("other", "Other", false, false)];
    assert!(g.facts().stock_times(false).is_empty() && !g.facts().has_catalog() == g.stocks.is_empty());
}
