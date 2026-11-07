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
    /// shows it: one settled render, one frame, the preview current. With no
    /// canvas there's no quick frame (B2): `a_film_click_shows_a_quick_frame_
    /// then_the_same_settled_frame` has one at a quarter of the pixels.
    fn a_film_change_renders_once_and_settles() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        crate::ui::canvas::set_fixed_scale(Some(0.0));
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
/// (−0.18 against −0.46 EV) and on the 33 MP ARW (−0.05 against −0.33): a
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
        assert_eq!(h.errors.last().map(String::as_str), Some("Couldn’t save: the preview didn’t develop, so the camera brightness couldn’t be matched."));
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


/// Matched far from Gold 200 on the test photograph (−0.07 against −0.46 EV).
const PHOENIX: &str = "harman_phoenix_200";

/// How the blocked solve is let go: a slider pressed and released without
/// a change, or Crop entered and cancelled.
#[derive(Clone, Copy)]
enum Blocker {
    Slider,
    Crop,
}

/// Review S1 (scenario L1): a film change whose frame is held behind a solve
/// for an earlier edit; the solve comes back stale while `blocker` keeps the
/// next one from starting. The frame must stay held (its exposure is the
/// old one) with a waiting Save, until the blocker lets go: then the solve
/// runs, only the matched frame shows and the Save writes it.
fn a_stale_solve_waits_for(blocker: Blocker) {
    let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
    let photo = h.photo.clone();
    open_matched(&mut h, &photo);
    let (gold_ev, previous) = (matched_ev(h.model()), h.model().shown_frame.unwrap());
    h.clear_logs();
    // Superia's frame is held and its solve (S1) starts in the same task.
    h.edit(|m| m.choose_film(SUPERIA));
    let superia = until_held(&mut h);
    assert!(h.model().matching_brightness, "S1 runs");
    // Phoenix during S1: its render reaches the helper before S1's
    // place_exposure (the reference is still being read), so its frame is
    // held behind S1. Checked, not assumed: the test stops here otherwise.
    h.edit(|m| m.choose_film(PHOENIX));
    let phoenix = until_held(&mut h);
    assert!(phoenix.revision > superia.revision);
    assert!(h.model().matching_brightness, "Phoenix's frame was held while S1 still ran");
    let dir = std::env::temp_dir().join(format!("negs-held-blocked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let saved = dir.join("saved.tif");
    let save = super::SaveSettings { bit_depth: 16, color_space: 0, jpeg_quality: 95 };
    h.edit(|m| m.save_preview(saved.clone(), super::ExportSize::FULL, save));
    match blocker {
        Blocker::Slider => h.edit(|m| m.begin_interaction()),
        Blocker::Crop => h.edit(|m| m.begin_crop()),
    }
    // S1 comes back stale; nothing can start S2 yet.
    h.idle();
    let m = h.model();
    assert_eq!(m.held_frame(), Some(phoenix), "the frame stays held while its match is pending");
    assert!(m.brightness_match_pending() && !m.matching_brightness);
    assert_eq!(m.shown_frame, Some(previous), "the old exposure isn't shown");
    assert!(h.frames.is_empty(), "{:?}", h.frames);
    assert!(h.model().waiting_output.is_some() && !saved.exists());
    let solves = h.command_names().iter().filter(|c| *c == "place_exposure").count();
    assert_eq!(solves, 1, "only S1");
    match blocker {
        Blocker::Slider => h.edit(|m| m.end_interaction("Change Adjustment")),
        Blocker::Crop => h.edit(|m| m.end_crop(false)),
    }
    assert!(h.model().matching_brightness, "letting go starts S2");
    h.idle();
    let m = h.model();
    let ev = matched_ev(m);
    assert!((ev - gold_ev).abs() >= 0.05, "Phoenix moves the match ({gold_ev} to {ev})");
    assert_eq!(m.revision, phoenix.revision + 1);
    assert!(m.preview_current && m.held_frame().is_none() && !m.brightness_match_pending());
    assert_eq!(h.frames.iter().map(|f| f.revision).collect::<Vec<_>>(), [phoenix.revision + 1], "only the matched frame shows");
    assert_eq!(h.command_names().iter().filter(|c| *c == "place_exposure").count(), 2);
    assert!(h.model().waiting_output.is_none() && h.model().status == "Saved saved.tif", "{}", h.model().status);
    let matched_state = h.model().state.clone();
    assert_eq!(std::fs::read(&saved).unwrap(), fast_render(&h, &matched_state, &dir.join("fast.tif"), save), "the Save wrote the matched frame");
    assert!(h.errors.is_empty(), "{:?}", h.errors);
    let _ = std::fs::remove_dir_all(&dir);
}

lifecycle_test! {
    /// Review S1 with a slider pressed and released without a change
    /// (`end_interaction`'s release, and `release_held`'s pending check).
    fn a_stale_solve_back_while_a_slider_is_pressed_waits_for_the_release() {
        a_stale_solve_waits_for(Blocker::Slider);
    }
}

lifecycle_test! {
    /// Review S1 with Crop entered and cancelled (`end_crop`'s release, and
    /// `release_held`'s pending check).
    fn a_stale_solve_back_while_cropping_waits_for_the_crop_to_end() {
        a_stale_solve_waits_for(Blocker::Crop);
    }
}

// MARK: Quick frames (B2, ticket 28)

/// The canvas in these tests: a quarter of the photograph's pixels per side.
const SCALE: f64 = 0.25;

fn quick(revision: u64, generation: u64) -> RenderRequest {
    RenderRequest { kind: FrameKind::Quick, revision, generation, interactive_scale: Some(SCALE) }
}

fn drag(revision: u64, generation: u64) -> RenderRequest {
    RenderRequest { kind: FrameKind::Drag, revision, generation, interactive_scale: Some(SCALE) }
}

/// Every frame presented since the logs were cleared, as (revision, kind).
fn presented(h: &Harness) -> Vec<(u64, FrameKind)> {
    h.app.model.frame_log.iter().map(|f| (f.revision, f.kind)).collect()
}

/// Opens the test photograph on a canvas at `SCALE`, matched or not, and
/// leaves it settled and idle with the logs cleared. The open itself has no
/// quick frame: there's no frame of the photograph to replace yet.
fn open_quick(h: &mut Harness, matched: bool) {
    crate::ui::canvas::set_fixed_scale(Some(SCALE));
    let photo = h.photo.clone();
    if matched {
        open_matched(h, &photo);
    } else {
        h.open(&photo);
    }
    let m = h.model();
    assert!(m.preview_current && m.shown_frame.is_some_and(|f| f.kind == FrameKind::Settled));
    assert!(h.errors.is_empty(), "{:?}", h.errors);
    h.clear_logs();
}

/// Waits until the current edit's quick frame is shown, and returns it.
fn until_quick(h: &mut Harness) -> FrameTag {
    h.run_until("the quick frame", |m| m.shown_frame.is_some_and(|f| f.kind == FrameKind::Quick && f.revision == m.revision));
    h.model().shown_frame.unwrap()
}

fn pixels(m: &super::Model) -> Vec<u8> {
    m.image.as_ref().unwrap().with_pixels(|p| p.to_vec()).expect("the frame keeps its pixels")
}

/// The helper's time in the status ("… · developed in 0.25 s …"), in ms.
fn developed_ms(status: &str) -> f64 {
    let after = status.split("developed in ").nth(1).unwrap_or_else(|| panic!("{status}"));
    after.split(" s").next().unwrap().parse::<f64>().unwrap() * 1000.0
}

/// The films of `candidates` in the catalog, other than the current one.
fn films_other_than(m: &super::Model, candidates: &[&'static str]) -> Vec<&'static str> {
    let films = m.films();
    candidates.iter().copied().filter(|id| *id != m.state.film && films.iter().any(|f| f.id == *id)).collect()
}

lifecycle_test! {
    /// The Mac's `testFilmClickShowsAQuickFrameThenTheSameSettledFrame`: a
    /// film click shows a canvas-size frame marked Preview, then at once the
    /// export-size frame, byte-identical to one rendered without the quick
    /// frame. The quick frame isn't the edit's preview: Save waits, the
    /// render id and the export-size flag stay, the preview cache doesn't
    /// keep it, and the photograph's size and preview scale (the canvas's
    /// fit and its 1:1 stop) stay as they were.
    fn a_film_click_shows_a_quick_frame_then_the_same_settled_frame() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let cache = crate::session::cache_dir().join("rendered-preview.cache");
        h.model().preview_cache.flush();
        assert!(cache.exists(), "the open's settled frame is cached");
        let m = h.model();
        let (generation, render_id, image_size, preview_scale) = (m.source_generation, m.render_id, m.image_size, m.preview_scale);
        let film = other(&m.state.film);
        h.edit(|m| m.choose_film(film));
        let edited = h.model().revision;
        let shown = until_quick(&mut h);
        assert_eq!((shown.revision, shown.generation), (edited, generation));
        let m = h.model();
        let picture = m.image.as_ref().unwrap();
        eprintln!("quick frame {} × {}: {}", picture.width, picture.height, m.status);
        assert!(picture.width < 1000, "a canvas-size frame");
        assert!(!m.preview_current && !m.can_save() && m.showing_interactive_frame);
        assert!(m.status.starts_with("Preview · ") && m.status.contains("developed in"), "{}", m.status);
        assert_eq!((m.render_id, m.render_at_export_size), (render_id, true));
        assert_eq!((m.image_size, m.preview_scale), (image_size, preview_scale));
        assert!(m.pending_render, "the settled render is due at once, with no delay");
        m.preview_cache.flush();
        assert!(!cache.exists(), "the quick frame isn't cached");
        h.idle();
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (edited, FrameKind::Settled)]);
        assert_eq!(h.command_names(), ["render", "render"]);
        let m = h.model();
        assert!(m.preview_current && !m.showing_interactive_frame && !m.status.starts_with("Preview"), "{}", m.status);
        assert_eq!(m.image.as_ref().unwrap().width, 3072);
        assert!(m.render_id > render_id && m.render_at_export_size);
        m.preview_cache.flush();
        assert!(cache.exists(), "the settled frame is cached");
        let after_quick = pixels(m);

        // The same film again without quick frames (no canvas).
        crate::ui::canvas::set_fixed_scale(Some(0.0));
        h.edit(|m| m.choose_film(other(film)));
        h.idle();
        h.clear_logs();
        h.edit(|m| m.choose_film(film));
        let direct = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(direct, generation)]);
        assert!(pixels(h.model()) == after_quick, "the settled frame is byte-identical");
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The Mac's `testRapidFilmSwitchingDoesNotPileUpRenders`. Films clicked
    /// as soon as the previous quick frame shows: a quick frame each, and one
    /// settled render, of the last. Films clicked while a quick render runs:
    /// that quick frame still shows although its edit is superseded (as with
    /// Ctrl+Z held), the edits in between coalesce, the last one settles.
    fn fast_film_switching_ends_on_the_last_film_settled() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let generation = h.model().source_generation;
        let films = films_other_than(h.model(), &[PORTRA, "kodak_ektar_100", SUPERIA, "kodak_ultramax_400", GOLD]);
        let films = &films[..4];
        let mut revisions = vec![];
        for film in films {
            h.edit(|m| m.choose_film(film));
            revisions.push(h.model().revision);
            until_quick(&mut h);
        }
        h.idle();
        let last = *revisions.last().unwrap();
        let mut expected: Vec<RenderRequest> = revisions.iter().map(|&r| quick(r, generation)).collect();
        expected.push(settled(last, generation));
        assert_eq!(h.renders(), expected, "a quick frame per film, one settled render");
        let mut frames: Vec<(u64, FrameKind)> = revisions.iter().map(|&r| (r, FrameKind::Quick)).collect();
        frames.push((last, FrameKind::Settled));
        assert_eq!(presented(&h), frames);
        let m = h.model();
        assert!(m.state.film == films[3] && m.preview_current && !m.status.starts_with("Preview"), "{}", m.status);

        h.clear_logs();
        h.edit(|m| m.choose_film(films[0]));
        let first = h.model().revision;
        h.run_until("the quick render", |m| m.rendering);
        h.edit(|m| m.choose_film(films[1]));
        h.edit(|m| m.choose_film(films[2]));
        let last = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [quick(first, generation), quick(last, generation), settled(last, generation)]);
        assert_eq!(
            presented(&h),
            [(first, FrameKind::Quick), (last, FrameKind::Quick), (last, FrameKind::Settled)],
            "the superseded edit's quick frame shows"
        );
        assert!(h.model().state.film == films[2] && h.model().preview_current);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The Mac's `testUndoBeforeTheSettledFrameSettlesTheRestoredFilm`: Undo
    /// while a film's quick frame shows settles the restored film, and the
    /// undone film never renders at export size.
    fn undo_right_after_a_quick_frame_settles_the_restored_film() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let (film, generation) = (h.model().state.film.clone(), h.model().source_generation);
        h.edit(|m| m.choose_film(other(&film)));
        let edited = h.model().revision;
        until_quick(&mut h);
        h.edit(|m| m.perform_undo());
        assert_eq!(h.model().state.film, film);
        let undone = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [quick(edited, generation), quick(undone, generation), settled(undone, generation)]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (undone, FrameKind::Quick), (undone, FrameKind::Settled)]);
        let m = h.model();
        assert!(m.preview_current && m.state.film == film && m.revision == undone);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The Mac's `testPostOnlyEditsSkipTheQuickFrame`: post adjustments alone
    /// reuse the helper's kept output, in milliseconds, with no quick frame;
    /// also right after a film change settled (its settled render is the
    /// kept output again). The helper answers a reuse with its kept reply,
    /// so its `pipeline_ms` is the kept render's to the last digit; the
    /// trace shows it (times alone vary with the machine's load).
    fn post_only_edits_skip_the_quick_frame() {
        let trace = std::env::temp_dir().join(format!("negs-quick-post-trace-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&trace);
        // SAFETY: the child process runs this test alone, before any thread
        // reads the environment.
        unsafe { std::env::set_var("NEGS_RENDER_TRACE", &trace) };
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        assert!(!h.model().value("post.contrast").is_null());
        let generation = h.model().source_generation;
        h.edit(|m| m.set("post.contrast", serde_json::json!(20)));
        let post = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(post, generation)]);
        let reused_ms = developed_ms(&h.model().status);

        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let changed = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [quick(changed, generation), settled(changed, generation)]);
        let film_ms = developed_ms(&h.model().status);

        h.clear_logs();
        h.edit(|m| m.set("post.contrast", serde_json::json!(-20)));
        let after_film = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(after_film, generation)], "no quick frame right after a film change");
        let after_film_ms = developed_ms(&h.model().status);
        eprintln!("post-only: {reused_ms:.0} ms; film change settled: {film_ms:.0} ms; post-only after it: {after_film_ms:.0} ms");
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
        drop(h);
        let events = trace_events(&trace);
        let _ = std::fs::remove_file(&trace);
        let settled_replies: Vec<&serde_json::Value> =
            events.iter().filter(|e| e["event"] == "render_reply" && e["kind"] == "settled").map(|e| &e["pipeline_ms"]).collect();
        let [opened, post_only, film_changed, after] = settled_replies[settled_replies.len() - 4..] else { unreachable!() };
        assert!(post_only.is_number() && post_only == opened, "the first post-only edit reused the open's output: {settled_replies:?}");
        assert!(film_changed != opened, "the film change rendered: {settled_replies:?}");
        assert_eq!(after, film_changed, "the post-only edit after the film change reused its output");
    }
}

