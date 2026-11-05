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
