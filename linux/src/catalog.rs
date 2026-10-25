//! Snapshot of repository metadata, replaced by the live helper catalog later.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Stock {
    pub id: String,
    pub name: String,
    pub paper: bool,
    pub positive: bool,
    pub bw: bool,
    pub group: String,
    pub target: String,
    pub times: Vec<f64>,
}
#[derive(Deserialize)]
struct Catalog {
    profiles: Vec<Stock>,
}
pub fn stocks(paper: bool) -> Vec<&'static Stock> {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../catalog.json")).expect("checked catalog snapshot")
        })
        .profiles
        .iter()
        .filter(|s| s.paper == paper)
        .collect()
}
pub fn selected(state: &crate::model::State, paper: bool) -> &'static Stock {
    let key = if paper { "Paper" } else { "Film" };
    let stocks = stocks(paper);
    let id = state.choices.get(&format!("{key} ID"));
    let legacy = state
        .choices
        .get(key)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let old = if paper {
        [
            "kodak_portra_endura",
            "kodak_ultra_endura",
            "fujifilm_crystal_archive_typeii",
            "ilford_mgrc_deluxe_grade2",
        ]
        .get(legacy)
        .copied()
    } else {
        crate::border::FILM_IDS.get(legacy).copied()
    };
    stocks
        .iter()
        .find(|s| Some(s.id.as_str()) == id.map(String::as_str).or(old))
        .copied()
        .unwrap_or(stocks[0])
}

#[cfg(test)]
mod tests {
    #[test]
    fn complete_catalog_and_pairing_metadata() {
        let films = super::stocks(false);
        let papers = super::stocks(true);
        assert_eq!(films.len(), 23);
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
}