lifecycle_test! {
    /// The Mac's `testAFailingEditReportsOnceAndStops`: the helper refuses
    /// the edit; its quick frame fails silently, the settled render reports
    /// the error once, and nothing renders again.
    fn a_failing_edit_reports_once_and_stops() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        assert!(!h.model().value("io.upscale_factor").is_null());
        let (previous, generation) = (h.model().shown_frame, h.model().source_generation);
        h.edit(|m| m.set("io.upscale_factor", serde_json::json!(5)));
        let edited = h.model().revision;
        h.idle();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while std::time::Instant::now() < deadline {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.idle();
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)], "no retry");
        assert_eq!(h.errors.len(), 1, "one error: {:?}", h.errors);
        eprintln!("error: {}", h.errors[0]);
        let m = h.model();
        assert!(m.render_failed && !m.preview_current);
        assert!(m.shown_frame == previous, "the quick frame failed, silently");
        assert!(presented(&h).is_empty());
    }
}

lifecycle_test! {
    /// The Mac's `testADragRightAfterAQuickFrameSettlesOnRelease`: a slider
    /// grabbed while a quick frame shows renders drag frames, and letting go
    /// renders at export size, with no second quick frame.
    fn a_drag_right_after_a_quick_frame_settles_on_release() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let generation = h.model().source_generation;
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let edited = h.model().revision;
        until_quick(&mut h);
        h.edit(|m| m.begin_interaction());
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.5)));
        let dragged = h.model().revision;
        h.idle();
        assert!(!h.model().preview_current);
        h.edit(|m| m.end_interaction("Change Adjustment"));
        h.idle();
        assert_eq!(h.renders(), [quick(edited, generation), drag(dragged, generation), settled(dragged, generation)]);
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The Mac's `testADragSettlesWithoutAQuickFrame`: letting go of a
    /// slider renders at export size at once; the drag frames were
    /// canvas-size already.
    fn a_drag_settles_without_a_quick_frame() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let generation = h.model().source_generation;
        h.edit(|m| m.begin_interaction());
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.5)));
        let first = h.model().revision;
        h.idle();
        assert!(h.model().status.starts_with("Preview"), "{}", h.model().status);
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.7)));
        let second = h.model().revision;
        h.edit(|m| m.end_interaction("Change Adjustment"));
        h.idle();
        // The second value coalesces into the release's render.
        assert_eq!(h.renders(), [drag(first, generation), settled(second, generation)]);
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The Mac's `testTheMatchRunsBeforeTheSettledRender`, with the held
    /// frame: with the match on, a film's quick frame is held while the
    /// solve runs from it (the helper holds its decode). A match that moves
    /// is an edit with a quick frame of its own, then the one settled render,
    /// of the matched exposure; the old exposure never renders at export
    /// size and never shows. One that doesn't move shows the held quick
    /// frame, then settles. The trace records the order.
    fn the_match_runs_on_the_quick_frame_before_the_settled_render() {
        let trace = std::env::temp_dir().join(format!("negs-quick-trace-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&trace);
        // SAFETY: the child process runs this test alone, before any thread
        // reads the environment.
        unsafe { std::env::set_var("NEGS_RENDER_TRACE", &trace) };
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        open_quick(&mut h, true);
        let (gold_ev, previous, generation) = (matched_ev(h.model()), h.model().shown_frame, h.model().source_generation);

        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!((held.revision, held.kind), (edited, FrameKind::Quick));
        assert!(h.model().matching_brightness, "the solve starts from the held quick frame");
        assert_eq!(h.model().shown_frame, previous);
        h.idle();
        let m = h.model();
        let ev = matched_ev(m);
        assert!((ev - gold_ev).abs() >= 0.05, "the film change moves the match ({gold_ev} to {ev})");
        assert!(m.preview_current && m.held_frame().is_none() && !m.brightness_match_pending() && m.revision == edited + 1);
        let matched = edited + 1;
        assert_eq!(h.renders(), [quick(edited, generation), quick(matched, generation), settled(matched, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render", "render"]);
        assert_eq!(presented(&h), [(matched, FrameKind::Quick), (matched, FrameKind::Settled)], "previous, matched quick, matched settled");
        assert!(h.errors.is_empty(), "{:?}", h.errors);

        // Back to Gold, then Portra 400, which Gold matches within 0.05 EV.
        h.edit(|m| m.choose_film(GOLD));
        h.idle();
        h.clear_logs();
        h.edit(|m| m.choose_film(PORTRA));
        let unchanged = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!((held.revision, held.kind), (unchanged, FrameKind::Quick));
        h.idle();
        let m = h.model();
        assert!(m.preview_current && m.held_frame().is_none() && m.revision == unchanged);
        assert_eq!(h.renders(), [quick(unchanged, generation), settled(unchanged, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render"]);
        assert_eq!(presented(&h), [(unchanged, FrameKind::Quick), (unchanged, FrameKind::Settled)], "the held quick frame, then settled");
        assert!(h.errors.is_empty(), "{:?}", h.errors);

        drop(h);
        let events = trace_events(&trace);
        let _ = std::fs::remove_file(&trace);
        let from = events.iter().position(|e| e["event"] == "edit" && e["revision"] == edited).expect("the edit is traced");
        let to = events.iter().position(|e| e["event"] == "edit" && e["revision"] == matched + 1).expect("the next edit is traced");
        let lifecycle: Vec<String> = events[from..to]
            .iter()
            .filter(|e| ["edit", "render_request", "render_finished", "match_start", "match_solved", "held_shown", "held_dropped"].contains(&e["event"].as_str().unwrap()))
            .map(|e| {
                let mut line = format!("{} r{}", e["event"].as_str().unwrap(), e["revision"]);
                if let Some(kind) = e["kind"].as_str() {
                    line += &format!(" {kind}");
                }
                if let Some(why) = e["why"].as_str() {
                    line += &format!(" {why}");
                }
                if e["event"] == "render_finished" {
                    line += if e["held"] == true { " held" } else if e["shown"] == true { " shown" } else { " dropped" };
                }
                line
            })
            .collect();
        eprintln!("trace:\n  {}", lifecycle.join("\n  "));
        let (old, new) = (edited, matched);
        assert_eq!(lifecycle, [
            format!("edit r{old}"),
            format!("render_request r{old} quick"),
            format!("render_finished r{old} quick held"),
            format!("match_start r{old}"),
            "match_solved rnull".to_owned(),
            format!("held_dropped r{old} edit"),
            format!("edit r{new}"),
            format!("render_request r{new} quick"),
            format!("render_finished r{new} quick shown"),
            format!("render_request r{new} settled"),
            format!("render_finished r{new} settled shown"),
        ]);
    }
}

lifecycle_test! {
    /// The Mac's `testTurningTheMatchOffReleasesAHeldFrame`: matching turned
    /// off while a quick frame is held for the solve shows that frame once
    /// the solve is back, then settles it.
    fn turning_the_match_off_releases_a_held_quick_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        open_quick(&mut h, true);
        let generation = h.model().source_generation;
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        let held = until_held(&mut h);
        assert_eq!(held.kind, FrameKind::Quick);
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.set_matches_brightness(false));
        assert_eq!(h.model().revision, edited);
        h.idle();
        let m = h.model();
        assert!(m.preview_current && m.held_frame().is_none() && !m.matches_brightness());
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (edited, FrameKind::Settled)]);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// Matching turned on while a quick frame shows, at exposure 0 (so no
    /// edit): the solve starts from the quick frame, and the settled render
    /// already scheduled waits for it. The match moves the exposure, an edit
    /// with its own quick frame; the old exposure never renders at export
    /// size. (The Mac sends that render.)
    fn turning_the_match_on_during_a_quick_frame_solves_before_settling() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        assert_eq!(crate::json::number(h.model().value(state::PREP_EXPOSURE)), 0.0);
        let generation = h.model().source_generation;
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        until_quick(&mut h);
        assert!(h.model().pending_render, "the settled render is scheduled");
        h.edit(|m| m.set_matches_brightness(true));
        assert_eq!(h.model().revision, edited, "no edit");
        assert!(h.model().matching_brightness, "the solve starts from the quick frame");
        h.idle();
        let m = h.model();
        assert!(m.preview_current && !m.brightness_match_pending() && m.revision == edited + 1, "the match moved the exposure");
        let matched = edited + 1;
        assert_eq!(h.renders(), [quick(edited, generation), quick(matched, generation), settled(matched, generation)]);
        assert_eq!(h.command_names(), ["render", "place_exposure", "render", "render"]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (matched, FrameKind::Quick), (matched, FrameKind::Settled)]);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}
/// Fast Export into a fresh folder for the S1 tests: (folder, file).
fn export_target(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("negs-quick-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("exported.tif");
    (dir, file)
}

fn export_now(h: &mut Harness, file: &std::path::Path) {
    let save = super::SaveSettings { bit_depth: 8, color_space: 0, jpeg_quality: 95 };
    let file = file.to_path_buf();
    h.edit(|m| m.export(file, crate::engine::ExportEngine::Fast, super::ExportSize::FULL, save));
}

lifecycle_test! {
    /// B2 review S1: Undo while the solve of a held quick frame runs. The
    /// undone state's match is solved, so its quick frame is shown, not held,
    /// while the stale solve still runs; only `brightness_solved`'s settle
    /// settles it. A Fast Export asked for meanwhile finishes.
    fn undo_during_the_solve_of_a_held_quick_frame_settles_the_undone_state() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        open_quick(&mut h, true);
        let generation = h.model().source_generation;
        h.edit(|m| m.choose_film(SUPERIA));
        assert_eq!(until_held(&mut h).kind, FrameKind::Quick);
        h.run_until("the solve", |m| m.matching_brightness);
        h.edit(|m| m.perform_undo());
        let undone = h.model().revision;
        h.run_until("the undone quick frame", |m| m.shown_frame.is_some_and(|f| f.revision == m.revision));
        let m = h.model();
        assert_eq!(m.shown_frame.map(|f| f.kind), Some(FrameKind::Quick));
        assert!(m.matching_brightness && m.held_frame().is_none(), "shown while the stale solve still runs");
        let (dir, file) = export_target("undo-solve");
        export_now(&mut h, &file);
        assert!(h.model().waiting_output.is_some(), "the Export waits");
        h.idle();
        let m = h.model();
        assert!(m.state.film == GOLD && m.revision == undone);
        assert!(m.preview_current && !m.matching_brightness, "the undone state's quick frame settles");
        assert_eq!(h.renders().last(), Some(&settled(undone, generation)));
        assert!(h.renders().iter().all(|r| r.kind != FrameKind::Settled || r.revision == undone), "{:?}", h.renders());
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Exported exported.tif" && file.exists(), "{}", m.status);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

lifecycle_test! {
    /// B2 review S1: matching turned on during a quick frame of a photograph
    /// that can't be matched (a transparent PNG here, the 60 MP DNG's
    /// refusal in the app). The failure leaves nothing held, so only
    /// `brightness_solved`'s settle settles the quick frame. A Fast Export
    /// asked for meanwhile finishes.
    fn turning_the_match_on_during_a_quick_frame_of_an_unmatchable_photo_settles() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        crate::ui::canvas::set_fixed_scale(Some(SCALE));
        let transparent = std::env::temp_dir().join(format!("negs-quick-transparent-{}.png", std::process::id()));
        let (w, h_) = (3072u32, 2048u32);
        let pixels: Vec<u8> = (0..w * h_).flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h_) as u8, 128, 200]
        }).collect();
        image::save_buffer(&transparent, &pixels, w, h_, image::ExtendedColorType::Rgba8).unwrap();
        h.open(&transparent);
        h.clear_logs();
        let generation = h.model().source_generation;
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let edited = h.model().revision;
        until_quick(&mut h);
        assert!(h.model().pending_render, "the settled render is scheduled");
        h.edit(|m| m.set_matches_brightness(true));
        assert_eq!(h.model().revision, edited, "no edit");
        assert!(h.model().matching_brightness, "the solve starts from the quick frame");
        let (dir, file) = export_target("unmatchable");
        export_now(&mut h, &file);
        assert!(h.model().waiting_output.is_some(), "the Export waits");
        h.idle();
        let _ = std::fs::remove_file(&transparent);
        let m = h.model();
        assert_eq!(m.brightness_match_note().as_deref(), Some("Transparent images can’t be matched."));
        assert!(m.preview_current && m.held_frame().is_none(), "the quick frame settles after the failed match");
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Exported exported.tif" && file.exists(), "{}", m.status);
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
/// With the film grid's thumbnails on screen, nothing but the match asks
/// the helper between a quick frame's reply and its settled render: no
/// thumbnail, tone curve or preset check. Every helper command is logged,
/// and the helper answers one at a time, so a command between the two
/// renders in the log went out after the quick reply.
fn nothing_else_runs_between_the_quick_and_settled_frames(matched: bool) {
    let Some(mut h) = Harness::start(harness::prefs(matched)) else { return };
    open_quick(&mut h, matched);
    h.edit(|m| m.set_previews(true));
    h.edit(|m| m.toggle_foldout(false));
    h.idle();
    assert!(h.command_names().iter().any(|c| c == "thumbnail"), "the grid's thumbnails were made");
    h.clear_logs();
    // Portra 400: Gold matches it within 0.05 EV, so the match stays.
    h.edit(|m| m.choose_film(PORTRA));
    let edited = h.model().revision;
    h.idle();
    let generation = h.model().source_generation;
    assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
    let names = h.command_names();
    eprintln!("commands: {names:?}");
    let renders: Vec<usize> = names.iter().enumerate().filter(|(_, c)| *c == "render").map(|(i, _)| i).collect();
    let between = &names[renders[0] + 1..renders[1]];
    let expected: &[&str] = if matched { &["place_exposure"] } else { &[] };
    assert_eq!(between, expected, "between the quick and the settled render");
    assert!(names[..renders[0]].iter().all(|c| c == "tone_curve"), "before the quick render: {names:?}");
    assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
}

lifecycle_test! {
    fn nothing_else_runs_between_the_quick_and_settled_frames_with_the_match_off() {
        nothing_else_runs_between_the_quick_and_settled_frames(false);
    }
}

lifecycle_test! {
    fn nothing_else_runs_between_the_quick_and_settled_frames_with_the_match_on() {
        nothing_else_runs_between_the_quick_and_settled_frames(true);
    }
}

lifecycle_test! {
    /// No quick frame, as before ticket 28: a photograph's first frame, the
    /// launch render under the restored frame from the preview cache, a canvas
    /// past half the photograph's pixels, a photograph under 4.5 MP; and
    /// holding Original renders nothing.
    fn first_frames_small_photos_and_close_zooms_settle_directly() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        crate::ui::canvas::set_fixed_scale(Some(SCALE));
        let photo = h.photo.clone();
        h.edit(|m| m.open(&photo));
        let opened = h.model().revision;
        h.idle();
        let generation = h.model().source_generation;
        assert_eq!(h.renders(), [settled(opened, generation)], "the first frame");

        // `finish_start`'s render of a restored session (another film than
        // the helper rendered last), with its cached frame on screen.
        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| {
            m.state.film = film.to_owned();
            m.showing_cached_preview = true;
            m.changed_with_cache(false);
        });
        let launch = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(launch, generation)], "the launch render");

        crate::ui::canvas::set_fixed_scale(Some(0.75));
        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let zoomed = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(zoomed, generation)], "0.56 of the pixels");
        crate::ui::canvas::set_fixed_scale(Some(SCALE));

        h.clear_logs();
        h.edit(|m| m.hold_original(true));
        h.idle();
        assert!(h.model().original.is_some());
        h.edit(|m| m.hold_original(false));
        h.idle();
        assert!(h.renders().is_empty() && h.command_names().iter().all(|c| c == "original"), "{:?}", h.command_names());

        // 2400 × 1600: 3.8 MP.
        let small = std::env::temp_dir().join(format!("negs-harness-small-{}.png", std::process::id()));
        let (w, h_) = (2400u32, 1600u32);
        let rgb: Vec<u8> = (0..w * h_).flat_map(|i| [((i % w) * 255 / w) as u8, ((i / w) * 255 / h_) as u8, 96]).collect();
        image::save_buffer(&small, &rgb, w, h_, image::ExtendedColorType::Rgb8).unwrap();
        h.open(&small);
        assert_eq!(h.model().image_size, crate::crop::Size::new(2400.0, 1600.0));
        let generation = h.model().source_generation;
        h.clear_logs();
        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        let edited = h.model().revision;
        h.idle();
        let _ = std::fs::remove_file(&small);
        assert_eq!(h.renders(), [settled(edited, generation)], "a small photograph");
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// Border, crop (Original size) and rotation edits render nothing: the
    /// canvas draws them over the developed frame. With Longest side the
    /// crop's extent is part of the render (`EditState::rendered`), and the
    /// whole crop (position and angle too) is part of the request, as on the
    /// Mac: moving or straightening a crop of the same extent renders
    /// nothing; a crop of another extent develops another frame, now with a
    /// quick frame first. Reuse *is* lost after a move or straightening: the
    /// next post-only edit's request has another crop than the kept settled
    /// render's, so it gets a quick frame and a full render, although the
    /// helper, keyed on the resolved working size, would reuse its output.
    /// That is the Mac's behaviour too; keying on the extent only is Noah's
    /// call (B2 review S2).
    fn border_crop_and_rotation_edits_and_the_longest_side_crop() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let generation = h.model().source_generation;
        h.edit(|m| m.set_border(|b| b.enabled = true, "Border"));
        h.idle();
        assert!(h.renders().is_empty(), "border: {:?}", h.renders());

        let crop = |x: f64, side: f64, angle: f64| (crate::crop::Rect { x, y: 0.05, width: side, height: side }, angle);
        let edit_crop = |h: &mut Harness, (rect, angle): (crate::crop::Rect, f64)| {
            h.clear_logs();
            h.edit(|m| {
                let mut next = m.state.clone();
                next.crop.rect = rect;
                next.crop.angle = angle;
                m.apply(next, "Crop");
            });
            let edited = h.model().revision;
            h.idle();
            (edited, h.renders().to_vec(), developed_ms(&h.model().status))
        };
        let (_, renders, _) = edit_crop(&mut h, crop(0.0, 0.9, 0.0));
        assert!(renders.is_empty(), "crop, Original size: {renders:?}");

        h.clear_logs();
        h.edit(|m| m.rotate());
        h.idle();
        assert!(h.renders().is_empty(), "rotation: {:?}", h.renders());

        h.edit(|m| m.set(state::SIZE_TARGET, serde_json::json!(3072)));
        h.idle();
        assert_eq!(h.model().size_mode(), state::SizeMode::LongestSide);
        let (_, renders, _) = edit_crop(&mut h, crop(0.1, 0.9, 0.0));
        assert!(renders.is_empty(), "a crop moved, Longest side: {renders:?}");
        let (edited, renders, quick_ms) = edit_crop(&mut h, crop(0.1, 0.8, 0.0));
        assert_eq!(renders, [quick(edited, generation), settled(edited, generation)], "a smaller crop, Longest side");
        let (_, renders, _) = edit_crop(&mut h, crop(0.1, 0.8, 2.0));
        assert!(renders.is_empty(), "a crop straightened, Longest side: {renders:?}");
        // The post-only edit after it: no reuse, a quick frame and a full render.
        let post = |h: &mut Harness, contrast: i64| {
            h.clear_logs();
            h.edit(|m| m.set("post.contrast", serde_json::json!(contrast)));
            let edited = h.model().revision;
            h.idle();
            (edited, h.renders().to_vec(), developed_ms(&h.model().status))
        };
        let (edited, renders, straightened_ms) = post(&mut h, 10);
        assert_eq!(renders, [quick(edited, generation), settled(edited, generation)], "post-only after a straightened crop");
        let (edited, renders, reused_ms) = post(&mut h, 20);
        assert_eq!(renders, [settled(edited, generation)], "post-only again: reused");
        let (_, renders, _) = edit_crop(&mut h, crop(0.15, 0.8, 2.0));
        assert!(renders.is_empty(), "a crop moved again: {renders:?}");
        let (edited, renders, panned_ms) = post(&mut h, 30);
        assert_eq!(renders, [quick(edited, generation), settled(edited, generation)], "post-only after a crop pan");
        crate::ui::canvas::set_fixed_scale(Some(0.0));
        let (_, renders, _) = edit_crop(&mut h, crop(0.1, 0.8, 2.0));
        assert!(renders.is_empty());
        let (edited, renders, direct_post_ms) = post(&mut h, 40);
        assert_eq!(renders, [settled(edited, generation)], "post-only after a crop pan, no canvas: the helper reuses");
        eprintln!(
            "Longest side, post-only: {straightened_ms:.0} ms after straightening and {panned_ms:.0} ms after a pan (quick + full), \
             {reused_ms:.0} ms reused, {direct_post_ms:.0} ms after a pan with no canvas (the helper reuses)"
        );
        let (edited, renders, direct_ms) = edit_crop(&mut h, crop(0.1, 0.9, 0.0));
        assert_eq!(renders, [settled(edited, generation)], "a larger crop, Longest side, no canvas");
        eprintln!("Longest side, a crop of another extent: settled render {quick_ms:.0} ms after a quick frame, {direct_ms:.0} ms without a canvas");
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

