//! The edit and the catalog: what the inspector shows and the engine renders.
//! Mirrors `Parameters.swift` and `Inspector.swift` in the macOS app.
//!
//! The shared core's (core/src/state, through `negs_core_rules::state`):
//! the edit state, stocks, colour filters, the control tables, the film
//! menus' groups and search, choosing a stock. This module keeps its API
//! and tests, and gives the film search Windows' own comparison.

pub use negs_core_rules::state::*;

#[cfg(test)]
use crate::crop::CropSettings;
#[cfg(test)]
use crate::json;
#[cfg(test)]
use serde_json::Value;

/// The stocks matching every word of `query` in their name, id or group
/// title, ignoring case and accents ("portra 160", "kodak slide"); groups
/// left empty drop out (`StockGroup.matching`). An empty query keeps them all.
pub fn matching<'a>(query: &str, groups: Vec<StockGroup<'a>>) -> Vec<StockGroup<'a>> {
    static CONTAINS: std::sync::Once = std::sync::Once::new();
    CONTAINS.call_once(|| set_contains(Some(contains_folded)));
    negs_core_rules::state::matching(query, groups)
}

/// Whether `text` contains `word`, ignoring case and diacritics, as
/// `localizedStandardContains` compares. The invariant locale keeps the
/// result the same on every machine; the catalog's names are English. The
/// core's film search asks it (negs_text_set_contains).
unsafe extern "C" fn contains_folded(text: *const u8, text_len: usize, word: *const u8, word_len: usize) -> u8 {
    use windows::Win32::Globalization::{FIND_FROMSTART, FindNLSStringEx, LINGUISTIC_IGNORECASE, LINGUISTIC_IGNOREDIACRITIC, LOCALE_NAME_INVARIANT};
    let utf8 = |data: *const u8, len: usize| -> &str {
        if data.is_null() || len == 0 { "" } else { std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) }).unwrap_or("") }
    };
    let (text, word): (Vec<u16>, Vec<u16>) = (utf8(text, text_len).encode_utf16().collect(), utf8(word, word_len).encode_utf16().collect());
    let flags = FIND_FROMSTART | LINGUISTIC_IGNORECASE.0 | LINGUISTIC_IGNOREDIACRITIC.0;
    (unsafe { FindNLSStringEx(LOCALE_NAME_INVARIANT, flags, &text, &word, None, None, None, windows::Win32::Foundation::LPARAM(0)) } >= 0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_before_lens_corrections_keep_them_off() {
        let defaults = serde_json::json!({"io": {"lens_correction": {"enabled": true}, "rotate_cw": 0}});
        let old = serde_json::json!({"io": {"rotate_cw": 1}});
        assert_eq!(restored_params(&defaults, &old, &[]), serde_json::json!({"io": {"lens_correction": {"enabled": false}, "rotate_cw": 1}}));
        for saved in [true, false] {
            let current = serde_json::json!({"io": {"lens_correction": {"enabled": saved}, "rotate_cw": 1}});
            assert_eq!(json::get(&restored_params(&defaults, &current, &[]), LENS_CORRECTION), &Value::Bool(saved));
        }
    }
    use serde_json::json;

    /// The same values as the macOS app's slider tests.
    #[test]
    fn snap_steps_and_clamps() {
        assert_eq!(snap(0.123456, 0.0, 2.0, 0.01), 0.12);
        assert_eq!(snap(2.7, 1.0, 4.0, 1.0), 3.0);
        assert_eq!(snap(99.0, -5.0, 5.0, 0.1), 5.0);
        assert_eq!(snap(0.0251, 0.0, 0.2, 0.001), 0.025);
        assert_eq!(snap(0.33, 0.1, 1.0, 0.05), 0.35);
        assert_eq!(snap(-1.234, -1.5, 1.5, 0.01), -1.23);
    }

    #[test]
    fn values_show_the_step_decimals() {
        assert_eq!(format_value(0.5, 0.01), "0.5");
        assert_eq!(format_value(-0.0001, 0.1), "0");
        assert_eq!(format_value(12.0, 1.0), "12");
        assert_eq!(format_value(0.025, 0.001), "0.025");
    }

    #[test]
    fn rendered_state_ignores_presentation() {
        let mut state = EditState { params: json!({"io": {"rotate_cw": 1}, "a": 2}), ..Default::default() };
        let rendered = state.rendered();
        state.border.enabled = true;
        state.crop.angle = 3.0;
        json::set(&mut state.params, "io.rotate_cw", json!(2));
        assert_eq!(state.rendered(), rendered);
        json::set(&mut state.params, "a", json!(2.5));
        assert_ne!(state.rendered(), rendered);
    }

    /// The crop's extent is the render's identity only with a target, as on the Mac.
    #[test]
    fn rendered_state_keeps_the_crop_extent_only_for_longest_side() {
        let mut state = EditState { params: json!({"io": {"rotate_cw": 0, "photo_long_side_px": null}}), ..Default::default() };
        let original = state.rendered();
        state.crop.rect = crate::crop::Rect::new(0.1, 0.2, 0.5, 0.5);
        assert_eq!(state.rendered(), original);
        json::set(&mut state.params, SIZE_TARGET, json!(4000));
        let sized = state.rendered();
        state.crop.rect.x = 0.3;
        state.crop.angle = -2.0;
        assert_eq!(state.rendered(), sized);
        state.crop.rect.width = 0.6;
        assert_ne!(state.rendered(), sized);
    }

    #[test]
    fn image_size_modes_and_resolution_follow_the_engine() {
        let params = |target: Value| json!({"io": {"photo_long_side_px": target}});
        assert_eq!(size_mode(&params(Value::Null)), SizeMode::Original);
        assert_eq!(size_mode(&params(json!(6000))), SizeMode::LongestSide);
        let centre = CropSettings { rect: crate::crop::Rect::new(0.25, 0.25, 0.5, 0.5), ..Default::default() };
        let size = resolve_size((6000, 4000), &params(json!(6000)), &centre).unwrap();
        assert_eq!((size.working, size.photo), ((12000, 8000), (6000, 4000)));
        assert_eq!(resolve_size((6000, 4000), &params(Value::Null), &centre).unwrap().working, (6000, 4000));
    }

    #[test]
    fn grain_lanes_follow_the_stock_preset() {
        assert_eq!(grain_lane("film_render.grain.rms_granularity.2"), Some(("rms_granularity", 2)));
        assert_eq!(grain_lane("film_render.grain.micro_structure.0"), None);
        assert_eq!(grain_lane("film_render.grain.blur"), None);
        assert!(color_only("film_render.grain.uniformity.1"));
        assert!(!color_only("film_render.grain.uniformity.0"));
        assert!(!color_only("film_render.grain.particle_scale_sublayers.2"));
        let lanes: Vec<_> = ADJUSTMENT_GROUPS.iter().flat_map(|g| g.controls).filter_map(|c| grain_lane(c.path())).collect();
        assert_eq!(lanes.len(), 9);
    }

    /// `FilmFormatTests`: formats read back from the long edge.
    #[test]
    fn film_formats_read_back_from_the_long_edge() {
        let name = |mm: f64| film_format(mm).map(|i| FILM_FORMATS[i].0);
        assert_eq!(name(35.0), Some("35 mm (135)"), "The engine default is 135");
        assert_eq!(name(56.0), Some("6×4.5 / 6×6"));
        assert_eq!(name(120.0), Some("4×5"));
        assert_eq!(name(24.0f32 as f64), Some("Half frame"));
        assert_eq!((name(45.0), name(36.0)), (None, None));
        let Adjustment::Slider { low, high, .. } = FILM_FORMAT else { unreachable!() };
        assert!(FILM_FORMATS.iter().all(|(_, mm)| (low..=high).contains(mm)));
        let mut edges: Vec<i64> = FILM_FORMATS.iter().map(|(_, mm)| *mm as i64).collect();
        edges.dedup();
        assert_eq!(edges.len(), FILM_FORMATS.len());
    }

    /// The match depends on what changes the developed brightness, not on
    /// the user's own exposure edits or the frame.
    #[test]
    fn match_inputs_leave_out_the_users_own_brightness() {
        let mut state = EditState { params: json!({"prep": {"exposure_ev": 0.5}, "enlarger": {"print_exposure": 1.0, "m_filter_shift": 0.0},
            "io": {"rotate_cw": 0, "output_color_space": "sRGB"}, "camera": {"exposure_compensation_ev": 0.0}}), ..Default::default() };
        let inputs = match_inputs(&state);
        for (path, value) in [("prep.exposure_ev", json!(2.0)), ("prep.shadows", json!(30)), ("enlarger.print_exposure", json!(1.4)),
            ("io.rotate_cw", json!(2)), ("io.output_color_space", json!("Display P3"))] {
            let mut edited = state.clone();
            json::set(&mut edited.params, path, value);
            assert!(json::same(&match_inputs(&edited), &inputs), "{path}");
        }
        state.crop.angle = 3.0;
        assert!(json::same(&match_inputs(&state), &inputs));
        for (path, value) in [("camera.exposure_compensation_ev", json!(1.0)), ("enlarger.m_filter_shift", json!(5.0))] {
            let mut edited = state.clone();
            json::set(&mut edited.params, path, value);
            assert!(!json::same(&match_inputs(&edited), &inputs), "{path}");
        }
        let mut paper = state.clone();
        paper.paper = "other".into();
        assert!(!json::same(&match_inputs(&paper), &inputs));
        // The match never re-renders by itself, and saves with the session.
        let mut matched = state.clone();
        matched.brightness_match = Some(BrightnessMatch { ev: Some(0.5), inputs: Some(inputs), highlight_limited: false });
        assert_eq!(matched.rendered(), state.rendered());
        let text = serde_json::to_string(&matched).unwrap();
        assert!(text.contains(r#""brightnessMatch":{"ev":0.5"#));
        assert_eq!(serde_json::from_str::<EditState>(&text).unwrap(), matched);
        assert!(!serde_json::to_string(&state).unwrap().contains("brightnessMatch"));
    }

    #[test]
    fn stocks_group_like_the_mac() {
        let stock = |id: &str, name: &str, paper: bool, positive: bool, bw: bool, target: &str| Stock {
            id: id.into(), name: name.into(), paper, positive, bw, target: target.into(), times: vec![], grain: Value::Null, fotufilm: false, manufacturer_data: true, negs_built: false, cine_print: false,
        };
        let films = [
            stock("gold", "Gold", false, false, false, "endura"),
            stock("vision", "Vision3", false, false, false, "print2383"),
            stock("velvia", "Velvia", false, true, false, ""),
            stock("trix", "Tri-X", false, false, true, ""),
        ];
        let papers = [stock("endura", "Endura", true, false, false, ""), stock("print2383", "2383 Print Film", true, false, false, "")];
        let (f, p): (Vec<&Stock>, Vec<&Stock>) = (films.iter().collect(), papers.iter().collect());
        let titles: Vec<_> = film_groups(&f, &p).iter().map(|g| g.title).collect();
        assert_eq!(titles, ["Color Negative", "Color Slide", "Motion Picture", "Black & White"]);
        let titles: Vec<_> = paper_groups(&p).iter().map(|g| (g.title, g.stocks.len())).collect();
        assert_eq!(titles, [("Color Paper", 1), ("Motion Picture Print", 1)]);
        assert_eq!(option_title("some_new_option"), "Some New Option");
        assert_eq!(ratio_name(1920.0, 1080.0), "16:9");
        assert_eq!(ratio_name(1000.0, 1300.0), "1.30:1");
    }

    /// `testFilmSearchMatchesEveryWordInNameIdOrGroup`, one case for one,
    /// then accents, ids and the order of the matches.
    #[test]
    fn film_search_matches_every_word_in_name_id_or_group() {
        let stock = |id: &str, name: &str, paper: bool, positive: bool, bw: bool| Stock {
            id: id.into(), name: name.into(), paper, positive, bw, target: String::new(), times: vec![], grain: Value::Null, fotufilm: false, manufacturer_data: true, negs_built: false, cine_print: false,
        };
        // The Mac's fixture: Gold 200, Velvia 50 and Tri-X 400.
        let films = [stock("gold", "Gold 200", false, false, false), stock("velvia", "Velvia 50", false, true, false), stock("trix", "Tri-X 400", false, false, true)];
        let f: Vec<&Stock> = films.iter().collect();
        let groups = || film_groups(&f, &[]);
        let ids = |query: &str| -> Vec<String> { matching(query, groups()).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        assert_eq!(ids(""), ["gold", "velvia", "trix"]);
        assert_eq!(ids("  "), ["gold", "velvia", "trix"]);
        assert_eq!(ids("tri-x"), ["trix"]);
        assert_eq!(ids("GOLD 200"), ["gold"]);
        assert_eq!(ids("slide"), ["velvia"]);
        assert_eq!(ids("black 400"), ["trix"]);
        assert!(ids("gold 400").is_empty());
        assert_eq!(matching("velvia", groups()).iter().map(|g| g.title).collect::<Vec<_>>(), ["Color Slide"]);

        // Accents and case in the query or the name; any whitespace splits words.
        let more = [
            stock("kodak_portra_400", "Portra 400", false, false, false),
            stock("kodak_portra_160", "Portra 160", false, false, false),
            stock("fujifilm_velvia_100", "Velvia 100", false, true, false),
            stock("lomography_lomochrome_metropolis", "LomoChrome Métropolis", false, false, false),
            stock("kodak_trix400_d76_8min", "Tri-X 400 (D-76, 8 min)", false, false, true),
        ];
        let m: Vec<&Stock> = more.iter().collect();
        let ids = |query: &str| -> Vec<String> { matching(query, film_groups(&m, &[])).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        assert_eq!(ids("PÓRTRÄ 400"), ["kodak_portra_400"]);
        assert_eq!(ids("metropolis"), ["lomography_lomochrome_metropolis"]);
        assert_eq!(ids("MÉTROPOLIS"), ["lomography_lomochrome_metropolis"]);
        assert_eq!(ids("portra\t160"), ["kodak_portra_160"]);
        // Words in either order; the matches keep the catalog's order.
        assert_eq!(ids("400 portra"), ids("portra 400"));
        assert_eq!(ids("portra"), ["kodak_portra_400", "kodak_portra_160"]);
        // The id with `_` read as spaces: "kodak" is only in the ids.
        assert_eq!(ids("kodak"), ["kodak_portra_400", "kodak_portra_160", "kodak_trix400_d76_8min"]);
        assert_eq!(ids("fujifilm velvia"), ["fujifilm_velvia_100"]);
        assert_eq!(ids("trix400 d76"), ["kodak_trix400_d76_8min"]);
        // An underscore in the query isn't a space, as on the Mac.
        assert!(ids("portra_400").is_empty());
        // Punctuation counts: "tri-x" isn't "trix", and a lone symbol matches only itself.
        assert_eq!(ids("tri-x"), ["kodak_trix400_d76_8min"]);
        assert!(ids("-x400").is_empty());
        // The group titles: Black & White, Color Slide, Color Negative.
        assert_eq!(ids("&"), ["kodak_trix400_d76_8min"]);
        assert_eq!(ids("color negative 400"), ["kodak_portra_400"]);
        assert!(ids("not-a-film").is_empty());
    }

    /// The helper's own catalog: every film finds itself. Skipped without a
    /// release helper.
    #[test]
    fn film_search_finds_the_live_catalog_films() {
        let Ok(paths) = crate::engine::EnginePaths::locate() else {
            eprintln!("skipped: build the release helper first");
            return;
        };
        let engine = crate::engine::EngineClient::start(paths).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        engine.request(json!({"command": "catalog"}), crate::engine::timeout_for("catalog"), move |result| {
            let _ = sender.send(result);
        });
        let catalog = receiver.recv().unwrap().unwrap().1;
        engine.stop();
        let stocks: Vec<Stock> = catalog["profiles"].as_array().map(|a| a.iter().map(Stock::new).collect()).unwrap_or_default();
        let films: Vec<&Stock> = stocks.iter().filter(|s| !s.paper).collect();
        let papers: Vec<&Stock> = stocks.iter().filter(|s| s.paper).collect();
        let ids = |query: &str| -> Vec<String> { matching(query, film_groups(&films, &papers)).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        eprintln!("{} films in the live catalog", films.len());
        assert_eq!(ids("").len(), films.len());
        // Every film finds itself by its whole name, and by its id.
        for film in &films {
            assert!(ids(&film.name).contains(&film.id), "{}", film.name);
            assert!(ids(&film.id.replace('_', " ")).contains(&film.id), "{}", film.id);
        }
        let portra = ids("portra");
        assert!(!portra.is_empty() && portra.iter().all(|id| id.contains("portra")), "{portra:?}");
        assert!(ids("gold 400").is_empty());
        assert!(ids("slide").iter().all(|id| stocks.iter().any(|s| &s.id == id && s.positive)));
    }
}
