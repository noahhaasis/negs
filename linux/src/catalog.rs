//! Live helper metadata, with a repository snapshot for isolated UI tests.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Stock {
    pub id: String,
    pub name: String,
    pub paper: bool,
    pub positive: bool,
    pub bw: bool,
    #[serde(default)]
    pub group: String,
    #[serde(default, alias = "target_print", deserialize_with = "nullable_string")]
    pub target: String,
    #[serde(alias = "development_times")]
    pub times: Vec<f64>,
    /// The resolved grain preset, per field of `GRAIN_STOCK_FIELDS`.
    #[serde(default)]
    pub grain: serde_json::Value,
}
#[derive(Deserialize)]
struct Catalog {
    profiles: Vec<Stock>,
    /// The settings a preset leaves as they are; missing from older helpers.
    #[serde(default)]
    preset_kept: Option<Vec<String>>,
}
fn nullable_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
static LIVE: OnceLock<Catalog> = OnceLock::new();
pub fn install(value: &serde_json::Value) -> Result<(), String> {
    let mut catalog: Catalog = serde_json::from_value(value.clone())
        .map_err(|e| format!("Invalid engine catalog: {e}"))?;
    if !catalog.profiles.iter().any(|s| !s.paper)
        || !catalog.profiles.iter().any(|s| s.paper)
        || catalog.profiles.len() > 1024
        || catalog.profiles.iter().any(|s| {
            s.id.is_empty()
                || s.id.len() > 256
                || s.name.is_empty()
                || s.times.iter().any(|t| !t.is_finite() || *t <= 0.)
        })
    {
        return Err("The engine catalog is incomplete or invalid.".into());
    }
    let cinema: Vec<_> = catalog
        .profiles
        .iter()
        .filter(|s| s.paper && (s.name.contains("Vision") || s.name.contains("Print Film")))
        .map(|s| s.id.clone())
        .collect();
    for stock in &mut catalog.profiles {
        stock.group = if stock.bw {
            "Black & White"
        } else if stock.paper {
            if cinema.contains(&stock.id) {
                "Motion Picture Print"
            } else {
                "Color Paper"
            }
        } else if stock.positive {
            "Color Slide"
        } else if cinema.contains(&stock.target) {
            "Motion Picture"
        } else {
            "Color Negative"
        }
        .into();
    }
    // One helper/data identity per process. A connection restart retains this
    // same immutable metadata; the next application launch discovers updates.
    let _ = LIVE.set(catalog);
    Ok(())
}
/// The settings a preset leaves as they are (dotted paths), or None when
/// the helper doesn't list them.
pub fn preset_kept() -> Option<&'static [String]> {
    LIVE.get()?.preset_kept.as_deref()
}
pub fn stocks(paper: bool) -> Vec<&'static Stock> {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    let mut stocks: Vec<_> = LIVE
        .get()
        .unwrap_or_else(|| {
            CATALOG.get_or_init(|| {
                serde_json::from_str(include_str!("../catalog.json"))
                    .expect("checked catalog snapshot")
            })
        })
        .profiles
        .iter()
        .filter(|s| s.paper == paper)
        .collect();
    // Match the macOS menu sections while retaining catalog order within each.
    stocks.sort_by_key(|stock| group_order(&stock.group));
    stocks
}
pub fn group_order(group: &str) -> usize {
    match group {
        "Color Negative" | "Color Paper" => 0,
        "Color Slide" | "Motion Picture Print" => 1,
        "Motion Picture" => 2,
        "Black & White" => 3,
        _ => 4,
    }
}

