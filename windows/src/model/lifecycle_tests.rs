//! The render and brightness lifecycle through the real helper, with the
//! harness (`model::harness`). Each test runs in a process of its own; see
//! there. These record the lifecycle as it is before the held frame (B1) and
//! quick frames (B2) of docs/windows-nov5-port.

use super::harness::{self, Harness, lifecycle_test};
use super::{FrameKind, FrameTag, RenderRequest};
use crate::state;

const GOLD: &str = "kodak_gold_200";
const PORTRA: &str = "kodak_portra_400";

fn settled(revision: u64, generation: u64) -> RenderRequest {
    RenderRequest { kind: FrameKind::Settled, revision, generation, interactive_scale: None }
}

/// The other film of the pair.
fn other(film: &str) -> &'static str {
    if film == GOLD { PORTRA } else { GOLD }
}

lifecycle_test! {
    /// A film change renders the new revision once, at export size, and
    /// shows it: one settled render, one frame, the preview current. A canvas
    /// showing the photograph at a quarter of its pixels changes nothing: only
    /// a drag renders at canvas size.
    fn a_film_change_renders_once_and_settles() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        crate::ui::canvas::set_fixed_scale(Some(0.25));
        let photo = h.photo.clone();
        h.open(&photo);
        let m = h.model();
        let (revision, generation) = (m.revision, m.source_generation);
        assert!(m.preview_current && m.image.is_some() && m.render_at_export_size, "{:?}", h.errors);
        assert_eq!(h.renders().last(), Some(&settled(revision, generation)), "the photograph's first render");
        assert!(h.renders().iter().all(|r| r.kind == FrameKind::Settled));
        assert_eq!(h.model().image_size, crate::crop::Size::new(3072.0, 2048.0));

        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let revision = h.model().revision;
        // The 400 ms activity timer, run now: the badge shows while the
        // render runs (its answer runs only when the harness steps).
        assert!(crate::ui::testing::next_timer().is_some());
        assert!(h.fire_timers(std::time::Duration::from_millis(400)) >= 1);
        assert!(h.model().shows_activity && !h.model().preview_current);
        h.idle();
        assert!(!h.model().shows_activity);
        assert_eq!(h.renders(), [settled(revision, generation)]);
        assert_eq!(h.command_names(), ["render"], "nothing else asks the helper");
        let shown = h.model().shown_frame.unwrap();
        assert_eq!((shown.revision, shown.generation, shown.kind), (revision, generation, FrameKind::Settled));
        assert_eq!(h.frames, [shown], "the canvas changes once");
        let m = h.model();
        assert!(m.preview_current && !m.showing_interactive_frame && m.render_at_export_size && m.state.film == film);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// With matching on, the solve starts once the settled frame of the edit
    /// is on screen (it reads that render's decode), and the solved exposure
    /// renders once more. Meanwhile the frame at the old exposure shows: the
    /// canvas changes twice. This is the behaviour B1 (the held frame) changes.
    fn the_match_solves_after_the_settled_frame_and_renders_again() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        let opened = h.model().revision;
        h.edit(|m| m.open(&photo));
        let generation = h.model().source_generation;
        // Stop where the solve starts: the first render is shown and current.
        h.run_until("the solve", |m| m.matching_brightness);
        let first = h.model().shown_frame.unwrap();
        assert_eq!((first.revision, first.kind), (opened + 1, FrameKind::Settled));
        assert!(h.model().preview_current);
        assert_eq!(h.command_names(), ["catalog", "render"]);
        h.idle();
        let m = h.model();
        let ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).expect("matched");
        assert!(!m.brightness_match_pending() && m.preview_current, "{:?} {:?}", m.brightness_match_note(), h.errors);
        assert!(ev.abs() > 1e-9, "the match moved the exposure ({ev}), so it renders again");
        assert!((crate::json::number(m.value(state::PREP_EXPOSURE)) - ev).abs() < 1e-6, "Exposure holds the match");
        let solved = m.revision;
        assert_eq!(solved, opened + 2, "the match is one more edit");
        assert_eq!(h.renders(), [settled(opened + 1, generation), settled(solved, generation)]);
        assert_eq!(h.command_names(), ["catalog", "render", "place_exposure", "render"]);
        let last = h.model().shown_frame.unwrap();
        assert_eq!(h.frames, [first, FrameTag { revision: solved, ..last }], "the old exposure, then the matched one");
        eprintln!("opened: matched at {ev:+.3} EV");

        // A film change. Gold 200 and Portra 400 match this photograph at the
        // same exposure, so the solve only marks the match current: the
        // film's frame, solved at the old exposure, is the final one.
        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let changed = h.model().revision;
        h.run_until("the solve", |m| m.matching_brightness);
        assert_eq!(h.model().shown_frame.map(|f| f.revision), Some(changed), "the film's frame shows during the solve");
        h.idle();
        let m = h.model();
        let film_ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).unwrap();
        eprintln!("{film}: matched at {film_ev:+.4} EV");
        assert!((film_ev - ev).abs() < 0.05, "under 0.05 EV apart, the match keeps its value");
        assert!(!m.brightness_match_pending() && m.preview_current && m.revision == changed);
        assert_eq!(h.renders(), [settled(changed, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure"]);
        assert_eq!(h.frames.len(), 1);

        // Turning off the print exposure's normalisation needs another
        // exposure (camera exposure compensation doesn't: the normalised print
        // takes it back). The settled frame at the old exposure shows during
        // the solve, then the matched one renders.
        h.clear_logs();
        h.edit(|m| m.set("enlarger.normalize_print_exposure", serde_json::json!(false)));
        let compensated = h.model().revision;
        h.run_until("the solve", |m| m.matching_brightness);
        let stale = h.model().shown_frame.unwrap();
        assert_eq!((stale.revision, stale.kind), (compensated, FrameKind::Settled), "the old exposure's frame is on screen during the solve");
        h.idle();
        let m = h.model();
        let new_ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).unwrap();
        eprintln!("print exposure not normalized: matched at {new_ev:+.4} EV");
        assert!((new_ev - film_ev).abs() >= 0.05, "the match moved ({film_ev} to {new_ev})");
        assert!(!m.brightness_match_pending() && m.preview_current);
        assert_eq!(m.revision, compensated + 1);
        assert_eq!(h.renders(), [settled(compensated, generation), settled(compensated + 1, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render"]);
        assert_eq!(h.frames.iter().map(|f| f.revision).collect::<Vec<_>>(), [compensated, compensated + 1], "the canvas changes twice");        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The canvas-scale hook: a drag renders at the fixed scale, and letting
    /// go renders the photograph at export size.
    fn a_drag_renders_at_the_fixed_canvas_scale() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        let photo = h.photo.clone();
        h.open(&photo);
        crate::ui::canvas::set_fixed_scale(Some(0.25));
        h.clear_logs();
        h.edit(|m| m.begin_interaction());
        h.edit(|m| m.set("prep.shadows", serde_json::json!(20)));
        let dragged = h.model().revision;
        h.idle();
        let generation = h.model().source_generation;
        assert_eq!(h.renders(), [RenderRequest { kind: FrameKind::Drag, revision: dragged, generation, interactive_scale: Some(0.25) }]);
        assert!(h.model().showing_interactive_frame && !h.model().preview_current);
        assert_eq!(h.model().shown_frame.map(|f| f.kind), Some(FrameKind::Drag));
        h.edit(|m| m.end_interaction("Change Adjustment"));
        h.idle();
        assert_eq!(h.renders().last(), Some(&settled(dragged, generation)));
        assert_eq!(h.renders().len(), 2);
        let m = h.model();
        assert!(m.preview_current && !m.showing_interactive_frame && m.render_at_export_size);
        assert_eq!(m.undo_title(), Some("Change Adjustment"));
    }
}

lifecycle_test! {
    /// A catalog cache saved by an older helper (fewer films) under the
    /// current helper's key is shown at launch, then replaced by the helper's
    /// own catalog, in the model and in the file. (A cache from another helper
    /// binary has another key and isn't read at all.)
    fn a_stale_catalog_cache_is_replaced_by_the_helpers() {
        let paths = match crate::engine::EnginePaths::locate() {
            Ok(paths) => paths,
            Err(_) => {
                assert!(std::env::var_os("NEGS_REQUIRE_HELPER").is_none(), "NEGS_REQUIRE_HELPER is set and the release helper isn't built");
                return eprintln!("lifecycle harness skipped: build the release helper first");
            }
        };
        // The current catalog, cut to the films from before the Fotufilm port.
        let current = {
            let h = Harness::start(harness::prefs(false)).unwrap();
            h.app.model.catalog.clone()
        };
        let mut stale = current.clone();
        let old: Vec<serde_json::Value> = current["profiles"].as_array().unwrap().iter()
            .filter(|p| p["paper"].as_bool() == Some(true) || !FOTUFILM.contains(&p["id"].as_str().unwrap()))
            .cloned().collect();
        stale["profiles"] = serde_json::Value::Array(old);
        crate::session::CatalogCache::save(&stale, &paths.helper, &paths.data);
        let film_count = |catalog: &serde_json::Value| catalog["profiles"].as_array().unwrap().iter().filter(|p| p["paper"].as_bool() != Some(true)).count();
        assert_eq!(film_count(&stale), 23);
        assert!(crate::session::CatalogCache::load(&paths.helper, &paths.data).is_some_and(|c| film_count(&c) == 23));

        let mut model = super::Model::new(harness::prefs(false));
        model.launch();
        assert_eq!(model.films().len(), 23, "the cached catalog fills in the stocks at launch");
        model.shutdown();
        let mut h = Harness::start(harness::prefs(false)).unwrap();
        assert_eq!(h.model().films().len(), 52);
        assert_eq!(h.model().papers().len(), 10);
        let saved = crate::session::CatalogCache::load(&paths.helper, &paths.data).unwrap();
        assert_eq!(film_count(&saved), 52, "the helper's catalog replaced the file");
    }
}

/// The 29 films of the Fotufilm port (`9fa749b`).
const FOTUFILM: [&str; 29] = [
    "agfa_agfachrome_100", "agfa_agfachrome_200", "agfa_agfachrome_50", "cinestill_400d", "cinestill_800t",
    "fotufilm_infrared_color", "fujifilm_acros_100_ii", "fujifilm_astia_100f", "fujifilm_eterna_500",
    "fujifilm_eterna_vivid_500", "fujifilm_f64d", "fujifilm_pro_160ns", "fujifilm_reala_100", "fujifilm_reala_500d",
    "fujifilm_superia_200", "fujifilm_velvia_50", "harman_phoenix_200", "ilford_delta_3200", "ilford_fp4_plus",
    "ilford_hp5_plus", "kodak_aerochrome_1443", "kodak_aerocolor_2460", "kodak_kodachrome_200", "kodak_kodachrome_25",
    "kodak_kodacolor_100", "kodak_tmax_100", "lomography_color_100", "lomography_lomochrome_metropolis",
    "lomography_lomochrome_turquoise",
];

lifecycle_test! {
    /// Fotufilm: the helper lists 52 films and 10 papers; every film lands in
    /// exactly one grid group; the 29 new films are there with their own 35mm
    /// strips (full and half frame), and every film's strip loads.
    fn the_catalog_has_the_fotufilm_films_grouped_with_their_borders() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        let m = h.model();
        let (films, papers) = (m.films(), m.papers());
        assert_eq!((films.len(), papers.len()), (52, 10));
        let groups = state::film_groups(&films, &papers);
        let grouped: Vec<&str> = groups.iter().flat_map(|g| g.stocks.iter().map(|s| s.id.as_str())).collect();
        let mut sorted = grouped.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!((grouped.len(), sorted.len()), (52, 52), "each film in exactly one group");
        for g in &groups {
            eprintln!("{}: {}", g.title, g.stocks.len());
        }
        let data = m.paths.as_ref().unwrap().data.clone();
        for id in FOTUFILM {
            assert!(films.iter().any(|f| f.id == id), "{id} is missing from the catalog");
            for format in ["full", "half"] {
                assert!(data.join("borders/35mm").join(format).join(format!("{id}.png")).exists(), "{id} has no {format} strip");
            }
        }
        let generic: Vec<&str> = films.iter().filter(|f| !data.join("borders/35mm/full").join(format!("{}.png", f.id)).exists()).map(|f| f.id.as_str()).collect();
        eprintln!("films on the generic strip: {generic:?}");
        for film in &films {
            for format in [crate::border::FilmFormat::Full, crate::border::FilmFormat::Half] {
                assert!(crate::border::FilmStripTexture::load(&data, &film.id, format).is_some(), "{}", film.id);
            }
        }
    }
}
/// Asks the harness's helper directly and waits for the answer.
fn ask(h: &Harness, command: serde_json::Value) -> serde_json::Value {
    let (sender, receiver) = std::sync::mpsc::channel();
    h.app.model.engine.as_ref().unwrap().request(command, std::time::Duration::from_secs(600), move |result| {
        let _ = sender.send(result);
    });
    receiver.recv().unwrap().unwrap().1
}

lifecycle_test! {
    /// The shared pipeline cache (ticket 29) through the persistent helper:
    /// a film's thumbnail builds the pipeline its main render then reuses;
    /// the main render's last films stay cached through more thumbnails than
    /// the cache holds and through `release_thumbnails`; and turning the
    /// previews off in Settings sends `release_thumbnails` and still renders.
    fn the_pipeline_cache_keeps_the_main_renders_films() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        let photo = h.photo.clone();
        h.open(&photo);
        let m = h.model();
        let (state, stocks, generation) = (m.render_state(), m.stocks.clone(), m.source_generation);
        let preview = m.engine.as_ref().unwrap().directory().join("probe.rgba");
        let thumbnail = |h: &Harness, id: &str| {
            let stock = stocks.iter().find(|s| s.id == id).unwrap();
            let mut command = crate::previews::stock_command(&state, stock, &stocks, false, &photo, generation);
            command["preview_path"] = serde_json::json!(h.app.model.engine.as_ref().unwrap().directory().join("probe-thumb.rgba"));
            ask(h, command)
        };
        let render = |h: &Harness, id: &str| {
            let chosen = state::choosing(&state, id, false, false, &stocks);
            let reply = ask(h, serde_json::json!({"command": "render", "input": photo, "source_generation": generation,
                "film": chosen.film, "paper": chosen.paper, "params": chosen.params, "limit_preview": false, "preview_path": preview}));
            (reply["cached_pipeline"].as_bool().unwrap(), reply["pipeline_ms"].as_f64().unwrap(), reply["elapsed_ms"].as_f64().unwrap())
        };
        let x = "kodak_ektar_100";
        let first = thumbnail(&h, x);
        assert_eq!(first["cached_pipeline"], serde_json::json!(false), "{x}'s pipeline is new");
        let (cached, pipeline_ms, _) = render(&h, x);
        assert!(cached, "the main render reuses the thumbnail's pipeline");
        eprintln!("{x}: thumbnail built its pipeline; main render cached, pipeline {pipeline_ms:.1} ms");

        let (a, b) = (GOLD, PORTRA);
        for round in 0..3 {
            for film in [a, b] {
                let (cached, pipeline_ms, elapsed) = render(&h, film);
                eprintln!("A/B round {round}: {film} cached {cached} pipeline {pipeline_ms:.1} ms render {elapsed:.0} ms");
                if round > 0 {
                    assert!(cached, "{film} round {round}");
                }
            }
        }
        // More thumbnails than the cache's 40 entries.
        let others: Vec<String> = stocks.iter().filter(|s| !s.paper && ![a, b, x].contains(&s.id.as_str())).map(|s| s.id.clone()).collect();
        assert!(others.len() > 40);
        for id in &others {
            thumbnail(&h, id);
        }
        for film in [a, b, x] {
            let (cached, pipeline_ms, _) = render(&h, film);
            eprintln!("after {} thumbnails: {film} cached {cached} pipeline {pipeline_ms:.1} ms", others.len());
            assert!(cached, "{film}: one of the main render's last films");
        }
        assert_eq!(ask(&h, serde_json::json!({"command": "release_thumbnails"})), serde_json::json!({}));
        for film in [a, b] {
            let (cached, _, _) = render(&h, film);
            assert!(cached, "{film} stays cached through release_thumbnails");
        }
        // A thumbnail's pipeline that no main render used is gone.
        let evicted = thumbnail(&h, &others[others.len() / 2]);
        assert_eq!(evicted["cached_pipeline"], serde_json::json!(false));

        // Settings › previews off, end to end: thumbnails made, then turned off.
        h.clear_logs();
        h.edit(|m| m.set_previews(true));
        h.edit(|m| m.toggle_foldout(false));
        let started = std::time::Instant::now();
        h.idle();
        let thumbnails = h.command_names().iter().filter(|c| *c == "thumbnail").count();
        eprintln!("{thumbnails} film thumbnails in {:.1} s (debug or release test build, 3072 × 2048 photograph)", started.elapsed().as_secs_f64());
        assert_eq!(thumbnails, 52);
        h.clear_logs();
        h.edit(|m| m.set_previews(false));
        h.idle();
        assert_eq!(h.command_names(), ["release_thumbnails"]);
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        h.idle();
        assert_eq!(h.command_names(), ["release_thumbnails", "render"]);
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
    }
}