lifecycle_test! {
    /// The helper restarted after a settled render (a crash, here a kill):
    /// the new helper keeps no output, so the next post-only edit isn't
    /// taken for a reuse. It gets a quick frame like any other edit; the one
    /// after reuses the new helper's output.
    fn a_restarted_helper_is_not_taken_to_keep_its_output() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let generation = h.model().source_generation;
        h.edit(|m| m.set("post.contrast", serde_json::json!(10)));
        let first = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(first, generation)], "reused");

        let engine = h.model().engine.clone().unwrap();
        let epoch = engine.helper_epoch();
        engine.kill_helper();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while engine.helper_epoch() == epoch {
            assert!(std::time::Instant::now() < deadline, "the helper didn't exit");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.clear_logs();
        h.edit(|m| m.set("post.contrast", serde_json::json!(20)));
        let restarted = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [quick(restarted, generation), settled(restarted, generation)], "no reuse of a dead helper's output");
        assert_eq!(engine.helper_epoch(), epoch + 1);
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);

        h.clear_logs();
        h.edit(|m| m.set("post.contrast", serde_json::json!(30)));
        let again = h.model().revision;
        h.idle();
        assert_eq!(h.renders(), [settled(again, generation)], "the new helper's output is reused");
    }
}

lifecycle_test! {
    /// Save Preview and Fast Export asked for while a quick frame shows wait
    /// for its settled frame, then write it: the Fast exporter's bytes of
    /// the same state. With matching off, so only the quick frame waits.
    fn save_and_export_after_a_quick_frame_wait_for_the_settled_frame() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        let dir = std::env::temp_dir().join(format!("negs-quick-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let save = super::SaveSettings { bit_depth: 16, color_space: 0, jpeg_quality: 95 };

        let film = other(&h.model().state.film);
        h.edit(|m| m.choose_film(film));
        until_quick(&mut h);
        let saved = dir.join("saved.tif");
        h.edit(|m| m.save_preview(saved.clone(), super::ExportSize::FULL, save));
        assert!(h.model().waiting_output.is_some() && !saved.exists());
        assert_eq!(h.model().status, "Developing the preview…");
        h.idle();
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Saved saved.tif", "{}", m.status);
        let state = m.state.clone();
        assert_eq!(h.command_names(), ["render", "render", "save"]);
        assert!(std::fs::read(&saved).unwrap() == fast_render(&h, &state, &dir.join("fast.tif"), save), "the settled frame was saved");

        h.clear_logs();
        h.edit(|m| m.choose_film(other(film)));
        until_quick(&mut h);
        let exported = dir.join("exported.tif");
        h.edit(|m| m.export(exported.clone(), crate::engine::ExportEngine::Fast, super::ExportSize::FULL, save));
        assert!(h.model().waiting_output.is_some());
        h.idle();
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Exported exported.tif", "{}", m.status);
        let state = m.state.clone();
        assert_eq!(h.command_names(), ["render", "render", "save"], "the settled preview was saved");
        assert!(std::fs::read(&exported).unwrap() == fast_render(&h, &state, &dir.join("fast-2.tif"), save));
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

lifecycle_test! {
    /// The longer wait with the match on: quick frame held, solve, matched
    /// quick frame, settled frame, then the Save, also past the 30 s check
    /// (moved clock) at the held and at the matched quick frame. The saved
    /// bytes are the matched state's.
    fn a_save_during_a_held_quick_frame_waits_for_the_matched_settled_frame() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        open_quick(&mut h, true);
        let dir = std::env::temp_dir().join(format!("negs-quick-held-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let save = super::SaveSettings { bit_depth: 16, color_space: 0, jpeg_quality: 95 };
        h.edit(|m| m.choose_film(SUPERIA));
        let edited = h.model().revision;
        assert_eq!(until_held(&mut h).kind, FrameKind::Quick);
        let saved = dir.join("saved.tif");
        h.edit(|m| m.save_preview(saved.clone(), super::ExportSize::FULL, save));
        assert_eq!(h.model().status, "Matching camera brightness…");
        let patience = super::OUTPUT_PATIENCE;
        h.model().waiting_output.as_mut().unwrap().0 -= patience;
        assert!(h.fire_timers(patience) >= 1);
        assert!(h.model().waiting_output.is_some() && h.errors.is_empty(), "{:?}", h.errors);
        // At the matched quick frame the Save still waits for its settled frame.
        h.run_until("the matched quick frame", |m| m.shown_frame.is_some_and(|f| f.kind == FrameKind::Quick && f.revision == edited + 1));
        assert!(h.model().waiting_output.is_some());
        h.model().waiting_output.as_mut().unwrap().0 -= patience;
        h.fire_timers(patience);
        assert!(h.model().waiting_output.is_some() && h.errors.is_empty(), "{:?}", h.errors);
        h.idle();
        let m = h.model();
        assert!(m.waiting_output.is_none() && m.status == "Saved saved.tif", "{}", m.status);
        assert_eq!(m.revision, edited + 1);
        let state = m.state.clone();
        assert_eq!(h.command_names(), ["render", "place_exposure", "render", "render", "save"]);
        assert!(std::fs::read(&saved).unwrap() == fast_render(&h, &state, &dir.join("fast.tif"), save), "the matched settled frame was saved");
        assert!(h.errors.is_empty(), "{:?}", h.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// MARK: Film search with the held and quick frames (chunk C)

/// Searches the Film grid for `query` as typing in the field does
/// (`inspector::film_search_changed`; the headless app has no grid
/// windows, so only the model's part runs), then refreshes.
fn search_films(h: &mut Harness, query: &str) {
    crate::ui::inspector::film_search_changed(&mut h.app, query.to_owned());
    h.edit(|_| {});
}

/// The films the Film grid lists for `query`, in its order.
fn film_matches(m: &super::Model, query: &str) -> Vec<String> {
    let (films, papers) = (m.films(), m.papers());
    state::matching(query, state::film_groups(&films, &papers)).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect()
}

/// The helper commands since the logs were cleared, with the film of each
/// thumbnail.
fn thumbnails_and_renders(h: &Harness) -> Vec<String> {
    h.commands()
        .iter()
        .map(|c| match c["command"].as_str().unwrap() {
            "thumbnail" => format!("thumbnail {}", c["film"].as_str().unwrap()),
            other => other.to_owned(),
        })
        .collect()
}

/// Asserts that every thumbnail went out after the last render (if any),
/// for a film in `matches`, each once, and returns how many there were.
fn thumbnails_after_the_settled_render(h: &Harness, matches: &[String]) -> usize {
    let commands = thumbnails_and_renders(h);
    eprintln!("commands: {commands:?}");
    let last_render = commands.iter().rposition(|c| c == "render").unwrap_or(0);
    let thumbnails: Vec<&str> = commands.iter().filter_map(|c| c.strip_prefix("thumbnail ")).collect();
    assert!(commands[..last_render].iter().all(|c| !c.starts_with("thumbnail ")), "a thumbnail before the settled render: {commands:?}");
    for film in &thumbnails {
        assert!(matches.iter().any(|m| m == film), "{film} isn't a match of the search: {commands:?}");
        assert_eq!(thumbnails.iter().filter(|f| *f == film).count(), 1, "{film} asked for twice");
    }
    thumbnails.len()
}

/// Large photo, match on, Film grid open and searching: Return in the field
/// (`inspector::choose_first_match`) chooses the first match. The canvas
/// goes previous → matched quick → settled. The search is widened while
/// the held quick frame's solve runs, and the new matches' thumbnails are
/// made only after the settled frame, at the matched exposure: the solve
/// leaves the helper free, so this is where `previews_may_render`'s
/// `preview_current` matters.
lifecycle_test! {
    fn a_searched_film_chosen_with_the_match_on_shows_the_matched_quick_frame_then_settles() {
        let Some(mut h) = Harness::start(harness::prefs(true)) else { return };
        open_quick(&mut h, true);
        let query = "superia 200";
        search_films(&mut h, query);
        h.edit(|m| m.set_previews(true));
        h.edit(|m| m.toggle_foldout(false));
        h.idle();
        let matches = film_matches(h.model(), query);
        assert_eq!(matches.first().map(String::as_str), Some(SUPERIA), "{matches:?}");
        assert_eq!(thumbnails_after_the_settled_render(&h, &matches), matches.len(), "only the matches' thumbnails");
        let (gold_ev, generation) = (matched_ev(h.model()), h.model().source_generation);
        h.clear_logs();

        crate::ui::inspector::choose_first_match(&mut h.app);
        h.edit(|_| {});
        let edited = h.model().revision;
        assert_eq!(h.model().state.film, SUPERIA);
        assert_eq!(h.model().undo_title(), Some("Change Film"));
        assert_eq!(until_held(&mut h).kind, FrameKind::Quick);
        let wider = "superia";
        search_films(&mut h, wider);
        let superias = film_matches(h.model(), wider);
        assert!(superias.len() > matches.len(), "{superias:?}");
        assert!(h.model().matching_brightness && h.model().held_frame().is_some(), "the search was typed during the solve");
        h.idle();
        let m = h.model();
        assert!((matched_ev(m) - gold_ev).abs() >= 0.05, "the film change moves the match");
        assert!(m.preview_current && m.held_frame().is_none() && m.revision == edited + 1);
        assert_eq!(m.previews.film_query, wider);
        let matched = edited + 1;
        assert_eq!(h.renders(), [quick(edited, generation), quick(matched, generation), settled(matched, generation)]);
        assert_eq!(presented(&h), [(matched, FrameKind::Quick), (matched, FrameKind::Settled)], "previous, matched quick, settled");
        // The matched exposure makes every match's thumbnail stale.
        assert_eq!(thumbnails_after_the_settled_render(&h, &superias), superias.len());
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

/// A search typed while a quick frame shows asks for no thumbnail until
/// the settled frame, and then only for its matches.
lifecycle_test! {
    fn searching_while_a_quick_frame_shows_waits_for_the_settled_frame() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        // The search first: it lasts while the foldout is closed, and the
        // grid then asks only for its matches from the start.
        search_films(&mut h, "portra");
        h.edit(|m| m.set_previews(true));
        h.edit(|m| m.toggle_foldout(false));
        h.idle();
        let portras = film_matches(h.model(), "portra");
        assert!(portras.len() >= 2, "{portras:?}");
        assert_eq!(thumbnails_after_the_settled_render(&h, &portras), portras.len(), "only the matches' thumbnails");
        h.clear_logs();

        let generation = h.model().source_generation;
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.5)));
        let edited = h.model().revision;
        until_quick(&mut h);
        search_films(&mut h, "gold");
        assert!(!h.model().preview_current);
        h.idle();
        let golds = film_matches(h.model(), "gold");
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (edited, FrameKind::Settled)]);
        assert_eq!(thumbnails_after_the_settled_render(&h, &golds), golds.len(), "the new exposure's thumbnails of the matches only");
        assert!(h.model().preview_current && h.errors.is_empty(), "{:?}", h.errors);
    }
}

/// Starring a match while searching, during the settled render after a
/// quick frame: the render isn't disturbed, nothing renders again, and the
/// matches' thumbnails, the starred one included, are made once each after
/// the settled frame.
lifecycle_test! {
    fn starring_a_match_during_the_settled_render_leaves_it_alone() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        // The search first: it lasts while the foldout is closed, and the
        // grid then asks only for its matches from the start.
        search_films(&mut h, "portra");
        h.edit(|m| m.set_previews(true));
        h.edit(|m| m.toggle_foldout(false));
        h.idle();
        let portras = film_matches(h.model(), "portra");
        let starred = portras.iter().find(|id| !h.app.model.prefs.is_favorite(id, false)).expect("an unstarred Portra").clone();
        h.clear_logs();

        let generation = h.model().source_generation;
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.5)));
        let edited = h.model().revision;
        h.run_until("the settled render", |m| m.rendering && m.render_log.last().is_some_and(|r| r.kind == FrameKind::Settled && r.revision == m.revision));
        h.edit(|m| m.toggle_favorite(&starred, false));
        assert!(h.model().prefs.is_favorite(&starred, false));
        h.idle();
        let m = h.model();
        assert!(m.preview_current && m.revision == edited, "starring isn't an edit");
        assert_eq!(h.renders(), [quick(edited, generation), settled(edited, generation)]);
        assert_eq!(presented(&h), [(edited, FrameKind::Quick), (edited, FrameKind::Settled)]);
        assert_eq!(thumbnails_after_the_settled_render(&h, &portras), portras.len());
        assert!(h.errors.is_empty(), "{:?}", h.errors);
    }
}

