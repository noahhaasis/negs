//! Which photographs the strip and grid show, and the words for it
//! (`LibraryFilter`, `SheetWords`, `FilterMenu`).

use crate::item::Summary;
use crate::json::Json;
use crate::sidecar::Flag;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RatingFilter {
    #[default]
    Any,
    /// At least this many stars, 1 to 5.
    AtLeast(u8),
    Unrated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlagFilter {
    Pick,
    Unflagged,
    Reject,
}

impl FlagFilter {
    /// In the menu's order, with its words.
    pub const CHOICES: [(FlagFilter, &'static str); 3] = [(FlagFilter::Pick, "Picks"), (FlagFilter::Unflagged, "Unflagged"), (FlagFilter::Reject, "Rejected")];

    fn key(self) -> &'static str {
        match self {
            FlagFilter::Pick => "pick",
            FlagFilter::Unflagged => "unflagged",
            FlagFilter::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EditsFilter {
    #[default]
    Any,
    Edited,
    Unedited,
}

/// Remembered for the app, not per folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub rating: RatingFilter,
    /// The flags shown; empty shows every one.
    pub flags: BTreeSet<FlagFilter>,
    pub edits: EditsFilter,
}

/// A part of the sheet's sentence: words, then an underlined one that
/// opens the filter menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterWord {
    pub lead: String,
    pub word: String,
}

impl Filter {
    pub fn active(&self) -> bool {
        *self != Filter::default()
    }

    pub fn includes(&self, summary: &Summary) -> bool {
        match self.rating {
            RatingFilter::Any => {}
            RatingFilter::AtLeast(stars) => {
                if summary.rating < stars {
                    return false;
                }
            }
            RatingFilter::Unrated => {
                if summary.rating != 0 {
                    return false;
                }
            }
        }
        if !self.flags.is_empty() {
            let flag = match summary.flag {
                Some(Flag::Pick) => FlagFilter::Pick,
                Some(Flag::Reject) => FlagFilter::Reject,
                None => FlagFilter::Unflagged,
            };
            if !self.flags.contains(&flag) {
                return false;
            }
        }
        match self.edits {
            EditsFilter::Any => true,
            EditsFilter::Edited => summary.edited,
            EditsFilter::Unedited => !summary.edited,
        }
    }

    /// The menu's flag items turn on and off.
    pub fn toggle_flag(&mut self, flag: FlagFilter) {
        if !self.flags.remove(&flag) {
            self.flags.insert(flag);
        }
    }

    /// Edited and Unedited exclude each other; choosing the one that's on
    /// turns it off.
    pub fn toggle_edits(&mut self, choice: EditsFilter) {
        self.edits = if self.edits == choice { EditsFilter::Any } else { choice };
    }

    /// What the filter lets through, as the sheet's sentence says it:
    /// "rated *★★★ or more*", "*picks or rejected*", "*edited*".
    pub fn words(&self) -> Vec<FilterWord> {
        let mut parts = Vec::new();
        let part = |lead: &str, word: String| FilterWord { lead: lead.into(), word };
        match self.rating {
            RatingFilter::Any => {}
            RatingFilter::AtLeast(5) => parts.push(part("rated ", "★★★★★".into())),
            RatingFilter::AtLeast(stars) => parts.push(part("rated ", format!("{} or more", "★".repeat(stars as usize)))),
            RatingFilter::Unrated => parts.push(part("", "unrated".into())),
        }
        let flags: Vec<String> = FlagFilter::CHOICES.iter().filter(|(f, _)| self.flags.contains(f)).map(|(_, t)| t.to_lowercase()).collect();
        if !flags.is_empty() {
            parts.push(part("", flags.join(" or ")));
        }
        match self.edits {
            EditsFilter::Any => {}
            EditsFilter::Edited => parts.push(part("", "edited".into())),
            EditsFilter::Unedited => parts.push(part("", "unedited".into())),
        }
        parts
    }

    /// The rating items of the filter menu, in order, with what each sets.
    pub fn rating_choices() -> Vec<(RatingFilter, String)> {
        let mut out = vec![(RatingFilter::Any, "Any Rating".to_string())];
        for stars in 1..=5u8 {
            let title = if stars == 5 { "★★★★★".to_string() } else { format!("≥ {}", "★".repeat(stars as usize)) };
            out.push((RatingFilter::AtLeast(stars), title));
        }
        out.push((RatingFilter::Unrated, "Unrated Only".into()));
        out
    }

    /// As macOS keeps it in its settings (`LibraryFilter`'s Codable form).
    pub fn to_json(&self) -> Json {
        let rating = match self.rating {
            RatingFilter::Any => Json::from_pairs([("any", Json::object())]),
            RatingFilter::AtLeast(n) => Json::from_pairs([("atLeast", Json::from_pairs([("_0", Json::from(n as i64))]))]),
            RatingFilter::Unrated => Json::from_pairs([("unrated", Json::object())]),
        };
        let edits = match self.edits {
            EditsFilter::Any => "any",
            EditsFilter::Edited => "edited",
            EditsFilter::Unedited => "unedited",
        };
        Json::from_pairs([
            ("rating", rating),
            ("flags", Json::Array(self.flags.iter().map(|f| Json::string(f.key())).collect())),
            ("edits", Json::string(edits)),
        ])
    }

    /// Reads `to_json`'s form, and filters saved before Unedited, which had
    /// only `editedOnly`. Anything unreadable is no filter.
    pub fn from_json(json: &Json) -> Filter {
        let mut filter = Filter::default();
        let rating = json.get("rating");
        if rating.as_object().is_some_and(|o| o.contains_key("unrated")) {
            filter.rating = RatingFilter::Unrated;
        } else if let Some(n) = rating.at("atLeast._0").as_integer().filter(|n| (1..=5).contains(n)) {
            filter.rating = RatingFilter::AtLeast(n as u8);
        }
        for flag in json.get("flags").as_array().into_iter().flatten() {
            if let Some((f, _)) = FlagFilter::CHOICES.iter().find(|(f, _)| Some(f.key()) == flag.as_str()) {
                filter.flags.insert(*f);
            }
        }
        filter.edits = match json.get("edits").as_str() {
            Some("edited") => EditsFilter::Edited,
            Some("unedited") => EditsFilter::Unedited,
            Some(_) => EditsFilter::Any,
            None if json.get("editedOnly") == &Json::Bool(true) => EditsFilter::Edited,
            None => EditsFilter::Any,
        };
        filter
    }
}

/// "1 photograph", "12 photographs".
pub fn photographs(count: usize) -> String {
    if count == 1 { "1 photograph".into() } else { format!("{count} photographs") }
}

/// The grid foot's count: "12 photographs · 1 selected", or "4 of 12
/// photographs" while filtered.
pub fn count_words(shown: usize, total: usize, selected: usize, filtered: bool) -> String {
    if filtered {
        return format!("{shown} of {}", photographs(total));
    }
    if selected > 0 { format!("{} · {selected} selected", photographs(total)) } else { photographs(total) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_rules() {
        let unrated = Summary::default();
        let three = Summary { rating: 3, ..Summary::default() };
        let five_pick = Summary { rating: 5, flag: Some(Flag::Pick), ..Summary::default() };
        let reject = Summary { rating: 4, flag: Some(Flag::Reject), ..Summary::default() };
        let edited = Summary { edited: true, ..Summary::default() };
        let all = [&unrated, &three, &five_pick, &reject, &edited];
        let mut filter = Filter::default();
        assert!(!filter.active());
        assert!(all.iter().all(|s| filter.includes(s)), "everything shows by default, rejects too");
        filter.rating = RatingFilter::AtLeast(3);
        assert_eq!(all.map(|s| filter.includes(s)), [false, true, true, true, false]);
        filter.rating = RatingFilter::AtLeast(5);
        assert_eq!(all.map(|s| filter.includes(s)), [false, false, true, false, false]);
        filter.rating = RatingFilter::Unrated;
        assert_eq!(all.map(|s| filter.includes(s)), [true, false, false, false, true]);
        let filter = Filter { flags: BTreeSet::from([FlagFilter::Pick, FlagFilter::Unflagged]), ..Filter::default() };
        assert_eq!(all.map(|s| filter.includes(s)), [true, true, true, false, true]);
        let filter = Filter { edits: EditsFilter::Edited, ..Filter::default() };
        assert_eq!(all.map(|s| filter.includes(s)), [false, false, false, false, true]);
        let filter = Filter { edits: EditsFilter::Unedited, ..Filter::default() };
        assert_eq!(all.map(|s| filter.includes(s)), [true, true, true, true, false]);
    }

    #[test]
    fn words_as_the_sheet_says_them() {
        let filter = Filter { rating: RatingFilter::AtLeast(3), flags: BTreeSet::from([FlagFilter::Reject, FlagFilter::Pick]), edits: EditsFilter::Edited };
        let words: Vec<(String, String)> = filter.words().into_iter().map(|w| (w.lead, w.word)).collect();
        assert_eq!(
            words,
            [("rated ".into(), "★★★ or more".into()), ("".into(), "picks or rejected".into()), ("".into(), "edited".into())] as [(String, String); 3]
        );
        assert_eq!(Filter { rating: RatingFilter::AtLeast(5), ..Filter::default() }.words()[0].word, "★★★★★");
        assert_eq!(count_words(4, 12, 0, true), "4 of 12 photographs");
        assert_eq!(count_words(12, 12, 1, false), "12 photographs · 1 selected");
        assert_eq!(count_words(1, 1, 0, false), "1 photograph");
        let titles: Vec<String> = Filter::rating_choices().into_iter().map(|(_, t)| t).collect();
        assert_eq!(titles, ["Any Rating", "≥ ★", "≥ ★★", "≥ ★★★", "≥ ★★★★", "★★★★★", "Unrated Only"]);
    }

    #[test]
    fn the_filter_is_remembered_in_the_macs_form() {
        let filter = Filter { rating: RatingFilter::AtLeast(2), flags: BTreeSet::from([FlagFilter::Reject]), edits: EditsFilter::Unedited };
        assert_eq!(filter.to_json().to_compact_sorted(), r#"{"edits":"unedited","flags":["reject"],"rating":{"atLeast":{"_0":2}}}"#);
        assert_eq!(Filter::from_json(&filter.to_json()), filter);
        let old = Json::parse(br#"{"rating":{"any":{}},"flags":["pick"],"editedOnly":true}"#).unwrap();
        assert_eq!(Filter::from_json(&old), Filter { flags: BTreeSet::from([FlagFilter::Pick]), edits: EditsFilter::Edited, ..Filter::default() });
        let mut toggled = Filter::default();
        toggled.toggle_edits(EditsFilter::Edited);
        toggled.toggle_edits(EditsFilter::Edited);
        toggled.toggle_flag(FlagFilter::Pick);
        toggled.toggle_flag(FlagFilter::Pick);
        assert!(!toggled.active());
    }
}
