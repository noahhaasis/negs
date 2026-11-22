//! windows/src's crop, exact_fit and histogram as they are now (on the
//! core, Phase 3b), compiled here so their own unit tests run on the core
//! on any OS; the Windows app's `cargo test` runs them on Windows too.
//! border.rs's tests read data/ beside windows/ and run there only (the
//! border differential test covers its rules); thumb_compose.rs's run in
//! the thumb_compose test.
//!
//! With them, the app's `CropSession` (its geometry on the core's
//! `SessionGeometry`, its fields its own) driven in lockstep with the
//! oracle's through seeded steps (NEGS_RULES_SEEDS / 4 sessions, default
//! 2,500, of 12 steps), every field compared after each.
//!
//! `state` stands in for what the tests read of windows/src/state.rs:
//! `rendered()` here is only the parameters, so the crop test that checks
//! a crop never changes the rendered state is a formality here; the app's
//! own run checks it against the real one.
#![allow(dead_code)]

mod picture_common;

use negs_core_diff_picture::crop as oracle;
use picture_common::{Random, Tally, cases, gate_shared, seed};

mod state {
    use serde_json::Value;

    #[derive(Clone, Debug, Default)]
    pub struct EditState {
        pub params: Value,
        pub crop: crate::crop::CropSettings,
    }

    impl EditState {
        pub fn rendered(&self) -> Value {
            self.params.clone()
        }
    }
}

#[path = "../../../windows/src/crop.rs"]
mod crop;
#[path = "../../../windows/src/exact_fit.rs"]
mod exact_fit;
#[path = "../../../windows/src/histogram.rs"]
mod histogram;

fn o_rect(r: crop::Rect) -> oracle::Rect {
    oracle::Rect::new(r.x, r.y, r.width, r.height)
}
fn o_size(s: crop::Size) -> oracle::Size {
    oracle::Size::new(s.width, s.height)
}
fn o_aspect(a: crop::CropAspect) -> oracle::CropAspect {
    oracle::CropAspect::ALL[crop::CropAspect::ALL.iter().position(|x| *x == a).unwrap()]
}

fn compare(t: &mut Tally, what: &dyn Fn() -> String, o: &oracle::CropSession, a: &crop::CropSession) {
    t.dbg(what, o.rect, o_rect(a.rect));
    t.eq(what, (o.aspect, o.portrait, o.rotation, o.flip, o.straightening, o.adjusting, o.typing), (o_aspect(a.aspect), a.portrait, a.rotation, a.flip, a.straightening, a.adjusting, a.typing));
    t.dbg(what, (o.custom, o.photo), (o_size(a.custom), o_size(a.photo)));
    t.f(what, o.angle, a.angle);
    let (os, s) = (o.settings(), a.settings());
    t.dbg(what, (os.rect, os.custom, os.angle), (o_rect(s.rect), o_size(s.custom), s.angle));
    t.eq(what, (os.aspect, o.modified(), o.orientation_portrait()), (o_aspect(s.aspect), a.modified(), a.orientation_portrait()));
    t.dbg(what, (o.ratio(), o.shown(), o.bounds()), (a.ratio(), o_rect(a.shown()), o_rect(a.bounds())));
}

fn session(r: &mut Random, t: &mut Tally, i: usize) {
    let rect = crop::Rect::new(r.range(-0.05, 0.6), r.range(-0.05, 0.6), r.range(0.0, 0.6), r.range(0.0, 0.6));
    let aspect = crop::CropAspect::ALL[r.below(9) as usize];
    let custom = crop::Size::new(r.range(0.1, 20.0), r.range(0.1, 20.0));
    let angle = if r.chance(2) { 0.0 } else { r.range(-45.0, 45.0) };
    let start = crop::CropSettings { rect, aspect, custom, angle };
    let photo = crop::Size::new(r.range(50.0, 7000.0), r.range(50.0, 7000.0));
    let (rot, flip) = (r.int(-5, 5) as i32, r.chance(2));
    let ostate = negs_core_diff_picture::state::EditState {
        crop: oracle::CropSettings { rect: o_rect(rect), aspect: o_aspect(aspect), custom: o_size(custom), angle },
        ..Default::default()
    };
    let mut o = oracle::CropSession::new(ostate, o_size(photo), rot, flip);
    let mut a = crop::CropSession::new(state::EditState { crop: start, ..Default::default() }, photo, rot, flip);
    compare(t, &|| format!("session {i}: new"), &o, &a);
    for step in 0..12 {
        let op = r.below(11);
        match op {
            0 => {
                let x = crop::CropAspect::ALL[r.below(9) as usize];
                o.choose(o_aspect(x));
                a.choose(x);
            }
            1 => {
                let p = r.chance(2);
                o.orient(p);
                a.orient(p);
            }
            2 => {
                let (wd, ht) = (r.chance(3).then(|| r.range(-5.0, 2000.0)), r.chance(3).then(|| r.range(-5.0, 2000.0)));
                o.set_custom(wd, ht);
                a.set_custom(wd, ht);
            }
            3 => {
                o.reset();
                a.reset();
            }
            4 => {
                o.begin_straightening();
                a.begin_straightening();
            }
            5 | 6 => {
                let d = r.range(-50.0, 50.0);
                o.straighten(d);
                a.straighten(d);
            }
            7 => {
                o.end_straightening();
                a.end_straightening();
            }
            8 => {
                let (p, rot, flip) = (if r.chance(3) { a.photo.swapped() } else { a.photo }, r.int(-5, 5) as i32, r.chance(2));
                o.update(o_size(p), rot, flip);
                a.update(p, rot, flip);
            }
            9 => {
                // A drag, as the canvas makes one, with the app's flags.
                let shown = a.shown();
                let moved = crop::constrain(shown, crop::move_rect(shown, r.range(-200.0, 200.0), r.range(-200.0, 200.0), a.bounds()), a.angle, a.photo);
                o.adjusting = true;
                a.adjusting = true;
                o.set_shown(o_rect(moved));
                a.set_shown(moved);
            }
            _ => {
                let typing = r.chance(2);
                o.typing = typing;
                a.typing = typing;
            }
        }
        compare(t, &|| format!("session {i} step {step} op {op}"), &o, &a);
    }
}

#[test]
fn the_apps_crop_session_matches_the_rust() {
    let seed = seed(0x3b_5e5);
    let n = cases(10_000) / 4;
    gate_shared("crop session (windows/src)", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            session(&mut r, t, i);
        }
        n
    });
}
