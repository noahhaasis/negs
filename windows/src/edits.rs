//! What each inspector tab holds (`TabEdits.swift`, `InspectorEdits`): the
//! controls that differ from their defaults, in the order the tab shows
//! them, for the count beside each tab, the pane header's "3 edits", and the
//! list a click on a count opens (each edit can go back alone, or the whole
//! tab at once).
//!
//! The shared core's (core/src/state/edits, through `negs_core_rules::edits`):
//! the core asks `InspectorFacts` what it needs for the call, so nothing is
//! converted. This module keeps its API and tests.

use negs_core_rules::edits as core;
use serde_json::Value;

use crate::inspector_view::InspectorFacts;
use crate::state::Section;

#[allow(unused_imports)]
pub use negs_core_rules::edits::{
    Edit, FILM_DEVELOPMENT, INPUT_PATHS, MoreAction, PRINT_DEVELOPMENT, Target, WHITE_BALANCE_PATHS, section_key, section_title, words,
};

/// The inspector's tabs. Windows' own type (inspector_view adds to it); the
/// core's by place.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tab {
    Prep,
    Lab,
    Output,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Prep, Tab::Lab, Tab::Output];

    pub fn title(self) -> &'static str {
        self.core().title()
    }

    fn core(self) -> core::Tab {
        core::Tab::ALL[self as usize]
    }
}
#[cfg(test)]
use crate::json;
#[cfg(test)]
use crate::state::ADJUSTMENT_GROUPS;
#[cfg(test)]
use negs_core_rules::edits::Format;

impl core::Facts for InspectorFacts<'_> {
    fn value(&self, path: &str) -> &Value {
        InspectorFacts::value(self, path)
    }
    fn is_modified(&self, path: &str) -> bool {
        InspectorFacts::is_modified(self, path)
    }
    fn film_is_bw(&self) -> bool {
        InspectorFacts::film_is_bw(self)
    }
    fn film_positive(&self) -> bool {
        self.stocks().iter().find(|s| !s.paper && s.id == self.state().film).is_some_and(|s| s.positive)
    }
    fn border_enabled(&self) -> bool {
        self.state().border.enabled
    }
    fn matches_brightness(&self) -> bool {
        InspectorFacts::matches_brightness(self)
    }
    fn exposure_offset(&self) -> f64 {
        InspectorFacts::exposure_offset(self)
    }
    fn exposure_offset_modified(&self) -> bool {
        InspectorFacts::exposure_offset_modified(self)
    }
    fn chosen_color_filter_name(&self) -> Option<&str> {
        self.chosen_color_filter().map(|f| f.name.as_str())
    }
    fn prefs(&self) -> core::Prefs<'_> {
        core::Prefs {
            lab_all_controls: self.prefs.lab_all_controls,
            more_adjustments: self.prefs.more_adjustments,
            match_new_photos: self.prefs.match_new_photos,
            color_space: self.prefs.color_space,
        }
    }
    fn is_raw(&self) -> bool {
        self.is_raw
    }
    fn lens_supported(&self) -> bool {
        self.lens_supported
    }
}

/// A module's count: its switch, then its controls.
pub fn group_count(index: usize, facts: &InspectorFacts) -> usize {
    core::group_count(index, facts)
}

#[allow(dead_code)] // edits.rs's API; the core uses its own.
pub fn scan_film_modified(facts: &InspectorFacts) -> bool {
    core::scan_film_modified(facts)
}

/// The Lab's Output module: the preview's colour space and its curve.
#[allow(dead_code)] // edits.rs's API; the core uses its own.
pub fn output_count(facts: &InspectorFacts) -> usize {
    core::output_count(facts)
}

pub fn section_count(section: Section, facts: &InspectorFacts) -> usize {
    core::section_count(section, facts)
}

#[allow(dead_code)] // edits.rs's API; the core uses its own.
pub fn brightness_count(facts: &InspectorFacts) -> usize {
    core::brightness_count(facts)
}

pub fn count(tab: Tab, facts: &InspectorFacts) -> usize {
    core::count(tab.core(), facts)
}

/// The controls on `tab` that are off their defaults, in the tab's order.
/// Always as many as `count(tab, facts)`.
pub fn edits(tab: Tab, facts: &InspectorFacts, decimal: char) -> Vec<Edit> {
    core::edits(tab.core(), facts, decimal)
}

/// Whether a section's extra modules and rows show.
pub fn shows_section(facts: &InspectorFacts, section: Section) -> bool {
    core::shows_section(facts, section)
}

/// A module shows when it has basics, its section's extras show, or something in it changed.
pub fn group_shows(facts: &InspectorFacts, index: usize) -> bool {
    core::group_shows(facts, index)
}

/// Whether a module's control at `place` shows: a basic, revealed, or changed.
pub fn control_shows(facts: &InspectorFacts, index: usize, place: usize) -> bool {
    core::control_shows(facts, index, place)
}

/// A module's "N more controls…", or once clicked "Hide N controls".
pub fn module_more(facts: &InspectorFacts, index: usize) -> Option<(String, MoreAction)> {
    core::module_more(facts, index)
}

/// A section's "2 more: Scanner, Color Management…" for the modules and
/// rows it keeps back; once clicked, "Hide Scanner, Color Management".
pub fn section_more(facts: &InspectorFacts, section: Section) -> Option<(String, MoreAction)> {
    core::section_more(facts, section)
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
