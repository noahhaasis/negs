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
    /// With matching on, the settled frame of an edit whose match is pending
    /// is held, not shown: the solve starts from it (it reads that render's
    /// decode), and the canvas goes from the previous frame straight to the
    /// matched one. Before B1 (docs/windows-nov5-port) the held frame was
    /// shown during the solve, so the canvas changed twice.
    fn the_match_solves_after_the_settled_frame_and_renders_again() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        let opened = h.model().revision;
        h.edit(|m| m.open(&photo));
        let generation = h.model().source_generation;
        // Stop where the solve starts: the first render is held, nothing shown.
        h.run_until("the solve", |m| m.matching_brightness);
        let first = h.model().held_frame().expect("the first frame is held");
        assert_eq!((first.revision, first.generation, first.kind), (opened + 1, generation, FrameKind::Settled));
        assert!(!h.model().preview_current && h.model().shown_frame.is_none() && h.model().image.is_none());
        assert_eq!(h.command_names(), ["catalog", "render"]);
        h.idle();
        let m = h.model();
        let ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).expect("matched");
        assert!(!m.brightness_match_pending() && m.preview_current && m.held_frame().is_none(), "{:?} {:?}", m.brightness_match_note(), h.errors);
        assert!(ev.abs() > 1e-9, "the match moved the exposure ({ev}), so it renders again");
        assert!((crate::json::number(m.value(state::PREP_EXPOSURE)) - ev).abs() < 1e-6, "Exposure holds the match");
        let solved = m.revision;
        assert_eq!(solved, opened + 2, "the match is one more edit");
        assert_eq!(h.renders(), [settled(opened + 1, generation), settled(solved, generation)]);
        assert_eq!(h.command_names(), ["catalog", "render", "place_exposure", "render"]);
        let last = h.model().shown_frame.unwrap();
        assert_eq!((last.revision, last.kind), (solved, FrameKind::Settled));
        assert_eq!(h.frames, [last], "only the matched frame is shown");
        eprintln!("opened: matched at {ev:+.3} EV");

        // A film change. Gold 200 and Portra 400 match this photograph at the
        // same exposure, so the solve only marks the match current: the held
        // frame is the final one, and is shown.
        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let changed = h.model().revision;
        h.run_until("the solve", |m| m.matching_brightness);
        let held = h.model().held_frame().expect("the film's frame is held during the solve");
        assert_eq!(held.revision, changed);
        assert_eq!(h.model().shown_frame, Some(last), "the previous frame stays on screen");
        h.idle();
        let m = h.model();
        let film_ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).unwrap();
        eprintln!("{film}: matched at {film_ev:+.4} EV");
        assert!((film_ev - ev).abs() < 0.05, "under 0.05 EV apart, the match keeps its value");
        assert!(!m.brightness_match_pending() && m.preview_current && m.revision == changed);
        assert_eq!(h.renders(), [settled(changed, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure"]);
        assert_eq!(h.frames, [held], "the held frame is shown");

        // Turning off the print exposure's normalisation needs another
        // exposure (camera exposure compensation doesn't: the normalised print
        // takes it back). The settled frame at the old exposure is held during
        // the solve and never shown; the matched one renders and shows.
        h.clear_logs();
        h.edit(|m| m.set("enlarger.normalize_print_exposure", serde_json::json!(false)));
        let compensated = h.model().revision;
        h.run_until("the solve", |m| m.matching_brightness);
        assert_eq!(h.model().held_frame().map(|f| (f.revision, f.kind)), Some((compensated, FrameKind::Settled)));
        assert_eq!(h.model().shown_frame, Some(held), "the film change's frame stays on screen during the solve");
        h.idle();
        let m = h.model();
        let new_ev = m.state.brightness_match.as_ref().and_then(|b| b.ev).unwrap();
        eprintln!("print exposure not normalized: matched at {new_ev:+.4} EV");
        assert!((new_ev - film_ev).abs() >= 0.05, "the match moved ({film_ev} to {new_ev})");
        assert!(!m.brightness_match_pending() && m.preview_current && m.held_frame().is_none());
        assert_eq!(m.revision, compensated + 1);
        assert_eq!(h.renders(), [settled(compensated, generation), settled(compensated + 1, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render"]);
        assert_eq!(h.frames.iter().map(|f| f.revision).collect::<Vec<_>>(), [compensated + 1], "the canvas changes once");
        assert!(h.errors.is_empty(), "{:?}", h.errors);
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

// MARK: The held frame (B1)

/// Matched at another exposure than Gold 200, on the test photograph
/// (−0.18 against −0.42 EV) and on the 33 MP ARW (−0.05 against −0.33): a
/// film change between them moves the match. The tests check that it does.
const SUPERIA: &str = "fujifilm_superia_200";

fn matched_ev(m: &super::Model) -> f64 {
    m.state.brightness_match.as_ref().and_then(|b| b.ev).expect("matched")
}

/// Opens `photo` and leaves it on Gold 200, matched, idle and shown.
fn open_matched(h: &mut Harness, photo: &std::path::Path) {
    h.open(photo);
    if h.model().state.film != GOLD {
        h.edit(|m| m.choose_film(GOLD));
        h.idle();
    }
    let m = h.model();
    assert!(m.matches_brightness() && !m.brightness_match_pending() && m.preview_current && m.held_frame().is_none(), "{:?} {:?}", m.brightness_match_note(), h.errors);
    assert!(h.errors.is_empty(), "{:?}", h.errors);
}

/// A copy of the test photograph under another name: another photograph to
/// the model.
fn second_photo(h: &Harness, name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("negs-harness-{name}-{}.png", std::process::id()));
    std::fs::copy(&h.photo, &path).unwrap();
    path
}

/// Waits until the current edit's frame is held, and returns it.
fn until_held(h: &mut Harness) -> FrameTag {
    h.run_until("the held frame", |m| m.held_frame().is_some_and(|f| f.revision == m.revision));
    h.model().held_frame().unwrap()
}

/// The trace's events as JSON objects.
fn trace_events(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path).unwrap().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

lifecycle_test! {
    /// A film change that moves the match: the frame at the old exposure is
    /// held while the solve runs, then dropped by the matched exposure's
    /// edit, and never shown. The previous frame and the activity badge stay
    /// up meanwhile; the canvas changes once, to the matched frame. The
    /// trace records render(old) → held → solve → render(new) → shown.
    fn a_moved_match_never_shows_the_old_exposure() {
        let trace = std::env::temp_dir().join(format!("negs-held-trace-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&trace);
        // SAFETY: the child process runs this test alone, before any thread
        // reads the environment.
        unsafe { std::env::set_var("NEGS_RENDER_TRACE", &trace) };
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let (gold_ev, previous) = (matched_ev(h.model()), h.model().shown_frame.unwrap());
        let previous_picture = h.model().image.as_ref().unwrap().id;
        let generation = h.model().source_generation;

        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!((held.revision, held.generation, held.kind), (edited, generation, FrameKind::Settled));
        let m = h.model();
        assert!(!m.preview_current && m.shown_frame == Some(previous) && m.image.as_ref().unwrap().id == previous_picture, "the previous frame stays");
        // The activity timer, run now: the badge shows while the frame is held.
        h.fire_timers(std::time::Duration::from_millis(400));
        assert_eq!(h.model().badge(), Some("Updating"));
        h.idle();

        let m = h.model();
        let ev = matched_ev(m);
        eprintln!("{SUPERIA}: matched at {ev:+.3} EV, {GOLD} at {gold_ev:+.3}");
        assert!((ev - gold_ev).abs() >= 0.05, "the film change moves the match ({gold_ev} to {ev})");
        assert_eq!(m.revision, edited + 1, "the matched exposure is one more edit");
        assert!(m.preview_current && !m.brightness_match_pending() && m.held_frame().is_none() && m.badge().is_none());
        assert_eq!(h.renders(), [settled(edited, generation), settled(edited + 1, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render"]);
        let shown = h.model().shown_frame.unwrap();
        assert_eq!((shown.revision, shown.kind), (edited + 1, FrameKind::Settled));
        assert_ne!(shown.request, held.request);
        assert_eq!(h.frames, [shown], "the canvas changes once, to the matched frame");
        assert!(h.errors.is_empty(), "{:?}", h.errors);

        // The trace of the film change, from its edit on.
        drop(h);
        let events = trace_events(&trace);
        let _ = std::fs::remove_file(&trace);
        let from = events.iter().position(|e| e["event"] == "edit" && e["revision"] == edited).expect("the edit is traced");
        let lifecycle: Vec<String> = events[from..]
            .iter()
            .filter(|e| ["edit", "render_request", "render_finished", "match_start", "match_solved", "held_shown", "held_dropped"].contains(&e["event"].as_str().unwrap()))
            .map(|e| {
                let mut line = format!("{} r{}", e["event"].as_str().unwrap(), e["revision"]);
                if e["event"] == "render_finished" {
                    line += if e["held"] == true { " held" } else if e["shown"] == true { " shown" } else { " dropped" };
                }
                if e["event"] == "held_dropped" {
                    line += &format!(" {}", e["why"].as_str().unwrap());
                }
                line
            })
            .collect();
        eprintln!("trace:\n  {}", lifecycle.join("\n  "));
        let (old, new) = (edited, edited + 1);
        assert_eq!(lifecycle, [
            format!("edit r{old}"),
            format!("render_request r{old}"),
            format!("render_finished r{old} held"),
            format!("match_start r{old}"),
            "match_solved rnull".to_owned(),
            format!("held_dropped r{old} edit"),
            format!("edit r{new}"),
            format!("render_request r{new}"),
            format!("render_finished r{new} shown"),
        ]);
    }
}

lifecycle_test! {
    /// A match that leaves the exposure as it is (Gold 200 to Portra 400):
    /// the held frame itself is shown once the solve answers, with no second
    /// render, and the preview is current.
    fn an_unchanged_match_shows_the_held_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let (gold_ev, previous) = (matched_ev(h.model()), h.model().shown_frame);
        h.clear_logs();
        h.edit(|m| m.choose_film(PORTRA));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!(h.model().shown_frame, previous);
        h.run_until("the solve", |m| m.matching_brightness);
        assert_eq!(h.model().shown_frame, previous, "the previous frame stays during the solve");
        h.idle();
        let m = h.model();
        assert!((matched_ev(m) - gold_ev).abs() < 0.05);
        assert!(m.preview_current && !m.brightness_match_pending() && m.held_frame().is_none() && m.revision == edited);
        assert!(m.render_at_export_size && m.status.contains("developed in"), "{}", m.status);
        assert_eq!(h.frames, [held], "the held render is the one shown");
        assert_eq!(h.renders(), [settled(edited, h.app.model.source_generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure"]);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// A failed match shows the held frame and its note: for the photograph
    /// (a transparent PNG can't be matched) and for the look (one that would
    /// need more than ±3 EV).
    fn a_failed_match_shows_the_held_frame_and_its_note() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        // For the photograph: the match is refused once the frame is held.
        let transparent = std::env::temp_dir().join(format!("negs-harness-transparent-{}.png", std::process::id()));
        let (w, h_) = (1536u32, 1024u32);
        let pixels: Vec<u8> = (0..w * h_).flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h_) as u8, 128, 200]
        }).collect();
        image::save_buffer(&transparent, &pixels, w, h_, image::ExtendedColorType::Rgba8).unwrap();
        h.edit(|m| m.open(&transparent));
        let generation = h.model().source_generation;
        let held = until_held(&mut h);
        assert!(h.model().shown_frame.is_none() && h.model().brightness_match_pending());
        h.idle();
        let _ = std::fs::remove_file(&transparent);
        let m = h.model();
        assert_eq!(m.brightness_match_note().as_deref(), Some("Transparent images can’t be matched."));
        assert!(m.brightness_match_unavailable().is_some() && !m.brightness_match_pending());
        assert!(m.preview_current && m.held_frame().is_none());
        assert_eq!(h.frames, [held], "the held frame is shown");
        assert_eq!(h.renders().iter().filter(|r| r.generation == generation).count(), 1);
        assert!(h.errors.is_empty(), "a refused match is a note: {:?}", h.errors);

        // For the look: with the print exposure not normalized, five stops
        // under need more than the match's ±3 EV.
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        h.edit(|m| m.set("enlarger.normalize_print_exposure", serde_json::json!(false)));
        h.idle();
        assert!(!h.model().brightness_match_pending() && h.model().preview_current);
        let previous = h.model().shown_frame;
        h.clear_logs();
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(-5.0)));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!(h.model().shown_frame, previous);
        h.idle();
        let m = h.model();
        let note = m.brightness_match_note();
        assert_eq!(note.as_deref(), Some("Matching would take more than ±3 EV."));
        assert!(m.brightness_match_unavailable().is_none() && !m.brightness_match_active(), "a failure for this look, not the photograph");
        assert!(m.preview_current && m.held_frame().is_none() && m.revision == edited);
        assert_eq!(h.frames, [held]);
        assert_eq!(h.renders(), [settled(edited, h.app.model.source_generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure"]);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// Matching turned off while the solve runs: the exposure stays where it
    /// was, so the held frame is the new state's render. It is shown once the
    /// solve (whose answer no longer applies) is back; nothing renders again
    /// and nothing stays held.
    fn turning_the_match_off_mid_solve_leaves_nothing_held() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let gold_exposure = crate::json::number(h.model().value(state::PREP_EXPOSURE));
        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.set_matches_brightness(false));
        assert_eq!(h.model().revision, edited, "turning the match off doesn't change the render");
        assert!(!h.model().brightness_match_pending());
        h.idle();
        let m = h.model();
        assert!(!m.matches_brightness() && m.held_frame().is_none() && m.preview_current);
        assert!((crate::json::number(m.value(state::PREP_EXPOSURE)) - gold_exposure).abs() < 1e-9, "the solve's answer isn't applied");
        assert_eq!(h.frames, [held]);
        assert_eq!(h.renders(), [settled(edited, h.app.model.source_generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure"]);
        assert_eq!(h.model().undo_title(), Some("Stop Matching Camera Brightness"));
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// Edits while the solve runs: film changes in a row, Undo, and opening
    /// another photograph. The held frame is dropped each time; only frames
    /// of the latest edit and photograph are shown, once matched.
    fn edits_during_the_solve_never_show_a_stale_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let (gold_ev, gold_frame) = (matched_ev(h.model()), h.model().shown_frame.unwrap());

        // Two more film changes while the first one's solve runs.
        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let superia = until_held(&mut h);
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.choose_film(PORTRA));
        assert!(h.model().held_frame().is_none(), "an edit drops the held frame");
        h.edit(|m| m.choose_film("harman_phoenix_200"));
        let last = h.model().revision;
        h.idle();
        let m = h.model();
        assert!(m.preview_current && m.held_frame().is_none() && !m.brightness_match_pending());
        assert!(m.revision >= last);
        let shown = m.shown_frame.unwrap();
        assert_eq!(shown.revision, m.revision);
        assert_eq!(h.frames, [shown], "only the latest edit's matched frame shows");
        assert!(h.renders().iter().all(|r| r.revision == superia.revision || r.revision >= last), "the Portra edit coalesced: {:?}", h.renders());
        assert!(h.errors.is_empty(), "{:?}", h.errors);

        // Undo while the solve runs: back to a state whose match is solved.
        h.edit(|m| m.choose_film(GOLD));
        h.idle();
        assert!((matched_ev(h.model()) - gold_ev).abs() < 1e-9);
        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let superia = until_held(&mut h);
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.perform_undo());
        assert!(h.model().held_frame().is_none());
        h.idle();
        let m = h.model();
        assert_eq!(m.state.film, GOLD);
        assert!((matched_ev(m) - gold_ev).abs() < 1e-9, "the Superia solve's answer isn't applied to Gold");
        assert!(m.preview_current && m.held_frame().is_none() && !m.brightness_match_pending());
        let shown = m.shown_frame.unwrap();
        assert_eq!(shown.revision, m.revision);
        assert_eq!(h.frames, [shown], "the undone state's frame, and nothing of Superia");
        assert_eq!(h.renders().iter().map(|r| r.revision).collect::<Vec<_>>(), [superia.revision, shown.revision]);
        // The undone state's render can reach the helper before the solve.
        let mut commands = h.command_names();
        commands.sort();
        assert_eq!(commands, ["place_exposure", "render", "render"]);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = gold_frame;

        // Opening another photograph while the solve runs.
        let other_photo = second_photo(&h, "held-other");
        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let old_generation = until_held(&mut h).generation;
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.open(&other_photo));
        let generation = h.model().source_generation;
        assert_ne!(generation, old_generation);
        assert!(h.model().held_frame().is_none() && h.model().shown_frame.is_none());
        // Every frame held from here on is the new photograph's.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            if let Some(held) = h.model().held_frame() {
                assert_eq!(held.generation, generation, "a held frame of the previous photograph");
            }
            if !h.step() {
                if h.quiescent() {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "not idle");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        h.idle();
        let _ = std::fs::remove_file(&other_photo);
        let m = h.model();
        assert!(m.preview_current && m.held_frame().is_none() && !m.brightness_match_pending());
        assert!(h.frames.iter().all(|f| f.generation == generation), "nothing of the previous photograph: {:?}", h.frames);
        assert_eq!(h.frames.len(), 1, "{:?}", h.frames);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// Closing drops a held frame.
    fn closing_drops_a_held_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let previous = h.model().shown_frame;
        h.edit(|m| m.choose_film(SUPERIA));
        until_held(&mut h);
        h.edit(|m| m.shutdown());
        assert!(h.model().held_frame().is_none() && h.model().shown_frame == previous);
    }
}

/// The Fast exporter's file for `state` of the open photograph, made apart
/// from the preview, as `check_native_macos.py` checks a saved preview.
fn fast_render(h: &Harness, state: &crate::state::EditState, destination: &std::path::Path, save: super::SaveSettings) -> Vec<u8> {
    let m = &h.app.model;
    let paths = m.paths.clone().unwrap();
    let mut params = state.params.clone();
    crate::json::set(&mut params, "settings.serial_grain", serde_json::Value::Bool(m.prefs.low_memory_export));
    let photo = m.photo_size();
    let request = crate::engine::ExportRequest {
        executable: paths.fast_exporter.clone().expect("the Fast exporter is built"),
        engine: crate::engine::ExportEngine::Fast,
        data: paths.data.clone(),
        scratch: m.engine.as_ref().unwrap().directory().to_path_buf(),
        source: m.source.clone().unwrap(),
        destination: destination.to_path_buf(),
        film: state.film.clone(),
        paper: state.paper.clone(),
        params,
        scan_film: crate::json::boolean(state.value("io.scan_film")),
        crop: state.crop.cropped().then_some(state.crop),
        border: state.border.exported(photo.width as u32, photo.height as u32),
        size_arguments: vec![],
        save_arguments: save.arguments(),
    };
    crate::engine::run_export(&request, &crate::engine::ExportHandle::default()).unwrap();
    std::fs::read(destination).unwrap()
}

lifecycle_test! {
    /// Save Preview and Fast Export asked for while a frame is held wait for
    /// the matched frame, also past the 30 s check while the lifecycle is
    /// still getting there, and write it: the same bytes as the Fast
    /// exporter's file of the matched state, not the held state's.
    fn save_and_export_wait_for_the_matched_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        let dir = std::env::temp_dir().join(format!("negs-held-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let save = super::SaveSettings { bit_depth: 16, color_space: 0, jpeg_quality: 95 };

        // Save Preview.
        h.clear_logs();
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        until_held(&mut h);
        let held_state = h.model().state.clone();
        let saved = dir.join("saved.tif");
        h.edit(|m| m.save_preview(saved.clone(), super::ExportSize::FULL, save));
        assert!(h.model().waiting_output.is_some() && h.model().status == "Matching camera brightness…");
        // The 30 s check fires while the solve and the matched render are
        // still to come: it keeps waiting.
        let patience = super::OUTPUT_PATIENCE;
        h.model().waiting_output.as_mut().unwrap().0 -= patience;
        assert!(h.fire_timers(patience) >= 1);
        assert!(h.model().waiting_output.is_some() && !saved.exists() && h.errors.is_empty(), "{:?}", h.errors);
        h.idle();
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Saved saved.tif", "{}", m.status);
        assert_eq!(m.revision, edited + 1, "the match moved");
        let matched_state = m.state.clone();
        assert_eq!(h.command_names(), ["render", "place_exposure", "render", "save"]);
        assert_eq!(h.frames.iter().map(|f| f.revision).collect::<Vec<_>>(), [edited + 1]);
        let saved_bytes = std::fs::read(&saved).unwrap();
        assert_eq!(saved_bytes, fast_render(&h, &matched_state, &dir.join("matched-fast.tif"), save), "the saved preview is the matched state's");
        assert_ne!(saved_bytes, fast_render(&h, &held_state, &dir.join("held-fast.tif"), save), "and not the held frame's");

        // Fast Export: the matched preview at export size is saved.
        h.clear_logs();
        h.edit(|m| m.choose_film(GOLD));
        let edited = h.model().revision;
        until_held(&mut h);
        let exported = dir.join("exported.tif");
        h.edit(|m| m.export(exported.clone(), crate::engine::ExportEngine::Fast, super::ExportSize::FULL, save));
        assert!(h.model().waiting_output.is_some());
        h.idle();
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Exported exported.tif", "{}", m.status);
        assert_eq!(m.revision, edited + 1, "the match moved back");
        let matched_state = m.state.clone();
        assert_eq!(h.command_names(), ["render", "place_exposure", "render", "save"], "exported from the matched preview");
        assert_eq!(
            std::fs::read(&exported).unwrap(),
            fast_render(&h, &matched_state, &dir.join("matched-fast-2.tif"), save),
            "the export is the matched state's"
        );
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

lifecycle_test! {
    /// A Save waiting for a match that nothing will finish (the render of
    /// the edit failed) gives up at once and says so, instead of saving
    /// nothing silently after 30 s.
    fn a_waiting_save_gives_up_when_the_render_failed() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        h.clear_logs();
        // A value the helper can't read fails the render; it's a match input,
        // so the match is pending.
        h.edit(|m| m.set("enlarger.y_filter_shift", serde_json::json!("not a number")));
        h.idle();
        assert!(h.model().render_failed && h.model().brightness_match_pending() && h.model().held_frame().is_none());
        assert_eq!(h.errors.len(), 1, "the render's own error");
        let saved = std::env::temp_dir().join(format!("negs-held-failed-{}.tif", std::process::id()));
        h.edit(|m| m.save_preview(saved.clone(), super::ExportSize::FULL, super::SaveSettings { bit_depth: 8, color_space: 0, jpeg_quality: 95 }));
        assert!(h.model().waiting_output.is_none() && !saved.exists());
        assert_eq!(h.errors.last().map(String::as_str), Some("Couldn’t save: the preview with the camera brightness match isn’t ready. Try again once it shows."));
    }
}

lifecycle_test! {
    /// A drag with the match on: drag frames show live, as before (a held
    /// frame would freeze the slider), and the solve waits for the release.
    /// The release's settled frame is then held for the match like any other.
    fn a_drag_with_the_match_on_shows_its_frames() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        let photo = h.photo.clone();
        open_matched(&mut h, &photo);
        crate::ui::canvas::set_fixed_scale(Some(0.25));
        h.clear_logs();
        h.edit(|m| m.begin_interaction());
        for shift in [0.1, 0.2, 0.3] {
            // The filter shift is a match input: the match is pending.
            h.edit(|m| m.set("enlarger.y_filter_shift", serde_json::json!(shift)));
            h.idle();
            let m = h.model();
            assert!(m.brightness_match_pending() && !m.matching_brightness && m.held_frame().is_none());
            assert_eq!(m.shown_frame.map(|f| (f.revision, f.kind)), Some((m.revision, FrameKind::Drag)), "the drag frame shows");
        }
        let dragged = h.model().revision;
        assert_eq!(h.frames.len(), 3);
        assert!(h.frames.iter().all(|f| f.kind == FrameKind::Drag));
        assert!(h.command_names().iter().all(|c| c == "render"), "no solve during the drag");
        h.clear_logs();
        h.edit(|m| m.end_interaction("Change Adjustment"));
        let held = until_held(&mut h);
        assert_eq!((held.revision, held.kind), (dragged, FrameKind::Settled));
        h.idle();
        let m = h.model();
        assert!(m.preview_current && !m.showing_interactive_frame && m.held_frame().is_none() && !m.brightness_match_pending());
        assert_eq!(m.undo_title(), Some("Change Adjustment"));
        let (generation, revision) = (m.source_generation, m.revision);
        assert_eq!(h.renders()[0], settled(dragged, generation));
        if revision == dragged {
            assert_eq!(h.frames, [held], "an unchanged match shows the release's frame");
        } else {
            assert_eq!(h.frames.iter().map(|f| (f.revision, f.kind)).collect::<Vec<_>>(), [(dragged + 1, FrameKind::Settled)], "a moved match shows only the matched frame");
        }
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