/// Film search follows the native catalog groups and matches every word,
/// ignoring case and accents. IDs are searchable as ordinary spaced names.
pub fn matches(stock: &Stock, query: &str) -> bool {
    fn fold(text: &str) -> String {
        use gtk::glib::Unichar;
        gtk::glib::normalize(gtk::glib::casefold(text), gtk::glib::NormalizeMode::All)
            .chars()
            .filter(|c| !c.is_mark())
            .collect()
    }
    let text = fold(&format!(
        "{} {} {}",
        stock.name,
        stock.id.replace('_', " "),
        stock.group
    ));
    fold(query)
        .split_whitespace()
        .all(|word| text.contains(word))
}
pub fn selected(state: &crate::model::State, paper: bool) -> &'static Stock {
    let key = if paper { "Paper" } else { "Film" };
    let stocks = stocks(paper);
    let id = state.choices.get(&format!("{key} ID"));
    // Old sessions stored a menu index; new edits start on `EditState`'s
    // stocks in `Parameters.swift`.
    let old = match state.choices.get(key).and_then(|s| s.parse::<usize>().ok()) {
        Some(legacy) if paper => [
            "kodak_portra_endura",
            "kodak_ultra_endura",
            "fujifilm_crystal_archive_typeii",
            "ilford_mgrc_deluxe_grade2",
        ]
        .get(legacy)
        .copied(),
        Some(legacy) => crate::border::FILM_IDS.get(legacy).copied(),
        None if paper => Some("kodak_portra_endura"),
        None => Some("kodak_gold_200"),
    };
    stocks
        .iter()
        .find(|s| Some(s.id.as_str()) == id.map(String::as_str).or(old))
        .copied()
        .unwrap_or(stocks[0])
}
/// Choosing a film or paper in the inspector: the stock's own development
/// time, and for a film its grain, DIR and halation presets (as upstream
/// re-applies them on a stock change), its scan mode and target paper —
/// unless `keep_paper` (the Keep paper when changing film setting).
pub fn choose(state: &mut crate::model::State, stock: &Stock, keep_paper: bool) {
    let paper = stock.paper;
    state.choices.insert(
        if paper { "Paper ID" } else { "Film ID" }.into(),
        stock.id.clone(),
    );
    state.choices.remove(if paper {
        "Print development"
    } else {
        "Development"
    });
    if !paper {
        crate::parameters::clear_grain_lanes(state);
        for path in crate::parameters::STOCK_PRESET_PATHS {
            crate::parameters::set(&mut state.params, path, serde_json::Value::Null);
        }
        state
            .choices
            .insert("Scan film directly".into(), stock.positive.to_string());
        if !keep_paper && !stock.target.is_empty() {
            state
                .choices
                .insert("Paper ID".into(), stock.target.clone());
            state.choices.remove("Print development");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn film_search_matches_all_words_in_names_ids_and_groups() {
        let films = super::stocks(false);
        let ids = |query: &str| {
            films
                .iter()
                .filter(|s| super::matches(s, query))
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(""), ids("  \t "));
        assert_eq!(ids("GOLD 200"), vec!["kodak_gold_200"]);
        assert_eq!(ids("KÓDAK góld"), vec!["kodak_gold_200"]);
        assert!(ids("gold 400").is_empty());
        assert!(ids("black 400").contains(&"kodak_trix400_d76_8min"));
        assert!(ids("slide velvia").iter().all(|id| id.contains("velvia")));
        assert_eq!(ids("kodak gold"), ids("GOLD KODAK"));
        assert!(ids("gold\nnegative").contains(&"kodak_gold_200"));
    }
    #[test]
    fn live_profiles_accept_no_target_paper() {
        let stock:super::Stock=serde_json::from_value(serde_json::json!({"id":"slide","name":"Slide","paper":false,"positive":true,"bw":false,"target_print":null,"development_times":[]})).unwrap();
        assert!(stock.target.is_empty());
    }
    #[test]
    fn complete_catalog_and_pairing_metadata() {
        let films = super::stocks(false);
        let papers = super::stocks(true);
        // 23, plus the 24 colour and 5 B&W films ported from Fotufilm.
        assert_eq!(films.len(), 52);
        assert_eq!(papers.len(), 10);
        assert!(
            films
                .iter()
                .any(|s| s.id == "kodak_doublex" && s.times.len() == 5)
        );
        for film in films {
            assert!(film.target.is_empty() || papers.iter().any(|s| s.id == film.target));
        }
    }
    #[test]
    fn keep_paper_leaves_the_paper_alone() {
        let film = super::stocks(false)
            .into_iter()
            .find(|s| !s.target.is_empty())
            .unwrap();
        let other = super::stocks(true)
            .into_iter()
            .find(|s| s.id != film.target)
            .unwrap();
        for keep in [false, true] {
            let mut state = crate::model::State::default();
            state.choices.insert("Paper ID".into(), other.id.clone());
            state.choices.insert("Print development".into(), "2".into());
            super::choose(&mut state, film, keep);
            let paper = if keep { &other.id } else { &film.target };
            assert_eq!(state.choices.get("Paper ID"), Some(paper));
            assert_eq!(state.choices.contains_key("Print development"), keep);
        }
    }
}