/// Clearing a search once idle asks for the other films' thumbnails at the
/// current state: an edit made while searching re-made only the matches,
/// so without the field's `dirty::PREVIEWS` mark the rest would keep the old
/// exposure until something unrelated refreshed the grid.
lifecycle_test! {
    fn clearing_a_search_remakes_the_other_films_thumbnails_at_the_new_state() {
        let Some(mut h) = Harness::start(harness::prefs(false)) else { return };
        open_quick(&mut h, false);
        search_films(&mut h, "portra");
        h.edit(|m| m.set_previews(true));
        h.edit(|m| m.toggle_foldout(false));
        h.idle();
        let portras = film_matches(h.model(), "portra");
        h.clear_logs();
        h.edit(|m| m.set("camera.exposure_compensation_ev", serde_json::json!(0.5)));
        h.idle();
        assert_eq!(thumbnails_after_the_settled_render(&h, &portras), portras.len(), "the edit re-made the matches only");
        h.clear_logs();

        search_films(&mut h, "");
        h.idle();
        let others: Vec<String> = film_matches(h.model(), "").into_iter().filter(|id| !portras.contains(id)).collect();
        assert_eq!(thumbnails_after_the_settled_render(&h, &others), others.len(), "every other film's thumbnail, once");
        for command in h.commands().iter().filter(|c| c["command"] == "thumbnail") {
            assert_eq!(command["params"]["camera"]["exposure_compensation_ev"], serde_json::json!(0.5), "{}", command["film"]);
        }
        assert!(h.renders().is_empty() && h.errors.is_empty(), "{:?}", h.errors);
    }
}
