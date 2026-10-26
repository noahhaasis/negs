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
}
#[derive(Deserialize)]
struct Catalog {
    profiles: Vec<Stock>,
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
pub fn stocks(paper: bool) -> Vec<&'static Stock> {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    LIVE.get()
        .unwrap_or_else(|| {
            CATALOG.get_or_init(|| {
                serde_json::from_str(include_str!("../catalog.json"))
                    .expect("checked catalog snapshot")
            })
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
    fn live_profiles_accept_no_target_paper() {
        let stock:super::Stock=serde_json::from_value(serde_json::json!({"id":"slide","name":"Slide","paper":false,"positive":true,"bw":false,"target_print":null,"development_times":[]})).unwrap();
        assert!(stock.target.is_empty());
    }
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
